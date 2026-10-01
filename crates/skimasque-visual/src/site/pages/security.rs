//! Security (`/security`): canonical spec §20. Every claim here is backed by
//! `docs/security/` or `docs/threat-model/`; the voice is "is designed to",
//! never an absolute guarantee.

use super::{doc, DocLinks};
use crate::diagrams::platform;
use crate::site::Page;
use crate::{
    Component, ConnKind, Connection, FeatureGrid, Flow, Hero, Node, NodeKind, Prose, Section,
    SitePage,
};

fn default_deny_flow() -> Flow {
    Flow::new("A request that matches no allow rule is denied.")
        .then(&Node::new(NodeKind::Workload).label("REQUEST"))
        .via(
            Connection::new(ConnKind::Normal),
            &Node::new(NodeKind::Policy).label("NO MATCH"),
        )
        .via(
            Connection::new(ConnKind::Denied),
            &Node::new(NodeKind::Deny).label("DENY"),
        )
}

fn firewall_flow() -> Flow {
    Flow::new(
        "Traffic leaves the SkiMasque gateway, crosses the customer's firewall, then reaches the private service.",
    )
    .then(&Node::new(NodeKind::Gateway).label("SKIMASQUE GATEWAY"))
    .via(
        Connection::new(ConnKind::Active),
        &Node::new(NodeKind::Firewall).label("CUSTOMER FIREWALL"),
    )
    .via(
        Connection::new(ConnKind::Active),
        &Node::new(NodeKind::Service).label("PRIVATE SERVICE"),
    )
}

pub fn page() -> Page {
    let hero = Hero::new("Security starts with reducing what needs to be trusted.")
        .lead("SkiMasque's security model is based on minimizing standing network access.");

    let layers = Section::new("Security layers").push(&platform::security_layers());

    let standing = Section::new("No standing access")
        .alt()
        .push(&Prose::new().p(
            "Instead of a permanent credential that opens the network, an identity is checked against policy, receives a temporary session, and the session expires.",
        ))
        .push(&platform::no_standing_access());

    let deny = Section::new("Default deny")
        .push(&Prose::new().quote("Only explicit matching access grants a session."))
        .push(&default_deny_flow());

    let least = Section::new("Least privilege").alt().push(
        &Prose::new()
            .p("Access can be constrained by:")
            .list(&[
                "identity",
                "application",
                "destination",
                "protocol",
                "port",
                "duration",
                "bandwidth",
            ])
            .p(
                "Bandwidth and connection limits apply to a whole policy, not to each session. The application name is session context, not proof: the gateway matches on it but does not treat it as authenticated, so policies are best written with the destination as the real constraint.",
            ),
    );

    let enforce = Section::new("How the gateway enforces it")
        .push(&Prose::new().p(
            "A valid tunnel credential alone does not open a tunnel. The gateway checks identity, application, destination, policy, and expiry itself, and all five must resolve to an allow.",
        ))
        .push(
            &FeatureGrid::new()
                .feature(
                    "Fail closed",
                    "Where authentication or authorization can't be established, no tunnel is created. A tunnel without a verifiable credential is refused, and a policy that fails to parse is ignored in favour of the last good one.",
                )
                .feature(
                    "Checked in the gateway",
                    "Identity, application, destination, policy and expiry are evaluated by the gateway, not assumed because a control plane said so.",
                )
                .feature(
                    "Destination address checks",
                    "Loopback, private, CGNAT, link-local (including the cloud metadata address) and multicast ranges are refused on the resolved address, so a hostname that resolves to one is still refused, unless a range was explicitly opted in.",
                )
                .feature(
                    "Keeps working during an outage",
                    "A control-plane outage is designed to degrade management, not enforcement. The gateway keeps enforcing its cached policy while the control plane is unreachable, and verifies credentials locally.",
                )
                .feature(
                    "Short-lived credentials",
                    "Managed CI/developer credentials default to 15 minutes and are capped at 1 hour; standalone gateway credentials default to 1 hour. Clients refresh supported credentials. Agent sessions default to 30 minutes and are capped at 4 hours or a lower organisation limit. A copied bearer token can be used until expiry or applicable revocation; policy max_duration is not an enforced gateway timeout.",
                ),
        )
        .push(&Prose::new().p(
            "SkiMasque is pre-1.0 and has not had an independent security review. It should not yet be the only control in front of production infrastructure.",
        ))
        .push(&DocLinks::new(&[
            ("Read the security model", doc("security.md")),
            ("Read the threat model", doc("threat-model.md")),
        ]));

    let firewall = Section::new("Customer firewall")
        .alt()
        .push(&Prose::new().p("SkiMasque does not eliminate the customer's network boundary."))
        .push(&Prose::new().p("For customer-operated gateways:"))
        .push(&firewall_flow())
        .push(&Prose::new().p(
            "Customers can continue to enforce their own network-level controls. The firewall decides what the gateway can reach at all; SkiMasque decides who may use that.",
        ));

    let compart = Section::new("Compartmentalisation")
        .push(&Prose::new().p(
            "Each workload is granted the destinations its policy names. Everything else is denied.",
        ))
        .push(&platform::compartmentalisation());

    let audit = Section::new("Auditability")
        .alt()
        .push(&Prose::new().p("Every access decision is designed to carry enough context to answer:"))
        .push(
            &Prose::new()
                .kv("WHO", "the verified identity")
                .kv("WHAT", "the application")
                .kv("WHERE", "the destination")
                .kv("WHEN", "the time of the decision")
                .kv("WHICH POLICY", "the policy and rule that decided")
                .kv("WHICH GATEWAY", "the gateway that enforced it (identified by the stream it ships on)")
                .kv("RESULT", "allow or deny, with a reason"),
        )
        .push(&Prose::new().p(
            "A gateway can write one JSON line per decision. A control-plane deployment ingests each gateway's hash-chained stream and rejects a break in the chain.",
        ));

    Page {
        path: "security/index.html",
        contents: SitePage::new(
            "../",
            "security",
            "Security · SkiMasque",
            "SkiMasque's security model minimizes standing network access: identity, policy, short-lived sessions and a gateway that enforces deny by default.",
        )
        .push(&hero)
        .push(&layers)
        .push(&standing)
        .push(&deny)
        .push(&least)
        .push(&enforce)
        .push(&firewall)
        .push(&compart)
        .push(&audit)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_follows_the_spec_with_designed_to_voice() {
        let p = page();
        assert_eq!(p.path, "security/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        for want in [
            "Security starts with reducing what needs to be trusted.",
            "IDENTITY",
            "POLICY",
            "SESSION",
            "GATEWAY",
            "NETWORK",
            "No standing access",
            "Default deny",
            "Only explicit matching access grants a session.",
            "Least privilege",
            "Customer firewall",
            "CUSTOMER FIREWALL",
            "Compartmentalisation",
            "Auditability",
            "WHICH GATEWAY",
            "identified by the stream it ships on",
            "while the control plane is unreachable",
            "How the gateway enforces it",
            "docs/security/",
            "docs/threat-model/",
            "has not had an independent security review",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
        let low = s.to_lowercase();
        for banned in ["guarantee", "prevents", "cannot", "zero trust"] {
            assert!(!low.contains(banned), "absolute wording {banned:?}");
        }
        assert!(
            !low.contains("egress region"),
            "egress region is not a documented constraint"
        );
    }
}
