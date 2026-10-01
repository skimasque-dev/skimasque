//! Exercise authentication and credential renewal through the real CLI process.
use skimasque::{
    exchange::{CredentialMinter, MintError, MintedCredential},
    policy::AddressPolicy,
    policy_engine::WorkloadIdentity,
    server::{ProxyConfig, Server},
    service::{Dispatch, IdentityLayer, IdentityVerifier, TcpProxy},
    tls,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::Command,
    time::{sleep, timeout},
};
use tower::ServiceBuilder;

#[derive(Clone, Debug, Default)]
struct Credentials {
    issued: Arc<AtomicUsize>,
    observed: Arc<Mutex<Vec<String>>>,
}
impl CredentialMinter for Credentials {
    fn mint(
        &self,
        token: String,
    ) -> Pin<Box<dyn Future<Output = Result<MintedCredential, MintError>> + Send>> {
        let issued = self.issued.clone();
        Box::pin(async move {
            if token != "test-oidc" {
                return Err(MintError::Unauthorized("invalid identity".into()));
            }
            let n = issued.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(MintedCredential {
                credential: format!("credential-{n}"),
                expires_in: Duration::from_secs(2),
            })
        })
    }
}
impl IdentityVerifier for Credentials {
    fn verify(
        &self,
        token: String,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadIdentity, String>> + Send>> {
        let observed = self.observed.clone();
        Box::pin(async move {
            if !token.starts_with("credential-") {
                return Err("invalid credential".into());
            }
            observed.lock().unwrap().push(token);
            Ok(WorkloadIdentity::default())
        })
    }
}

async fn check_proxy(oidc: &str) {
    let credentials = Credentials::default();
    let generated = tls::generate_self_signed(vec!["localhost".into()]).unwrap();
    let service = ServiceBuilder::new()
        .layer(IdentityLayer::new(Arc::new(credentials.clone())))
        .service(Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive())));
    let server = Server::bind(
        "127.0.0.1:0".parse().unwrap(),
        tls::server_config_from_pem(
            generated.certificate_pem.as_bytes(),
            generated.key_pem.as_bytes(),
        )
        .unwrap(),
        service,
        ProxyConfig::new("localhost")
            .unwrap()
            .with_minter(Arc::new(credentials.clone())),
    )
    .unwrap();
    let gateway = server.local_addr().unwrap();
    let gateway_task = tokio::spawn(server.run());
    let dir =
        std::env::temp_dir().join(format!("skimasque-binary-{}-{}", std::process::id(), oidc));
    std::fs::create_dir_all(&dir).unwrap();
    let ca = dir.join("ca.pem");
    let ready = dir.join("ready.json");
    let _ = std::fs::remove_file(&ready);
    std::fs::write(&ca, generated.certificate_pem).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_skimasque-client"))
        .args([
            "--proxy",
            &gateway.to_string(),
            "--authority",
            &format!("localhost:{}", gateway.port()),
            "--oidc-token",
            oidc,
            "--oidc-audience",
            "test",
            "--ca",
            ca.to_str().unwrap(),
            "proxy",
            "--http-listen",
            "127.0.0.1:0",
            "--socks-listen",
            "127.0.0.1:0",
            "--ready-file",
            ready.to_str().unwrap(),
        ])
        .env("SKIMASQUE_CONFIG_HOME", &dir)
        .env_remove("SKIMASQUE_TOKEN")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    if oidc != "test-oidc" {
        assert!(!timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success());
        assert!(
            !ready.exists(),
            "authentication failure must not publish readiness"
        );
    } else {
        timeout(Duration::from_secs(10), async {
            while !ready.exists() {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "client exited before readiness"
                );
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&ready).unwrap()).unwrap();
        assert_eq!(record["pid"], child.id().unwrap());
        assert_eq!(record["gateway"], gateway.to_string());
        assert_eq!(
            credentials.issued.load(Ordering::SeqCst),
            1,
            "short TTL must not busy-loop"
        );
        let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_addr = target.local_addr().unwrap();
        let target_task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = target.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).await.unwrap();
                    stream.write_all(&byte).await.unwrap();
                });
            }
        });
        let connect = || async {
            let mut stream = TcpStream::connect(record["http"].as_str().unwrap())
                .await
                .unwrap();
            stream
                .write_all(
                    format!("CONNECT {target_addr} HTTP/1.1\r\nHost: {target_addr}\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            let mut response = Vec::new();
            while !response.ends_with(b"\r\n\r\n") {
                response.push(stream.read_u8().await.unwrap());
            }
            assert!(response.starts_with(b"HTTP/1.1 200"));
            stream
        };
        let mut existing = timeout(Duration::from_secs(5), connect()).await.unwrap();
        timeout(Duration::from_secs(5), async {
            while credentials.issued.load(Ordering::SeqCst) < 2 {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        sleep(Duration::from_millis(100)).await; // allow the exchange reply to install the replacement
        let mut renewed = timeout(Duration::from_secs(5), connect()).await.unwrap();
        for stream in [&mut existing, &mut renewed] {
            stream.write_all(b"x").await.unwrap();
            assert_eq!(
                timeout(Duration::from_secs(5), stream.read_u8())
                    .await
                    .unwrap()
                    .unwrap(),
                b'x'
            );
        }
        assert!(
            credentials
                .observed
                .lock()
                .unwrap()
                .iter()
                .any(|token| token != "credential-1"),
            "new tunnels must carry the renewed credential"
        );
        #[cfg(unix)]
        {
            assert!(std::process::Command::new("kill")
                .args(["-TERM", &child.id().unwrap().to_string()])
                .status()
                .unwrap()
                .success());
            assert!(timeout(Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success());
            assert!(!ready.exists(), "SIGTERM removes readiness");
        }
        #[cfg(windows)]
        child.kill().await.unwrap();
        target_task.abort();
    }
    gateway_task.abort();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn authentication_failure_never_publishes_readiness() {
    check_proxy("invalid-oidc").await;
}

#[tokio::test]
async fn short_credentials_refresh_for_new_tunnels_without_interrupting_existing_ones() {
    check_proxy("test-oidc").await;
}
