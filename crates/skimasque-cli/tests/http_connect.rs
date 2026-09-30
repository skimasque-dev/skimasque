//! End-to-end tests of the HTTP CONNECT front end: raw HTTP/1.1 bytes, the
//! front end, a real MASQUE gateway with a policy, and a real TCP echo target.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use skimasque::client::{Client, Session};
use skimasque::policy::AddressPolicy;
use skimasque::server::{ProxyConfig, Server};
use skimasque::service::{Dispatch, PolicyLayer, TcpProxy};
use skimasque::tls;
use skimasque_cli::http_connect;
use skimasque_core::UriTemplate;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tower::ServiceBuilder;

const T: Duration = Duration::from_secs(10);

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

/// A gateway allowing `curl` to `allowed` only, and the front end before it.
async fn spawn_front(allowed: SocketAddr) -> (SocketAddr, Arc<Session>) {
    let policy = format!(
        "name = \"t\"\n[[rules]]\napplication = \"curl\"\naction = \"allow\"\ndestinations = [\"127.0.0.1:{}\"]\n",
        allowed.port()
    );
    let set =
        skimasque::policy_engine::PolicySet::from_documents([("p.toml", policy.as_str())]).unwrap();
    let service = ServiceBuilder::new()
        .layer(PolicyLayer::new(set))
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
    let gw = server.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.run().await;
    });

    let client_tls = tls::client_config_with_ca(generated.certificate_pem.as_bytes()).unwrap();
    let template = UriTemplate::default_connect_udp(&format!("localhost:{}", gw.port())).unwrap();
    let mut headers = http::HeaderMap::new();
    headers.insert(
        skimasque::APPLICATION_HEADER,
        http::HeaderValue::from_static("curl"),
    );
    let session = Client::new(client_tls)
        .unwrap()
        .connect(gw, template)
        .await
        .unwrap()
        .with_default_headers(headers);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let front = listener.local_addr().unwrap();
    let session = Arc::new(session);
    let served = session.clone();
    tokio::spawn(async move {
        let _ = http_connect::serve(listener, served).await;
    });
    (front, session)
}

async fn read_head(s: &mut TcpStream) -> String {
    let mut got = Vec::new();
    let mut byte = [0u8; 1];
    while !got.ends_with(b"\r\n\r\n") {
        timeout(T, s.read_exact(&mut byte))
            .await
            .expect("timed out")
            .unwrap();
        got.push(byte[0]);
    }
    String::from_utf8(got).unwrap()
}

#[tokio::test]
async fn an_allowed_connect_is_established_and_carries_bytes() {
    let echo = spawn_echo(b"ok:").await;
    let (front, _) = spawn_front(echo).await;
    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(format!("CONNECT {echo} HTTP/1.1\r\nHost: {echo}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    assert!(read_head(&mut s).await.starts_with("HTTP/1.1 200 "));
    s.write_all(b"hi").await.unwrap();
    let mut reply = [0u8; 5];
    timeout(T, s.read_exact(&mut reply)).await.unwrap().unwrap();
    assert_eq!(&reply, b"ok:hi");
}

#[tokio::test]
async fn bytes_sent_with_the_connect_head_reach_the_destination() {
    let echo = spawn_echo(b"ok:").await;
    let (front, _) = spawn_front(echo).await;
    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(format!("CONNECT {echo} HTTP/1.1\r\n\r\nearly").as_bytes())
        .await
        .unwrap();
    assert!(read_head(&mut s).await.starts_with("HTTP/1.1 200 "));
    let mut reply = [0u8; 8];
    timeout(T, s.read_exact(&mut reply)).await.unwrap().unwrap();
    assert_eq!(&reply, b"ok:early");
}

#[tokio::test]
async fn a_policy_denial_answers_403_with_the_reason() {
    let echo = spawn_echo(b"ok:").await;
    let other = spawn_echo(b"no:").await;
    let (front, _) = spawn_front(echo).await;
    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(format!("CONNECT {other} HTTP/1.1\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let head = read_head(&mut s).await;
    assert!(head.starts_with("HTTP/1.1 403 "), "{head}");
}

#[tokio::test]
async fn other_methods_get_405_and_garbage_gets_400() {
    let echo = spawn_echo(b"ok:").await;
    let (front, _) = spawn_front(echo).await;
    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(b"GET http://example.com/ HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    assert!(read_head(&mut s).await.starts_with("HTTP/1.1 405 "));

    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(b"CONNECT nonsense HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    assert!(read_head(&mut s).await.starts_with("HTTP/1.1 400 "));

    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(&vec![b'a'; http_connect::MAX_HEAD + 1])
        .await
        .unwrap();
    assert!(read_head(&mut s).await.starts_with("HTTP/1.1 400 "));
}

#[tokio::test]
async fn a_forward_listener_splices_to_its_destination() {
    use skimasque_cli::forward::{self, ForwardSpec};
    let echo = spawn_echo(b"fw:").await;
    let (_, session) = spawn_front(echo).await;
    let spec = ForwardSpec::parse(&echo.to_string()).unwrap();
    let listener = forward::bind(&spec).await.unwrap();
    let local = listener.local_addr().unwrap();
    assert!(local.ip().is_loopback());
    tokio::spawn(forward::serve(listener, spec.target.clone(), session));
    let mut s = TcpStream::connect(local).await.unwrap();
    s.write_all(b"hi").await.unwrap();
    let mut reply = [0u8; 5];
    timeout(T, s.read_exact(&mut reply)).await.unwrap().unwrap();
    assert_eq!(&reply, b"fw:hi");
}
