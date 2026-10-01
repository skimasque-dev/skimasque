//! Trust and reliability (`/trust`): canonical spec §28. Each topic states only
//! what `docs/security.md`, `docs/threat-model.md`, `docs/control-plane.md` and
//! `SECURITY.md` document; everything else is a Planned line. No availability
//! target, SLA, certification or security mailbox is claimed.

use super::{doc, DocLinks};
use crate::site::{Page, REPO_URL};
use crate::{Compare, Component, Hero, PlannedBlock, Prose, Section, SitePage, Tone};

fn planned(note: &str, line: &str) -> PlannedBlock {
    PlannedBlock::new(note, &Prose::new().p(line))
}

pub fn page() -> Page {
    let hero = Hero::new("Network access is infrastructure. Treat it like infrastructure.").lead(
        "SkiMasque is pre-1.0 and has not had an independent security review. It should not yet be the only control in front of production infrastructure. What follows states what is documented today; what is not built is marked Planned.",
    );

    let architecture = Section::new("Service architecture")
        .push(&Prose::new().p(
            "The control plane holds identity, the policy workflow, the fleet view and the audit trail. It is never on the traffic path: gateways enforce, and the control plane is authoritative only for desired state. In SkiMasque Cloud, SkiMasque operates the control plane; when you self-host, you do.",
        ))
        .push(&DocLinks::new(&[
            ("Control plane", doc("control-plane.md")),
            ("Architecture", doc("architecture.md")),
        ]));

    let availability = Section::new("Availability")
        .alt()
        .push(&Prose::new().p(
            "A gateway that loses its control plane keeps enforcing the last policy it cached. It reports itself degraded after a soft lease (15 minutes by default) and escalates after a hard TTL (30 minutes by default), and it never fails open. A control-plane outage is designed to degrade management, not enforcement.",
        ))
        .push(&planned(
            "no availability commitments yet",
            "Published availability targets and service-level agreements are planned.",
        ));

    let health = Section::new("Gateway health")
        .push(&Prose::new().p(
            "A control-plane gateway reports health and usage counters by heartbeat, every 30 seconds by default. The control plane shows the desired-versus-actual fleet view and marks a gateway offline when its heartbeats lapse. A gateway can also expose health, readiness and metrics endpoints for your own monitoring.",
        ))
        .push(&planned(
            "live status reporting is not available yet",
            "A public live status view of SkiMasque Cloud is planned.",
        ));

    let sessions = Section::new("Session handling")
        .alt()
        .push(&Prose::new().list(&[
            "Access is denied unless a policy rule explicitly allows it.",
            "Managed CI/developer credentials default to 15 minutes and are capped at 1 hour; standalone gateway credentials default to 1 hour. Clients refresh supported credentials. Agent sessions default to 30 minutes and are capped at 4 hours or a lower organisation limit. A copied bearer token can be used until expiry or applicable revocation; policy max_duration is not an enforced gateway timeout.",
            "Set agent-session TTL and per-kind organisation ceilings to bound credential lifetime. Policy max_duration currently provides metadata only.",
            "Credentials are bearer tokens: a token stolen from a running job grants that job's access for its lifetime, bounded by that job's policy.",
        ]))
        .push(&DocLinks::new(&[("Security model", doc("security.md"))]));

    let audit = Section::new("Auditability")
        .push(&Prose::new().p(
            "Each access decision is recorded, and every denial is logged with a reason. A gateway can write one JSON line per decision. A control-plane deployment ingests each gateway's hash-chained decision stream, rejects a break in the chain, and exposes fleet history and per-organization usage.",
        ))
        .push(&planned(
            "advanced audit and search are still being built",
            "Advanced audit search is planned.",
        ));

    let boundaries = Section::new("Operational boundaries")
        .alt()
        .push(&Prose::new().list(&[
            "The control plane is not on the traffic path and cannot open or redirect a tunnel.",
            "The gateway trusts the issuer's signing keys and the organization's public key. It does not trust the client's claimed application, the destination, or a credential it cannot verify.",
            "In the fully managed deployment, organizations share the SkiMasque endpoint but are isolated from each other's traffic and policy.",
            "Your firewall remains the outer boundary: it decides what a gateway can reach at all.",
        ]))
        .push(&DocLinks::new(&[("Threat model", doc("threat-model.md"))]));

    let reporting = Section::new("Security reporting")
        .push(&Prose::new().p(
            "Report suspected vulnerabilities privately, not in a public issue. The repository's security policy describes a GitHub private security advisory for this.",
        ))
        .push(&DocLinks::new(&[
            (
                "Report a vulnerability",
                format!("{REPO_URL}/security/advisories/new"),
            ),
            (
                "Read the security policy",
                format!("{REPO_URL}/blob/main/SECURITY.md"),
            ),
        ]))
        .push(&planned(
            "no dedicated mailbox yet",
            "A dedicated security reporting mailbox is planned.",
        ));

    let incident = Section::new("Incident response")
        .alt()
        .push(&Prose::new().p(
            "The security policy describes how reported vulnerabilities are handled: fixes are developed privately, disclosure is coordinated, and a release ships with a security advisory. It also lists the steps operators should take if active exploitation is suspected.",
        ))
        .push(&planned(
            "no operational process published yet",
            "A published incident-response process for SkiMasque Cloud is planned.",
        ));

    let data = Section::new("Data handling")
        .push(&Prose::new().list(&[
            "Traffic between a workload and a gateway is carried over QUIC with TLS 1.3, always on.",
            "Past the gateway, traffic is plaintext by nature of a proxy unless the destination speaks its own TLS: the private destination sees the gateway's address.",
            "The control plane stores identity, policy revisions, audit history and usage for your organization.",
        ]))
        .push(&planned(
            "no formal data-handling documents yet",
            "Formal data-handling documents, such as retention and subprocessor details, are planned.",
        ));

    let responsibility = Section::new("Customer responsibility")
        .alt()
        .push(&Prose::new().p(
            "SkiMasque does not bypass your network controls, and an over-broad policy silently widens access. The responsibility model:",
        ))
        .push(
            &Compare::new("What SkiMasque operates, and what the customer is responsible for.")
                .side(
                    "SKIMASQUE",
                    Tone::Structure,
                    &Prose::new().list(&[
                        "Control plane (in SkiMasque Cloud)",
                        "Platform",
                        "Managed gateways (in the fully managed deployment)",
                    ]),
                )
                .side(
                    "CUSTOMER",
                    Tone::Edge,
                    &Prose::new().list(&[
                        "Policies",
                        "Destinations",
                        "Firewall",
                        "Customer gateways",
                        "Identity configuration",
                    ]),
                ),
        )
        .push(&Prose::new().p(
            "When you self-host, you also operate the control plane and everything it depends on.",
        ))
        .push(&DocLinks::new(&[("Security model", doc("security.md"))]));

    Page {
        path: "trust/index.html",
        contents: SitePage::new(
            "../",
            "trust",
            "Trust · SkiMasque",
            "What SkiMasque documents today about architecture, availability, sessions, audit, reporting and responsibility, and what is still planned.",
        )
        .push(&hero)
        .push(&architecture)
        .push(&availability)
        .push(&health)
        .push(&sessions)
        .push(&audit)
        .push(&boundaries)
        .push(&reporting)
        .push(&incident)
        .push(&data)
        .push(&responsibility)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_has_ten_topics_a_two_sided_responsibility_model_and_planned_lines() {
        let p = page();
        assert_eq!(p.path, "trust/index.html");
        let s = &p.contents;
        assert_eq!(s.matches("<section class=\"v-section").count(), 10);
        for want in [
            "Service architecture",
            "Availability",
            "Gateway health",
            "Session handling",
            "Auditability",
            "Operational boundaries",
            "Security reporting",
            "Incident response",
            "Data handling",
            "Customer responsibility",
            "SKIMASQUE",
            "CUSTOMER",
            "Identity configuration",
            "PLANNED",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
        assert_eq!(s.matches("v-compare-side").count(), 2);
        let low = s.to_lowercase();
        for banned in ["99.9", "sla ", "soc 2", "iso 27001", "guarantee"] {
            assert!(!low.contains(banned), "unbacked claim {banned:?}");
        }
    }
}
