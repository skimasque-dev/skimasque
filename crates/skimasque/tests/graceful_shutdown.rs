//! Graceful shutdown: on a shutdown signal the proxy stops accepting new
//! tunnels but lets the ones already open keep running until they finish or the
//! drain deadline passes.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use skimasque::client::{Client, Session};
use skimasque::policy::AddressPolicy;
use skimasque::server::{ProxyConfig, Server};
use skimasque::service::{Dispatch, TcpProxy};
use skimasque::tls;
use skimasque_core::target::Target;
use skimasque_core::UriTemplate;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// A loopback TCP server that echoes every chunk back with `tag` prepended.
async fn spawn_echo(tag: &'static [u8]) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 4096];
                loop {
                    let n = match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    let mut reply = tag.to_vec();
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
    handle: JoinHandle<Result<(), skimasque::Error>>,
    shutdown: oneshot::Sender<()>,
    addr: SocketAddr,
    certificate_pem: String,
}

/// Bind a TCP-only proxy and drive it with [`Server::run_until`], returning a
/// handle plus the channel that triggers its shutdown.
fn spawn_proxy(grace: Duration) -> Proxy {
    let generated = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();
    let server_tls = tls::server_config_from_pem(
        generated.certificate_pem.as_bytes(),
        generated.key_pem.as_bytes(),
    )
    .unwrap();
    let service = Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive()));
    let config = ProxyConfig::new("localhost").unwrap();
    let server =
        Server::bind("127.0.0.1:0".parse().unwrap(), server_tls, service, config).unwrap();
    let addr = server.local_addr().unwrap();

    let (shutdown, rx) = oneshot::channel();
    let handle = tokio::spawn(server.run_until(
        async move {
            let _ = rx.await;
        },
        grace,
    ));

    Proxy {
        handle,
        shutdown,
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
async fn an_open_tunnel_survives_the_drain_and_new_ones_are_refused() {
    let echo = spawn_echo(b"echo:").await;
    let proxy = spawn_proxy(Duration::from_secs(10));
    let session = connect(&proxy).await;

    let mut tunnel = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    round_trip(&mut tunnel, b"before", b"echo:before").await;

    // Signal shutdown and give the GOAWAY a moment to reach the client.
    proxy.shutdown.send(()).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The tunnel that was already open keeps working during the grace period.
    round_trip(&mut tunnel, b"during", b"echo:during").await;

    // A new tunnel on the same session is refused now that GOAWAY has been sent.
    let refused = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await;
    assert!(refused.is_err(), "a new tunnel must be refused during drain");

    // Closing the last tunnel lets the drain finish well inside the 10s grace.
    tunnel.close().await.unwrap();
    let result = timeout(Duration::from_secs(5), proxy.handle)
        .await
        .expect("shutdown did not complete after the tunnel closed")
        .expect("run_until task panicked");
    result.expect("run_until returned an error");
}

#[tokio::test]
async fn the_drain_deadline_bounds_shutdown_when_a_tunnel_will_not_close() {
    let echo = spawn_echo(b"echo:").await;
    let proxy = spawn_proxy(Duration::from_secs(1));
    let session = connect(&proxy).await;

    let mut tunnel = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    round_trip(&mut tunnel, b"hi", b"echo:hi").await;

    // Trigger shutdown but never close the tunnel: the deadline must cut it off.
    proxy.shutdown.send(()).unwrap();
    let started = Instant::now();
    let result = timeout(Duration::from_secs(6), proxy.handle)
        .await
        .expect("shutdown exceeded its own deadline")
        .expect("run_until task panicked");
    result.expect("run_until returned an error");

    assert!(
        started.elapsed() < Duration::from_secs(4),
        "shutdown took {:?}, expected roughly the 1s grace",
        started.elapsed()
    );

    // The tunnel is only dropped here, so it was genuinely in flight throughout.
    drop(tunnel);
    drop(session);
}
