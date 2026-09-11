//! The MASQUE proxy: accepting CONNECT-UDP requests and relaying datagrams.
//!
//! The proxy is a thin shell around a [`tower::Service`]. Everything policy-
//! shaped -- who may open a tunnel, to where, how many at once -- belongs in
//! that service or in a layer above it; what lives here is the part that is the
//! same for every deployment: validating the request against RFC 9298, sending
//! a conforming response, and moving bytes between a UDP socket and a QUIC
//! connection.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::{Buf, Bytes};
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode};
use skimasque_core::connect_udp::{self, Target};
use skimasque_core::{Protocol, UriTemplate, CAPSULE_PROTOCOL_HEADER, CAPSULE_PROTOCOL_TRUE};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{mpsc, watch};
use tokio::time::{sleep, Instant};
use tower::{Service, ServiceExt};
use tracing::{debug, info, trace, Instrument};

use crate::capsules::CapsulePump;
use crate::dgram::{DatagramRoute, DatagramRouter};
use crate::service::{Accepted, AcceptedKind, Destination, Rejection, TunnelLimits, TunnelRequest};
use crate::{tls, Error};

type H3Connection = h3::server::Connection<h3_quinn::Connection, Bytes>;
type BidiStream = h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;
type RecvStream = h3::server::RequestStream<h3_quinn::RecvStream, Bytes>;
type SendStream = h3::server::RequestStream<h3_quinn::SendStream<Bytes>, Bytes>;

/// The largest UDP datagram that can arrive on a socket.
const MAX_UDP_PAYLOAD: usize = 65_535;

/// The read buffer for each direction of a TCP relay. QUIC flow control, not
/// this number, bounds how much is in flight; it only sets the syscall size.
const TCP_RELAY_BUFFER: usize = 64 * 1024;

/// How long [`Server::run`] lets in-flight tunnels finish after a shutdown
/// signal before the QUIC endpoint is closed under them. [`Server::run_until`]
/// takes this as an argument.
pub const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

/// The QUIC application error code the proxy closes connections with when it is
/// shutting down. Arbitrary but stable, so a client can recognise it.
const SHUTTING_DOWN_CODE: u32 = 0x100;

/// Holds the `skimasque_connections_active` gauge up for a connection's life,
/// decrementing it on drop so the count stays right even if the task panics.
struct ActiveConnection;

impl ActiveConnection {
    fn start() -> Self {
        crate::metrics::connection_started();
        Self
    }
}

impl Drop for ActiveConnection {
    fn drop(&mut self) {
        crate::metrics::connection_ended();
    }
}

/// The `skimasque_tunnels_active` equivalent of [`ActiveConnection`]; also emits
/// the `skimasque_tunnels_opened_total` counter for `protocol` on creation.
struct ActiveTunnel;

impl ActiveTunnel {
    fn open(protocol: &'static str) -> Self {
        crate::metrics::tunnel_opened(protocol);
        Self
    }
}

impl Drop for ActiveTunnel {
    fn drop(&mut self) {
        crate::metrics::tunnel_closed();
    }
}

/// The rate at which new QUIC connections are accepted, as a token bucket.
///
/// `per_second` is the sustained ceiling; `burst` is how many connections may
/// arrive in a clump -- a CI matrix starting fifty jobs at once -- before the
/// sustained rate takes over. A connection that arrives with the bucket empty is
/// refused with `CONNECTION_REFUSED`; a well-behaved client backs off and
/// retries.
#[derive(Debug, Clone, Copy)]
pub struct ConnectionRate {
    pub per_second: u32,
    pub burst: u32,
}

/// A leaky/token bucket over [`Instant`], refilled lazily on each check. Not
/// thread safe -- the accept loop is the only caller.
#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_per_sec: f64,
    last: std::time::Instant,
}

impl TokenBucket {
    fn new(rate: &ConnectionRate) -> Self {
        let capacity = f64::from(rate.burst.max(1));
        Self {
            tokens: capacity,
            capacity,
            refill_per_sec: f64::from(rate.per_second).max(0.0),
            last: std::time::Instant::now(),
        }
    }

    /// Take one token if the bucket has one. `false` means "over the rate".
    fn try_take(&mut self) -> bool {
        let now = std::time::Instant::now();
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Would the bucket be back at capacity if checked now? Such a bucket holds
    /// no more state than an absent one, so [`PerSourceRate`] can drop it.
    fn is_full(&self) -> bool {
        let elapsed = self.last.elapsed().as_secs_f64();
        self.tokens + elapsed * self.refill_per_sec >= self.capacity
    }
}

/// The new-connection rate limit applied *per remote IP address* rather than
/// globally: each source gets its own [`TokenBucket`], so one client stuck in a
/// connect/disconnect loop cannot spend the whole gateway's budget and starve
/// every other runner.
///
/// The map of buckets is bounded at [`MAX_SOURCES`]. When it is full a new
/// source first triggers a sweep of fully-refilled (idle) buckets, which are
/// indistinguishable from absent ones; if that does not free a slot the
/// least-recently-seen source is evicted. An evicted source simply gets a fresh
/// full bucket next time it appears, so eviction is fail-open for that source
/// and cannot itself be used to deny a victim.
///
/// The peer address comes from [`quinn::Incoming::remote_address`] and may not
/// be QUIC-address-validated yet, so a spoofed-source flood can still churn the
/// map -- but only within the [`MAX_SOURCES`] bound, and quinn's Retry-based
/// validation gates the expensive handshake regardless.
#[derive(Debug)]
struct PerSourceRate {
    rate: ConnectionRate,
    buckets: HashMap<IpAddr, TokenBucket>,
}

/// The most distinct source IPs [`PerSourceRate`] tracks at once. At ~64 bytes a
/// bucket this is a few hundred KB; large enough to cover any real fleet's
/// egress addresses, small enough that a spoofed-source flood cannot grow it.
const MAX_SOURCES: usize = 4096;

impl PerSourceRate {
    fn new(rate: ConnectionRate) -> Self {
        Self {
            rate,
            buckets: HashMap::new(),
        }
    }

    /// Take a token from `ip`'s bucket. `false` means that source is over its
    /// rate.
    fn try_take(&mut self, ip: IpAddr) -> bool {
        if !self.buckets.contains_key(&ip) && self.buckets.len() >= MAX_SOURCES {
            self.make_room();
        }
        let rate = self.rate;
        self.buckets
            .entry(ip)
            .or_insert_with(|| TokenBucket::new(&rate))
            .try_take()
    }

    /// Free at least one slot: drop every idle bucket, then, if still full, the
    /// least-recently-seen source.
    fn make_room(&mut self) {
        self.buckets.retain(|_, bucket| !bucket.is_full());
        if self.buckets.len() < MAX_SOURCES {
            return;
        }
        if let Some(oldest) = self
            .buckets
            .iter()
            .min_by_key(|(_, bucket)| bucket.last)
            .map(|(ip, _)| *ip)
        {
            self.buckets.remove(&oldest);
        }
    }
}

/// A per-source rate limit on the token-exchange endpoint.
///
/// The QUIC accept loop's [`PerSourceRate`] runs single-threaded; the exchange
/// handler runs on many connection tasks at once, so this wraps the same
/// structure in a mutex. It exists because each exchange verifies an RS256
/// signature (and may fetch a JWK Set), and the endpoint is answered *before*
/// the tower concurrency limit -- so without it a single QUIC connection can
/// stream unbounded mint attempts.
#[derive(Debug)]
pub struct ExchangeRateLimiter(std::sync::Mutex<PerSourceRate>);

impl ExchangeRateLimiter {
    fn new(rate: ConnectionRate) -> Self {
        Self(std::sync::Mutex::new(PerSourceRate::new(rate)))
    }

    /// `false` means this source is over its exchange rate.
    fn check(&self, ip: IpAddr) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_take(ip)
    }
}

/// Ceilings that keep one client, or one connection, from consuming the whole
/// gateway.
///
/// These are orthogonal to the [`tower`] layer stack: `GlobalConcurrencyLimitLayer`
/// caps how many tunnel *requests* are being authorized at once, while these cap
/// the connections and established tunnels the accept path is holding open,
/// bound how fast new connections arrive, and reclaim tunnels that have gone
/// silent. The defaults suit a public gateway; a trusted single-tenant
/// deployment can widen or disable them.
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// QUIC connections served at once. A further [`quinn::Incoming`] is refused
    /// with `CONNECTION_REFUSED` until one ends. `None` removes the cap.
    pub max_connections: Option<usize>,
    /// The rate at which new connections are accepted. `max_connections` bounds
    /// the steady state; this bounds the churn -- a client that opens and closes
    /// connections in a loop never reaches the concurrent cap. Global across all
    /// sources. `None` removes the limit.
    pub connection_rate: Option<ConnectionRate>,
    /// The same limit as `connection_rate` but tracked *per remote IP*, so a
    /// single misbehaving source cannot spend the global budget and starve
    /// everyone else. Checked before `connection_rate`. Sized generously by
    /// default because a self-hosted fleet often shares one NAT egress IP; raise
    /// it, or set `None`, for a deployment where many runners sit behind one
    /// address.
    pub per_source_rate: Option<ConnectionRate>,
    /// Per-remote-IP rate limit on the token-exchange endpoint (distinct from
    /// tunnels: an exchange costs an RS256 verify and maybe a JWKS fetch, and is
    /// answered before the tower concurrency limit). `None` removes it. Shares
    /// its type with the connection rates but the unit here is exchange
    /// requests, not connections.
    pub exchange_rate: Option<ConnectionRate>,
    /// Tunnels open at once on a single QUIC connection. A request over that
    /// number is answered `503` and the connection stays up. `None` removes the
    /// cap.
    pub max_tunnels_per_connection: Option<usize>,
    /// Tear a tunnel down once it has carried no payload in either direction for
    /// this long. The QUIC idle timeout still applies to the connection as a
    /// whole; this reclaims a single idle tunnel within an otherwise busy
    /// connection. `None` disables it.
    pub tunnel_idle_timeout: Option<Duration>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_connections: Some(1024),
            connection_rate: Some(ConnectionRate {
                per_second: 50,
                burst: 200,
            }),
            per_source_rate: Some(ConnectionRate {
                per_second: 20,
                burst: 60,
            }),
            exchange_rate: Some(ConnectionRate {
                per_second: 10,
                burst: 30,
            }),
            max_tunnels_per_connection: Some(256),
            tunnel_idle_timeout: Some(Duration::from_secs(120)),
        }
    }
}

impl ResourceLimits {
    /// Every ceiling removed. For a trusted deployment that does its own
    /// resource control, or a test.
    pub fn unlimited() -> Self {
        Self {
            max_connections: None,
            connection_rate: None,
            per_source_rate: None,
            exchange_rate: None,
            max_tunnels_per_connection: None,
            tunnel_idle_timeout: None,
        }
    }
}

/// Deployment-wide proxy settings.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// The URI Template this proxy serves. Requests whose path does not match
    /// it are rejected, which is also how the target is recovered.
    pub template: UriTemplate,
    /// The name this proxy gives itself in `Proxy-Status` headers (RFC 9209).
    pub proxy_name: String,
    /// If set, the proxy also answers the token-exchange endpoint
    /// ([`CREDENTIAL_EXCHANGE_PATH`](crate::exchange::CREDENTIAL_EXCHANGE_PATH)):
    /// a client POSTs an identity token and gets a platform credential back.
    pub minter: Option<Arc<dyn crate::exchange::CredentialMinter>>,
    /// Ceilings on connections, tunnels-per-connection, and tunnel idle time.
    pub limits: ResourceLimits,
    /// The live token-exchange rate limiter, rebuilt from
    /// `limits.exchange_rate` whenever the limits are set. Shared across the
    /// connection tasks, so cloning a `ProxyConfig` shares one limiter.
    exchange_limiter: Option<Arc<ExchangeRateLimiter>>,
}

impl ProxyConfig {
    /// Serve the default `.well-known` CONNECT-UDP template for `authority`.
    pub fn new(authority: &str) -> Result<Self, Error> {
        Ok(Self::for_template(UriTemplate::default_connect_udp(authority)?))
    }

    /// Serve `template`, with default name and [`ResourceLimits`]. Use the
    /// `with_*` methods to adjust; the private exchange limiter is kept in step
    /// with `limits` through them.
    pub fn for_template(template: UriTemplate) -> Self {
        let limits = ResourceLimits::default();
        Self {
            template,
            proxy_name: "skimasque".to_owned(),
            minter: None,
            exchange_limiter: exchange_limiter_for(&limits),
            limits,
        }
    }

    /// Answer the token-exchange endpoint with `minter`.
    pub fn with_minter(mut self, minter: Arc<dyn crate::exchange::CredentialMinter>) -> Self {
        self.minter = Some(minter);
        self
    }

    /// Replace the [`ResourceLimits`].
    pub fn with_limits(mut self, limits: ResourceLimits) -> Self {
        self.exchange_limiter = exchange_limiter_for(&limits);
        self.limits = limits;
        self
    }
}

fn exchange_limiter_for(limits: &ResourceLimits) -> Option<Arc<ExchangeRateLimiter>> {
    limits
        .exchange_rate
        .map(|rate| Arc::new(ExchangeRateLimiter::new(rate)))
}

/// A running MASQUE proxy.
#[derive(Debug)]
pub struct Server<S> {
    endpoint: quinn::Endpoint,
    service: S,
    config: Arc<ProxyConfig>,
}

/// A handle that swaps the certificate and key a [`Server`] presents.
///
/// [`reload`](Self::reload) replaces the TLS material used for *new* QUIC
/// handshakes; connections already established keep the certificate they
/// handshook with. Clone it freely -- every clone drives the same endpoint. This
/// is how a gateway rotates its certificate without a restart or a dropped
/// tunnel.
#[derive(Clone, Debug)]
pub struct TlsReloader {
    endpoint: quinn::Endpoint,
}

impl TlsReloader {
    /// Present `tls` to connections opened from now on.
    pub fn reload(&self, tls: rustls::ServerConfig) -> Result<(), Error> {
        self.endpoint
            .set_server_config(Some(tls::quic_server_config(tls)?));
        Ok(())
    }
}

impl<S> Server<S>
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection> + Clone + Send + 'static,
    S::Future: Send,
{
    /// Bind a QUIC endpoint and prepare to serve `service`.
    pub fn bind(
        addr: SocketAddr,
        tls: rustls::ServerConfig,
        service: S,
        config: ProxyConfig,
    ) -> Result<Self, Error> {
        let endpoint = quinn::Endpoint::server(tls::quic_server_config(tls)?, addr)?;
        Ok(Self {
            endpoint,
            service,
            config: Arc::new(config),
        })
    }

    /// The address the proxy is listening on.
    pub fn local_addr(&self) -> Result<SocketAddr, Error> {
        Ok(self.endpoint.local_addr()?)
    }

    /// A handle for rotating the certificate and key while the proxy runs.
    pub fn tls_reloader(&self) -> TlsReloader {
        TlsReloader {
            endpoint: self.endpoint.clone(),
        }
    }

    /// Accept connections until the QUIC endpoint is closed.
    ///
    /// This never returns on its own; drop the future, or use
    /// [`run_until`](Self::run_until) to shut the proxy down gracefully.
    pub async fn run(self) -> Result<(), Error> {
        self.run_until(std::future::pending::<()>(), DEFAULT_SHUTDOWN_GRACE)
            .await
    }

    /// Accept connections until `shutdown` resolves, then drain.
    ///
    /// When `shutdown` completes the proxy stops accepting new connections and
    /// asks every open HTTP/3 connection to close gracefully: each sends a
    /// GOAWAY and keeps serving the tunnels already in flight. Connections that
    /// have not finished within `grace` are then closed with a CONNECTION_CLOSE.
    /// Returns once every connection task has ended and the endpoint has
    /// flushed its closing frames to the network.
    pub async fn run_until(
        self,
        shutdown: impl Future<Output = ()>,
        grace: Duration,
    ) -> Result<(), Error> {
        info!(
            addr = %self.local_addr()?,
            template = self.config.template.as_str(),
            "MASQUE proxy listening"
        );

        // `drain` is flipped once to tell live connection tasks to send GOAWAY
        // and wind down; `connections` tracks those tasks so shutdown can wait
        // on them.
        let (drain_tx, drain_rx) = watch::channel(false);
        let mut connections = tokio::task::JoinSet::new();
        tokio::pin!(shutdown);

        let max_connections = self.config.limits.max_connections;
        let mut connection_rate = self.config.limits.connection_rate.as_ref().map(TokenBucket::new);
        let mut per_source_rate = self.config.limits.per_source_rate.map(PerSourceRate::new);

        loop {
            tokio::select! {
                incoming = self.endpoint.accept() => {
                    let Some(incoming) = incoming else { break };

                    // Reap anything that has already finished so the count is
                    // current, then turn a flood away here -- before a handshake
                    // task is spawned for it -- so an unwanted connection costs
                    // close to nothing.
                    while connections.try_join_next().is_some() {}
                    if max_connections.is_some_and(|max| connections.len() >= max) {
                        debug!(
                            client = %incoming.remote_address(),
                            open = connections.len(),
                            "connection limit reached; refusing"
                        );
                        crate::metrics::connection_refused("connection_limit");
                        incoming.refuse();
                        continue;
                    }
                    if per_source_rate
                        .as_mut()
                        .is_some_and(|limiter| !limiter.try_take(incoming.remote_address().ip()))
                    {
                        debug!(
                            client = %incoming.remote_address(),
                            "per-source connection rate exceeded; refusing"
                        );
                        crate::metrics::connection_refused("per_source_rate");
                        incoming.refuse();
                        continue;
                    }
                    if connection_rate.as_mut().is_some_and(|bucket| !bucket.try_take()) {
                        debug!(
                            client = %incoming.remote_address(),
                            "connection rate limit exceeded; refusing"
                        );
                        crate::metrics::connection_refused("rate_limit");
                        incoming.refuse();
                        continue;
                    }

                    crate::metrics::connection_accepted();
                    let service = self.service.clone();
                    let config = self.config.clone();
                    let drain = drain_rx.clone();
                    connections.spawn(async move {
                        let _active = ActiveConnection::start();
                        let client = incoming.remote_address();
                        if let Err(error) = serve_connection(incoming, service, config, drain).await {
                            debug!(%client, %error, "connection ended");
                        }
                    });
                }
                // Reap finished connection tasks so the set does not grow
                // without bound over the life of the proxy.
                Some(_) = connections.join_next(), if !connections.is_empty() => {}
                () = &mut shutdown => {
                    info!(connections = connections.len(), "shutdown requested; draining tunnels");
                    break;
                }
            }
        }

        // Stop accepting; ask every connection to GOAWAY and finish its tunnels.
        let _ = drain_tx.send(true);

        let drained = tokio::time::timeout(grace, async {
            while connections.join_next().await.is_some() {}
        })
        .await;
        match drained {
            Ok(()) => info!("all tunnels drained"),
            Err(_) => info!(
                remaining = connections.len(),
                "drain deadline reached; closing remaining tunnels"
            ),
        }

        // Close anything still open, then let quinn flush CONNECTION_CLOSE
        // before the endpoint (and often the process) goes away.
        self.close();
        connections.shutdown().await;
        self.endpoint.wait_idle().await;
        Ok(())
    }

    /// Close the QUIC endpoint immediately, aborting any in-flight tunnels.
    ///
    /// [`run_until`](Self::run_until) is the graceful path; this is the abrupt
    /// one.
    pub fn close(&self) {
        self.endpoint
            .close(SHUTTING_DOWN_CODE.into(), b"proxy shutting down");
    }
}

async fn serve_connection<S>(
    incoming: quinn::Incoming,
    service: S,
    config: Arc<ProxyConfig>,
    mut drain: watch::Receiver<bool>,
) -> Result<(), Error>
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection> + Clone + Send + 'static,
    S::Future: Send,
{
    let quic = incoming.await?;
    let client = quic.remote_address();
    let span = tracing::info_span!("connection", %client);

    async move {
        debug!("QUIC connection established");
        let router = DatagramRouter::spawn(quic.clone());
        let closer = quic.clone();
        let mut h3: H3Connection = h3::server::builder()
            .enable_datagram(true)
            .enable_extended_connect(true)
            .build(h3_quinn::Connection::new(quic))
            .await?;

        // Requests on this connection run as their own tasks; `tunnels` tracks
        // them so a graceful shutdown can wait for the ones in flight. Once the
        // proxy is draining, a GOAWAY goes out and no new request is accepted --
        // h3 rejects any that race it -- so the loop ends when `tunnels` empties.
        let mut tunnels = tokio::task::JoinSet::new();
        let mut draining = false;
        let mut outcome = Ok(());
        let max_tunnels = config.limits.max_tunnels_per_connection;

        loop {
            if draining && tunnels.is_empty() {
                break;
            }
            tokio::select! {
                accepted = h3.accept() => match accepted {
                    Ok(Some(resolver)) => {
                        // Reap finished tunnels so the count is current, then
                        // decide whether this one is over the per-connection cap.
                        while tunnels.try_join_next().is_some() {}
                        let over_limit =
                            max_tunnels.is_some_and(|max| tunnels.len() >= max);
                        if over_limit {
                            debug!(open = tunnels.len(), "per-connection tunnel limit reached; refusing");
                        }

                        let service = service.clone();
                        let config = config.clone();
                        let router = router.clone();
                        tunnels.spawn(async move {
                            let (request, stream) = match resolver.resolve_request().await {
                                Ok(resolved) => resolved,
                                Err(error) => {
                                    debug!(%error, "could not read request headers");
                                    return;
                                }
                            };
                            if over_limit {
                                crate::metrics::tunnel_rejected("per_connection_limit");
                                refuse_tunnel(stream, &config).await;
                                return;
                            }
                            serve_request(request, stream, service, config, router, client).await;
                        });
                    }
                    Ok(None) => break,
                    Err(error) => {
                        outcome = Err(Error::H3Connection(error));
                        break;
                    }
                },

                // Reap finished tunnels; while draining this is what eventually
                // empties the set and ends the loop.
                Some(_) = tunnels.join_next(), if !tunnels.is_empty() => {}

                changed = drain.changed(), if !draining => {
                    // A closed channel means the proxy itself is going away;
                    // drain this connection regardless.
                    let _ = changed;
                    draining = true;
                    debug!(open = tunnels.len(), "connection draining; sending GOAWAY");
                    if let Err(error) = h3.shutdown(0).await {
                        debug!(%error, "could not send GOAWAY");
                    }
                }
            }
        }

        // Any tunnels still here on exit are past the deadline or lost their
        // connection; drop them and tell the peer why the connection ended.
        tunnels.shutdown().await;
        if draining {
            closer.close(SHUTTING_DOWN_CODE.into(), b"proxy shutting down");
        }
        outcome
    }
    .instrument(span)
    .await
}

async fn serve_request<S>(
    request: Request<()>,
    mut stream: BidiStream,
    mut service: S,
    config: Arc<ProxyConfig>,
    router: DatagramRouter,
    client: SocketAddr,
) where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection> + Send,
    S::Future: Send,
{
    let stream_id = stream.id().into_inner();

    // The token-exchange endpoint is an ordinary request, not a tunnel: answer
    // it here before anything tries to read a target out of the path.
    if request.method() == Method::POST
        && request.uri().path() == crate::exchange::CREDENTIAL_EXCHANGE_PATH
    {
        serve_exchange(request, stream, &config, client.ip()).await;
        return;
    }

    let outcome = match parse_request(request, &config.template, client) {
        Ok(tunnel) => {
            let target = tunnel
                .target()
                .cloned()
                .expect("parse_request only accepts connect-udp and connect-tcp");
            match service.ready().await {
                Ok(service) => service
                    .call(tunnel)
                    .await
                    .map(|accepted| (accepted, target)),
                Err(rejection) => Err(rejection),
            }
        }
        Err(rejection) => Err(rejection),
    };

    let (accepted, target) = match outcome {
        Ok(accepted) => accepted,
        Err(rejection) => {
            debug!(stream_id, %rejection, "refusing tunnel");
            let response = rejection_response(rejection, &config.proxy_name);
            if let Err(error) = stream.send_response(response).await {
                trace!(%error, "could not send rejection");
            }
            let _ = stream.finish().await;
            return;
        }
    };

    let (kind, extra_headers, tunnel_guards, limits) = accepted.into_parts();
    let extra_headers = extra_headers.map(|headers| *headers).unwrap_or_default();
    let idle = config.limits.tunnel_idle_timeout;
    let span = tracing::info_span!("tunnel", stream_id, %target);

    match kind {
        AcceptedKind::Udp { socket, peer } => {
            // Register before responding. A client is entitled to send its first
            // datagram the instant it sees the 2xx, and that datagram can
            // overtake the response; claiming the route first means it is
            // queued, not dropped.
            let (route, inbound) = router.register(stream_id);

            if let Err(error) = stream.send_response(success_response(extra_headers)).await {
                debug!(%error, "could not send success response");
                return;
            }
            let (_send, recv) = stream.split();

            let _active = ActiveTunnel::open(Protocol::ConnectUdp.upgrade_token());
            info!(stream_id, %target, %peer, "tunnel open");
            relay(socket, route, inbound, recv, idle, limits)
                .instrument(span)
                .await;
        }
        AcceptedKind::Tcp { stream: tcp, peer } => {
            // A CONNECT-TCP stream carries raw bytes, not capsules: no datagram
            // router, and the success response must not announce the Capsule
            // Protocol.
            if let Err(error) = stream
                .send_response(tcp_success_response(extra_headers))
                .await
            {
                debug!(%error, "could not send success response");
                return;
            }
            let (send, recv) = stream.split();

            let _active = ActiveTunnel::open(Protocol::ConnectTcp.upgrade_token());
            info!(stream_id, %target, %peer, "tcp tunnel open");
            relay_tcp(tcp, send, recv, idle, limits)
                .instrument(span)
                .await;
        }
        #[cfg(feature = "connect-ip")]
        AcceptedKind::Ip(_) => {
            // parse_request rejects connect-ip before a service is ever called.
            unreachable!("server::serve_request only accepts connect-udp and connect-tcp");
        }
    }

    // Held until the tunnel is done: dropping these releases any quota permits
    // a layer reserved for it.
    drop(tunnel_guards);
    info!(stream_id, %target, "tunnel closed");
}

/// Answer a tunnel request `503` because the connection is already carrying its
/// maximum number of tunnels, then close just that stream. The connection stays
/// up so the client can retry once one of its tunnels ends.
async fn refuse_tunnel(mut stream: BidiStream, config: &ProxyConfig) {
    let rejection = Rejection::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "this connection is already carrying its maximum number of tunnels",
    )
    .with_proxy_error("connection_limit_reached");
    if let Err(error) = stream
        .send_response(rejection_response(rejection, &config.proxy_name))
        .await
    {
        trace!(%error, "could not send the tunnel-limit rejection");
    }
    let _ = stream.finish().await;
}

/// Answer the token-exchange endpoint: verify the presented identity token,
/// hand back a platform credential.
async fn serve_exchange(
    request: Request<()>,
    mut stream: BidiStream,
    config: &ProxyConfig,
    client: IpAddr,
) {
    use crate::exchange::{ExchangeBody, MintError};

    let Some(minter) = config.minter.clone() else {
        finish_json(
            &mut stream,
            StatusCode::NOT_FOUND,
            None,
            &error_body(
                "exchange_disabled",
                "this gateway does not issue credentials",
            ),
        )
        .await;
        return;
    };

    // Before the RS256 verify (and possible JWKS fetch): one source cannot
    // stream unbounded mint attempts down a single QUIC connection.
    if config
        .exchange_limiter
        .as_ref()
        .is_some_and(|limiter| !limiter.check(client))
    {
        debug!(%client, "token-exchange rate exceeded for this source; refusing");
        crate::metrics::exchange_throttled();
        finish_json(
            &mut stream,
            StatusCode::TOO_MANY_REQUESTS,
            Some((http::header::RETRY_AFTER, HeaderValue::from_static("1"))),
            &error_body(
                "rate_limited",
                "too many credential exchanges from this source; retry shortly",
            ),
        )
        .await;
        return;
    }

    let token = request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_owned);
    let Some(token) = token else {
        finish_json(
            &mut stream,
            StatusCode::UNAUTHORIZED,
            Some((
                http::header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Bearer"),
            )),
            &error_body(
                "missing_token",
                "present the identity token as `Authorization: Bearer`",
            ),
        )
        .await;
        return;
    };

    match minter.mint(token).await {
        Ok(minted) => {
            let body = serde_json::to_vec(&ExchangeBody {
                credential: minted.credential,
                token_type: "Bearer".to_owned(),
                expires_in: minted.expires_in.as_secs(),
            })
            .unwrap_or_default();
            finish_json(&mut stream, StatusCode::OK, None, &body).await;
        }
        Err(MintError::Unauthorized(detail)) => {
            debug!(%detail, "credential exchange refused");
            finish_json(
                &mut stream,
                StatusCode::FORBIDDEN,
                None,
                &error_body("invalid_grant", &detail),
            )
            .await;
        }
        Err(MintError::Unavailable(detail)) => {
            debug!(%detail, "credential exchange unavailable");
            finish_json(
                &mut stream,
                StatusCode::BAD_GATEWAY,
                None,
                &error_body("temporarily_unavailable", &detail),
            )
            .await;
        }
    }
}

/// Send a JSON body and close the stream.
async fn finish_json(
    stream: &mut BidiStream,
    status: StatusCode,
    extra_header: Option<(http::header::HeaderName, HeaderValue)>,
    body: &[u8],
) {
    let mut response = Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(())
        .expect("a status and one header always build");
    if let Some((name, value)) = extra_header {
        response.headers_mut().insert(name, value);
    }
    if let Err(error) = stream.send_response(response).await {
        trace!(%error, "could not send the exchange response");
        return;
    }
    if let Err(error) = stream.send_data(Bytes::copy_from_slice(body)).await {
        trace!(%error, "could not send the exchange body");
        return;
    }
    let _ = stream.finish().await;
}

fn error_body(code: &str, detail: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "error": code, "error_description": detail }))
        .unwrap_or_else(|_| br#"{"error":"internal_error"}"#.to_vec())
}

/// Check a request against RFC 9298, Section 3.4 and recover its target.
fn parse_request(
    request: Request<()>,
    template: &UriTemplate,
    client: SocketAddr,
) -> Result<TunnelRequest, Rejection> {
    let (parts, ()) = request.into_parts();

    if parts.method != Method::CONNECT {
        return Err(Rejection::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "MASQUE requests use the CONNECT method",
        ));
    }

    // The `:protocol` pseudo-header is what distinguishes an extended CONNECT
    // from a plain one; h3 surfaces it as a request extension.
    let Some(protocol) = parts.extensions.get::<h3::ext::Protocol>() else {
        // Classic CONNECT: the target is the authority, with no `:protocol`,
        // `:scheme` or `:path`. This is the TCP tunnel.
        let authority = parts
            .uri
            .authority()
            .map(|authority| authority.as_str().to_owned())
            .ok_or_else(|| {
                Rejection::bad_request("classic CONNECT needs an authority-form target")
            })?;
        let target = Target::parse(&authority).map_err(|error| {
            Rejection::new(StatusCode::BAD_REQUEST, format!("{error}"))
                .with_proxy_error("destination_not_found")
        })?;
        return Ok(TunnelRequest::new(
            Protocol::ConnectTcp,
            Destination::Tcp(target),
            client,
            parts,
        ));
    };

    let protocol: Protocol = protocol
        .as_str()
        .parse()
        .map_err(|_| Rejection::new(StatusCode::NOT_IMPLEMENTED, "unsupported :protocol"))?;
    if protocol != Protocol::ConnectUdp {
        return Err(Rejection::new(
            StatusCode::NOT_IMPLEMENTED,
            "this proxy supports extended CONNECT for connect-udp and classic CONNECT for TCP",
        ));
    }

    let path = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str().to_owned())
        .ok_or_else(|| Rejection::bad_request("request has no :path"))?;

    // Recovering the target is the same operation as matching the template, so
    // a path that does not match is indistinguishable from an unknown target.
    let target = Target::from_path(template, &path).map_err(|error| {
        Rejection::new(StatusCode::NOT_FOUND, format!("{error}"))
            .with_proxy_error("destination_not_found")
    })?;

    Ok(TunnelRequest::new(
        protocol,
        Destination::Udp(target),
        client,
        parts,
    ))
}

fn success_response(extra: HeaderMap) -> Response<()> {
    let mut response = Response::builder()
        .status(StatusCode::OK)
        // RFC 9297, Section 3.4: announce the Capsule Protocol so intermediaries
        // can process the stream without understanding connect-udp.
        .header(CAPSULE_PROTOCOL_HEADER, CAPSULE_PROTOCOL_TRUE)
        .body(())
        .expect("a status and one header always build");
    response.headers_mut().extend(extra);
    response
}

/// The success response for a CONNECT-TCP tunnel: `200` and nothing else.
///
/// Unlike [`success_response`], it must **not** carry `Capsule-Protocol`: the
/// stream that follows is a raw byte stream, not a capsule stream.
fn tcp_success_response(extra: HeaderMap) -> Response<()> {
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .body(())
        .expect("a bare status always builds");
    response.headers_mut().extend(extra);
    response
}

fn rejection_response(rejection: Rejection, proxy_name: &str) -> Response<()> {
    let (status, proxy_error, detail, extra) = rejection.into_parts();
    let mut response = Response::builder()
        .status(status)
        .body(())
        .expect("status codes always build");
    if let Some(extra) = extra {
        response.headers_mut().extend(*extra);
    }

    if let Some(error) = proxy_error {
        let value = format!(
            "{}; error={error}; details=\"{}\"",
            structured_token(proxy_name),
            structured_string(&detail)
        );
        if let Ok(value) = HeaderValue::from_str(&value) {
            response.headers_mut().insert("proxy-status", value);
        }
    }
    response
}

/// Reduce a name to something usable as a Structured Fields token (RFC 8941).
fn structured_token(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    if cleaned.is_empty() || !cleaned.starts_with(|c: char| c.is_ascii_alphabetic()) {
        "proxy".to_owned()
    } else {
        cleaned
    }
}

/// Escape a Structured Fields string, and keep it short enough for a header.
fn structured_string(detail: &str) -> String {
    detail
        .chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| {
            // Only backslash and double quote are escapable inside a string.
            if matches!(c, '"' | '\\') {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .take(256)
        .collect()
}

/// Add `n` to `moved` and report whether the tunnel's `total_bytes` ceiling has
/// been reached.
fn transfer_exceeded(moved: &mut u64, n: usize, cap: Option<u64>) -> bool {
    *moved = moved.saturating_add(n as u64);
    match cap {
        Some(limit) if *moved >= limit => {
            crate::metrics::transfer_cap_reached();
            true
        }
        _ => false,
    }
}

/// Move payloads between the UDP socket and the tunnel until either end closes
/// or the tunnel sits idle past `idle`.
///
/// `limits` is enforced here: the shared bandwidth and packet-rate buckets pace
/// each datagram (the relay task sleeps when a bucket is dry), and a per-tunnel
/// `total_bytes` ceiling closes the tunnel once it is reached.
async fn relay(
    socket: UdpSocket,
    route: DatagramRoute,
    mut inbound: mpsc::Receiver<Bytes>,
    stream: RecvStream,
    idle: Option<Duration>,
    limits: TunnelLimits,
) {
    let mut reader = tokio::spawn(read_capsules(stream, route.sink()));
    let mut buf = vec![0u8; MAX_UDP_PAYLOAD];
    let mut moved: u64 = 0;

    // A datagram in either direction pushes this deadline back; if it fires, the
    // tunnel has gone silent and is reclaimed. Disabled tunnels still arm the
    // timer far in the future and never consult it.
    let idle_step = idle.unwrap_or(Duration::from_secs(3600));
    let idle_timer = sleep(idle_step);
    tokio::pin!(idle_timer);

    loop {
        tokio::select! {
            // Prefer draining client traffic, so a burst of requests is not
            // starved by a chatty target.
            biased;

            payload = inbound.recv() => {
                let Some(payload) = payload else { break };
                idle_timer.as_mut().reset(Instant::now() + idle_step);
                match connect_udp::decode_payload(payload) {
                    Ok(connect_udp::Incoming::UdpPayload(payload)) => {
                        if let Some(bandwidth) = &limits.bandwidth {
                            bandwidth.consume(payload.len()).await;
                        }
                        if let Some(packets) = &limits.packet_rate {
                            packets.consume(1).await;
                        }
                        if let Err(error) = socket.send(&payload).await {
                            debug!(%error, "could not forward to the target");
                            break;
                        }
                        crate::metrics::bytes_relayed("to_target", payload.len() as u64);
                        if transfer_exceeded(&mut moved, payload.len(), limits.total_bytes) {
                            debug!("udp tunnel reached its transfer ceiling; closing");
                            break;
                        }
                    }
                    // RFC 9298, Section 5: an unregistered context is dropped,
                    // never treated as a failure of the tunnel.
                    Ok(connect_udp::Incoming::UnknownContext { context, .. }) => {
                        trace!(context = context.get(), "dropping datagram in an unknown context");
                    }
                    Err(error) => trace!(%error, "dropping malformed datagram"),
                }
            }

            result = socket.recv(&mut buf) => {
                match result {
                    Ok(len) => {
                        idle_timer.as_mut().reset(Instant::now() + idle_step);
                        if let Some(bandwidth) = &limits.bandwidth {
                            bandwidth.consume(len).await;
                        }
                        if let Some(packets) = &limits.packet_rate {
                            packets.consume(1).await;
                        }
                        if let Err(error) = route.send(&connect_udp::encode_payload(&buf[..len])) {
                            // RFC 9298, Section 3.1: oversized datagrams are
                            // dropped rather than fragmented.
                            trace!(%error, len, "dropping reply");
                        } else {
                            crate::metrics::bytes_relayed("to_client", len as u64);
                        }
                        if transfer_exceeded(&mut moved, len, limits.total_bytes) {
                            debug!("udp tunnel reached its transfer ceiling; closing");
                            break;
                        }
                    }
                    Err(error) => {
                        // The socket is unusable -- an ICMP unreachable, for
                        // instance -- and RFC 9298 requires closing the stream.
                        debug!(%error, "target socket failed");
                        break;
                    }
                }
            }

            _ = &mut reader => break,

            _ = &mut idle_timer, if idle.is_some() => {
                debug!(?idle, "udp tunnel idle; closing");
                break;
            }
        }
    }
    reader.abort();
}

/// Read the request stream, forwarding DATAGRAM capsules into `sink`.
///
/// Returning is how [`relay`] learns the client closed the tunnel.
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
                debug!(%error, "request stream ended");
                return;
            }
        }
    }
    pump.finish();
}

/// Pump raw bytes between the target TCP connection and the QUIC request
/// stream, in both directions, until both are closed or either errors.
///
/// The two halves close independently: a FIN from the client
/// (`recv_data` -> `Ok(None)`) is forwarded as a write shutdown to the target,
/// and EOF from the target is forwarded as `finish()` on the response stream.
///
/// `limits.bandwidth` paces the stream (its `sleep` flow-controls QUIC), and
/// `limits.total_bytes` caps how much one tunnel may carry. `packet_rate` does
/// not apply to a byte stream.
async fn relay_tcp(
    tcp: TcpStream,
    mut send: SendStream,
    mut recv: RecvStream,
    idle: Option<Duration>,
    limits: TunnelLimits,
) {
    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let mut buf = vec![0u8; TCP_RELAY_BUFFER];
    let mut client_done = false;
    let mut target_done = false;
    let mut moved: u64 = 0;

    // Bytes in either direction push this deadline back; if it fires, a
    // half-open or forgotten tunnel is reclaimed rather than held forever.
    let idle_step = idle.unwrap_or(Duration::from_secs(3600));
    let idle_timer = sleep(idle_step);
    tokio::pin!(idle_timer);

    while !(client_done && target_done) {
        tokio::select! {
            biased;

            chunk = recv.recv_data(), if !client_done => match chunk {
                Ok(Some(mut data)) => {
                    idle_timer.as_mut().reset(Instant::now() + idle_step);
                    let bytes = data.copy_to_bytes(data.remaining());
                    if let Some(bandwidth) = &limits.bandwidth {
                        bandwidth.consume(bytes.len()).await;
                    }
                    if let Err(error) = tcp_write.write_all(&bytes).await {
                        debug!(%error, "could not write to the target");
                        break;
                    }
                    crate::metrics::bytes_relayed("to_target", bytes.len() as u64);
                    if transfer_exceeded(&mut moved, bytes.len(), limits.total_bytes) {
                        debug!("tcp tunnel reached its transfer ceiling; closing");
                        break;
                    }
                }
                Ok(None) => {
                    client_done = true;
                    let _ = tcp_write.shutdown().await;
                }
                Err(error) => {
                    debug!(%error, "client stream ended");
                    break;
                }
            },

            result = tcp_read.read(&mut buf), if !target_done => match result {
                Ok(0) => {
                    target_done = true;
                    let _ = send.finish().await;
                }
                Ok(len) => {
                    idle_timer.as_mut().reset(Instant::now() + idle_step);
                    if let Some(bandwidth) = &limits.bandwidth {
                        bandwidth.consume(len).await;
                    }
                    if let Err(error) = send.send_data(Bytes::copy_from_slice(&buf[..len])).await {
                        debug!(%error, "could not send to the client");
                        break;
                    }
                    crate::metrics::bytes_relayed("to_client", len as u64);
                    if transfer_exceeded(&mut moved, len, limits.total_bytes) {
                        debug!("tcp tunnel reached its transfer ceiling; closing");
                        break;
                    }
                }
                Err(error) => {
                    debug!(%error, "target read failed");
                    break;
                }
            },

            _ = &mut idle_timer, if idle.is_some() => {
                debug!(?idle, "tcp tunnel idle; closing");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_resource_limits_are_bounded_and_unlimited_removes_them() {
        let default = ResourceLimits::default();
        assert!(default.max_connections.is_some());
        assert!(default.connection_rate.is_some());
        assert!(default.per_source_rate.is_some());
        assert!(default.exchange_rate.is_some());
        assert!(default.max_tunnels_per_connection.is_some());
        assert!(default.tunnel_idle_timeout.is_some());

        let none = ResourceLimits::unlimited();
        assert!(none.max_connections.is_none());
        assert!(none.connection_rate.is_none());
        assert!(none.per_source_rate.is_none());
        assert!(none.exchange_rate.is_none());
        assert!(none.max_tunnels_per_connection.is_none());
        assert!(none.tunnel_idle_timeout.is_none());
    }

    #[test]
    fn the_exchange_limiter_tracks_the_configured_rate_and_toggles_with_it() {
        let cfg = ProxyConfig::new("localhost").unwrap();
        assert!(cfg.exchange_limiter.is_some(), "on by default");

        let limiter = ExchangeRateLimiter::new(ConnectionRate {
            per_second: 0,
            burst: 1,
        });
        let ip: IpAddr = "198.51.100.4".parse().unwrap();
        assert!(limiter.check(ip));
        assert!(!limiter.check(ip), "second exchange from the source is over the rate");

        let off = ProxyConfig::new("localhost")
            .unwrap()
            .with_limits(ResourceLimits {
                exchange_rate: None,
                ..ResourceLimits::default()
            });
        assert!(off.exchange_limiter.is_none());
    }

    #[test]
    fn a_per_source_limiter_throttles_one_ip_without_touching_another() {
        let mut limiter = PerSourceRate::new(ConnectionRate {
            per_second: 0,
            burst: 2,
        });
        let noisy: IpAddr = "203.0.113.7".parse().unwrap();
        let quiet: IpAddr = "203.0.113.8".parse().unwrap();

        assert!(limiter.try_take(noisy));
        assert!(limiter.try_take(noisy));
        assert!(!limiter.try_take(noisy), "noisy source is over its burst");

        // A different source still has its full burst.
        assert!(limiter.try_take(quiet));
        assert!(limiter.try_take(quiet));
        assert!(!limiter.try_take(quiet));
    }

    #[test]
    fn a_per_source_limiter_stays_bounded_and_eviction_is_fail_open() {
        // rate 0 so no bucket ever refills to full on its own; every distinct
        // source that has spent a token is "live" and must be evicted by count.
        let mut limiter = PerSourceRate::new(ConnectionRate {
            per_second: 0,
            burst: 1,
        });
        for octet in 0..(MAX_SOURCES + 500) {
            let ip: IpAddr = format!("10.{}.{}.1", octet / 256, octet % 256).parse().unwrap();
            assert!(limiter.try_take(ip), "a fresh source gets its burst");
        }
        assert!(limiter.buckets.len() <= MAX_SOURCES, "the map is bounded");
    }

    #[test]
    fn the_token_bucket_allows_a_burst_then_throttles_to_the_rate() {
        // A burst of 3, refilling at 100/s.
        let mut bucket = TokenBucket::new(&ConnectionRate {
            per_second: 100,
            burst: 3,
        });
        // The full burst is available immediately.
        assert!(bucket.try_take());
        assert!(bucket.try_take());
        assert!(bucket.try_take());
        // The fourth, arriving in the same instant, is over the rate.
        assert!(!bucket.try_take());

        // After 50ms at 100/s, ~5 tokens have refilled (capped at the burst).
        std::thread::sleep(Duration::from_millis(50));
        assert!(bucket.try_take());
        assert!(bucket.try_take());
    }

    #[test]
    fn a_zero_rate_bucket_never_yields_a_token_beyond_its_burst() {
        let mut bucket = TokenBucket::new(&ConnectionRate {
            per_second: 0,
            burst: 1,
        });
        assert!(bucket.try_take());
        assert!(!bucket.try_take());
        std::thread::sleep(Duration::from_millis(20));
        assert!(!bucket.try_take(), "nothing refills at rate 0");
    }

    #[test]
    fn the_tunnel_limit_rejection_is_a_retryable_503() {
        let config = ProxyConfig::new("localhost").unwrap();
        // `refuse_tunnel` is async and needs a stream; check the rejection it
        // builds directly instead.
        let rejection = Rejection::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "this connection is already carrying its maximum number of tunnels",
        )
        .with_proxy_error("connection_limit_reached");
        let response = rejection_response(rejection, &config.proxy_name);
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(response
            .headers()
            .get("proxy-status")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("connection_limit_reached"));
    }

    #[test]
    fn proxy_names_are_reduced_to_valid_structured_tokens() {
        assert_eq!(structured_token("skimasque"), "skimasque");
        assert_eq!(structured_token("my proxy!"), "myproxy");
        assert_eq!(structured_token("123"), "proxy");
        assert_eq!(structured_token(""), "proxy");
        assert_eq!(structured_token("a.b-c_d"), "a.b-c_d");
    }

    /// A detail string is attacker-influenced -- it can contain a hostname the
    /// client chose -- so it must not be able to break out of the quoted string
    /// and inject header structure.
    #[test]
    fn detail_strings_cannot_escape_their_quotes() {
        assert_eq!(structured_string("plain"), "plain");
        assert_eq!(structured_string(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(structured_string(r"back\slash"), r"back\\slash");
        assert_eq!(structured_string("new\nline\r\0"), "newline");
        assert_eq!(structured_string(&"x".repeat(1000)).len(), 256);
    }

    #[test]
    fn a_successful_response_announces_the_capsule_protocol() {
        let response = success_response(HeaderMap::new());
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CAPSULE_PROTOCOL_HEADER).unwrap(),
            CAPSULE_PROTOCOL_TRUE
        );
    }

    #[test]
    fn a_rejection_renders_a_proxy_status_header() {
        let response = rejection_response(Rejection::dns_error("resolving x: nope"), "skimasque");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response.headers().get("proxy-status").unwrap(),
            "skimasque; error=dns_error; details=\"resolving x: nope\""
        );
    }

    /// A rejection with no proxy error type carries no `Proxy-Status`, rather
    /// than an empty one.
    #[test]
    fn a_plain_rejection_has_no_proxy_status() {
        let response = rejection_response(Rejection::bad_request("nope"), "skimasque");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(response.headers().get("proxy-status").is_none());
    }

    #[test]
    fn an_authentication_challenge_survives_into_the_response() {
        let response = rejection_response(Rejection::proxy_auth_required("Bearer"), "skimasque");
        assert_eq!(response.status(), StatusCode::PROXY_AUTHENTICATION_REQUIRED);
        assert_eq!(
            response
                .headers()
                .get(http::header::PROXY_AUTHENTICATE)
                .unwrap(),
            "Bearer"
        );
    }
}
