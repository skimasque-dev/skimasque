//! Helpers for the diagnostic `probe` subcommand.
//!
//! The probe exists to answer "is the tunnel actually carrying bytes, and what
//! came back?" when testing against another implementation. It therefore treats
//! payloads as opaque and prints them, with one exception: it can build a DNS
//! query, because a public resolver is the easiest MASQUE target to reach and
//! the reply proves the round trip end to end.

use std::fmt::Write as _;

/// Render bytes as offset, hex, and printable ASCII.
pub fn hexdump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (offset, chunk) in bytes.chunks(16).enumerate() {
        let _ = write!(out, "{:08x}  ", offset * 16);
        for i in 0..16 {
            match chunk.get(i) {
                Some(byte) => {
                    let _ = write!(out, "{byte:02x} ");
                }
                None => out.push_str("   "),
            }
            if i == 7 {
                out.push(' ');
            }
        }
        out.push_str(" |");
        for byte in chunk {
            out.push(if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            });
        }
        out.push_str("|\n");
    }
    out
}

/// Parse a hex string, ignoring spaces and colons so pasted dumps work.
pub fn parse_hex(input: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .collect();
    if !cleaned.len().is_multiple_of(2) {
        return Err("hex input has an odd number of digits".to_owned());
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&cleaned[i..i + 2], 16)
                .map_err(|_| format!("{:?} is not a hex byte", &cleaned[i..i + 2]))
        })
        .collect()
}

/// DNS record types the probe can ask for.
pub const TYPE_A: u16 = 1;
pub const TYPE_AAAA: u16 = 28;

/// Build a standard recursive DNS query for `name`.
///
/// The transaction id is the caller's, so the reply can be matched to the
/// request rather than assumed.
pub fn dns_query(id: u16, name: &str, qtype: u16) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(32 + name.len());
    out.extend_from_slice(&id.to_be_bytes());
    // QR=0 (query), OPCODE=0 (standard), RD=1 (recursion desired).
    out.extend_from_slice(&0x0100u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // AN, NS, AR counts

    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() {
            return Err(format!("{name:?} has an empty label"));
        }
        if label.len() > 63 {
            return Err(format!("label {label:?} exceeds 63 bytes"));
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0); // root label
    out.extend_from_slice(&qtype.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // QCLASS = IN
    Ok(out)
}

/// The header fields of a DNS reply.
///
/// Only the header is parsed. Decoding the answer section means implementing
/// name compression, which is a DNS library's job, not a probe's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DnsHeader {
    pub id: u16,
    pub is_response: bool,
    pub response_code: u8,
    pub questions: u16,
    pub answers: u16,
}

impl DnsHeader {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let header: &[u8; 12] = bytes.get(..12)?.try_into().ok()?;
        let flags = u16::from_be_bytes([header[2], header[3]]);
        Some(Self {
            id: u16::from_be_bytes([header[0], header[1]]),
            is_response: flags & 0x8000 != 0,
            response_code: (flags & 0x000f) as u8,
            questions: u16::from_be_bytes([header[4], header[5]]),
            answers: u16::from_be_bytes([header[6], header[7]]),
        })
    }

    /// The RCODE's name, for codes defined in RFC 1035.
    pub fn response_code_name(&self) -> &'static str {
        match self.response_code {
            0 => "NOERROR",
            1 => "FORMERR",
            2 => "SERVFAIL",
            3 => "NXDOMAIN",
            4 => "NOTIMP",
            5 => "REFUSED",
            _ => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dns_query_has_the_layout_a_resolver_expects() {
        let query = dns_query(0xbeef, "example.com", TYPE_A).unwrap();
        assert_eq!(
            query,
            vec![
                0xbe, 0xef, // id
                0x01, 0x00, // flags: RD
                0x00, 0x01, // QDCOUNT
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // AN, NS, AR
                7, b'e', b'x', b'a', b'm', b'p', b'l', b'e',
                3, b'c', b'o', b'm',
                0,    // root
                0x00, 0x01, // QTYPE = A
                0x00, 0x01, // QCLASS = IN
            ]
        );
    }

    #[test]
    fn a_trailing_dot_does_not_produce_an_empty_label() {
        assert_eq!(
            dns_query(1, "example.com.", TYPE_A).unwrap(),
            dns_query(1, "example.com", TYPE_A).unwrap()
        );
    }

    #[test]
    fn malformed_names_are_rejected() {
        assert!(dns_query(1, "example..com", TYPE_A).is_err());
        assert!(dns_query(1, &"x".repeat(64), TYPE_A).is_err());
    }

    /// A reply is only meaningful if its id matches the query that caused it.
    #[test]
    fn a_reply_header_round_trips_the_transaction_id() {
        let mut reply = dns_query(0x1234, "example.com", TYPE_A).unwrap();
        reply[2] = 0x81; // QR=1, RD=1
        reply[3] = 0x83; // RA=1, RCODE=3 (NXDOMAIN)
        let header = DnsHeader::parse(&reply).unwrap();
        assert_eq!(header.id, 0x1234);
        assert!(header.is_response);
        assert_eq!(header.response_code_name(), "NXDOMAIN");
        assert_eq!(header.questions, 1);
    }

    #[test]
    fn a_short_reply_is_not_a_header() {
        assert_eq!(DnsHeader::parse(&[0u8; 11]), None);
    }

    #[test]
    fn hex_input_tolerates_the_separators_people_paste() {
        assert_eq!(parse_hex("de ad:be\nef").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(parse_hex("").unwrap(), Vec::<u8>::new());
        assert!(parse_hex("abc").is_err());
        assert!(parse_hex("zz").is_err());
    }

    #[test]
    fn hexdump_pads_a_short_final_line_and_shows_ascii() {
        let dump = hexdump(b"hi\x00");
        assert_eq!(dump.lines().count(), 1);
        assert!(dump.starts_with("00000000  68 69 00 "));
        assert!(dump.trim_end().ends_with("|hi.|"));
    }

    #[test]
    fn hexdump_wraps_at_sixteen_bytes() {
        let dump = hexdump(&[0u8; 33]);
        assert_eq!(dump.lines().count(), 3);
        assert!(dump.lines().nth(1).unwrap().starts_with("00000010"));
        assert!(dump.lines().nth(2).unwrap().starts_with("00000020"));
    }
}
