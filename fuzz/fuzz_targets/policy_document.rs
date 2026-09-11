#![no_main]

use libfuzzer_sys::fuzz_target;
use skimasque_policy::{parse_bitrate, parse_bytes, parse_duration, parse_rate, Policy};

// The policy document parser runs on operator-controlled files (`--policy-dir`),
// but a crash there takes the gateway's config path down, and hot-reload feeds
// it whatever lands on disk. The `toml` / `serde_yaml` layers are fuzzed
// upstream; what is exercised here is our validation and unit parsing on top.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = Policy::from_toml(s);
        let _ = Policy::from_yaml(s);

        // The hand-written unit parsers for the `[limits]` / `[session]` fields.
        let _ = parse_duration(s);
        let _ = parse_bytes(s);
        let _ = parse_bitrate(s);
        let _ = parse_rate(s);
    }
});
