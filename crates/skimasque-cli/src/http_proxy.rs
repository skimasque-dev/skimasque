//! Streaming HTTP/1 proxy. All upstream sockets are MASQUE TCP tunnels.
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, Uri};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use skimasque::client::Session;
use skimasque_core::connect_udp::Target;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tracing::debug;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type Body = UnsyncBoxBody<Bytes, BoxError>;
const OPEN_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn serve(listener: TcpListener, session: Arc<Session>) -> std::io::Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        let session = session.clone();
        tokio::spawn(async move {
            let service = service_fn(move |request| {
                let session = session.clone();
                async move { Ok::<_, Infallible>(handle(request, session).await) }
            });
            let mut builder = hyper::server::conn::http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .header_read_timeout(Duration::from_secs(15))
                .max_buf_size(16 * 1024);
            if let Err(error) = builder
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades()
                .await
            {
                debug!(%peer, %error, "HTTP proxy connection ended");
            }
        });
    }
}

fn small(status: StatusCode, text: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(
            Full::new(Bytes::from_static(text.as_bytes()))
                .map_err(|never| match never {})
                .boxed_unsync(),
        )
        .expect("literal response")
}

/// Connection-nominated headers are hop-by-hop too. Capture names before removal.
fn strip_hop_headers(headers: &mut HeaderMap) {
    let nominated: Vec<String> = headers
        .get_all(http::header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|v| v.trim().to_owned())
        .collect();
    for name in nominated {
        headers.remove(name);
    }
    for name in [
        "connection",
        "proxy-authorization",
        "proxy-authenticate",
        "proxy-connection",
        "keep-alive",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
}

fn prepare_request(
    mut request: Request<Incoming>,
) -> Result<(Target, Request<Incoming>), StatusCode> {
    let uri = request.uri();
    if uri.scheme_str() != Some("http") || request.headers().contains_key("upgrade") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let authority = uri
        .authority()
        .ok_or(StatusCode::BAD_REQUEST)?
        .as_str()
        .to_owned();
    if authority.contains('@') || uri.to_string().contains('#') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let endpoint = if uri.port().is_some() {
        authority.clone()
    } else {
        format!("{authority}:80")
    };
    let target = Target::parse(&endpoint).map_err(|_| StatusCode::BAD_REQUEST)?;
    let path: Uri = uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    strip_hop_headers(request.headers_mut());
    request.headers_mut().insert(
        http::header::HOST,
        HeaderValue::from_str(&authority).map_err(|_| StatusCode::BAD_REQUEST)?,
    );
    *request.uri_mut() = path;
    Ok((target, request))
}

async fn handle(mut request: Request<Incoming>, session: Arc<Session>) -> Response<Body> {
    if request.method() == Method::CONNECT {
        let Some(authority) = request.uri().authority().map(|a| a.as_str()) else {
            return small(StatusCode::BAD_REQUEST, "CONNECT requires host:port\n");
        };
        let Ok(target) = Target::parse(authority) else {
            return small(StatusCode::BAD_REQUEST, "CONNECT requires host:port\n");
        };
        let tunnel = match timeout(OPEN_TIMEOUT, session.connect_tcp(target)).await {
            Ok(Ok(tunnel)) => tunnel,
            _ => {
                return small(
                    StatusCode::BAD_GATEWAY,
                    "Gateway could not open the destination\n",
                )
            }
        };
        let upgraded = hyper::upgrade::on(&mut request);
        tokio::spawn(async move {
            if let Ok(Ok(io)) = timeout(OPEN_TIMEOUT, upgraded).await {
                let _ = tunnel.relay(TokioIo::new(io)).await;
            }
        });
        return small(StatusCode::OK, "");
    }
    let (target, request) = match prepare_request(request) {
        Ok(value) => value,
        Err(status) => return small(status, "An absolute http:// destination is required\n"),
    };
    let tunnel = match timeout(OPEN_TIMEOUT, session.connect_tcp(target)).await {
        Ok(Ok(tunnel)) => tunnel,
        _ => {
            return small(
                StatusCode::BAD_GATEWAY,
                "Gateway could not open the destination\n",
            )
        }
    };
    let (local, upstream) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let _ = tunnel.relay(upstream).await;
    });
    let Ok(Ok((mut sender, connection))) = timeout(
        OPEN_TIMEOUT,
        hyper::client::conn::http1::handshake(TokioIo::new(local)),
    )
    .await
    else {
        return small(StatusCode::BAD_GATEWAY, "Upstream connection failed\n");
    };
    tokio::spawn(async move {
        let _ = connection.await;
    });
    match timeout(OPEN_TIMEOUT, sender.send_request(request)).await {
        Ok(Ok(mut response)) => {
            strip_hop_headers(response.headers_mut());
            response.map(|body| body.map_err(|e| -> BoxError { Box::new(e) }).boxed_unsync())
        }
        _ => small(StatusCode::BAD_GATEWAY, "Upstream response failed\n"),
    }
}
