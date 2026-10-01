//! Validate the complete packet before handing it to the userspace stack.
use netstack_smoltcp::smoltcp::wire::{
    IpAddress, IpProtocol, Ipv4Packet, Ipv6Packet, TcpPacket, UdpPacket,
};
pub fn valid(bytes: &[u8]) -> bool {
    if bytes.len() > 1280 {
        return false;
    }
    let (source, destination, protocol, payload) = match bytes.first().map(|b| b >> 4) {
        Some(4) => {
            let Ok(p) = Ipv4Packet::new_checked(bytes) else {
                return false;
            };
            if usize::from(p.total_len()) != bytes.len()
                || !p.verify_checksum()
                || p.more_frags()
                || p.frag_offset() != 0
            {
                return false;
            }
            (
                IpAddress::Ipv4(p.src_addr()),
                IpAddress::Ipv4(p.dst_addr()),
                p.next_header(),
                p.payload(),
            )
        }
        Some(6) => {
            let Ok(p) = Ipv6Packet::new_checked(bytes) else {
                return false;
            };
            if usize::from(p.payload_len()) + 40 != bytes.len() {
                return false;
            }
            (
                IpAddress::Ipv6(p.src_addr()),
                IpAddress::Ipv6(p.dst_addr()),
                p.next_header(),
                p.payload(),
            )
        }
        _ => return false,
    };
    match protocol {
        IpProtocol::Tcp => {
            TcpPacket::new_checked(payload).is_ok_and(|p| p.verify_checksum(&source, &destination))
        }
        IpProtocol::Udp => UdpPacket::new_checked(payload).is_ok_and(|p| {
            usize::from(p.len()) == payload.len() && p.verify_checksum(&source, &destination)
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(ipv6: bool) -> Vec<u8> {
        let header = if ipv6 { 40 } else { 20 };
        let mut bytes = vec![0u8; header + 8];
        let source = if ipv6 {
            IpAddress::Ipv6("fd42::1".parse().unwrap())
        } else {
            IpAddress::Ipv4("10.0.0.1".parse().unwrap())
        };
        let dest = if ipv6 {
            IpAddress::Ipv6("fd42::2".parse().unwrap())
        } else {
            IpAddress::Ipv4("10.0.0.2".parse().unwrap())
        };
        if ipv6 {
            let mut p = Ipv6Packet::new_unchecked(&mut bytes[..]);
            p.set_version(6);
            p.set_payload_len(8);
            p.set_next_header(IpProtocol::Udp);
            p.set_hop_limit(64);
            if let (IpAddress::Ipv6(s), IpAddress::Ipv6(d)) = (source, dest) {
                p.set_src_addr(s);
                p.set_dst_addr(d);
            }
        } else {
            let mut p = Ipv4Packet::new_unchecked(&mut bytes[..]);
            p.set_version(4);
            p.set_header_len(20);
            p.set_total_len(28);
            p.set_next_header(IpProtocol::Udp);
            p.set_hop_limit(64);
            if let (IpAddress::Ipv4(s), IpAddress::Ipv4(d)) = (source, dest) {
                p.set_src_addr(s);
                p.set_dst_addr(d);
            }
            p.fill_checksum();
        }
        let mut udp = UdpPacket::new_unchecked(&mut bytes[header..]);
        udp.set_src_port(4000);
        udp.set_dst_port(53);
        udp.set_len(8);
        udp.fill_checksum(&source, &dest);
        bytes
    }
    #[test]
    fn validates_empty_udp_and_rejects_corrupt_packets_without_poisoning_next_packet() {
        for ipv6 in [false, true] {
            let good = packet(ipv6);
            assert!(valid(&good));
            for n in 0..good.len() {
                assert!(!valid(&good[..n]));
            }
            let mut corrupt = good.clone();
            *corrupt.last_mut().unwrap() ^= 1;
            assert!(!valid(&corrupt));
            assert!(!valid(&vec![0; 1281]));
            assert!(valid(&good));
        }
    }
    #[test]
    fn fragments_and_icmp_cannot_fake_remote_success() {
        let mut bytes = packet(false);
        let mut p = Ipv4Packet::new_unchecked(&mut bytes[..]);
        p.set_more_frags(true);
        p.fill_checksum();
        assert!(!valid(&bytes));
        let mut bytes = packet(false);
        let mut p = Ipv4Packet::new_unchecked(&mut bytes[..]);
        p.set_next_header(IpProtocol::Icmp);
        p.fill_checksum();
        assert!(!valid(&bytes));
    }
}
