#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use skimasque_core::connect_ip::IpCapsule;
use skimasque_core::{connect_ip, CapsuleDecoder};

// Two CONNECT-IP (RFC 9484) parse paths on client-controlled bytes:
//   - the datagram payload carrying a forwarded IP packet, and
//   - the ADDRESS_ASSIGN / ADDRESS_REQUEST / ROUTE_ADVERTISEMENT capsules that
//     carry IP prefixes and ranges, which have their own varint + address
//     decoding.
fuzz_target!(|data: &[u8]| {
    let _ = connect_ip::decode_packet(Bytes::copy_from_slice(data));

    let mut decoder = CapsuleDecoder::new();
    decoder.push(data);
    while let Ok(Some(capsule)) = decoder.next_capsule() {
        let _ = IpCapsule::from_capsule(&capsule);
    }
});
