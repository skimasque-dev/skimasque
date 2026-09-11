//! The MASQUE client: opening CONNECT-UDP tunnels through a proxy.
//!
//! ```no_run
//! # async fn example() -> Result<(), skimasque::Error> {
//! use skimasque::{client::Client, tls};
//! use skimasque_core::{connect_udp::Target, UriTemplate};
//!
//! let template = UriTemplate::default_connect_udp("proxy.example:4433")?;
//! let client = Client::new(tls::client_config_with_webpki_roots())?;
//! let session = client.connect("127.0.0.1:4433".parse().unwrap(), template).await?;
//!
//! let mut tunnel = session.connect_udp(Target::parse("1.1.1.1:53")?).await?;
//! tunnel.send(b"...dns query...")?;
//! let reply = tunnel.recv().await;
//! # Ok(()) }
//! ```

use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use bytes::{Buf, BufMut, Bytes};
use h3::ext::Protocol;
use http::{HeaderMap, HeaderValue, Method, Request, Uri};
use skimasque_core::connect_udp::{self, Target};
use skimasque_core::{UriTemplate, CAPSULE_PROTOCOL_HEADER, CAPSULE_PROTOCOL_TRUE};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, trace};

use crate::capsules::CapsulePump;
use crate::dgram::{DatagramRoute, DatagramRouter, SendError};
use crate::{tls, Error};

type SendStream = h3::client::RequestStream<h3_quinn::SendStream<Bytes>, Bytes>;
type RecvStream = h3::client::RequestStream<h3_quinn::RecvStream, Bytes>;
type SendRequest = h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>;

/// A QUIC endpoint from which proxy sessions are opened.
///
/// One client can hold sessions to several proxies; the endpoint owns the UDP
/// socket they share.
#[derive(Debug, Clone)]
pub struct Client {
    endpoint: quinn::Endpoint,
}

impl Client {
    /// Bind an ephemeral local port and use `tls` for every session.
    pub fn new(tls: rustls::ClientConfig) -> Result<Self, Error> {
        Self::bind("0.0.0.0:0".parse().expect("literal address"), tls)
    }

    /// Bind a specific local address.
    pub fn bind(local: SocketAddr, tls: rustls::ClientConfig) -> Result<Self, Error> {
        let mut config = tls::quic_client_config(tls)?;
        config.transport_config(Arc::new(tls::datagram_transport_config()));
        let mut endpoint = quinn::Endpoint::client(local)?;
        endpoint.set_default_client_config(config);
        Ok(Self { endpoint })
    }

    /// Open an HTTP/3 session to the proxy at `proxy`, which serves `template`.
    ///
    /// The TLS server name and the `:authority` of every request are taken from
    /// the template's authority, so `proxy` may be any address that reaches it.
    pub async fn connect(
        &self,
        proxy: SocketAddr,
        template: UriTemplate,
    ) -> Result<Session, Error> {
        let authority = template.authority().to_owned();
        // The TLS server name is the host alone; the port is not part of it.
        let server_name = server_name_of(&authority);

        let quic = self
            .endpoint
            .connect(proxy, server_name)?
            .await
            .map_err(Error::Connection)?;

        if quic.max_datagram_size().is_none() {
            quic.close(0u32.into(), b"peer does not support QUIC datagrams");
            return Err(Error::DatagramsUnavailable);
        }

        let router = DatagramRouter::spawn(quic.clone());
        let (mut driver, send_request) = h3::client::builder()
            .enable_datagram(true)
            .enable_extended_connect(true)
            .build::<_, h3_quinn::OpenStreams, Bytes>(h3_quinn::Connection::new(quic.clone()))
            .await?;

        // The connection makes no progress unless something polls it.
        let driver = tokio::spawn(async move {
            let error = driver.wait_idle().await;
            debug!(%error, "HTTP/3 client connection closed");
        });

        Ok(Session {
            quic,
            router,
            send_request,
            template,
            authority,
            default_headers: Arc::new(RwLock::new(HeaderMap::new())),
            driver: AbortOnDrop(driver),
        })
    }

    /// The local address the endpoint is bound to.
    pub fn local_addr(&self) -> Result<SocketAddr, Error> {
        Ok(self.endpoint.local_addr()?)
    }

    /// Wait for all connections to close, so a process can exit cleanly.
    pub async fn wait_idle(&self) {
        self.endpoint.wait_idle().await;
    }
}

/// Strip the port from an authority, leaving the TLS server name.
fn server_name_of(authority: &str) -> &str {
    match authority.rfind(']') {
        // `[::1]:4433` -- the host ends at the bracket.
        Some(bracket) => &authority[..=bracket],
        None => authority.split(':').next().unwrap_or(authority),
    }
}

/// A platform credential obtained from a gateway's token-exchange endpoint.
///
/// Present [`token`](Self::token) on tunnels as `Proxy-Authorization: Bearer`.
/// It is short-lived; [`expires_in`](Self::expires_in) says how long from when
/// it was issued.
#[derive(Debug, Clone)]
pub struct Credential {
    pub token: String,
    pub expires_in: Duration,
}

/// An HTTP/3 session with a proxy, from which tunnels are opened.
pub struct Session {
    quic: quinn::Connection,
    router: DatagramRouter,
    send_request: SendRequest,
    template: UriTemplate,
    authority: String,
    /// Headers added to every tunnel request. Behind a lock because
    /// [`Session::set_credential`] can replace the credential from another task
    /// while tunnels are being opened.
    default_headers: Arc<RwLock<HeaderMap>>,
    /// Held for its `Drop`: the HTTP/3 driver task must not outlive the session.
    #[allow(dead_code)]
    driver: AbortOnDrop,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `h3`'s SendRequest is not Debug, so name the parts that identify the
        // session instead of deriving.
        f.debug_struct("Session")
            .field("proxy", &self.quic.remote_address())
            .field("template", &self.template.as_str())
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Send `headers` on every request opened from this session.
    ///
    /// The usual reason is `Proxy-Authorization`, which is constant for the
    /// life of a session but has to appear on each request.
    pub fn with_default_headers(mut self, headers: HeaderMap) -> Self {
        self.default_headers = Arc::new(RwLock::new(headers));
        self
    }

    /// Replace the credential presented on tunnels opened from here on.
    ///
    /// A credential rides on the CONNECT request and is checked once, when the
    /// tunnel opens, so tunnels already running are untouched -- this changes
    /// only what the next [`connect_udp`](Self::connect_udp) or
    /// [`connect_tcp`](Self::connect_tcp) will send as
    /// `Proxy-Authorization: Bearer`. It is how a session that outlives its
    /// first credential stays authorized: re-run
    /// [`exchange_credential`](Self::exchange_credential) before the old
    /// credential expires and install the result here.
    pub fn set_credential(&self, credential: &Credential) -> Result<(), Error> {
        let value = HeaderValue::from_str(&format!("Bearer {}", credential.token)).map_err(|_| {
            Error::Invalid("the credential contains characters a header cannot carry".to_owned())
        })?;
        self.default_headers
            .write()
            .expect("the session's header lock is not poisoned")
            .insert(http::header::PROXY_AUTHORIZATION, value);
        Ok(())
    }

    /// Exchange an identity token for a platform credential at the gateway's
    /// [`CREDENTIAL_EXCHANGE_PATH`](crate::exchange::CREDENTIAL_EXCHANGE_PATH).
    ///
    /// The gateway verifies `identity_token` (a GitHub OIDC token, say) and
    /// returns a short-lived credential to present on tunnels, usually via
    /// [`with_default_headers`](Self::with_default_headers) as
    /// `Proxy-Authorization: Bearer <credential>`.
    pub async fn exchange_credential(&self, identity_token: &str) -> Result<Credential, Error> {
        let uri = Uri::builder()
            .scheme(self.template.scheme())
            .authority(self.authority.as_str())
            .path_and_query(crate::exchange::CREDENTIAL_EXCHANGE_PATH)
            .build()
            .map_err(|e| Error::Invalid(format!("building the exchange URI: {e}")))?;

        let request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(
                http::header::AUTHORIZATION,
                format!("Bearer {identity_token}"),
            )
            .body(())
            .map_err(|e| Error::Invalid(format!("building the exchange request: {e}")))?;

        let mut stream = self.send_request.clone().send_request(request).await?;
        // No request body; close the send side so the gateway can respond.
        stream.finish().await?;

        let response = stream.recv_response().await?;
        let status = response.status();

        let mut body = Vec::new();
        while let Some(mut chunk) = stream.recv_data().await? {
            let len = chunk.remaining();
            body.put(chunk.copy_to_bytes(len));
            if body.len() > 64 * 1024 {
                return Err(Error::Invalid(
                    "the exchange response is implausibly large".to_owned(),
                ));
            }
        }

        if !status.is_success() {
            let detail = String::from_utf8_lossy(&body).trim().to_owned();
            return Err(Error::ExchangeFailed {
                status,
                detail: (!detail.is_empty()).then_some(detail),
            });
        }

        let parsed: crate::exchange::ExchangeBody = serde_json::from_slice(&body)
            .map_err(|e| Error::Invalid(format!("parsing the exchange response: {e}")))?;
        debug!(
            expires_in = parsed.expires_in,
            "obtained a platform credential"
        );
        Ok(Credential {
            token: parsed.credential,
            expires_in: Duration::from_secs(parsed.expires_in),
        })
    }

    /// Open a CONNECT-UDP tunnel to `target` (RFC 9298).
    ///
    /// Returns once the proxy has answered. A successful response means the
    /// proxy has already resolved the target and opened a socket to it, so a
    /// tunnel that opens is a tunnel that can carry traffic.
    pub async fn connect_udp(&self, target: Target) -> Result<UdpTunnel, Error> {
        let path = target.expand_path(&self.template)?;
        let uri = Uri::builder()
            .scheme(self.template.scheme())
            .authority(self.authority.as_str())
            .path_and_query(path)
            .build()
            .map_err(|e| Error::Invalid(format!("building request URI: {e}")))?;

        let mut request = Request::builder()
            .method(Method::CONNECT)
            .uri(uri)
            .header(CAPSULE_PROTOCOL_HEADER, CAPSULE_PROTOCOL_TRUE)
            .body(())
            .map_err(|e| Error::Invalid(format!("building request: {e}")))?;
        request.headers_mut().extend(
            self.default_headers
                .read()
                .expect("the session's header lock is not poisoned")
                .iter()
                .map(|(n, v)| (n.clone(), v.clone())),
        );
        // The `:protocol` pseudo-header that makes this an extended CONNECT.
        request.extensions_mut().insert(Protocol::CONNECT_UDP);

        let mut stream = self.send_request.clone().send_request(request).await?;
        let stream_id = stream.id().into_inner();

        // Claim the route before reading the response: the proxy may send
        // datagrams as soon as it has answered, and they can arrive first. If
        // the request is refused, the route is dropped unused.
        let (route, inbound) = self.router.register(stream_id);

        let response = stream.recv_response().await?;

        // RFC 9298, Section 3.5: any response outside 2xx means the request
        // failed and the client must abort it.
        if !response.status().is_success() {
            let proxy_status = response
                .headers()
                .get("proxy-status")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            return Err(Error::Rejected {
                status: response.status(),
                proxy_status,
            });
        }

        let (send, recv) = stream.split();

        let reader = tokio::spawn(read_capsules(recv, route.sink()));

        debug!(%target, stream_id, "CONNECT-UDP tunnel established");
        Ok(UdpTunnel {
            target,
            route,
            inbound,
            send,
            reader: Some(reader),
        })
    }

    /// Open a raw TCP tunnel to `target` with a classic `CONNECT` request
    /// (`draft-ietf-httpbis-connect-tcp`).
    ///
    /// The request is a bare `CONNECT host:port`: no `:scheme`, `:path`,
    /// `:protocol` or `Capsule-Protocol`. A 2xx response means the proxy has
    /// already opened the upstream TCP connection, so a tunnel that opens is one
    /// that can carry traffic.
    pub async fn connect_tcp(&self, target: Target) -> Result<TcpTunnel, Error> {
        // The target belongs in `:authority`; `Target`'s Display brackets IPv6.
        let authority = target.to_string();
        let uri = Uri::builder()
            .scheme(self.template.scheme())
            .authority(authority.as_str())
            .path_and_query("/")
            .build()
            .map_err(|e| Error::Invalid(format!("building the CONNECT URI: {e}")))?;

        let mut request = Request::builder()
            .method(Method::CONNECT)
            .uri(uri)
            .body(())
            .map_err(|e| Error::Invalid(format!("building request: {e}")))?;
        request.headers_mut().extend(
            self.default_headers
                .read()
                .expect("the session's header lock is not poisoned")
                .iter()
                .map(|(n, v)| (n.clone(), v.clone())),
        );
        // No `:protocol` extension: h3 drops `:scheme` and `:path` for a
        // CONNECT with no protocol and sends only `:method` and `:authority`.

        let mut stream = self.send_request.clone().send_request(request).await?;
        let stream_id = stream.id().into_inner();

        let response = stream.recv_response().await?;
        if !response.status().is_success() {
            let proxy_status = response
                .headers()
                .get("proxy-status")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            return Err(Error::Rejected {
                status: response.status(),
                proxy_status,
            });
        }

        let (send, recv) = stream.split();
        debug!(%target, stream_id, "CONNECT-TCP tunnel established");
        Ok(TcpTunnel {
            target,
            stream_id,
            send,
            recv: Some(recv),
        })
    }

    /// The proxy's address.
    pub fn remote_address(&self) -> SocketAddr {
        self.quic.remote_address()
    }

    /// The template this session sends requests against.
    pub fn template(&self) -> &UriTemplate {
        &self.template
    }

    /// Close the session and every tunnel on it.
    pub fn close(self) {
        // HTTP/3 error code H3_NO_ERROR.
        self.quic.close(0x100u32.into(), b"client shutting down");
    }
}

/// Read the request stream, forwarding DATAGRAM capsules into `sink`.
///
/// A proxy that has QUIC datagrams available will not normally send capsules,
/// but RFC 9297 gives the two encodings identical semantics and permits an
/// intermediary to convert between them, so a conforming client has to accept
/// both. The task ending is also how the tunnel learns the proxy closed it.
async fn read_capsules(mut stream: RecvStream, sink: mpsc::Sender<Bytes>) {
    let mut pump = CapsulePump::new(sink);
    loop {
        match stream.recv_data().await {
            Ok(Some(chunk)) => {
                if pump.push(bytes::Buf::chunk(&chunk)).await.is_break() {
                    return;
                }
            }
            Ok(None) => break,
            Err(error) => {
                debug!(%error, "tunnel stream ended");
                return;
            }
        }
    }
    pump.finish();
}

/// An open CONNECT-UDP tunnel to one target.
///
/// The tunnel has UDP semantics end to end: [`send`](Self::send) does not block
/// and does not guarantee delivery, and [`recv`](Self::recv) may miss datagrams
/// the proxy sent. That is the point -- a reliable tunnel would add a second
/// layer of retransmission under whatever protocol is being carried.
pub struct UdpTunnel {
    target: Target,
    route: DatagramRoute,
    inbound: mpsc::Receiver<Bytes>,
    send: SendStream,
    /// Ends when the proxy closes the request stream. `None` once observed.
    reader: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for UdpTunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdpTunnel")
            .field("target", &self.target)
            .field("stream_id", &self.route.stream_id())
            .finish_non_exhaustive()
    }
}

impl UdpTunnel {
    /// The target this tunnel carries traffic to.
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// The QUIC stream id of the underlying request.
    pub fn stream_id(&self) -> u64 {
        self.route.stream_id()
    }

    /// The largest UDP payload that currently fits in one datagram.
    ///
    /// Anything larger has to be dropped rather than fragmented: RFC 9298,
    /// Section 3.1 forbids the proxy from fragmenting at the IP layer.
    pub fn max_payload_size(&self) -> Option<usize> {
        // One byte of context id, since context 0 encodes as a single zero byte.
        self.route.max_payload_size()?.checked_sub(1)
    }

    /// Send a UDP payload to the target.
    pub fn send(&self, payload: &[u8]) -> Result<(), SendError> {
        self.route.send(&connect_udp::encode_payload(payload))
    }

    /// Receive the next UDP payload from the target, or `None` once the tunnel
    /// is closed.
    pub async fn recv(&mut self) -> Option<Bytes> {
        loop {
            let raw = match self.reader.as_mut() {
                Some(reader) => tokio::select! {
                    // Drain buffered datagrams before noticing the close, so a
                    // reply that arrived with the shutdown is not lost.
                    biased;
                    payload = self.inbound.recv() => payload?,
                    _ = reader => {
                        self.reader = None;
                        return None;
                    }
                },
                None => return None,
            };

            match connect_udp::decode_payload(raw) {
                Ok(connect_udp::Incoming::UdpPayload(payload)) => return Some(payload),
                Ok(connect_udp::Incoming::UnknownContext { context, .. }) => {
                    trace!(
                        context = context.get(),
                        "dropping datagram in an unknown context"
                    );
                }
                Err(error) => trace!(%error, "dropping malformed datagram"),
            }
        }
    }

    /// Close the tunnel, releasing the proxy's socket.
    pub async fn close(mut self) -> Result<(), Error> {
        self.send.finish().await?;
        Ok(())
    }
}

/// An open CONNECT-TCP tunnel: a raw, ordered byte stream to one target.
///
/// Unlike [`UdpTunnel`], this has reliable stream semantics -- the QUIC request
/// stream provides them -- so there is no datagram framing and no loss to
/// tolerate. Each direction closes independently:
/// [`shutdown`](Self::shutdown) sends a FIN to the target,
/// and [`read`](Self::read) returns `None` when the target closes its side.
pub struct TcpTunnel {
    target: Target,
    stream_id: u64,
    send: SendStream,
    /// `None` once the target's side of the stream has ended.
    recv: Option<RecvStream>,
}

impl std::fmt::Debug for TcpTunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpTunnel")
            .field("target", &self.target)
            .field("stream_id", &self.stream_id)
            .finish_non_exhaustive()
    }
}

impl TcpTunnel {
    /// The target this tunnel carries traffic to.
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// The QUIC stream id of the underlying request.
    pub fn stream_id(&self) -> u64 {
        self.stream_id
    }

    /// Send bytes to the target.
    pub async fn write(&mut self, data: &[u8]) -> Result<(), Error> {
        self.send.send_data(Bytes::copy_from_slice(data)).await?;
        Ok(())
    }

    /// Receive the next chunk of bytes from the target, or `None` once the
    /// target has closed its side of the connection.
    pub async fn read(&mut self) -> Result<Option<Bytes>, Error> {
        let Some(recv) = self.recv.as_mut() else {
            return Ok(None);
        };
        match recv.recv_data().await? {
            Some(mut buf) => Ok(Some(buf.copy_to_bytes(buf.remaining()))),
            None => {
                self.recv = None;
                Ok(None)
            }
        }
    }

    /// Half-close the send direction: the target sees a TCP FIN, but bytes can
    /// still arrive from it.
    pub async fn shutdown(&mut self) -> Result<(), Error> {
        self.send.finish().await?;
        Ok(())
    }

    /// Close the tunnel.
    pub async fn close(mut self) -> Result<(), Error> {
        self.send.finish().await?;
        Ok(())
    }

    /// Bridge `io` -- a local socket, typically -- to the tunnel until both
    /// directions close.
    ///
    /// This is `tokio::io::copy_bidirectional` in spirit; the explicit pump is
    /// only because `TcpTunnel` is not `AsyncRead + AsyncWrite` yet.
    pub async fn relay<T>(mut self, mut io: T) -> Result<(), Error>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        let mut buf = vec![0u8; 64 * 1024];
        let mut local_done = false;
        let mut remote_done = self.recv.is_none();

        while !(local_done && remote_done) {
            tokio::select! {
                biased;

                chunk = async { self.recv.as_mut().unwrap().recv_data().await }, if !remote_done => {
                    match chunk? {
                        Some(mut data) => {
                            let bytes = data.copy_to_bytes(data.remaining());
                            io.write_all(&bytes).await?;
                        }
                        None => {
                            self.recv = None;
                            remote_done = true;
                            io.shutdown().await?;
                        }
                    }
                }

                result = io.read(&mut buf), if !local_done => {
                    let len = result?;
                    if len == 0 {
                        local_done = true;
                        self.send.finish().await?;
                    } else {
                        self.send.send_data(Bytes::copy_from_slice(&buf[..len])).await?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Aborts a background task when the owner is dropped.
#[derive(Debug)]
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_server_name_drops_the_port() {
        assert_eq!(
            server_name_of("proxy.example.org:4433"),
            "proxy.example.org"
        );
        assert_eq!(server_name_of("proxy.example.org"), "proxy.example.org");
        assert_eq!(server_name_of("[2001:db8::1]:4433"), "[2001:db8::1]");
        assert_eq!(server_name_of("[2001:db8::1]"), "[2001:db8::1]");
    }
}
