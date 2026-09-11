//! HTTP Datagrams: the RFC 9297 outer frame and the context-ID layer that both
//! CONNECT-UDP (RFC 9298) and CONNECT-IP (RFC 9484) put inside it.
//!
//! Three nested formats are involved, and it helps to keep them apart:
//!
//! ```text
//! QUIC DATAGRAM frame payload
//! +- HTTP/3 Datagram       Quarter Stream ID (i) | HTTP Datagram Payload (..)  [RFC 9297 S2.1]
//!    +- Proxying Payload   Context ID (i)        | Payload (..)                [RFC 9298 S5]
//! ```
//!
//! We frame the outer layer ourselves rather than delegating to `h3-datagram`,
//! because that crate's `Datagram::encode` writes the Quarter Stream ID into a
//! scratch buffer and then discards it, emitting zero bytes in its place. That
//! is invisible on stream 0 and silently misroutes every other stream, which
//! would break the moment a connection carries more than one tunnel.

use bytes::{BufMut, Bytes, BytesMut};

use crate::varint;

/// The largest legal Quarter Stream ID, `2^60 - 1` (RFC 9297, Section 2.1).
pub const MAX_QUARTER_STREAM_ID: u64 = (1 << 60) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("QUIC stream id {0} is not a client-initiated bidirectional stream")]
    NotARequestStream(u64),
    #[error("quarter stream id {0} exceeds the 2^60-1 maximum")]
    QuarterStreamIdTooLarge(u64),
    #[error("datagram payload is too short to parse a {field}")]
    Truncated { field: &'static str },
    #[error("context id {0} exceeds the 62-bit varint maximum")]
    ContextIdTooLarge(u64),
}

/// A QUIC stream id divided by four, identifying the request a datagram belongs to.
///
/// HTTP requests travel on client-initiated bidirectional streams, whose ids are
/// always divisible by four, so the wire format transmits the quotient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QuarterStreamId(u64);

impl QuarterStreamId {
    /// Derive the quarter stream id for a request stream.
    ///
    /// Fails if `stream_id` is not divisible by four, i.e. is not a
    /// client-initiated bidirectional stream and so cannot carry a request.
    pub fn from_stream_id(stream_id: u64) -> Result<Self, Error> {
        if !stream_id.is_multiple_of(4) {
            return Err(Error::NotARequestStream(stream_id));
        }
        Ok(Self(stream_id / 4))
    }

    /// Wrap an already-divided value, as read off the wire.
    pub fn from_quarter(quarter: u64) -> Result<Self, Error> {
        if quarter > MAX_QUARTER_STREAM_ID {
            return Err(Error::QuarterStreamIdTooLarge(quarter));
        }
        Ok(Self(quarter))
    }

    /// The value as it appears on the wire.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The QUIC stream id this refers to.
    pub const fn stream_id(self) -> u64 {
        self.0 * 4
    }
}

/// Prefix `payload` with its Quarter Stream ID, producing a QUIC DATAGRAM body.
pub fn encode_http3_datagram(stream: QuarterStreamId, payload: &[u8]) -> Bytes {
    let mut out = BytesMut::with_capacity(varint::encoded_len(stream.get()) + payload.len());
    varint::encode(stream.get(), &mut out).expect("quarter stream id is bounded by 2^60-1");
    out.put_slice(payload);
    out.freeze()
}

/// Split a QUIC DATAGRAM body into its Quarter Stream ID and HTTP Datagram Payload.
///
/// A failure here is an `H3_DATAGRAM_ERROR` connection error, not a stream error.
pub fn decode_http3_datagram(mut buf: Bytes) -> Result<(QuarterStreamId, Bytes), Error> {
    let quarter = varint::take(&mut buf).map_err(|_| Error::Truncated {
        field: "quarter stream id",
    })?;
    Ok((QuarterStreamId::from_quarter(quarter)?, buf))
}

/// A datagram context identifier (RFC 9298, Section 4).
///
/// Context 0 is reserved: it means "this payload is a bare UDP payload" under
/// CONNECT-UDP and "this payload is a full IP packet" under CONNECT-IP. Non-zero
/// even ids are allocated by the client and odd ids by the proxy, so neither
/// side has to coordinate with the other before allocating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContextId(u64);

impl ContextId {
    /// The reserved context carrying UDP payloads (RFC 9298) or IP packets (RFC 9484).
    pub const DEFAULT: ContextId = ContextId(0);

    pub fn new(value: u64) -> Result<Self, Error> {
        if value > varint::MAX {
            return Err(Error::ContextIdTooLarge(value));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    /// Whether this is the reserved context for raw UDP payloads / IP packets.
    pub const fn is_default(self) -> bool {
        self.0 == 0
    }

    /// Which peer is allowed to allocate this id. Context 0 is pre-allocated by
    /// the specification and belongs to neither.
    pub const fn allocator(self) -> Option<Allocator> {
        match self.0 {
            0 => None,
            n if n % 2 == 0 => Some(Allocator::Client),
            _ => Some(Allocator::Proxy),
        }
    }
}

/// The endpoint permitted to allocate a given context id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allocator {
    Client,
    Proxy,
}

/// Hands out context ids belonging to one endpoint's half of the id space.
///
/// The namespace is per-request, so each tunnel owns its own allocator. Ids are
/// never reused within a request (RFC 9298, Section 4), hence the monotonic
/// counter and the absence of any release operation.
#[derive(Debug)]
pub struct ContextAllocator {
    next: u64,
}

impl ContextAllocator {
    /// Start allocating in `allocator`'s half of the space. The client's first
    /// id is 2, because 0 is reserved; the proxy's is 1.
    pub const fn new(allocator: Allocator) -> Self {
        Self {
            next: match allocator {
                Allocator::Client => 2,
                Allocator::Proxy => 1,
            },
        }
    }

    /// Allocate the next id, or `None` once the 62-bit space is exhausted.
    pub fn allocate(&mut self) -> Option<ContextId> {
        if self.next > varint::MAX {
            return None;
        }
        let id = ContextId(self.next);
        self.next = self.next.saturating_add(2);
        Some(id)
    }
}

/// The HTTP Datagram Payload shared by CONNECT-UDP and CONNECT-IP: a context id
/// followed by opaque bytes whose meaning that context defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyingPayload {
    pub context: ContextId,
    pub payload: Bytes,
}

impl ProxyingPayload {
    /// A payload in the default context: a UDP payload, or a full IP packet.
    pub fn default_context(payload: Bytes) -> Self {
        Self {
            context: ContextId::DEFAULT,
            payload,
        }
    }

    pub fn encode(&self) -> Bytes {
        let mut out =
            BytesMut::with_capacity(varint::encoded_len(self.context.get()) + self.payload.len());
        self.encode_into(&mut out);
        out.freeze()
    }

    pub fn encode_into(&self, out: &mut BytesMut) {
        varint::encode(self.context.get(), out).expect("context id is a validated varint");
        out.put_slice(&self.payload);
    }

    pub fn decode(mut buf: Bytes) -> Result<Self, Error> {
        let context =
            varint::take(&mut buf).map_err(|_| Error::Truncated { field: "context id" })?;
        Ok(Self {
            context: ContextId::new(context)?,
            payload: buf,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_stream_id_round_trips_through_the_wire_form() {
        // Stream 0 is the degenerate case that hides encoding bugs, so check a
        // stream whose quarter id needs more than one byte too.
        for stream_id in [0u64, 4, 8, 4 * 16_384, 4 * MAX_QUARTER_STREAM_ID] {
            let quarter = QuarterStreamId::from_stream_id(stream_id).unwrap();
            let wire = encode_http3_datagram(quarter, b"hello");
            let (decoded, payload) = decode_http3_datagram(wire).unwrap();
            assert_eq!(decoded, quarter);
            assert_eq!(decoded.stream_id(), stream_id);
            assert_eq!(&payload[..], b"hello");
        }
    }

    /// Two tunnels on one connection must not collide. This is exactly what
    /// `h3-datagram` 0.0.2 gets wrong.
    #[test]
    fn distinct_streams_produce_distinct_wire_prefixes() {
        let a = encode_http3_datagram(QuarterStreamId::from_stream_id(0).unwrap(), b"x");
        let b = encode_http3_datagram(QuarterStreamId::from_stream_id(4).unwrap(), b"x");
        assert_ne!(a, b);
        assert_eq!(&a[..], &[0x00, b'x']);
        assert_eq!(&b[..], &[0x01, b'x']);
    }

    #[test]
    fn rejects_stream_ids_that_cannot_carry_a_request() {
        // 2 is client-initiated unidirectional; 1 and 3 are server-initiated.
        for stream_id in [1u64, 2, 3, 5, 6, 7] {
            assert!(QuarterStreamId::from_stream_id(stream_id).is_err());
        }
    }

    #[test]
    fn rejects_quarter_stream_ids_above_the_maximum() {
        assert!(QuarterStreamId::from_quarter(MAX_QUARTER_STREAM_ID).is_ok());
        assert!(QuarterStreamId::from_quarter(MAX_QUARTER_STREAM_ID + 1).is_err());
    }

    #[test]
    fn empty_datagram_is_truncated_not_empty_payload() {
        assert!(matches!(
            decode_http3_datagram(Bytes::new()),
            Err(Error::Truncated { .. })
        ));
    }

    /// RFC 9297 says the HTTP Datagram Payload may be empty, so a datagram that
    /// is nothing but a quarter stream id is well formed.
    #[test]
    fn empty_payload_is_legal() {
        let wire = encode_http3_datagram(QuarterStreamId::from_stream_id(0).unwrap(), b"");
        let (_, payload) = decode_http3_datagram(wire).unwrap();
        assert!(payload.is_empty());
    }

    #[test]
    fn proxying_payload_round_trips() {
        let payload = ProxyingPayload {
            context: ContextId::new(16_384).unwrap(),
            payload: Bytes::from_static(b"\x01\x02\x03"),
        };
        assert_eq!(ProxyingPayload::decode(payload.encode()).unwrap(), payload);
    }

    #[test]
    fn context_zero_belongs_to_neither_peer() {
        assert!(ContextId::DEFAULT.is_default());
        assert_eq!(ContextId::DEFAULT.allocator(), None);
    }

    #[test]
    fn allocators_stay_in_their_own_half_of_the_space() {
        let mut client = ContextAllocator::new(Allocator::Client);
        let mut proxy = ContextAllocator::new(Allocator::Proxy);
        for _ in 0..8 {
            let c = client.allocate().unwrap();
            let p = proxy.allocate().unwrap();
            assert_eq!(c.allocator(), Some(Allocator::Client));
            assert_eq!(p.allocator(), Some(Allocator::Proxy));
            assert!(!c.is_default());
            assert_ne!(c, p);
        }
    }
}
