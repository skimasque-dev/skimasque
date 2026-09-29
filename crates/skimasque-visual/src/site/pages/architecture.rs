//! Architecture (`/architecture`): canonical spec §21.

use super::{doc, DocLinks};
use crate::diagrams::platform;
use crate::site::Page;
use crate::{Component, Hero, Prose, Section, SitePage};

pub fn page() -> Page {
    let hero = Hero::new("Identity-aware access, built around a real network protocol.")
        .lead("SkiMasque separates the control plane from the network data path.");

    let overview = Section::new("High-level architecture")
        .push(&platform::control_data_plane())
        .push(&platform::architecture());

    let control = Section::new("Control plane").alt().push(
        &Prose::new()
            .p("Responsible for:")
            .list(&[
                "identities",
                "authentication",
                "policies",
                "authorization",
                "session issuance",
                "gateway management",
                "audit records",
                "organization configuration",
            ])
            .p("In SkiMasque Cloud, SkiMasque operates the control plane. Either way it is not on the traffic path."),
    );

    let data = Section::new("Data plane").push(
        &Prose::new()
            .p("Responsible for:")
            .list(&[
                "network sessions",
                "traffic forwarding",
                "gateway connectivity",
                "protocol handling",
            "In SkiMasque Cloud, SkiMasque operates the control plane",
            "while the control plane is unreachable",
            ])
            .p(
                "The gateway enforces policy itself. If the control plane is unreachable it keeps enforcing its cached policy while the control plane is unreachable.",
            ),
    );

    let why = Section::new("Why separation matters")
        .alt()
        .push(
            &Prose::new()
                .p("The control plane decides:")
                .quote("Should this session exist?"),
        )
        .push(
            &Prose::new()
                .p("The data plane handles:")
                .quote("How does the traffic move?"),
        )
        .push(
            &Prose::new()
                .p("A control-plane outage is designed to degrade management, not enforcement."),
        )
        .push(&DocLinks::new(&[(
            "Read the architecture document",
            doc("architecture.md"),
        )]));

    Page {
        path: "architecture/index.html",
        contents: SitePage::new(
            "../",
            "architecture",
            "Architecture · SkiMasque",
            "SkiMasque separates the control plane, which decides whether a session should exist, from the data plane, which carries the traffic.",
        )
        .push(&hero)
        .push(&overview)
        .push(&control)
        .push(&data)
        .push(&why)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn architecture_separates_control_and_data_plane() {
        let p = page();
        assert_eq!(p.path, "architecture/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        for want in [
            "Identity-aware access, built around a real network protocol.",
            "CONTROL PLANE",
            "DATA PLANE",
            "Control plane",
            "Data plane",
            "session issuance",
            "protocol handling",
            "In SkiMasque Cloud, SkiMasque operates the control plane",
            "while the control plane is unreachable",
            "Should this session exist?",
            "How does the traffic move?",
            "docs/architecture.md",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
    }
}
