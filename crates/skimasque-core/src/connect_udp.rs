//! Proxying UDP in HTTP (RFC 9298).
//!
//! This module owns the parts of CONNECT-UDP that are pure data: the upgrade
//! token, the target address and its URI Template encoding, and the datagram
//! payload format. Opening sockets and moving bytes lives in the `skimasque`
//! crate; keeping them apart is what makes the wire format testable without a
//! network.

use bytes::Bytes;

use crate::datagram::{ContextId, ProxyingPayload};

/// The target address of a CONNECT-UDP request. Shared with CONNECT-TCP: both
/// specs use the same `target_host` / `target_port` encoding, so the type lives
/// in [`crate::target`] and is re-exported here for callers that think in terms
/// of RFC 9298.
pub use crate::target::{Error, Target, TargetHost};

/// The HTTP upgrade token, used as `:protocol` on HTTP/2 and HTTP/3 and in the
/// `Upgrade` header on HTTP/1.1.
pub const UPGRADE_TOKEN: &str = "connect-udp";

/// Wrap a UDP payload as an HTTP Datagram Payload in the reserved context.
pub fn encode_payload(udp_payload: &[u8]) -> Bytes {
    ProxyingPayload::default_context(Bytes::copy_from_slice(udp_payload)).encode()
}

/// The result of decoding an HTTP Datagram Payload on a CONNECT-UDP stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// A UDP payload to forward, from context 0.
    UdpPayload(Bytes),
    /// A payload in a context we never registered. RFC 9298, Section 5 says to
    /// drop or briefly buffer these, never to fail the request.
    UnknownContext { context: ContextId, payload: Bytes },
}

/// Interpret an HTTP Datagram Payload received on a CONNECT-UDP request stream.
pub fn decode_payload(payload: Bytes) -> Result<Incoming, crate::datagram::Error> {
    let payload = ProxyingPayload::decode(payload)?;
    Ok(if payload.context.is_default() {
        Incoming::UdpPayload(payload.payload)
    } else {
        Incoming::UnknownContext {
            context: payload.context,
            payload: payload.payload,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_payloads_use_the_reserved_context() {
        let wire = encode_payload(b"query");
        assert_eq!(&wire[..], &[0x00, b'q', b'u', b'e', b'r', b'y']);
        assert_eq!(
            decode_payload(wire).unwrap(),
            Incoming::UdpPayload(Bytes::from_static(b"query"))
        );
    }

    #[test]
    fn unregistered_contexts_are_reported_not_rejected() {
        let payload = ProxyingPayload {
            context: ContextId::new(3).unwrap(),
            payload: Bytes::from_static(b"extension"),
        };
        assert!(matches!(
            decode_payload(payload.encode()).unwrap(),
            Incoming::UnknownContext { .. }
        ));
    }

    /// An empty UDP payload is a legal datagram, and must not be confused with
    /// a truncated one.
    #[test]
    fn empty_udp_payload_is_distinct_from_a_truncated_datagram() {
        assert_eq!(
            decode_payload(encode_payload(b"")).unwrap(),
            Incoming::UdpPayload(Bytes::new())
        );
        assert!(decode_payload(Bytes::new()).is_err());
    }
}
