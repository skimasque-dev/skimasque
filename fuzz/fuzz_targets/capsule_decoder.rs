#![no_main]

use libfuzzer_sys::fuzz_target;
use skimasque_core::CapsuleDecoder;

// The Capsule Protocol (RFC 9297) framing on a CONNECT data stream: a stream of
// (type, length, value) records. The decoder is fed bytes as they arrive, so
// the fuzzer splits the input into arbitrary chunks and feeds them one at a
// time, the way a hostile peer would drip-feed a stream.
fuzz_target!(|data: &[u8]| {
    let mut decoder = CapsuleDecoder::new();

    // First byte, if any, picks a chunk size in 1..=16.
    let (chunk, rest) = match data.split_first() {
        Some((n, rest)) => ((*n % 16) as usize + 1, rest),
        None => return,
    };

    for piece in rest.chunks(chunk) {
        decoder.push(piece);
        loop {
            match decoder.next_capsule() {
                Ok(Some(capsule)) => {
                    // A yielded capsule re-encodes to the same bytes.
                    let bytes = capsule.encode();
                    let mut check = CapsuleDecoder::new();
                    check.push(&bytes);
                    let round = check.next_capsule().expect("re-encoded capsule decodes");
                    assert_eq!(round.as_ref().map(|c| c.encode()), Some(bytes));
                }
                Ok(None) => break,
                Err(_) => return,
            }
        }
    }
});
