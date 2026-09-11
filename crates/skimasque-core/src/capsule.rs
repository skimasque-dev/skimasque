//! The Capsule Protocol (RFC 9297, Section 3).
//!
//! A capsule is a type-length-value tuple, and the data stream of a CONNECT-UDP
//! or CONNECT-IP request is nothing but a sequence of them. Capsules carry
//! reliable, ordered control messages alongside the unreliable datagram path,
//! and they carry the datagrams themselves when the transport has no QUIC
//! DATAGRAM frame to put them in.
//!
//! [`CapsuleDecoder`] deliberately surfaces capsules of every type, including
//! unknown ones. RFC 9297 requires endpoints to silently drop unknown capsule
//! types, but that is a decision for the protocol layer above: an intermediary
//! forwards them unmodified, so the decoder must not eat them.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::varint;

/// Default cap on a single capsule value, applied by [`CapsuleDecoder::new`].
///
/// RFC 9297, Section 3.2 warns against buffering an entire capsule value before
/// acting on it, because a value larger than the flow control window deadlocks
/// the stream. Every capsule this crate understands is small -- a datagram or a
/// handful of addresses -- so we buffer, but refuse to buffer without bound.
pub const DEFAULT_MAX_VALUE_LEN: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("capsule type {kind} declares a {len}-byte value, over the {max}-byte limit")]
    ValueTooLarge {
        kind: CapsuleType,
        len: u64,
        max: usize,
    },
    /// The peer closed the stream cleanly part-way through a capsule. RFC 9297,
    /// Section 3.3 requires this be treated as a malformed message.
    #[error("stream ended with an incomplete capsule ({buffered} bytes buffered)")]
    IncompleteAtEndOfStream { buffered: usize },
    #[error("capsule value is malformed: {0}")]
    Malformed(&'static str),
}

/// A capsule type code. Unknown codes are represented faithfully so they can be
/// forwarded or ignored rather than rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapsuleType(pub u64);

impl CapsuleType {
    /// Carries an HTTP Datagram on the stream (RFC 9297, Section 3.5).
    pub const DATAGRAM: Self = Self(0x00);
    /// Assigns IP addresses or prefixes to the peer (RFC 9484, Section 4.7.1).
    pub const ADDRESS_ASSIGN: Self = Self(0x01);
    /// Requests an address assignment from the peer (RFC 9484, Section 4.7.2).
    pub const ADDRESS_REQUEST: Self = Self(0x02);
    /// Advertises routable address ranges (RFC 9484, Section 4.7.3).
    pub const ROUTE_ADVERTISEMENT: Self = Self(0x03);

    /// The registered name of this type, or `None` if we do not know it.
    pub const fn name(self) -> Option<&'static str> {
        match self.0 {
            0x00 => Some("DATAGRAM"),
            0x01 => Some("ADDRESS_ASSIGN"),
            0x02 => Some("ADDRESS_REQUEST"),
            0x03 => Some("ROUTE_ADVERTISEMENT"),
            _ => None,
        }
    }
}

impl std::fmt::Display for CapsuleType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.name() {
            Some(name) => write!(f, "{name}"),
            None => write!(f, "0x{:x}", self.0),
        }
    }
}

/// One capsule, with its value still opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capsule {
    pub kind: CapsuleType,
    pub value: Bytes,
}

impl Capsule {
    pub fn new(kind: CapsuleType, value: impl Into<Bytes>) -> Self {
        Self {
            kind,
            value: value.into(),
        }
    }

    /// Wrap an HTTP Datagram Payload in a DATAGRAM capsule.
    pub fn datagram(payload: impl Into<Bytes>) -> Self {
        Self::new(CapsuleType::DATAGRAM, payload)
    }

    pub fn encode(&self) -> Bytes {
        let mut out = BytesMut::with_capacity(encoded_len(self.kind, self.value.len()));
        self.encode_into(&mut out);
        out.freeze()
    }

    pub fn encode_into(&self, out: &mut BytesMut) {
        write_header(self.kind, self.value.len(), out);
        out.put_slice(&self.value);
    }
}

/// Bytes a capsule of this type and value length occupies on the wire.
pub fn encoded_len(kind: CapsuleType, value_len: usize) -> usize {
    varint::encoded_len(kind.0) + varint::encoded_len(value_len as u64) + value_len
}

/// Write just the type and length fields, for callers streaming a large value.
pub fn write_header(kind: CapsuleType, value_len: usize, out: &mut BytesMut) {
    varint::encode(kind.0, out).expect("capsule type is a varint by construction");
    varint::encode(value_len as u64, out).expect("a usize length always fits in a varint");
}

/// Reassembles capsules from the arbitrary chunks a stream delivers them in.
///
/// Feed it with [`push`](Self::push) and drain it with
/// [`next_capsule`](Self::next_capsule) until that yields `None`; call
/// [`finish`](Self::finish) when the peer closes its send side, to catch a
/// stream that ended mid-capsule.
#[derive(Debug)]
pub struct CapsuleDecoder {
    buf: BytesMut,
    max_value_len: usize,
}

impl Default for CapsuleDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl CapsuleDecoder {
    pub fn new() -> Self {
        Self::with_max_value_len(DEFAULT_MAX_VALUE_LEN)
    }

    pub fn with_max_value_len(max_value_len: usize) -> Self {
        Self {
            buf: BytesMut::new(),
            max_value_len,
        }
    }

    /// Append freshly received stream bytes.
    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.put_slice(chunk);
    }

    /// Bytes held back awaiting the rest of a capsule.
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// Pop the next complete capsule, or `None` if more bytes are needed.
    pub fn next_capsule(&mut self) -> Result<Option<Capsule>, Error> {
        let head = &self.buf[..];
        let Ok((kind, type_len)) = varint::decode_slice(head) else {
            return Ok(None);
        };
        let Ok((value_len, len_len)) = varint::decode_slice(&head[type_len..]) else {
            return Ok(None);
        };
        let kind = CapsuleType(kind);

        // Check the declared length before waiting for it, so an absurd length
        // fails fast instead of buffering until the connection dies.
        if value_len > self.max_value_len as u64 {
            return Err(Error::ValueTooLarge {
                kind,
                len: value_len,
                max: self.max_value_len,
            });
        }
        let header_len = type_len + len_len;
        let total = header_len + value_len as usize;
        if self.buf.len() < total {
            return Ok(None);
        }

        self.buf.advance(header_len);
        let value = self.buf.split_to(value_len as usize).freeze();
        Ok(Some(Capsule { kind, value }))
    }

    /// Assert the stream ended on a capsule boundary.
    pub fn finish(&self) -> Result<(), Error> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(Error::IncompleteAtEndOfStream {
                buffered: self.buf.len(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(decoder: &mut CapsuleDecoder) -> Vec<Capsule> {
        let mut out = Vec::new();
        while let Some(capsule) = decoder.next_capsule().unwrap() {
            out.push(capsule);
        }
        out
    }

    #[test]
    fn datagram_capsule_matches_the_rfc9297_layout() {
        let wire = Capsule::datagram(Bytes::from_static(b"abc")).encode();
        // Type 0x00, length 3, then the value.
        assert_eq!(&wire[..], &[0x00, 0x03, b'a', b'b', b'c']);
    }

    #[test]
    fn round_trips_a_sequence_of_capsules() {
        let capsules = vec![
            Capsule::datagram(Bytes::from_static(b"one")),
            Capsule::new(CapsuleType::ADDRESS_REQUEST, Bytes::from_static(b"two")),
            Capsule::new(CapsuleType(0x1234), Bytes::new()),
        ];
        let mut wire = BytesMut::new();
        for capsule in &capsules {
            capsule.encode_into(&mut wire);
        }

        let mut decoder = CapsuleDecoder::new();
        decoder.push(&wire);
        assert_eq!(drain(&mut decoder), capsules);
        decoder.finish().unwrap();
    }

    /// Streams hand over arbitrary chunk boundaries, including boundaries that
    /// split a varint. Feeding one byte at a time is the harshest version.
    #[test]
    fn reassembles_across_byte_at_a_time_delivery() {
        let expected = Capsule::new(CapsuleType(0x1234), Bytes::from(vec![0x5a; 300]));
        let wire = expected.encode();

        let mut decoder = CapsuleDecoder::new();
        for (i, byte) in wire.iter().enumerate() {
            decoder.push(&[*byte]);
            let popped = decoder.next_capsule().unwrap();
            if i + 1 == wire.len() {
                assert_eq!(popped, Some(expected.clone()));
            } else {
                assert_eq!(popped, None, "yielded a capsule after {} bytes", i + 1);
            }
        }
        decoder.finish().unwrap();
    }

    #[test]
    fn zero_length_value_is_a_complete_capsule() {
        let mut decoder = CapsuleDecoder::new();
        decoder.push(&[0x00, 0x00]);
        assert_eq!(
            drain(&mut decoder),
            vec![Capsule::datagram(Bytes::new())]
        );
    }

    #[test]
    fn refuses_to_buffer_an_oversized_value() {
        let mut decoder = CapsuleDecoder::with_max_value_len(16);
        let mut wire = BytesMut::new();
        write_header(CapsuleType::DATAGRAM, 1_000_000, &mut wire);
        decoder.push(&wire);
        // The value has not arrived, but the declared length is already fatal.
        assert!(matches!(
            decoder.next_capsule(),
            Err(Error::ValueTooLarge { max: 16, .. })
        ));
    }

    #[test]
    fn a_stream_ending_mid_capsule_is_malformed() {
        let wire = Capsule::datagram(Bytes::from_static(b"truncated")).encode();
        let mut decoder = CapsuleDecoder::new();
        decoder.push(&wire[..wire.len() - 1]);
        assert_eq!(decoder.next_capsule().unwrap(), None);
        assert!(matches!(
            decoder.finish(),
            Err(Error::IncompleteAtEndOfStream { .. })
        ));
    }

    #[test]
    fn unknown_types_are_surfaced_rather_than_swallowed() {
        let mut decoder = CapsuleDecoder::new();
        decoder.push(&Capsule::new(CapsuleType(0x3fff_ffff), Bytes::from_static(b"?")).encode());
        let capsule = decoder.next_capsule().unwrap().unwrap();
        assert_eq!(capsule.kind.name(), None);
        assert_eq!(capsule.kind.to_string(), "0x3fffffff");
    }
}
