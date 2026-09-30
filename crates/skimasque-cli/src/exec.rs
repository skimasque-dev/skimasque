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
    #[arg(long, value_name = "NAME")]
    pub policy: Option<String>,

    /// The application to declare (`X-Masque-Application`). Defaults to the
    /// command's file name, e.g. `terraform`.
    #[arg(long, value_name = "NAME")]
    pub app: Option<String>,

    /// The gateway, as `host[:port]`. Defaults to gateway.skimasque.com when the
    /// control plane is SkiMasque Cloud; required for a self-hosted one.
    #[arg(long, env = "SKIMASQUE_GATEWAY", value_name = "HOST[:PORT]")]
    pub gateway: Option<String>,

    /// The control plane. Defaults to the one you signed in to, else SkiMasque
    /// Cloud.
    #[arg(long, env = "SKIMASQUE_CONTROL_PLANE", value_name = "URL")]
    pub control_plane: Option<String>,

    /// Listen on a loopback port and tunnel each connection to HOST:PORT, for
    /// tools that ignore proxy settings. Repeatable. Its address is also in
    /// `$SKIMASQUE_FORWARD_<HOST>_<PORT>`.
    #[arg(long = "forward", value_name = "[LOCAL_PORT:]HOST:PORT", value_parser = ForwardSpec::parse)]
    pub forwards: Vec<ForwardSpec>,

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

/// Run exec to completion and return the process exit status. Every failure
/// before the command starts is printed and returns [`EXIT_FAILED`].
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
    use std::sync::Arc;

    use http::{HeaderMap, HeaderValue};
    use tokio::net::TcpListener;

    let login = crate::account::load().map_err(|e| failed(format!("{e:#}")))?;
    let control_plane = resolve_control_plane(args.control_plane.as_deref(), login.as_ref());
    let gateway = resolve_gateway(args.gateway.as_deref(), &control_plane).map_err(failed)?;
    let program = args.command[0].clone();
    let app = args.app.clone().unwrap_or_else(|| default_app(&program));

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

    let connected = crate::session::open_session(&gateway, &args.tls, &args.auth, headers)
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
    // Every task is `JoinHandle<()>` so they can be aborted together.
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

    let mut child = tokio::process::Command::new(&program);
    child
        .args(&args.command[1..])
        .envs(child_env(http_addr, socks_addr, &forwards));
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
        Ok(mut child) => supervise(&mut child).await.map_err(failed)?,
    };

    for task in &tasks {
        task.abort();
    }
    for task in tasks {
        let _ = task.await;
    }
    if let Ok(session) = Arc::try_unwrap(session) {
        session.close();
    }
    Ok(code)
}

/// Wait for the child. Ctrl-C does not end exec (the child receives it and
/// decides); SIGTERM and SIGHUP are passed on to the child.
async fn supervise(child: &mut tokio::process::Child) -> std::io::Result<i32> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate())?;
        let mut hup = signal(SignalKind::hangup())?;
        let mut int = signal(SignalKind::interrupt())?;
        loop {
            tokio::select! {
                status = child.wait() => return Ok(status_code(status?)),
                _ = int.recv() => {}
                _ = term.recv() => forward_signal(child, libc::SIGTERM),
                _ = hup.recv() => forward_signal(child, libc::SIGHUP),
            }
        }
    }
    #[cfg(not(unix))]
    {
        loop {
            tokio::select! {
                status = child.wait() => return Ok(status_code(status?)),
                _ = tokio::signal::ctrl_c() => {}
            }
        }
    }
}

#[cfg(unix)]
fn forward_signal(child: &tokio::process::Child, signal: libc::c_int) {
    if let Some(pid) = child.id() {
        // SAFETY: `kill` has no memory-safety preconditions; a stale pid at
        // worst signals nothing (the child is still ours until it is reaped).
        unsafe {
            libc::kill(pid as libc::pid_t, signal);
        }
    }
}

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
}
