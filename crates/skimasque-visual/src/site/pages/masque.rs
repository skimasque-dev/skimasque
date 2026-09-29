//! MASQUE (`/technology/masque`): canonical spec §22. The IP-support sentence
//! is replaced with the real status: CONNECT-IP wire formats are done, TUN
//! forwarding is not wired.

use crate::diagrams::platform;
use crate::site::Page;
use crate::{ComparisonTable, Component, Hero, PlannedBlock, Prose, Section, SitePage};

pub fn page() -> Page {
    let hero = Hero::new("Real MASQUE underneath.").lead(
        "SkiMasque is built around the IETF MASQUE architecture rather than presenting a proprietary tunnel abstraction as the product.",
    );

    let stack = Section::new("Protocol stack").push(&platform::masque_stack());

    let quic = Section::new("HTTP/3 + QUIC").alt().push(
        &Prose::new().p(
            "The transport layer provides the foundation for modern multiplexed network sessions. Only HTTP/3 is supported today.",
        ),
    );

    let connect =
        Section::new("Extended CONNECT")
            .push(&Prose::new().p(
                "SkiMasque uses the HTTP/3 extended CONNECT mechanisms associated with MASQUE.",
            ));

    let udp = Section::new("UDP")
        .alt()
        .push(&Prose::new().p("UDP semantics can be preserved end-to-end where supported."))
        .push(&platform::connect_udp_sequence());

    let ip = Section::new("IP support")
        .push(&Prose::new().p(
            "IP proxying (RFC 9484) is partly implemented. The wire formats are complete; forwarding IP packets through a TUN device is not wired yet.",
        ))
        .push(&PlannedBlock::new(
            "CONNECT-IP forwarding through a TUN device",
            &Prose::new().p(
                "Today the gateway carries UDP and TCP tunnels and a SOCKS5 front end. Forwarding whole IP packets is not available yet.",
            ),
        ));

    let status =
        Section::new("Standards status")
            .alt()
            .push(&Prose::new().p(
                "The transport implements the IETF MASQUE specifications in Rust, on quinn and h3.",
            ))
            .push(
                &ComparisonTable::new(&["Specification", "Status", "Detail"])
                    .row(&[
                        "RFC 9297 — HTTP Datagrams and the Capsule Protocol",
                        "Complete",
                        "in both encodings",
                    ])
                    .row(&[
                        "RFC 9298 — Proxying UDP in HTTP",
                        "Complete",
                        "over HTTP/3 extended CONNECT",
                    ])
                    .row(&[
                        "draft-ietf-httpbis-connect-tcp — Proxying TCP in HTTP",
                        "On by default",
                        "classic CONNECT host:port",
                    ])
                    .row(&[
                        "RFC 9484 — Proxying IP in HTTP",
                        "Partial",
                        "wire formats complete; TUN forwarding not yet wired",
                    ])
                    .row(&["RFC 1928 — SOCKS5", "Complete", "CONNECT and UDP ASSOCIATE"]),
            );

    let positioning = Section::new("Where MASQUE fits").push(
        &Prose::new()
            .quote("SkiMasque provides identity-aware network access.")
            .quote("MASQUE is the network transport underneath it."),
    );

    Page {
        path: "technology/masque/index.html",
        contents: SitePage::new(
            "../../",
            "technology/masque",
            "MASQUE · SkiMasque",
            "SkiMasque is built around the IETF MASQUE architecture: HTTP/3, QUIC, extended CONNECT and UDP proxying, with IP proxying partly implemented.",
        )
        .push(&hero)
        .push(&stack)
        .push(&quic)
        .push(&connect)
        .push(&udp)
        .push(&ip)
        .push(&status)
        .push(&positioning)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masque_page_is_honest_about_ip_proxying() {
        let p = page();
        assert_eq!(p.path, "technology/masque/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        assert!(
            s.contains("../../assets/visual.css"),
            "page depth root is ../../"
        );
        for want in [
            "Real MASQUE underneath.",
            "Application",
            "MASQUE",
            "HTTP/3",
            "QUIC",
            "UDP/IP",
            "Extended CONNECT",
            "RFC 9484",
            "RFC 9297",
            "RFC 1928",
            "TUN forwarding not yet wired",
            "identity-aware network access",
            "the network transport underneath it",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
        assert!(!s.contains("evolving SkiMasque transport"));
        let main = &s[s.find("<main").unwrap()..s.find("</main>").unwrap()];
        assert_eq!(
            main.matches("PLANNED").count(),
            1,
            "TUN forwarding is Planned"
        );
    }
}
