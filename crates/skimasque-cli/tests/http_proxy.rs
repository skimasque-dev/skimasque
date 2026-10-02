//! Real HTTP client -> HTTP proxy -> MASQUE gateway -> HTTP/TCP origin tests.
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use skimasque::client::{Client, Session};
use skimasque::policy::AddressPolicy;
use skimasque::server::{ProxyConfig, Server};
use skimasque::service::{Dispatch, TcpProxy};
use skimasque::tls;
use skimasque_cli::http_proxy;
use skimasque_core::UriTemplate;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::timeout;

struct Fixture {
    proxy: SocketAddr,
    tasks: Vec<JoinHandle<()>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn fixture() -> Fixture {
    fixture_with_policy(AddressPolicy::permissive()).await
}

async fn fixture_with_policy(address_policy: AddressPolicy) -> Fixture {
    let generated = tls::generate_self_signed(vec!["localhost".into()]).unwrap();
    let server = Server::bind(
        "127.0.0.1:0".parse().unwrap(),
        tls::server_config_from_pem(
            generated.certificate_pem.as_bytes(),
            generated.key_pem.as_bytes(),
        )
        .unwrap(),
        Dispatch::new().with_tcp(TcpProxy::new(address_policy)),
        ProxyConfig::new("localhost").unwrap(),
    )
    .unwrap();
    let gateway = server.local_addr().unwrap();
    let driver = tokio::spawn(async move {
        let _ = server.run().await;
    });
    let session: Session =
        Client::new(tls::client_config_with_ca(generated.certificate_pem.as_bytes()).unwrap())
            .unwrap()
            .connect(
                gateway,
                UriTemplate::default_connect_udp(&format!("localhost:{}", gateway.port())).unwrap(),
            )
            .await
            .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = listener.local_addr().unwrap();
    let relay = tokio::spawn(async move {
        let _ = http_proxy::serve(listener, Arc::new(session)).await;
    });
    Fixture {
        proxy,
        tasks: vec![driver, relay],
    }
}

#[tokio::test]
async fn gateway_denials_remain_forbidden_for_http_and_connect() {
    let f = fixture_with_policy(AddressPolicy::default()).await;
    for head in [
        "GET http://127.0.0.1:1/ HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        "CONNECT 127.0.0.1:1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    ] {
        let mut stream = TcpStream::connect(f.proxy).await.unwrap();
        stream.write_all(head.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        timeout(Duration::from_secs(10), stream.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    }
}

async fn origin(tag: &'static str) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let service =
                    service_fn(move |request: Request<hyper::body::Incoming>| async move {
                        assert_eq!(request.headers()["host"], addr.to_string());
                        assert!(!request.headers().contains_key("proxy-authorization"));
                        assert!(!request.headers().contains_key("x-strip"));
                        let path = request.uri().to_string();
                        let body = request.into_body().collect().await.unwrap().to_bytes();
                        let mut reply = format!("{tag}:{path}:").into_bytes();
                        reply.extend_from_slice(&body);
                        Ok::<_, std::convert::Infallible>(
                            Response::builder()
                                .header("connection", "x-secret")
                                .header("x-secret", "do-not-forward")
                                .body(Full::new(Bytes::from(reply)))
                                .unwrap(),
                        )
                    });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    (addr, task)
}

#[tokio::test]
async fn ordinary_http_rewrites_authority_and_streams_large_bodies_without_proxy_headers() {
    let mut f = fixture().await;
    let (addr, origin_task) = origin("echo").await;
    f.tasks.push(origin_task);
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(format!("http://{}", f.proxy)).unwrap())
        .build()
        .unwrap();
    let body = "x".repeat(1024 * 1024 + 7);
    let response = timeout(
        Duration::from_secs(15),
        client
            .post(format!("http://{addr}/upload?q=1"))
            .header("host", "wrong.example")
            .header("proxy-authorization", "Bearer secret")
            .header("connection", "x-strip")
            .header("x-strip", "secret")
            .body(body.clone())
            .send(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.status(), 200);
    assert!(!response.headers().contains_key("x-secret"));
    assert_eq!(
        response.text().await.unwrap(),
        format!("echo:/upload?q=1:{body}")
    );
}

#[tokio::test]
async fn one_proxy_connection_can_reach_different_authorities() {
    let mut f = fixture().await;
    let (first, t1) = origin("first").await;
    let (second, t2) = origin("second").await;
    f.tasks.extend([t1, t2]);
    let socket = TcpStream::connect(f.proxy).await.unwrap();
    let (mut client, connection) = hyper::client::conn::http1::handshake(TokioIo::new(socket))
        .await
        .unwrap();
    f.tasks.push(tokio::spawn(async move {
        let _ = connection.await;
    }));
    for (addr, want) in [(first, "first:/:"), (second, "second:/:")] {
        let request = Request::builder()
            .uri(format!("http://{addr}/"))
            .body(Full::new(Bytes::new()))
            .unwrap();
        let response = client.send_request(request).await.unwrap();
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            want
        );
    }
}

#[tokio::test]
async fn connect_preserves_payload_sent_with_the_request_head() {
    let f = fixture().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let echo = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut payload = [0u8; 5];
        socket.read_exact(&mut payload).await.unwrap();
        socket.write_all(&payload).await.unwrap();
        socket.shutdown().await.unwrap();
    });
    let mut socket = TcpStream::connect(f.proxy).await.unwrap();
    socket
        .write_all(format!("CONNECT {addr} HTTP/1.1\r\nHost: {addr}\r\n\r\nhello").as_bytes())
        .await
        .unwrap();
    let mut reply = Vec::new();
    timeout(Duration::from_secs(10), socket.read_to_end(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert!(reply.starts_with(b"HTTP/1.1 200"), "{reply:?}");
    assert!(reply.ends_with(b"hello"));
    echo.await.unwrap();
}

#[tokio::test]
async fn malformed_unsupported_and_refused_requests_return_errors() {
    let f = fixture().await;
    for (request, status) in [
        ("GET /relative HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n", "400"),
        ("GET https://example.com/ HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n", "400"),
        ("CONNECT missing-port HTTP/1.1\r\nHost: missing-port\r\nConnection: close\r\n\r\n", "400"),
        ("GET http://user:secret@example.com/ HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n", "400"),
        ("GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\nUpgrade: websocket\r\nConnection: close\r\n\r\n", "400"),
        ("GET http://127.0.0.1:1/ HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", "502"),
    ] {
        let mut socket = TcpStream::connect(f.proxy).await.unwrap();
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut reply = Vec::new();
        timeout(Duration::from_secs(10), socket.read_to_end(&mut reply)).await.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&reply).starts_with(&format!("HTTP/1.1 {status}")), "{reply:?}");
    }
}

#[tokio::test]
async fn chunked_request_and_response_are_reframed() {
    let f = fixture().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = listener.local_addr().unwrap();
    let target = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut byte = [0; 1];
        while !request.ends_with(b"0\r\n\r\n") {
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }
        assert!(String::from_utf8_lossy(&request).starts_with("POST /chunk HTTP/1.1"));
        assert!(request.windows(5).any(|s| s == b"hello"));
        stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nworld\r\n0\r\n\r\n").await.unwrap();
    });
    let mut stream = TcpStream::connect(f.proxy).await.unwrap();
    stream.write_all(format!("POST http://{origin}/chunk HTTP/1.1\r\nHost: {origin}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n").as_bytes()).await.unwrap();
    let mut response = Vec::new();
    timeout(Duration::from_secs(10), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert!(response.windows(5).any(|s| s == b"world"));
    target.await.unwrap();
}

#[tokio::test]
async fn truncated_upstream_body_is_not_reported_as_complete() {
    let f = fixture().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let target = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0; 1024];
        let read = stream.read(&mut buf).await.unwrap();
        assert!(read > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort")
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(format!("http://{}", f.proxy)).unwrap())
        .build()
        .unwrap();
    if let Ok(response) = client.get(format!("http://{addr}/")).send().await {
        assert!(response.bytes().await.is_err());
    }
    target.await.unwrap();
}

#[tokio::test]
async fn delayed_response_headers_can_outlive_connection_open_timeout() {
    let f = fixture().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let target = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 1024];
        assert!(stream.read(&mut request).await.unwrap() > 0);
        tokio::time::sleep(Duration::from_secs(31)).await;
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await;
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(format!("http://{}", f.proxy)).unwrap())
        .build()
        .unwrap();
    let response = timeout(
        Duration::from_secs(45),
        client.get(format!("http://{addr}/")).send(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "ok");
    target.await.unwrap();
}

#[tokio::test]
async fn progressing_upload_can_outlive_connection_open_timeout() {
    let f = fixture().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let target = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"0\r\n\r\n") {
            if stream.read_exact(&mut byte).await.is_err() {
                return;
            }
            request.push(byte[0]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
    });
    let mut stream = TcpStream::connect(f.proxy).await.unwrap();
    stream.write_all(format!("POST http://{addr}/ HTTP/1.1\r\nHost: {addr}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    for _ in 0..32 {
        stream.write_all(b"1\r\nx\r\n").await.unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    stream.write_all(b"0\r\n\r\n").await.unwrap();
    let mut response = Vec::new();
    timeout(Duration::from_secs(10), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(
        response.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&response)
    );
    target.await.unwrap();
}
