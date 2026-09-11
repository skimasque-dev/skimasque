#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use skimasque_core::connect_udp;

// A datagram payload arriving on a CONNECT-UDP request stream: decoded, then
// either forwarded to the target socket (default context) or dropped.
fuzz_target!(|data: &[u8]| {
    let _ = connect_udp::decode_payload(Bytes::copy_from_slice(data));
});
