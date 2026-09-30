//! A minimal HTTP/1.1 `CONNECT` proxy in front of the gateway session.
//!
//! `HTTPS_PROXY=http://…` is understood by far more software than
//! `socks5h://` (Go, Python, Node, curl, git), so `skimasque exec` points those
//! variables here. Each `CONNECT host:port` becomes a TCP tunnel through the
//! gateway; the name is resolved there, not here. Nothing else is proxied:
//! plain-HTTP requests are refused with `405`, and one stderr line says so.

use std::sync::Arc;

use skimasque::client::Session;
use skimasque_core::target::Target;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::debug;

use crate::session::refusal_line;

/// The largest request head accepted, in bytes.
pub const MAX_HEAD: usize = 8 * 1024;

/// What a request head asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Head {
    /// `CONNECT <authority> HTTP/1.x`.
    Connect(String),
    /// A well-formed request with another method; for an absolute-form
    /// `http://` target, its authority.
    OtherMethod(Option<String>),
    /// Anything that is not an HTTP/1.x request line.
    Malformed,
}

/// Classify a request head (the bytes up to and including the blank line).
pub fn parse_head(head: &[u8]) -> Head {
    let Ok(text) = std::str::from_utf8(head) else {
        return Head::Malformed;
    };
    let line = text.split("\r\n").next().unwrap_or_default();
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Head::Malformed;
    };
    if !version.starts_with("HTTP/1.") || target.is_empty() {
        return Head::Malformed;
    }
    if method != "CONNECT" {
        return Head::OtherMethod(plain_http_authority(target).map(str::to_owned));
    }
    Head::Connect(target.to_owned())
}

/// The authority of an absolute-form `http://` request target (without any
/// userinfo), e.g. `api.example:8080` for `http://api.example:8080/v1`.
pub fn plain_http_authority(target: &str) -> Option<&str> {
    const SCHEME: &str = "http://";
    let rest = target
        .get(..SCHEME.len())
        .filter(|scheme| scheme.eq_ignore_ascii_case(SCHEME))
        .map(|_| &target[SCHEME.len()..])?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    (!host.is_empty()).then_some(host)
}

/// Accept connections on `listener` until it fails, handling each on its own
/// task.
pub async fn serve(listener: TcpListener, session: Arc<Session>) -> std::io::Result<()> {
    loop {
        let (stream, client) = listener.accept().await?;
        let session = session.clone();
        tokio::spawn(async move {
            if let Err(error) = handle(stream, session).await {
                debug!(%client, %error, "HTTP CONNECT connection ended with an error");
            }
        });
    }
}

async fn handle(mut stream: TcpStream, session: Arc<Session>) -> anyhow::Result<()> {
    let Some((head, early)) = read_head(&mut stream).await? else {
        return respond(&mut stream, "400 Bad Request").await;
    };
    let authority = match parse_head(&head) {
        Head::Connect(authority) => authority,
        Head::OtherMethod(plain_http) => {
            if let Some(authority) = plain_http {
                eprintln!(
                    "skimasque: plain-HTTP request to {authority} refused; only HTTPS (CONNECT) \
                     goes through HTTP_PROXY. Use ALL_PROXY (socks5h) or --forward."
                );
            }
            return respond(&mut stream, "405 Method Not Allowed\r\nAllow: CONNECT").await;
        }
        Head::Malformed => return respond(&mut stream, "400 Bad Request").await,
    };
    let Ok(target) = Target::parse(&authority) else {
        return respond(&mut stream, "400 Bad Request").await;
    };
    let mut tunnel = match session.connect_tcp(target).await {
        Ok(tunnel) => tunnel,
        Err(error) => {
            eprintln!("skimasque: {}", refusal_line(&authority, &error));
            let forbidden = matches!(
                &error,
                skimasque::Error::Rejected { status, .. } if *status == http::StatusCode::FORBIDDEN
            );
            let status = if forbidden {
                "403 Forbidden"
            } else {
                "502 Bad Gateway"
            };
            return respond(&mut stream, status).await;
        }
    };
    stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    if !early.is_empty() {
        tunnel.write(&early).await?;
    }
    tunnel.relay(stream).await?;
    Ok(())
}

/// Read up to the end of the request head. Returns the head and any bytes the
/// client sent after it, or `None` if the head is larger than [`MAX_HEAD`] or
/// the client closed first.
async fn read_head(stream: &mut TcpStream) -> std::io::Result<Option<(Vec<u8>, Vec<u8>)>> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let early = buf.split_off(end + 4);
            return Ok(Some((buf, early)));
        }
        if buf.len() > MAX_HEAD {
            return Ok(None);
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

async fn respond(stream: &mut TcpStream, status: &str) -> anyhow::Result<()> {
    stream
        .write_all(
            format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connect_head_yields_its_authority() {
        assert_eq!(
            parse_head(b"CONNECT db.prod:5432 HTTP/1.1\r\nHost: db.prod:5432\r\n\r\n"),
            Head::Connect("db.prod:5432".into())
        );
        assert_eq!(
            parse_head(b"CONNECT [fd00::5]:443 HTTP/1.0\r\n\r\n"),
            Head::Connect("[fd00::5]:443".into())
        );
    }

    #[test]
    fn a_plain_http_target_yields_its_authority() {
        assert_eq!(plain_http_authority("http://x/"), Some("x"));
        assert_eq!(
            plain_http_authority("HTTP://api.example:8080/v1?q=1"),
            Some("api.example:8080")
        );
        assert_eq!(plain_http_authority("http://u:p@host?x"), Some("host"));
        assert_eq!(
            plain_http_authority("http://[fd00::5]:80#f"),
            Some("[fd00::5]:80")
        );
        assert_eq!(plain_http_authority("http:///path"), None);
        assert_eq!(plain_http_authority("https://x/"), None);
        assert_eq!(plain_http_authority("/index.html"), None);
    }

    #[test]
    fn other_methods_and_garbage_are_told_apart() {
        assert_eq!(
            parse_head(b"GET http://x/ HTTP/1.1\r\n\r\n"),
            Head::OtherMethod(Some("x".into()))
        );
        assert_eq!(
            parse_head(b"GET /index.html HTTP/1.1\r\n\r\n"),
            Head::OtherMethod(None)
        );
        assert_eq!(parse_head(b"CONNECT x:1\r\n\r\n"), Head::Malformed);
        assert_eq!(parse_head(b"CONNECT x:1 SPDY/3\r\n\r\n"), Head::Malformed);
        assert_eq!(parse_head(b"CONNECT  HTTP/1.1\r\n\r\n"), Head::Malformed);
        assert_eq!(parse_head(&[0xff, 0xfe]), Head::Malformed);
    }
}
