#![no_main]

use libfuzzer_sys::fuzz_target;
use skimasque_core::varint;

// QUIC variable-length integers (RFC 9000 §16). Every other wire parser in the
// crate starts by pulling one of these off the front of attacker-controlled
// bytes.
fuzz_target!(|data: &[u8]| {
    if let Ok((value, len)) = varint::decode_slice(data) {
        // A successful decode must report a length within the input and one of
        // the four legal encodings.
        assert!(len <= data.len());
        assert!(matches!(len, 1 | 2 | 4 | 8));
        // And re-decoding the consumed prefix must yield the same value.
        let (again, again_len) = varint::decode_slice(&data[..len]).expect("prefix re-decodes");
        assert_eq!((value, len), (again, again_len));
    }

    // The take_* form advances a cursor; it must never advance past the end.
    let mut cursor = data;
    while let Ok(_value) = varint::take_slice(&mut cursor) {
        assert!(cursor.len() <= data.len());
    }
});
