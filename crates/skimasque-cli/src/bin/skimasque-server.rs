//! `skimasque-server` -- a MASQUE UDP proxy (RFC 9298) over HTTP/3.
//!
//! This is the gateway data plane. `skimasque gateway <args>` is the front door
//! for it -- it execs this binary with the arguments passed through -- but the
//! two are equivalent and this one can be run directly.

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Context;
use clap::{ArgAction, Parser};
use skimasque::audit::{AuditSink, JsonlAuditSink, TracingAuditSink};
use skimasque::exchange::{CredentialMinter, MintError, MintedCredential};
use skimasque::policy::AddressPolicy;
use skimasque::rustls;
use skimasque::server::{ConnectionRate, ProxyConfig, ResourceLimits, Server, TlsReloader};
use skimasque::service::{
    Accepted, AuthorizeLayer, Dispatch, IdentityLayer, IdentityVerifier, PolicyHandle, PolicyLayer,
    QuotaLayer, Rejection, TcpProxy, TunnelRequest, UdpProxy,
};
use skimasque::tls;
use skimasque_cli::init_tracing;
use skimasque_core::template::CONNECT_UDP_VARIABLES;
use skimasque_core::UriTemplate;
use skimasque_identity::{
    ClaimNames, CredentialIssuer, CredentialVerifier, OidcVerifier, Provider,
};
use skimasque_policy::WorkloadIdentity;
use tower::limit::GlobalConcurrencyLimitLayer;
use tower::util::BoxCloneService;
use tower::ServiceBuilder;

/// A MASQUE proxy for UDP over HTTP/3.
///
/// For a public deployment, pass `--acme` (and `--hostname <name>` unless it is
/// `gateway.skimasque.com`): the gateway obtains a Let's Encrypt certificate
/// over TLS-ALPN-01 and renews it automatically, and clients connect with no
/// `--ca`.
/// Otherwise pass `--cert`/`--key`, or give neither and a throwaway self-signed
/// certificate is generated (`--write-cert` saves it for a client to pin with
/// `--ca`).
#[derive(Debug, Parser)]
#[command(
    name = "skimasque-server",
    version,
    about = "The skimasque MASQUE gateway (also reachable as `skimasque gateway`)",
    long_about = None
)]
struct Args {
    /// Address to listen on.
    #[arg(long, default_value = "0.0.0.0:4433")]
    listen: SocketAddr,

    /// PEM certificate chain to serve.
    #[arg(long, requires = "key", value_name = "PATH")]
    cert: Option<PathBuf>,

    /// PEM private key matching `--cert`.
    #[arg(long, requires = "cert", value_name = "PATH")]
    key: Option<PathBuf>,

    /// Re-read `--cert` and `--key` when they change on disk and present the new
    /// certificate to connections opened afterwards, without a restart.
    ///
    /// Connections already established keep the certificate they handshook with.
    /// A revision that fails to load -- bad PEM, or a key that does not match the
    /// certificate -- is logged and ignored. On Unix, `SIGHUP` also forces an
    /// immediate re-read.
    #[arg(long, requires = "cert")]
    tls_reload: bool,

    /// How often to check `--cert` / `--key` for changes when `--tls-reload` is
    /// set. Accepts `5s`, `1m`.
    #[arg(
        long,
        value_name = "DURATION",
        default_value = "5s",
        requires = "tls_reload"
    )]
    tls_reload_interval: String,

    /// Subject name for a generated certificate. Repeatable.
    #[arg(
        long = "self-signed-name",
        value_name = "NAME",
        default_value = "localhost"
    )]
    self_signed_names: Vec<String>,

    /// Write the certificate being served to this path, for clients to pin.
    #[arg(long, value_name = "PATH")]
    write_cert: Option<PathBuf>,

    /// The public name this gateway is reached at. Used as the certificate
    /// domain for `--acme`, and as the default `--authority`.
    #[arg(long, value_name = "HOST[:PORT]", default_value = "gateway.skimasque.com")]
    hostname: String,

    /// Obtain a publicly-trusted certificate from Let's Encrypt via ACME
    /// (TLS-ALPN-01) for `--hostname`, and renew it automatically ahead of
    /// expiry.
    ///
    /// The only requirement is that `--hostname` resolves to this gateway and
    /// TCP port 443 is reachable from the internet. Clients then connect with
    /// no `--ca` (the default Mozilla root store trusts the certificate).
    /// Mutually exclusive with `--cert`.
    #[cfg(feature = "acme")]
    #[arg(long, conflicts_with_all = ["cert", "tls_reload", "write_cert"])]
    acme: bool,

    /// An additional name to include on the ACME certificate, beyond
    /// `--hostname`. Repeatable.
    #[cfg(feature = "acme")]
    #[arg(long = "acme-extra-domain", value_name = "DOMAIN", requires = "acme")]
    acme_extra_domains: Vec<String>,

    /// ACME account contact; Let's Encrypt uses it for expiry warnings. A bare
    /// address is given a `mailto:` prefix.
    #[cfg(feature = "acme")]
    #[arg(long, value_name = "EMAIL", requires = "acme")]
    acme_email: Option<String>,

    /// Directory for the cached ACME account key and issued certificate. Must
    /// survive restarts, or Let's Encrypt's rate limits will bite.
    #[cfg(feature = "acme")]
    #[arg(long, value_name = "DIR", default_value = "skimasque-acme", requires = "acme")]
    acme_cache: PathBuf,

    /// Use the Let's Encrypt staging environment (certificates browsers do not
    /// trust, but with generous rate limits) instead of production. For testing.
    #[cfg(feature = "acme")]
    #[arg(long, requires = "acme")]
    acme_staging: bool,

    /// TCP port for the ACME TLS-ALPN-01 challenge. Defaults to the `--listen`
    /// port. Let's Encrypt only ever validates on port 443; override this only
    /// for a local test rig.
    #[cfg(feature = "acme")]
    #[arg(long, value_name = "PORT", requires = "acme")]
    acme_challenge_port: Option<u16>,

    /// The authority this proxy is reached at, used to build its URI Template.
    /// Defaults to `--hostname`.
    #[arg(long, value_name = "HOST[:PORT]")]
    authority: Option<String>,

    /// A complete URI Template to serve, overriding `--authority`.
    #[arg(long, value_name = "TEMPLATE")]
    template: Option<String>,

    /// Require `Proxy-Authorization: Bearer <token>` on every request.
    #[arg(
        long,
        env = "SKIMASQUE_TOKEN",
        value_name = "TOKEN",
        hide_env_values = true
    )]
    auth_token: Option<String>,

    /// Permit tunnels to loopback, private and link-local addresses.
    ///
    /// Off by default: a proxy that forwards to private space is a way to reach
    /// whatever is behind it, including cloud metadata services. Prefer
    /// `--allow-cidr` to name the exact internal targets you need.
    #[arg(long)]
    allow_private: bool,

    /// Permit tunnels to addresses in this network, overriding the private /
    /// loopback / link-local floor for that range only. Repeatable, e.g.
    /// `--allow-cidr 10.0.5.0/24 --allow-cidr [fd00:1::]/64`. The port rules
    /// still apply. This is the precise alternative to `--allow-private`.
    #[arg(long = "allow-cidr", value_name = "CIDR")]
    allow_cidrs: Vec<ipnet::IpNet>,

    /// Restrict destinations to this port. Repeatable; unset means any port.
    #[arg(long = "allow-port", value_name = "PORT")]
    allow_ports: Vec<u16>,

    /// Enforce the identity-aware policies in this directory (`*.toml`,
    /// `*.yaml`).
    ///
    /// Without it the proxy applies only the address floor above; with it,
    /// every tunnel must be allowed by a matching policy rule, and the
    /// per-policy concurrency limits are enforced.
    #[arg(long, value_name = "DIR", group = "policy_source")]
    policy_dir: Option<PathBuf>,

    /// Enforce these specific policy files instead of scanning `--policy-dir`.
    #[arg(
        long = "policy-file",
        value_name = "PATH",
        conflicts_with = "policy_dir",
        group = "policy_source"
    )]
    policy_files: Vec<PathBuf>,

    /// Get policy from a control plane instead of local files. `skimasque
    /// gateway register` prints this line for you, with the URL (SkiMasque
    /// Cloud, or a self-hosted control plane) and a registration token filled
    /// in.
    ///
    /// The gateway registers once (with `--control-plane-token`), pulls its
    /// policy, then long-polls for changes and sends heartbeats. If the control
    /// plane is unreachable it keeps enforcing the last policy it cached under
    /// `--control-plane-state`; enforcement never depends on the control plane
    /// being up.
    #[arg(long, value_name = "URL", group = "policy_source", requires = "control_plane_state")]
    control_plane: Option<String>,

    /// Directory for the control-plane identity and the cached policy. Required
    /// with `--control-plane`.
    #[arg(long, value_name = "DIR")]
    control_plane_state: Option<PathBuf>,

    /// One-time registration token for the first `--control-plane` connection.
    /// Not needed once the gateway has registered (its identity is in
    /// `--control-plane-state`).
    #[arg(
        long,
        env = "SKIMASQUE_CONTROL_TOKEN",
        value_name = "TOKEN",
        hide_env_values = true
    )]
    control_plane_token: Option<String>,

    /// The name this gateway is shown as in the control plane's fleet view.
    /// Defaults to the listen address.
    #[arg(long, value_name = "NAME")]
    control_plane_name: Option<String>,

    /// How long each policy long-poll waits server-side for a change, and the
    /// heartbeat interval. Accepts `30s`, `1m`.
    #[arg(long, value_name = "DURATION", default_value = "30s")]
    control_plane_interval: String,

    /// Soft policy lease. Once the control plane has been unreachable for this
    /// long the gateway reports itself degraded, but keeps enforcing the cached
    /// policy. Accepts `15m`, `1h`.
    #[arg(long, value_name = "DURATION", default_value = "15m")]
    control_plane_policy_lease: String,

    /// Hard policy cache TTL. Once the cached policy is this old and the control
    /// plane is still unreachable the gateway escalates the alarm (an
    /// error-level log and the `skimasque_control_plane_policy_expired` gauge)
    /// but *still keeps enforcing it* -- a control-plane outage never stops the
    /// data plane, and the gateway never fails open. Must be at least
    /// `--control-plane-policy-lease`. Accepts `30m`, `2h`.
    #[arg(long, value_name = "DURATION", default_value = "30m")]
    control_plane_cache_ttl: String,

    /// With `--control-plane` and `--oidc`, do not fall back to signing
    /// platform credentials locally when the control plane's mint endpoint is
    /// unreachable. Token exchange then fails (502) during a control-plane
    /// outage instead of issuing a short-lived credential from the local
    /// `--credential-secret`. Tunnel enforcement is unaffected either way.
    #[arg(long, requires = "control_plane")]
    control_plane_no_credential_fallback: bool,

    /// How often to re-fetch the org's Ed25519 signing key from the control
    /// plane, picking up a rotation without a restart. The key changes rarely,
    /// so this is slow by default. Accepts `1h`, `30m`.
    #[arg(long, value_name = "DURATION", default_value = "1h")]
    control_plane_signing_key_interval: String,

    /// An attribute this gateway declares about itself, `KEY=VALUE`, repeatable.
    /// The control plane serves a policy document to this gateway only if the
    /// document's `target` is a subset of these labels; a document with no
    /// target goes to every gateway (D3 policy scoping). Re-sent on every start,
    /// so it also updates a previously registered gateway.
    #[arg(long = "control-plane-label", value_name = "KEY=VALUE", requires = "control_plane")]
    control_plane_label: Vec<String>,

    /// Observe instead of enforce: log what each policy would decide (as
    /// `masque::observe` events) but allow every tunnel. Feeds
    /// `skimasque policy learn`.
    #[arg(long, requires = "policy_source")]
    policy_observe: bool,

    /// Re-read the policy source when it changes on disk and swap it in without
    /// a restart.
    ///
    /// Live tunnels are undisturbed -- a policy decision is made only while a
    /// tunnel is opening -- and the new set applies to every tunnel opened after
    /// it loads. A revision that fails to parse is logged and ignored, so the
    /// gateway keeps enforcing the last good one. On Unix, `SIGHUP` also forces
    /// an immediate re-read.
    #[arg(long, requires = "policy_source", conflicts_with = "control_plane")]
    policy_reload: bool,

    /// How often to check the policy source for changes when `--policy-reload`
    /// is set. Accepts `5s`, `1m`.
    #[arg(
        long,
        value_name = "DURATION",
        default_value = "5s",
        requires = "policy_reload"
    )]
    policy_reload_interval: String,

    /// Append a JSON-lines audit record of every policy allow and deny to this
    /// file.
    ///
    /// One decision per line: timestamp, workload identity, application,
    /// destination, and the rule or the denial reason. This is the compliance
    /// artifact. Without it, decisions still go to the `masque::audit` tracing
    /// target. Requires an enforcing policy, so it conflicts with
    /// `--policy-observe`.
    #[arg(
        long,
        value_name = "PATH",
        requires = "policy_source",
        conflicts_with = "policy_observe"
    )]
    audit_log: Option<PathBuf>,

    /// Verify CI OIDC tokens and enable the token-exchange endpoint.
    ///
    /// A client POSTs its OIDC token to the exchange endpoint; the gateway
    /// verifies it, maps the claims to a workload identity, and returns a
    /// short-lived platform credential. Tunnels then present the credential in
    /// `Proxy-Authorization: Bearer`, which the gateway verifies locally.
    ///
    /// This replaces `--auth-token`: a request without a verifiable credential
    /// is refused. `--github-oidc` is the same flag.
    #[arg(long = "oidc", alias = "github-oidc", conflicts_with = "auth_token")]
    oidc: bool,

    /// Which CI system's OIDC tokens to accept: `github`, `gitlab`, `buildkite`,
    /// or `generic` (map claims with `--oidc-claim`).
    #[arg(
        long = "oidc-provider",
        value_name = "NAME",
        default_value = "github",
        requires = "oidc"
    )]
    oidc_provider: String,

    /// Accept this string as the OIDC token's `aud` claim. Repeatable.
    ///
    /// Required with `--oidc`. Choose a stable name for this gateway and have
    /// each pipeline request its token for that audience.
    #[arg(long = "oidc-audience", value_name = "AUD", requires = "oidc")]
    oidc_audiences: Vec<String>,

    /// The OIDC issuer to trust and to discover signing keys from.
    ///
    /// Defaults to the provider's hosted issuer; set it for a self-managed
    /// GitLab, a GitHub Enterprise Server host, or `--oidc-provider generic`.
    #[arg(long = "oidc-issuer", value_name = "URL", requires = "oidc")]
    oidc_issuer: Option<String>,

    /// For `--oidc-provider generic`: `FIELD=CLAIM`, mapping an identity field to
    /// the token claim it comes from. Repeatable.
    ///
    /// FIELD is one of `organization`, `repository`, `workflow`, `ref`,
    /// `environment`, `actor` -- e.g. `--oidc-claim repository=project_path`.
    #[arg(long = "oidc-claim", value_name = "FIELD=CLAIM", requires = "oidc")]
    oidc_claims: Vec<String>,

    /// Hex-encoded HS256 secret for signing platform credentials.
    ///
    /// Set this (the same value on every gateway) so a credential one gateway
    /// issues is accepted by another. Unset means a random secret per start:
    /// fine for one gateway, and clients simply re-exchange after a restart.
    #[arg(
        long,
        env = "SKIMASQUE_CREDENTIAL_SECRET",
        value_name = "HEX",
        hide_env_values = true,
        requires = "oidc"
    )]
    credential_secret: Option<String>,

    /// How long an issued platform credential is valid. Accepts `45m`, `2h`.
    #[arg(long, value_name = "DURATION", default_value = "1h")]
    credential_ttl: String,

    /// Also serve TCP tunnels via classic `CONNECT host:port`. This is what a
    /// SOCKS front end needs to carry `curl`, `git` and database traffic.
    #[arg(long)]
    connect_tcp: bool,

    /// Serve `/healthz`, `/readyz` and Prometheus `/metrics` on this address.
    ///
    /// A plain-HTTP listener separate from the QUIC data plane, for a load
    /// balancer's health checks and a metrics scrape. Keep it on an interface
    /// only your infrastructure can reach.
    #[arg(long, value_name = "ADDR")]
    metrics_listen: Option<SocketAddr>,

    /// Cap on tunnels being opened at once, across all connections.
    ///
    /// This bounds work in progress in the authorization path. It is distinct
    /// from `--max-connections` (open QUIC connections) and
    /// `--max-tunnels-per-connection` (established tunnels on one connection).
    #[arg(long, default_value_t = 1024, value_name = "N")]
    max_concurrent_requests: usize,

    /// Cap on QUIC connections served at once. A further connection is refused
    /// until one ends. `0` removes the cap.
    #[arg(long, default_value_t = 1024, value_name = "N")]
    max_connections: usize,

    /// Sustained ceiling on *new* connections per second. `--max-connections`
    /// bounds how many are open; this bounds how fast they arrive, so a client
    /// that opens and closes connections in a loop cannot burn handshake CPU.
    /// Global across all clients. `0` removes the limit.
    #[arg(long, default_value_t = 50, value_name = "N")]
    max_connection_rate: u32,

    /// How many new connections may arrive in a clump before
    /// `--max-connection-rate` throttles them -- sized for a CI matrix starting
    /// many jobs at once. Ignored when the rate limit is off.
    #[arg(long, default_value_t = 200, value_name = "N")]
    max_connection_burst: u32,

    /// `--max-connection-rate`, but enforced per remote IP, so one misbehaving
    /// source cannot spend the global budget and starve other runners. Checked
    /// first. `0` removes the per-source limit.
    ///
    /// Raise or disable this when many runners share one NAT egress address --
    /// they all count as one source.
    #[arg(long, default_value_t = 20, value_name = "N")]
    max_source_connection_rate: u32,

    /// Burst allowance for `--max-source-connection-rate`. Ignored when the
    /// per-source limit is off.
    #[arg(long, default_value_t = 60, value_name = "N")]
    max_source_connection_burst: u32,

    /// Sustained ceiling on token-exchange requests per remote IP per second.
    /// Each exchange costs an RS256 verify (and sometimes a JWKS fetch) and is
    /// handled before `--max-concurrent-requests`, so one connection could
    /// otherwise flood it. Only relevant with `--oidc`. `0` removes the limit.
    #[arg(long, default_value_t = 10, value_name = "N")]
    max_exchange_rate: u32,

    /// Burst allowance for `--max-exchange-rate`. Ignored when that limit is off.
    #[arg(long, default_value_t = 30, value_name = "N")]
    max_exchange_burst: u32,

    /// Cap on tunnels open at once on a single QUIC connection. A request over
    /// this number is answered `503` and the connection stays up. `0` removes
    /// the cap.
    #[arg(long, default_value_t = 256, value_name = "N")]
    max_tunnels_per_connection: usize,

    /// Tear a tunnel down once it has carried no data in either direction for
    /// this long. Accepts `90s`, `5m`; `0` disables it.
    ///
    /// The QUIC idle timeout still applies to a connection as a whole; this
    /// reclaims one idle tunnel inside an otherwise busy connection, which
    /// matters most for long-lived `--connect-tcp` tunnels.
    #[arg(long, value_name = "DURATION", default_value = "120s")]
    tunnel_idle_timeout: String,

    /// How long to let in-flight tunnels finish after a shutdown signal
    /// (`SIGINT`/`SIGTERM`) before the QUIC endpoint is closed under them.
    ///
    /// On the signal the proxy stops accepting connections and sends every
    /// client a GOAWAY; existing tunnels keep running until they close or this
    /// deadline passes. Accepts `10s`, `2m`.
    #[arg(long, value_name = "DURATION", default_value = "10s")]
    shutdown_grace: String,

    /// Increase logging; repeat for more.
    #[arg(short, long, action = ArgAction::Count)]
    verbose: u8,
}

/// The proxy service, boxed so the optional layers do not change its type.
type ProxyService = BoxCloneService<TunnelRequest, Accepted, Rejection>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    init_tracing(args.verbose);

    // Install the metrics recorder before anything can emit, so early counters
    // land. The readiness latch is flipped once the endpoint is up and policy
    // is loaded.
    let ops = match args.metrics_listen {
        Some(_) => Some((skimasque_cli::ops::install_metrics()?, skimasque_cli::ops::Ready::new())),
        None => None,
    };

    let (server_tls, tls_setup) = load_tls(&args).await?;
    match &tls_setup {
        TlsSetup::Static(certificate_pem) => {
            if let Some(path) = &args.write_cert {
                tokio::fs::write(path, certificate_pem.as_bytes())
                    .await
                    .with_context(|| format!("writing the certificate to {}", path.display()))?;
                tracing::info!(path = %path.display(), "wrote the serving certificate");
            }
        }
        #[cfg(feature = "acme")]
        TlsSetup::Acme(_) => {}
    }

    let oidc = build_oidc(&args)?;
    if oidc.as_ref().is_some_and(|o| o.generated_secret) {
        eprintln!(
            "warning: no --credential-secret set; using a random one, so credentials do not \
             survive a restart and cannot be verified by another gateway"
        );
    }

    let mut config =
        ProxyConfig::for_template(build_template(&args)?).with_limits(build_limits(&args)?);

    let shutdown_grace = skimasque_policy::parse_duration(&args.shutdown_grace)
        .map_err(|e| anyhow::anyhow!("parsing --shutdown-grace: {e}"))?;

    let reload_interval = if args.policy_reload {
        Some(
            skimasque_policy::parse_duration(&args.policy_reload_interval)
                .map_err(|e| anyhow::anyhow!("parsing --policy-reload-interval: {e}"))?,
        )
    } else {
        None
    };

    let tls_reload_interval = if args.tls_reload {
        Some(
            skimasque_policy::parse_duration(&args.tls_reload_interval)
                .map_err(|e| anyhow::anyhow!("parsing --tls-reload-interval: {e}"))?,
        )
    } else {
        None
    };

    // When `--control-plane` is set: register (or reuse the stored identity),
    // pull the initial policy into the on-disk cache, and hand the cache dir to
    // `build_service` as the policy source. A control plane that is unreachable
    // at startup is tolerated as long as a cached policy exists.
    let control_plane = bootstrap_control_plane(&args).await?;
    let control_policy_dir = control_plane.as_ref().map(|cp| cp.control.policy_dir());

    // With both `--control-plane` and `--oidc`, re-wire issuance/verification to
    // the org's Ed25519 key (D1). Otherwise the local HS256 issuer stands.
    let (oidc, signing_key_refresh) = match (oidc, control_plane.as_ref()) {
        (Some(parts), Some(cp)) => {
            let (parts, refresh) =
                wire_oidc_to_control_plane(parts, &cp.control, &cp.identity, &args).await?;
            (Some(parts), Some(refresh))
        }
        (other, _) => (other, None),
    };
    if let Some(o) = oidc.as_ref() {
        config = config.with_minter(o.minter.clone());
    }

    // In `--control-plane` mode the audit sink also ships the trail up (D7);
    // `run_audit_shipping` drains this receiver.
    let (audit_tx, audit_rx) = if control_plane.is_some() && !args.policy_observe {
        let (tx, rx) = skimasque_cli::audit_ship::ControlPlaneAuditSink::channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let (service, policy_handle) = build_service(
        &args,
        oidc.as_ref(),
        control_policy_dir.as_deref(),
        audit_tx,
    )?;
    let server = Server::bind(args.listen, server_tls, service, config)?;

    let addr = server.local_addr()?;
    eprintln!("skimasque-server listening on {addr}");
    for warning in startup_warnings(&args) {
        eprintln!("warning: {warning}");
    }

    #[cfg(feature = "acme")]
    if let TlsSetup::Acme(acme) = tls_setup {
        let directory = if acme.is_production() {
            "Let's Encrypt"
        } else {
            "Let's Encrypt staging"
        };
        eprintln!(
            "obtaining a certificate for {} via ACME ({directory}); TLS-ALPN-01 on tcp/{}",
            acme.domains().join(", "),
            acme_challenge_addr(&args),
        );
        tokio::spawn(acme.run());
    }
    if args.connect_tcp {
        eprintln!("serving TCP tunnels via classic CONNECT");
    }
    if let Some(path) = &args.audit_log {
        eprintln!("auditing policy decisions to {}", path.display());
    }

    if let Some(cp) = control_plane {
        let handle = policy_handle
            .clone()
            .expect("--control-plane yields a policy source and so a PolicyHandle");
        eprintln!(
            "syncing policy from {} every {}s (gateway {})",
            cp.url,
            cp.interval.as_secs(),
            cp.identity.gateway_id
        );
        let CpBootstrap {
            control,
            identity,
            interval,
            initial_version,
            cache_age,
            policy_lease,
            cache_ttl,
            ..
        } = cp;
        let state = std::sync::Arc::new(skimasque_cli::control::SyncState::new(
            initial_version,
            cache_age,
            policy_lease,
            cache_ttl,
        ));
        tokio::spawn(run_control_plane_sync(
            control.clone(),
            identity.clone(),
            handle,
            state.clone(),
            interval,
        ));
        if let Some(refresh) = signing_key_refresh {
            eprintln!(
                "re-fetching the org signing key every {}s",
                refresh.interval.as_secs()
            );
            tokio::spawn(run_signing_key_refresh(
                control.clone(),
                identity.clone(),
                refresh.tx,
                refresh.interval,
            ));
        }
        if let Some(audit_rx) = audit_rx {
            eprintln!("shipping the audit trail to the control plane");
            tokio::spawn(skimasque_cli::audit_ship::run_audit_shipping(
                control.clone(),
                identity.clone(),
                audit_rx,
                control.audit_chain_path(),
            ));
        }
        tokio::spawn(run_control_plane_heartbeat(control, identity, state, interval));
    }

    if let Some(interval) = reload_interval {
        let spec = skimasque_cli::policy::SourceSpec::from_args(
            args.policy_dir.as_deref(),
            &args.policy_files,
        )
        .expect("--policy-reload requires a policy source, which clap enforces");
        let handle = policy_handle.expect("a policy source always yields a PolicyHandle");
        eprintln!(
            "watching {} for policy changes every {}s",
            spec.describe(),
            interval.as_secs()
        );
        tokio::spawn(run_policy_reload(spec, handle, interval));
    }

    if let Some(interval) = tls_reload_interval {
        let cert = args
            .cert
            .clone()
            .expect("--tls-reload requires --cert, which clap enforces");
        let key = args
            .key
            .clone()
            .expect("--cert requires --key, which clap enforces");
        eprintln!(
            "watching {} for certificate changes every {}s",
            cert.display(),
            interval.as_secs()
        );
        tokio::spawn(run_tls_reload(cert, key, server.tls_reloader(), interval));
    }

    if let (Some(addr), Some((metrics, ready))) = (args.metrics_listen, ops.as_ref()) {
        eprintln!("ops endpoints on http://{addr} (/healthz, /readyz, /metrics)");
        let (metrics, ready) = (metrics.clone(), ready.clone());
        tokio::spawn(async move {
            if let Err(error) = skimasque_cli::ops::serve(addr, metrics, ready).await {
                tracing::error!(%error, "ops listener stopped");
            }
        });
    }
    // Bound, policy loaded, reloaders running: ready for traffic.
    if let Some((_, ready)) = ops.as_ref() {
        ready.mark_ready();
    }

    let shutdown = async {
        shutdown_signal().await;
        eprintln!(
            "\nshutting down: no new tunnels, draining for up to {}s",
            shutdown_grace.as_secs()
        );
    };
    server.run_until(shutdown, shutdown_grace).await?;
    eprintln!("shutdown complete");
    Ok(())
}

/// Resolve when the process is asked to stop: `Ctrl+C` on any platform, or
/// `SIGTERM` on Unix (what an orchestrator sends on a rollout).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(term) => term,
            Err(error) => {
                tracing::warn!(%error, "could not install SIGTERM handler; Ctrl+C only");
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Where the serving certificate comes from, kept alongside the
/// [`rustls::ServerConfig`] so `main` can act on it after binding.
enum TlsSetup {
    /// A concrete certificate we are serving (from `--cert` or generated). The
    /// string is its PEM, for `--write-cert`.
    Static(String),
    /// An ACME manager that must be spawned to obtain and renew the cert.
    /// Boxed: it is much larger than the `Static` variant.
    #[cfg(feature = "acme")]
    Acme(Box<skimasque::acme::Acme>),
}

#[cfg(feature = "acme")]
fn acme_challenge_addr(args: &Args) -> SocketAddr {
    let port = args.acme_challenge_port.unwrap_or(args.listen.port());
    SocketAddr::new(args.listen.ip(), port)
}

/// The host in `--hostname`, without any `:port` — a certificate is for a name,
/// not an address.
#[cfg(feature = "acme")]
fn hostname_only(hostname: &str) -> &str {
    hostname
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']').map(|(h, _)| h))
        .or_else(|| match hostname.rsplit_once(':') {
            Some((h, port)) if port.parse::<u16>().is_ok() => Some(h),
            _ => None,
        })
        .unwrap_or(hostname)
}

async fn load_tls(args: &Args) -> anyhow::Result<(rustls::ServerConfig, TlsSetup)> {
    #[cfg(feature = "acme")]
    if args.acme {
        let mut domains = vec![hostname_only(&args.hostname).to_owned()];
        for extra in &args.acme_extra_domains {
            if !domains.contains(extra) {
                domains.push(extra.clone());
            }
        }
        let contact = args.acme_email.as_deref().map(|e| {
            if e.contains(':') {
                e.to_owned()
            } else {
                format!("mailto:{e}")
            }
        });
        let acme = skimasque::acme::start(skimasque::acme::AcmeParams {
            domains,
            contact,
            cache_dir: args.acme_cache.clone(),
            production: !args.acme_staging,
            challenge_addr: acme_challenge_addr(args),
        })?;
        let config = acme.server_config();
        return Ok((config, TlsSetup::Acme(Box::new(acme))));
    }

    match (&args.cert, &args.key) {
        (Some(cert), Some(key)) => {
            let cert_pem = tokio::fs::read(cert)
                .await
                .with_context(|| format!("reading {}", cert.display()))?;
            let key_pem = tokio::fs::read(key)
                .await
                .with_context(|| format!("reading {}", key.display()))?;
            let config = tls::server_config_from_pem(&cert_pem, &key_pem)?;
            Ok((config, TlsSetup::Static(String::from_utf8_lossy(&cert_pem).into_owned())))
        }
        _ => {
            let generated = tls::generate_self_signed(args.self_signed_names.clone())?;
            eprintln!(
                "using a generated self-signed certificate for {}",
                args.self_signed_names.join(", ")
            );
            let config = tls::server_config_from_pem(
                generated.certificate_pem.as_bytes(),
                generated.key_pem.as_bytes(),
            )?;
            Ok((config, TlsSetup::Static(generated.certificate_pem)))
        }
    }
}

/// Translate the resource-ceiling flags into a [`ResourceLimits`]. A `0` on any
/// count flag, or `0s` on the idle timeout, removes that ceiling.
fn build_limits(args: &Args) -> anyhow::Result<ResourceLimits> {
    let idle = skimasque_policy::parse_duration(&args.tunnel_idle_timeout)
        .map_err(|e| anyhow::anyhow!("parsing --tunnel-idle-timeout: {e}"))?;
    Ok(ResourceLimits {
        max_connections: (args.max_connections > 0).then_some(args.max_connections),
        connection_rate: (args.max_connection_rate > 0).then_some(ConnectionRate {
            per_second: args.max_connection_rate,
            burst: args.max_connection_burst.max(1),
        }),
        per_source_rate: (args.max_source_connection_rate > 0).then_some(ConnectionRate {
            per_second: args.max_source_connection_rate,
            burst: args.max_source_connection_burst.max(1),
        }),
        exchange_rate: (args.max_exchange_rate > 0).then_some(ConnectionRate {
            per_second: args.max_exchange_rate,
            burst: args.max_exchange_burst.max(1),
        }),
        max_tunnels_per_connection: (args.max_tunnels_per_connection > 0)
            .then_some(args.max_tunnels_per_connection),
        tunnel_idle_timeout: (!idle.is_zero()).then_some(idle),
    })
}

/// Operational warnings to print at startup, in order. Kept separate from
/// `main` so the set is testable.
///
/// The policy-vs-identity one matters for the trust model: with an identity
/// source but no policy, every authenticated client reaches the proxy's
/// "trusting" path -- any address the `AddressPolicy` floor permits, on any
/// allowed port -- with nothing authorizing the destination.
fn startup_warnings(args: &Args) -> Vec<String> {
    let mut warnings = Vec::new();
    let has_identity = args.auth_token.is_some() || args.oidc;
    let has_policy = args.policy_dir.is_some()
        || !args.policy_files.is_empty()
        || args.control_plane.is_some();

    if !has_identity {
        warnings.push(
            "no --auth-token or --oidc set; anyone who can reach this port can use it".to_owned(),
        );
    }
    if has_identity && !has_policy {
        warnings.push(
            "no --policy-dir/--policy-file: authenticated clients may reach any address the \
             network floor permits; add a policy to authorize per destination"
                .to_owned(),
        );
    }
    if args.allow_private {
        warnings
            .push("--allow-private lets clients reach loopback and private networks".to_owned());
    }
    if args.max_connections == 0
        || args.max_tunnels_per_connection == 0
        || args.max_connection_rate == 0
    {
        warnings.push(
            "a connection, rate or tunnel ceiling is disabled; one client can exhaust the gateway"
                .to_owned(),
        );
    }
    #[cfg(feature = "acme")]
    if args.acme && acme_challenge_addr(args).port() != 443 {
        warnings.push(
            "ACME challenge port is not 443; Let's Encrypt validates TLS-ALPN-01 only on 443, so \
             issuance will fail unless something forwards 443 to it"
                .to_owned(),
        );
    }
    warnings
}

fn build_template(args: &Args) -> anyhow::Result<UriTemplate> {
    let template = match (&args.template, &args.authority) {
        (Some(raw), _) => UriTemplate::parse(raw).context("parsing --template")?,
        (None, Some(authority)) => UriTemplate::default_connect_udp(authority)?,
        // `--hostname` (default `gateway.skimasque.com`) is the public name;
        // only the path of the template is used for matching, so this authority
        // is informational.
        (None, None) => UriTemplate::default_connect_udp(&args.hostname)?,
    };
    template
        .require_variables(&CONNECT_UDP_VARIABLES)
        .context("a connect-udp template needs target_host and target_port")?;
    Ok(template)
}

/// Build the proxy service, and -- when a policy source is configured -- the
/// [`PolicyHandle`] that reloads it. The handle is returned rather than kept
/// inside the boxed stack because [`run_policy_reload`] needs it after the
/// service has been consumed into [`Server`].
fn build_service(
    args: &Args,
    oidc: Option<&OidcParts>,
    control_policy_dir: Option<&std::path::Path>,
    audit_tx: Option<tokio::sync::mpsc::Sender<skimasque::audit::AuditEvent>>,
) -> anyhow::Result<(ProxyService, Option<PolicyHandle>)> {
    let mut policy = if args.allow_private {
        AddressPolicy::permissive()
    } else {
        AddressPolicy::default()
    };
    if !args.allow_ports.is_empty() {
        policy = policy.with_allowed_ports(args.allow_ports.iter().copied());
    }
    if !args.allow_cidrs.is_empty() {
        policy = policy.with_allowed_cidrs(args.allow_cidrs.iter().copied());
    }

    // `GlobalConcurrencyLimitLayer` shares one semaphore across every clone of
    // the service, so the cap is a property of the proxy rather than of each
    // connection.
    let limit = GlobalConcurrencyLimitLayer::new(args.max_concurrent_requests);
    let mut dispatch = Dispatch::new().with_udp(UdpProxy::new(policy.clone()));
    if args.connect_tcp {
        dispatch = dispatch.with_tcp(TcpProxy::new(policy));
    }

    let auth = args.auth_token.as_deref().map(AuthorizeLayer::bearer);

    let identity = oidc.map(|o| o.layer.clone());

    let policy_layer = load_policy_layer(args, control_policy_dir, audit_tx)?;
    let policy_handle = policy_layer.as_ref().map(PolicyLayer::handle);
    // The quota layer only does anything with an enforcing policy behind it: it
    // reads the limits off the decision that layer attaches.
    let quota = policy_layer
        .as_ref()
        .filter(|_| !args.policy_observe)
        .map(|_| QuotaLayer::new());

    // Outer to inner: request concurrency cap, workload-identity verification,
    // bearer auth, identity-aware policy, per-policy quota, then the proxy
    // (whose `AddressPolicy` floor still runs after DNS). Identity is outermost
    // so the policy layer can read the `WorkloadIdentity` it leaves behind.
    let service = BoxCloneService::new(
        ServiceBuilder::new()
            .layer(limit)
            .option_layer(identity)
            .option_layer(auth)
            .option_layer(policy_layer)
            .option_layer(quota)
            .service(dispatch),
    );
    Ok((service, policy_handle))
}

/// Poll `fingerprint` every `interval` (and on `SIGHUP`, on Unix). When it
/// changes -- or a `SIGHUP` forces it -- run `reload`.
///
/// A `reload` that returns `Ok` advances the tracked fingerprint and its
/// message is logged; one that returns `Err` is logged and the fingerprint is
/// left alone, so the next tick retries once the files are fixed. `what` names
/// the thing in log lines; `kind` is the `skimasque_reloads_total` label
/// (`policy` or `tls`). Runs until the process exits.
async fn watch_and_reload<F, R>(
    what: &'static str,
    kind: &'static str,
    interval: std::time::Duration,
    mut fingerprint: F,
    mut reload: R,
) where
    F: FnMut() -> skimasque_cli::policy::Fingerprint,
    R: FnMut() -> anyhow::Result<String>,
{
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The first tick fires immediately; the state was just loaded, so skip it.
    ticker.tick().await;

    let mut current = fingerprint();

    #[cfg(unix)]
    let mut sighup = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()) {
        Ok(stream) => Some(stream),
        Err(error) => {
            tracing::warn!(%error, what, "could not install a SIGHUP handler; timed polling only");
            None
        }
    };

    loop {
        #[cfg(unix)]
        let forced = tokio::select! {
            _ = ticker.tick() => false,
            _ = async {
                match sighup.as_mut() {
                    Some(stream) => {
                        stream.recv().await;
                    }
                    None => std::future::pending::<()>().await,
                }
            } => true,
        };
        #[cfg(not(unix))]
        let forced = {
            ticker.tick().await;
            false
        };

        let next = fingerprint();
        if !forced && next == current {
            continue;
        }
        if forced {
            tracing::info!(what, "SIGHUP received; reloading");
        }

        match reload() {
            Ok(detail) => {
                current = next;
                skimasque::metrics::reload(kind, "ok");
                tracing::info!(what, detail, "reloaded");
                eprintln!("{what} reloaded: {detail}");
            }
            Err(error) => {
                skimasque::metrics::reload(kind, "error");
                tracing::warn!(what, error = %format!("{error:#}"), "reload failed; keeping the current one");
                eprintln!("{what} reload failed, keeping the current one: {error:#}");
            }
        }
    }
}

/// Hot-swap the [`PolicySet`](skimasque_policy::PolicySet) when `spec`'s files
/// change. A revision that fails to parse is kept out; the last good set stays
/// in force.
async fn run_policy_reload(
    spec: skimasque_cli::policy::SourceSpec,
    handle: PolicyHandle,
    interval: std::time::Duration,
) {
    let fingerprint_spec = spec.clone();
    watch_and_reload(
        "policy",
        "policy",
        interval,
        move || fingerprint_spec.fingerprint(),
        move || {
            let loaded = spec.load()?;
            let n = loaded.set.policies().len();
            let detail = format!(
                "{n} polic{} from {} file(s)",
                if n == 1 { "y" } else { "ies" },
                loaded.sources.len()
            );
            skimasque_cli::policy::log_lints(&loaded.set);
            handle.store(loaded.set);
            Ok(detail)
        },
    )
    .await
}

/// Present a fresh certificate to new handshakes when `--cert` / `--key` change
/// on disk. A revision that fails to load is kept out; the current certificate
/// stays in use.
async fn run_tls_reload(
    cert: PathBuf,
    key: PathBuf,
    reloader: TlsReloader,
    interval: std::time::Duration,
) {
    let fingerprint_paths = vec![cert.clone(), key.clone()];
    watch_and_reload(
        "TLS certificate",
        "tls",
        interval,
        move || skimasque_cli::policy::Fingerprint::of(fingerprint_paths.clone()),
        move || {
            let cert_pem = std::fs::read(&cert)
                .with_context(|| format!("reading {}", cert.display()))?;
            let key_pem =
                std::fs::read(&key).with_context(|| format!("reading {}", key.display()))?;
            let config = tls::server_config_from_pem(&cert_pem, &key_pem)?;
            reloader.reload(config)?;
            Ok(format!("from {}", cert.display()))
        },
    )
    .await
}

/// What [`bootstrap_control_plane`] hands back: a ready client, the gateway's
/// identity, and the initial policy version (if the initial pull succeeded).
struct CpBootstrap {
    control: skimasque_cli::control::ControlPlane,
    identity: skimasque_cli::control::GatewayIdentity,
    url: String,
    interval: std::time::Duration,
    initial_version: Option<u64>,
    /// How old the cached policy already is at startup (`Some(0)` after a fresh
    /// pull, the persisted age when starting from an unreachable control plane).
    cache_age: Option<std::time::Duration>,
    /// Soft lease: past this age the gateway reports itself degraded.
    policy_lease: std::time::Duration,
    /// Hard TTL: past this age the gateway escalates the staleness alarm.
    cache_ttl: std::time::Duration,
}

/// Parse `--control-plane-label KEY=VALUE` occurrences into a label map.
fn parse_labels(raw: &[String]) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    let mut labels = std::collections::BTreeMap::new();
    for spec in raw {
        let (key, value) = spec
            .split_once('=')
            .with_context(|| format!("--control-plane-label {spec:?} is not KEY=VALUE"))?;
        let (key, value) = (key.trim(), value.trim());
        anyhow::ensure!(
            !key.is_empty() && !value.is_empty(),
            "--control-plane-label {spec:?} has an empty key or value"
        );
        if labels.insert(key.to_owned(), value.to_owned()).is_some() {
            anyhow::bail!("--control-plane-label {key:?} is set more than once");
        }
    }
    Ok(labels)
}

/// Register with (or reconnect to) the control plane and populate the policy
/// cache before the service is built.
async fn bootstrap_control_plane(args: &Args) -> anyhow::Result<Option<CpBootstrap>> {
    use skimasque_cli::control::{ControlPlane, PolicyFetch};

    let Some(url) = args.control_plane.clone() else {
        return Ok(None);
    };
    let state = args
        .control_plane_state
        .clone()
        .expect("--control-plane requires --control-plane-state, which clap enforces");
    let interval = skimasque_policy::parse_duration(&args.control_plane_interval)
        .map_err(|e| anyhow::anyhow!("parsing --control-plane-interval: {e}"))?;
    let policy_lease = skimasque_policy::parse_duration(&args.control_plane_policy_lease)
        .map_err(|e| anyhow::anyhow!("parsing --control-plane-policy-lease: {e}"))?;
    let cache_ttl = skimasque_policy::parse_duration(&args.control_plane_cache_ttl)
        .map_err(|e| anyhow::anyhow!("parsing --control-plane-cache-ttl: {e}"))?;
    if cache_ttl < policy_lease {
        anyhow::bail!(
            "--control-plane-cache-ttl ({}s) must be at least --control-plane-policy-lease ({}s)",
            cache_ttl.as_secs(),
            policy_lease.as_secs()
        );
    }
    let name = args
        .control_plane_name
        .clone()
        .unwrap_or_else(|| args.listen.to_string());
    let labels = parse_labels(&args.control_plane_label)?;

    let control = ControlPlane::new(&url, state)?;

    let identity = match control.load_identity()? {
        Some(identity) => {
            // Re-assert labels only if the operator passed any this run, so a
            // gateway started without the flag never clears an admin-set label.
            if !labels.is_empty() {
                if let Err(error) = control.set_labels(&identity, &labels).await {
                    eprintln!("warning: could not update gateway labels ({error})");
                }
            }
            identity
        }
        None => {
            let token = args.control_plane_token.as_deref().context(
                "the first --control-plane connection needs --control-plane-token \
                 (SKIMASQUE_CONTROL_TOKEN)",
            )?;
            let identity = control
                .register(token, &name, &labels)
                .await
                .context("registering with the control plane")?;
            eprintln!("registered with the control plane as {}", identity.gateway_id);
            identity
        }
    };

    let (initial_version, cache_age) = match control.fetch_policy(&identity, None, None).await {
        Ok(PolicyFetch::Updated { version, .. }) => {
            eprintln!("pulled policy version {version} from the control plane");
            (Some(version), Some(std::time::Duration::ZERO))
        }
        Ok(PolicyFetch::None) => {
            anyhow::bail!("the control plane has no policy for this gateway's organisation yet");
        }
        Ok(PolicyFetch::Unchanged) => (None, Some(std::time::Duration::ZERO)),
        Err(error) if control.has_cached_policy() => {
            let age = control.cached_policy_age();
            match age {
                Some(age) => eprintln!(
                    "warning: control plane unreachable at startup ({error}); \
                     enforcing the cached policy (fetched {}s ago)",
                    age.as_secs()
                ),
                None => eprintln!(
                    "warning: control plane unreachable at startup ({error}); \
                     enforcing the cached policy (age unknown)"
                ),
            }
            (control.load_cache_meta().map(|m| m.version), age)
        }
        Err(error) => return Err(error).context("pulling the initial policy"),
    };

    Ok(Some(CpBootstrap {
        control,
        identity,
        url,
        interval,
        initial_version,
        cache_age,
        policy_lease,
        cache_ttl,
    }))
}

/// Long-poll the control plane for policy changes and swap them into the live
/// [`PolicyHandle`]. On any error the current policy stays in force; the loop
/// backs off and retries.
async fn run_control_plane_sync(
    control: skimasque_cli::control::ControlPlane,
    identity: skimasque_cli::control::GatewayIdentity,
    handle: PolicyHandle,
    state: Arc<skimasque_cli::control::SyncState>,
    interval: std::time::Duration,
) {
    use skimasque_cli::control::PolicyFetch;

    let mut backoff = std::time::Duration::from_secs(1);
    loop {
        match control
            .fetch_policy(&identity, state.version(), Some(interval))
            .await
        {
            Ok(PolicyFetch::Updated { version, documents }) => {
                state.mark_contact();
                let parsed = skimasque_policy::PolicySet::from_documents(
                    documents.iter().map(|d| (d.name.as_str(), d.text.as_str())),
                );
                match parsed {
                    Ok(set) => {
                        skimasque_cli::policy::log_lints(&set);
                        handle.store(set);
                        state.set_version(version);
                        metrics::counter!("skimasque_control_plane_sync_total", "outcome" => "applied")
                            .increment(1);
                        tracing::info!(version, "control plane pushed a new policy");
                    }
                    Err(error) => {
                        metrics::counter!("skimasque_control_plane_sync_total", "outcome" => "rejected")
                            .increment(1);
                        tracing::warn!(
                            %error,
                            version,
                            "control plane sent a policy that does not load; keeping the current one"
                        );
                    }
                }
                backoff = std::time::Duration::from_secs(1);
            }
            Ok(PolicyFetch::Unchanged | PolicyFetch::None) => {
                state.mark_contact();
                backoff = std::time::Duration::from_secs(1);
            }
            Err(error) => {
                state.mark_unreachable();
                metrics::counter!("skimasque_control_plane_sync_total", "outcome" => "error")
                    .increment(1);
                tracing::warn!(
                    %error,
                    "control plane policy poll failed; still enforcing the cached policy"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(std::time::Duration::from_secs(60));
            }
        }
    }
}

/// Send a heartbeat on `interval`, and on the same tick evaluate how stale the
/// cached policy has become. Enforcement is never affected -- a control-plane
/// outage never stops the data plane -- but staleness is surfaced loudly: the
/// heartbeat reports `degraded`, a gauge tracks the policy's age, and crossing
/// the soft lease / hard TTL is logged at `warn` / `error`.
async fn run_control_plane_heartbeat(
    control: skimasque_cli::control::ControlPlane,
    identity: skimasque_cli::control::GatewayIdentity,
    state: Arc<skimasque_cli::control::SyncState>,
    interval: std::time::Duration,
) {
    use skimasque_cli::control::Freshness;

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_freshness = Freshness::Fresh;
    loop {
        ticker.tick().await;

        let age = state.policy_age();
        let freshness = state.freshness();
        metrics::gauge!("skimasque_control_plane_policy_age_seconds").set(age.as_secs_f64());
        metrics::gauge!("skimasque_control_plane_policy_expired")
            .set(if freshness == Freshness::Expired { 1.0 } else { 0.0 });

        if freshness != last_freshness {
            let age_secs = age.as_secs();
            match freshness {
                Freshness::Fresh => tracing::info!(
                    age_secs,
                    "control plane reachable again; the cached policy is fresh"
                ),
                Freshness::Stale => tracing::warn!(
                    age_secs,
                    "policy lease expired and the control plane is unreachable; \
                     still enforcing the cached policy, management is degraded"
                ),
                Freshness::Expired => tracing::error!(
                    age_secs,
                    "policy cache TTL exceeded and the control plane is still unreachable; \
                     still enforcing the last policy (an outage never stops enforcement) \
                     but it may be badly out of date"
                ),
            }
            last_freshness = freshness;
        }

        let healthy = state.healthy() && freshness == Freshness::Fresh;
        let snapshot = skimasque::metrics::usage_snapshot();
        let usage = Some(skimasque_cli::control::UsageReport {
            tunnels_opened: snapshot.tunnels_opened,
            bytes_to_target: snapshot.bytes_to_target,
            bytes_to_client: snapshot.bytes_to_client,
        });
        let outcome = match control
            .heartbeat(&identity, healthy, state.version(), usage)
            .await
        {
            Ok(()) => "ok",
            Err(error) => {
                tracing::debug!(%error, "heartbeat failed");
                "error"
            }
        };
        metrics::counter!("skimasque_control_plane_heartbeat_total", "outcome" => outcome)
            .increment(1);
    }
}

/// The OIDC pieces of a gateway: the token-exchange minter (OIDC in, credential
/// out) and the tunnel-side identity layer (verifies the credential).
struct OidcParts {
    layer: IdentityLayer,
    minter: Arc<dyn CredentialMinter>,
    /// True when the HS256 secret was generated rather than configured.
    generated_secret: bool,
    /// Kept so control-plane mode can re-wire issuance to the control plane's
    /// mint endpoint (D1) while reusing the same OIDC verification and keeping
    /// the local HS256 issuer as the static fallback.
    oidc: Arc<OidcVerifier>,
    credentials: Arc<CredentialIssuer>,
}

/// Resolve `--oidc-provider` and `--oidc-claim` into a [`Provider`].
fn build_provider(args: &Args) -> anyhow::Result<Provider> {
    if args.oidc_provider.eq_ignore_ascii_case("generic") {
        let mut names = ClaimNames::default();
        for spec in &args.oidc_claims {
            let (field, claim) = spec
                .split_once('=')
                .with_context(|| format!("--oidc-claim {spec:?} is not FIELD=CLAIM"))?;
            let slot = match field.trim() {
                "organization" => &mut names.organization,
                "repository" => &mut names.repository,
                "workflow" => &mut names.workflow,
                "ref" => &mut names.git_ref,
                "environment" => &mut names.environment,
                "actor" => &mut names.actor,
                other => anyhow::bail!(
                    "--oidc-claim: unknown field {other:?} (use organization/repository/workflow/ref/environment/actor)"
                ),
            };
            *slot = Some(claim.trim().to_owned());
        }
        anyhow::ensure!(
            names != ClaimNames::default(),
            "--oidc-provider generic needs at least one --oidc-claim"
        );
        return Ok(Provider::Generic(names));
    }
    if !args.oidc_claims.is_empty() {
        anyhow::bail!("--oidc-claim only applies to --oidc-provider generic");
    }
    Provider::from_name(&args.oidc_provider).with_context(|| {
        format!(
            "unknown --oidc-provider {:?} (github, gitlab, buildkite, generic)",
            args.oidc_provider
        )
    })
}

fn build_oidc(args: &Args) -> anyhow::Result<Option<OidcParts>> {
    if !args.oidc {
        return Ok(None);
    }
    if args.oidc_audiences.is_empty() {
        anyhow::bail!(
            "--oidc needs at least one --oidc-audience (the value your pipeline requests)"
        );
    }

    let provider = build_provider(args)?;
    let issuer = match (&args.oidc_issuer, provider.default_issuer()) {
        (Some(url), _) => url.clone(),
        (None, Some(default)) => default.to_owned(),
        (None, None) => {
            anyhow::bail!("--oidc-provider generic needs an explicit --oidc-issuer")
        }
    };

    let ttl = skimasque_policy::parse_duration(&args.credential_ttl)
        .map_err(|e| anyhow::anyhow!("parsing --credential-ttl: {e}"))?;

    let (credentials, generated_secret) = match &args.credential_secret {
        Some(hex) => {
            let secret = skimasque_cli::probe::parse_hex(hex)
                .map_err(|e| anyhow::anyhow!("parsing --credential-secret: {e}"))?;
            anyhow::ensure!(
                secret.len() >= 16,
                "--credential-secret must be at least 16 bytes (32 hex characters)"
            );
            (CredentialIssuer::new(&secret, ttl), false)
        }
        None => (CredentialIssuer::generate(ttl), true),
    };
    let credentials = Arc::new(credentials);

    let verifier = OidcVerifier::hosted_at(provider, &issuer, args.oidc_audiences.clone())
        .context("building the OIDC verifier")?;
    let oidc = Arc::new(verifier);

    let minter: Arc<dyn CredentialMinter> = Arc::new(OidcMinter {
        oidc: oidc.clone(),
        credentials: credentials.clone(),
    });
    let layer = IdentityLayer::new(Arc::new(CredentialIdentity {
        ed: None,
        hs: credentials.clone(),
    }));

    Ok(Some(OidcParts {
        layer,
        minter,
        generated_secret,
        oidc,
        credentials,
    }))
}

/// The short lifetime a locally signed *fallback* credential gets, well under
/// the control-plane-minted default -- the fallback is a brief bridge over a
/// control-plane outage, not a steady state.
const FALLBACK_CREDENTIAL_TTL: std::time::Duration = std::time::Duration::from_secs(300);

/// A running gateway's live view of the org signing key: the tunnel-side
/// verifier reads it on each credential check, [`run_signing_key_refresh`]
/// swaps a rotated key in.
type SigningKeyChannel = tokio::sync::watch::Sender<Arc<CredentialVerifier>>;

/// What [`wire_oidc_to_control_plane`] hands back alongside the wired parts so
/// `main` can start the refresh task.
struct SigningKeyRefresh {
    tx: SigningKeyChannel,
    interval: std::time::Duration,
}

/// Re-wire an OIDC gateway to the control plane (D1): verify tunnel credentials
/// against the org's Ed25519 public key (offline), and mint through the control
/// plane's endpoint instead of signing locally -- keeping the local HS256
/// issuer as a short static fallback unless it was disabled.
async fn wire_oidc_to_control_plane(
    parts: OidcParts,
    control: &skimasque_cli::control::ControlPlane,
    identity: &skimasque_cli::control::GatewayIdentity,
    args: &Args,
) -> anyhow::Result<(OidcParts, SigningKeyRefresh)> {
    let signing_key = match control.fetch_signing_key(&identity.org_id).await {
        Ok(key) => {
            eprintln!("verifying credentials against the org's Ed25519 signing key");
            key
        }
        Err(error) => match control.load_cached_signing_key() {
            Some(key) => {
                eprintln!(
                    "warning: could not refresh the org signing key ({error}); \
                     using the cached one"
                );
                key
            }
            None => {
                return Err(error).context(
                    "the control plane's org signing key is needed to verify credentials \
                     and is not cached",
                )
            }
        },
    };
    let verifier = Arc::new(CredentialVerifier::from_ed_public_keys(
        &signing_key.all_public_key_bytes()?,
    ));
    let (tx, rx) = tokio::sync::watch::channel(verifier);

    let ttl = skimasque_policy::parse_duration(&args.credential_ttl)
        .map_err(|e| anyhow::anyhow!("parsing --credential-ttl: {e}"))?;
    let key_interval = skimasque_policy::parse_duration(&args.control_plane_signing_key_interval)
        .map_err(|e| anyhow::anyhow!("parsing --control-plane-signing-key-interval: {e}"))?;
    let fallback = if args.control_plane_no_credential_fallback {
        None
    } else {
        Some(parts.credentials.clone())
    };
    if fallback.is_none() {
        eprintln!("token exchange will fail during a control-plane outage (no credential fallback)");
    }

    let minter: Arc<dyn CredentialMinter> = Arc::new(RemoteMinter {
        oidc: parts.oidc.clone(),
        control: control.clone(),
        identity: identity.clone(),
        ttl,
        fallback,
    });
    let layer = IdentityLayer::new(Arc::new(CredentialIdentity {
        ed: Some(rx),
        hs: parts.credentials.clone(),
    }));

    Ok((
        OidcParts {
            layer,
            minter,
            ..parts
        },
        SigningKeyRefresh {
            tx,
            interval: key_interval,
        },
    ))
}

/// Re-fetch the org signing key on `interval` and swap a rotated one into the
/// live verifier. A fetch failure keeps the current key and is logged; the org
/// key changes rarely, so this loop is slow.
async fn run_signing_key_refresh(
    control: skimasque_cli::control::ControlPlane,
    identity: skimasque_cli::control::GatewayIdentity,
    tx: SigningKeyChannel,
    interval: std::time::Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await; // the first tick is immediate; we already have the key
    loop {
        ticker.tick().await;
        match control.fetch_signing_key(&identity.org_id).await {
            Ok(key) => match key.all_public_key_bytes() {
                Ok(keys) => {
                    let verifier = Arc::new(CredentialVerifier::from_ed_public_keys(&keys));
                    if tx.send(verifier).is_err() {
                        return; // the verifier was dropped; nothing to update
                    }
                    metrics::counter!("skimasque_control_plane_signing_key_total", "outcome" => "refreshed")
                        .increment(1);
                }
                Err(error) => {
                    tracing::warn!(%error, "the refreshed org signing key did not decode; keeping the current one");
                }
            },
            Err(error) => {
                metrics::counter!("skimasque_control_plane_signing_key_total", "outcome" => "error")
                    .increment(1);
                tracing::debug!(%error, "refreshing the org signing key failed; keeping the current one");
            }
        }
    }
}

/// The token-exchange minter in control-plane mode: verify the CI OIDC token
/// locally, then ask the control plane to mint the credential against the org
/// key. If the control plane cannot be reached and a fallback is configured,
/// sign a short-lived one locally so a brief outage does not stall CI.
#[derive(Debug)]
struct RemoteMinter {
    oidc: Arc<OidcVerifier>,
    control: skimasque_cli::control::ControlPlane,
    identity: skimasque_cli::control::GatewayIdentity,
    ttl: std::time::Duration,
    fallback: Option<Arc<CredentialIssuer>>,
}

impl CredentialMinter for RemoteMinter {
    fn mint(
        &self,
        identity_token: String,
    ) -> Pin<Box<dyn Future<Output = Result<MintedCredential, MintError>> + Send>> {
        let oidc = self.oidc.clone();
        let control = self.control.clone();
        let gateway = self.identity.clone();
        let ttl = self.ttl;
        let fallback = self.fallback.clone();
        Box::pin(async move {
            let claims = oidc.verify_claims(&identity_token).await.map_err(|e| match e {
                skimasque_identity::Error::Discovery(_) | skimasque_identity::Error::Jwks(_) => {
                    MintError::Unavailable(e.to_string())
                }
                other => MintError::Unauthorized(other.to_string()),
            })?;
            let workload = oidc.provider().identify(&claims);

            match control
                .mint_credential(&gateway, &workload, claims.subject(), ttl)
                .await
            {
                Ok(minted) => {
                    metrics::counter!("skimasque_control_plane_mint_total", "outcome" => "control_plane")
                        .increment(1);
                    Ok(MintedCredential {
                        credential: minted.token,
                        expires_in: minted.expires_in,
                    })
                }
                Err(error) => match &fallback {
                    Some(issuer) => {
                        metrics::counter!("skimasque_control_plane_mint_total", "outcome" => "fallback")
                            .increment(1);
                        tracing::warn!(
                            %error,
                            "control plane could not mint a credential; issued a short-lived \
                             one locally"
                        );
                        let issued = issuer
                            .issue_for(&workload, claims.subject(), FALLBACK_CREDENTIAL_TTL)
                            .map_err(|e| MintError::Unavailable(e.to_string()))?;
                        Ok(MintedCredential {
                            credential: issued.token,
                            expires_in: issued.expires_in,
                        })
                    }
                    None => {
                        metrics::counter!("skimasque_control_plane_mint_total", "outcome" => "error")
                            .increment(1);
                        Err(MintError::Unavailable(format!(
                            "the control plane could not mint a credential: {error}"
                        )))
                    }
                },
            }
        })
    }
}

/// The token-exchange minter: verify a CI OIDC token, issue a credential.
#[derive(Debug)]
struct OidcMinter {
    oidc: Arc<OidcVerifier>,
    credentials: Arc<CredentialIssuer>,
}

impl CredentialMinter for OidcMinter {
    fn mint(
        &self,
        identity_token: String,
    ) -> Pin<Box<dyn Future<Output = Result<MintedCredential, MintError>> + Send>> {
        let oidc = self.oidc.clone();
        let credentials = self.credentials.clone();
        Box::pin(async move {
            let claims = oidc
                .verify_claims(&identity_token)
                .await
                .map_err(|e| match e {
                    skimasque_identity::Error::Discovery(_)
                    | skimasque_identity::Error::Jwks(_) => MintError::Unavailable(e.to_string()),
                    other => MintError::Unauthorized(other.to_string()),
                })?;
            let identity = oidc.provider().identify(&claims);
            let issued = credentials
                .issue(&identity, claims.subject())
                .map_err(|e| MintError::Unavailable(e.to_string()))?;
            Ok(MintedCredential {
                credential: issued.token,
                expires_in: issued.expires_in,
            })
        })
    }
}

/// The tunnel-side verifier: a presented credential is checked locally, with no
/// network. In control-plane mode `ed` is a live view of the org's Ed25519
/// signing key(s) -- current plus previous across a rotation -- and verifies
/// control-plane-minted credentials; `hs` is the local HS256 issuer, which
/// verifies both a self-hosted fleet's credentials and the short-lived fallback
/// ones this gateway signs during a control-plane outage.
#[derive(Debug)]
struct CredentialIdentity {
    ed: Option<tokio::sync::watch::Receiver<Arc<CredentialVerifier>>>,
    hs: Arc<CredentialIssuer>,
}

impl IdentityVerifier for CredentialIdentity {
    fn verify(
        &self,
        token: String,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadIdentity, String>> + Send>> {
        let ed = self.ed.as_ref().map(|rx| rx.borrow().clone());
        let hs = self.hs.clone();
        Box::pin(async move {
            if let Some(ed) = ed {
                if let Ok(identity) = ed.verify(&token) {
                    return Ok(identity);
                }
            }
            hs.verify(&token).map_err(|e| e.to_string())
        })
    }
}

fn load_policy_layer(
    args: &Args,
    control_policy_dir: Option<&std::path::Path>,
    audit_tx: Option<tokio::sync::mpsc::Sender<skimasque::audit::AuditEvent>>,
) -> anyhow::Result<Option<PolicyLayer>> {
    let loaded = match (control_policy_dir, &args.policy_dir, args.policy_files.as_slice()) {
        (Some(dir), _, _) => Some(skimasque_cli::policy::load_dir(dir)?),
        (None, Some(dir), _) => Some(skimasque_cli::policy::load_dir(dir)?),
        (None, None, files) if !files.is_empty() => Some(skimasque_cli::policy::load_files(files)?),
        (None, None, _) => None,
    };
    let Some(loaded) = loaded else {
        return Ok(None);
    };

    skimasque_cli::policy::log_lints(&loaded.set);

    if args.policy_observe {
        return Ok(Some(PolicyLayer::new(loaded.set).observe()));
    }

    // An enforcing gateway always keeps an audit trail: to the file if one is
    // named, otherwise to the `masque::audit` tracing target. In control-plane
    // mode it also ships the trail up, without displacing the local sink.
    let mut sink = build_audit_sink(args)?;
    if let Some(tx) = audit_tx {
        sink = skimasque_cli::audit_ship::ControlPlaneAuditSink::wrap(sink, tx);
    }
    Ok(Some(PolicyLayer::new(loaded.set).with_audit(sink)))
}

fn build_audit_sink(args: &Args) -> anyhow::Result<Arc<dyn AuditSink>> {
    match &args.audit_log {
        Some(path) => {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("opening the audit log {}", path.display()))?;
            Ok(Arc::new(JsonlAuditSink::new(file)))
        }
        None => Ok(Arc::new(TracingAuditSink)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_definition_is_consistent() {
        Args::command().debug_assert();
    }

    fn parse(args: &[&str]) -> Args {
        Args::try_parse_from(std::iter::once("skimasque-server").chain(args.iter().copied()))
            .unwrap()
    }

    #[test]
    fn the_default_template_is_the_well_known_one() {
        let args = parse(&["--authority", "proxy.example:4433"]);
        assert_eq!(
            build_template(&args).unwrap().as_str(),
            "https://proxy.example:4433/.well-known/masque/udp/{target_host}/{target_port}/"
        );
    }

    /// A template without the required variables is a configuration error, not
    /// something to discover when the first client is refused.
    #[test]
    fn a_template_missing_target_variables_is_rejected() {
        let args = parse(&["--template", "https://p.example/masque/{target_host}/"]);
        assert!(build_template(&args).is_err());
    }

    #[test]
    fn a_certificate_requires_its_key() {
        assert!(Args::try_parse_from(["skimasque-server", "--cert", "c.pem"]).is_err());
        assert!(Args::try_parse_from(["skimasque-server", "--key", "k.pem"]).is_err());
    }

    #[test]
    fn a_policy_dir_and_explicit_files_are_mutually_exclusive() {
        assert!(Args::try_parse_from([
            "skimasque-server",
            "--policy-dir",
            ".masque/policies",
            "--policy-file",
            "prod.toml",
        ])
        .is_err());
    }

    #[test]
    fn github_oidc_and_a_static_token_are_mutually_exclusive() {
        assert!(Args::try_parse_from([
            "skimasque-server",
            "--github-oidc",
            "--oidc-audience",
            "https://masque.example",
            "--auth-token",
            "s3cret",
        ])
        .is_err());
    }

    #[test]
    fn github_oidc_without_an_audience_is_a_build_error() {
        assert!(
            build_oidc(&parse(&["--github-oidc"])).is_err(),
            "--github-oidc with no --oidc-audience must fail closed"
        );
    }

    #[test]
    fn a_short_credential_secret_is_rejected() {
        let args = parse(&[
            "--github-oidc",
            "--oidc-audience",
            "https://masque.example",
            "--credential-secret",
            "abcd",
        ]);
        assert!(
            build_oidc(&args).is_err(),
            "an 2-byte secret must be refused"
        );
    }

    #[test]
    fn a_bad_credential_ttl_is_a_build_error() {
        let args = parse(&[
            "--github-oidc",
            "--oidc-audience",
            "https://masque.example",
            "--credential-ttl",
            "soon",
        ]);
        assert!(build_oidc(&args).is_err());
    }

    #[test]
    fn the_service_builds_with_a_github_oidc_verifier_and_exchange() {
        let args = parse(&["--github-oidc", "--oidc-audience", "https://masque.example"]);
        let oidc = build_oidc(&args).unwrap();
        assert!(oidc.is_some());
        assert!(build_service(&args, oidc.as_ref(), None, None).is_ok());
    }

    #[test]
    fn the_oidc_provider_selects_the_claim_mapping() {
        use skimasque_identity::Provider;

        assert_eq!(
            build_provider(&parse(&["--oidc", "--oidc-audience", "a"])).unwrap(),
            Provider::GitHubActions,
            "the default provider is github"
        );
        assert_eq!(
            build_provider(&parse(&[
                "--oidc", "--oidc-audience", "a", "--oidc-provider", "gitlab"
            ]))
            .unwrap(),
            Provider::GitLab
        );
        assert!(build_provider(&parse(&[
            "--oidc", "--oidc-audience", "a", "--oidc-provider", "jenkins"
        ]))
        .is_err());

        // generic needs claim mappings and an issuer.
        assert!(build_provider(&parse(&[
            "--oidc", "--oidc-audience", "a", "--oidc-provider", "generic"
        ]))
        .is_err());
        let generic = build_provider(&parse(&[
            "--oidc",
            "--oidc-audience",
            "a",
            "--oidc-provider",
            "generic",
            "--oidc-claim",
            "repository=project_path",
            "--oidc-claim",
            "ref=branch_ref",
        ]))
        .unwrap();
        assert!(matches!(generic, Provider::Generic(_)));
        assert!(
            build_oidc(&parse(&[
                "--oidc",
                "--oidc-audience",
                "a",
                "--oidc-provider",
                "generic",
                "--oidc-claim",
                "repository=project_path",
            ]))
            .is_err(),
            "generic without --oidc-issuer must fail"
        );
    }

    /// A fresh Ed25519 keypair as `(pkcs8_der, raw_public_key)` -- the same
    /// encoding the control plane's per-org signing keys use (`ring`'s
    /// `Ed25519KeyPair::generate_pkcs8`), so a credential signed here verifies
    /// exactly as a control-plane-minted one would.
    fn ed25519_keypair() -> (Vec<u8>, Vec<u8>) {
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        (pkcs8.as_ref().to_vec(), pair.public_key().as_ref().to_vec())
    }

    #[test]
    fn the_credential_verifier_accepts_both_signing_schemes_and_picks_up_a_rotation() {
        use skimasque_identity::CredentialSigner;
        use skimasque_policy::WorkloadIdentity;

        let ttl = std::time::Duration::from_secs(300);
        let workload = WorkloadIdentity {
            repository: Some("acme/widget".to_owned()),
            ..Default::default()
        };

        // The local HS256 issuer, and an Ed25519 org key like the control
        // plane's.
        let hs = Arc::new(CredentialIssuer::generate(ttl));
        let (der1, pub1) = ed25519_keypair();
        let signer1 = CredentialSigner::from_pkcs8_der(&der1, ttl);
        let sign1 = || signer1.issue(&workload, None).unwrap().token;

        let ed = Arc::new(CredentialVerifier::from_ed_public_key(&pub1));
        let (tx, rx) = tokio::sync::watch::channel(ed);

        let verifier = CredentialIdentity {
            ed: Some(rx),
            hs: hs.clone(),
        };

        // A control-plane (Ed25519) credential verifies; so does an HS256 one.
        assert_eq!(
            tokio_test_block_on(verifier.verify(sign1())).unwrap(),
            workload
        );
        assert_eq!(
            tokio_test_block_on(verifier.verify(hs.issue(&workload, None).unwrap().token)).unwrap(),
            workload
        );
        assert!(tokio_test_block_on(verifier.verify("nonsense".to_owned())).is_err());

        // Rotate: push a verifier holding the new key plus the old one. Both a
        // pre-rotation and a post-rotation credential verify.
        let (der2, pub2) = ed25519_keypair();
        let signer2 = CredentialSigner::from_pkcs8_der(&der2, ttl);
        tx.send(Arc::new(CredentialVerifier::from_ed_public_keys(&[
            pub2.clone(),
            pub1.clone(),
        ])))
        .unwrap();
        assert_eq!(
            tokio_test_block_on(verifier.verify(sign1())).unwrap(),
            workload
        );
        assert_eq!(
            tokio_test_block_on(verifier.verify(signer2.issue(&workload, None).unwrap().token))
                .unwrap(),
            workload
        );
    }

    fn tokio_test_block_on<F: Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn control_plane_labels_parse_key_value_pairs() {
        let ok = parse_labels(&["env=prod".into(), " region = eu ".into()]).unwrap();
        assert_eq!(ok.get("env").map(String::as_str), Some("prod"));
        assert_eq!(ok.get("region").map(String::as_str), Some("eu"));

        assert!(parse_labels(&["noequals".into()]).is_err());
        assert!(parse_labels(&["=value".into()]).is_err());
        assert!(parse_labels(&["key=".into()]).is_err());
        assert!(parse_labels(&["env=a".into(), "env=b".into()]).is_err());

        // The flag is only meaningful with --control-plane.
        assert!(Args::try_parse_from(["skimasque-server", "--control-plane-label", "env=prod"])
            .is_err());
    }

    #[test]
    fn the_signing_key_refresh_interval_defaults_to_an_hour() {
        let args = parse(&["--control-plane", "https://cp.example", "--control-plane-state", "/s"]);
        assert_eq!(
            skimasque_policy::parse_duration(&args.control_plane_signing_key_interval).unwrap(),
            std::time::Duration::from_secs(3600)
        );
    }

    #[test]
    fn the_credential_fallback_flag_needs_control_plane() {
        assert!(Args::try_parse_from([
            "skimasque-server",
            "--control-plane-no-credential-fallback",
        ])
        .is_err());
        assert!(Args::try_parse_from([
            "skimasque-server",
            "--control-plane",
            "https://cp.example",
            "--control-plane-state",
            "/s",
            "--control-plane-no-credential-fallback",
        ])
        .is_ok());
    }

    #[test]
    fn a_gitlab_issuer_defaults_and_the_service_builds() {
        let args = parse(&[
            "--oidc",
            "--oidc-provider",
            "gitlab",
            "--oidc-audience",
            "https://masque.example",
        ]);
        assert!(build_oidc(&args).unwrap().is_some());
    }

    #[test]
    fn startup_warns_when_identity_is_set_but_no_policy_is() {
        // Nothing configured: the open-port warning, not the policy one.
        let open = startup_warnings(&parse(&[]));
        assert!(open.iter().any(|w| w.contains("anyone who can reach this port")));
        assert!(!open.iter().any(|w| w.contains("authorize per destination")));

        // Identity but no policy: the trust-model gap.
        let gap = startup_warnings(&parse(&["--auth-token", "s3cret"]));
        assert!(
            gap.iter().any(|w| w.contains("authorize per destination")),
            "{gap:?}"
        );

        // Identity and a policy: neither warning.
        let dir = std::env::temp_dir().join("skimasque-warn-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.toml"), "name = \"any\"\n").unwrap();
        let ok = startup_warnings(&parse(&[
            "--auth-token",
            "s3cret",
            "--policy-dir",
            dir.to_str().unwrap(),
        ]));
        assert!(!ok.iter().any(|w| w.contains("authorize per destination")), "{ok:?}");
        assert!(!ok.iter().any(|w| w.contains("anyone who can reach this port")), "{ok:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_service_builds_with_no_policy_and_with_one() {
        let (_, handle) = build_service(&parse(&[]), None, None, None).unwrap();
        assert!(handle.is_none(), "no policy source means no reload handle");

        let dir = std::env::temp_dir().join("skimasque-server-policy-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.toml"), "name = \"any\"\n").unwrap();
        let args = parse(&["--policy-dir", dir.to_str().unwrap()]);
        let (_, handle) = build_service(&args, None, None, None).unwrap();
        assert!(handle.is_some(), "a policy source yields a reload handle");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_metrics_listener_is_off_unless_an_address_is_given() {
        assert!(parse(&[]).metrics_listen.is_none());
        assert_eq!(
            parse(&["--metrics-listen", "127.0.0.1:9100"])
                .metrics_listen
                .unwrap()
                .to_string(),
            "127.0.0.1:9100"
        );
        assert!(
            Args::try_parse_from(["skimasque-server", "--metrics-listen", "not-an-addr"]).is_err()
        );
    }

    #[test]
    fn hostname_defaults_to_the_public_gateway() {
        assert_eq!(
            Args::try_parse_from(["skimasque-server"]).unwrap().hostname,
            "gateway.skimasque.com"
        );
    }

    #[cfg(feature = "acme")]
    #[test]
    fn acme_is_a_flag_over_hostname_and_conflicts_with_a_static_cert() {
        // `--acme` alone: a cert for `--hostname`.
        let args = Args::try_parse_from(["skimasque-server", "--acme"]).unwrap();
        assert!(args.acme);
        assert_eq!(args.hostname, "gateway.skimasque.com");
        assert_eq!(hostname_only("gateway.skimasque.com:8443"), "gateway.skimasque.com");
        assert_eq!(hostname_only("[2001:db8::1]:443"), "2001:db8::1");
        assert!(args.acme_email.is_none() && !args.acme_staging);

        // --acme and --cert are mutually exclusive.
        assert!(Args::try_parse_from([
            "skimasque-server", "--acme", "--cert", "c.pem", "--key", "k.pem",
        ])
        .is_err());
        // The acme sub-flags require --acme.
        assert!(
            Args::try_parse_from(["skimasque-server", "--acme-email", "ops@example.com"]).is_err()
        );
        assert!(Args::try_parse_from(["skimasque-server", "--acme-staging"]).is_err());
        assert!(
            Args::try_parse_from(["skimasque-server", "--acme-extra-domain", "alt.example"]).is_err()
        );

        // The challenge address defaults to the --listen port.
        let args =
            Args::try_parse_from(["skimasque-server", "--listen", "0.0.0.0:443", "--acme"]).unwrap();
        assert_eq!(acme_challenge_addr(&args).port(), 443);
        let args =
            Args::try_parse_from(["skimasque-server", "--acme", "--acme-challenge-port", "8443"])
                .unwrap();
        assert_eq!(acme_challenge_addr(&args).port(), 8443);
    }

    #[test]
    fn policy_reload_requires_a_policy_source() {
        assert!(
            Args::try_parse_from(["skimasque-server", "--policy-reload"]).is_err(),
            "--policy-reload alone is meaningless"
        );
        assert!(
            Args::try_parse_from([
                "skimasque-server",
                "--policy-reload-interval",
                "10s",
                "--policy-dir",
                ".masque/policies",
            ])
            .is_err(),
            "--policy-reload-interval without --policy-reload is rejected"
        );
        assert!(Args::try_parse_from([
            "skimasque-server",
            "--policy-dir",
            ".masque/policies",
            "--policy-reload",
            "--policy-reload-interval",
            "10s",
        ])
        .is_ok());
    }

    #[test]
    fn control_plane_is_its_own_policy_source() {
        // --control-plane needs a state directory.
        assert!(
            Args::try_parse_from(["skimasque-server", "--control-plane", "https://cp.example"])
                .is_err()
        );
        // ...and is mutually exclusive with the file sources and file reload.
        assert!(Args::try_parse_from([
            "skimasque-server",
            "--control-plane",
            "https://cp.example",
            "--control-plane-state",
            "/var/lib/skm",
            "--policy-dir",
            ".masque/policies",
        ])
        .is_err());
        assert!(Args::try_parse_from([
            "skimasque-server",
            "--control-plane",
            "https://cp.example",
            "--control-plane-state",
            "/var/lib/skm",
            "--policy-reload",
        ])
        .is_err());
        // The minimal valid form, and it satisfies --policy-observe's requirement.
        assert!(Args::try_parse_from([
            "skimasque-server",
            "--control-plane",
            "https://cp.example",
            "--control-plane-state",
            "/var/lib/skm",
            "--policy-observe",
        ])
        .is_ok());
    }

    #[test]
    fn control_plane_lease_and_ttl_default_to_the_roadmap_values() {
        let args = parse(&["--control-plane", "https://cp.example", "--control-plane-state", "/s"]);
        let lease = skimasque_policy::parse_duration(&args.control_plane_policy_lease).unwrap();
        let ttl = skimasque_policy::parse_duration(&args.control_plane_cache_ttl).unwrap();
        assert_eq!(lease, std::time::Duration::from_secs(15 * 60));
        assert_eq!(ttl, std::time::Duration::from_secs(30 * 60));
        assert!(ttl >= lease, "the hard TTL must not precede the soft lease");
    }

    #[test]
    fn tls_reload_requires_a_certificate_and_key() {
        assert!(
            Args::try_parse_from(["skimasque-server", "--tls-reload"]).is_err(),
            "--tls-reload needs a --cert to reload"
        );
        assert!(
            Args::try_parse_from(["skimasque-server", "--tls-reload-interval", "10s"]).is_err(),
            "--tls-reload-interval without --tls-reload is rejected"
        );
        assert!(Args::try_parse_from([
            "skimasque-server",
            "--cert",
            "c.pem",
            "--key",
            "k.pem",
            "--tls-reload",
            "--tls-reload-interval",
            "10s",
        ])
        .is_ok());
    }

    #[test]
    fn the_resource_limits_default_to_bounded_and_zero_removes_a_ceiling() {
        let limits = build_limits(&parse(&[])).unwrap();
        assert_eq!(limits.max_connections, Some(1024));
        assert_eq!(limits.max_tunnels_per_connection, Some(256));
        assert_eq!(
            limits.tunnel_idle_timeout,
            Some(std::time::Duration::from_secs(120))
        );
        let rate = limits.connection_rate.expect("a default connection rate");
        assert_eq!((rate.per_second, rate.burst), (50, 200));
        let per_source = limits.per_source_rate.expect("a default per-source rate");
        assert_eq!((per_source.per_second, per_source.burst), (20, 60));
        let exchange = limits.exchange_rate.expect("a default exchange rate");
        assert_eq!((exchange.per_second, exchange.burst), (10, 30));

        let limits = build_limits(&parse(&[
            "--max-connections",
            "0",
            "--max-connection-rate",
            "0",
            "--max-source-connection-rate",
            "0",
            "--max-exchange-rate",
            "0",
            "--max-tunnels-per-connection",
            "0",
            "--tunnel-idle-timeout",
            "0s",
        ]))
        .unwrap();
        assert!(limits.max_connections.is_none());
        assert!(limits.connection_rate.is_none());
        assert!(limits.per_source_rate.is_none());
        assert!(limits.exchange_rate.is_none());
        assert!(limits.max_tunnels_per_connection.is_none());
        assert!(limits.tunnel_idle_timeout.is_none());
    }

    #[test]
    fn a_bad_tunnel_idle_timeout_is_a_build_error() {
        assert!(build_limits(&parse(&["--tunnel-idle-timeout", "soon"])).is_err());
    }
}
