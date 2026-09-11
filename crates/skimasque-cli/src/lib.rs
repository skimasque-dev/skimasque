//! Shared pieces of the `skimasque-server` and `skimasque-client` binaries.

pub mod account;
pub mod audit_ship;
pub mod control;
pub mod ops;
pub mod policy;
pub mod probe;
pub mod socks5;

use tracing_subscriber::EnvFilter;

/// Normalise a control-plane or gateway address: a bare host (`control.example`,
/// `gw.example:8443`) gains an `https://` scheme, and a trailing slash is
/// trimmed. An address that already carries a scheme is left as-is, so
/// `http://localhost:8080` still works for local testing.
pub fn normalize_base_url(addr: &str) -> String {
    let addr = addr.trim().trim_end_matches('/');
    if addr.contains("://") {
        addr.to_string()
    } else {
        format!("https://{addr}")
    }
}

/// Install a tracing subscriber, with `RUST_LOG` taking precedence.
///
/// `verbosity` is a count of `-v` flags: none is warnings only — plus the
/// policy decision trail and certificate (ACME) lifecycle, which stay visible
/// at every level; `-v` adds this crate's info, `-vv` its debug, `-vvv`
/// everything including QUIC internals.
pub fn init_tracing(verbosity: u8) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_directives(verbosity)));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(verbosity >= 2)
        .init();
}

/// The `EnvFilter` directives applied when `RUST_LOG` is unset.
///
/// `masque::audit` (every policy decision) and `skimasque::acme` (obtaining and
/// renewing the certificate) stay at `info` even at the quietest level: an
/// operator running without `--audit-log`, or bringing a `--acme` gateway up
/// for the first time, is relying on those lines reaching the logs.
fn default_directives(verbosity: u8) -> &'static str {
    match verbosity {
        0 => "warn,masque::audit=info,skimasque::acme=info",
        1 => "warn,masque::audit=info,skimasque=info,skimasque_cli=info",
        2 => "info,skimasque=debug,skimasque_cli=debug",
        _ => "debug,skimasque=trace,skimasque_cli=trace",
    }
}

#[cfg(test)]
mod tests {
    use super::{default_directives, normalize_base_url};
    use tracing_subscriber::EnvFilter;

    #[test]
    fn every_level_parses_and_the_quietest_still_shows_certs_and_audit() {
        for v in 0..=4 {
            EnvFilter::new(default_directives(v));
        }
        let quiet = default_directives(0);
        assert!(quiet.contains("skimasque::acme=info"), "{quiet}");
        assert!(quiet.contains("masque::audit=info"), "{quiet}");
    }

    #[test]
    fn a_bare_host_gains_https_and_a_scheme_is_kept() {
        assert_eq!(normalize_base_url("control.skimasque.com"), "https://control.skimasque.com");
        assert_eq!(normalize_base_url("gw.example:8443"), "https://gw.example:8443");
        assert_eq!(normalize_base_url("https://control.example/"), "https://control.example");
        assert_eq!(normalize_base_url("http://localhost:8080"), "http://localhost:8080");
        assert_eq!(normalize_base_url("  control.example  "), "https://control.example");
    }
}
