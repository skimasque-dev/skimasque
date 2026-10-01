//! `skimasque exec`: run a command with policy-scoped network access that ends
//! when the command does.
//!
//! One gateway session; on loopback, an HTTP CONNECT front end, the SOCKS5
//! relay and any `--forward` listeners; proxy variables in the child's
//! environment. When the child exits, the listeners and the session go with it.
//! This module holds the arguments and the decisions; [`run`] wires them up.

use std::ffi::{OsStr, OsString};
use std::net::SocketAddr;
use std::path::Path;

use clap::builder::NonEmptyStringValueParser;
use skimasque_policy::WorkloadIdentity;

use crate::account::{Credentials, SimulateResult};
use crate::forward::ForwardSpec;
use crate::session::{AuthArgs, TlsArgs};

pub const CLOUD_CONTROL_PLANE: &str = "https://control.skimasque.com";
pub const CLOUD_GATEWAY: &str = "gateway.skimasque.com";
/// Simulated when there are no forwards: denied, but it names the policy the
/// identity selects.
pub const PREFLIGHT_PLACEHOLDER: &str = "preflight.invalid:1";

pub const EXIT_FAILED: i32 = 125;
pub const EXIT_NOT_EXECUTABLE: i32 = 126;
pub const EXIT_NOT_FOUND: i32 = 127;

/// `skimasque exec [OPTIONS] -- COMMAND [ARGS...]`.
#[derive(Debug, clap::Args)]
pub struct ExecArgs {
    /// Only run if this is the policy selected for your identity. Sent to the
    /// gateway as `X-Masque-Policy`, which denies every tunnel otherwise.
    #[arg(long, value_name = "NAME", value_parser = NonEmptyStringValueParser::new())]
    pub policy: Option<String>,

    /// The application to declare (`X-Masque-Application`). Defaults to the
    /// command's file name, e.g. `terraform`.
    #[arg(long, value_name = "NAME", value_parser = NonEmptyStringValueParser::new())]
    pub app: Option<String>,

    /// The gateway, as `host[:port]`. Defaults to gateway.skimasque.com when the
    /// control plane is SkiMasque Cloud; required for a self-hosted one.
    #[arg(
        long,
        env = "SKIMASQUE_GATEWAY",
        value_name = "HOST[:PORT]",
        value_parser = NonEmptyStringValueParser::new()
    )]
    pub gateway: Option<String>,

    /// The control plane. Defaults to the one you signed in to, else SkiMasque
    /// Cloud.
    #[arg(
        long,
        env = "SKIMASQUE_CONTROL_PLANE",
        value_name = "URL",
        value_parser = NonEmptyStringValueParser::new()
    )]
    pub control_plane: Option<String>,

    /// Listen on a loopback port and tunnel each connection to HOST:PORT, for
    /// tools that ignore proxy settings. Repeatable. Its address is also in
    /// `$SKIMASQUE_FORWARD_<HOST>_<PORT>`.
    #[arg(long = "forward", value_name = "[LOCAL_PORT:]HOST:PORT", value_parser = ForwardSpec::parse)]
    pub forwards: Vec<ForwardSpec>,

    /// Run the command as a coding agent: start an agent session for it (your
    /// identity plus `kind = agent`), give the command only that session's
    /// access, and end the session when the command exits. Needs
    /// `skimasque login`. The credential stays in this process; the command
    /// never sees it.
    #[arg(long)]
    pub agent: bool,

    /// How long the agent session lasts, e.g. `45m`. Default 30m, at most 4h.
    #[arg(long, requires = "agent", value_name = "DURATION")]
    pub ttl: Option<String>,

    /// Label the session with the agent runtime, for the audit log. Defaults to
    /// the command's file name. Never matched by policy.
    #[arg(long, requires = "agent", value_name = "NAME", value_parser = NonEmptyStringValueParser::new())]
    pub runtime: Option<String>,

    /// Label the session with a run id, for the audit log. Never matched by policy.
    #[arg(long, requires = "agent", value_name = "ID", value_parser = NonEmptyStringValueParser::new())]
    pub run_id: Option<String>,

    /// Delegate from this agent session: the new one never outlives it.
    #[arg(long, requires = "agent", value_name = "SESSION", value_parser = NonEmptyStringValueParser::new())]
    pub parent: Option<String>,

    /// Confine the command so SkiMasque is its only way out. `srt` is
    /// Anthropic's sandbox runtime (`npm install -g @anthropic-ai/sandbox-runtime`),
    /// which must be on PATH. exec checks that the sandbox really blocks a
    /// direct connection before it starts the command, and refuses to if not.
    #[arg(long, value_enum, value_name = "RUNTIME")]
    pub sandbox: Option<SandboxRuntime>,

    /// A domain the sandbox may let through to SkiMasque, e.g. `*.acme.dev` or
    /// `api.acme.dev:443`. Repeatable; required with `--sandbox`, because the
    /// sandbox runtime accepts no bare `*`. The gateway's policy still decides
    /// every connection; this is an outer fence.
    #[arg(
        long = "allow-domain",
        requires = "sandbox",
        value_name = "DOMAIN",
        value_parser = crate::sandbox::check_allow_domain
    )]
    pub allow_domains: Vec<String>,

    /// Your own sandbox-runtime settings (filesystem rules and so on), merged
    /// with the network settings exec generates. Its `network.allowedDomains`
    /// and `network.parentProxy` are replaced.
    #[arg(long, requires = "sandbox", value_name = "PATH")]
    pub sandbox_settings: Option<std::path::PathBuf>,

    /// Run an agent with no sandbox. Nothing then stops it connecting directly,
    /// bypassing SkiMasque; use only when something else confines it.
    #[arg(long, requires = "agent", conflicts_with = "sandbox")]
    pub unsandboxed: bool,

    /// Do not print the access summary.
    #[arg(long, short)]
    pub quiet: bool,

    #[command(flatten)]
    pub tls: TlsArgs,

    #[command(flatten)]
    pub auth: AuthArgs,

    /// The command to run, after `--`.
    #[arg(last = true, required = true, value_name = "COMMAND")]
    pub command: Vec<OsString>,
}

/// The sandbox runtimes exec can confine a command with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SandboxRuntime {
    /// Anthropic's sandbox runtime.
    Srt,
}

/// Combinations that make no sense or would run an agent unconfined by
/// accident. Checked before anything is started.
pub fn validate(args: &ExecArgs) -> Result<(), String> {
    if args.agent && args.sandbox.is_none() && !args.unsandboxed {
        return Err(
            "an agent needs a sandbox: pass --sandbox srt (with --allow-domain), or \
             --unsandboxed to run it with nothing stopping direct connections"
                .to_owned(),
        );
    }
    if args.sandbox.is_some() {
        if args.allow_domains.is_empty() {
            return Err(
                "--sandbox needs at least one --allow-domain: the sandbox runtime allows no \
                 network access until domains are named, and it will not accept \"*\""
                    .to_owned(),
            );
        }
        if !args.forwards.is_empty() {
            return Err(
                "--forward cannot be combined with --sandbox: the sandboxed command cannot \
                 reach loopback listeners outside the sandbox"
                    .to_owned(),
            );
        }
    }
    Ok(())
}

/// `--control-plane` / `$SKIMASQUE_CONTROL_PLANE`, else the signed-in
/// session's, else SkiMasque Cloud — normalised like every other base URL.
pub fn resolve_control_plane(explicit: Option<&str>, login: Option<&Credentials>) -> String {
    crate::normalize_base_url(
        explicit
            .map(str::to_owned)
            .or_else(|| login.map(|c| c.control_plane.clone()))
            .as_deref()
            .unwrap_or(CLOUD_CONTROL_PLANE),
    )
}

/// The gateway to use, or the message explaining that one is needed.
pub fn resolve_gateway(explicit: Option<&str>, control_plane: &str) -> Result<String, String> {
    match explicit {
        Some(gateway) => Ok(gateway.to_owned()),
        None if control_plane == CLOUD_CONTROL_PLANE => Ok(CLOUD_GATEWAY.to_owned()),
        None => Err(format!(
            "No gateway is configured for {control_plane}. Pass --gateway or set SKIMASQUE_GATEWAY."
        )),
    }
}

/// The command's file stem: `terraform` for `terraform`, `psql` for
/// `/usr/bin/psql.exe`.
pub fn default_app(command: &OsStr) -> String {
    Path::new(command)
        .file_stem()
        .unwrap_or(command)
        .to_string_lossy()
        .into_owned()
}

/// The variables added to the child's environment.
pub fn child_env(
    http: SocketAddr,
    socks: SocketAddr,
    forwards: &[(ForwardSpec, SocketAddr)],
) -> Vec<(String, String)> {
    let http_url = format!("http://{http}");
    let socks_url = format!("socks5h://{socks}");
    let mut env = Vec::new();
    for name in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
        env.push((name.to_owned(), http_url.clone()));
    }
    for name in ["ALL_PROXY", "all_proxy"] {
        env.push((name.to_owned(), socks_url.clone()));
    }
    // Replaced, not merged: under exec every non-loopback destination goes
    // through the tunnel, and forward listeners must not be proxied.
    for name in ["NO_PROXY", "no_proxy"] {
        env.push((name.to_owned(), "localhost,127.0.0.1,::1".to_owned()));
    }
    env.push(("SKIMASQUE_PROXY_HTTP".to_owned(), http.to_string()));
    env.push(("SKIMASQUE_PROXY_SOCKS5".to_owned(), socks.to_string()));
    for (spec, local) in forwards {
        env.push((spec.env_name(), local.to_string()));
    }
    env
}

/// Who the credential says you are: the actor, else the repository, else the
/// organisation, else `unknown`.
pub fn identity_label(identity: Option<&WorkloadIdentity>) -> String {
    identity
        .and_then(|id| {
            id.actor
                .clone()
                .or_else(|| id.repository.clone())
                .or_else(|| id.organization.clone())
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

/// What the preflight established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// The policy the control plane selected for the identity.
    pub selected: Option<String>,
    /// Per forward, in order: whether it would be allowed. Empty with no forwards.
    pub allowed: Vec<bool>,
    /// The first allowed probe's `duration` limit.
    pub session: Option<String>,
}

/// Decide from the preflight simulations whether to run. `probes` is one per
/// forward, in order — or the single [`PREFLIGHT_PLACEHOLDER`] probe when
/// `forwards` is 0.
pub fn judge(
    pin: Option<&str>,
    who: &str,
    probes: &[(String, SimulateResult)],
    forwards: usize,
) -> Result<Checked, String> {
    let selected = probes.first().and_then(|(_, r)| r.policy.clone());
    if let Some(pin) = pin {
        if selected.as_deref() != Some(pin) {
            let chose = match &selected {
                Some(s) => format!("\"{s}\""),
                None => "no policy".to_owned(),
            };
            return Err(format!(
                "Policy \"{pin}\" does not apply to you ({who}). SkiMasque selected {chose} for this identity."
            ));
        }
    }
    if forwards == 0 {
        return Ok(Checked {
            selected,
            allowed: Vec::new(),
            session: None,
        });
    }
    let allowed: Vec<bool> = probes.iter().map(|(_, r)| r.outcome == "allow").collect();
    if !allowed.iter().any(|a| *a) {
        let mut message = "Every destination would be denied:".to_owned();
        for (destination, r) in probes {
            message.push_str(&format!("\n  {destination}: denied."));
            if let Some(rule) = &r.suggested_rule {
                message.push_str(&format!(" To allow it: {rule}"));
            }
        }
        return Err(message);
    }
    let session = probes
        .iter()
        .find(|(_, r)| r.outcome == "allow")
        .and_then(|(_, r)| r.limits.iter().find(|l| l.label == "duration"))
        .map(|l| l.value.clone());
    Ok(Checked {
        selected,
        allowed,
        session,
    })
}

/// One `Access` line of the header.
#[derive(Debug, Clone)]
pub struct AccessLine {
    pub destination: String,
    /// `Some` only when the preflight ran.
    pub allowed: Option<bool>,
    pub local: SocketAddr,
}

/// Everything the header shows.
#[derive(Debug, Clone)]
pub struct HeaderView {
    pub identity: String,
    pub policy: String,
    pub application: String,
    pub gateway: String,
    pub access: Vec<AccessLine>,
    pub session: Option<String>,
}

/// The access summary printed to stderr before the command starts.
pub fn render_header(view: &HeaderView) -> String {
    let mut out = String::from("SkiMasque\n\n");
    for (label, value) in [
        ("Identity", &view.identity),
        ("Policy", &view.policy),
        ("Application", &view.application),
        ("Gateway", &view.gateway),
    ] {
        out.push_str(&format!("{label:<13}{value}\n"));
    }
    if !view.access.is_empty() {
        out.push_str("\nAccess\n");
        for line in &view.access {
            match line.allowed {
                Some(ok) => out.push_str(&format!(
                    "  {:<16} {}  → {}\n",
                    line.destination,
                    if ok { "✓" } else { "✗" },
                    line.local
                )),
                None => out.push_str(&format!("  {:<16} → {}\n", line.destination, line.local)),
            }
        }
    }
    if let Some(session) = &view.session {
        out.push_str(&format!("\nSession\n  {session}\n"));
    }
    out.push_str("\nConnected.\n");
    out
}

/// Find a bare command name on `path` the way cmd.exe does: in each `PATH`
/// directory in order, try the name with each `PATHEXT` extension in order
/// (default `.COM;.EXE;.BAT;.CMD`). Rust's own lookup on Windows only tries
/// `.exe`, so `npm` (really `npm.cmd`) would not be found. `None` when the
/// name already has a directory or an extension, or nothing matches; the
/// caller then spawns the name as given.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn resolve_program(
    program: &OsStr,
    path: Option<&OsStr>,
    pathext: Option<&OsStr>,
    is_file: impl Fn(&Path) -> bool,
) -> Option<std::path::PathBuf> {
    let bare = Path::new(program);
    if bare.components().count() != 1 || bare.extension().is_some() {
        return None;
    }
    let pathext = pathext
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned());
    let extensions: Vec<&str> = pathext.split(';').filter(|e| !e.is_empty()).collect();
    std::env::split_paths(path?)
        .filter(|dir| !dir.as_os_str().is_empty())
        .flat_map(|dir| {
            extensions.iter().map(move |ext| {
                let mut name = program.to_owned();
                name.push(ext);
                dir.join(name)
            })
        })
        .find(|candidate| is_file(candidate))
}

/// The exit status for a command that could not be started.
pub fn spawn_error_code(error: &std::io::Error) -> i32 {
    match error.kind() {
        std::io::ErrorKind::NotFound => EXIT_NOT_FOUND,
        _ => EXIT_NOT_EXECUTABLE,
    }
}

/// The child's exit status as exec's own: its code, or `128 + signal`.
pub fn status_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    EXIT_FAILED
}

/// Run exec to completion and return the process exit status: the command's
/// own (see [`status_code`]); [`EXIT_NOT_FOUND`] / [`EXIT_NOT_EXECUTABLE`] when
/// it cannot be started; [`EXIT_FAILED`] when exec fails before starting it
/// (configuration, signal registration, authentication, session, preflight,
/// loopback listeners) or cannot wait for it; `128 + signal` when SIGINT,
/// SIGTERM or SIGHUP (Ctrl-C on Windows: 130) arrives before the command
/// starts. Every failure is printed to stderr.
pub async fn run(args: ExecArgs) -> i32 {
    match run_inner(args).await {
        Ok(code) => code,
        Err(Failure(code, message)) => {
            eprintln!("skimasque exec: {message}");
            code
        }
    }
}

struct Failure(i32, String);

fn failed(message: impl std::fmt::Display) -> Failure {
    Failure(EXIT_FAILED, message.to_string())
}

async fn run_inner(args: ExecArgs) -> Result<i32, Failure> {
    validate(&args).map_err(failed)?;
    // Registered before anything else, so no signal ends exec by its default
    // action while the session is half built; one that arrives during setup
    // ends it here instead, before the command starts.
    let mut signals =
        Signals::register().map_err(|e| failed(format!("listening for signals: {e}")))?;
    let interrupted = |signal: i32| {
        Failure(128 + signal, "interrupted before the command started".to_owned())
    };
    // The agent session is started here, outside `launch`, so that every way
    // out of it -- success, failure, a signal during setup -- ends the session.
    let lease = if args.agent {
        Some(tokio::select! {
            lease = AgentLease::start(&args) => lease?,
            signal = signals.next() => return Err(interrupted(signal)),
        })
    } else {
        None
    };
    let outcome = launch(&args, &mut signals, lease.as_ref()).await;
    if let Some(lease) = &lease {
        lease.end().await;
    }
    outcome
}

/// An agent session this process started, and ends.
struct AgentLease {
    api: crate::account::Api,
    login_token: String,
    org: String,
    session_id: String,
    /// The session's credential. Held here and handed to the gateway client;
    /// never written anywhere or put in the command's environment.
    credential: String,
}

impl AgentLease {
    async fn start(args: &ExecArgs) -> Result<Self, Failure> {
        use crate::account;
        let creds = account::load()
            .map_err(|e| failed(format!("{e:#}")))?
            .ok_or_else(|| {
                failed("--agent needs a SkiMasque login to start the session: run `skimasque login`")
            })?;
        let api = account::Api::new(&creds.control_plane).map_err(|e| failed(format!("{e:#}")))?;
        let org = account::resolve_org(&api, &creds, args.auth.org.clone())
            .await
            .map_err(|e| failed(format!("{e:#}")))?;
        let ttl_seconds = args
            .ttl
            .as_deref()
            .map(|t| {
                skimasque_policy::parse_duration(t)
                    .map(|d| d.as_secs())
                    .map_err(|e| failed(format!("--ttl: {e}")))
            })
            .transpose()?;
        let request = skimasque_protocol::AgentSessionRequest {
            ttl_seconds,
            run_id: args.run_id.clone(),
            runtime: Some(
                args.runtime
                    .clone()
                    .unwrap_or_else(|| default_app(&args.command[0])),
            ),
            parent: args.parent.clone(),
        };
        let started = api
            .start_agent_session(&creds.session_token, &org, &request)
            .await
            .map_err(|e| failed(format!("starting the agent session: {e:#}")))?;
        Ok(Self {
            api,
            login_token: creds.session_token,
            org,
            session_id: started.session_id,
            credential: started.credential,
        })
    }

    async fn end(&self) {
        if let Err(error) = self
            .api
            .end_agent_session(&self.login_token, &self.org, &self.session_id)
            .await
        {
            eprintln!(
                "skimasque exec: could not end agent session {}: {error:#}. End it with: \
                 skimasque agent-session end {}",
                self.session_id, self.session_id
            );
        }
    }
}

async fn launch(
    args: &ExecArgs,
    signals: &mut Signals,
    lease: Option<&AgentLease>,
) -> Result<i32, Failure> {
    let Prepared {
        session,
        tasks,
        env,
        http_addr,
    } = tokio::select! {
        prepared = prepare(args, lease.map(|l| l.credential.as_str())) => prepared?,
        signal = signals.next() => {
            return Err(Failure(
                128 + signal,
                "interrupted before the command started".to_owned(),
            ));
        }
    };

    // Under a sandbox the command is `srt ... -- <command>`, after a check that
    // the sandbox blocks direct connections. The settings directory is removed
    // when this guard drops.
    let (command_line, _settings_dir) = match &args.sandbox {
        Some(SandboxRuntime::Srt) => {
            let (line, dir) = srt_command_line(args, http_addr).await?;
            (line, Some(dir))
        }
        None => (args.command.clone(), None),
    };
    let program = &command_line[0];

    // Rust looks up only `.exe` on Windows; resolve `npm` to `npm.cmd` etc.
    #[cfg(windows)]
    let executable = resolve_program(
        program,
        std::env::var_os("PATH").as_deref(),
        std::env::var_os("PATHEXT").as_deref(),
        |p| p.is_file(),
    )
    .unwrap_or_else(|| program.into());
    #[cfg(not(windows))]
    let executable = std::path::PathBuf::from(program);
    let mut child = tokio::process::Command::new(&executable);
    // An agent's command must not inherit a credential or a proxy of its own
    // from this environment; the variables exec sets are added after.
    if args.agent || args.sandbox.is_some() {
        for name in crate::sandbox::SCRUBBED_ENV {
            child.env_remove(name);
        }
    }
    child.args(&command_line[1..]).envs(env);
    let code = match child.spawn() {
        Err(error) => {
            let code = spawn_error_code(&error);
            let what = if code == EXIT_NOT_FOUND {
                "command not found".to_owned()
            } else {
                error.to_string()
            };
            eprintln!("skimasque exec: {}: {what}", program.to_string_lossy());
            code
        }
        Ok(mut child) => match supervise(&mut child, signals).await {
            Ok(code) => code,
            Err(error) => {
                // Never leave the command running without its tunnel.
                let _ = child.start_kill();
                eprintln!("skimasque exec: waiting for the command: {error}");
                EXIT_FAILED
            }
        },
    };

    for task in &tasks {
        task.abort();
    }
    for task in tasks {
        let _ = task.await;
    }
    // Per-connection tasks may still hold clones of the session, so close it
    // through the shared handle rather than by ownership, and give the
    // CONNECTION_CLOSE a moment to go out before the runtime is dropped.
    session.close();
    session.wait_closed(std::time::Duration::from_secs(1)).await;
    Ok(code)
}

/// What setup leaves for launching the command.
struct Prepared {
    session: std::sync::Arc<skimasque::client::Session>,
    /// Every task is `JoinHandle<()>` so they can be aborted together.
    tasks: Vec<tokio::task::JoinHandle<()>>,
    /// The variables added to the child's environment.
    env: Vec<(String, String)>,
    /// The loopback HTTP CONNECT front end; a sandbox chains to it.
    http_addr: SocketAddr,
}

/// Open the session, run the preflight, start the loopback listeners and
/// print the header.
async fn prepare(args: &ExecArgs, agent_credential: Option<&str>) -> Result<Prepared, Failure> {
    use std::sync::Arc;

    use http::{HeaderMap, HeaderValue};
    use tokio::net::TcpListener;

    let login = crate::account::load().map_err(|e| failed(format!("{e:#}")))?;
    let control_plane = resolve_control_plane(args.control_plane.as_deref(), login.as_ref());
    let gateway = resolve_gateway(args.gateway.as_deref(), &control_plane).map_err(failed)?;
    let app = args
        .app
        .clone()
        .unwrap_or_else(|| default_app(&args.command[0]));

    let mut headers = HeaderMap::new();
    headers.insert(
        skimasque::APPLICATION_HEADER,
        HeaderValue::from_str(&app)
            .map_err(|_| failed("the application name contains characters a header cannot carry"))?,
    );
    if let Some(pin) = &args.policy {
        headers.insert(
            skimasque::POLICY_HEADER,
            HeaderValue::from_str(pin)
                .map_err(|_| failed("the policy name contains characters a header cannot carry"))?,
        );
    }

    // An agent presents its session's credential and nothing else: not a token
    // from the environment, and not the login that started the session.
    let auth = match agent_credential {
        Some(credential) => AuthArgs {
            auth_token: Some(credential.to_owned()),
            auth_token_file: None,
            github_oidc: false,
            oidc_token: None,
            oidc_audience: None,
            org: args.auth.org.clone(),
        },
        None => args.auth.clone(),
    };
    let connected = crate::session::open_session(&gateway, &args.tls, &auth, headers)
        .await
        .map_err(|e| failed(format!("{e:#}")))?;
    let identity = connected
        .credential
        .as_deref()
        .and_then(|token| skimasque_identity::peek_identity(token).ok());
    let who = identity_label(identity.as_ref());

    // Preflight: only with a login session on this control plane.
    let mut checked: Option<Checked> = None;
    if let Some(login) = connected
        .login
        .as_ref()
        .filter(|l| crate::normalize_base_url(&l.creds.control_plane) == control_plane)
    {
        let destinations: Vec<String> = if args.forwards.is_empty() {
            vec![PREFLIGHT_PLACEHOLDER.to_owned()]
        } else {
            args.forwards.iter().map(|f| f.target.to_string()).collect()
        };
        match preflight(login, identity.clone().unwrap_or_default(), &app, &destinations).await {
            Ok(probes) => {
                checked = Some(
                    judge(args.policy.as_deref(), &who, &probes, args.forwards.len())
                        .map_err(failed)?,
                );
            }
            Err(error) => eprintln!(
                "skimasque exec: could not check access in advance ({error:#}); the gateway still enforces"
            ),
        }
    }

    let crate::session::Connected {
        session, refresh, ..
    } = connected;
    let session = Arc::new(session);
    let mut tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    if let Some((exchange, ttl)) = refresh {
        let s = session.clone();
        tasks.push(tokio::spawn(async move {
            crate::session::refresh_credential(s, exchange, ttl, false).await;
        }));
    }

    let bind = |port: u16| async move {
        TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await
    };
    let http_listener = bind(0)
        .await
        .map_err(|e| failed(format!("listening on loopback: {e}")))?;
    let socks_listener = bind(0)
        .await
        .map_err(|e| failed(format!("listening on loopback: {e}")))?;
    let http_addr = http_listener.local_addr().map_err(failed)?;
    let socks_addr = socks_listener.local_addr().map_err(failed)?;
    let mut forwards = Vec::new();
    for spec in &args.forwards {
        let listener = crate::forward::bind(spec).await.map_err(|e| {
            failed(format!(
                "could not listen on 127.0.0.1:{} for {}: {e}",
                spec.local_port, spec.target
            ))
        })?;
        let local = listener.local_addr().map_err(failed)?;
        let (s, target) = (session.clone(), spec.target.clone());
        tasks.push(tokio::spawn(async move {
            let _ = crate::forward::serve(listener, target, s).await;
        }));
        forwards.push((spec.clone(), local));
    }
    let s = session.clone();
    tasks.push(tokio::spawn(async move {
        let _ = crate::http_connect::serve(http_listener, s).await;
    }));
    let s = session.clone();
    tasks.push(tokio::spawn(async move {
        let _ = crate::socks5::serve(socks_listener, s).await;
    }));

    if !args.quiet {
        let view = HeaderView {
            identity: who.clone(),
            policy: args
                .policy
                .clone()
                .or_else(|| checked.as_ref().and_then(|c| c.selected.clone()))
                .unwrap_or_else(|| "(selected by the gateway)".to_owned()),
            application: app.clone(),
            gateway: gateway.clone(),
            access: forwards
                .iter()
                .enumerate()
                .map(|(i, (spec, local))| AccessLine {
                    destination: spec.target.to_string(),
                    allowed: checked.as_ref().and_then(|c| c.allowed.get(i).copied()),
                    local: *local,
                })
                .collect(),
            session: checked.as_ref().and_then(|c| c.session.clone()),
        };
        eprint!("{}", render_header(&view));
    }

    Ok(Prepared {
        session,
        tasks,
        // A sandbox gets its proxy settings from srt, which points them at
        // itself; ours would be unreachable from inside it.
        env: if args.sandbox.is_some() {
            Vec::new()
        } else {
            child_env(http_addr, socks_addr, &forwards)
        },
        http_addr,
    })
}

/// Build `srt --settings <file> -- <command>`, and prove the sandbox blocks a
/// direct connection before returning it.
async fn srt_command_line(
    args: &ExecArgs,
    http_addr: SocketAddr,
) -> Result<(Vec<OsString>, crate::sandbox::SettingsDir), Failure> {
    use crate::sandbox;

    let srt = sandbox::locate(
        OsStr::new("srt"),
        std::env::var_os("PATH").as_deref(),
        std::env::var_os("PATHEXT").as_deref(),
    )
    .ok_or_else(|| {
        failed(
            "--sandbox srt needs the sandbox runtime on PATH: \
             npm install -g @anthropic-ai/sandbox-runtime",
        )
    })?;

    let base = match &args.sandbox_settings {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| failed(format!("reading {}: {e}", path.display())))?;
            Some(
                serde_json::from_str(&text)
                    .map_err(|e| failed(format!("{} is not valid JSON: {e}", path.display())))?,
            )
        }
        None => None,
    };
    let doc = sandbox::settings(base, &args.allow_domains, http_addr).map_err(failed)?;
    let (dir, settings_file) = sandbox::SettingsDir::create(&doc)
        .map_err(|e| failed(format!("writing the sandbox settings: {e}")))?;

    // Enforcement check: inside the sandbox, a direct connection to this host's
    // own non-loopback address must fail.
    let (listener, target) = sandbox::probe_target().map_err(|e| {
        failed(format!(
            "cannot check the sandbox: this host has no non-loopback address to test against ({e})"
        ))
    })?;
    let me = std::env::current_exe().map_err(|e| failed(format!("locating skimasque: {e}")))?;
    let probe: Vec<OsString> = vec![
        me.into_os_string(),
        "sandbox-probe".into(),
        target.to_string().into(),
    ];
    let mut check = tokio::process::Command::new(&srt);
    check.args(sandbox::argv(&settings_file, &probe)).kill_on_drop(true);
    for name in sandbox::SCRUBBED_ENV {
        check.env_remove(name);
    }
    let output = tokio::time::timeout(std::time::Duration::from_secs(30), check.output())
        .await
        .map_err(|_| failed("the sandbox check timed out; refusing to start the agent"))?
        .map_err(|e| failed(format!("running {}: {e}", srt.display())))?;
    drop(listener);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // srt may print its own banner; the verdict is the last line.
    let verdict = stdout.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
    sandbox::judge_probe(verdict, output.status.success()).map_err(failed)?;

    let mut line: Vec<OsString> = vec![srt.into_os_string()];
    line.extend(sandbox::argv(&settings_file, &args.command));
    Ok((line, dir))
}

/// The signals exec handles: SIGINT, SIGTERM and SIGHUP on Unix, Ctrl-C on
/// Windows. Registering them replaces their default action (ending exec).
struct Signals {
    #[cfg(unix)]
    int: tokio::signal::unix::Signal,
    #[cfg(unix)]
    term: tokio::signal::unix::Signal,
    #[cfg(unix)]
    hup: tokio::signal::unix::Signal,
    #[cfg(windows)]
    ctrl_c: tokio::signal::windows::CtrlC,
}

/// SIGINT's number; Ctrl-C is reported as it on Windows too.
const SIGINT: i32 = 2;

impl Signals {
    fn register() -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            Ok(Self {
                int: signal(SignalKind::interrupt())?,
                term: signal(SignalKind::terminate())?,
                hup: signal(SignalKind::hangup())?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                ctrl_c: tokio::signal::windows::ctrl_c()?,
            })
        }
    }

    /// The number of the next signal received. Cancel-safe.
    async fn next(&mut self) -> i32 {
        #[cfg(unix)]
        {
            tokio::select! {
                Some(()) = self.int.recv() => libc::SIGINT,
                Some(()) = self.term.recv() => libc::SIGTERM,
                Some(()) = self.hup.recv() => libc::SIGHUP,
                else => std::future::pending::<i32>().await,
            }
        }
        #[cfg(windows)]
        {
            match self.ctrl_c.recv().await {
                Some(()) => SIGINT,
                None => std::future::pending::<i32>().await,
            }
        }
    }
}

/// Wait for the child. Ctrl-C / SIGINT does not end exec (the terminal already
/// delivered it to the child, which decides); SIGTERM and SIGHUP are passed on
/// to the child.
async fn supervise(
    child: &mut tokio::process::Child,
    signals: &mut Signals,
) -> std::io::Result<i32> {
    loop {
        tokio::select! {
            status = child.wait() => return Ok(status_code(status?)),
            signal = signals.next() => {
                if signal != SIGINT {
                    forward_signal(child, signal);
                }
            }
        }
    }
}

#[cfg(unix)]
fn forward_signal(child: &tokio::process::Child, signal: i32) {
    if let Some(pid) = child.id() {
        // SAFETY: `kill` has no memory-safety preconditions; a stale pid at
        // worst signals nothing (the child is still ours until it is reaped).
        unsafe {
            libc::kill(pid as libc::pid_t, signal);
        }
    }
}

/// Windows has only Ctrl-C here, which the child receives itself.
#[cfg(not(unix))]
fn forward_signal(_child: &tokio::process::Child, _signal: i32) {}

/// Simulate each destination on the control plane as the gateway would see it.
async fn preflight(
    login: &crate::session::Login,
    identity: WorkloadIdentity,
    app: &str,
    destinations: &[String],
) -> anyhow::Result<Vec<(String, SimulateResult)>> {
    let api = crate::account::Api::new(&login.creds.control_plane)?;
    let mut probes = Vec::new();
    for destination in destinations {
        let body = serde_json::json!({
            "identity": identity,
            "application": app,
            "destination": destination,
            "transport": "tcp",
        });
        let result = api
            .simulate(&login.creds.session_token, &login.org, None, false, None, &body)
            .await?;
        probes.push((destination.clone(), result));
    }
    Ok(probes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{Credentials, SimLimit, SimulateResult};
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct Harness {
        #[command(flatten)]
        exec: ExecArgs,
    }

    fn creds(cp: &str) -> Credentials {
        Credentials {
            control_plane: cp.into(),
            session_token: "t".into(),
            github_login: "alice".into(),
            user_id: "usr_1".into(),
        }
    }

    fn sim(outcome: &str, policy: Option<&str>, duration: Option<&str>) -> SimulateResult {
        SimulateResult {
            against: "policy revision v1".into(),
            outcome: outcome.into(),
            policy: policy.map(str::to_owned),
            rule: None,
            steps: Vec::new(),
            suggested_rule: Some("allow psql db.prod:5432".into()),
            closest: Vec::new(),
            limits: duration
                .map(|d| {
                    vec![SimLimit {
                        label: "duration".into(),
                        value: d.into(),
                    }]
                })
                .unwrap_or_default(),
        }
    }

    #[test]
    fn the_command_follows_a_double_dash() {
        let h = Harness::try_parse_from([
            "x",
            "--policy",
            "production",
            "--forward",
            "15432:db.prod:5432",
            "--",
            "psql",
            "-h",
            "127.0.0.1",
        ])
        .unwrap();
        assert_eq!(h.exec.policy.as_deref(), Some("production"));
        assert_eq!(h.exec.forwards[0].local_port, 15432);
        assert_eq!(h.exec.command, ["psql", "-h", "127.0.0.1"]);
        assert!(
            Harness::try_parse_from(["x", "--"]).is_err(),
            "a command is required"
        );
    }

    #[test]
    fn empty_names_are_refused() {
        for flag in ["--policy", "--app", "--gateway", "--control-plane"] {
            assert!(
                Harness::try_parse_from(["x", flag, "", "--", "true"]).is_err(),
                "{flag} \"\" is accepted"
            );
            let h = Harness::try_parse_from(["x", flag, "v", "--", "true"]).unwrap();
            assert!(
                [
                    &h.exec.policy,
                    &h.exec.app,
                    &h.exec.gateway,
                    &h.exec.control_plane
                ]
                .iter()
                .any(|v| v.as_deref() == Some("v")),
                "{flag}"
            );
        }
    }

    #[test]
    fn the_control_plane_defaults_to_the_session_then_cloud() {
        assert_eq!(resolve_control_plane(None, None), CLOUD_CONTROL_PLANE);
        assert_eq!(
            resolve_control_plane(None, Some(&creds("https://cp.example/"))),
            "https://cp.example"
        );
        assert_eq!(
            resolve_control_plane(Some("cp2.example"), Some(&creds("https://cp.example"))),
            "https://cp2.example"
        );
    }

    #[test]
    fn cloud_gets_the_cloud_gateway() {
        assert_eq!(
            resolve_gateway(None, CLOUD_CONTROL_PLANE).unwrap(),
            CLOUD_GATEWAY
        );
        assert_eq!(
            resolve_gateway(Some("gw.example:8443"), CLOUD_CONTROL_PLANE).unwrap(),
            "gw.example:8443"
        );
    }

    #[test]
    fn a_self_hosted_control_plane_needs_a_gateway() {
        assert_eq!(
            resolve_gateway(None, "https://cp.example").unwrap_err(),
            "No gateway is configured for https://cp.example. Pass --gateway or set SKIMASQUE_GATEWAY."
        );
        assert_eq!(
            resolve_gateway(Some("gw.example"), "https://cp.example").unwrap(),
            "gw.example"
        );
    }

    #[test]
    fn the_app_defaults_to_the_command_stem() {
        assert_eq!(default_app(OsStr::new("terraform")), "terraform");
        assert_eq!(default_app(OsStr::new("/usr/bin/psql.exe")), "psql");
        assert_eq!(default_app(OsStr::new("./tools/kubectl")), "kubectl");
    }

    #[test]
    fn child_env_replaces_no_proxy_and_sets_both_cases() {
        let http: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let socks: SocketAddr = "127.0.0.1:4002".parse().unwrap();
        let fwd = ForwardSpec::parse("db.prod:5432").unwrap();
        let env = child_env(http, socks, &[(fwd, "127.0.0.1:15432".parse().unwrap())]);
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        for k in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
            assert_eq!(get(k), Some("http://127.0.0.1:4001"), "{k}");
        }
        for k in ["ALL_PROXY", "all_proxy"] {
            assert_eq!(get(k), Some("socks5h://127.0.0.1:4002"), "{k}");
        }
        for k in ["NO_PROXY", "no_proxy"] {
            assert_eq!(get(k), Some("localhost,127.0.0.1,::1"), "{k}");
        }
        assert_eq!(get("SKIMASQUE_PROXY_HTTP"), Some("127.0.0.1:4001"));
        assert_eq!(get("SKIMASQUE_PROXY_SOCKS5"), Some("127.0.0.1:4002"));
        assert_eq!(
            get("SKIMASQUE_FORWARD_DB_PROD_5432"),
            Some("127.0.0.1:15432")
        );
    }

    #[test]
    fn the_identity_label_prefers_the_actor() {
        let mut id = WorkloadIdentity {
            repository: Some("acme/widget".into()),
            ..Default::default()
        };
        assert_eq!(identity_label(Some(&id)), "acme/widget");
        id.actor = Some("alice".into());
        assert_eq!(identity_label(Some(&id)), "alice");
        assert_eq!(
            identity_label(Some(&WorkloadIdentity::default())),
            "unknown"
        );
        assert_eq!(identity_label(None), "unknown");
    }

    #[test]
    fn a_pin_the_preflight_does_not_select_refuses_to_run() {
        let probes = [(
            "db.prod:5432".to_owned(),
            sim("allow", Some("staging"), None),
        )];
        assert_eq!(
            judge(Some("production"), "alice", &probes, 1).unwrap_err(),
            r#"Policy "production" does not apply to you (alice). SkiMasque selected "staging" for this identity."#
        );
        let probes = [(PREFLIGHT_PLACEHOLDER.to_owned(), sim("deny", None, None))];
        assert_eq!(
            judge(Some("production"), "alice", &probes, 0).unwrap_err(),
            r#"Policy "production" does not apply to you (alice). SkiMasque selected no policy for this identity."#
        );
    }

    #[test]
    fn every_forward_denied_refuses_to_run_and_lists_the_fixes() {
        let probes = [(
            "db.prod:5432".to_owned(),
            sim("deny", Some("production"), None),
        )];
        let e = judge(Some("production"), "alice", &probes, 1).unwrap_err();
        assert!(e.starts_with("Every destination would be denied:"), "{e}");
        assert!(
            e.contains("  db.prod:5432: denied. To allow it: allow psql db.prod:5432"),
            "{e}"
        );
    }

    #[test]
    fn partly_denied_forwards_run_and_report_the_session() {
        let probes = [
            (
                "db.prod:5432".to_owned(),
                sim("allow", Some("production"), Some("20m")),
            ),
            (
                "cache:6379".to_owned(),
                sim("deny", Some("production"), None),
            ),
        ];
        let c = judge(None, "alice", &probes, 2).unwrap();
        assert_eq!(c.selected.as_deref(), Some("production"));
        assert_eq!(c.allowed, [true, false]);
        assert_eq!(c.session.as_deref(), Some("20m"));
    }

    #[test]
    fn the_placeholder_probe_only_reveals_the_policy() {
        let probes = [(
            PREFLIGHT_PLACEHOLDER.to_owned(),
            sim("deny", Some("production"), None),
        )];
        let c = judge(Some("production"), "alice", &probes, 0).unwrap();
        assert_eq!(c.selected.as_deref(), Some("production"));
        assert!(c.allowed.is_empty());
        assert_eq!(c.session, None);
    }

    #[test]
    fn the_header_matches_the_spec_layout() {
        let view = HeaderView {
            identity: "alice".into(),
            policy: "production".into(),
            application: "terraform".into(),
            gateway: "gateway.skimasque.com".into(),
            access: vec![AccessLine {
                destination: "db.prod:5432".into(),
                allowed: Some(true),
                local: "127.0.0.1:15432".parse().unwrap(),
            }],
            session: Some("20m".into()),
        };
        assert_eq!(
            render_header(&view),
            "SkiMasque\n\n\
             Identity     alice\n\
             Policy       production\n\
             Application  terraform\n\
             Gateway      gateway.skimasque.com\n\n\
             Access\n  db.prod:5432     ✓  → 127.0.0.1:15432\n\n\
             Session\n  20m\n\n\
             Connected.\n"
        );
    }

    #[test]
    fn the_header_omits_empty_sections_and_unknown_marks() {
        let mut view = HeaderView {
            identity: "unknown".into(),
            policy: "(selected by the gateway)".into(),
            application: "curl".into(),
            gateway: "gw:443".into(),
            access: Vec::new(),
            session: None,
        };
        let s = render_header(&view);
        assert!(!s.contains("Access") && !s.contains("Session"), "{s}");
        view.access.push(AccessLine {
            destination: "db:1".into(),
            allowed: None,
            local: "127.0.0.1:2".parse().unwrap(),
        });
        assert!(render_header(&view).contains("\n  db:1             → 127.0.0.1:2\n"));
    }

    #[test]
    fn spawn_errors_map_to_126_and_127() {
        use std::io::{Error, ErrorKind};
        assert_eq!(
            spawn_error_code(&Error::from(ErrorKind::NotFound)),
            EXIT_NOT_FOUND
        );
        assert_eq!(
            spawn_error_code(&Error::from(ErrorKind::PermissionDenied)),
            EXIT_NOT_EXECUTABLE
        );
    }

    fn fake_path(dirs: &[&str]) -> OsString {
        std::env::join_paths(dirs).unwrap()
    }

    /// A fake file system that, like NTFS, ignores case.
    fn on_disk(files: &[&str]) -> impl Fn(&Path) -> bool {
        let files: Vec<String> = files.iter().map(|f| f.to_lowercase()).collect();
        move |p: &Path| {
            let p = p.to_string_lossy().replace('\\', "/").to_lowercase();
            files.contains(&p)
        }
    }

    #[test]
    fn resolve_program_follows_path_order_then_pathext_order() {
        let path = fake_path(&["first", "second"]);
        let npm = OsStr::new("npm");
        // An earlier directory wins over a preferred extension in a later one.
        assert_eq!(
            resolve_program(
                npm,
                Some(&path),
                None,
                on_disk(&["first/npm.cmd", "second/npm.exe"])
            ),
            Some(Path::new("first").join("npm.CMD"))
        );
        // Within one directory, PATHEXT order decides.
        let tool = OsStr::new("tool");
        let both = || on_disk(&["first/tool.bat", "first/tool.cmd"]);
        assert_eq!(
            resolve_program(tool, Some(&path), Some(OsStr::new(".CMD;.BAT")), both()),
            Some(Path::new("first").join("tool.CMD"))
        );
        assert_eq!(
            resolve_program(tool, Some(&path), None, both()),
            Some(Path::new("first").join("tool.BAT")),
            "the default PATHEXT puts .BAT before .CMD"
        );
    }

    #[test]
    fn resolve_program_leaves_absent_or_qualified_names_alone() {
        let path = fake_path(&["first"]);
        assert_eq!(
            resolve_program(OsStr::new("npm"), Some(&path), None, |_| false),
            None
        );
        assert_eq!(
            resolve_program(OsStr::new("npm"), None, None, |_| true),
            None
        );
        for name in ["tools/npm", "npm.cmd", "./npm"] {
            assert_eq!(
                resolve_program(OsStr::new(name), Some(&path), None, |_| true),
                None,
                "{name}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_signal_death_is_128_plus_the_signal() {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            status_code(std::process::ExitStatus::from_raw(15)),
            128 + 15
        );
        assert_eq!(status_code(std::process::ExitStatus::from_raw(3 << 8)), 3);
    }

    fn parse(args: &[&str]) -> Result<ExecArgs, clap::Error> {
        Harness::try_parse_from(std::iter::once("x").chain(args.iter().copied())).map(|h| h.exec)
    }

    #[test]
    fn agent_options_need_agent() {
        for flag in ["--ttl", "--runtime", "--run-id", "--parent"] {
            assert!(parse(&[flag, "v", "--", "true"]).is_err(), "{flag} without --agent");
            assert!(parse(&["--agent", "--unsandboxed", flag, "v", "--", "true"]).is_ok(), "{flag}");
        }
        assert!(parse(&["--unsandboxed", "--", "true"]).is_err(), "--unsandboxed is for agents");
    }

    #[test]
    fn an_agent_is_never_run_unconfined_by_accident() {
        let bare = parse(&["--agent", "--", "claude"]).unwrap();
        let why = validate(&bare).unwrap_err();
        assert!(why.contains("--sandbox srt") && why.contains("--unsandboxed"), "{why}");

        assert!(validate(&parse(&["--agent", "--unsandboxed", "--", "claude"]).unwrap()).is_ok());
        assert!(
            parse(&["--agent", "--unsandboxed", "--sandbox", "srt", "--", "claude"]).is_err(),
            "the two choices contradict each other"
        );
    }

    #[test]
    fn a_sandbox_needs_named_domains_and_no_loopback_forwards() {
        let none = parse(&["--sandbox", "srt", "--", "true"]).unwrap();
        assert!(validate(&none).unwrap_err().contains("--allow-domain"));

        let ok = parse(&["--sandbox", "srt", "--allow-domain", "*.acme.dev", "--", "true"]).unwrap();
        assert!(validate(&ok).is_ok());
        assert_eq!(ok.allow_domains, ["*.acme.dev"]);

        let forwarded = parse(&[
            "--sandbox", "srt", "--allow-domain", "a.dev", "--forward", "db.prod:5432", "--", "true",
        ])
        .unwrap();
        assert!(validate(&forwarded).unwrap_err().contains("--forward"));

        assert!(parse(&["--allow-domain", "a.dev", "--", "true"]).is_err(), "needs --sandbox");
        assert!(
            parse(&["--sandbox", "srt", "--allow-domain", "*", "--", "true"]).is_err(),
            "srt refuses a bare *, so we do at the prompt"
        );
        assert!(parse(&["--sandbox", "bwrap", "--allow-domain", "a.dev", "--", "true"]).is_err());
    }
}
