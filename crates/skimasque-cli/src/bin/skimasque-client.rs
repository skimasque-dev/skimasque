//! `skimasque-client` -- open MASQUE tunnels through a proxy.
//!
//! Modes: `connect` bridges one TCP tunnel to stdin/stdout, `socks5` puts a
//! SOCKS5 relay in front of the tunnel so ordinary applications can use it, and
//! `probe` sends a payload and prints what comes back, for checking a proxy by
//! hand.
//!
//! `skimasque connect <destination> <args>` is the front door for the `connect`
//! mode -- it execs this binary as `skimasque-client <args> connect --target
//! <destination>` -- but this binary can also be run directly.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{bail, Context};
use clap::{ArgAction, Args as ClapArgs, Parser, Subcommand};
use http::{HeaderMap, HeaderValue};
use skimasque::client::{Client, Credential, Session};
use skimasque::tls;
use skimasque_cli::probe::{self, DnsHeader, TYPE_A, TYPE_AAAA};
use skimasque_cli::{init_tracing, socks5};
use skimasque_core::connect_udp::Target;
use skimasque_core::template::CONNECT_UDP_VARIABLES;
use skimasque_core::UriTemplate;
use tokio::net::TcpListener;
use tokio::time::timeout;

/// A MASQUE client for UDP over HTTP/3.
#[derive(Debug, Parser)]
#[command(
    name = "skimasque-client",
    version,
    about = "Open MASQUE tunnels through a gateway (the `connect` mode is also `skimasque connect`)",
    long_about = None
)]
struct Args {
    #[command(flatten)]
    connection: ConnectionArgs,

    /// Increase logging; repeat for more.
    #[arg(short, long, action = ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, ClapArgs)]
struct ConnectionArgs {
    /// The gateway to reach, as `host[:port]` (an IP is fine too). The host is
    /// resolved for the QUIC socket and used as the TLS server name; the port
    /// defaults to 443.
    #[arg(long, value_name = "HOST[:PORT]", default_value = "gateway.skimasque.com")]
    proxy: String,

    /// The authority to present: the TLS server name and the `:authority` of
    /// each request. Defaults to the host in `--proxy`.
    #[arg(long, value_name = "HOST[:PORT]")]
    authority: Option<String>,

    /// The proxy's URI Template. Defaults to the well-known CONNECT-UDP one.
    #[arg(long, value_name = "TEMPLATE")]
    template: Option<String>,

    /// PEM file of certificates to trust instead of the system roots.
    #[arg(long, value_name = "PATH")]
    ca: Option<PathBuf>,

    /// Accept any certificate without verifying it.
    ///
    /// This gives up the only defence against a machine-in-the-middle. Use
    /// `--ca` with the proxy's certificate instead; it is no more work.
    #[arg(long, conflicts_with = "ca")]
    insecure: bool,

    /// Bearer token to send as `Proxy-Authorization`.
    #[arg(
        long,
        env = "SKIMASQUE_TOKEN",
        value_name = "TOKEN",
        hide_env_values = true
    )]
    auth_token: Option<String>,

    /// Exchange a GitHub Actions OIDC token for a platform credential, and
    /// present that on every tunnel.
    ///
    /// Fetches the OIDC token from the runner (the job needs
    /// `permissions: id-token: write`), POSTs it to the gateway's exchange
    /// endpoint, and uses the returned credential. Needs `--oidc-audience`.
    #[arg(long, conflicts_with = "auth_token")]
    github_oidc: bool,

    /// Use this OIDC token for the exchange instead of fetching one from the
    /// runner. Needs `--oidc-audience`.
    #[arg(
        long,
        env = "SKIMASQUE_OIDC_TOKEN",
        value_name = "JWT",
        hide_env_values = true,
        conflicts_with = "auth_token"
    )]
    oidc_token: Option<String>,

    /// The audience the OIDC token is minted for. Must match one of the
    /// gateway's `--oidc-audience` values.
    #[arg(long, value_name = "AUD")]
    oidc_audience: Option<String>,

    /// The organisation to mint a credential for, when authenticating with a
    /// `skimasque login` session instead of `--github-oidc`/`--auth-token`.
    /// Defaults to the session's only organisation; required if it belongs to
    /// more than one.
    #[arg(long, value_name = "ORG")]
    org: Option<String>,

    /// The application name to declare, sent as `X-Masque-Application`.
    ///
    /// A policy-enforcing proxy matches on this. It is session context, not an
    /// authenticated fact -- the proxy trusts it exactly as far as it trusts
    /// this client.
    #[arg(long, value_name = "NAME")]
    app: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run a SOCKS5 server whose UDP associations go through the proxy.
    Socks5 {
        /// Address to serve SOCKS5 on.
        #[arg(long, default_value = "127.0.0.1:1080", value_name = "ADDR")]
        listen: SocketAddr,
    },
    /// Open a raw TCP tunnel and bridge it to stdin/stdout.
    ///
    /// Useful as an SSH `ProxyCommand` (`ProxyCommand skimasque-client ...
    /// connect --target %h:%p`) and for interop testing.
    Connect {
        /// The destination, as `host:port`.
        #[arg(long, value_name = "HOST:PORT")]
        target: String,
    },
    /// Send a payload through a tunnel and print the replies.
    Probe {
        /// The destination, as `host:port`.
        #[arg(long, value_name = "HOST:PORT")]
        target: String,

        /// Send a DNS query for this name.
        #[arg(long, value_name = "NAME", group = "payload")]
        dns: Option<String>,

        /// Ask for AAAA instead of A. Only meaningful with `--dns`.
        #[arg(long, requires = "dns")]
        aaaa: bool,

        /// Send this text.
        #[arg(long, value_name = "TEXT", group = "payload")]
        text: Option<String>,

        /// Send these hex bytes. Spaces and colons are ignored.
        #[arg(long, value_name = "HEX", group = "payload")]
        hex: Option<String>,

        /// How many times to send.
        #[arg(long, default_value_t = 1, value_name = "N")]
        count: u32,

        /// How long to wait for each reply.
        #[arg(long, default_value_t = 3000, value_name = "MS")]
        timeout: u64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    init_tracing(args.verbose);

    let Connected { session, refresh } = open_session(&args.connection).await?;
    let session = Arc::new(session);
    eprintln!(
        "connected to {} serving {}",
        session.remote_address(),
        session.template().as_str()
    );

    // In OIDC mode the platform credential outlives neither a long deploy nor
    // the gateway's `--credential-ttl`, so keep it fresh for as long as we run.
    if let Some((exchange, ttl)) = refresh {
        tokio::spawn(refresh_credential(session.clone(), exchange, ttl));
    }

    match args.command {
        Command::Socks5 { listen } => run_socks5(listen, session).await,
        Command::Connect { target } => run_connect(session, &target).await,
        Command::Probe {
            target,
            dns,
            aaaa,
            text,
            hex,
            count,
            timeout,
        } => {
            run_probe(
                session,
                &target,
                build_payload(dns.as_deref(), aaaa, text.as_deref(), hex.as_deref())?,
                count,
                Duration::from_millis(timeout),
            )
            .await
        }
    }
}

/// An open session, plus -- when the credential is short-lived -- what a
/// background task needs to replace it before it expires.
struct Connected {
    session: Session,
    refresh: Option<(RefreshMode, Duration)>,
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

async fn open_session(args: &ConnectionArgs) -> anyhow::Result<Connected> {
    let (proxy, proxy_authority) = resolve_proxy(&args.proxy).await?;

    let client_tls = match (&args.ca, args.insecure) {
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

    let authority = args.authority.clone().unwrap_or(proxy_authority);
    let template = match &args.template {
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
        .with_context(|| format!("connecting to the gateway at {} ({proxy})", args.proxy))?;

    let bearer = resolve_bearer(args, &session).await?;

    let mut headers = HeaderMap::new();
    if let Some(bearer) = &bearer.header {
        let value = HeaderValue::from_str(&format!("Bearer {bearer}"))
            .context("the credential contains characters a header cannot carry")?;
        headers.insert(http::header::PROXY_AUTHORIZATION, value);
    }
    if let Some(app) = &args.app {
        let value = HeaderValue::from_str(app)
            .context("the application name contains characters a header cannot carry")?;
        headers.insert(skimasque::APPLICATION_HEADER, value);
    }

    let session = if headers.is_empty() {
        session
    } else {
        session.with_default_headers(headers)
    };
    Ok(Connected {
        session,
        refresh: bearer.refresh,
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

fn auth_mode(args: &ConnectionArgs, has_session: bool) -> AuthMode {
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
async fn resolve_bearer(args: &ConnectionArgs, session: &Session) -> anyhow::Result<Bearer> {
    let creds = skimasque_cli::account::load()?;
    match auth_mode(args, creds.is_some()) {
        AuthMode::Oidc => {
            let audience = args.oidc_audience.as_deref().context(
                "--github-oidc / --oidc-token also need --oidc-audience (the value the gateway expects)",
            )?;
            let exchange = OidcExchange {
                audience: audience.to_owned(),
                static_token: args.oidc_token.clone(),
            };
            let credential = exchange.run(session).await?;
            eprintln!(
                "exchanged an OIDC token for a platform credential (valid {}s)",
                credential.expires_in.as_secs()
            );
            let ttl = credential.expires_in;
            Ok(Bearer {
                header: Some(credential.token),
                refresh: Some((RefreshMode::Oidc(exchange), ttl)),
            })
        }
        AuthMode::Static => Ok(Bearer {
            header: args.auth_token.clone(),
            refresh: None,
        }),
        AuthMode::Session => {
            let creds = creds.expect("AuthMode::Session implies a stored session");
            let api = skimasque_cli::account::Api::new(&creds.control_plane)?;
            let org = skimasque_cli::account::resolve_org(&api, &creds, args.org.clone()).await?;
            let exchange = SessionExchange { creds, org };
            let credential = exchange.run().await?;
            eprintln!(
                "minted a platform credential from your skimasque login session (valid {}s)",
                credential.expires_in.as_secs()
            );
            let ttl = credential.expires_in;
            Ok(Bearer {
                header: Some(credential.token),
                refresh: Some((RefreshMode::Session(exchange), ttl)),
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
struct OidcExchange {
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
/// (`skimasque_cli::account::Api`), not through the gateway's exchange
/// endpoint: the session is a management-API credential the client already
/// holds, and the resulting credential -- Ed25519-signed with the org's key
/// -- verifies against any gateway in the org, not just the one currently
/// connected to.
struct SessionExchange {
    creds: skimasque_cli::account::Credentials,
    org: String,
}

impl SessionExchange {
    /// Mint a fresh platform credential from the stored login session.
    async fn run(&self) -> anyhow::Result<Credential> {
        let api = skimasque_cli::account::Api::new(&self.creds.control_plane)?;
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
enum RefreshMode {
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
fn refresh_lead_time(ttl: Duration) -> Duration {
    (ttl / 4).clamp(Duration::from_secs(10), Duration::from_secs(15 * 60))
}

/// Keep `session`'s platform credential fresh for as long as the process runs.
///
/// A credential has a finite TTL (the gateway's `--credential-ttl` in OIDC
/// mode, or the control plane's cap in session mode), so a job or an
/// interactive session that outlives one would see its tunnels start failing
/// with `407`/`403`. This re-runs `exchange` ahead of each expiry and installs
/// the new credential on the live session; tunnels opened afterwards carry
/// it, and tunnels already open are undisturbed.
async fn refresh_credential(session: Arc<Session>, exchange: RefreshMode, mut ttl: Duration) {
    loop {
        tokio::time::sleep(ttl.saturating_sub(refresh_lead_time(ttl))).await;

        match exchange.run(&session).await {
            Ok(credential) => match session.set_credential(&credential) {
                Ok(()) => {
                    eprintln!(
                        "refreshed the platform credential (valid {}s)",
                        credential.expires_in.as_secs()
                    );
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

async fn run_connect(session: Arc<Session>, target: &str) -> anyhow::Result<()> {
    let target = Target::parse(target).context("parsing --target")?;
    let tunnel = session
        .connect_tcp(target.clone())
        .await
        .with_context(|| format!("opening a TCP tunnel to {target}"))?;
    eprintln!("tunnel open to {target}; bridging stdin/stdout");
    let stdio = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
    tunnel.relay(stdio).await?;
    Ok(())
}

async fn run_socks5(listen: SocketAddr, session: Arc<Session>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("binding {listen}"))?;
    let addr = listener.local_addr()?;
    eprintln!("SOCKS5 listening on {addr} (CONNECT and UDP ASSOCIATE)");
    eprintln!("point a client at socks5h://{addr} so names resolve at the proxy");

    tokio::select! {
        result = socks5::serve(listener, session) => Ok(result?),
        _ = tokio::signal::ctrl_c() => {
            eprintln!("\nshutting down");
            Ok(())
        }
    }
}

fn build_payload(
    dns: Option<&str>,
    aaaa: bool,
    text: Option<&str>,
    hex: Option<&str>,
) -> anyhow::Result<Payload> {
    match (dns, text, hex) {
        (Some(name), _, _) => {
            // Any unpredictable id will do; the point is to notice a reply that
            // does not belong to this query.
            let id = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u16)
                .unwrap_or(0x1234);
            let qtype = if aaaa { TYPE_AAAA } else { TYPE_A };
            let bytes = probe::dns_query(id, name, qtype).map_err(anyhow::Error::msg)?;
            Ok(Payload {
                bytes,
                dns_id: Some(id),
            })
        }
        (None, Some(text), _) => Ok(Payload {
            bytes: text.as_bytes().to_vec(),
            dns_id: None,
        }),
        (None, None, Some(hex)) => Ok(Payload {
            bytes: probe::parse_hex(hex).map_err(anyhow::Error::msg)?,
            dns_id: None,
        }),
        (None, None, None) => {
            anyhow::bail!("give the probe something to send: --dns, --text or --hex")
        }
    }
}

struct Payload {
    bytes: Vec<u8>,
    /// Set when the payload is a DNS query, so the reply can be checked.
    dns_id: Option<u16>,
}

async fn run_probe(
    session: Arc<Session>,
    target: &str,
    payload: Payload,
    count: u32,
    reply_timeout: Duration,
) -> anyhow::Result<()> {
    let target = Target::parse(target).context("parsing --target")?;
    let mut tunnel = session
        .connect_udp(target.clone())
        .await
        .with_context(|| format!("opening a tunnel to {target}"))?;

    println!(
        "tunnel to {target} open on stream {} (max payload {} bytes)",
        tunnel.stream_id(),
        tunnel
            .max_payload_size()
            .map_or_else(|| "unknown".to_owned(), |n| n.to_string())
    );
    println!(
        "sending {} bytes:\n{}",
        payload.bytes.len(),
        probe::hexdump(&payload.bytes)
    );

    let mut received = 0u32;
    for attempt in 1..=count {
        let sent_at = Instant::now();
        tunnel.send(&payload.bytes)?;

        match timeout(reply_timeout, tunnel.recv()).await {
            Ok(Some(reply)) => {
                received += 1;
                println!(
                    "reply {attempt}: {} bytes in {:.1?}",
                    reply.len(),
                    sent_at.elapsed()
                );
                if let Some(query_id) = payload.dns_id {
                    print_dns_summary(query_id, &reply);
                }
                print!("{}", probe::hexdump(&reply));
            }
            Ok(None) => {
                println!("reply {attempt}: the proxy closed the tunnel");
                break;
            }
            Err(_) => println!("reply {attempt}: timed out after {reply_timeout:.1?}"),
        }
    }

    println!("{received}/{count} replies received");
    tunnel.close().await?;

    // A tunnel that sent but never heard back is a failure worth an exit code.
    if received == 0 {
        anyhow::bail!("no replies");
    }
    Ok(())
}

fn print_dns_summary(query_id: u16, reply: &[u8]) {
    let Some(header) = DnsHeader::parse(reply) else {
        println!("  (too short to be a DNS reply)");
        return;
    };
    if header.id != query_id {
        println!(
            "  warning: transaction id {:#06x} does not match the query's {:#06x}",
            header.id, query_id
        );
    }
    println!(
        "  DNS: {} rcode={} questions={} answers={}",
        if header.is_response {
            "response"
        } else {
            "query"
        },
        header.response_code_name(),
        header.questions,
        header.answers
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// clap's own consistency checks. Without this, a contradiction like
    /// "global and required" only surfaces the first time someone runs the
    /// binary.
    #[test]
    fn the_command_definition_is_consistent() {
        Args::command().debug_assert();
    }

    #[test]
    fn the_refresh_lead_time_stays_within_bounds() {
        // A quarter of the lifetime in the ordinary case.
        assert_eq!(
            refresh_lead_time(Duration::from_secs(3600)),
            Duration::from_secs(900)
        );
        // Clamped up, so a short-lived credential still leaves a usable window.
        assert_eq!(
            refresh_lead_time(Duration::from_secs(8)),
            Duration::from_secs(10)
        );
        // Clamped down, so an hours-long credential is not re-minted constantly.
        assert_eq!(
            refresh_lead_time(Duration::from_secs(24 * 3600)),
            Duration::from_secs(15 * 60)
        );
    }

    #[test]
    fn the_proxy_gets_a_default_port_of_443() {
        assert_eq!(proxy_with_port("gateway.skimasque.com"), "gateway.skimasque.com:443");
        assert_eq!(proxy_with_port("gateway.skimasque.com:8443"), "gateway.skimasque.com:8443");
        assert_eq!(proxy_with_port("10.0.0.1"), "10.0.0.1:443");
        assert_eq!(proxy_with_port("10.0.0.1:4433"), "10.0.0.1:4433");
        assert_eq!(proxy_with_port("::1"), "[::1]:443");
        assert_eq!(proxy_with_port("[::1]"), "[::1]:443");
        assert_eq!(proxy_with_port("[::1]:4433"), "[::1]:4433");
        assert_eq!(proxy_with_port("2001:db8::1"), "[2001:db8::1]:443");
    }

    #[test]
    fn a_probe_needs_something_to_send() {
        let payload = build_payload(None, false, None, None);
        assert!(payload.is_err());
    }

    #[test]
    fn probe_payloads_come_from_whichever_source_was_given() {
        assert_eq!(
            build_payload(None, false, Some("hi"), None).unwrap().bytes,
            b"hi"
        );
        assert_eq!(
            build_payload(None, false, None, Some("de ad"))
                .unwrap()
                .bytes,
            vec![0xde, 0xad]
        );
        let dns = build_payload(Some("example.com"), false, None, None).unwrap();
        assert!(dns.dns_id.is_some(), "a DNS probe must track its query id");
        assert_eq!(
            &dns.bytes[dns.bytes.len() - 4..],
            &[0, 1, 0, 1],
            "QTYPE=A QCLASS=IN"
        );

        let aaaa = build_payload(Some("example.com"), true, None, None).unwrap();
        assert_eq!(
            &aaaa.bytes[aaaa.bytes.len() - 4..],
            &[0, 28, 0, 1],
            "QTYPE=AAAA"
        );
    }

    fn connection_args(extra: &[&str]) -> ConnectionArgs {
        let mut argv = vec!["skimasque-client"];
        argv.extend_from_slice(extra);
        argv.extend_from_slice(&["connect", "--target", "1.1.1.1:53"]);
        Args::try_parse_from(argv).unwrap().connection
    }

    #[test]
    fn auth_mode_picks_oidc_over_everything_else() {
        let args = connection_args(&["--github-oidc", "--oidc-audience", "https://gw.example"]);
        assert_eq!(auth_mode(&args, true), AuthMode::Oidc);
        assert_eq!(auth_mode(&args, false), AuthMode::Oidc);
    }

    #[test]
    fn auth_mode_picks_static_token_when_no_oidc_flag_is_given() {
        let args = connection_args(&["--auth-token", "secret"]);
        assert_eq!(auth_mode(&args, true), AuthMode::Static);
        assert_eq!(auth_mode(&args, false), AuthMode::Static);
    }

    #[test]
    fn auth_mode_falls_back_to_a_stored_session_when_nothing_else_is_given() {
        let args = connection_args(&[]);
        assert_eq!(auth_mode(&args, true), AuthMode::Session);
    }

    #[test]
    fn auth_mode_is_none_with_no_flags_and_no_session() {
        // The exact case that used to send no credential at all and let the
        // gateway's 407 be the only signal -- resolve_bearer now fails fast
        // on this instead.
        let args = connection_args(&[]);
        assert_eq!(auth_mode(&args, false), AuthMode::None);
    }
}
