//! Deployment (`/deployment`): canonical spec §18. The run colours describe who
//! operates what; they say nothing about quality.

use askama::Template;

use super::doc;
use crate::diagrams::platform;
use crate::site::Page;
use crate::{
    Component, ConnKind, Connection, Flow, Hero, Node, NodeKind, Prose, Run, Section, SitePage,
    TrailMarker,
};

/// A single documentation link.
#[derive(Template)]
#[template(
    source = r#"<p class="v-cta-row"><a class="v-btn" href="{{ href }}">{{ label }}</a></p>"#,
    ext = "html"
)]
struct DocLink {
    label: String,
    href: String,
}
impl Component for DocLink {}

fn flow(caption: &str, nodes: [(NodeKind, &str); 4]) -> Flow {
    let mut it = nodes.into_iter();
    let (k, l) = it.next().expect("four nodes");
    let mut f = Flow::new(caption).then(&Node::new(k).label(l));
    for (k, l) in it {
        f = f.via(Connection::new(ConnKind::Active), &Node::new(k).label(l));
    }
    f
}

fn marker(run: Run) -> TrailMarker {
    TrailMarker::new(run.shape(), run.tone(), run.name())
}

pub fn page() -> Page {
    let hero = Hero::new("You choose where the network edge lives.")
        .lead("SkiMasque supports three deployment models.");
    let models = Section::new("Three deployment models").push(&platform::deployment_models());

    let green = Section::new("Green Run — SkiMasque Cloud").alt()
        .push(&marker(Run::Green))
        .push(
            &Prose::new()
                .p("You run your apps. We run SkiMasque.")
                .p("Live today with a free tier. Paid plans are not available yet.")
                .kv("Control plane", "SkiMasque")
                .kv("Gateway", "SkiMasque")
                .kv("Operations", "Minimal"),
        )
        .push(&flow(
            "An application reaches SkiMasque Cloud, then the SkiMasque gateway, then your network.",
            [
                (NodeKind::Application, "APPLICATION"),
                (NodeKind::ControlPlane, "SKIMASQUE CLOUD"),
                (NodeKind::Gateway, "SKIMASQUE GATEWAY"),
                (NodeKind::Network, "YOUR NETWORK"),
            ],
        ))
        .push(
            &Prose::new().p("Best for:").list(&[
                "getting started quickly",
                "teams that don't want to operate networking infrastructure",
                "standard deployments",
            ]),
        );

    let blue = Section::new("Blue Run — Your Gateway")
        .push(&marker(Run::Blue))
        .push(
            &Prose::new()
                .p("We run the control plane. You run the network edge.")
                .kv("Control plane", "SkiMasque")
                .kv("Gateway", "Customer")
                .p("The gateway lives inside the customer's network."),
        )
        .push(&flow(
            "An application reaches SkiMasque Cloud, then your gateway, then your VPC.",
            [
                (NodeKind::Application, "APPLICATION"),
                (NodeKind::ControlPlane, "SKIMASQUE CLOUD"),
                (NodeKind::Gateway, "YOUR GATEWAY"),
                (NodeKind::Network, "YOUR VPC"),
            ],
        ))
        .push(
            &Prose::new()
                .p("This is useful when customers need:")
                .list(&[
                    "private routing",
                    "internal firewall control",
                    "customer-owned egress IP",
                    "traffic to remain inside their infrastructure",
                ]),
        );

    let black = Section::new("Black Run — Self-hosted").alt()
        .push(&marker(Run::Black))
        .push(
            &Prose::new()
                .p("You run everything.")
                .kv("Control plane", "Customer")
                .kv("Gateway", "Customer")
                .kv("Operations", "Customer"),
        )
        .push(&flow(
            "An application reaches the customer's control plane, then the customer's gateway, then the customer's network.",
            [
                (NodeKind::Application, "APPLICATION"),
                (NodeKind::ControlPlane, "CUSTOMER CONTROL PLANE"),
                (NodeKind::Gateway, "CUSTOMER GATEWAY"),
                (NodeKind::Network, "CUSTOMER NETWORK"),
            ],
        ))
        .push(&Prose::new().p("Suitable for customers requiring full infrastructure ownership."))
        .push(
            &Prose::new()
                .p(
                    "There is no open-source control plane. To self-host, you implement the documented control protocol or license SkiMasque's control-plane distribution; expect real engineering work, not a config switch.",
                )
        )
        .push(&DocLink {
            label: "Read the self-hosting guide".into(),
            href: doc("self-hosting.md"),
        });

    let positioning = Section::new("Important positioning").push(
        &Prose::new()
            .p("The run colors represent operational responsibility, not product quality.")
            .kv("GREEN", "SkiMasque operates more.")
            .kv("BLUE", "Shared responsibility.")
            .kv("BLACK", "Customer operates more."),
    );

    Page {
        path: "deployment/index.html",
        contents: SitePage::new(
            "../",
            "deployment",
            "Deployment · SkiMasque",
            "SkiMasque supports three deployment models: SkiMasque Cloud, your own gateway, or fully self-hosted. You choose where the network edge lives.",
        )
        .push(&hero)
        .push(&models)
        .push(&green)
        .push(&blue)
        .push(&black)
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
    fn deployment_describes_three_runs_by_colour_and_technical_name() {
        let p = page();
        assert_eq!(p.path, "deployment/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        let main = &s[s.find("<main").unwrap()..s.find("</main>").unwrap()];
        for want in [
            "You choose where the network edge lives.",
            "Green Run",
            "Blue Run",
            "Black Run",
            "SkiMasque Cloud",
            "Your Gateway",
            "Self-hosted",
            "customer-owned egress IP",
            "operational responsibility, not product quality",
            "There is no open-source control plane.",
            "license SkiMasque's control-plane distribution",
            "not a config switch",
            "docs/self-hosting.md",
            "CUSTOMER CONTROL PLANE",
            "YOUR VPC",
        ] {
            assert!(main.contains(want), "missing {want:?}");
        }
        assert_eq!(
            main.matches("PLANNED").count(),
            1,
            "only paid plans are planned, once, in the run card"
        );
        assert!(main.contains("Free tier") && main.contains("paid plans"));
        assert!(s.contains(r#"aria-current="page""#));
    }
}
