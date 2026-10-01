//! End-to-end: revoking a session closes the tunnels it has open and refuses
//! new ones, over a real QUIC connection and a real loopback target.

use std::net::SocketAddr;
use std::task::{Context, Poll};
use std::time::Duration;

use skimasque::client::{Client, Session};
use skimasque::policy::AddressPolicy;
use skimasque::server::{ProxyConfig, Server};
use skimasque::service::{Accepted, Dispatch, Rejection, TcpProxy, TunnelRequest};
use skimasque::{tls, RevocationLayer, Revocations};
use skimasque_core::target::Target;
use skimasque_core::UriTemplate;
use skimasque_policy::WorkloadIdentity;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tower::{Layer, Service};

const STEP: Duration = Duration::from_secs(10);

async fn spawn_echo() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                loop {
                    let n = match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    if stream.write_all(&buf[..n]).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

/// Stands in for an identity layer: every request is from a workload whose
/// credential names session `sid`.
#[derive(Clone)]
struct Stamp<S> {
    inner: S,
    sid: &'static str,
}

impl<S> Service<TunnelRequest> for Stamp<S>
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection>,
{
    type Response = Accepted;
    type Error = Rejection;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: TunnelRequest) -> Self::Future {
        request.extensions_mut().insert(WorkloadIdentity {
            sid: Some(self.sid.to_owned()),
            ..Default::default()
        });
        self.inner.call(request)
    }
}

/// A proxy whose every tunnel belongs to session `sid`, sharing `revocations`.
async fn session_proxy(sid: &'static str, revocations: &Revocations) -> Session {
    let service = Stamp {
        inner: RevocationLayer::new(revocations.clone())
            .layer(Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive()))),
        sid,
    };
    let generated = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();
    let server_tls = tls::server_config_from_pem(
        generated.certificate_pem.as_bytes(),
        generated.key_pem.as_bytes(),
    )
    .unwrap();
    let config = ProxyConfig::new("localhost").unwrap();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), server_tls, service, config).unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.run().await;
    });

    let client_tls = tls::client_config_with_ca(generated.certificate_pem.as_bytes()).unwrap();
    let client = Client::new(client_tls).unwrap();
    let template = UriTemplate::default_connect_udp(&format!("localhost:{}", addr.port())).unwrap();
    client.connect(addr, template).await.unwrap()
}

async fn assert_echoes(tunnel: &mut skimasque::client::TcpTunnel, text: &[u8]) {
    tunnel.write(text).await.unwrap();
    let reply = timeout(STEP, tunnel.read())
        .await
        .expect("timed out")
        .expect("tunnel error")
        .expect("tunnel closed early");
    assert_eq!(&reply[..], text);
}

#[tokio::test]
async fn revoking_a_session_closes_its_open_tunnels_and_refuses_new_ones() {
    let echo = spawn_echo().await;
    let target = Target::parse(&echo.to_string()).unwrap();
    let revocations = Revocations::new();

    let doomed = session_proxy("sess_doomed", &revocations).await;
    let bystander = session_proxy("sess_other", &revocations).await;

    let mut open = doomed.connect_tcp(target.clone()).await.unwrap();
    let mut unrelated = bystander.connect_tcp(target.clone()).await.unwrap();
    assert_echoes(&mut open, b"before").await;
    assert_echoes(&mut unrelated, b"before").await;

    revocations.replace(["sess_doomed".to_owned()]);

    // The open tunnel ends promptly (EOF or an error), not at some idle timeout.
    let ended = timeout(Duration::from_secs(5), open.read())
        .await
        .expect("the revoked session's tunnel stayed open");
    assert!(matches!(ended, Ok(None) | Err(_)), "{ended:?}");

    // A new tunnel for the same session is refused.
    assert!(
        doomed.connect_tcp(target).await.is_err(),
        "a revoked session opened a new tunnel"
    );

    // Other sessions are untouched.
    assert_echoes(&mut unrelated, b"after").await;
}
