//! Wire-format types for IETF MASQUE.
//!
//! This crate is deliberately transport-free: it does no I/O, spawns no tasks,
//! and depends only on `bytes`. Everything here is a codec or a validated
//! value type, so the protocol can be tested exhaustively without a network,
//! and so the same types serve a client, a proxy, and an intermediary.
//!
//! | Module | Specification |
//! |---|---|
//! | [`varint`] | QUIC variable-length integers (RFC 9000, Section 16) |
//! | [`datagram`] | HTTP Datagrams and context ids (RFC 9297, Section 2) |
//! | [`capsule`] | The Capsule Protocol (RFC 9297, Section 3) |
//! | [`template`] | Proxy URI Templates (RFC 6570, profiled by RFC 9298) |
//! | [`target`] | The `host:port` target address, shared by CONNECT-UDP and CONNECT-TCP |
//! | [`connect_udp`] | Proxying UDP in HTTP (RFC 9298) |
//! | [`connect_tcp`] | Proxying TCP in HTTP (`draft-ietf-httpbis-connect-tcp`) |
//! | [`connect_ip`] | Proxying IP in HTTP (RFC 9484) |
//!
//! The transport that carries these formats lives in the `skimasque` crate.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod capsule;
pub mod connect_ip;
pub mod connect_tcp;
pub mod connect_udp;
pub mod datagram;
pub mod target;
pub mod template;
pub mod varint;

pub use capsule::{Capsule, CapsuleDecoder, CapsuleType};
pub use datagram::{ContextId, ProxyingPayload, QuarterStreamId};
pub use template::UriTemplate;

/// The `Capsule-Protocol` header field name (RFC 9297, Section 3.4).
pub const CAPSULE_PROTOCOL_HEADER: &str = "capsule-protocol";

/// The only `Capsule-Protocol` value that turns the protocol on: the Structured
/// Fields spelling of boolean true.
pub const CAPSULE_PROTOCOL_TRUE: &str = "?1";

/// Interpret a `Capsule-Protocol` header value.
///
/// RFC 9297 says a value that is not a Structured Fields boolean must be
/// treated as if the field were absent, so this returns `false` rather than an
/// error for anything unrecognised.
pub fn capsule_protocol_enabled(value: &str) -> bool {
    value.trim() == CAPSULE_PROTOCOL_TRUE
}

/// The MASQUE proxying modes this crate can encode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    /// RFC 9298, `connect-udp`.
    ConnectUdp,
    /// `draft-ietf-httpbis-connect-tcp`, `connect-tcp`. Names the
    /// template-driven variant; classic `CONNECT` carries no `:protocol`.
    ConnectTcp,
    /// RFC 9484, `connect-ip`.
    ConnectIp,
}

impl Protocol {
    /// The HTTP upgrade token, used as the `:protocol` pseudo-header on
    /// HTTP/2 and HTTP/3 and in `Upgrade` on HTTP/1.1.
    pub const fn upgrade_token(self) -> &'static str {
        match self {
            Self::ConnectUdp => connect_udp::UPGRADE_TOKEN,
            Self::ConnectTcp => connect_tcp::UPGRADE_TOKEN,
            Self::ConnectIp => connect_ip::UPGRADE_TOKEN,
        }
    }

    /// The `.well-known` URI Template a proxy offers when it advertises none.
    pub fn default_template(self, proxy_authority: &str) -> Result<UriTemplate, template::Error> {
        match self {
            Self::ConnectUdp => UriTemplate::default_connect_udp(proxy_authority),
            Self::ConnectTcp => UriTemplate::default_connect_tcp(proxy_authority),
            Self::ConnectIp => UriTemplate::default_connect_ip(proxy_authority),
        }
    }
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.upgrade_token())
    }
}

impl std::str::FromStr for Protocol {
    type Err = UnknownProtocol;

    fn from_str(token: &str) -> Result<Self, Self::Err> {
        match token {
            connect_udp::UPGRADE_TOKEN => Ok(Self::ConnectUdp),
            connect_tcp::UPGRADE_TOKEN => Ok(Self::ConnectTcp),
            connect_ip::UPGRADE_TOKEN => Ok(Self::ConnectIp),
            other => Err(UnknownProtocol(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a MASQUE upgrade token")]
pub struct UnknownProtocol(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_tokens_round_trip() {
        for protocol in [
            Protocol::ConnectUdp,
            Protocol::ConnectTcp,
            Protocol::ConnectIp,
        ] {
            assert_eq!(
                protocol.upgrade_token().parse::<Protocol>().unwrap(),
                protocol
            );
        }
        assert!("webtransport".parse::<Protocol>().is_err());
    }

    #[test]
    fn connect_tcp_default_template_matches_the_draft_shape() {
        let template = Protocol::ConnectTcp
            .default_template("proxy.example:4433")
            .unwrap();
        template
            .require_variables(&template::CONNECT_TCP_VARIABLES)
            .unwrap();
        let target = connect_tcp::Target::parse("192.0.2.6:443").unwrap();
        let path = target.expand_path(&template).unwrap();
        assert_eq!(path, "/.well-known/masque/tcp/192.0.2.6/443/");
        assert_eq!(
            connect_tcp::Target::from_path(&template, &path).unwrap(),
            target
        );
    }

    #[test]
    fn only_structured_boolean_true_enables_the_capsule_protocol() {
        assert!(capsule_protocol_enabled("?1"));
        assert!(capsule_protocol_enabled(" ?1 "));
        for value in ["?0", "1", "true", "", "?1, ?1"] {
            assert!(!capsule_protocol_enabled(value), "{value:?}");
        }
    }
}
