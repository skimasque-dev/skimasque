//! A minimal HTTP/1.1 `CONNECT` proxy in front of the gateway session.
//!
//! `HTTPS_PROXY=http://…` is understood by far more software than
//! `socks5h://` (Go, Python, Node, curl, git), so `skimasque exec` points those
//! variables here. Each `CONNECT host:port` becomes a TCP tunnel through the
//! gateway; the name is resolved there, not here.
//!
//! An absolute-form `http://` request (`GET http://host/path HTTP/1.1`, what a
//! program sends its proxy for a plain-HTTP URL) is carried too: the tunnel is
//! opened to the request's host and port, the request is rewritten to origin
//! form with `Connection: close`, and the rest is relayed raw. One connection
//! carries one exchange, so the destination the gateway authorised is the only
//! one that connection can reach. Anything else is refused with `405`.

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

/// A plain-HTTP request ready to send down a tunnel.
#[derive(Debug, PartialEq, Eq)]
pub struct Forward {
    /// `host:port` to tunnel to; port 80 when the URL named none.
    pub target: String,
    /// The request head rewritten to origin form, ending in the blank line.
    pub head: Vec<u8>,
}

/// Headers a proxy must not pass on, whatever the request says.
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "proxy-connection",
    "proxy-authorization",
    "proxy-authenticate",
    "keep-alive",
    "te",
    "upgrade",
    "host",
];

/// Rewrite an absolute-form `http://` request head for the origin server:
/// request line to origin form, `Host` set from the URL, hop-by-hop headers
/// (and any the `Connection` header names) dropped, `Connection: close` added so
/// the connection carries exactly this one exchange. `None` when the head is not
/// an absolute-form `http://` request.
pub fn rewrite_plain(head: &[u8]) -> Option<Forward> {
    let text = std::str::from_utf8(head).ok()?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split(' ');
    let (method, target, version, None) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next())
    else {
        return None;
    };
    if method == "CONNECT" || !version.starts_with("HTTP/1.") {
        return None;
    }
    let authority = plain_http_authority(target)?;
    let rest = &target["http://".len()..];
    let after_authority = &rest[rest.find(['/', '?', '#']).unwrap_or(rest.len())..];
    let path = match after_authority.chars().next() {
        None => "/".to_owned(),
        Some('/') => after_authority.to_owned(),
        Some(_) => format!("/{after_authority}"),
    };

    let headers: Vec<&str> = lines.take_while(|l| !l.is_empty()).collect();
    // Headers the client named in `Connection` are hop-by-hop too.
    let named: Vec<String> = headers
        .iter()
        .filter_map(|h| h.split_once(':'))
        .filter(|(n, _)| n.trim().eq_ignore_ascii_case("connection"))
        .flat_map(|(_, v)| v.split(',').map(|t| t.trim().to_ascii_lowercase()))
        .collect();

    let mut out = format!("{method} {path} {version}\r\nHost: {authority}\r\n");
    for header in headers {
        let (name, _) = header.split_once(':')?;
        let name = name.trim().to_ascii_lowercase();
        if HOP_BY_HOP.contains(&name.as_str()) || named.contains(&name) {
            continue;
        }
        out.push_str(header);
        out.push_str("\r\n");
    }
    out.push_str("Connection: close\r\n\r\n");

    let has_port = match authority.strip_prefix('[') {
        Some(v6) => v6.split_once(']').is_some_and(|(_, after)| after.starts_with(':')),
        None => authority.contains(':'),
    };
    let target = if has_port {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    };
    Some(Forward {
        target,
        head: out.into_bytes(),
    })
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
    let mut plain: Option<Vec<u8>> = None;
    let authority = match parse_head(&head) {
        Head::Connect(authority) => authority,
        Head::OtherMethod(Some(_)) => match rewrite_plain(&head) {
            Some(forward) => {
                plain = Some(forward.head);
                forward.target
            }
            None => return respond(&mut stream, "400 Bad Request").await,
        },
        Head::OtherMethod(None) => {
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
    match plain {
        // The request itself goes to the origin; there is no 200 to send.
        Some(head) => tunnel.write(&head).await?,
        None => {
            stream
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await?
        }
    }
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

    fn rewritten(head: &str) -> Option<(String, String)> {
        rewrite_plain(head.as_bytes()).map(|f| (f.target, String::from_utf8(f.head).unwrap()))
    }

    #[test]
    fn a_plain_request_becomes_origin_form_for_one_exchange() {
        let (target, head) = rewritten(
            "POST http://api.example:8080/v1/x?q=1 HTTP/1.1\r\nHost: other\r\n\
             Proxy-Authorization: Basic abc\r\nProxy-Connection: keep-alive\r\n\
             Connection: keep-alive, X-Hop\r\nX-Hop: 1\r\nContent-Length: 2\r\n\
             Accept: */*\r\n\r\n",
        )
        .unwrap();
        assert_eq!(target, "api.example:8080");
        assert_eq!(
            head,
            "POST /v1/x?q=1 HTTP/1.1\r\nHost: api.example:8080\r\nContent-Length: 2\r\n\
             Accept: */*\r\nConnection: close\r\n\r\n",
            "origin-form target, Host from the URL, no hop-by-hop or proxy credentials"
        );
    }

    #[test]
    fn the_port_defaults_and_the_path_is_never_empty() {
        let (target, head) = rewritten("GET http://example.com HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(target, "example.com:80");
        assert!(head.starts_with("GET / HTTP/1.1\r\nHost: example.com\r\n"), "{head}");
        let (_, head) = rewritten("GET http://example.com?x=1 HTTP/1.1\r\n\r\n").unwrap();
        assert!(head.starts_with("GET /?x=1 HTTP/1.1\r\n"), "{head}");
        assert_eq!(rewritten("GET http://[fd00::5]/ HTTP/1.1\r\n\r\n").unwrap().0, "[fd00::5]:80");
        assert_eq!(rewritten("GET http://[fd00::5]:81/ HTTP/1.1\r\n\r\n").unwrap().0, "[fd00::5]:81");
        let (target, head) = rewritten("GET http://u:p@host/ HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(target, "host:80", "userinfo is not part of the destination");
        assert!(!head.contains("u:p"), "{head}");
    }

    #[test]
    fn only_absolute_http_requests_are_rewritten() {
        assert!(rewritten("GET /index.html HTTP/1.1\r\n\r\n").is_none());
        assert!(rewritten("GET https://x/ HTTP/1.1\r\n\r\n").is_none());
        assert!(rewritten("CONNECT x:443 HTTP/1.1\r\n\r\n").is_none());
        assert!(rewritten("GET http://x/ SPDY/3\r\n\r\n").is_none());
        assert!(rewritten("GET http://x/ HTTP/1.1\r\nno colon here\r\n\r\n").is_none());
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
