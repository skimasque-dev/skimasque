//! Opening a gateway session from command-line flags — TLS trust, the bearer
//! credential, and keeping a short-lived credential fresh. Shared by
//! `skimasque-client` and `skimasque exec`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use clap::Args as ClapArgs;
use http::{HeaderMap, HeaderValue};
use skimasque::client::{Client, Credential, Session};
use skimasque::tls;
use skimasque_core::template::CONNECT_UDP_VARIABLES;
use skimasque_core::UriTemplate;

use crate::account;

/// How to trust the gateway and where to find it on the wire.
#[derive(Debug, Clone, ClapArgs)]
pub struct TlsArgs {
    /// The authority to present: the TLS server name and the `:authority` of
    /// each request. Defaults to the host in `--proxy`.
    #[arg(long, value_name = "HOST[:PORT]")]
    pub authority: Option<String>,

    /// The proxy's URI Template. Defaults to the well-known CONNECT-UDP one.
    #[arg(long, value_name = "TEMPLATE")]
    pub template: Option<String>,

    /// PEM file of certificates to trust instead of the system roots.
    #[arg(long, value_name = "PATH")]
    pub ca: Option<PathBuf>,

    /// Accept any certificate without verifying it.
    ///
    /// This gives up the only defence against a machine-in-the-middle. Use
    /// `--ca` with the proxy's certificate instead; it is no more work.
    #[arg(long, conflicts_with = "ca")]
    pub insecure: bool,
}

/// Where the bearer credential for tunnels comes from.
#[derive(Debug, Clone, ClapArgs)]
pub struct AuthArgs {
    /// Bearer token to send as `Proxy-Authorization`.
    #[arg(
        long,
        env = "SKIMASQUE_TOKEN",
        value_name = "TOKEN",
        hide_env_values = true
    )]
    pub auth_token: Option<String>,

    /// Exchange a GitHub Actions OIDC token for a platform credential, and
    /// present that on every tunnel.
    ///
    /// Fetches the OIDC token from the runner (the job needs
    /// `permissions: id-token: write`), POSTs it to the gateway's exchange
    /// endpoint, and uses the returned credential. Needs `--oidc-audience`.
    #[arg(long, conflicts_with = "auth_token")]
    pub github_oidc: bool,

    /// Use this OIDC token for the exchange instead of fetching one from the
    /// runner. Needs `--oidc-audience`.
    #[arg(
        long,
        env = "SKIMASQUE_OIDC_TOKEN",
        value_name = "JWT",
        hide_env_values = true,
        conflicts_with = "auth_token"
    )]
    pub oidc_token: Option<String>,

    /// The audience the OIDC token is minted for. Must match one of the
    /// gateway's `--oidc-audience` values.
    #[arg(long, value_name = "AUD")]
    pub oidc_audience: Option<String>,

    /// The organisation to mint a credential for, when authenticating with a
    /// `skimasque login` session instead of `--github-oidc`/`--auth-token`.
    /// Defaults to the session's only organisation; required if it belongs to
    /// more than one.
    #[arg(long, value_name = "ORG")]
    pub org: Option<String>,
}

/// The `skimasque login` session a credential was minted from, and the
/// organisation it was minted for.
pub struct Login {
    pub creds: account::Credentials,
    pub org: String,
}

/// An open session, plus -- when the credential is short-lived -- what a
/// background task needs to replace it before it expires.
pub struct Connected {
    pub session: Session,
    pub refresh: Option<(RefreshMode, Duration)>,
    /// The bearer token in use, if any.
    pub credential: Option<String>,
    /// A line worth showing the user about how the credential was obtained.
    pub note: Option<String>,
    /// Set when the credential came from a `skimasque login` session.
    pub login: Option<Login>,
}

/// Give `--proxy` an explicit port (default 443), bracketing a bare IPv6
/// literal so it round-trips through `lookup_host`.
fn proxy_with_port(spec: &str) -> String {
    if spec.starts_with('[') {
        // `[::1]` -> add a port; `[::1]:443` -> already has one.
        if spec.ends_with(']') {
            format!("{spec}:443")
        } else {
            spec.to_owned()
        }
    } else if spec.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{spec}]:443")
    } else if let Some((_, port)) = spec.rsplit_once(':') {
        if port.parse::<u16>().is_ok() {
            spec.to_owned()
        } else {
            format!("{spec}:443")
        }
    } else {
        format!("{spec}:443")
    }
}

/// Resolve `--proxy` (`host[:port]`, port defaulting to 443) to a socket
/// address for the QUIC endpoint, and return the `host:port` string to present
/// as the authority / TLS server name.
async fn resolve_proxy(spec: &str) -> anyhow::Result<(SocketAddr, String)> {
    let with_port = proxy_with_port(spec);
    let addr = tokio::net::lookup_host(&with_port)
        .await
        .with_context(|| format!("resolving the gateway address {with_port:?}"))?
        .next()
        .with_context(|| format!("{with_port:?} did not resolve to any address"))?;
    Ok((addr, with_port))
}

/// Connect to `gateway` (`host[:port]`, port defaulting to 443), resolve the
/// bearer credential, and return a session that sends it — plus `headers`
/// (application, policy pin) — on every tunnel.
pub async fn open_session(
    gateway: &str,
    tls: &TlsArgs,
    auth: &AuthArgs,
    mut headers: HeaderMap,
) -> anyhow::Result<Connected> {
    let (proxy, proxy_authority) = resolve_proxy(gateway).await?;

    let client_tls = match (&tls.ca, tls.insecure) {
        (Some(path), _) => {
            let pem = tokio::fs::read(path)
                .await
                .with_context(|| format!("reading {}", path.display()))?;
            tls::client_config_with_ca(&pem)?
        }
        (None, true) => {
            eprintln!("warning: --insecure disables certificate verification");
            tls::dangerous_client_config_without_verification()
        }
        (None, false) => tls::client_config_with_webpki_roots(),
    };

    let authority = tls.authority.clone().unwrap_or(proxy_authority);
    let template = match &tls.template {
        Some(raw) => UriTemplate::parse(raw).context("parsing --template")?,
        None => UriTemplate::default_connect_udp(&authority)?,
    };
    template
        .require_variables(&CONNECT_UDP_VARIABLES)
        .context("a connect-udp template needs target_host and target_port")?;

    let client = Client::new(client_tls)?;
    let session = client
        .connect(proxy, template)
        .await
        .with_context(|| format!("connecting to the gateway at {gateway} ({proxy})"))?;

    let bearer = resolve_bearer(auth, &session).await?;

    if let Some(token) = &bearer.header {
        let value = HeaderValue::from_str(&format!("Bearer {token}"))
            .context("the credential contains characters a header cannot carry")?;
        headers.insert(http::header::PROXY_AUTHORIZATION, value);
    }

    let session = if headers.is_empty() {
        session
    } else {
        session.with_default_headers(headers)
    };
    Ok(Connected {
        session,
        refresh: bearer.refresh,
        credential: bearer.header,
        note: bearer.note,
        login: bearer.login,
    })
}

/// The bearer credential for tunnels, and -- when it is short-lived -- how to
/// renew it.
struct Bearer {
    /// The token to send as `Proxy-Authorization: Bearer`, if any.
    header: Option<String>,
    /// The exchange to re-run before the credential expires, and how long the
    /// current one lasts. Unset for a static `--auth-token`, which does not
    /// expire from this client's point of view.
    refresh: Option<(RefreshMode, Duration)>,
    /// How the credential was obtained, for the caller to show.
    note: Option<String>,
    /// The login session the credential was minted from, if it was.
    login: Option<Login>,
}

/// Which credential source to use, decided from the flags (and whether a
/// `skimasque login` session is on disk) alone -- no I/O. Kept separate from
/// [`resolve_bearer`] so the priority order is unit-testable without a live
/// session or network access.
#[derive(Debug, PartialEq, Eq)]
enum AuthMode {
    /// `--github-oidc` or `--oidc-token`.
    Oidc,
    /// The static `--auth-token`.
    Static,
    /// Neither, but a `skimasque login` session exists on disk.
    Session,
    /// Neither, and no session either -- `resolve_bearer` fails fast on this.
    None,
}

fn auth_mode(args: &AuthArgs, has_session: bool) -> AuthMode {
    if args.github_oidc || args.oidc_token.is_some() {
        AuthMode::Oidc
    } else if args.auth_token.is_some() {
        AuthMode::Static
    } else if has_session {
        AuthMode::Session
    } else {
        AuthMode::None
    }
}

/// Work out the bearer credential for tunnels, in priority order: a platform
/// credential from a GitHub Actions OIDC exchange, the static `--auth-token`,
/// or -- falling back to whatever `skimasque login` left on disk -- a
/// credential minted from that session. Fails fast with a clear error rather
/// than silently sending no credential (and leaving a `407` from the gateway
/// as the only signal) when none of these are available.
async fn resolve_bearer(auth: &AuthArgs, session: &Session) -> anyhow::Result<Bearer> {
    let creds = account::load()?;
    match auth_mode(auth, creds.is_some()) {
        AuthMode::Oidc => {
            let audience = auth.oidc_audience.as_deref().context(
                "--github-oidc / --oidc-token also need --oidc-audience (the value the gateway expects)",
            )?;
            let exchange = OidcExchange {
                audience: audience.to_owned(),
                static_token: auth.oidc_token.clone(),
            };
            let credential = exchange.run(session).await?;
            let note = format!(
                "exchanged an OIDC token for a platform credential (valid {}s)",
                credential.expires_in.as_secs()
            );
            let ttl = credential.expires_in;
            Ok(Bearer {
                header: Some(credential.token),
                refresh: Some((RefreshMode::Oidc(exchange), ttl)),
                note: Some(note),
                login: None,
            })
        }
        AuthMode::Static => Ok(Bearer {
            header: auth.auth_token.clone(),
            refresh: None,
            note: None,
            login: None,
        }),
        AuthMode::Session => {
            let creds = creds.expect("AuthMode::Session implies a stored session");
            let api = account::Api::new(&creds.control_plane)?;
            let org = account::resolve_org(&api, &creds, auth.org.clone()).await?;
            let login = Login {
                creds: creds.clone(),
                org: org.clone(),
            };
            let exchange = SessionExchange { creds, org };
            let credential = exchange.run().await?;
            let note = format!(
                "minted a platform credential from your skimasque login session (valid {}s)",
                credential.expires_in.as_secs()
            );
            let ttl = credential.expires_in;
            Ok(Bearer {
                header: Some(credential.token),
                refresh: Some((RefreshMode::Session(exchange), ttl)),
                note: Some(note),
                login: Some(login),
            })
        }
        AuthMode::None => bail!(
            "not authenticated: run `skimasque login`, or pass \
             --github-oidc/--oidc-token/--auth-token"
        ),
    }
}

/// Everything needed to mint a fresh platform credential mid-session: the
/// audience the gateway expects, and a static OIDC token if one was supplied
/// with `--oidc-token` instead of being fetched from the runner.
pub struct OidcExchange {
    audience: String,
    static_token: Option<String>,
}

impl OidcExchange {
    /// Obtain a current OIDC token and exchange it for a platform credential.
    /// Goes through the *gateway's* exchange endpoint -- OIDC verification is
    /// audience-scoped to the specific gateway being connected to.
    async fn run(&self, session: &Session) -> anyhow::Result<Credential> {
        let oidc = match &self.static_token {
            Some(token) => token.clone(),
            None => skimasque_identity::github_actions_id_token(&self.audience)
                .await
                .context("fetching a GitHub Actions OIDC token")?,
        };
        session
            .exchange_credential(&oidc)
            .await
            .context("exchanging the OIDC token for a platform credential")
    }
}

/// Everything needed to mint a fresh platform credential mid-session from a
/// `skimasque login` session: the account session and which org to mint
/// against.
///
/// Unlike [`OidcExchange`], this talks directly to the *control plane*
/// (`account::Api`), not through the gateway's exchange
/// endpoint: the session is a management-API credential the client already
/// holds, and the resulting credential -- Ed25519-signed with the org's key
/// -- verifies against any gateway in the org, not just the one currently
/// connected to.
pub struct SessionExchange {
    creds: account::Credentials,
    org: String,
}

impl SessionExchange {
    /// Mint a fresh platform credential from the stored login session.
    async fn run(&self) -> anyhow::Result<Credential> {
        let api = account::Api::new(&self.creds.control_plane)?;
        let minted = api
            .mint_credential(&self.creds.session_token, &self.org, None)
            .await?;
        Ok(Credential {
            token: minted.token,
            expires_in: minted.expires_in,
        })
    }
}

/// How to obtain a fresh platform credential mid-session, and what it takes.
pub enum RefreshMode {
    Oidc(OidcExchange),
    Session(SessionExchange),
}

impl RefreshMode {
    async fn run(&self, session: &Session) -> anyhow::Result<Credential> {
        match self {
            RefreshMode::Oidc(exchange) => exchange.run(session).await,
            RefreshMode::Session(exchange) => exchange.run().await,
        }
    }
}

/// After a failed refresh, how long to wait before trying again.
const REFRESH_RETRY: Duration = Duration::from_secs(30);

/// How far ahead of a credential's expiry to obtain its replacement.
///
/// A quarter of the lifetime, so a slow exchange or a brief retry loop still
/// lands before tunnels start being refused, but a one-hour credential is not
/// re-minted every few minutes.
pub fn refresh_lead_time(ttl: Duration) -> Duration {
    (ttl / 4).clamp(Duration::from_secs(10), Duration::from_secs(15 * 60))
}

/// Keep `session`'s platform credential fresh for as long as the process runs.
///
/// A credential has a finite TTL (the gateway's `--credential-ttl` in OIDC
/// mode, or the control plane's cap in session mode), so a job or an
/// interactive session that outlives one would see its tunnels start failing
/// with `407`/`403`. This re-runs `exchange` ahead of each expiry and installs
/// the new credential on the live session; tunnels opened afterwards carry
/// it, and tunnels already open are undisturbed. `announce` controls whether
/// each successful refresh is reported on stderr; failures always are.
pub async fn refresh_credential(
    session: Arc<Session>,
    exchange: RefreshMode,
    mut ttl: Duration,
    announce: bool,
) {
    loop {
        tokio::time::sleep(ttl.saturating_sub(refresh_lead_time(ttl))).await;

        match exchange.run(&session).await {
            Ok(credential) => match session.set_credential(&credential) {
                Ok(()) => {
                    if announce {
                        eprintln!(
                            "refreshed the platform credential (valid {}s)",
                            credential.expires_in.as_secs()
                        );
                    }
                    ttl = credential.expires_in;
                }
                Err(error) => {
                    eprintln!("stopping credential refresh: {error}");
                    return;
                }
            },
            Err(error) => {
                eprintln!("credential refresh failed, retrying shortly: {error:#}");
                ttl = REFRESH_RETRY;
            }
        }
    }
}

/// One line for stderr when the gateway refuses a tunnel: a policy denial
/// reads `denied db.prod:5432: No matching allow rule. To allow it: allow
/// psql db.prod:5432`; anything else `could not reach <dest>: <error>`.
pub fn refusal_line(destination: &str, error: &skimasque::Error) -> String {
    if let skimasque::Error::Rejected {
        status,
        proxy_status,
    } = error
    {
        if *status == http::StatusCode::FORBIDDEN {
            return match proxy_status.as_deref().and_then(proxy_status_details) {
                Some(details) => match details.split_once(" suggested rule: ") {
                    Some((reason, rule)) => {
                        format!("denied {destination}: {reason} To allow it: {rule}")
                    }
                    None => format!("denied {destination}: {details}"),
                },
                None => format!("denied {destination}"),
            };
        }
    }
    format!("could not reach {destination}: {error}")
}

/// The `details` string parameter of a `Proxy-Status` value (RFC 9209),
/// with Structured Fields escapes (`\"`, `\\`) undone.
fn proxy_status_details(value: &str) -> Option<String> {
    let start = value.find("details=\"")? + "details=\"".len();
    let mut out = String::new();
    let mut chars = value[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::time::Duration;

    #[derive(Debug, Parser)]
    struct Harness {
        #[command(flatten)]
        tls: TlsArgs,
        #[command(flatten)]
        auth: AuthArgs,
    }

    fn auth(extra: &[&str]) -> AuthArgs {
        let mut argv = vec!["x"];
        argv.extend_from_slice(extra);
        Harness::try_parse_from(argv).unwrap().auth
    }

    #[test]
    fn the_harness_is_a_consistent_command() {
        use clap::CommandFactory;
        Harness::command().debug_assert();
    }

    #[test]
    fn auth_mode_picks_oidc_over_everything_else() {
        let a = auth(&["--github-oidc", "--oidc-audience", "https://gw.example"]);
        assert_eq!(auth_mode(&a, true), AuthMode::Oidc);
        assert_eq!(auth_mode(&a, false), AuthMode::Oidc);
    }

    #[test]
    fn auth_mode_picks_static_token_when_no_oidc_flag_is_given() {
        let a = auth(&["--auth-token", "secret"]);
        assert_eq!(auth_mode(&a, true), AuthMode::Static);
        assert_eq!(auth_mode(&a, false), AuthMode::Static);
    }

    #[test]
    fn auth_mode_falls_back_to_a_stored_session_when_nothing_else_is_given() {
        assert_eq!(auth_mode(&auth(&[]), true), AuthMode::Session);
    }

    #[test]
    fn auth_mode_is_none_with_no_flags_and_no_session() {
        assert_eq!(auth_mode(&auth(&[]), false), AuthMode::None);
    }

    #[test]
    fn the_refresh_lead_time_stays_within_bounds() {
        assert_eq!(
            refresh_lead_time(Duration::from_secs(3600)),
            Duration::from_secs(900)
        );
        assert_eq!(
            refresh_lead_time(Duration::from_secs(8)),
            Duration::from_secs(10)
        );
        assert_eq!(
            refresh_lead_time(Duration::from_secs(24 * 3600)),
            Duration::from_secs(15 * 60)
        );
    }

    #[test]
    fn the_proxy_gets_a_default_port_of_443() {
        assert_eq!(
            proxy_with_port("gateway.skimasque.com"),
            "gateway.skimasque.com:443"
        );
        assert_eq!(
            proxy_with_port("gateway.skimasque.com:8443"),
            "gateway.skimasque.com:8443"
        );
        assert_eq!(proxy_with_port("10.0.0.1"), "10.0.0.1:443");
        assert_eq!(proxy_with_port("10.0.0.1:4433"), "10.0.0.1:4433");
        assert_eq!(proxy_with_port("::1"), "[::1]:443");
        assert_eq!(proxy_with_port("[::1]"), "[::1]:443");
        assert_eq!(proxy_with_port("[::1]:4433"), "[::1]:4433");
        assert_eq!(proxy_with_port("2001:db8::1"), "[2001:db8::1]:443");
    }

    fn rejected(status: u16, proxy_status: Option<&str>) -> skimasque::Error {
        skimasque::Error::Rejected {
            status: http::StatusCode::from_u16(status).unwrap(),
            proxy_status: proxy_status.map(str::to_owned),
        }
    }

    #[test]
    fn a_policy_denial_reads_as_reason_and_fix() {
        let e = rejected(
            403,
            Some(
                r#"gw; error=destination_prohibited; details="No matching allow rule. suggested rule: allow psql db.prod:5432""#,
            ),
        );
        assert_eq!(
            refusal_line("db.prod:5432", &e),
            "denied db.prod:5432: No matching allow rule. To allow it: allow psql db.prod:5432"
        );
    }

    #[test]
    fn a_pin_mismatch_denial_has_no_fix_and_unescapes_quotes() {
        let e = rejected(
            403,
            Some(
                r#"gw; error=destination_prohibited; details="Policy \"staging\" does not apply to this identity; \"prod\" does.""#,
            ),
        );
        assert_eq!(
            refusal_line("db.prod:5432", &e),
            r#"denied db.prod:5432: Policy "staging" does not apply to this identity; "prod" does."#
        );
    }

    #[test]
    fn other_failures_say_the_destination_could_not_be_reached() {
        let e = rejected(502, Some(r#"gw; error=dns_error; details="nope""#));
        assert!(refusal_line("x:1", &e).starts_with("could not reach x:1: "));
        assert_eq!(refusal_line("x:1", &rejected(403, None)), "denied x:1");
    }
}
