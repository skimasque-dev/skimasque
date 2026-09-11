//! QUIC variable-length integers, as used by every MASQUE wire format.
//!
//! See [RFC 9000, Section 16](https://www.rfc-editor.org/rfc/rfc9000#section-16).
//!
//! The primitive here is [`decode_slice`], which never consumes input it could
//! not fully parse. Capsules arrive in stream fragments, so a decoder that
//! swallowed a partial varint would corrupt the stream on the next poll.

use bytes::{Buf, BufMut, Bytes};

/// The largest value a QUIC varint can carry: `2^62 - 1`.
pub const MAX: u64 = (1 << 62) - 1;

/// The longest encoding a QUIC varint can occupy.
pub const MAX_ENCODED_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("value {0} exceeds the 62-bit varint maximum")]
    Overflow(u64),
    /// The input ran out mid-varint. For streaming decoders this is not fatal:
    /// it means "wait for more bytes", not "the peer is broken".
    #[error("input ended in the middle of a varint")]
    UnexpectedEnd,
}

/// Number of bytes `value` occupies on the wire.
///
/// # Panics
/// Panics in debug builds if `value` exceeds [`MAX`].
pub const fn encoded_len(value: u64) -> usize {
    debug_assert!(value <= MAX);
    if value < (1 << 6) {
        1
    } else if value < (1 << 14) {
        2
    } else if value < (1 << 30) {
        4
    } else {
        8
    }
}

/// Append `value` to `out` in QUIC varint form.
pub fn encode(value: u64, out: &mut impl BufMut) -> Result<(), Error> {
    if value > MAX {
        return Err(Error::Overflow(value));
    }
    match encoded_len(value) {
        1 => out.put_u8(value as u8),
        2 => out.put_u16(value as u16 | (0b01 << 14)),
        4 => out.put_u32(value as u32 | (0b10 << 30)),
        _ => out.put_u64(value | (0b11 << 62)),
    }
    Ok(())
}

/// Decode the varint at the head of `input`, returning it with its length.
///
/// Returns [`Error::UnexpectedEnd`] without consuming anything if `input` holds
/// only part of a varint.
pub fn decode_slice(input: &[u8]) -> Result<(u64, usize), Error> {
    let Some(&first) = input.first() else {
        return Err(Error::UnexpectedEnd);
    };
    let len = 1usize << (first >> 6);
    if input.len() < len {
        return Err(Error::UnexpectedEnd);
    }
    let mut value = u64::from(first & 0x3f);
    for &byte in &input[1..len] {
        value = (value << 8) | u64::from(byte);
    }
    Ok((value, len))
}

/// Decode a varint from the front of `input`, advancing it only on success.
pub fn take_slice(input: &mut &[u8]) -> Result<u64, Error> {
    let (value, len) = decode_slice(input)?;
    *input = &input[len..];
    Ok(value)
}

/// Decode a varint from the front of `buf`, advancing it only on success.
pub fn take(buf: &mut Bytes) -> Result<u64, Error> {
    let (value, len) = decode_slice(buf.chunk())?;
    buf.advance(len);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    /// The four worked examples from RFC 9000, Appendix A.1.
    #[test]
    fn rfc9000_appendix_a1_vectors() {
        let vectors: &[(&[u8], u64)] = &[
            (&[0xc2, 0x19, 0x7c, 0x5e, 0xff, 0x14, 0xe8, 0x8c], 151_288_809_941_952_652),
            (&[0x9d, 0x7f, 0x3e, 0x7d], 494_878_333),
            (&[0x7b, 0xbd], 15_293),
            (&[0x25], 37),
        ];
        for (wire, value) in vectors {
            assert_eq!(decode_slice(wire).unwrap(), (*value, wire.len()));
            let mut out = BytesMut::new();
            encode(*value, &mut out).unwrap();
            assert_eq!(&out[..], *wire, "re-encoding {value}");
        }
    }

    /// RFC 9000 permits non-minimal encodings on the wire (`0x40 0x25` is a
    /// legal two-byte spelling of 37), so decoding must accept them even though
    /// we always emit the shortest form.
    #[test]
    fn accepts_non_minimal_encodings() {
        assert_eq!(decode_slice(&[0x40, 0x25]).unwrap(), (37, 2));
        assert_eq!(decode_slice(&[0x80, 0, 0, 37]).unwrap(), (37, 4));
    }

    #[test]
    fn boundary_lengths() {
        for (value, len) in [(0, 1), (63, 1), (64, 2), (16_383, 2), (16_384, 4), (MAX, 8)] {
            let mut out = BytesMut::new();
            encode(value, &mut out).unwrap();
            assert_eq!(out.len(), len, "encoding {value}");
            assert_eq!(decode_slice(&out).unwrap(), (value, len));
        }
    }

    #[test]
    fn rejects_values_above_the_62_bit_maximum() {
        let mut out = BytesMut::new();
        assert_eq!(encode(MAX + 1, &mut out), Err(Error::Overflow(MAX + 1)));
        assert!(out.is_empty());
    }

    /// A truncated varint must leave the cursor untouched so the caller can
    /// retry once more of the stream has arrived.
    #[test]
    fn truncated_input_does_not_consume() {
        assert_eq!(decode_slice(&[]), Err(Error::UnexpectedEnd));
        for prefix_len in 1..8 {
            let wire = &[0xc2, 0x19, 0x7c, 0x5e, 0xff, 0x14, 0xe8, 0x8c][..prefix_len];
            assert_eq!(decode_slice(wire), Err(Error::UnexpectedEnd));
            let mut cursor = wire;
            assert!(take_slice(&mut cursor).is_err());
            assert_eq!(cursor.len(), prefix_len, "cursor advanced on failure");
        }
    }
}
