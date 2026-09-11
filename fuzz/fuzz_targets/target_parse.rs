#![no_main]

use libfuzzer_sys::fuzz_target;
use skimasque_core::target::{Target, TargetHost};

// `host:port` as it arrives in the authority form of a classic CONNECT request,
// and the bare host as it arrives already percent-decoded from a template
// variable. Both are on the request path before any socket is opened.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = Target::parse(s);
        let _ = TargetHost::parse(s);
    }
});
