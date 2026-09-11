#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use skimasque_core::ProxyingPayload;

// The HTTP Datagram Payload of RFC 9298 / RFC 9484: a context-id varint plus the
// inner payload. Decoded once per datagram on every UDP and IP tunnel.
fuzz_target!(|data: &[u8]| {
    if let Ok(payload) = ProxyingPayload::decode(Bytes::copy_from_slice(data)) {
        let reencoded = payload.encode();
        let again = ProxyingPayload::decode(reencoded).expect("re-encoded payload decodes");
        assert_eq!(payload, again);
    }
});
