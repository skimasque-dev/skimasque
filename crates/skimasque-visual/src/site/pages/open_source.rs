//! Open source (`/open-source`): canonical spec §24. What is open, and under
//! which licence, comes from `README.md`, `docs/architecture/`,
//! `docs/self-hosting/` and the workspace `Cargo.toml` (MIT OR Apache-2.0).
//! There is no open-source control-plane server, and this page never implies one.

use super::{doc, DocLinks};
use crate::site::{Page, REPO_URL};
use crate::{
    Component, ConnKind, Connection, Flow, Hero, Node, NodeKind, PlannedBlock, Prose, Section,
    SitePage,
};

fn open_flow() -> Flow {
    Flow::new("Open source components provide the core networking, which you can self-host.")
        .then(&Node::new(NodeKind::Service).label("OPEN SOURCE"))
        .via(
            Connection::new(ConnKind::Active),
            &Node::new(NodeKind::Network).label("CORE NETWORKING"),
        )
        .via(
            Connection::new(ConnKind::Active),
            &Node::new(NodeKind::Gateway).label("SELF-HOSTED"),
        )
}

pub fn page() -> Page {
    let hero = Hero::new("Network access infrastructure you can inspect.").lead(
        "SkiMasque's open-source components provide transparency and technical credibility while the hosted service adds operational capabilities.",
    );

    let architecture = Section::new("Open architecture")
        .push(&Prose::new().list(&[
            "Control plane: the control protocol a gateway speaks to a control plane is open and documented. The control-plane server itself is part of SkiMasque Cloud and is not open source; there is no open-source control-plane server.",
            "Gateway: the gateway, skimasque-server, is open source and does full policy enforcement, deny by default and the SSRF floor on its own.",
            "Protocol implementation: the MASQUE transport stack, the policy engine, OIDC verification and the control protocol are open source.",
            "Configuration: policies are files (TOML or YAML) that the gateway re-reads in place; the gateway is configured with command-line flags and environment variables.",
            "Deployment: run the gateway with Docker, systemd, a Helm chart or a Terraform starting point from the repository's deploy directory.",
            "Extension points: the documented control protocol lets you implement your own control plane; the gateway does not change.",
        ]))
        .push(&Prose::new().p(
            "The open-source gateway is not crippled: core networking is not withheld to create a distinction. Licensed MIT OR Apache-2.0, at your option.",
        ))
        .push(&DocLinks::new(&[
            ("Source on GitHub", REPO_URL.to_owned()),
            (
                "skimasque-protocol",
                "https://crates.io/crates/skimasque-protocol".to_owned(),
            ),
            (
                "skimasque-policy",
                "https://crates.io/crates/skimasque-policy".to_owned(),
            ),
            (
                "skimasque-core",
                "https://crates.io/crates/skimasque-core".to_owned(),
            ),
            ("Read the architecture", doc("architecture.md")),
        ]));

    let cloud = Section::new("Cloud vs self-hosted")
        .alt()
        .push(&open_flow())
        .push(&Prose::new().sub("The hosted service adds").list(&[
            "Managed infrastructure: SkiMasque Cloud operates the control plane",
            "Hosted dashboard",
            "Organization management",
            "Managed gateways, in the fully managed deployment",
        ]))
        .push(&PlannedBlock::new(
            "hosted-service additions still being built",
            &Prose::new().list(&[
                "Billing",
                "Advanced audit and search",
                "Enterprise integrations",
                "Multi-region orchestration",
                "Support",
            ]),
        ));

    let positioning = Section::new("Open protocol, managed platform")
        .push(&Prose::new().quote("Run SkiMasque yourself, or let us operate it for you."))
        .push(&Prose::new().p(
            "Running your own control plane is the advanced path: you implement the documented control protocol or license SkiMasque's control-plane distribution.",
        ))
        .push(&DocLinks::new(&[(
            "Read the self-hosting guide",
            doc("self-hosting.md"),
        )]));

    Page {
        path: "open-source/index.html",
        contents: SitePage::new(
            "../",
            "open-source",
            "Open Source · SkiMasque",
            "SkiMasque's gateway, client, CLI, policy engine and control protocol are open source under MIT OR Apache-2.0; SkiMasque Cloud adds hosted operations.",
        )
        .push(&hero)
        .push(&architecture)
        .push(&cloud)
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
    fn open_source_names_the_licence_crates_and_one_planned_block() {
        let p = page();
        assert_eq!(p.path, "open-source/index.html");
        let s = &p.contents;
        for want in [
            "Network access infrastructure you can inspect.",
            "MIT OR Apache-2.0",
            "https://crates.io/crates/skimasque-protocol",
            "https://crates.io/crates/skimasque-policy",
            "https://crates.io/crates/skimasque-core",
            "docs/architecture/",
            "OPEN SOURCE",
            "CORE NETWORKING",
            "SELF-HOSTED",
            "hosted-service additions still being built",
            "Run SkiMasque yourself, or let us operate it for you.",
            "there is no open-source control-plane server",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
        assert_eq!(s.matches("class=\"v-planned-block\"").count(), 1);
        assert!(s.contains("MIT"));
    }
}
