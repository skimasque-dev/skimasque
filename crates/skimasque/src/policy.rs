//! Which destinations a proxy is willing to reach.
//!
//! A proxy that forwards anywhere is a tool for reaching whatever is behind it:
//! the loopback interface it runs on, the private network it sits in, the cloud
//! metadata service at `169.254.169.254`. The default policy here therefore
//! denies loopback, private, link-local and multicast destinations, and a
//! deployment that genuinely wants them has to say so.
//!
//! The check runs on *resolved addresses*, after DNS. Filtering on the
//! hostname a client sent would be defeated by any name that resolves to a
//! private address, which anyone can arrange.
//!
//! A deployment that needs one internal target -- a production database, say --
//! should name it with [`allowed_cidrs`](AddressPolicy::allowed_cidrs) rather
//! than flipping [`allow_private`](AddressPolicy::allow_private), which opens
//! every private range at once, link-local metadata included.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use ipnet::IpNet;

/// Rules applied to every resolved destination address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressPolicy {
    /// Permit `127.0.0.0/8` and `::1`.
    pub allow_loopback: bool,
    /// Permit RFC 1918 space, RFC 4193 unique-local addresses, and the RFC 6598
    /// shared address space used for carrier-grade NAT.
    pub allow_private: bool,
    /// Permit `169.254.0.0/16` and `fe80::/10`, which includes the address
    /// cloud providers serve instance metadata on.
    pub allow_link_local: bool,
    /// Permit multicast and IPv4 broadcast destinations.
    pub allow_multicast: bool,
    /// Permit the unspecified addresses `0.0.0.0` and `::`.
    pub allow_unspecified: bool,
    /// If set, the only destination ports permitted.
    pub allowed_ports: Option<BTreeSet<u16>>,
    /// Ports refused even when `allowed_ports` would admit them.
    pub denied_ports: BTreeSet<u16>,
    /// Networks whose addresses are permitted regardless of the category flags
    /// above. This is the precise alternative to `allow_private` /
    /// `allow_link_local` / `allow_loopback`: list the exact internal targets a
    /// deployment needs (`10.0.5.0/24`, `[fd00:1::]/64`) and nothing else in
    /// private space is reachable. An operator who lists `169.254.169.254/32`
    /// here is knowingly opting the metadata address in; the blunt flags never
    /// single it out. The port rules still apply.
    pub allowed_cidrs: Vec<IpNet>,
}

impl Default for AddressPolicy {
    /// The policy for a proxy reachable by people you do not know.
    fn default() -> Self {
        Self {
            allow_loopback: false,
            allow_private: false,
            allow_link_local: false,
            allow_multicast: false,
            allow_unspecified: false,
            allowed_ports: None,
            denied_ports: BTreeSet::new(),
            allowed_cidrs: Vec::new(),
        }
    }
}

impl AddressPolicy {
    /// A policy that permits everything.
    ///
    /// Appropriate for a proxy on your own machine that you are testing
    /// against, and for nothing that strangers can reach.
    pub fn permissive() -> Self {
        Self {
            allow_loopback: true,
            allow_private: true,
            allow_link_local: true,
            allow_multicast: true,
            allow_unspecified: true,
            allowed_ports: None,
            denied_ports: BTreeSet::new(),
            allowed_cidrs: Vec::new(),
        }
    }

    /// Restrict destinations to `ports`.
    pub fn with_allowed_ports(mut self, ports: impl IntoIterator<Item = u16>) -> Self {
        self.allowed_ports = Some(ports.into_iter().collect());
        self
    }

    /// Permit addresses inside `cidrs` even when a category flag would refuse
    /// them. See [`allowed_cidrs`](Self::allowed_cidrs).
    pub fn with_allowed_cidrs(mut self, cidrs: impl IntoIterator<Item = IpNet>) -> Self {
        self.allowed_cidrs = cidrs.into_iter().collect();
        self
    }

    /// Check a resolved destination, returning why it is refused.
    pub fn permits(&self, addr: &SocketAddr) -> Result<(), &'static str> {
        if let Some(allowed) = &self.allowed_ports {
            if !allowed.contains(&addr.port()) {
                return Err("destination port is not in the allowed set");
            }
        }
        if self.denied_ports.contains(&addr.port()) {
            return Err("destination port is denied");
        }

        // An IPv4-mapped IPv6 address reaches the same host as the IPv4 address
        // it wraps, so it has to be judged by the IPv4 rules. Skipping this is
        // how `::ffff:127.0.0.1` walks through a loopback ban.
        let ip = match addr.ip() {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
            v4 => v4,
        };

        // An explicitly allow-listed network overrides the category rules -- it
        // is the operator naming exactly what they want reachable. Matched on
        // the unwrapped address so `10.0.0.0/8` also covers `::ffff:10.x`.
        if self.allowed_cidrs.iter().any(|net| net.contains(&ip)) {
            return Ok(());
        }

        match ip {
            IpAddr::V4(v4) => self.permits_v4(v4),
            IpAddr::V6(v6) => self.permits_v6(v6),
        }
    }

    fn permits_v4(&self, addr: Ipv4Addr) -> Result<(), &'static str> {
        if addr.is_unspecified() && !self.allow_unspecified {
            return Err("destination is the unspecified address");
        }
        if addr.is_loopback() && !self.allow_loopback {
            return Err("destination is a loopback address");
        }
        if addr.is_link_local() && !self.allow_link_local {
            return Err("destination is a link-local address");
        }
        if (addr.is_multicast() || addr.is_broadcast()) && !self.allow_multicast {
            return Err("destination is a multicast or broadcast address");
        }
        if (addr.is_private() || is_shared_v4(addr)) && !self.allow_private {
            return Err("destination is a private address");
        }
        Ok(())
    }

    fn permits_v6(&self, addr: Ipv6Addr) -> Result<(), &'static str> {
        if addr.is_unspecified() && !self.allow_unspecified {
            return Err("destination is the unspecified address");
        }
        if addr.is_loopback() && !self.allow_loopback {
            return Err("destination is a loopback address");
        }
        if is_link_local_v6(addr) && !self.allow_link_local {
            return Err("destination is a link-local address");
        }
        if addr.is_multicast() && !self.allow_multicast {
            return Err("destination is a multicast address");
        }
        if is_unique_local_v6(addr) && !self.allow_private {
            return Err("destination is a unique-local address");
        }
        Ok(())
    }
}

/// RFC 6598 shared address space, `100.64.0.0/10`.
///
/// `Ipv4Addr::is_shared` is still unstable, so this open-codes it.
fn is_shared_v4(addr: Ipv4Addr) -> bool {
    let [a, b, ..] = addr.octets();
    a == 100 && (64..128).contains(&b)
}

/// RFC 4291 link-local unicast, `fe80::/10`.
fn is_link_local_v6(addr: Ipv6Addr) -> bool {
    addr.segments()[0] & 0xffc0 == 0xfe80
}

/// RFC 4193 unique local addresses, `fc00::/7`.
fn is_unique_local_v6(addr: Ipv6Addr) -> bool {
    addr.segments()[0] & 0xfe00 == 0xfc00
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn the_default_policy_refuses_the_networks_a_proxy_can_be_abused_to_reach() {
        let policy = AddressPolicy::default();
        for destination in [
            "127.0.0.1:53",
            "[::1]:53",
            "10.0.0.1:53",
            "172.16.0.1:53",
            "192.168.1.1:53",
            "100.64.0.1:53",
            "169.254.169.254:80",
            "[fe80::1]:53",
            "[fc00::1]:53",
            "[fd00::1]:53",
            "224.0.0.1:53",
            "255.255.255.255:53",
            "0.0.0.0:53",
            "[::]:53",
        ] {
            assert!(
                policy.permits(&addr(destination)).is_err(),
                "{destination} should be refused by default"
            );
        }
    }

    #[test]
    fn the_default_policy_permits_ordinary_public_destinations() {
        let policy = AddressPolicy::default();
        for destination in ["1.1.1.1:53", "8.8.8.8:53", "[2001:4860:4860::8888]:53"] {
            assert_eq!(policy.permits(&addr(destination)), Ok(()), "{destination}");
        }
    }

    /// The classic bypass: wrap a banned IPv4 address in an IPv6 one.
    #[test]
    fn ipv4_mapped_addresses_cannot_smuggle_past_the_ipv4_rules() {
        let policy = AddressPolicy::default();
        assert!(policy.permits(&addr("[::ffff:127.0.0.1]:53")).is_err());
        assert!(policy.permits(&addr("[::ffff:10.0.0.1]:53")).is_err());
        assert!(policy.permits(&addr("[::ffff:169.254.169.254]:80")).is_err());
        // A mapped public address is still fine.
        assert_eq!(policy.permits(&addr("[::ffff:1.1.1.1]:53")), Ok(()));
    }

    #[test]
    fn the_permissive_policy_permits_everything_the_default_refuses() {
        let policy = AddressPolicy::permissive();
        for destination in ["127.0.0.1:53", "[::1]:53", "192.168.1.1:53", "[fe80::1]:53"] {
            assert_eq!(policy.permits(&addr(destination)), Ok(()), "{destination}");
        }
    }

    #[test]
    fn an_allowed_cidr_opens_exactly_its_range_and_nothing_else_private() {
        let policy = AddressPolicy::default()
            .with_allowed_cidrs(["10.0.5.0/24".parse().unwrap()]);

        // The named range is reachable...
        assert_eq!(policy.permits(&addr("10.0.5.20:5432")), Ok(()));
        // ...via an IPv4-mapped address too...
        assert_eq!(policy.permits(&addr("[::ffff:10.0.5.20]:5432")), Ok(()));
        // ...but the rest of private space, and the metadata address, are not.
        assert!(policy.permits(&addr("10.0.6.1:5432")).is_err());
        assert!(policy.permits(&addr("192.168.1.1:5432")).is_err());
        assert!(policy.permits(&addr("169.254.169.254:80")).is_err());
    }

    #[test]
    fn an_allowed_cidr_can_single_out_the_metadata_address() {
        let policy = AddressPolicy::default()
            .with_allowed_cidrs(["169.254.169.254/32".parse().unwrap()]);
        assert_eq!(policy.permits(&addr("169.254.169.254:80")), Ok(()));
        // A neighbour in the same link-local block is still refused.
        assert!(policy.permits(&addr("169.254.169.253:80")).is_err());
    }

    #[test]
    fn an_allowed_cidr_still_answers_to_the_port_rules() {
        let policy = AddressPolicy::default()
            .with_allowed_cidrs(["10.0.5.0/24".parse().unwrap()])
            .with_allowed_ports([5432]);
        assert_eq!(policy.permits(&addr("10.0.5.20:5432")), Ok(()));
        assert!(policy.permits(&addr("10.0.5.20:22")).is_err());
    }

    #[test]
    fn port_restrictions_apply_independently_of_the_address_rules() {
        let policy = AddressPolicy::permissive().with_allowed_ports([53, 853]);
        assert_eq!(policy.permits(&addr("1.1.1.1:53")), Ok(()));
        assert_eq!(policy.permits(&addr("1.1.1.1:853")), Ok(()));
        assert!(policy.permits(&addr("1.1.1.1:443")).is_err());

        let mut policy = AddressPolicy::permissive();
        policy.denied_ports.insert(25);
        assert!(policy.permits(&addr("1.1.1.1:25")).is_err());
        assert_eq!(policy.permits(&addr("1.1.1.1:53")), Ok(()));
    }

    #[test]
    fn range_helpers_match_their_prefixes() {
        assert!(is_shared_v4("100.64.0.0".parse().unwrap()));
        assert!(is_shared_v4("100.127.255.255".parse().unwrap()));
        assert!(!is_shared_v4("100.63.255.255".parse().unwrap()));
        assert!(!is_shared_v4("100.128.0.0".parse().unwrap()));

        assert!(is_link_local_v6("fe80::".parse().unwrap()));
        assert!(is_link_local_v6("febf:ffff::".parse().unwrap()));
        assert!(!is_link_local_v6("fec0::".parse().unwrap()));

        assert!(is_unique_local_v6("fc00::".parse().unwrap()));
        assert!(is_unique_local_v6("fdff::".parse().unwrap()));
        assert!(!is_unique_local_v6("fe00::".parse().unwrap()));
    }
}
