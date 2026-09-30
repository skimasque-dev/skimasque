//! `skimasque exec` end to end: the real binary, a real gateway with a policy
//! and an audit sink, TCP echo targets, and this test executable re-run as the
//! child (the `child_probe` test, which does nothing unless SKM_EXEC_CHILD is
//! set).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream as StdTcp};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use skimasque::audit::{AuditEvent, AuditSink};
use skimasque::policy::AddressPolicy;
use skimasque::server::{ProxyConfig, Server};
use skimasque::service::{Dispatch, PolicyLayer, TcpProxy};
use skimasque::tls;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tower::ServiceBuilder;

#[derive(Debug, Default)]
struct Sink(Mutex<Vec<AuditEvent>>);
impl AuditSink for Sink {
    fn record(&self, event: &AuditEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

async fn spawn_echo(tag: &'static [u8]) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 {
                        return;
                    }
                    let mut reply = tag.to_vec();
                    reply.extend_from_slice(&buf[..n]);
                    if s.write_all(&reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

struct Gateway {
    addr: SocketAddr,
    ca: PathBuf,
    sink: Arc<Sink>,
    _dir: TempDir,
}

/// A throwaway directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("skm-exec-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A gateway whose policy `t` lets `curl` reach `allowed`.
async fn spawn_gateway(tag: &str, allowed: &[SocketAddr]) -> Gateway {
    // A rule with no destinations is invalid, so an empty `allowed` still names one.
    let mut list: Vec<String> = allowed.iter().map(|a| format!("\"{a}\"")).collect();
    if list.is_empty() {
        list.push("\"192.0.2.1:9\"".to_owned());
    }
    let policy = format!(
        "name = \"t\"\n[[rules]]\napplication = \"curl\"\naction = \"allow\"\ndestinations = [{}]\n",
        list.join(", ")
    );
    let set =
        skimasque::policy_engine::PolicySet::from_documents([("p.toml", policy.as_str())]).unwrap();
    let sink = Arc::new(Sink::default());
    let service = ServiceBuilder::new()
        .layer(PolicyLayer::new(set).with_audit(sink.clone()))
        .service(Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive())));
    let generated = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();
    let server_tls = tls::server_config_from_pem(
        generated.certificate_pem.as_bytes(),
        generated.key_pem.as_bytes(),
    )
    .unwrap();
    let server = Server::bind(
        "127.0.0.1:0".parse().unwrap(),
        server_tls,
        service,
        ProxyConfig::new("localhost").unwrap(),
    )
    .unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    let dir = TempDir::new(tag);
    let ca = dir.0.join("gateway.pem");
    std::fs::write(&ca, generated.certificate_pem).unwrap();
    Gateway {
        addr,
        ca,
        sink,
        _dir: dir,
    }
}

/// Run `skimasque exec <extra> -- <this test binary as child_probe>`.
async fn exec(gw: &Gateway, extra: &[&str], child_env: &[(&str, String)]) -> (i32, String) {
    let config = TempDir::new("config");
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_skimasque"));
    cmd.arg("exec")
        .args(["--gateway", &gw.addr.to_string()])
        .args(["--authority", &format!("localhost:{}", gw.addr.port())])
        .arg("--ca")
        .arg(&gw.ca)
        .args(["--auth-token", "test-token", "--app", "curl"])
        .args(extra)
        .arg("--")
        .arg(std::env::current_exe().unwrap())
        .args(["child_probe", "--exact", "--nocapture", "--test-threads=1"])
        .env("SKIMASQUE_CONFIG_HOME", &config.0)
        .env_remove("SKIMASQUE_GATEWAY")
        .env_remove("SKIMASQUE_CONTROL_PLANE")
        .env_remove("SKIMASQUE_TOKEN");
    for (k, v) in child_env {
        cmd.env(k, v);
    }
    let out = tokio::time::timeout(Duration::from_secs(60), cmd.output())
        .await
        .expect("exec timed out")
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The child: talks through `HTTPS_PROXY` and a forward, per SKM_EXEC_CHILD.
#[test]
fn child_probe() {
    let Ok(mode) = std::env::var("SKM_EXEC_CHILD") else {
        return;
    };
    let proxy = std::env::var("SKIMASQUE_PROXY_HTTP").expect("exec sets SKIMASQUE_PROXY_HTTP");
    let target = std::env::var("SKM_ECHO").unwrap();
    if let Ok(path) = std::env::var("SKM_REPORT") {
        std::fs::write(path, &proxy).unwrap();
    }
    let mut s = StdTcp::connect(&proxy).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
    }
    let head = String::from_utf8(head).unwrap();
    if mode == "denied" {
        std::process::exit(if head.starts_with("HTTP/1.1 403 ") {
            9
        } else {
            1
        });
    }
    assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
    s.write_all(b"hi").unwrap();
    let mut reply = [0u8; 5];
    s.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"ok:hi");

    let fwd_var = std::env::var("SKM_FORWARD_VAR").unwrap();
    let fwd = std::env::var(&fwd_var).unwrap_or_else(|_| panic!("{fwd_var} is set"));
    let mut f = StdTcp::connect(&fwd).unwrap();
    f.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    f.write_all(b"hi").unwrap();
    let mut reply = [0u8; 5];
    f.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"fw:hi");
    assert_eq!(
        std::env::var("NO_PROXY").unwrap(),
        "localhost,127.0.0.1,::1"
    );
    std::process::exit(7);
}

#[tokio::test]
async fn exec_gives_the_command_access_and_passes_its_exit_code() {
    let echo = spawn_echo(b"ok:").await;
    let fwd = spawn_echo(b"fw:").await;
    let gw = spawn_gateway("access", &[echo, fwd]).await;
    let report = gw._dir.0.join("proxy.txt");
    let var = format!("SKIMASQUE_FORWARD_127_0_0_1_{}", fwd.port());
    let (code, stderr) = exec(
        &gw,
        &["--forward", &fwd.to_string()],
        &[
            ("SKM_EXEC_CHILD", "ok".into()),
            ("SKM_ECHO", echo.to_string()),
            ("SKM_FORWARD_VAR", var),
            ("SKM_REPORT", report.display().to_string()),
            ("NO_PROXY", "example.com".into()),
        ],
    )
    .await;
    assert_eq!(code, 7, "{stderr}");
    assert!(stderr.contains("Connected."), "{stderr}");

    // The front end is gone once exec has exited.
    let proxy = std::fs::read_to_string(&report).unwrap();
    assert!(
        StdTcp::connect(proxy.trim()).is_err(),
        "listener still open after exit"
    );
}

#[tokio::test]
async fn a_pin_that_does_not_match_is_denied_and_audited() {
    let echo = spawn_echo(b"ok:").await;
    let gw = spawn_gateway("pin", &[echo]).await;
    let (code, stderr) = exec(
        &gw,
        &["--policy", "production"],
        &[
            ("SKM_EXEC_CHILD", "denied".into()),
            ("SKM_ECHO", echo.to_string()),
        ],
    )
    .await;
    assert_eq!(code, 9, "{stderr}");
    assert!(
        stderr.contains(r#"Policy "production" does not apply to this identity; "t" does."#),
        "{stderr}"
    );
    let events = gw.sink.0.lock().unwrap();
    let denied = events
        .iter()
        .find(|e| e.decision == "deny")
        .expect("a denial was audited");
    assert_eq!(denied.requested_policy.as_deref(), Some("production"));
}

#[tokio::test]
async fn a_missing_command_exits_127() {
    let gw = spawn_gateway("missing", &[]).await;
    let config = TempDir::new("missing-config");
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_skimasque"))
        .args(["exec", "--gateway", &gw.addr.to_string()])
        .args(["--authority", &format!("localhost:{}", gw.addr.port())])
        .arg("--ca")
        .arg(&gw.ca)
        .args([
            "--auth-token",
            "t",
            "--quiet",
            "--",
            "skm-definitely-not-a-command",
        ])
        .env("SKIMASQUE_CONFIG_HOME", &config.0)
        .output()
        .await
        .unwrap();
    assert_eq!(out.status.code(), Some(127));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("skm-definitely-not-a-command: command not found"),
        "{stderr}"
    );
}

#[tokio::test]
async fn a_self_hosted_control_plane_without_a_gateway_exits_125() {
    let config = TempDir::new("nogw-config");
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_skimasque"))
        .args([
            "exec",
            "--control-plane",
            "https://cp.example",
            "--auth-token",
            "t",
            "--",
            "anything",
        ])
        .env("SKIMASQUE_CONFIG_HOME", &config.0)
        .env_remove("SKIMASQUE_GATEWAY")
        .output()
        .await
        .unwrap();
    assert_eq!(out.status.code(), Some(125));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(
            "No gateway is configured for https://cp.example. Pass --gateway or set SKIMASQUE_GATEWAY."
        ),
        "{stderr}"
    );
}
