//! TLS rotation: `TlsReloader::reload` swaps the certificate presented to new
//! QUIC handshakes without disturbing connections already established.

use std::net::SocketAddr;
use std::time::Duration;

use skimasque::client::{Client, Session};
use skimasque::policy::AddressPolicy;
use skimasque::server::{ProxyConfig, Server, TlsReloader};
use skimasque::service::{Dispatch, TcpProxy};
use skimasque::tls::{self, SelfSigned};
use skimasque_core::target::Target;
use skimasque_core::UriTemplate;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

async fn spawn_echo() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
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
                    let mut reply = b"echo:".to_vec();
                    reply.extend_from_slice(&buf[..n]);
                    if stream.write_all(&reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

struct Proxy {
    addr: SocketAddr,
    reloader: TlsReloader,
}

fn spawn_proxy(first: &SelfSigned) -> Proxy {
    let server_tls = tls::server_config_from_pem(
        first.certificate_pem.as_bytes(),
        first.key_pem.as_bytes(),
    )
    .unwrap();
    let service = Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive()));
    let config = ProxyConfig::new("localhost").unwrap();
    let server =
        Server::bind("127.0.0.1:0".parse().unwrap(), server_tls, service, config).unwrap();
    let addr = server.local_addr().unwrap();
    let reloader = server.tls_reloader();
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    Proxy { addr, reloader }
}

async fn connect(proxy_addr: SocketAddr, ca_pem: &str) -> Result<Session, skimasque::Error> {
    let client_tls = tls::client_config_with_ca(ca_pem.as_bytes()).unwrap();
    let client = Client::new(client_tls).unwrap();
    let template =
        UriTemplate::default_connect_udp(&format!("localhost:{}", proxy_addr.port())).unwrap();
    client.connect(proxy_addr, template).await
}

async fn round_trip(tunnel: &mut skimasque::client::TcpTunnel, msg: &[u8], want: &[u8]) {
    tunnel.write(msg).await.unwrap();
    let reply = timeout(REPLY_TIMEOUT, tunnel.read())
        .await
        .expect("timed out")
        .expect("tunnel error")
        .expect("tunnel closed early");
    assert_eq!(&reply[..], want);
}

#[tokio::test]
async fn reloading_the_certificate_rebinds_new_handshakes_only() {
    let echo = spawn_echo().await;
    let cert_a = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();
    let cert_b = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();

    let proxy = spawn_proxy(&cert_a);

    // A client that pins cert A connects and opens a tunnel.
    let session_a = connect(proxy.addr, &cert_a.certificate_pem).await.unwrap();
    let mut tunnel = session_a
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    round_trip(&mut tunnel, b"before", b"echo:before").await;

    // Rotate to cert B.
    let tls_b = tls::server_config_from_pem(
        cert_b.certificate_pem.as_bytes(),
        cert_b.key_pem.as_bytes(),
    )
    .unwrap();
    proxy.reloader.reload(tls_b).unwrap();

    // The connection opened under cert A is undisturbed.
    round_trip(&mut tunnel, b"after", b"echo:after").await;

    // A new client still pinning cert A now fails the handshake.
    let stale = connect(proxy.addr, &cert_a.certificate_pem).await;
    assert!(
        stale.is_err(),
        "a handshake after the swap must not accept the old certificate"
    );

    // A new client pinning cert B connects and works.
    let session_b = connect(proxy.addr, &cert_b.certificate_pem)
        .await
        .expect("the rotated certificate is accepted");
    let mut fresh = session_b
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    round_trip(&mut fresh, b"rotated", b"echo:rotated").await;
}
