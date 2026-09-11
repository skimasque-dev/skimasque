#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use skimasque_core::datagram;

// A QUIC DATAGRAM frame carrying an HTTP/3 Datagram: a quarter-stream-id varint
// followed by the payload. This is the frame the relay demultiplexes tunnels
// on, so a parser bug here is a cross-tunnel bug.
fuzz_target!(|data: &[u8]| {
    let buf = Bytes::copy_from_slice(data);
    if let Ok((quarter, payload)) = datagram::decode_http3_datagram(buf) {
        // Re-encoding must round-trip.
        let reencoded = datagram::encode_http3_datagram(quarter, &payload);
        let (again_quarter, again_payload) =
            datagram::decode_http3_datagram(reencoded).expect("re-encoded datagram decodes");
        assert_eq!(quarter, again_quarter);
        assert_eq!(payload, again_payload);
        // The quarter id maps back to a real stream id.
        let _ = again_quarter.stream_id();
    }
});
