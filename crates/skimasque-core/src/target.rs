//! The target address of a proxying request.
//!
//! A `host:port` pair and its URI Template encoding, shared by CONNECT-UDP
//! (RFC 9298) and CONNECT-TCP (`draft-ietf-httpbis-connect-tcp`). The type is
//! transport-neutral on purpose: RFC 9298 and the CONNECT-TCP draft use the
//! same `target_host` / `target_port` template variables, and a proxy resolves
//! and validates the destination the same way regardless of transport.

use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use crate::template::{self, UriTemplate};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("target host must not be empty")]
    EmptyHost,
    #[error("target port must be between 1 and 65535, got {0:?}")]
    InvalidPort(String),
    #[error("target host {0:?} carries an IPv6 zone identifier, which RFC 9298 does not support")]
    ZoneIdentifier(String),
    #[error("target host {0:?} contains a character that cannot appear in a host")]
    IllegalHost(String),
    #[error("request path {0:?} does not match the configured URI Template")]
    PathDoesNotMatch(String),
    #[error(transparent)]
    Template(#[from] template::Error),
}

/// The `target_host` of a proxying request: a DNS name or an IP literal.
///
/// Kept distinct from `IpAddr` because RFC 9298 makes DNS resolution the
/// proxy's job, and it is the proxy that must reject a name it cannot resolve.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TargetHost {
    Ip(IpAddr),
    Name(String),
}

impl TargetHost {
    /// Validate a host as it appears in a template variable.
    ///
    /// IPv6 literals arrive here already percent-decoded, so `2001:db8::42`
    /// rather than `2001%3Adb8%3A%3A42`, and without the surrounding brackets
    /// that a URI authority would use.
    pub fn parse(host: &str) -> Result<Self, Error> {
        if host.is_empty() {
            return Err(Error::EmptyHost);
        }
        if host.contains('%') {
            // A '%' surviving percent-decoding is a zone id, e.g. `fe80::1%eth0`.
            return Err(Error::ZoneIdentifier(host.to_owned()));
        }
        // Accept the bracketed form too, since it is what a URI authority looks
        // like and clients reach for it by habit.
        let unbracketed = host
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .unwrap_or(host);
        if let Ok(ip) = IpAddr::from_str(unbracketed) {
            return Ok(Self::Ip(ip));
        }
        // A reg-name: no delimiters, no whitespace, no control characters.
        if unbracketed.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, '/' | '?' | '#' | '[' | ']' | '@' | ':')
        }) {
            return Err(Error::IllegalHost(host.to_owned()));
        }
        Ok(Self::Name(unbracketed.to_owned()))
    }

    /// The form that goes into the `target_host` template variable, before
    /// percent-encoding.
    pub fn to_variable(&self) -> String {
        match self {
            Self::Ip(ip) => ip.to_string(),
            Self::Name(name) => name.clone(),
        }
    }
}

impl fmt::Display for TargetHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Bracket IPv6 for display so `host:port` stays unambiguous.
            Self::Ip(IpAddr::V6(ip)) => write!(f, "[{ip}]"),
            Self::Ip(IpAddr::V4(ip)) => write!(f, "{ip}"),
            Self::Name(name) => f.write_str(name),
        }
    }
}

/// The destination of a proxying request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    pub host: TargetHost,
    pub port: u16,
}

impl Target {
    pub fn new(host: TargetHost, port: u16) -> Result<Self, Error> {
        if port == 0 {
            return Err(Error::InvalidPort("0".to_owned()));
        }
        Ok(Self { host, port })
    }

    /// Parse a `host:port` pair as a user would type it on a command line, or
    /// as it arrives in the authority form of a classic `CONNECT` request.
    pub fn parse(input: &str) -> Result<Self, Error> {
        // Split at the last colon so IPv6 literals survive; bracketed forms are
        // handled by splitting after the closing bracket instead.
        let (host, port) = match input.rfind(']') {
            Some(bracket) => {
                let (host, rest) = input.split_at(bracket + 1);
                let port = rest
                    .strip_prefix(':')
                    .ok_or_else(|| Error::InvalidPort(rest.to_owned()))?;
                (host, port)
            }
            None => input
                .rsplit_once(':')
                .ok_or_else(|| Error::InvalidPort(String::new()))?,
        };
        let port: u16 = port
            .parse()
            .map_err(|_| Error::InvalidPort(port.to_owned()))?;
        Self::new(TargetHost::parse(host)?, port)
    }

    /// Expand `template` into the `:path` of a request for this target.
    pub fn expand_path(&self, template: &UriTemplate) -> Result<String, Error> {
        let mut values = BTreeMap::new();
        values.insert("target_host", self.host.to_variable());
        values.insert("target_port", self.port.to_string());
        Ok(template.expand_path(&values)?)
    }

    /// Recover the target a client asked for from the `:path` it sent.
    pub fn from_path(template: &UriTemplate, path: &str) -> Result<Self, Error> {
        let values = template
            .match_path(path)
            .ok_or_else(|| Error::PathDoesNotMatch(path.to_owned()))?;
        let host = values
            .get("target_host")
            .ok_or_else(|| Error::PathDoesNotMatch(path.to_owned()))?;
        let port = values
            .get("target_port")
            .ok_or_else(|| Error::PathDoesNotMatch(path.to_owned()))?;
        let port: u16 = port.parse().map_err(|_| Error::InvalidPort(port.clone()))?;
        Self::new(TargetHost::parse(host)?, port)
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

impl FromStr for Target {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::CONNECT_UDP_VARIABLES;

    fn default_template() -> UriTemplate {
        let template = UriTemplate::default_connect_udp("proxy.example.org:4443").unwrap();
        template.require_variables(&CONNECT_UDP_VARIABLES).unwrap();
        template
    }

    /// The worked example from RFC 9298, Section 3.4.
    #[test]
    fn rfc9298_example_request_path() {
        let template = UriTemplate::parse(
            "https://example.org/.well-known/masque/udp/{target_host}/{target_port}/",
        )
        .unwrap();
        let target = Target::parse("192.0.2.6:443").unwrap();
        assert_eq!(
            target.expand_path(&template).unwrap(),
            "/.well-known/masque/udp/192.0.2.6/443/"
        );
    }

    #[test]
    fn targets_survive_the_round_trip_through_a_path() {
        let template = default_template();
        for input in [
            "192.0.2.6:443",
            "2001:db8::42:53",
            "[2001:db8::42]:53",
            "dns.example.com:853",
        ] {
            let target = Target::parse(input).unwrap();
            let path = target.expand_path(&template).unwrap();
            assert_eq!(Target::from_path(&template, &path).unwrap(), target);
        }
    }

    #[test]
    fn bracketed_and_bare_ipv6_targets_are_the_same_target() {
        assert_eq!(
            Target::parse("[2001:db8::42]:53").unwrap(),
            Target::parse("2001:db8::42:53").unwrap()
        );
    }

    #[test]
    fn display_brackets_ipv6_so_host_port_stays_readable() {
        assert_eq!(
            Target::parse("2001:db8::42:53").unwrap().to_string(),
            "[2001:db8::42]:53"
        );
        assert_eq!(
            Target::parse("1.2.3.4:53").unwrap().to_string(),
            "1.2.3.4:53"
        );
    }

    #[test]
    fn rejects_targets_rfc9298_forbids() {
        // Port 0 is outside the permitted 1-65535 range.
        assert!(matches!(
            Target::parse("192.0.2.6:0"),
            Err(Error::InvalidPort(_))
        ));
        assert!(matches!(
            Target::parse("192.0.2.6:65536"),
            Err(Error::InvalidPort(_))
        ));
        assert!(matches!(Target::parse(":443"), Err(Error::EmptyHost)));
        assert!(matches!(
            Target::parse("192.0.2.6"),
            Err(Error::InvalidPort(_))
        ));
        // Zone identifiers are explicitly unsupported.
        assert!(matches!(
            TargetHost::parse("fe80::1%eth0"),
            Err(Error::ZoneIdentifier(_))
        ));
    }

    #[test]
    fn a_path_for_a_different_template_is_rejected() {
        let template = default_template();
        assert!(matches!(
            Target::from_path(&template, "/.well-known/masque/ip/1.2.3.4/6/"),
            Err(Error::PathDoesNotMatch(_))
        ));
    }

    /// A path that matches the template shape but carries a port outside the
    /// legal range must be rejected rather than silently truncated.
    #[test]
    fn out_of_range_port_in_a_matching_path_is_rejected() {
        let template = default_template();
        assert!(matches!(
            Target::from_path(&template, "/.well-known/masque/udp/1.2.3.4/70000/"),
            Err(Error::InvalidPort(_))
        ));
    }
}
