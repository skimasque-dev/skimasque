//! Proxying IP in HTTP (RFC 9484).
//!
//! The wire formats are implemented in full here -- the three control capsules,
//! the datagram payload, and the URI Template variables -- so that the transport
//! layer can adopt CONNECT-IP without revisiting the encoding. What is not yet
//! built is the plumbing above it: a TUN device, address management, and a
//! forked `h3` that recognises `connect-ip` as a `:protocol` value (upstream
//! `h3` 0.0.8 hardcodes `webtransport` and `connect-udp` and rejects the rest).

use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use bytes::{BufMut, Bytes, BytesMut};

use crate::capsule::{Capsule, CapsuleType};
use crate::datagram::{ContextId, ProxyingPayload};
use crate::template::{self, UriTemplate};
use crate::varint;

/// The HTTP upgrade token for IP proxying.
pub const UPGRADE_TOKEN: &str = "connect-ip";

/// The Internet Protocol Number meaning "all protocols" in a route advertisement.
pub const IPPROTO_ANY: u8 = 0;

/// The template value meaning "any allowable value" for `target` or `ipproto`.
pub const WILDCARD: &str = "*";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("capsule is malformed: {0}")]
    Malformed(&'static str),
    #[error("IP version {0} is neither 4 nor 6")]
    BadIpVersion(u8),
    #[error("prefix length {prefix_len} exceeds the {bits}-bit address")]
    PrefixTooLong { prefix_len: u8, bits: u8 },
    #[error("address {addr} has bits set below its /{prefix_len} prefix")]
    PrefixHostBitsSet { addr: IpAddr, prefix_len: u8 },
    #[error("address range start {start} is greater than end {end}")]
    RangeReversed { start: IpAddr, end: IpAddr },
    #[error("address range mixes IPv4 and IPv6")]
    RangeFamilyMismatch,
    #[error("ADDRESS_REQUEST must carry at least one requested address")]
    EmptyAddressRequest,
    #[error("request id must not be zero in an ADDRESS_REQUEST")]
    ZeroRequestId,
    #[error("ROUTE_ADVERTISEMENT ranges are not in the required ascending order")]
    RoutesOutOfOrder,
    #[error("ipproto {0:?} is not a number in 0-255 or {WILDCARD:?}")]
    BadIpProtocol(String),
    #[error(transparent)]
    Template(#[from] template::Error),
}

/// An IP address together with a prefix length, with the host bits required to
/// be zero (RFC 9484, Sections 4.7.1 and 4.7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IpPrefix {
    addr: IpAddr,
    prefix_len: u8,
}

impl IpPrefix {
    pub fn new(addr: IpAddr, prefix_len: u8) -> Result<Self, Error> {
        let bits = address_bits(&addr);
        if prefix_len > bits {
            return Err(Error::PrefixTooLong { prefix_len, bits });
        }
        if masked(&addr, prefix_len) != addr {
            return Err(Error::PrefixHostBitsSet { addr, prefix_len });
        }
        Ok(Self { addr, prefix_len })
    }

    /// A single host address, i.e. a prefix covering the whole address.
    pub fn host(addr: IpAddr) -> Self {
        Self {
            prefix_len: address_bits(&addr),
            addr,
        }
    }

    /// The "no preference" / "not assigned" form: `0.0.0.0/32` or `::/128`.
    ///
    /// RFC 9484 gives this two meanings depending on direction. In an
    /// ADDRESS_REQUEST it asks for any address of that family; in an
    /// ADDRESS_ASSIGN it says the corresponding request was refused.
    pub fn unspecified(v6: bool) -> Self {
        if v6 {
            Self::host(IpAddr::V6(Ipv6Addr::UNSPECIFIED))
        } else {
            Self::host(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
        }
    }

    pub const fn addr(&self) -> IpAddr {
        self.addr
    }

    pub const fn prefix_len(&self) -> u8 {
        self.prefix_len
    }

    /// Whether this is an all-zero address at maximum prefix length.
    pub fn is_unspecified_host(&self) -> bool {
        self.prefix_len == address_bits(&self.addr) && self.addr.is_unspecified()
    }

    fn encode_into(&self, out: &mut BytesMut) {
        match self.addr {
            IpAddr::V4(v4) => {
                out.put_u8(4);
                out.put_slice(&v4.octets());
            }
            IpAddr::V6(v6) => {
                out.put_u8(6);
                out.put_slice(&v6.octets());
            }
        }
        out.put_u8(self.prefix_len);
    }

    fn decode(input: &mut &[u8]) -> Result<Self, Error> {
        let addr = decode_versioned_address(input)?;
        let prefix_len = take_u8(input)?;
        Self::new(addr, prefix_len)
    }
}

impl fmt::Display for IpPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix_len)
    }
}

/// One entry of an ADDRESS_ASSIGN capsule (RFC 9484, Figure 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssignedAddress {
    /// The `request_id` this answers, or zero if it was sent unprompted.
    pub request_id: u64,
    pub prefix: IpPrefix,
}

/// One entry of an ADDRESS_REQUEST capsule (RFC 9484, Figure 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestedAddress {
    /// A non-zero identifier, unique within this request stream.
    pub request_id: u64,
    pub prefix: IpPrefix,
}

/// One entry of a ROUTE_ADVERTISEMENT capsule (RFC 9484, Figure 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpAddressRange {
    pub start: IpAddr,
    pub end: IpAddr,
    /// Internet Protocol Number, or [`IPPROTO_ANY`] for all protocols.
    pub protocol: u8,
}

impl IpAddressRange {
    pub fn new(start: IpAddr, end: IpAddr, protocol: u8) -> Result<Self, Error> {
        match (start, end) {
            (IpAddr::V4(a), IpAddr::V4(b)) if a > b => {
                return Err(Error::RangeReversed { start, end });
            }
            (IpAddr::V6(a), IpAddr::V6(b)) if a > b => {
                return Err(Error::RangeReversed { start, end });
            }
            (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_)) => {}
            _ => return Err(Error::RangeFamilyMismatch),
        }
        Ok(Self {
            start,
            end,
            protocol,
        })
    }

    fn version(&self) -> u8 {
        if self.start.is_ipv4() {
            4
        } else {
            6
        }
    }
}

/// A control capsule specific to IP proxying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpCapsule {
    /// The full set of prefixes now assigned to the peer. An empty list
    /// withdraws every previous assignment.
    AddressAssign(Vec<AssignedAddress>),
    /// A request for address assignment. Never empty.
    AddressRequest(Vec<RequestedAddress>),
    /// The full set of ranges the sender will route, in ascending order.
    RouteAdvertisement(Vec<IpAddressRange>),
}

impl IpCapsule {
    pub fn capsule_type(&self) -> CapsuleType {
        match self {
            Self::AddressAssign(_) => CapsuleType::ADDRESS_ASSIGN,
            Self::AddressRequest(_) => CapsuleType::ADDRESS_REQUEST,
            Self::RouteAdvertisement(_) => CapsuleType::ROUTE_ADVERTISEMENT,
        }
    }

    /// Encode as a generic [`Capsule`], ready to be written to the data stream.
    pub fn to_capsule(&self) -> Result<Capsule, Error> {
        let mut value = BytesMut::new();
        match self {
            Self::AddressAssign(addresses) => {
                for address in addresses {
                    varint::encode(address.request_id, &mut value)
                        .map_err(|_| Error::Malformed("request id exceeds the varint maximum"))?;
                    address.prefix.encode_into(&mut value);
                }
            }
            Self::AddressRequest(addresses) => {
                if addresses.is_empty() {
                    return Err(Error::EmptyAddressRequest);
                }
                for address in addresses {
                    if address.request_id == 0 {
                        return Err(Error::ZeroRequestId);
                    }
                    varint::encode(address.request_id, &mut value)
                        .map_err(|_| Error::Malformed("request id exceeds the varint maximum"))?;
                    address.prefix.encode_into(&mut value);
                }
            }
            Self::RouteAdvertisement(ranges) => {
                check_route_ordering(ranges)?;
                for range in ranges {
                    value.put_u8(range.version());
                    put_address(&mut value, &range.start);
                    put_address(&mut value, &range.end);
                    value.put_u8(range.protocol);
                }
            }
        }
        Ok(Capsule::new(self.capsule_type(), value.freeze()))
    }

    /// Interpret a capsule read off the data stream.
    ///
    /// Returns `Ok(None)` for capsule types this module does not define, which
    /// RFC 9297 requires be dropped rather than treated as an error.
    pub fn from_capsule(capsule: &Capsule) -> Result<Option<Self>, Error> {
        let mut input = &capsule.value[..];
        let parsed = match capsule.kind {
            CapsuleType::ADDRESS_ASSIGN => {
                let mut addresses = Vec::new();
                while !input.is_empty() {
                    let request_id = take_varint(&mut input)?;
                    addresses.push(AssignedAddress {
                        request_id,
                        prefix: IpPrefix::decode(&mut input)?,
                    });
                }
                Self::AddressAssign(addresses)
            }
            CapsuleType::ADDRESS_REQUEST => {
                let mut addresses = Vec::new();
                while !input.is_empty() {
                    let request_id = take_varint(&mut input)?;
                    if request_id == 0 {
                        return Err(Error::ZeroRequestId);
                    }
                    addresses.push(RequestedAddress {
                        request_id,
                        prefix: IpPrefix::decode(&mut input)?,
                    });
                }
                if addresses.is_empty() {
                    return Err(Error::EmptyAddressRequest);
                }
                Self::AddressRequest(addresses)
            }
            CapsuleType::ROUTE_ADVERTISEMENT => {
                let mut ranges = Vec::new();
                while !input.is_empty() {
                    let version = take_u8(&mut input)?;
                    let start = decode_address_of_version(&mut input, version)?;
                    let end = decode_address_of_version(&mut input, version)?;
                    let protocol = take_u8(&mut input)?;
                    ranges.push(IpAddressRange::new(start, end, protocol)?);
                }
                check_route_ordering(&ranges)?;
                Self::RouteAdvertisement(ranges)
            }
            _ => return Ok(None),
        };
        // RFC 9297, Section 3.3: trailing bytes make the capsule malformed.
        if !input.is_empty() {
            return Err(Error::Malformed("trailing bytes after the capsule value"));
        }
        Ok(Some(parsed))
    }
}

/// RFC 9484, Section 4.7.3 orders ranges by version, then protocol, then
/// address, so a receiver can install them into a routing table without
/// checking every pair for overlap.
fn check_route_ordering(ranges: &[IpAddressRange]) -> Result<(), Error> {
    for pair in ranges.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let ordered = match a.version().cmp(&b.version()) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Greater => false,
            std::cmp::Ordering::Equal => match a.protocol.cmp(&b.protocol) {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Greater => false,
                std::cmp::Ordering::Equal => a.end < b.start,
            },
        };
        if !ordered {
            return Err(Error::RoutesOutOfOrder);
        }
    }
    Ok(())
}

/// The scope of an IP proxying request, from the `target` and `ipproto` URI
/// Template variables (RFC 9484, Section 4.6). Both are optional, and the
/// wildcard `*` means "anything the proxy allows".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IpScope {
    /// A hostname or IP prefix, or `None` for any allowable host.
    pub target: Option<String>,
    /// An Internet Protocol Number, or `None` for any allowable protocol.
    pub protocol: Option<u8>,
}

impl IpScope {
    /// A request for an unrestricted tunnel.
    pub fn any() -> Self {
        Self::default()
    }

    pub fn expand_path(&self, template: &UriTemplate) -> Result<String, Error> {
        let mut values = BTreeMap::new();
        if template.variables().iter().any(|v| v == "target") {
            values.insert(
                "target",
                self.target.clone().unwrap_or_else(|| WILDCARD.to_owned()),
            );
        }
        if template.variables().iter().any(|v| v == "ipproto") {
            values.insert(
                "ipproto",
                self.protocol
                    .map_or_else(|| WILDCARD.to_owned(), |p| p.to_string()),
            );
        }
        Ok(template.expand_path(&values)?)
    }

    pub fn from_path(template: &UriTemplate, path: &str) -> Result<Self, Error> {
        let values = template.match_path(path).ok_or(template::Error::MissingVariable(
            "target",
        ))?;
        let target = match values.get("target").map(String::as_str) {
            None | Some(WILDCARD) => None,
            Some(target) => Some(target.to_owned()),
        };
        let protocol = match values.get("ipproto").map(String::as_str) {
            None | Some(WILDCARD) => None,
            Some(raw) => Some(
                raw.parse::<u8>()
                    .map_err(|_| Error::BadIpProtocol(raw.to_owned()))?,
            ),
        };
        Ok(Self { target, protocol })
    }
}

/// Wrap a full IP packet as an HTTP Datagram Payload in the reserved context.
pub fn encode_packet(packet: &[u8]) -> Bytes {
    ProxyingPayload::default_context(Bytes::copy_from_slice(packet)).encode()
}

/// The result of decoding an HTTP Datagram Payload on a CONNECT-IP stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// A full IP packet, from context 0.
    Packet(Bytes),
    /// A payload in a context we never registered; drop or briefly buffer it.
    UnknownContext { context: ContextId, payload: Bytes },
}

/// Interpret an HTTP Datagram Payload received on a CONNECT-IP request stream.
pub fn decode_packet(payload: Bytes) -> Result<Incoming, crate::datagram::Error> {
    let payload = ProxyingPayload::decode(payload)?;
    Ok(if payload.context.is_default() {
        Incoming::Packet(payload.payload)
    } else {
        Incoming::UnknownContext {
            context: payload.context,
            payload: payload.payload,
        }
    })
}

fn address_bits(addr: &IpAddr) -> u8 {
    match addr {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

fn masked(addr: &IpAddr, prefix_len: u8) -> IpAddr {
    match addr {
        IpAddr::V4(v4) => {
            let bits = u32::from_be_bytes(v4.octets());
            let mask = if prefix_len == 0 {
                0
            } else {
                u32::MAX << (32 - prefix_len)
            };
            IpAddr::V4(Ipv4Addr::from((bits & mask).to_be_bytes()))
        }
        IpAddr::V6(v6) => {
            let bits = u128::from_be_bytes(v6.octets());
            let mask = if prefix_len == 0 {
                0
            } else {
                u128::MAX << (128 - prefix_len)
            };
            IpAddr::V6(Ipv6Addr::from((bits & mask).to_be_bytes()))
        }
    }
}

fn put_address(out: &mut BytesMut, addr: &IpAddr) {
    match addr {
        IpAddr::V4(v4) => out.put_slice(&v4.octets()),
        IpAddr::V6(v6) => out.put_slice(&v6.octets()),
    }
}

fn take_u8(input: &mut &[u8]) -> Result<u8, Error> {
    let (&first, rest) = input
        .split_first()
        .ok_or(Error::Malformed("capsule value ended early"))?;
    *input = rest;
    Ok(first)
}

fn take_varint(input: &mut &[u8]) -> Result<u64, Error> {
    varint::take_slice(input).map_err(|_| Error::Malformed("capsule value ended mid-varint"))
}

fn decode_versioned_address(input: &mut &[u8]) -> Result<IpAddr, Error> {
    let version = take_u8(input)?;
    decode_address_of_version(input, version)
}

fn decode_address_of_version(input: &mut &[u8], version: u8) -> Result<IpAddr, Error> {
    let len = match version {
        4 => 4,
        6 => 16,
        other => return Err(Error::BadIpVersion(other)),
    };
    let bytes = input
        .get(..len)
        .ok_or(Error::Malformed("capsule value ended mid-address"))?;
    *input = &input[len..];
    Ok(match version {
        4 => IpAddr::V4(Ipv4Addr::from(
            <[u8; 4]>::try_from(bytes).expect("length checked"),
        )),
        _ => IpAddr::V6(Ipv6Addr::from(
            <[u8; 16]>::try_from(bytes).expect("length checked"),
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn v6(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn address_assign_round_trips() {
        let capsule = IpCapsule::AddressAssign(vec![
            AssignedAddress {
                request_id: 0,
                prefix: IpPrefix::new(v4("192.0.2.0"), 24).unwrap(),
            },
            AssignedAddress {
                request_id: 7,
                prefix: IpPrefix::new(v6("2001:db8::"), 64).unwrap(),
            },
        ]);
        let encoded = capsule.to_capsule().unwrap();
        assert_eq!(encoded.kind, CapsuleType::ADDRESS_ASSIGN);
        assert_eq!(IpCapsule::from_capsule(&encoded).unwrap(), Some(capsule));
    }

    /// An empty ADDRESS_ASSIGN is meaningful: it withdraws every assignment.
    #[test]
    fn empty_address_assign_is_legal() {
        let capsule = IpCapsule::AddressAssign(vec![]);
        let encoded = capsule.to_capsule().unwrap();
        assert!(encoded.value.is_empty());
        assert_eq!(IpCapsule::from_capsule(&encoded).unwrap(), Some(capsule));
    }

    /// An empty ADDRESS_REQUEST is not: RFC 9484 requires aborting the stream.
    #[test]
    fn empty_address_request_is_rejected_in_both_directions() {
        assert_eq!(
            IpCapsule::AddressRequest(vec![]).to_capsule(),
            Err(Error::EmptyAddressRequest)
        );
        let empty = Capsule::new(CapsuleType::ADDRESS_REQUEST, Bytes::new());
        assert_eq!(
            IpCapsule::from_capsule(&empty),
            Err(Error::EmptyAddressRequest)
        );
    }

    #[test]
    fn zero_request_id_is_rejected_in_a_request() {
        let capsule = IpCapsule::AddressRequest(vec![RequestedAddress {
            request_id: 0,
            prefix: IpPrefix::unspecified(false),
        }]);
        assert_eq!(capsule.to_capsule(), Err(Error::ZeroRequestId));
    }

    #[test]
    fn route_advertisement_round_trips() {
        let capsule = IpCapsule::RouteAdvertisement(vec![
            IpAddressRange::new(v4("10.0.0.0"), v4("10.0.0.255"), IPPROTO_ANY).unwrap(),
            IpAddressRange::new(v4("192.0.2.0"), v4("192.0.2.255"), IPPROTO_ANY).unwrap(),
            IpAddressRange::new(v6("2001:db8::"), v6("2001:db8::ffff"), 17).unwrap(),
        ]);
        let encoded = capsule.to_capsule().unwrap();
        assert_eq!(IpCapsule::from_capsule(&encoded).unwrap(), Some(capsule));
    }

    #[test]
    fn out_of_order_routes_are_rejected() {
        let unordered = vec![
            IpAddressRange::new(v4("192.0.2.0"), v4("192.0.2.255"), 0).unwrap(),
            IpAddressRange::new(v4("10.0.0.0"), v4("10.0.0.255"), 0).unwrap(),
        ];
        assert_eq!(
            IpCapsule::RouteAdvertisement(unordered).to_capsule(),
            Err(Error::RoutesOutOfOrder)
        );
    }

    /// Adjacent ranges must not touch: the end of one has to be strictly below
    /// the start of the next.
    #[test]
    fn touching_routes_are_rejected() {
        let touching = vec![
            IpAddressRange::new(v4("10.0.0.0"), v4("10.0.0.5"), 0).unwrap(),
            IpAddressRange::new(v4("10.0.0.5"), v4("10.0.0.9"), 0).unwrap(),
        ];
        assert_eq!(
            IpCapsule::RouteAdvertisement(touching).to_capsule(),
            Err(Error::RoutesOutOfOrder)
        );
    }

    #[test]
    fn prefixes_must_have_their_host_bits_cleared() {
        assert!(IpPrefix::new(v4("192.0.2.0"), 24).is_ok());
        assert_eq!(
            IpPrefix::new(v4("192.0.2.1"), 24),
            Err(Error::PrefixHostBitsSet {
                addr: v4("192.0.2.1"),
                prefix_len: 24
            })
        );
        assert_eq!(
            IpPrefix::new(v4("192.0.2.1"), 33),
            Err(Error::PrefixTooLong {
                prefix_len: 33,
                bits: 32
            })
        );
        // A /0 covers everything, so only the all-zero address is valid.
        assert!(IpPrefix::new(v4("0.0.0.0"), 0).is_ok());
        assert!(IpPrefix::new(v4("10.0.0.0"), 0).is_err());
    }

    #[test]
    fn unspecified_prefix_means_no_preference() {
        let v4_any = IpPrefix::unspecified(false);
        assert_eq!(v4_any.to_string(), "0.0.0.0/32");
        assert!(v4_any.is_unspecified_host());
        assert_eq!(IpPrefix::unspecified(true).to_string(), "::/128");
    }

    #[test]
    fn reversed_and_mixed_family_ranges_are_rejected() {
        assert!(matches!(
            IpAddressRange::new(v4("10.0.0.9"), v4("10.0.0.1"), 0),
            Err(Error::RangeReversed { .. })
        ));
        assert_eq!(
            IpAddressRange::new(v4("10.0.0.1"), v6("2001:db8::"), 0),
            Err(Error::RangeFamilyMismatch)
        );
    }

    #[test]
    fn bad_ip_version_is_rejected() {
        // Request id 1, version 5, then filler.
        let capsule = Capsule::new(
            CapsuleType::ADDRESS_ASSIGN,
            Bytes::from_static(&[0x01, 0x05, 0, 0, 0, 0, 32]),
        );
        assert_eq!(
            IpCapsule::from_capsule(&capsule),
            Err(Error::BadIpVersion(5))
        );
    }

    #[test]
    fn trailing_bytes_make_a_capsule_malformed() {
        let mut value = BytesMut::from(
            &IpCapsule::AddressAssign(vec![AssignedAddress {
                request_id: 1,
                prefix: IpPrefix::host(v4("192.0.2.1")),
            }])
            .to_capsule()
            .unwrap()
            .value[..],
        );
        value.put_u8(0xff);
        let capsule = Capsule::new(CapsuleType::ADDRESS_ASSIGN, value.freeze());
        assert!(matches!(
            IpCapsule::from_capsule(&capsule),
            Err(Error::Malformed(_))
        ));
    }

    #[test]
    fn capsules_from_other_protocols_are_ignored_not_rejected() {
        let datagram = Capsule::datagram(Bytes::from_static(b"payload"));
        assert_eq!(IpCapsule::from_capsule(&datagram), Ok(None));
    }

    #[test]
    fn scope_round_trips_through_a_path() {
        let template = UriTemplate::default_connect_ip("proxy.example").unwrap();
        for scope in [
            IpScope::any(),
            IpScope {
                target: Some("192.0.2.0/24".to_owned()),
                protocol: Some(17),
            },
            IpScope {
                target: Some("2001:db8::1".to_owned()),
                protocol: None,
            },
        ] {
            let path = scope.expand_path(&template).unwrap();
            assert_eq!(IpScope::from_path(&template, &path).unwrap(), scope);
        }
    }

    #[test]
    fn ip_packets_use_the_reserved_context() {
        let packet = [0x45u8, 0x00, 0x00, 0x14];
        let wire = encode_packet(&packet);
        assert_eq!(&wire[..], &[0x00, 0x45, 0x00, 0x00, 0x14]);
        assert_eq!(
            decode_packet(wire).unwrap(),
            Incoming::Packet(Bytes::copy_from_slice(&packet))
        );
    }
}
