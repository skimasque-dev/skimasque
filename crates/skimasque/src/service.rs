//! The proxy as a [`tower::Service`].
//!
//! A MASQUE proxy is, at its core, a function from "a client asked for a tunnel
//! to X" to "here is a socket, or here is why not". Expressing that as a
//! `Service<TunnelRequest, Response = Accepted, Error = Rejection>` means
//! authorization, rate limiting, concurrency caps, timeouts and metrics are
//! ordinary tower layers rather than options bolted onto the proxy:
//!
//! ```no_run
//! use std::time::Duration;
//! use tower::ServiceBuilder;
//! use skimasque::service::{AuthorizeLayer, UdpProxy};
//!
//! let service = ServiceBuilder::new()
//!     .layer(AuthorizeLayer::bearer("hunter2"))
//!     .concurrency_limit(512)
//!     .timeout(Duration::from_secs(5))
//!     .service(UdpProxy::new(Default::default()));
//! ```
//!
//! The innermost service is where DNS resolution happens, which is also why the
//! address policy lives there rather than in a layer: a layer above resolution
//! can only see the name a client asked for, and a name is not a destination.

use std::borrow::Cow;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use http::{HeaderMap, HeaderValue, StatusCode, Uri};
use skimasque_core::connect_ip::IpScope;
use skimasque_core::target::{Target, TargetHost};
use skimasque_core::Protocol;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::watch;
use tower::{Layer, Service};
use tracing::debug;

use crate::audit::{AuditEvent, AuditSink};
use crate::policy::AddressPolicy;

/// Where a tunnel is being asked to go.
///
/// The two MASQUE protocols name their destination differently: CONNECT-UDP
/// carries one host and port, while CONNECT-IP carries a scope that may be as
/// broad as "anything you will allow". Both variants exist regardless of which
/// cargo features are on, so a service written against this enum keeps
/// compiling when `connect-ip` is switched on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// RFC 9298: a single UDP endpoint.
    Udp(Target),
    /// `draft-ietf-httpbis-connect-tcp`: a single TCP endpoint. Reached either
    /// by classic `CONNECT host:port` or by the template-driven variant.
    Tcp(Target),
    /// RFC 9484: the scope of an IP tunnel.
    Ip(IpScope),
}

impl Destination {
    pub fn as_udp(&self) -> Option<&Target> {
        match self {
            Self::Udp(target) => Some(target),
            Self::Tcp(_) | Self::Ip(_) => None,
        }
    }

    pub fn as_tcp(&self) -> Option<&Target> {
        match self {
            Self::Tcp(target) => Some(target),
            Self::Udp(_) | Self::Ip(_) => None,
        }
    }

    /// The `host:port` target, whichever transport carries it. `None` only for
    /// a CONNECT-IP scope, which has no single endpoint.
    pub fn as_target(&self) -> Option<&Target> {
        match self {
            Self::Udp(target) | Self::Tcp(target) => Some(target),
            Self::Ip(_) => None,
        }
    }

    pub fn as_ip(&self) -> Option<&IpScope> {
        match self {
            Self::Ip(scope) => Some(scope),
            Self::Udp(_) | Self::Tcp(_) => None,
        }
    }
}

impl std::fmt::Display for Destination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Udp(target) => write!(f, "{target}"),
            Self::Tcp(target) => write!(f, "{target}"),
            Self::Ip(scope) => write!(
                f,
                "ip:{}/{}",
                scope.target.as_deref().unwrap_or("*"),
                scope
                    .protocol
                    .map_or_else(|| "*".to_owned(), |p| p.to_string())
            ),
        }
    }
}

/// A destination policy has approved for a tunnel.
///
/// The innermost proxy resolves and forwards to an `AuthorizedDestination`,
/// never a bare [`Target`], so a forwarding path cannot be written without
/// something having authorized it first. There are only two ways to get one:
///
///  - [`PolicyLayer`] mints one from an `Allow` decision -- the real path, and
///  - [`AuthorizedDestination::trusting`], the single, greppable escape hatch
///    for a deployment whose only control is the network floor
///    ([`AddressPolicy`], which still runs afterwards on the resolved address).
#[derive(Debug, Clone)]
pub struct AuthorizedDestination {
    target: Target,
    decision: Option<skimasque_policy::Allowed>,
}

impl AuthorizedDestination {
    /// Approve `target` with no policy behind it. The name says what it is: a
    /// deployment that calls this is trusting whatever sits in front of the
    /// proxy, plus the [`AddressPolicy`] floor, and nothing else.
    pub fn trusting(target: Target) -> Self {
        Self {
            target,
            decision: None,
        }
    }

    pub(crate) fn from_decision(target: Target, decision: skimasque_policy::Allowed) -> Self {
        Self {
            target,
            decision: Some(decision),
        }
    }

    /// The approved target.
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// The policy decision behind this authorization, if a policy made one.
    pub fn decision(&self) -> Option<&skimasque_policy::Allowed> {
        self.decision.as_ref()
    }
}

/// A tunnel the proxy has been asked to open.
#[derive(Debug)]
pub struct TunnelRequest {
    protocol: Protocol,
    destination: Destination,
    client: SocketAddr,
    parts: http::request::Parts,
    authorized: Option<AuthorizedDestination>,
}

impl TunnelRequest {
    pub(crate) fn new(
        protocol: Protocol,
        destination: Destination,
        client: SocketAddr,
        parts: http::request::Parts,
    ) -> Self {
        Self {
            protocol,
            destination,
            client,
            parts,
            authorized: None,
        }
    }

    /// Attach a policy authorization. Called by [`PolicyLayer`]; once set, the
    /// proxy forwards to this destination rather than the one in the request
    /// path.
    pub fn authorize(&mut self, destination: AuthorizedDestination) {
        self.authorized = Some(destination);
    }

    /// The attached authorization, if a layer added one.
    pub fn authorized_destination(&self) -> Option<&AuthorizedDestination> {
        self.authorized.as_ref()
    }

    /// Which MASQUE protocol the client used.
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// Where the tunnel is asked to go, recovered from the request path via
    /// the URI Template.
    pub fn destination(&self) -> &Destination {
        &self.destination
    }

    /// The `host:port` target, for CONNECT-UDP and CONNECT-TCP alike. `None`
    /// only for a CONNECT-IP scope.
    pub fn target(&self) -> Option<&Target> {
        self.destination.as_target()
    }

    /// The client's address, as seen by the proxy.
    pub fn client_addr(&self) -> SocketAddr {
        self.client
    }

    /// The request headers, for layers that authenticate or route on them.
    pub fn headers(&self) -> &HeaderMap {
        &self.parts.headers
    }

    /// The reconstructed request URI.
    pub fn uri(&self) -> &Uri {
        &self.parts.uri
    }

    /// Request extensions, so a layer can pass state to an inner service.
    pub fn extensions(&self) -> &http::Extensions {
        &self.parts.extensions
    }

    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        &mut self.parts.extensions
    }
}

/// A shared token bucket, backing a policy's rate limits.
///
/// One instance meters bytes (`limits.bandwidth`) and another meters datagrams
/// (`limits.packets_per_second`). Every tunnel a policy allows shares the same
/// instances, so a limit is aggregate across the policy -- the stand-in for
/// per-session until issued credentials carry a session. [`consume`](Self::consume)
/// is called by the data-plane relay before it moves a chunk in either
/// direction; when the bucket is dry the relay task sleeps, which flow-controls
/// the QUIC stream for CONNECT-TCP and paces datagrams for CONNECT-UDP.
pub struct RateLimiter {
    /// The label for `skimasque_rate_limit_throttled_total`: `bandwidth` or
    /// `packets`.
    kind: &'static str,
    units_per_sec: f64,
    /// Burst allowance. Floored so a full relay chunk (or one datagram) always
    /// fits and [`consume`](Self::consume) cannot wait forever for a bucket that
    /// will never hold enough.
    capacity: f64,
    state: std::sync::Mutex<BucketState>,
}

struct BucketState {
    tokens: f64,
    last: tokio::time::Instant,
}

impl RateLimiter {
    /// A byte limiter for `bits_per_sec` (what a policy's `bandwidth` field
    /// parses to). Burst allowance is one second of rate, floored at 64 KiB.
    pub fn bandwidth(bits_per_sec: u64) -> Self {
        Self::new("bandwidth", (bits_per_sec as f64 / 8.0).max(1.0), 64.0 * 1024.0)
    }

    /// A datagram limiter for `packets_per_sec` (`limits.packets_per_second`).
    /// Burst allowance is one second of rate, floored at 64 datagrams.
    pub fn packets(packets_per_sec: u64) -> Self {
        Self::new("packets", (packets_per_sec as f64).max(1.0), 64.0)
    }

    fn new(kind: &'static str, units_per_sec: f64, min_capacity: f64) -> Self {
        let capacity = units_per_sec.max(min_capacity);
        Self {
            kind,
            units_per_sec,
            capacity,
            state: std::sync::Mutex::new(BucketState {
                tokens: capacity,
                last: tokio::time::Instant::now(),
            }),
        }
    }

    /// Wait until `units` of budget are available, then spend them. `units` is a
    /// byte count for a [`bandwidth`](Self::bandwidth) limiter and `1` per
    /// datagram for a [`packets`](Self::packets) one.
    pub async fn consume(&self, units: usize) {
        let need = (units as f64).min(self.capacity);
        loop {
            let wait = {
                let mut state = self.state.lock().expect("rate bucket is not poisoned");
                let now = tokio::time::Instant::now();
                let elapsed = now.saturating_duration_since(state.last).as_secs_f64();
                state.tokens = (state.tokens + elapsed * self.units_per_sec).min(self.capacity);
                state.last = now;
                if state.tokens >= need {
                    state.tokens -= need;
                    return;
                }
                Duration::from_secs_f64((need - state.tokens) / self.units_per_sec)
            };
            // Cap each nap so a very large `wait` still rechecks periodically.
            let nap = wait.min(Duration::from_secs(1));
            crate::metrics::rate_limit_throttled(self.kind, nap);
            tokio::time::sleep(nap).await;
        }
    }
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimiter")
            .field("kind", &self.kind)
            .field("units_per_sec", &self.units_per_sec)
            .finish()
    }
}

/// The per-tunnel resource limits a policy decision carries, bundled so the
/// data-plane relay takes one argument.
///
/// `bandwidth` and `packet_rate` are shared across the policy (see
/// [`RateLimiter`]); `total_bytes` is a per-tunnel ceiling for now -- it becomes
/// per-session once a credential carries one.
#[derive(Debug, Default, Clone)]
pub struct TunnelLimits {
    pub(crate) bandwidth: Option<Arc<RateLimiter>>,
    pub(crate) packet_rate: Option<Arc<RateLimiter>>,
    pub(crate) total_bytes: Option<u64>,
}

impl TunnelLimits {
    fn is_empty(&self) -> bool {
        self.bandwidth.is_none() && self.packet_rate.is_none() && self.total_bytes.is_none()
    }
}

/// An opaque resource kept alive for a tunnel's lifetime.
///
/// A layer that reserves something for a tunnel -- a quota permit, most often
/// -- attaches it here with [`Accepted::hold`]. The server keeps every guard
/// alive while the tunnel relays and drops them when it ends, which is what
/// releases the reservation. The value is never inspected, only held.
pub struct TunnelGuard(#[allow(dead_code)] Box<dyn std::any::Any + Send + Sync>);

impl std::fmt::Debug for TunnelGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TunnelGuard")
    }
}

/// A tunnel the proxy has agreed to open, carrying whatever serves it.
#[derive(Debug)]
pub struct Accepted {
    kind: AcceptedKind,
    headers: Option<Box<HeaderMap>>,
    guards: Vec<TunnelGuard>,
    limits: TunnelLimits,
}

/// What the proxy will use to carry a tunnel's traffic.
#[derive(Debug)]
pub(crate) enum AcceptedKind {
    /// A connected UDP socket, for CONNECT-UDP.
    Udp {
        socket: UdpSocket,
        peer: SocketAddr,
    },
    /// An open TCP connection to the target, for CONNECT-TCP.
    Tcp {
        stream: TcpStream,
        peer: SocketAddr,
    },
    /// An endpoint on the proxy's IP network, for CONNECT-IP. Boxed so the
    /// CONNECT-IP case does not enlarge every `Accepted`.
    #[cfg(feature = "connect-ip")]
    Ip(Box<crate::connect_ip::IpEndpoint>),
}

impl Accepted {
    /// Accept a CONNECT-UDP tunnel, relaying through `socket`, which must
    /// already be connected to `peer`.
    pub fn udp(socket: UdpSocket, peer: SocketAddr) -> Self {
        Self {
            kind: AcceptedKind::Udp { socket, peer },
            headers: None,
            guards: Vec::new(),
            limits: TunnelLimits::default(),
        }
    }

    /// Accept a CONNECT-TCP tunnel, relaying raw bytes over `stream`, which is
    /// already connected to `peer`.
    pub fn tcp(stream: TcpStream, peer: SocketAddr) -> Self {
        Self {
            kind: AcceptedKind::Tcp { stream, peer },
            headers: None,
            guards: Vec::new(),
            limits: TunnelLimits::default(),
        }
    }

    /// Accept a CONNECT-IP tunnel, carrying its packets through `endpoint`.
    #[cfg(feature = "connect-ip")]
    pub fn ip(endpoint: crate::connect_ip::IpEndpoint) -> Self {
        Self {
            kind: AcceptedKind::Ip(Box::new(endpoint)),
            headers: None,
            guards: Vec::new(),
            limits: TunnelLimits::default(),
        }
    }

    /// Add a header to the success response.
    pub fn with_header(mut self, name: http::header::HeaderName, value: HeaderValue) -> Self {
        self.headers
            .get_or_insert_with(Box::default)
            .insert(name, value);
        self
    }

    /// Hold `guard` for as long as the tunnel lives. Used by layers that
    /// reserve a resource -- see [`TunnelGuard`].
    pub fn hold(mut self, guard: impl std::any::Any + Send + Sync + 'static) -> Self {
        self.guards.push(TunnelGuard(Box::new(guard)));
        self
    }

    /// Attach the per-tunnel resource limits from the policy decision.
    /// [`QuotaLayer`] fills these in; the data-plane relay enforces them.
    pub fn with_limits(mut self, limits: TunnelLimits) -> Self {
        self.limits = limits;
        self
    }

    #[cfg(test)]
    pub(crate) fn limits(&self) -> &TunnelLimits {
        &self.limits
    }

    /// The address the tunnel's socket is connected to, for UDP and TCP.
    pub fn peer(&self) -> Option<SocketAddr> {
        match &self.kind {
            AcceptedKind::Udp { peer, .. } | AcceptedKind::Tcp { peer, .. } => Some(*peer),
            #[cfg(feature = "connect-ip")]
            AcceptedKind::Ip(_) => None,
        }
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn into_parts(
        self,
    ) -> (
        AcceptedKind,
        Option<Box<HeaderMap>>,
        Vec<TunnelGuard>,
        TunnelLimits,
    ) {
        (self.kind, self.headers, self.guards, self.limits)
    }
}

/// Why the proxy will not open a tunnel.
///
/// RFC 9298, Section 3.5 makes any non-2xx response a failure the client must
/// abort on, so the status code is the only thing a client is guaranteed to act
/// on; `proxy_error` and `detail` populate a `Proxy-Status` header (RFC 9209)
/// for diagnostics.
#[derive(Debug, Clone)]
pub struct Rejection {
    status: StatusCode,
    proxy_error: Option<&'static str>,
    detail: Cow<'static, str>,
    /// Boxed because most rejections carry no headers, and `Rejection` is the
    /// error type of a `Service`: an inline `HeaderMap` would make every
    /// `Result` on the hot path as large as the rare failure case.
    headers: Option<Box<HeaderMap>>,
}

impl Rejection {
    pub fn new(status: StatusCode, detail: impl Into<Cow<'static, str>>) -> Self {
        Self {
            status,
            proxy_error: None,
            detail: detail.into(),
            headers: None,
        }
    }

    /// Attach an RFC 9209 Proxy Error Type, such as `dns_error`.
    pub fn with_proxy_error(mut self, error: &'static str) -> Self {
        self.proxy_error = Some(error);
        self
    }

    pub fn with_header(mut self, name: http::header::HeaderName, value: HeaderValue) -> Self {
        self.headers
            .get_or_insert_with(Box::default)
            .insert(name, value);
        self
    }

    /// The client is not allowed to reach this target.
    pub fn forbidden(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::FORBIDDEN, detail).with_proxy_error("destination_ip_prohibited")
    }

    /// The target name did not resolve.
    pub fn dns_error(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, detail).with_proxy_error("dns_error")
    }

    /// The proxy could not open a socket.
    pub fn unavailable(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, detail).with_proxy_error("proxy_internal_error")
    }

    /// The client must authenticate. Carries the `Proxy-Authenticate` challenge.
    pub fn proxy_auth_required(scheme: &'static str) -> Self {
        Self::new(
            StatusCode::PROXY_AUTHENTICATION_REQUIRED,
            "proxy authentication required",
        )
        .with_proxy_error("http_request_denied")
        .with_header(
            http::header::PROXY_AUTHENTICATE,
            HeaderValue::from_static(scheme),
        )
    }

    /// The request was not a well-formed MASQUE request.
    pub fn bad_request(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, detail)
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn into_parts(
        self,
    ) -> (
        StatusCode,
        Option<&'static str>,
        Cow<'static, str>,
        Option<Box<HeaderMap>>,
    ) {
        (self.status, self.proxy_error, self.detail, self.headers)
    }
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.status, self.detail)
    }
}

impl std::error::Error for Rejection {}

/// A boxed future, since `Service::Future` has to be a named type.
pub type TunnelFuture = Pin<Box<dyn Future<Output = Result<Accepted, Rejection>> + Send>>;

/// The innermost proxy service: resolve the target, check it, open a socket.
///
/// RFC 9298, Section 3.1 requires DNS resolution to complete *before* the
/// response is sent, because a client that gets a 2xx is entitled to assume the
/// proxy is ready to forward. That is why this is an async service rather than
/// something that lazily connects on first datagram.
#[derive(Debug, Clone)]
pub struct UdpProxy {
    policy: Arc<AddressPolicy>,
}

impl UdpProxy {
    pub fn new(policy: AddressPolicy) -> Self {
        Self {
            policy: Arc::new(policy),
        }
    }
}

impl Default for UdpProxy {
    fn default() -> Self {
        Self::new(AddressPolicy::default())
    }
}

impl Service<TunnelRequest> for UdpProxy {
    type Response = Accepted;
    type Error = Rejection;
    type Future = TunnelFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: TunnelRequest) -> Self::Future {
        let policy = self.policy.clone();
        Box::pin(async move { open_tunnel(policy, request).await })
    }
}

async fn open_tunnel(
    policy: Arc<AddressPolicy>,
    request: TunnelRequest,
) -> Result<Accepted, Rejection> {
    let Some(raw_target) = request.destination().as_udp() else {
        return Err(Rejection::new(
            StatusCode::NOT_IMPLEMENTED,
            "UdpProxy only serves connect-udp",
        ));
    };

    // Forward to whatever policy authorized. With no `PolicyLayer` in the
    // stack, that is the request's own target, trusted -- the `AddressPolicy`
    // floor below still runs on the resolved address either way.
    let authorized = match request.authorized_destination() {
        Some(authorized) => authorized.clone(),
        None => AuthorizedDestination::trusting(raw_target.clone()),
    };
    let target = authorized.target();
    let candidates = resolve(target).await?;

    // Try each resolved address in turn: a name may resolve to a mix of
    // permitted and prohibited addresses, and to families we cannot reach.
    let mut last_error = None;
    for addr in &candidates {
        if let Err(reason) = policy.permits(addr) {
            debug!(%addr, reason, "address rejected by policy");
            last_error = Some(Rejection::forbidden(reason));
            continue;
        }
        match connect_socket(*addr).await {
            Ok(socket) => return Ok(Accepted::udp(socket, *addr)),
            Err(error) => {
                debug!(%addr, %error, "could not open socket");
                last_error = Some(Rejection::unavailable(format!(
                    "could not open a socket to {addr}: {error}"
                )));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| Rejection::dns_error(format!("{target} resolved to nothing"))))
}

/// Resolve a target to the addresses worth trying, preserving resolver order.
async fn resolve(target: &Target) -> Result<Vec<SocketAddr>, Rejection> {
    match &target.host {
        TargetHost::Ip(ip) => Ok(vec![SocketAddr::new(*ip, target.port)]),
        TargetHost::Name(name) => {
            let addrs = tokio::net::lookup_host((name.as_str(), target.port))
                .await
                .map_err(|error| Rejection::dns_error(format!("resolving {name}: {error}")))?
                .collect::<Vec<_>>();
            if addrs.is_empty() {
                return Err(Rejection::dns_error(format!("{name} resolved to nothing")));
            }
            Ok(addrs)
        }
    }
}

/// Bind a socket of the right family and connect it.
///
/// RFC 9298, Section 3.1 recommends connected sockets, which is what makes the
/// kernel drop packets from anything but the target -- otherwise the proxy has
/// to filter the source of every packet itself.
async fn connect_socket(addr: SocketAddr) -> std::io::Result<UdpSocket> {
    let bind: SocketAddr = if addr.is_ipv4() {
        ([0, 0, 0, 0], 0).into()
    } else {
        ([0u16; 8], 0).into()
    };
    let socket = UdpSocket::bind(bind).await?;
    socket.connect(addr).await?;
    Ok(socket)
}

/// The innermost proxy service for CONNECT-TCP: resolve the target, check it
/// against the [`AddressPolicy`] floor, open a TCP connection.
///
/// A sibling of [`UdpProxy`]. It shares [`resolve`] and the SSRF floor unchanged
/// -- both are transport-neutral -- and differs only in opening a `TcpStream`
/// rather than binding a datagram socket. There is no payload codec: the QUIC
/// request stream already carries a reliable ordered byte stream.
#[derive(Debug, Clone)]
pub struct TcpProxy {
    policy: Arc<AddressPolicy>,
}

impl TcpProxy {
    pub fn new(policy: AddressPolicy) -> Self {
        Self {
            policy: Arc::new(policy),
        }
    }
}

impl Default for TcpProxy {
    fn default() -> Self {
        Self::new(AddressPolicy::default())
    }
}

impl Service<TunnelRequest> for TcpProxy {
    type Response = Accepted;
    type Error = Rejection;
    type Future = TunnelFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: TunnelRequest) -> Self::Future {
        let policy = self.policy.clone();
        Box::pin(async move { open_tcp_tunnel(policy, request).await })
    }
}

async fn open_tcp_tunnel(
    policy: Arc<AddressPolicy>,
    request: TunnelRequest,
) -> Result<Accepted, Rejection> {
    let Some(raw_target) = request.destination().as_tcp() else {
        return Err(Rejection::new(
            StatusCode::NOT_IMPLEMENTED,
            "TcpProxy only serves connect-tcp",
        ));
    };

    let authorized = match request.authorized_destination() {
        Some(authorized) => authorized.clone(),
        None => AuthorizedDestination::trusting(raw_target.clone()),
    };
    let target = authorized.target();
    let candidates = resolve(target).await?;

    let mut last_error = None;
    for addr in &candidates {
        if let Err(reason) = policy.permits(addr) {
            debug!(%addr, reason, "address rejected by policy");
            last_error = Some(Rejection::forbidden(reason));
            continue;
        }
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                // Nagle off: a tunnel relays whatever the client writes, and it
                // is the carried protocol's job to batch if it wants to.
                let _ = stream.set_nodelay(true);
                return Ok(Accepted::tcp(stream, *addr));
            }
            Err(error) => {
                debug!(%addr, %error, "could not open a TCP connection");
                last_error = Some(tcp_connect_rejection(*addr, &error));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| Rejection::dns_error(format!("{target} resolved to nothing"))))
}

/// Map a failed `TcpStream::connect` to a rejection whose `Proxy-Status` error
/// type (RFC 9209) a front end can turn into, say, a SOCKS reply code.
fn tcp_connect_rejection(addr: SocketAddr, error: &std::io::Error) -> Rejection {
    use std::io::ErrorKind;
    let proxy_error = match error.kind() {
        ErrorKind::ConnectionRefused => "connection_refused",
        ErrorKind::TimedOut => "connection_timeout",
        _ => "proxy_internal_error",
    };
    Rejection::new(
        StatusCode::BAD_GATEWAY,
        format!("could not connect to {addr}: {error}"),
    )
    .with_proxy_error(proxy_error)
}

/// Routes a [`TunnelRequest`] to the inner proxy for its protocol.
///
/// The layer stack (`identity -> auth -> policy -> quota`) sits above this
/// unchanged; a deployment turns transports on by handing [`Dispatch`] the
/// proxies it wants. A request for a transport that was not enabled is answered
/// `501`, never passed through.
#[derive(Debug, Clone, Default)]
pub struct Dispatch {
    udp: Option<UdpProxy>,
    tcp: Option<TcpProxy>,
}

impl Dispatch {
    /// An empty router. Add transports with [`with_udp`](Self::with_udp) and
    /// [`with_tcp`](Self::with_tcp).
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_udp(mut self, proxy: UdpProxy) -> Self {
        self.udp = Some(proxy);
        self
    }

    pub fn with_tcp(mut self, proxy: TcpProxy) -> Self {
        self.tcp = Some(proxy);
        self
    }
}

impl Service<TunnelRequest> for Dispatch {
    type Response = Accepted;
    type Error = Rejection;
    type Future = TunnelFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: TunnelRequest) -> Self::Future {
        match request.protocol() {
            Protocol::ConnectUdp => match &mut self.udp {
                Some(proxy) => proxy.call(request),
                None => Box::pin(std::future::ready(Err(Rejection::new(
                    StatusCode::NOT_IMPLEMENTED,
                    "this proxy does not serve connect-udp",
                )))),
            },
            Protocol::ConnectTcp => match &mut self.tcp {
                Some(proxy) => proxy.call(request),
                None => Box::pin(std::future::ready(Err(Rejection::new(
                    StatusCode::NOT_IMPLEMENTED,
                    "this proxy does not serve connect-tcp",
                )))),
            },
            Protocol::ConnectIp => Box::pin(std::future::ready(Err(Rejection::new(
                StatusCode::NOT_IMPLEMENTED,
                "this proxy does not serve connect-ip",
            )))),
        }
    }
}

/// The header a client uses to name the application it is running.
///
/// RFC-shaped intent, not an authenticated fact: the platform has no way yet
/// to tell what process is really behind a tunnel, so a policy that leans on
/// this alone is only as strong as the client's honesty. It is matched, but it
/// is "session context" until stronger application identity exists.
pub const APPLICATION_HEADER: &str = "x-masque-application";

/// Turns a bearer credential into a verified
/// [`WorkloadIdentity`](skimasque_policy::WorkloadIdentity).
///
/// An implementation typically does I/O -- fetching and caching an issuer's
/// signing keys -- and is shared across requests, so `verify` takes `&self`
/// and an owned token and returns a boxed, `'static` future. The
/// `skimasque-identity` crate provides one for GitHub Actions OIDC; a test can
/// provide a canned one.
///
/// A `String` error keeps this trait free of a dependency on any particular
/// verifier's error type; the layer only logs it and answers `403`.
pub trait IdentityVerifier: Send + Sync + std::fmt::Debug {
    fn verify(
        &self,
        token: String,
    ) -> Pin<Box<dyn Future<Output = Result<skimasque_policy::WorkloadIdentity, String>> + Send>>;
}

/// Verifies a workload-identity token and hands the identity to the policy
/// layer.
///
/// A prior step -- a GitHub Actions job, say -- presents a signed token in
/// `Proxy-Authorization: Bearer`. This layer gives it to an
/// [`IdentityVerifier`] and, on success, puts the resulting
/// [`WorkloadIdentity`](skimasque_policy::WorkloadIdentity) in the request
/// extensions, where [`PolicyLayer`] reads it as WHO.
///
/// When it is in the stack it *is* the authentication boundary, and it is
/// fail-closed: a request with no bearer token is answered `407`, and one whose
/// token does not verify is answered `403`. Neither reaches the inner service.
/// It therefore replaces [`AuthorizeLayer`] rather than sitting beside it.
#[derive(Clone)]
pub struct IdentityLayer {
    verifier: Arc<dyn IdentityVerifier>,
}

impl IdentityLayer {
    pub fn new(verifier: Arc<dyn IdentityVerifier>) -> Self {
        Self { verifier }
    }
}

impl std::fmt::Debug for IdentityLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityLayer").finish_non_exhaustive()
    }
}

impl<S> Layer<S> for IdentityLayer {
    type Service = Identify<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Identify {
            inner,
            verifier: self.verifier.clone(),
        }
    }
}

/// The service [`IdentityLayer`] produces.
#[derive(Clone, Debug)]
pub struct Identify<S> {
    inner: S,
    verifier: Arc<dyn IdentityVerifier>,
}

impl<S> Service<TunnelRequest> for Identify<S>
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Accepted;
    type Error = Rejection;
    type Future = TunnelFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: TunnelRequest) -> Self::Future {
        let token = request
            .headers()
            .get(http::header::PROXY_AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::to_owned);

        let Some(token) = token else {
            return Box::pin(std::future::ready(Err(Rejection::proxy_auth_required(
                "Bearer",
            ))));
        };

        let verifier = self.verifier.clone();
        // The standard tower move: take a ready clone of the inner service to
        // own across the await, leaving an equivalent one in its place.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        Box::pin(async move {
            match verifier.verify(token).await {
                Ok(identity) => {
                    debug!(
                        organization = ?identity.organization,
                        repository = ?identity.repository,
                        "verified workload identity"
                    );
                    request.extensions_mut().insert(identity);
                    inner.call(request).await
                }
                Err(reason) => {
                    debug!(%reason, "workload identity token rejected");
                    Err(
                        Rejection::new(
                            StatusCode::FORBIDDEN,
                            format!("workload identity token rejected: {reason}"),
                        )
                        .with_proxy_error("http_request_denied"),
                    )
                }
            }
        })
    }
}

/// A live handle to the policy set a [`PolicyLayer`] enforces.
///
/// A [`PolicyLayer`] evaluates every request against whatever set this handle
/// currently holds. Calling [`store`](Self::store) swaps in a new set
/// atomically: a request already being evaluated finishes against the set it
/// started with, every request after the swap sees the new one, and tunnels
/// already open are untouched -- a policy decision is made only while a tunnel
/// is opening.
///
/// Clone it freely; every clone, and the [`PolicyLayer`] it came from, share one
/// set. This is the hook a gateway uses to reload policy from disk without a
/// restart or a dropped tunnel.
#[derive(Clone)]
pub struct PolicyHandle {
    set: watch::Sender<Arc<skimasque_policy::PolicySet>>,
}

impl PolicyHandle {
    /// A handle seeded with `set`.
    pub fn new(set: skimasque_policy::PolicySet) -> Self {
        Self {
            set: watch::Sender::new(Arc::new(set)),
        }
    }

    /// Replace the policy set every future request is evaluated against.
    ///
    /// Takes effect immediately for requests that have not yet reached the
    /// policy layer; anything already mid-evaluation completes against the
    /// previous set.
    pub fn store(&self, set: skimasque_policy::PolicySet) {
        self.set.send_replace(Arc::new(set));
    }

    /// The set in force right now.
    pub fn current(&self) -> Arc<skimasque_policy::PolicySet> {
        self.set.borrow().clone()
    }

    fn subscribe(&self) -> watch::Receiver<Arc<skimasque_policy::PolicySet>> {
        self.set.subscribe()
    }
}

impl std::fmt::Debug for PolicyHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyHandle")
            .field("policies", &self.set.borrow().policies().len())
            .finish()
    }
}

/// Enforces an identity-aware [`skimasque_policy::PolicySet`] on every request.
///
/// It reads WHO (a [`skimasque_policy::WorkloadIdentity`] a prior layer left in
/// the request extensions, or the empty identity), WHAT (the
/// [`APPLICATION_HEADER`]), and WHERE (the tunnel's destination), evaluates the
/// set, and either attaches an [`AuthorizedDestination`] and forwards, or
/// answers `403` with the denial reason and a suggested rule in `Proxy-Status`.
///
/// It is fail-closed: a request it cannot turn into a policy question -- a
/// non-UDP destination, for instance -- is denied, not passed through.
///
/// In [`observe`](Self::observe) mode it denies nothing: it evaluates every
/// request, emits a `masque::observe` tracing event recording what the policy
/// *would* have decided, and forwards regardless. That is the "observe" step of
/// learning mode -- run the workload, collect the events, feed them to
/// `skimasque policy learn`.
///
/// Given a sink with [`with_audit`](Self::with_audit), it emits one
/// [`AuditEvent`] per enforced decision -- the compliance trail of every allow
/// and deny. Observe mode does not audit: its would-be decisions are the
/// `masque::observe` events, not authorizations.
///
/// The policy set is not fixed for the life of the layer:
/// [`handle`](Self::handle) hands back a [`PolicyHandle`] whose
/// [`store`](PolicyHandle::store) swaps in a new set for every subsequent
/// request, which is how a gateway hot-reloads policy from disk without a
/// restart.
#[derive(Clone)]
pub struct PolicyLayer {
    handle: PolicyHandle,
    mode: Mode,
    audit: Option<Arc<dyn AuditSink>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Enforce,
    Observe,
}

impl PolicyLayer {
    pub fn new(policies: skimasque_policy::PolicySet) -> Self {
        Self::from_handle(PolicyHandle::new(policies))
    }

    /// Build a layer around an existing [`PolicyHandle`], so the caller keeps a
    /// clone to [`store`](PolicyHandle::store) new policy into later.
    pub fn from_handle(handle: PolicyHandle) -> Self {
        Self {
            handle,
            mode: Mode::Enforce,
            audit: None,
        }
    }

    /// A handle to this layer's policy set, for reloading it without a restart.
    pub fn handle(&self) -> PolicyHandle {
        self.handle.clone()
    }

    /// Switch to observe mode: log what the policy would decide, allow
    /// everything.
    pub fn observe(mut self) -> Self {
        self.mode = Mode::Observe;
        self
    }

    /// Record every enforced allow and deny to `sink`. Has no effect in observe
    /// mode.
    pub fn with_audit(mut self, sink: Arc<dyn AuditSink>) -> Self {
        self.audit = Some(sink);
        self
    }
}

impl std::fmt::Debug for PolicyLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyLayer")
            .field("policies", &self.handle.current().policies().len())
            .field("mode", &self.mode)
            .field("audit", &self.audit.is_some())
            .finish()
    }
}

impl<S> Layer<S> for PolicyLayer {
    type Service = Enforce<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Enforce {
            inner,
            policies: self.handle.subscribe(),
            mode: self.mode,
            audit: self.audit.clone(),
        }
    }
}

/// The service [`PolicyLayer`] produces.
///
/// It reads the current policy set through a [`watch::Receiver`] on every call,
/// so a [`PolicyHandle::store`] elsewhere is visible to the next request without
/// rebuilding the stack.
#[derive(Clone, Debug)]
pub struct Enforce<S> {
    inner: S,
    policies: watch::Receiver<Arc<skimasque_policy::PolicySet>>,
    mode: Mode,
    audit: Option<Arc<dyn AuditSink>>,
}

impl<S> Service<TunnelRequest> for Enforce<S>
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection>,
    S::Future: Send + 'static,
{
    type Response = Accepted;
    type Error = Rejection;
    type Future = TunnelFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: TunnelRequest) -> Self::Future {
        let Some(target) = request.target().cloned() else {
            return Box::pin(std::future::ready(Err(Rejection::new(
                StatusCode::NOT_IMPLEMENTED,
                "policy enforcement covers connect-udp and connect-tcp only",
            ))));
        };

        let application = request
            .headers()
            .get(APPLICATION_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();

        let workload = request
            .extensions()
            .get::<skimasque_policy::WorkloadIdentity>()
            .cloned()
            .unwrap_or_default();

        let transport = policy_transport(request.protocol());
        let ctx = skimasque_policy::RequestContext {
            workload,
            application: application.clone(),
            transport,
            destination: policy_destination(&target),
        };
        // Read the live set once, so this request is evaluated against a single
        // consistent snapshot even if a reload lands mid-call.
        let policies = self.policies.borrow().clone();
        let decision = policies.evaluate(&ctx);

        if self.mode == Mode::Observe {
            let would = if decision.is_allow() { "allow" } else { "deny" };
            tracing::info!(
                target: "masque::observe",
                application = %application,
                transport = transport.as_str(),
                destination = %target,
                would_be = would,
                "observed tunnel request"
            );
            // Authorize by trust so the proxy's own floor is still the only
            // thing between the client and the network.
            request.authorize(AuthorizedDestination::trusting(target));
            return Box::pin(self.inner.call(request));
        }

        // The audit trail records the decision itself, before it is acted on: a
        // later quota, resolution or address-floor failure can still stop an
        // `allow` from becoming a tunnel, and that is a separate event.
        if let Some(sink) = &self.audit {
            sink.record(&AuditEvent::from_decision(
                &decision,
                request.protocol().upgrade_token(),
                application.as_str(),
                target.to_string(),
                request.client_addr().to_string(),
                &ctx.workload,
            ));
        }

        match decision {
            skimasque_policy::Decision::Allow(allowed) => {
                debug!(policy = %allowed.policy, rule = %allowed.rule, "policy allowed the tunnel");
                request.authorize(AuthorizedDestination::from_decision(target, allowed));
                Box::pin(self.inner.call(request))
            }
            skimasque_policy::Decision::Deny(denied) => {
                debug!(reason = ?denied.reason, "policy denied the tunnel");
                Box::pin(std::future::ready(Err(denial_rejection(&denied))))
            }
        }
    }
}

/// The policy engine's transport for a MASQUE protocol. Only connect-udp and
/// connect-tcp reach here -- connect-ip is turned away before the policy layer.
fn policy_transport(protocol: Protocol) -> skimasque_policy::Transport {
    match protocol {
        Protocol::ConnectUdp => skimasque_policy::Transport::Udp,
        _ => skimasque_policy::Transport::Tcp,
    }
}

/// Translate a resolved-name-agnostic [`Target`] into the policy engine's
/// pre-DNS destination type.
fn policy_destination(target: &Target) -> skimasque_policy::Destination {
    let host = match &target.host {
        TargetHost::Name(name) => skimasque_policy::Host::Name(name.to_ascii_lowercase()),
        TargetHost::Ip(ip) => skimasque_policy::Host::Ip(*ip),
    };
    skimasque_policy::Destination {
        host,
        port: target.port,
    }
}

/// Render a policy denial as a `403` a client can act on, carrying the reason
/// and the fix in `Proxy-Status`.
fn denial_rejection(denied: &skimasque_policy::Denied) -> Rejection {
    let detail = format!("{} suggested rule: {}", denied.reason.summary(), denied.suggested_rule);
    Rejection::new(StatusCode::FORBIDDEN, detail).with_proxy_error("destination_prohibited")
}

/// Enforces the resource limits a policy decision carries.
///
/// It sits directly below [`PolicyLayer`] and reads the `Allow` decision that
/// layer attached:
///
///  - `limits.connections` -- a per-policy semaphore of that size; a request
///    that cannot get a permit is refused `503`. The permit rides on the
///    [`Accepted`] as a [`TunnelGuard`].
///  - `limits.bandwidth` and `limits.packets_per_second` -- a per-policy
///    [`RateLimiter`] each, shared by every tunnel the policy allows, that the
///    data-plane relay paces traffic against.
///  - `limits.total_bytes` -- a per-tunnel transfer ceiling the relay stops at.
///
/// The per-policy maps are keyed by policy name, so a policy whose limit changes
/// on hot-reload keeps the limiter it was first seen with until the proxy
/// restarts. `limits.connections`/`bandwidth`/`packets_per_second` are aggregate
/// across the policy -- the stand-in for per-session until issued credentials
/// carry a session -- while `total_bytes` is per-tunnel for now.
#[derive(Clone, Default)]
pub struct QuotaLayer {
    permits: Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>>>,
    bandwidth: Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<RateLimiter>>>>,
    packets: Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<RateLimiter>>>>,
}

impl QuotaLayer {
    pub fn new() -> Self {
        Self::default()
    }

    fn semaphore_for(&self, policy: &str, size: usize) -> Arc<tokio::sync::Semaphore> {
        self.permits
            .lock()
            .expect("quota table is not poisoned")
            .entry(policy.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(size)))
            .clone()
    }

    fn bandwidth_for(&self, policy: &str, bits_per_sec: u64) -> Arc<RateLimiter> {
        self.bandwidth
            .lock()
            .expect("quota table is not poisoned")
            .entry(policy.to_owned())
            .or_insert_with(|| Arc::new(RateLimiter::bandwidth(bits_per_sec)))
            .clone()
    }

    fn packets_for(&self, policy: &str, packets_per_sec: u64) -> Arc<RateLimiter> {
        self.packets
            .lock()
            .expect("quota table is not poisoned")
            .entry(policy.to_owned())
            .or_insert_with(|| Arc::new(RateLimiter::packets(packets_per_sec)))
            .clone()
    }
}

impl std::fmt::Debug for QuotaLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuotaLayer").finish_non_exhaustive()
    }
}

impl<S> Layer<S> for QuotaLayer {
    type Service = Meter<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Meter {
            inner,
            layer: self.clone(),
        }
    }
}

/// The service [`QuotaLayer`] produces.
#[derive(Clone)]
pub struct Meter<S> {
    inner: S,
    layer: QuotaLayer,
}

impl<S> std::fmt::Debug for Meter<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Meter").finish_non_exhaustive()
    }
}

impl<S> Service<TunnelRequest> for Meter<S>
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection>,
    S::Future: Send + 'static,
{
    type Response = Accepted;
    type Error = Rejection;
    type Future = TunnelFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: TunnelRequest) -> Self::Future {
        let decision = request
            .authorized_destination()
            .and_then(|authorized| authorized.decision());
        let permit = decision.and_then(|allowed| {
            allowed
                .limits
                .concurrent_connections
                .map(|max| (allowed.policy.clone(), max))
        });
        let tunnel_limits = decision.map_or_else(TunnelLimits::default, |allowed| TunnelLimits {
            bandwidth: allowed
                .limits
                .bandwidth_bits_per_sec
                .map(|bits| self.layer.bandwidth_for(&allowed.policy, bits)),
            packet_rate: allowed
                .limits
                .packets_per_sec
                .map(|pps| self.layer.packets_for(&allowed.policy, pps)),
            total_bytes: allowed.limits.total_bytes,
        });

        let permit = match permit {
            Some((policy, max)) => {
                let semaphore = self.layer.semaphore_for(&policy, max as usize);
                match tokio::sync::Semaphore::try_acquire_owned(semaphore) {
                    Ok(permit) => Some(permit),
                    Err(_) => {
                        debug!(%policy, max, "concurrent tunnel limit reached");
                        return Box::pin(std::future::ready(Err(Rejection::new(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "the concurrent tunnel limit for this policy is reached",
                        )
                        .with_proxy_error("connection_limit_reached"))));
                    }
                }
            }
            None => None,
        };

        let future = self.inner.call(request);
        Box::pin(async move {
            let mut accepted = future.await?;
            if let Some(permit) = permit {
                accepted = accepted.hold(permit);
            }
            if !tunnel_limits.is_empty() {
                accepted = accepted.with_limits(tunnel_limits);
            }
            Ok(accepted)
        })
    }
}

/// Requires a bearer token in `Proxy-Authorization`.
///
/// A public MASQUE proxy without authorization is an open relay, so this is
/// deliberately easy to reach for. It is the simplest useful scheme, not the
/// only one worth having: anything that inspects
/// [`TunnelRequest::headers`] can replace it.
#[derive(Clone)]
pub struct AuthorizeLayer {
    expected: Arc<str>,
}

impl AuthorizeLayer {
    pub fn bearer(token: impl Into<Arc<str>>) -> Self {
        Self {
            expected: token.into(),
        }
    }
}

impl std::fmt::Debug for AuthorizeLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the token.
        f.debug_struct("AuthorizeLayer").finish_non_exhaustive()
    }
}

impl<S> Layer<S> for AuthorizeLayer {
    type Service = Authorize<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Authorize {
            inner,
            expected: self.expected.clone(),
        }
    }
}

/// The service produced by [`AuthorizeLayer`].
#[derive(Clone, Debug)]
pub struct Authorize<S> {
    inner: S,
    expected: Arc<str>,
}

impl<S> Service<TunnelRequest> for Authorize<S>
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection>,
    S::Future: Send + 'static,
{
    type Response = Accepted;
    type Error = Rejection;
    type Future = TunnelFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: TunnelRequest) -> Self::Future {
        let presented = request
            .headers()
            .get(http::header::PROXY_AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));

        match presented {
            Some(token) if constant_time_eq(token.as_bytes(), self.expected.as_bytes()) => {
                let future = self.inner.call(request);
                Box::pin(future)
            }
            _ => Box::pin(std::future::ready(Err(Rejection::proxy_auth_required(
                "Bearer",
            )))),
        }
    }
}

/// Compare two byte strings without leaking their contents through timing.
///
/// Length is not secret here -- an attacker learns it from the challenge
/// anyway -- but the byte comparison must not stop at the first mismatch.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn udp_request(target: &str, app: Option<&str>) -> TunnelRequest {
        let mut builder = http::Request::builder();
        if let Some(app) = app {
            builder = builder.header(APPLICATION_HEADER, app);
        }
        let parts = builder.body(()).unwrap().into_parts().0;
        TunnelRequest::new(
            Protocol::ConnectUdp,
            Destination::Udp(Target::parse(target).unwrap()),
            "203.0.113.1:9000".parse().unwrap(),
            parts,
        )
    }

    /// An inner service that records the authorized destination it was handed
    /// and then declines, so no real socket is needed.
    #[derive(Clone, Default)]
    struct Spy(Arc<Mutex<Option<Option<Target>>>>);

    impl Service<TunnelRequest> for Spy {
        type Response = Accepted;
        type Error = Rejection;
        type Future = TunnelFuture;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: TunnelRequest) -> Self::Future {
            *self.0.lock().unwrap() = Some(
                request
                    .authorized_destination()
                    .map(|authorized| authorized.target().clone()),
            );
            Box::pin(std::future::ready(Err(Rejection::unavailable("spy"))))
        }
    }

    fn policy_set(toml: &str) -> skimasque_policy::PolicySet {
        skimasque_policy::PolicySet::from_documents([("p.toml", toml)]).unwrap()
    }

    const ALLOW_TF: &str = r#"
        name = "prod"
        [[rules]]
        application = "terraform"
        action = "allow"
        destinations = ["api.production.example.com:443"]
    "#;

    #[tokio::test]
    async fn the_policy_layer_attaches_an_authorized_destination_on_allow() {
        let spy = Spy::default();
        let mut service = PolicyLayer::new(policy_set(ALLOW_TF)).layer(spy.clone());
        let request = udp_request("api.production.example.com:443", Some("terraform"));

        let _ = service.call(request).await;
        let seen = spy.0.lock().unwrap().clone();
        let authorized = seen.expect("inner was called").expect("a destination was attached");
        assert_eq!(authorized.to_string(), "api.production.example.com:443");
    }

    #[tokio::test]
    async fn the_policy_layer_denies_an_unlisted_destination_without_calling_inner() {
        let spy = Spy::default();
        let mut service = PolicyLayer::new(policy_set(ALLOW_TF)).layer(spy.clone());
        let request = udp_request("evil.example.com:443", Some("terraform"));

        let rejection = service.call(request).await.unwrap_err();
        assert_eq!(rejection.status(), StatusCode::FORBIDDEN);
        assert!(spy.0.lock().unwrap().is_none(), "inner must not be called on a denial");
    }

    #[tokio::test]
    async fn a_missing_application_header_is_just_an_empty_application() {
        // The empty application matches no `terraform` rule, so this denies --
        // fail-closed, not a panic.
        let spy = Spy::default();
        let mut service = PolicyLayer::new(policy_set(ALLOW_TF)).layer(spy.clone());
        let request = udp_request("api.production.example.com:443", None);
        assert!(service.call(request).await.is_err());
    }

    #[tokio::test]
    async fn observe_mode_forwards_a_would_be_denial_with_a_trusting_authorization() {
        let spy = Spy::default();
        let mut service = PolicyLayer::new(policy_set(ALLOW_TF))
            .observe()
            .layer(spy.clone());
        let request = udp_request("evil.example.com:443", Some("terraform"));

        let _ = service.call(request).await;
        let seen = spy.0.lock().unwrap().clone();
        let attached = seen
            .expect("inner was called despite the would-be denial")
            .expect("observe mode still attaches a trusting authorization");
        assert_eq!(attached.to_string(), "evil.example.com:443");
    }

    #[derive(Debug, Default)]
    struct RecordingSink(Mutex<Vec<AuditEvent>>);

    impl AuditSink for RecordingSink {
        fn record(&self, event: &AuditEvent) {
            self.0.lock().unwrap().push(event.clone());
        }
    }

    #[tokio::test]
    async fn the_policy_layer_audits_an_allow_with_the_rule_that_permitted_it() {
        let sink = Arc::new(RecordingSink::default());
        let mut service = PolicyLayer::new(policy_set(ALLOW_TF))
            .with_audit(sink.clone())
            .layer(Spy::default());

        let request = udp_request("api.production.example.com:443", Some("terraform"));
        let _ = service.call(request).await;

        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].decision, "allow");
        assert_eq!(events[0].protocol, "connect-udp");
        assert_eq!(events[0].application, "terraform");
        assert_eq!(events[0].destination, "api.production.example.com:443");
        assert!(events[0].rule.is_some(), "an allow names its rule");
    }

    #[tokio::test]
    async fn the_policy_layer_audits_a_denial_with_its_reason() {
        let sink = Arc::new(RecordingSink::default());
        let mut service = PolicyLayer::new(policy_set(ALLOW_TF))
            .with_audit(sink.clone())
            .layer(Spy::default());

        let request = udp_request("evil.example.com:443", Some("terraform"));
        let _ = service.call(request).await;

        let events = sink.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].decision, "deny");
        assert_eq!(events[0].reason.as_deref(), Some("No matching allow rule."));
    }

    #[tokio::test]
    async fn observe_mode_does_not_write_audit_records() {
        let sink = Arc::new(RecordingSink::default());
        let mut service = PolicyLayer::new(policy_set(ALLOW_TF))
            .observe()
            .with_audit(sink.clone())
            .layer(Spy::default());

        let request = udp_request("evil.example.com:443", Some("terraform"));
        let _ = service.call(request).await;

        assert!(
            sink.0.lock().unwrap().is_empty(),
            "observe mode authorizes nothing, so it audits nothing"
        );
    }

    #[tokio::test]
    async fn storing_a_new_policy_set_takes_effect_on_the_next_request() {
        // Start with a set that denies the destination for `terraform`, build the
        // stack once, then swap in one that allows it and show the very next call
        // flips from deny to allow without rebuilding the layer.
        let layer = PolicyLayer::new(policy_set(
            r#"
            name = "prod"
            [[rules]]
            application = "terraform"
            action = "deny"
            destinations = ["api.production.example.com:443"]
        "#,
        ));
        let handle = layer.handle();
        let spy = Spy::default();
        let mut service = layer.layer(spy.clone());

        let denied = service
            .call(udp_request("api.production.example.com:443", Some("terraform")))
            .await
            .unwrap_err();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert!(spy.0.lock().unwrap().is_none(), "the first set denies");

        handle.store(policy_set(ALLOW_TF));

        let _ = service
            .call(udp_request("api.production.example.com:443", Some("terraform")))
            .await;
        let seen = spy.0.lock().unwrap().clone();
        assert_eq!(
            seen.expect("inner was called after the reload")
                .expect("a destination was attached")
                .to_string(),
            "api.production.example.com:443",
            "the reloaded set allows the tunnel"
        );
    }

    #[test]
    fn the_handle_reports_the_set_currently_in_force() {
        let handle = PolicyHandle::new(policy_set(ALLOW_TF));
        assert_eq!(handle.current().policies()[0].rules.len(), 1);

        handle.store(policy_set(
            r#"
            name = "prod"
            [[rules]]
            application = "terraform"
            action = "allow"
            destinations = ["api.production.example.com:443"]
            [[rules]]
            application = "dns"
            action = "allow"
            destinations = ["1.1.1.1:53"]
        "#,
        ));
        assert_eq!(handle.current().policies()[0].rules.len(), 2);
    }

    /// An inner service that accepts every request with a real (loopback)
    /// socket, so the quota layer's permit handling can be exercised.
    #[derive(Clone)]
    struct AcceptAll;

    impl Service<TunnelRequest> for AcceptAll {
        type Response = Accepted;
        type Error = Rejection;
        type Future = TunnelFuture;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _request: TunnelRequest) -> Self::Future {
            Box::pin(async {
                let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                Ok(Accepted::udp(socket, "127.0.0.1:9".parse().unwrap()))
            })
        }
    }

    #[tokio::test]
    async fn the_quota_layer_caps_concurrent_tunnels_per_policy() {
        let set = policy_set(
            r#"
            name = "prod"
            [limits]
            connections = 1
            [[rules]]
            application = "dns"
            action = "allow"
            destinations = ["1.1.1.1:53"]
        "#,
        );
        let mut service = PolicyLayer::new(set).layer(QuotaLayer::new().layer(AcceptAll));

        let first = service
            .call(udp_request("1.1.1.1:53", Some("dns")))
            .await
            .expect("first tunnel is under the limit");

        let second = service.call(udp_request("1.1.1.1:53", Some("dns"))).await;
        assert_eq!(
            second.unwrap_err().status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "the second tunnel is over the per-policy limit of 1"
        );

        // Ending the first tunnel returns its permit.
        drop(first);
        assert!(
            service
                .call(udp_request("1.1.1.1:53", Some("dns")))
                .await
                .is_ok(),
            "a permit freed by a closed tunnel is reusable"
        );
    }

    #[tokio::test]
    async fn the_quota_layer_shares_one_limiter_per_policy_for_bandwidth_and_packets() {
        let quota = QuotaLayer::new();
        let mut service = PolicyLayer::new(policy_set(
            r#"
            name = "prod"
            [limits]
            bandwidth = "1Mbps"
            packets_per_second = 500
            bytes = "10MB"
            [[rules]]
            application = "dns"
            action = "allow"
            destinations = ["1.1.1.1:53"]
        "#,
        ))
        .layer(quota.layer(AcceptAll));

        let one = service
            .call(udp_request("1.1.1.1:53", Some("dns")))
            .await
            .unwrap();
        let two = service
            .call(udp_request("1.1.1.1:53", Some("dns")))
            .await
            .unwrap();

        let (a, b) = (one.limits(), two.limits());
        assert!(
            Arc::ptr_eq(
                a.bandwidth.as_ref().expect("bandwidth limited"),
                b.bandwidth.as_ref().expect("bandwidth limited"),
            ),
            "every tunnel of a policy shares one bandwidth limiter"
        );
        assert!(
            Arc::ptr_eq(
                a.packet_rate.as_ref().expect("packet limited"),
                b.packet_rate.as_ref().expect("packet limited"),
            ),
            "and one packet-rate limiter"
        );
        assert_eq!(a.total_bytes, Some(10_000_000), "the transfer ceiling is per tunnel");

        // A policy with no limits attaches nothing.
        let mut open =
            PolicyLayer::new(policy_set(ALLOW_TF)).layer(QuotaLayer::new().layer(AcceptAll));
        let plain = open
            .call(udp_request("api.production.example.com:443", Some("terraform")))
            .await
            .unwrap();
        assert!(plain.limits().bandwidth.is_none());
        assert!(plain.limits().packet_rate.is_none());
        assert!(plain.limits().total_bytes.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn the_rate_limiter_paces_at_the_configured_rate() {
        // 1 Mbps = 125 000 bytes/s; the burst allowance is one second of that.
        let limiter = RateLimiter::bandwidth(1_000_000);
        let started = tokio::time::Instant::now();

        // The first full burst is immediate.
        limiter.consume(125_000).await;
        assert!(started.elapsed() < Duration::from_millis(50), "the burst is not paced");

        // The next 125 000 bytes have to be earned back: ~1 second.
        limiter.consume(125_000).await;
        let waited = started.elapsed();
        assert!(
            (900..=1_200).contains(&waited.as_millis()),
            "expected ~1s of pacing, waited {waited:?}"
        );

        // The packet limiter is the same bucket over a different unit: a
        // 100-packet burst (one second of rate), then 100/s.
        let packets = RateLimiter::packets(100);
        let start = tokio::time::Instant::now();
        for _ in 0..150 {
            packets.consume(1).await;
        }
        assert!(
            (400..=800).contains(&start.elapsed().as_millis()),
            "100 burst + 50 paced packets at 100/s is ~0.5s, took {:?}",
            start.elapsed()
        );
    }

    /// A verifier that answers with a fixed result, so the layer's plumbing
    /// can be tested without any crypto.
    #[derive(Debug)]
    struct StubVerifier(Result<skimasque_policy::WorkloadIdentity, String>);

    impl IdentityVerifier for StubVerifier {
        fn verify(
            &self,
            _token: String,
        ) -> Pin<Box<dyn Future<Output = Result<skimasque_policy::WorkloadIdentity, String>> + Send>>
        {
            let result = self.0.clone();
            Box::pin(std::future::ready(result))
        }
    }

    /// An inner service that records the workload identity it was handed, then
    /// declines.
    #[derive(Clone, Default)]
    struct IdentitySpy(Arc<Mutex<Option<Option<skimasque_policy::WorkloadIdentity>>>>);

    impl Service<TunnelRequest> for IdentitySpy {
        type Response = Accepted;
        type Error = Rejection;
        type Future = TunnelFuture;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: TunnelRequest) -> Self::Future {
            *self.0.lock().unwrap() = Some(
                request
                    .extensions()
                    .get::<skimasque_policy::WorkloadIdentity>()
                    .cloned(),
            );
            Box::pin(std::future::ready(Err(Rejection::unavailable("spy"))))
        }
    }

    fn bearer_request(token: Option<&str>) -> TunnelRequest {
        let mut builder = http::Request::builder();
        if let Some(token) = token {
            builder = builder.header(http::header::PROXY_AUTHORIZATION, format!("Bearer {token}"));
        }
        let parts = builder.body(()).unwrap().into_parts().0;
        TunnelRequest::new(
            Protocol::ConnectUdp,
            Destination::Udp(Target::parse("api.example.com:443").unwrap()),
            "203.0.113.1:9000".parse().unwrap(),
            parts,
        )
    }

    #[tokio::test]
    async fn the_identity_layer_forwards_a_verified_identity_to_the_inner_service() {
        let identity = skimasque_policy::WorkloadIdentity {
            organization: Some("acme".to_owned()),
            repository: Some("acme/widget".to_owned()),
            ..Default::default()
        };
        let spy = IdentitySpy::default();
        let mut service =
            IdentityLayer::new(Arc::new(StubVerifier(Ok(identity.clone())))).layer(spy.clone());

        let _ = service.call(bearer_request(Some("a.token"))).await;

        let seen = spy.0.lock().unwrap().clone();
        assert_eq!(
            seen.expect("inner was called").expect("an identity was attached"),
            identity
        );
    }

    #[tokio::test]
    async fn a_request_with_no_bearer_is_challenged_and_the_inner_is_untouched() {
        let spy = IdentitySpy::default();
        let mut service = IdentityLayer::new(Arc::new(StubVerifier(Ok(Default::default()))))
            .layer(spy.clone());

        let rejection = service.call(bearer_request(None)).await.unwrap_err();
        assert_eq!(rejection.status(), StatusCode::PROXY_AUTHENTICATION_REQUIRED);
        assert!(spy.0.lock().unwrap().is_none(), "inner must not be called");
    }

    #[tokio::test]
    async fn a_token_that_does_not_verify_is_forbidden_and_the_inner_is_untouched() {
        let spy = IdentitySpy::default();
        let mut service =
            IdentityLayer::new(Arc::new(StubVerifier(Err("bad signature".to_owned()))))
                .layer(spy.clone());

        let rejection = service.call(bearer_request(Some("nope"))).await.unwrap_err();
        assert_eq!(rejection.status(), StatusCode::FORBIDDEN);
        assert!(spy.0.lock().unwrap().is_none(), "inner must not be called");
    }

    #[test]
    fn constant_time_comparison_agrees_with_equality() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"tokeN"));
        assert!(!constant_time_eq(b"token", b"token "));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn rejections_carry_the_status_a_client_will_act_on() {
        assert_eq!(Rejection::forbidden("no").status(), StatusCode::FORBIDDEN);
        assert_eq!(Rejection::dns_error("no").status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            Rejection::proxy_auth_required("Bearer").status(),
            StatusCode::PROXY_AUTHENTICATION_REQUIRED
        );
    }

    /// The token must never reach a log line through the layer's Debug output.
    #[test]
    fn authorize_layer_does_not_print_its_token() {
        let layer = AuthorizeLayer::bearer("super-secret");
        assert!(!format!("{layer:?}").contains("super-secret"));
    }

    #[tokio::test]
    async fn a_literal_ip_target_resolves_to_itself() {
        let target = Target::parse("192.0.2.6:443").unwrap();
        assert_eq!(
            resolve(&target).await.unwrap(),
            vec!["192.0.2.6:443".parse::<SocketAddr>().unwrap()]
        );
    }

    #[tokio::test]
    async fn an_unresolvable_name_is_a_dns_error() {
        let target = Target::parse("this-name-does-not-exist.invalid:53").unwrap();
        let rejection = resolve(&target).await.unwrap_err();
        assert_eq!(rejection.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn sockets_are_bound_in_the_targets_family() {
        let v4 = connect_socket("127.0.0.1:9".parse().unwrap()).await.unwrap();
        assert!(v4.local_addr().unwrap().is_ipv4());
    }
}

#[cfg(test)]
mod size_tests {
    use super::Rejection;

    /// `Rejection` is the error half of every `Result` the proxy service
    /// returns, so it is worth keeping small. Boxing the header map is what
    /// buys this; the bound is a tripwire, not a target.
    #[test]
    fn rejection_stays_small_enough_to_pass_around_by_value() {
        assert!(
            std::mem::size_of::<Rejection>() <= 64,
            "Rejection grew to {} bytes",
            std::mem::size_of::<Rejection>()
        );
    }
}
