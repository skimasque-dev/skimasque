//! WHERE a workload wants to go, and the patterns a policy matches it with.
//!
//! Two types do the work here. [`Destination`] is a concrete request: one host
//! -- a name or an IP literal -- and one port. [`DestinationSpec`] is a pattern
//! a policy is written with: an exact host, a `*.suffix` wildcard, an IP, a
//! CIDR, or `*`, paired with an exact port, a range, or `*`.
//!
//! The match is performed on the destination *as the client named it*, before
//! DNS. A policy that lists `api.example.com:443` matches a request for that
//! name; it does not match a request that names a raw IP address, even one the
//! name would resolve to. Re-checking the resolved address against the
//! network's own floor (no loopback, no link-local, no metadata service) is a
//! separate concern that belongs next to the resolver, not here.

use std::fmt;
use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseDestinationError {
    #[error("{0:?} is empty")]
    Empty(String),
    #[error("{0:?} has no port; expected host:port")]
    MissingPort(String),
    #[error("{value:?} is not a valid destination: {reason}")]
    Malformed { value: String, reason: &'static str },
    #[error("port {0:?} is not in 1..=65535")]
    BadPort(String),
    #[error("prefix length {len} is too long for an {family} address")]
    BadPrefix { len: u16, family: &'static str },
}

fn malformed(value: &str, reason: &'static str) -> ParseDestinationError {
    ParseDestinationError::Malformed {
        value: value.to_owned(),
        reason,
    }
}

/// The host half of a [`Destination`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    /// A DNS name, stored lowercased so comparison is case-insensitive.
    Name(String),
    /// An IP literal.
    Ip(IpAddr),
}

/// A concrete destination: exactly where one request wants to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub host: Host,
    pub port: u16,
}

impl Destination {
    /// Parse `host:port`. The host may be a name, an IPv4 literal, or a
    /// bracketed IPv6 literal (`[2001:db8::1]:443`).
    pub fn parse(input: &str) -> Result<Self, ParseDestinationError> {
        let text = input.trim();
        if text.is_empty() {
            return Err(ParseDestinationError::Empty(input.to_owned()));
        }

        let (host_part, port_part) = split_host_port(text)
            .ok_or_else(|| ParseDestinationError::MissingPort(input.to_owned()))?;

        let port: u16 = port_part
            .parse()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| ParseDestinationError::BadPort(port_part.to_owned()))?;

        let host = parse_host_literal(host_part)?;
        Ok(Self { host, port })
    }
}

impl fmt::Display for Destination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.host {
            Host::Name(name) => write!(f, "{name}:{}", self.port),
            Host::Ip(IpAddr::V4(v4)) => write!(f, "{v4}:{}", self.port),
            Host::Ip(IpAddr::V6(v6)) => write!(f, "[{v6}]:{}", self.port),
        }
    }
}

/// How a [`DestinationSpec`] constrains the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPattern {
    /// `*` -- any host.
    Any,
    /// An exact name, lowercased.
    Exact(String),
    /// `*.example.com` -- the stored string is `.example.com`, and it matches
    /// any name ending in it, but not the apex `example.com` itself.
    Suffix(String),
    /// A single IP literal.
    Ip(IpAddr),
    /// A CIDR block; matches an IP destination inside it.
    Cidr { addr: IpAddr, len: u8 },
}

/// How a [`DestinationSpec`] constrains the port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortPattern {
    /// `*` or an omitted port -- any port.
    Any,
    /// One port.
    Exact(u16),
    /// An inclusive range, `8000-8100`.
    Range(u16, u16),
}

impl PortPattern {
    fn matches(&self, port: u16) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(p) => *p == port,
            Self::Range(lo, hi) => (*lo..=*hi).contains(&port),
        }
    }
}

/// A destination pattern from a policy rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationSpec {
    pub host: HostPattern,
    pub port: PortPattern,
    /// The text this was parsed from, kept so denials can quote the policy
    /// back verbatim.
    raw: String,
}

impl DestinationSpec {
    /// Parse a spec such as `api.example.com:443`, `*.example.com:443`,
    /// `10.0.0.0/8`, `192.0.2.1:5432`, `registry.example.com` (any port) or
    /// `*` (anything).
    pub fn parse(input: &str) -> Result<Self, ParseDestinationError> {
        let text = input.trim();
        if text.is_empty() {
            return Err(ParseDestinationError::Empty(input.to_owned()));
        }
        let raw = text.to_owned();

        if text == "*" {
            return Ok(Self {
                host: HostPattern::Any,
                port: PortPattern::Any,
                raw,
            });
        }

        // A CIDR spec (`10.0.0.0/8`) carries no port: the slash is unambiguous,
        // and a port on a whole block is not a thing operators write.
        if let Some((addr, len)) = text.split_once('/') {
            let addr: IpAddr = addr
                .parse()
                .map_err(|_| malformed(input, "the part before / is not an IP address"))?;
            let len: u16 = len
                .parse()
                .map_err(|_| malformed(input, "the prefix length is not a number"))?;
            let max = if addr.is_ipv6() { 128 } else { 32 };
            if len > max {
                return Err(ParseDestinationError::BadPrefix {
                    len,
                    family: if addr.is_ipv6() { "IPv6" } else { "IPv4" },
                });
            }
            return Ok(Self {
                host: HostPattern::Cidr { addr, len: len as u8 },
                port: PortPattern::Any,
                raw,
            });
        }

        let (host_part, port) = match split_host_port(text) {
            Some((host, port_str)) => (host, parse_port_pattern(port_str)?),
            None => (text, PortPattern::Any),
        };

        let host = if host_part == "*" {
            HostPattern::Any
        } else if let Some(suffix) = host_part.strip_prefix("*.") {
            if suffix.is_empty() || suffix.contains('*') {
                return Err(malformed(input, "a wildcard host must be *.<domain>"));
            }
            HostPattern::Suffix(format!(".{}", suffix.to_ascii_lowercase()))
        } else if let Ok(ip) = host_part.parse::<IpAddr>() {
            HostPattern::Ip(ip)
        } else {
            match parse_host_literal(host_part)? {
                Host::Name(name) => HostPattern::Exact(name),
                Host::Ip(ip) => HostPattern::Ip(ip),
            }
        };

        Ok(Self { host, port, raw })
    }

    /// The text this spec was written as.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Whether this spec covers `destination`.
    pub fn matches(&self, destination: &Destination) -> bool {
        self.port.matches(destination.port) && self.host_matches(&destination.host)
    }

    fn host_matches(&self, host: &Host) -> bool {
        match (&self.host, host) {
            (HostPattern::Any, _) => true,
            (HostPattern::Exact(want), Host::Name(got)) => want == got,
            (HostPattern::Suffix(suffix), Host::Name(got)) => got.ends_with(suffix.as_str()),
            (HostPattern::Ip(want), Host::Ip(got)) => want == got,
            (HostPattern::Cidr { addr, len }, Host::Ip(got)) => cidr_contains(*addr, *len, *got),
            // A name pattern never matches an IP request and vice versa: the
            // client named one or the other, and policy judges what was named.
            _ => false,
        }
    }
}

impl fmt::Display for DestinationSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

/// Split `host:port`, respecting a bracketed IPv6 literal and refusing a bare
/// IPv6 literal (whose colons are ambiguous). Returns `None` when there is no
/// port to split off.
fn split_host_port(text: &str) -> Option<(&str, &str)> {
    if let Some(rest) = text.strip_prefix('[') {
        // `[2001:db8::1]:443`
        let (addr, after) = rest.split_once(']')?;
        let port = after.strip_prefix(':')?;
        return Some((addr, port));
    }
    let (host, port) = text.rsplit_once(':')?;
    // More than one colon left in an unbracketed host means an IPv6 literal
    // without brackets: reject by refusing to treat the last colon as the
    // port separator.
    if host.contains(':') {
        return None;
    }
    Some((host, port))
}

fn parse_port_pattern(text: &str) -> Result<PortPattern, ParseDestinationError> {
    if text == "*" {
        return Ok(PortPattern::Any);
    }
    if let Some((lo, hi)) = text.split_once('-') {
        let lo: u16 = lo.parse().map_err(|_| ParseDestinationError::BadPort(text.to_owned()))?;
        let hi: u16 = hi.parse().map_err(|_| ParseDestinationError::BadPort(text.to_owned()))?;
        if lo == 0 || hi == 0 || lo > hi {
            return Err(ParseDestinationError::BadPort(text.to_owned()));
        }
        return Ok(PortPattern::Range(lo, hi));
    }
    let port: u16 = text
        .parse()
        .ok()
        .filter(|p| *p != 0)
        .ok_or_else(|| ParseDestinationError::BadPort(text.to_owned()))?;
    Ok(PortPattern::Exact(port))
}

/// Validate and normalise a host literal that is meant to be a name or an IP.
fn parse_host_literal(host: &str) -> Result<Host, ParseDestinationError> {
    if host.is_empty() {
        return Err(malformed(host, "the host is empty"));
    }
    let unbracketed = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = unbracketed.parse::<IpAddr>() {
        return Ok(Host::Ip(ip));
    }
    // A reg-name: letters, digits, hyphen and dot, nothing structural.
    if unbracketed
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')))
    {
        return Err(malformed(host, "the host contains an illegal character"));
    }
    Ok(Host::Name(unbracketed.to_ascii_lowercase()))
}

/// Whether `addr`/`len` contains `candidate`.
fn cidr_contains(addr: IpAddr, len: u8, candidate: IpAddr) -> bool {
    match (addr, candidate) {
        (IpAddr::V4(net), IpAddr::V4(got)) => bits_match(&net.octets(), &got.octets(), len),
        (IpAddr::V6(net), IpAddr::V6(got)) => bits_match(&net.octets(), &got.octets(), len),
        _ => false,
    }
}

fn bits_match(net: &[u8], got: &[u8], len: u8) -> bool {
    let whole = (len / 8) as usize;
    if net[..whole] != got[..whole] {
        return false;
    }
    let remainder = len % 8;
    if remainder == 0 {
        return true;
    }
    let mask = 0xffu8 << (8 - remainder);
    (net[whole] & mask) == (got[whole] & mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dest(s: &str) -> Destination {
        Destination::parse(s).unwrap()
    }
    fn spec(s: &str) -> DestinationSpec {
        DestinationSpec::parse(s).unwrap()
    }

    #[test]
    fn destinations_parse_names_and_both_ip_families() {
        assert_eq!(dest("api.example.com:443").host, Host::Name("api.example.com".into()));
        assert_eq!(dest("API.Example.COM:443").host, Host::Name("api.example.com".into()));
        assert_eq!(dest("192.0.2.1:5432").host, Host::Ip("192.0.2.1".parse().unwrap()));
        assert_eq!(dest("[2001:db8::1]:53").host, Host::Ip("2001:db8::1".parse().unwrap()));
        assert_eq!(dest("api.example.com:443").port, 443);
    }

    #[test]
    fn a_destination_needs_a_nonzero_port() {
        assert!(Destination::parse("api.example.com").is_err());
        assert!(Destination::parse("api.example.com:0").is_err());
        assert!(Destination::parse("api.example.com:99999").is_err());
        assert!(Destination::parse("2001:db8::1:53").is_err(), "bare v6 is ambiguous");
    }

    #[test]
    fn an_exact_spec_matches_only_that_name_and_port() {
        let s = spec("api.production.example.com:443");
        assert!(s.matches(&dest("api.production.example.com:443")));
        assert!(!s.matches(&dest("api.production.example.com:8443")));
        assert!(!s.matches(&dest("other.production.example.com:443")));
    }

    #[test]
    fn a_wildcard_suffix_matches_subdomains_but_not_the_apex() {
        let s = spec("*.example.com:443");
        assert!(s.matches(&dest("api.example.com:443")));
        assert!(s.matches(&dest("a.b.example.com:443")));
        assert!(!s.matches(&dest("example.com:443")));
        assert!(!s.matches(&dest("notexample.com:443")));
    }

    #[test]
    fn a_port_range_and_star_behave() {
        assert!(spec("api.example.com:8000-8100").matches(&dest("api.example.com:8050")));
        assert!(!spec("api.example.com:8000-8100").matches(&dest("api.example.com:9000")));
        assert!(spec("api.example.com").matches(&dest("api.example.com:443")));
        assert!(spec("api.example.com:*").matches(&dest("api.example.com:1")));
    }

    #[test]
    fn cidr_specs_contain_the_addresses_you_expect() {
        assert!(spec("10.0.0.0/8").matches(&dest("10.1.2.3:5432")));
        assert!(!spec("10.0.0.0/8").matches(&dest("11.0.0.1:5432")));
        assert!(spec("192.0.2.0/24").matches(&dest("192.0.2.200:443")));
        assert!(!spec("192.0.2.0/24").matches(&dest("192.0.3.1:443")));
        assert!(spec("2001:db8::/32").matches(&dest("[2001:db8:1234::1]:53")));
        assert!(!spec("2001:db8::/32").matches(&dest("[2001:db9::1]:53")));
        // A CIDR never matches a name, and a name spec never matches an IP.
        assert!(!spec("10.0.0.0/8").matches(&dest("host.example.com:443")));
        assert!(!spec("api.example.com:443").matches(&dest("192.0.2.1:443")));
    }

    #[test]
    fn the_bare_star_matches_anything() {
        let s = spec("*");
        assert!(s.matches(&dest("anywhere.example.com:1")));
        assert!(s.matches(&dest("192.0.2.1:65535")));
    }

    #[test]
    fn malformed_specs_are_rejected() {
        assert!(DestinationSpec::parse("").is_err());
        assert!(DestinationSpec::parse("*.*.com:443").is_err());
        assert!(DestinationSpec::parse("10.0.0.0/40").is_err());
        assert!(DestinationSpec::parse("host name:443").is_err());
        assert!(DestinationSpec::parse("api.example.com:-5").is_err());
    }
}
