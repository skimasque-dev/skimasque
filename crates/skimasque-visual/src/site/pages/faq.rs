//! FAQ (`/faq`): canonical spec §27. The developer answer names the real
//! commands, including `skimasque exec`.

use crate::site::Page;
use crate::{Component, ConnKind, Connection, Faq, Flow, Hero, Node, NodeKind, Section, SitePage};

fn default_deny_flow() -> Flow {
    Flow::new("A request that matches no policy is denied.")
        .then(&Node::new(NodeKind::Policy).label("NO MATCH"))
        .via(
            Connection::new(ConnKind::Denied),
            &Node::new(NodeKind::Deny).label("DENY"),
        )
}

pub fn page() -> Page {
    let hero = Hero::new("Frequently asked questions");

    let first = Faq::new()
        .item(
            "Is SkiMasque a VPN?",
            &[
                "SkiMasque grants access to specific destinations for a bounded session.",
                "SkiMasque provides temporary, identity-aware network access rather than making a user or workload a permanent member of a private network.",
            ],
        )
        .item(
            "Does traffic go through a SkiMasque gateway?",
            &[
                "Yes.",
                "The gateway is the network edge through which the authorized session reaches the destination.",
            ],
        )
        .item(
            "Can the gateway run in my VPC?",
            &[
                "Yes.",
                "The customer-operated gateway model places the gateway inside the customer's infrastructure.",
            ],
        )
        .item(
            "Do I need to change my firewall?",
            &[
                "Customers still control their own network boundaries.",
                "A customer-operated gateway can be allowed through existing firewall controls using its network identity/source address.",
            ],
        )
        .item(
            "Does SkiMasque replace IAM?",
            &[
                "No.",
                "SkiMasque complements identity systems by using authenticated workload identity as an input to network authorization.",
            ],
        )
        .item(
            "Does SkiMasque replace a VPN?",
            &[
                "It can address some use cases commonly handled with VPNs, particularly temporary workload and developer access.",
                "Whether it fits depends on the destinations, protocols and identities your workflow needs.",
            ],
        )
        .item(
            "What happens if no policy matches?",
            &["The request is denied."],
        );

    let second = Faq::new()
        .item(
            "Does access expire?",
            &[
                "Yes.",
                "Policy sets the session duration. The command wrapper also closes its local tunnels when the command exits.",
            ],
        )
        .item(
            "Can developers use it?",
            &[
                "Yes.",
                "The CLI supports local policy work, skimasque connect for a single tunnel, and skimasque exec --policy production -- terraform plan to run a command with access.",
            ],
        )
        .item(
            "Can CI use it?",
            &["Yes.", "The GitHub Action exchanges the job’s OIDC token for a short-lived credential and starts a local proxy for tools that honour ALL_PROXY."],
        );

    let faq = Section::new("Questions")
        .push(&first)
        .push(&default_deny_flow())
        .push(&second);

    Page {
        path: "faq/index.html",
        contents: SitePage::new(
            "../",
            "faq",
            "FAQ · SkiMasque",
            "Answers to common questions about SkiMasque: how it differs from a VPN, gateways, firewalls, IAM, default deny, expiry, developers and CI.",
        )
        .push(&hero)
        .push(&faq)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_questions_a_deny_flow_and_exec_in_the_developer_answer() {
        let p = page();
        assert_eq!(p.path, "faq/index.html");
        let s = &p.contents;
        assert_eq!(s.matches("<details class=\"v-faq-item\"").count(), 10);
        for want in ["Is SkiMasque a VPN?", "NO MATCH", "DENY", "Can CI use it?"] {
            assert!(s.contains(want), "missing {want:?}");
        }
        assert!(
            !s.contains("v-planned-block"),
            "nothing on the FAQ is planned now"
        );
        assert!(s.contains("skimasque exec --policy production -- terraform plan"));
        // The deny flow sits between the default-deny answer and the next question.
        let deny = s.find("The request is denied.").unwrap();
        let flow = s.find("NO MATCH").unwrap();
        let next = s.find("Does access expire?").unwrap();
        assert!(deny < flow && flow < next);
    }
}
