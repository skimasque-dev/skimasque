//! Proxying IP in HTTP (RFC 9484) over HTTP/3.
//!
//! Available with the `connect-ip` cargo feature.
//!
//! An IP tunnel differs from a UDP one in two ways that shape this module.
//! Packets are whole IP datagrams rather than UDP payloads, and the request
//! stream carries real conversation: the endpoints assign each other addresses
//! and advertise routes with capsules, reliably and in order, alongside the
//! unreliable packet flow. [`IpTunnel`] therefore surfaces both as a single
//! stream of [`IpEvent`]s, and is used by *both* ends -- a proxy's side of a
//! tunnel is the same object as a client's, pointed the other way.
//!
//! # What this module does not do
//!
//! It does not forward packets to a real network. Doing that needs a TUN device
//! or a userspace stack, which needs privileges and platform-specific code, and
//! which deployments disagree about. Instead [`IpProxy`] handles the protocol
//! and address management and hands each accepted tunnel to the application as
//! an [`IpSession`], whose [`IpLink`] is a plain pair of packet queues. Connect
//! that to a TUN device, to a userspace stack, or in tests to an echo.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use bytes::Bytes;
use http::StatusCode;
use skimasque_core::capsule::Capsule;
use skimasque_core::connect_ip;
use skimasque_core::Protocol;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tower::Service;
use tracing::{debug, trace, warn};

use crate::capsules::CapsuleWriter;
use crate::dgram::{DatagramRoute, SendError};
use crate::service::{Accepted, Rejection, TunnelFuture, TunnelRequest};
use crate::Error;

pub use skimasque_core::connect_ip::{
    AssignedAddress, IpAddressRange, IpCapsule, IpPrefix, IpScope, RequestedAddress, IPPROTO_ANY,
    UPGRADE_TOKEN, WILDCARD,
};

/// Queue depth for packets waiting to cross between a tunnel and its network.
const DEFAULT_LINK_CAPACITY: usize = 1024;

/// Something that arrived on an IP tunnel.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IpEvent {
    /// A full IP packet, from the reserved context.
    Packet(Bytes),
    /// The peer told us the complete set of prefixes it has assigned us.
    /// Each ADDRESS_ASSIGN supersedes the last, so an empty list withdraws
    /// everything.
    AddressesAssigned(Vec<AssignedAddress>),
    /// The peer asked us to assign it addresses. RFC 9484, Section 4.7.2
    /// requires an ADDRESS_ASSIGN in reply, with matching request ids.
    AddressesRequested(Vec<RequestedAddress>),
    /// The peer told us the complete set of ranges it will route for us.
    RoutesAdvertised(Vec<IpAddressRange>),
}

/// One end of an IP tunnel. Both the client and the proxy hold one.
pub struct IpTunnel {
    scope: IpScope,
    route: DatagramRoute,
    datagrams: mpsc::Receiver<Bytes>,
    capsules: mpsc::Receiver<Capsule>,
    writer: Box<dyn CapsuleWriter>,
    /// Ends when the peer closes the request stream. `None` once observed.
    reader: Option<JoinHandle<()>>,
    assigned: Vec<AssignedAddress>,
    peer_routes: Vec<IpAddressRange>,
    next_request_id: u64,
}

impl std::fmt::Debug for IpTunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpTunnel")
            .field("scope", &self.scope)
            .field("stream_id", &self.route.stream_id())
            .field("assigned", &self.assigned)
            .finish_non_exhaustive()
    }
}

impl IpTunnel {
    pub(crate) fn new(
        scope: IpScope,
        route: DatagramRoute,
        datagrams: mpsc::Receiver<Bytes>,
        capsules: mpsc::Receiver<Capsule>,
        writer: Box<dyn CapsuleWriter>,
        reader: JoinHandle<()>,
    ) -> Self {
        Self {
            scope,
            route,
            datagrams,
            capsules,
            writer,
            reader: Some(reader),
            assigned: Vec::new(),
            peer_routes: Vec::new(),
            // RFC 9484, Section 4.7.2: request ids must not be zero, and must
            // not be reused by an endpoint.
            next_request_id: 1,
        }
    }

    /// The scope this tunnel was opened with.
    pub fn scope(&self) -> &IpScope {
        &self.scope
    }

    /// The QUIC stream id of the underlying request.
    pub fn stream_id(&self) -> u64 {
        self.route.stream_id()
    }

    /// The largest IP packet that currently fits in one datagram.
    ///
    /// Packets larger than this are dropped rather than fragmented; see
    /// RFC 9484, Section 10.1 on choosing an MTU for the tunnel interface.
    pub fn max_packet_size(&self) -> Option<usize> {
        // One byte of context id, since context 0 encodes as a single zero byte.
        self.route.max_payload_size()?.checked_sub(1)
    }

    /// The prefixes the peer has assigned us, from the most recent
    /// ADDRESS_ASSIGN it sent.
    pub fn assigned_addresses(&self) -> &[AssignedAddress] {
        &self.assigned
    }

    /// The ranges the peer has said it will route, from the most recent
    /// ROUTE_ADVERTISEMENT it sent.
    pub fn peer_routes(&self) -> &[IpAddressRange] {
        &self.peer_routes
    }

    /// Send a full IP packet. Unreliable, like the tunnel itself.
    pub fn send_packet(&self, packet: &[u8]) -> Result<(), SendError> {
        self.route.send(&connect_ip::encode_packet(packet))
    }

    /// Send a control capsule on the request stream.
    pub async fn send_capsule(&mut self, capsule: &IpCapsule) -> Result<(), Error> {
        let encoded = capsule
            .to_capsule()
            .map_err(|error| Error::Invalid(format!("building capsule: {error}")))?;
        self.writer.write_capsule(encoded.encode()).await
    }

    /// Ask the peer to assign us these prefixes.
    ///
    /// Returns the request id allocated for each, in the order given, so the
    /// caller can match them against the ADDRESS_ASSIGN that follows. Use
    /// [`IpPrefix::unspecified`] to ask for any address of a family.
    pub async fn request_addresses(&mut self, prefixes: &[IpPrefix]) -> Result<Vec<u64>, Error> {
        let mut ids = Vec::with_capacity(prefixes.len());
        let requests = prefixes
            .iter()
            .map(|prefix| {
                let request_id = self.next_request_id;
                self.next_request_id += 1;
                ids.push(request_id);
                RequestedAddress {
                    request_id,
                    prefix: *prefix,
                }
            })
            .collect();
        self.send_capsule(&IpCapsule::AddressRequest(requests))
            .await?;
        Ok(ids)
    }

    /// Tell the peer the complete set of prefixes it may use as a source.
    pub async fn assign_addresses(&mut self, addresses: Vec<AssignedAddress>) -> Result<(), Error> {
        self.send_capsule(&IpCapsule::AddressAssign(addresses)).await
    }

    /// Tell the peer the complete set of ranges we will route for it.
    pub async fn advertise_routes(&mut self, routes: Vec<IpAddressRange>) -> Result<(), Error> {
        self.send_capsule(&IpCapsule::RouteAdvertisement(routes))
            .await
    }

    /// Receive the next event, or `None` once the tunnel is closed.
    pub async fn recv(&mut self) -> Option<IpEvent> {
        loop {
            let reader = self.reader.as_mut()?;

            let received = tokio::select! {
                // Control capsules first: an address assignment arriving with a
                // burst of packets should be acted on before them.
                biased;
                capsule = self.capsules.recv() => Received::Capsule(capsule?),
                payload = self.datagrams.recv() => Received::Datagram(payload?),
                _ = reader => {
                    self.reader = None;
                    return None;
                }
            };

            match received {
                Received::Datagram(payload) => match connect_ip::decode_packet(payload) {
                    Ok(connect_ip::Incoming::Packet(packet)) => {
                        return Some(IpEvent::Packet(packet))
                    }
                    Ok(connect_ip::Incoming::UnknownContext { context, .. }) => {
                        trace!(context = context.get(), "dropping datagram in an unknown context");
                    }
                    Err(error) => trace!(%error, "dropping malformed datagram"),
                },
                Received::Capsule(capsule) => match IpCapsule::from_capsule(&capsule) {
                    // Each capsule of these types carries the full current set
                    // and supersedes the last, so replace rather than merge.
                    Ok(Some(IpCapsule::AddressAssign(addresses))) => {
                        self.assigned = addresses.clone();
                        return Some(IpEvent::AddressesAssigned(addresses));
                    }
                    Ok(Some(IpCapsule::RouteAdvertisement(routes))) => {
                        self.peer_routes = routes.clone();
                        return Some(IpEvent::RoutesAdvertised(routes));
                    }
                    Ok(Some(IpCapsule::AddressRequest(requests))) => {
                        return Some(IpEvent::AddressesRequested(requests));
                    }
                    Ok(None) => trace!(kind = %capsule.kind, "ignoring capsule"),
                    Err(error) => {
                        // RFC 9484 makes a malformed capsule grounds for
                        // aborting the request stream.
                        warn!(%error, "malformed IP capsule; closing the tunnel");
                        return None;
                    }
                },
            }
        }
    }

    /// Close the tunnel.
    pub async fn close(mut self) -> Result<(), Error> {
        self.writer.finish_stream().await
    }
}

enum Received {
    Datagram(Bytes),
    Capsule(Capsule),
}

/// Answer an ADDRESS_REQUEST from what we have already assigned.
///
/// RFC 9484, Section 4.7.2 requires one Assigned Address per Requested Address,
/// carrying the same request id, and requires refusals to be spelled as an
/// all-zero address at maximum prefix length.
pub fn answer_address_request(
    requests: &[RequestedAddress],
    assigned: &[IpPrefix],
) -> Vec<AssignedAddress> {
    requests
        .iter()
        .map(|request| {
            let wanted_v6 = request.prefix.addr().is_ipv6();
            let matching = assigned
                .iter()
                .find(|prefix| prefix.addr().is_ipv6() == wanted_v6);
            AssignedAddress {
                request_id: request.request_id,
                prefix: matching
                    .copied()
                    .unwrap_or_else(|| IpPrefix::unspecified(wanted_v6)),
            }
        })
        .collect()
}

/// The proxy's end of an IP tunnel: what it assigns, what it routes, and the
/// queues carrying packets to and from the proxy's network.
#[derive(Debug)]
pub struct IpEndpoint {
    assigned: Vec<AssignedAddress>,
    routes: Vec<IpAddressRange>,
    /// Packets from the client, headed for the network.
    to_network: mpsc::Sender<Bytes>,
    /// Packets from the network, headed for the client.
    from_network: mpsc::Receiver<Bytes>,
}

impl IpEndpoint {
    /// Build an endpoint and the [`IpLink`] the application drives it with.
    pub fn channel(
        assigned: Vec<AssignedAddress>,
        routes: Vec<IpAddressRange>,
    ) -> (Self, IpLink) {
        Self::channel_with_capacity(assigned, routes, DEFAULT_LINK_CAPACITY)
    }

    pub fn channel_with_capacity(
        assigned: Vec<AssignedAddress>,
        routes: Vec<IpAddressRange>,
        capacity: usize,
    ) -> (Self, IpLink) {
        let (to_network, inbound) = mpsc::channel(capacity);
        let (outbound, from_network) = mpsc::channel(capacity);
        (
            Self {
                assigned,
                routes,
                to_network,
                from_network,
            },
            IpLink { inbound, outbound },
        )
    }

    /// The prefixes this endpoint will assign the client.
    pub fn assigned(&self) -> &[AssignedAddress] {
        &self.assigned
    }

    /// The ranges this endpoint will advertise.
    pub fn routes(&self) -> &[IpAddressRange] {
        &self.routes
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<AssignedAddress>,
        Vec<IpAddressRange>,
        mpsc::Sender<Bytes>,
        mpsc::Receiver<Bytes>,
    ) {
        (self.assigned, self.routes, self.to_network, self.from_network)
    }
}

/// The application's end of an IP tunnel: raw packets in both directions.
#[derive(Debug)]
pub struct IpLink {
    /// Packets the client sent.
    inbound: mpsc::Receiver<Bytes>,
    /// Packets to deliver to the client.
    outbound: mpsc::Sender<Bytes>,
}

impl IpLink {
    /// Deliver a packet to the client.
    ///
    /// Does not block, and drops the packet if the tunnel is backed up: the
    /// tunnel is unreliable by contract, and blocking here would apply
    /// backpressure to an entire network interface.
    pub fn send(&self, packet: Bytes) -> Result<(), LinkError> {
        use mpsc::error::TrySendError;
        match self.outbound.try_send(packet) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(LinkError::Congested),
            Err(TrySendError::Closed(_)) => Err(LinkError::Closed),
        }
    }

    /// The next packet the client sent, or `None` once the tunnel closes.
    pub async fn recv(&mut self) -> Option<Bytes> {
        self.inbound.recv().await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    #[error("the tunnel is congested; packet dropped")]
    Congested,
    #[error("the tunnel is closed")]
    Closed,
}

/// Hands out one address per tunnel from a configured prefix.
///
/// The first usable address in the prefix is reserved for the proxy itself, as
/// a VPN deployment would expect; the rest are leased. A lease is released when
/// its [`IpLease`] is dropped, which happens when the tunnel that holds it ends.
#[derive(Debug, Clone)]
pub struct IpAddressPool {
    inner: Arc<Mutex<PoolState>>,
}

#[derive(Debug)]
struct PoolState {
    prefix: IpPrefix,
    /// Inclusive bounds of the leasable range, as integers.
    first_lease: u128,
    last_lease: u128,
    /// Cursor, so successive leases do not immediately reuse a released
    /// address. Wraps around the range.
    cursor: u128,
    leased: HashSet<u128>,
}

impl IpAddressPool {
    /// Build a pool covering `prefix`.
    pub fn new(prefix: IpPrefix) -> Self {
        let (first_usable, last_usable) = usable_bounds(&prefix);
        // The proxy takes the first usable address; leases start after it.
        let first_lease = first_usable.saturating_add(1);
        Self {
            inner: Arc::new(Mutex::new(PoolState {
                prefix,
                first_lease,
                last_lease: last_usable,
                cursor: first_lease,
                leased: HashSet::new(),
            })),
        }
    }

    /// Parse `10.0.0.0/24` or `2001:db8::/64` into a pool.
    pub fn parse(input: &str) -> Result<Self, Error> {
        let (addr, len) = input
            .split_once('/')
            .ok_or_else(|| Error::Invalid(format!("{input:?} is not an address/prefix")))?;
        let addr: IpAddr = addr
            .parse()
            .map_err(|_| Error::Invalid(format!("{addr:?} is not an IP address")))?;
        let len: u8 = len
            .parse()
            .map_err(|_| Error::Invalid(format!("{len:?} is not a prefix length")))?;
        let prefix = IpPrefix::new(addr, len)
            .map_err(|error| Error::Invalid(format!("{input:?}: {error}")))?;
        Ok(Self::new(prefix))
    }

    /// The prefix this pool covers.
    pub fn prefix(&self) -> IpPrefix {
        self.inner.lock().expect("pool is not poisoned").prefix
    }

    /// The address reserved for the proxy itself.
    pub fn proxy_address(&self) -> IpAddr {
        let state = self.inner.lock().expect("pool is not poisoned");
        // `first_lease` is one past the proxy's own address, except in a pool
        // too small to lease anything at all.
        let proxy = usable_bounds(&state.prefix).0;
        from_integer(proxy, state.prefix.addr().is_ipv6())
    }

    /// How many addresses this pool can lease at once.
    pub fn capacity(&self) -> u128 {
        let state = self.inner.lock().expect("pool is not poisoned");
        state
            .last_lease
            .checked_sub(state.first_lease)
            .map_or(0, |span| span + 1)
    }

    /// How many are leased right now.
    pub fn leased(&self) -> usize {
        self.inner.lock().expect("pool is not poisoned").leased.len()
    }

    /// Lease one address, or `None` if the pool is exhausted.
    pub fn lease(&self) -> Option<IpLease> {
        let mut state = self.inner.lock().expect("pool is not poisoned");
        if state.first_lease > state.last_lease {
            return None;
        }
        let span = state.last_lease - state.first_lease + 1;

        let mut candidate = state.cursor;
        for _ in 0..span {
            if candidate > state.last_lease {
                candidate = state.first_lease;
            }
            if !state.leased.contains(&candidate) {
                state.leased.insert(candidate);
                state.cursor = candidate.saturating_add(1);
                let v6 = state.prefix.addr().is_ipv6();
                let address = from_integer(candidate, v6);
                drop(state);
                return Some(IpLease {
                    pool: self.clone(),
                    key: candidate,
                    prefix: IpPrefix::host(address),
                });
            }
            candidate = candidate.saturating_add(1);
        }
        None
    }

    fn release(&self, key: u128) {
        self.inner
            .lock()
            .expect("pool is not poisoned")
            .leased
            .remove(&key);
    }
}

/// A leased address, returned to its pool when dropped.
#[derive(Debug)]
pub struct IpLease {
    pool: IpAddressPool,
    key: u128,
    prefix: IpPrefix,
}

impl IpLease {
    /// The leased address as a host prefix, ready for an ADDRESS_ASSIGN.
    pub fn prefix(&self) -> IpPrefix {
        self.prefix
    }

    pub fn address(&self) -> IpAddr {
        self.prefix.addr()
    }
}

impl Drop for IpLease {
    fn drop(&mut self) {
        self.pool.release(self.key);
    }
}

/// The inclusive bounds of the addresses in `prefix` that may be handed out.
///
/// IPv4 prefixes shorter than /31 exclude the network and broadcast addresses,
/// which is what an operator writing `10.0.0.0/24` expects. IPv6 excludes only
/// the subnet-router anycast address at the base. /31 and /32 (and /127, /128)
/// are point-to-point sizes where those conventions do not apply.
fn usable_bounds(prefix: &IpPrefix) -> (u128, u128) {
    let v6 = prefix.addr().is_ipv6();
    let bits = if v6 { 128 } else { 32 };
    let base = to_integer(prefix.addr());
    let host_bits = u32::from(bits - prefix.prefix_len());
    let size = 1u128.checked_shl(host_bits).unwrap_or(0);
    let last = base + size - 1;

    match (v6, host_bits) {
        // A single address, or a two-address point-to-point link.
        (_, 0) | (_, 1) => (base, last),
        (false, _) => (base + 1, last - 1),
        (true, _) => (base + 1, last),
    }
}

fn to_integer(addr: IpAddr) -> u128 {
    match addr {
        IpAddr::V4(v4) => u128::from(u32::from_be_bytes(v4.octets())),
        IpAddr::V6(v6) => u128::from_be_bytes(v6.octets()),
    }
}

fn from_integer(value: u128, v6: bool) -> IpAddr {
    if v6 {
        IpAddr::V6(Ipv6Addr::from(value.to_be_bytes()))
    } else {
        IpAddr::V4(Ipv4Addr::from((value as u32).to_be_bytes()))
    }
}

/// A CONNECT-IP proxy that assigns addresses from a pool and advertises routes.
///
/// It does not forward packets. Each accepted tunnel is published as an
/// [`IpSession`] on the receiver returned by [`new`](Self::new); the
/// application takes the session's [`IpLink`] and connects it to whatever
/// network it wants to expose.
#[derive(Debug, Clone)]
pub struct IpProxy {
    pool: IpAddressPool,
    routes: Arc<Vec<IpAddressRange>>,
    sessions: mpsc::Sender<IpSession>,
}

/// An accepted IP tunnel, handed to the application to connect to a network.
#[derive(Debug)]
pub struct IpSession {
    /// The client's address, as seen by the proxy.
    pub client: SocketAddr,
    /// The scope the client asked for. Enforcing it, beyond what the advertised
    /// routes already say, is the application's decision.
    pub scope: IpScope,
    /// The address assigned to this client.
    pub assigned: IpPrefix,
    /// Packets in and out.
    pub link: IpLink,
    /// Held so the address stays leased for as long as the session does.
    lease: IpLease,
}

impl IpSession {
    /// The leased address. Dropping the session returns it to the pool.
    pub fn lease(&self) -> &IpLease {
        &self.lease
    }
}

impl IpProxy {
    /// Build a proxy leasing from `pool` and advertising `routes`.
    ///
    /// Returns the service and the stream of accepted sessions. Dropping the
    /// receiver makes the proxy refuse new tunnels, since nothing would be
    /// carrying their packets.
    pub fn new(
        pool: IpAddressPool,
        routes: Vec<IpAddressRange>,
    ) -> (Self, mpsc::Receiver<IpSession>) {
        let (sessions, incoming) = mpsc::channel(64);
        (
            Self {
                pool,
                routes: Arc::new(routes),
                sessions,
            },
            incoming,
        )
    }

    /// The pool this proxy leases from.
    pub fn pool(&self) -> &IpAddressPool {
        &self.pool
    }
}

impl Service<TunnelRequest> for IpProxy {
    type Response = Accepted;
    type Error = Rejection;
    type Future = TunnelFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: TunnelRequest) -> Self::Future {
        let pool = self.pool.clone();
        let routes = self.routes.clone();
        let sessions = self.sessions.clone();

        Box::pin(async move {
            if request.protocol() != Protocol::ConnectIp {
                return Err(Rejection::new(
                    StatusCode::NOT_IMPLEMENTED,
                    "IpProxy only serves connect-ip",
                ));
            }
            let scope = request
                .destination()
                .as_ip()
                .cloned()
                .ok_or_else(|| Rejection::bad_request("request carried no IP scope"))?;

            let lease = pool.lease().ok_or_else(|| {
                Rejection::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "the proxy's address pool is exhausted",
                )
                .with_proxy_error("proxy_internal_error")
            })?;

            let assigned = vec![AssignedAddress {
                // Unprompted assignments carry request id zero (RFC 9484,
                // Section 4.7.1); a reply to an ADDRESS_REQUEST will echo the
                // client's id instead.
                request_id: 0,
                prefix: lease.prefix(),
            }];
            let (endpoint, link) =
                IpEndpoint::channel(assigned.clone(), routes.as_ref().clone());

            let session = IpSession {
                client: request.client_addr(),
                scope,
                assigned: lease.prefix(),
                link,
                lease,
            };
            sessions.send(session).await.map_err(|_| {
                Rejection::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "the proxy has no network attached",
                )
                .with_proxy_error("proxy_internal_error")
            })?;

            debug!(assigned = %assigned[0].prefix, "accepted an IP tunnel");
            Ok(Accepted::ip(endpoint))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix(s: &str) -> IpPrefix {
        let (addr, len) = s.split_once('/').unwrap();
        IpPrefix::new(addr.parse().unwrap(), len.parse().unwrap()).unwrap()
    }

    #[test]
    fn a_pool_reserves_the_first_address_and_skips_network_and_broadcast() {
        let pool = IpAddressPool::parse("192.0.2.0/24").unwrap();
        assert_eq!(pool.proxy_address().to_string(), "192.0.2.1");
        // 254 usable, less the proxy's own.
        assert_eq!(pool.capacity(), 253);

        let first = pool.lease().unwrap();
        assert_eq!(first.address().to_string(), "192.0.2.2");
    }

    #[test]
    fn ipv6_pools_skip_only_the_subnet_router_anycast_address() {
        let pool = IpAddressPool::parse("2001:db8::/126").unwrap();
        // Base is the anycast address, base+1 is the proxy, leaving two.
        assert_eq!(pool.proxy_address().to_string(), "2001:db8::1");
        assert_eq!(pool.capacity(), 2);
        assert_eq!(pool.lease().unwrap().address().to_string(), "2001:db8::2");
    }

    #[test]
    fn point_to_point_prefixes_use_every_address() {
        let p31 = IpAddressPool::parse("192.0.2.0/31").unwrap();
        assert_eq!(p31.proxy_address().to_string(), "192.0.2.0");
        assert_eq!(p31.capacity(), 1);
        assert_eq!(p31.lease().unwrap().address().to_string(), "192.0.2.1");

        // A /32 is the proxy's own address and nothing else.
        let p32 = IpAddressPool::parse("192.0.2.7/32").unwrap();
        assert_eq!(p32.proxy_address().to_string(), "192.0.2.7");
        assert_eq!(p32.capacity(), 0);
        assert!(p32.lease().is_none());
    }

    #[test]
    fn leases_are_unique_and_returned_when_dropped() {
        let pool = IpAddressPool::parse("192.0.2.0/29").unwrap();
        // /29 is 8 addresses, less network, broadcast and the proxy: 5.
        assert_eq!(pool.capacity(), 5);

        let leases: Vec<_> = (0..5).map(|_| pool.lease().unwrap()).collect();
        let addresses: HashSet<_> = leases.iter().map(|l| l.address()).collect();
        assert_eq!(addresses.len(), 5, "a pool handed out a duplicate");
        assert_eq!(pool.leased(), 5);
        assert!(pool.lease().is_none(), "exhausted pool still leased");

        drop(leases);
        assert_eq!(pool.leased(), 0);
        assert!(pool.lease().is_some(), "released addresses were not reusable");
    }

    /// Releasing an address should not make the next lease reuse it
    /// immediately: recycling an address a client may still be sending from
    /// invites confusion.
    #[test]
    fn the_pool_cycles_rather_than_reusing_the_last_release() {
        let pool = IpAddressPool::parse("192.0.2.0/24").unwrap();
        let first = pool.lease().unwrap();
        let first_address = first.address();
        drop(first);
        let second = pool.lease().unwrap();
        assert_ne!(second.address(), first_address);
    }

    #[test]
    fn malformed_pool_specifications_are_rejected() {
        assert!(IpAddressPool::parse("192.0.2.0").is_err());
        assert!(IpAddressPool::parse("192.0.2.0/33").is_err());
        // Host bits set below the prefix.
        assert!(IpAddressPool::parse("192.0.2.1/24").is_err());
        assert!(IpAddressPool::parse("nonsense/24").is_err());
    }

    #[test]
    fn an_address_request_is_answered_with_matching_request_ids() {
        let requests = vec![
            RequestedAddress {
                request_id: 7,
                prefix: IpPrefix::unspecified(false),
            },
            RequestedAddress {
                request_id: 9,
                prefix: IpPrefix::unspecified(true),
            },
        ];
        let assigned = [prefix("192.0.2.5/32")];
        let answer = answer_address_request(&requests, &assigned);

        assert_eq!(answer.len(), 2);
        assert_eq!(answer[0].request_id, 7);
        assert_eq!(answer[0].prefix, prefix("192.0.2.5/32"));

        // We have no IPv6 address, so RFC 9484 wants an all-zero refusal at the
        // maximum prefix length rather than a silent omission.
        assert_eq!(answer[1].request_id, 9);
        assert!(answer[1].prefix.is_unspecified_host());
        assert_eq!(answer[1].prefix.to_string(), "::/128");
    }

    #[tokio::test]
    async fn a_link_carries_packets_and_reports_congestion() {
        let (_endpoint, mut link) = IpEndpoint::channel_with_capacity(vec![], vec![], 1);
        assert_eq!(link.send(Bytes::from_static(b"one")), Ok(()));
        assert_eq!(
            link.send(Bytes::from_static(b"two")),
            Err(LinkError::Congested),
            "a full link must drop rather than block"
        );

        // Nothing has been sent from the client's side yet.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), link.recv())
                .await
                .is_err()
        );
    }
}
