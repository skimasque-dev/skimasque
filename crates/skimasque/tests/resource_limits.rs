//! Resource ceilings: the per-connection tunnel cap and the tunnel idle
//! timeout, exercised over a real QUIC connection.

use std::net::SocketAddr;
use std::time::Duration;

use skimasque::client::{Client, Session};
use skimasque::policy::AddressPolicy;
use skimasque::server::{ConnectionRate, ProxyConfig, ResourceLimits, Server};
use skimasque::service::{Dispatch, TcpProxy};
use skimasque::tls;
use skimasque_core::target::Target;
use skimasque_core::UriTemplate;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// A loopback TCP server that echoes every chunk back with `echo:` prepended.
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
    certificate_pem: String,
}

/// A TCP-only proxy with `limits`, driven by [`Server::run`].
fn spawn_proxy(limits: ResourceLimits) -> Proxy {
    let generated = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();
    let server_tls = tls::server_config_from_pem(
        generated.certificate_pem.as_bytes(),
        generated.key_pem.as_bytes(),
    )
    .unwrap();
    let service = Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive()));
    let config = ProxyConfig::new("localhost").unwrap().with_limits(limits);
    let server =
        Server::bind("127.0.0.1:0".parse().unwrap(), server_tls, service, config).unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    Proxy {
        addr,
        certificate_pem: generated.certificate_pem,
    }
}

async fn connect(proxy: &Proxy) -> Session {
    let client_tls = tls::client_config_with_ca(proxy.certificate_pem.as_bytes()).unwrap();
    let client = Client::new(client_tls).unwrap();
    let template =
        UriTemplate::default_connect_udp(&format!("localhost:{}", proxy.addr.port())).unwrap();
    client.connect(proxy.addr, template).await.unwrap()
}

async fn round_trip(tunnel: &mut skimasque::client::TcpTunnel, payload: &[u8], expected: &[u8]) {
    tunnel.write(payload).await.unwrap();
    let reply = timeout(REPLY_TIMEOUT, tunnel.read())
        .await
        .expect("timed out")
        .expect("tunnel error")
        .expect("tunnel closed early");
    assert_eq!(&reply[..], expected);
}

#[tokio::test]
async fn a_connection_past_its_tunnel_cap_is_refused_but_stays_usable() {
    let echo = spawn_echo().await;
    let proxy = spawn_proxy(ResourceLimits {
        max_tunnels_per_connection: Some(1),
        ..ResourceLimits::unlimited()
    });
    let session = connect(&proxy).await;

    // The one permitted tunnel opens and works.
    let mut first = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    round_trip(&mut first, b"one", b"echo:one").await;

    // A second concurrent tunnel is over the cap: 503, connection intact.
    match session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap_err()
    {
        skimasque::Error::Rejected { status, .. } => {
            assert_eq!(status, http::StatusCode::SERVICE_UNAVAILABLE)
        }
        other => panic!("expected a 503, got {other:?}"),
    }

    // The first tunnel is unaffected by the refusal.
    round_trip(&mut first, b"two", b"echo:two").await;

    // Closing it frees the slot, so a fresh tunnel opens on the same connection.
    first.close().await.unwrap();
    // The server reaps the finished tunnel on its next accept; give it a beat.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut third = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .expect("a slot freed by a closed tunnel is reusable");
    round_trip(&mut third, b"three", b"echo:three").await;
}

#[tokio::test]
async fn a_tunnel_that_goes_silent_is_reclaimed_after_the_idle_timeout() {
    let echo = spawn_echo().await;
    let proxy = spawn_proxy(ResourceLimits {
        tunnel_idle_timeout: Some(Duration::from_secs(1)),
        ..ResourceLimits::unlimited()
    });
    let session = connect(&proxy).await;

    let mut tunnel = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    // It works while it is active.
    round_trip(&mut tunnel, b"hi", b"echo:hi").await;

    // Then say nothing for longer than the idle timeout.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // The next read observes the proxy having closed the tunnel.
    let after_idle = timeout(REPLY_TIMEOUT, tunnel.read())
        .await
        .expect("timed out waiting for the idle close");
    match after_idle {
        Ok(None) => {}
        Err(_) => {}
        Ok(Some(bytes)) => panic!("expected the idle tunnel to be closed, got {bytes:?}"),
    }
}

#[tokio::test]
async fn new_connections_past_the_rate_are_refused() {
    // A burst of one and no refill: the second connection in quick succession
    // has no token.
    let proxy = spawn_proxy(ResourceLimits {
        connection_rate: Some(ConnectionRate {
            per_second: 0,
            burst: 1,
        }),
        ..ResourceLimits::unlimited()
    });

    // The first connection spends the single token; its handshake completing is
    // the proof it was accepted. Keep it alive so it stays counted.
    let _first = connect(&proxy).await;

    // A second connection, opened straight away, is turned down by the accept
    // loop before its handshake completes.
    let client_tls = tls::client_config_with_ca(proxy.certificate_pem.as_bytes()).unwrap();
    let client = Client::new(client_tls).unwrap();
    let template =
        UriTemplate::default_connect_udp(&format!("localhost:{}", proxy.addr.port())).unwrap();
    let refused = timeout(REPLY_TIMEOUT, client.connect(proxy.addr, template))
        .await
        .expect("connect neither succeeded nor failed within the timeout");
    assert!(
        refused.is_err(),
        "a connection opened over the rate limit must be refused"
    );
}

#[tokio::test]
async fn new_connections_past_the_per_source_rate_are_refused() {
    // Every connection in this test comes from 127.0.0.1, so a per-source burst
    // of one behaves like the global limit above -- which is the point: the
    // accept loop consults the per-source limiter before the global one.
    let proxy = spawn_proxy(ResourceLimits {
        per_source_rate: Some(ConnectionRate {
            per_second: 0,
            burst: 1,
        }),
        ..ResourceLimits::unlimited()
    });

    let _first = connect(&proxy).await;

    let client_tls = tls::client_config_with_ca(proxy.certificate_pem.as_bytes()).unwrap();
    let client = Client::new(client_tls).unwrap();
    let template =
        UriTemplate::default_connect_udp(&format!("localhost:{}", proxy.addr.port())).unwrap();
    let refused = timeout(REPLY_TIMEOUT, client.connect(proxy.addr, template))
        .await
        .expect("connect neither succeeded nor failed within the timeout");
    assert!(
        refused.is_err(),
        "a second connection from the same source over its rate must be refused"
    );
}

#[tokio::test]
async fn an_idle_timeout_does_not_disturb_a_tunnel_that_keeps_talking() {
    let echo = spawn_echo().await;
    let proxy = spawn_proxy(ResourceLimits {
        tunnel_idle_timeout: Some(Duration::from_secs(1)),
        ..ResourceLimits::unlimited()
    });
    let session = connect(&proxy).await;

    let mut tunnel = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();

    // Traffic every 300ms keeps the 1s idle timer from ever firing.
    for i in 0..6 {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let msg = format!("n{i}");
        round_trip(&mut tunnel, msg.as_bytes(), format!("echo:{msg}").as_bytes()).await;
    }

    tunnel.close().await.unwrap();
}
