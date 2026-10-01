//! Gateways (`/gateways`): canonical spec Ã‚Â§19.

use crate::diagrams::{platform, public};
use crate::site::Page;
use crate::{
    Component, ConnKind, Connection, Flow, Hero, Node, NodeKind, PlannedBlock, Prose, Section,
    SitePage,
};

fn basic_flow() -> Flow {
    let conn = || Connection::new(ConnKind::Active);
    Flow::new(
        "A workload reaches the control plane, which authorizes a session through the gateway \
         into the customer network and on to the destination.",
    )
    .then(&Node::new(NodeKind::Workload).label("WORKLOAD"))
    .via(
        conn(),
        &Node::new(NodeKind::ControlPlane).label("SKIMASQUE CONTROL PLANE"),
    )
    .via(conn(), &Node::new(NodeKind::Gateway).label("GATEWAY"))
    .via(
        conn(),
        &Node::new(NodeKind::Network).label("CUSTOMER NETWORK"),
    )
    .via(conn(), &Node::new(NodeKind::Service).label("DESTINATION"))
}

pub fn page() -> Page {
    let hero = Hero::new("Put the network edge where your infrastructure lives.")
        .lead(
            "The gateway is the point where SkiMasque-controlled sessions enter the customer's \
             network.",
        )
        .aside(&basic_flow());

    let operated = Section::new("Customer-operated gateway")
        .push(&public::customer_vpc())
        .push(
            &Prose::new()
                .p("The customer can configure their firewall to permit the gateway's traffic."),
        );

    let managed_egress = Prose::new().p(
        "A dedicated address that SkiMasque reserves for your organization, so you can allowlist \
         exactly one source address without running a gateway yourself.",
    );
    let egress = Section::new("Egress IP")
        .alt()
        .push(&Prose::new().p(
            "A customer-operated gateway uses the egress address configured in your network. \
             Configure a stable address or NAT mapping if your infrastructure relies on source-IP allowlists.",
        ))
        .push(&PlannedBlock::new(
            "SkiMasque-provided dedicated egress IPs",
            &managed_egress,
        ));

    let multiple = Section::new("Multiple gateways").push(&platform::multiple_gateways());

    Page {
        path: "gateways/index.html",
        contents: SitePage::new(
            "../",
            "gateways",
            "Gateways Ã‚Â· SkiMasque",
            "The gateway is the point where SkiMasque-controlled sessions enter your network. Run it where your infrastructure lives.",
        )
        .push(&hero)
        .push(&operated)
        .push(&egress)
        .push(&multiple)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateways_follow_the_spec_and_mark_dedicated_egress_ips_planned() {
        let p = page();
        assert_eq!(p.path, "gateways/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        let main = &s[s.find("<main").unwrap()..s.find("</main>").unwrap()];
        for want in [
            "Put the network edge where your infrastructure lives.",
            "WORKLOAD",
            "DESTINATION",
            "YOUR VPC",
            "Customer-operated gateway",
            "permit the gateway's traffic",
            "egress address configured in your network",
            "us-west",
        ] {
            assert!(main.contains(want), "missing {want:?}");
        }
        let planned = &main[main.find("v-planned-block").unwrap()..];
        assert!(planned.contains("PLANNED") && planned.contains("dedicated egress IPs"));
        assert_eq!(main.matches("PLANNED").count(), 1);
    }
}
