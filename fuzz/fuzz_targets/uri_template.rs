#![no_main]

use libfuzzer_sys::fuzz_target;
use skimasque_core::UriTemplate;

// The URI Template (RFC 6570, level-limited) the gateway matches request paths
// against and the client expands. Operator-supplied via `--template`, but also
// the shape a malformed one takes decides whether a path matches.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(template) = UriTemplate::parse(s) {
            // A parsed template's own string re-parses.
            let _ = UriTemplate::parse(template.as_str()).expect("a template's as_str re-parses");
        }
    }
});
