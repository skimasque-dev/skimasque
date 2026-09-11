//! End-to-end tests for CONNECT-TCP: a real QUIC connection, a real HTTP/3
//! classic `CONNECT`, and a real TCP connection to a loopback target.

use std::net::SocketAddr;
use std::time::Duration;

use skimasque::client::{Client, Session};
use skimasque::policy::AddressPolicy;
use skimasque::server::{ProxyConfig, Server};
use skimasque::service::{
    Accepted, Dispatch, PolicyHandle, PolicyLayer, QuotaLayer, Rejection, TcpProxy, TunnelRequest,
};
use skimasque::tls;
use skimasque_core::target::Target;
use skimasque_core::UriTemplate;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tower::{Service, ServiceBuilder};

const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// A TCP server that prefixes `tag` to every chunk it reads back, and closes
/// its side when it sees `b"bye"`.
async fn spawn_echo(tag: &'static [u8]) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
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
                    if buf[..n].windows(3).any(|w| w == b"bye") {
                        let _ = stream.shutdown().await;
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

fn spawn_proxy<S>(service: S) -> Proxy
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection> + Clone + Send + 'static,
    S::Future: Send,
{
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
    Proxy {
        addr,
        certificate_pem: generated.certificate_pem,
    }
}

fn tcp_only_proxy(policy: AddressPolicy) -> Proxy {
    spawn_proxy(Dispatch::new().with_tcp(TcpProxy::new(policy)))
}

async fn connect(proxy: &Proxy) -> Session {
    let client_tls = tls::client_config_with_ca(proxy.certificate_pem.as_bytes()).unwrap();
    let client = Client::new(client_tls).unwrap();
    let template =
        UriTemplate::default_connect_udp(&format!("localhost:{}", proxy.addr.port())).unwrap();
    client.connect(proxy.addr, template).await.unwrap()
}

#[tokio::test]
async fn bytes_reach_the_target_and_come_back() {
    let echo = spawn_echo(b"echo:").await;
    let proxy = tcp_only_proxy(AddressPolicy::permissive());
    let session = connect(&proxy).await;

    let mut tunnel = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();

    tunnel.write(b"hello").await.unwrap();
    let reply = timeout(REPLY_TIMEOUT, tunnel.read())
        .await
        .expect("timed out")
        .expect("tunnel error")
        .expect("tunnel closed early");
    assert_eq!(&reply[..], b"echo:hello");

    tunnel.close().await.unwrap();
}

#[tokio::test]
async fn the_target_closing_its_side_surfaces_as_eof() {
    let echo = spawn_echo(b"e:").await;
    let proxy = tcp_only_proxy(AddressPolicy::permissive());
    let session = connect(&proxy).await;

    let mut tunnel = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();

    // `bye` makes the echo server reply once and then close.
    tunnel.write(b"bye").await.unwrap();
    let reply = timeout(REPLY_TIMEOUT, tunnel.read())
        .await
        .expect("timed out")
        .expect("tunnel error")
        .expect("tunnel closed early");
    assert_eq!(&reply[..], b"e:bye");

    // The next read observes the target's FIN.
    let eof = timeout(REPLY_TIMEOUT, tunnel.read())
        .await
        .expect("timed out")
        .expect("tunnel error");
    assert!(eof.is_none(), "expected EOF after the target closed");
}

#[tokio::test]
async fn the_relay_helper_bridges_a_local_socket() {
    let echo = spawn_echo(b"echo:").await;
    let proxy = tcp_only_proxy(AddressPolicy::permissive());
    let session = connect(&proxy).await;

    // A local listener stands in for whatever a front end would hand `relay`.
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_addr = local.local_addr().unwrap();

    let tunnel = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    let relay = tokio::spawn(async move {
        let (server_side, _) = local.accept().await.unwrap();
        tunnel.relay(server_side).await
    });

    let mut client_side = TcpStream::connect(local_addr).await.unwrap();
    client_side.write_all(b"through").await.unwrap();
    let mut buf = vec![0u8; 64];
    let n = timeout(REPLY_TIMEOUT, client_side.read(&mut buf))
        .await
        .expect("timed out")
        .unwrap();
    assert_eq!(&buf[..n], b"echo:through");

    drop(client_side);
    let _ = timeout(REPLY_TIMEOUT, relay).await;
}

#[tokio::test]
async fn the_address_policy_refuses_a_prohibited_destination() {
    let echo = spawn_echo(b"echo:").await;
    // The default policy bans loopback, where the echo server lives.
    let proxy = tcp_only_proxy(AddressPolicy::default());
    let session = connect(&proxy).await;

    let error = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap_err();
    match error {
        skimasque::Error::Rejected { status, .. } => {
            assert_eq!(status, http::StatusCode::FORBIDDEN);
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn a_connection_to_a_dead_port_is_a_bad_gateway() {
    // Bind then drop, so the port is almost certainly closed.
    let dead = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);

    let proxy = tcp_only_proxy(AddressPolicy::permissive());
    let session = connect(&proxy).await;

    let error = session
        .connect_tcp(Target::parse(&dead_addr.to_string()).unwrap())
        .await
        .unwrap_err();
    match error {
        skimasque::Error::Rejected {
            status,
            proxy_status,
        } => {
            assert_eq!(status, http::StatusCode::BAD_GATEWAY);
            assert!(
                proxy_status
                    .as_deref()
                    .is_some_and(|s| s.contains("connection_refused")),
                "unexpected Proxy-Status: {proxy_status:?}"
            );
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn the_policy_layer_covers_tcp() {
    let allowed = spawn_echo(b"ok:").await;
    let blocked = spawn_echo(b"no:").await;

    let policy = format!(
        r#"
        name = "test"
        [[rules]]
        application = "curl"
        action = "allow"
        destinations = ["127.0.0.1:{}"]
    "#,
        allowed.port()
    );
    let set =
        skimasque::policy_engine::PolicySet::from_documents([("p.toml", policy.as_str())]).unwrap();

    let service = ServiceBuilder::new()
        .layer(PolicyLayer::new(set))
        .service(Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive())));
    let proxy = spawn_proxy(service);

    let mut headers = http::HeaderMap::new();
    headers.insert(
        skimasque::APPLICATION_HEADER,
        http::HeaderValue::from_static("curl"),
    );
    let session = connect(&proxy).await.with_default_headers(headers);

    // The listed destination opens.
    let mut tunnel = session
        .connect_tcp(Target::parse(&allowed.to_string()).unwrap())
        .await
        .unwrap();
    tunnel.write(b"hi").await.unwrap();
    let reply = timeout(REPLY_TIMEOUT, tunnel.read())
        .await
        .expect("timed out")
        .expect("tunnel error")
        .expect("closed early");
    assert_eq!(&reply[..], b"ok:hi");

    // An unlisted one is denied by policy, not by the network floor.
    match session
        .connect_tcp(Target::parse(&blocked.to_string()).unwrap())
        .await
        .unwrap_err()
    {
        skimasque::Error::Rejected { status, .. } => {
            assert_eq!(status, http::StatusCode::FORBIDDEN);
        }
        other => panic!("expected a policy denial, got {other:?}"),
    }
}

#[tokio::test]
async fn a_transport_scoped_rule_denies_the_other_transport() {
    let echo = spawn_echo(b"ok:").await;

    // `curl` may reach the echo target over TCP only.
    let policy = format!(
        r#"
        name = "test"
        [[rules]]
        application = "curl"
        transport = "tcp"
        action = "allow"
        destinations = ["127.0.0.1:{}"]
    "#,
        echo.port()
    );
    let set =
        skimasque::policy_engine::PolicySet::from_documents([("p.toml", policy.as_str())]).unwrap();

    let service = ServiceBuilder::new()
        .layer(PolicyLayer::new(set))
        .service(
            Dispatch::new()
                .with_tcp(TcpProxy::new(AddressPolicy::permissive()))
                .with_udp(skimasque::service::UdpProxy::new(AddressPolicy::permissive())),
        );
    let proxy = spawn_proxy(service);

    let mut headers = http::HeaderMap::new();
    headers.insert(
        skimasque::APPLICATION_HEADER,
        http::HeaderValue::from_static("curl"),
    );
    let session = connect(&proxy).await.with_default_headers(headers);
    let target = Target::parse(&echo.to_string()).unwrap();

    // TCP to the target is allowed.
    let mut tunnel = session.connect_tcp(target.clone()).await.unwrap();
    tunnel.write(b"hi").await.unwrap();
    let reply = timeout(REPLY_TIMEOUT, tunnel.read())
        .await
        .expect("timed out")
        .expect("tunnel error")
        .expect("closed early");
    assert_eq!(&reply[..], b"ok:hi");

    // UDP to the same host:port is denied -- the rule is `transport = "tcp"`.
    match session.connect_udp(target).await.unwrap_err() {
        skimasque::Error::Rejected { status, .. } => {
            assert_eq!(status, http::StatusCode::FORBIDDEN)
        }
        other => panic!("expected a transport-scoped denial, got {other:?}"),
    }
}

/// A one-rule policy set: `curl` may reach a single loopback port.
fn allow_curl_to(port: u16) -> skimasque::policy_engine::PolicySet {
    let toml = format!(
        r#"
        name = "test"
        [[rules]]
        application = "curl"
        action = "allow"
        destinations = ["127.0.0.1:{port}"]
    "#
    );
    skimasque::policy_engine::PolicySet::from_documents([("p.toml", toml.as_str())]).unwrap()
}

#[tokio::test]
async fn a_policy_reload_rebinds_new_tunnels_without_disturbing_open_ones() {
    let first = spawn_echo(b"first:").await;
    let second = spawn_echo(b"second:").await;

    // Start enforcing "curl may reach `first`, nothing else".
    let layer = PolicyLayer::new(allow_curl_to(first.port()));
    let handle: PolicyHandle = layer.handle();
    let service = ServiceBuilder::new()
        .layer(layer)
        .service(Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive())));
    let proxy = spawn_proxy(service);

    let mut headers = http::HeaderMap::new();
    headers.insert(
        skimasque::APPLICATION_HEADER,
        http::HeaderValue::from_static("curl"),
    );
    let session = connect(&proxy).await.with_default_headers(headers);

    // A tunnel to `first` opens and works under the original policy.
    let mut open = session
        .connect_tcp(Target::parse(&first.to_string()).unwrap())
        .await
        .unwrap();
    open.write(b"one").await.unwrap();
    let reply = timeout(REPLY_TIMEOUT, open.read())
        .await
        .expect("timed out")
        .expect("tunnel error")
        .expect("closed early");
    assert_eq!(&reply[..], b"first:one");

    // Swap the policy: `curl` may now reach `second` and no longer `first`.
    handle.store(allow_curl_to(second.port()));

    // The tunnel that was already open is undisturbed by the swap.
    open.write(b"two").await.unwrap();
    let reply = timeout(REPLY_TIMEOUT, open.read())
        .await
        .expect("timed out")
        .expect("tunnel error")
        .expect("closed early");
    assert_eq!(&reply[..], b"first:two");

    // A new tunnel to the now-unlisted `first` is denied.
    match session
        .connect_tcp(Target::parse(&first.to_string()).unwrap())
        .await
        .unwrap_err()
    {
        skimasque::Error::Rejected { status, .. } => {
            assert_eq!(status, http::StatusCode::FORBIDDEN)
        }
        other => panic!("expected a denial after the reload, got {other:?}"),
    }

    // A new tunnel to the newly-listed `second` opens.
    let mut fresh = session
        .connect_tcp(Target::parse(&second.to_string()).unwrap())
        .await
        .unwrap();
    fresh.write(b"three").await.unwrap();
    let reply = timeout(REPLY_TIMEOUT, fresh.read())
        .await
        .expect("timed out")
        .expect("tunnel error")
        .expect("closed early");
    assert_eq!(&reply[..], b"second:three");
}

/// `curl` may reach `port`, with an optional `[limits]` block (given verbatim).
fn allow_curl_with_limits(port: u16, limits: &str) -> skimasque::policy_engine::PolicySet {
    let toml = format!(
        r#"
        name = "test"
        {limits}
        [[rules]]
        application = "curl"
        action = "allow"
        destinations = ["127.0.0.1:{port}"]
    "#
    );
    skimasque::policy_engine::PolicySet::from_documents([("p.toml", toml.as_str())]).unwrap()
}

/// Write `payload`, then read until at least `payload.len()` bytes have come
/// back, returning how long that took.
async fn timed_echo(tunnel: &mut skimasque::client::TcpTunnel, payload: &[u8]) -> std::time::Duration {
    let start = std::time::Instant::now();
    tunnel.write(payload).await.unwrap();
    let mut received = 0usize;
    while received < payload.len() {
        let chunk = timeout(std::time::Duration::from_secs(20), tunnel.read())
            .await
            .expect("timed out")
            .expect("tunnel error")
            .expect("closed early");
        received += chunk.len();
    }
    start.elapsed()
}

#[tokio::test]
async fn a_bandwidth_limited_tunnel_is_paced_and_an_unlimited_one_is_not() {
    let echo = spawn_echo(b"").await;

    let build = |set| {
        ServiceBuilder::new()
            .layer(PolicyLayer::new(set))
            .layer(QuotaLayer::new())
            .service(Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive())))
    };
    let mut headers = http::HeaderMap::new();
    headers.insert(
        skimasque::APPLICATION_HEADER,
        http::HeaderValue::from_static("curl"),
    );

    // 256 KiB out and 256 KiB echoed back is 512 KiB through the limiter; at
    // 1 Mbps (125 000 B/s aggregate), minus one second of burst allowance, that
    // is roughly three seconds.
    let payload = vec![0x5a_u8; 256 * 1024];

    let limited = spawn_proxy(build(allow_curl_with_limits(
        echo.port(),
        "[limits]\nbandwidth = \"1Mbps\"\n",
    )));
    let session = connect(&limited).await.with_default_headers(headers.clone());
    let mut tunnel = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    let paced = timed_echo(&mut tunnel, &payload).await;

    let open = spawn_proxy(build(allow_curl_with_limits(echo.port(), "")));
    let session = connect(&open).await.with_default_headers(headers);
    let mut tunnel = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    let unpaced = timed_echo(&mut tunnel, &payload).await;

    assert!(
        paced >= std::time::Duration::from_millis(2_000),
        "the 1 Mbps tunnel should take ~3s for 512 KiB, took {paced:?}"
    );
    assert!(
        unpaced < std::time::Duration::from_millis(1_000),
        "an unlimited tunnel should finish well under a second, took {unpaced:?}"
    );
}

#[tokio::test]
async fn a_tunnel_is_cut_off_at_its_total_bytes_ceiling() {
    let echo = spawn_echo(b"").await;
    // 96 KiB total: a 64 KiB write is forwarded (64 KiB counted), then the echo
    // back trips the ceiling around 32 KiB in.
    let service = ServiceBuilder::new()
        .layer(PolicyLayer::new(allow_curl_with_limits(
            echo.port(),
            "[limits]\nbytes = \"96KiB\"\n",
        )))
        .layer(QuotaLayer::new())
        .service(Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive())));
    let proxy = spawn_proxy(service);

    let mut headers = http::HeaderMap::new();
    headers.insert(
        skimasque::APPLICATION_HEADER,
        http::HeaderValue::from_static("curl"),
    );
    let session = connect(&proxy).await.with_default_headers(headers);
    let mut tunnel = session
        .connect_tcp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();

    tunnel.write(&vec![0x5a_u8; 64 * 1024]).await.unwrap();

    // Read until the proxy closes the tunnel under us.
    let mut received = 0usize;
    while let Ok(Some(chunk)) = timeout(REPLY_TIMEOUT, tunnel.read()).await.expect("timed out") {
        received += chunk.len();
    }
    assert!(
        received > 0 && received < 64 * 1024,
        "the tunnel should be cut off partway through the 64 KiB echo, got {received} bytes"
    );
}
