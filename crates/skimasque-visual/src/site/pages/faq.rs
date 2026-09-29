//! FAQ (`/faq`): canonical spec §27. The developer answer avoids the literal
//! command wrapper; that command appears only in the planned block below.

use crate::site::Page;
use crate::{
    Component, ConnKind, Connection, Faq, Flow, Hero, Node, NodeKind, PlannedBlock, Prose, Section,
    SitePage,
};

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
                "Not conceptually.",
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
                "It is not intended to imply that every VPN use case should be replaced.",
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
                "Sessions are designed to be short-lived and policy-controlled.",
            ],
        )
        .item(
            "Can developers use it?",
            &[
                "Yes.",
                "The CLI supports local policy work and opening a tunnel with skimasque connect; a command wrapper is planned.",
            ],
        )
        .item(
            "Can CI use it?",
            &["Yes.", "GitHub Actions is an initial target use case."],
        );

    let faq = Section::new("Questions")
        .push(&first)
        .push(&default_deny_flow())
        .push(&second)
        .push(&PlannedBlock::new(
            "a command wrapper for developers",
            &Prose::new().p("skimasque exec --policy production -- terraform plan"),
        ));

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
    fn ten_questions_a_deny_flow_and_the_wrapper_only_in_a_planned_block() {
        let p = page();
        assert_eq!(p.path, "faq/index.html");
        let s = &p.contents;
        assert_eq!(s.matches("<details class=\"v-faq-item\"").count(), 10);
        for want in ["Is SkiMasque a VPN?", "NO MATCH", "DENY", "Can CI use it?"] {
            assert!(s.contains(want), "missing {want:?}");
        }
        assert_eq!(s.matches("skimasque exec").count(), 1);
        let planned = s.find("v-planned-block").expect("planned block");
        assert!(s.find("skimasque exec").unwrap() > planned);
        // The deny flow sits between the default-deny answer and the next question.
        let deny = s.find("The request is denied.").unwrap();
        let flow = s.find("NO MATCH").unwrap();
        let next = s.find("Does access expire?").unwrap();
        assert!(deny < flow && flow < next);
    }
}
