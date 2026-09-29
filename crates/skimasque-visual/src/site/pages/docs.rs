//! Documentation index (`/docs`): canonical spec §26, amended. The page links
//! only to guides that exist in the repository's `docs/`; a test checks that.

use super::{doc, DocLinks};
use crate::site::Page;
use crate::{Component, Hero, PlannedBlock, Prose, Section, SitePage};

/// `(section, [(link label, file in docs/)])`, mirroring the canonical nav groups.
const INDEX: &[(&str, &[(&str, &str)])] = &[
    (
        "Getting Started",
        &[("Getting started", "getting-started.md")],
    ),
    (
        "Concepts",
        &[
            ("Policies", "policies.md"),
            ("Control plane", "control-plane.md"),
            ("Gateways", "gateways.md"),
            ("Control protocol", "protocol.md"),
        ],
    ),
    ("CI/CD", &[("GitHub Actions", "github-actions.md")]),
    ("Developers", &[("CLI", "cli.md")]),
    (
        "Deployment",
        &[
            ("Deployment modes", "deployment-modes.md"),
            ("Gateways", "gateways.md"),
            ("Self-hosting", "self-hosting.md"),
        ],
    ),
    (
        "Security",
        &[
            ("Security model", "security.md"),
            ("Threat model", "threat-model.md"),
        ],
    ),
    (
        "Reference",
        &[
            ("CLI", "cli.md"),
            ("Configuration", "configuration.md"),
            ("Policies", "policies.md"),
        ],
    ),
    (
        "Architecture",
        &[
            ("Architecture", "architecture.md"),
            ("Control protocol", "protocol.md"),
        ],
    ),
    (
        "Contributing",
        &[
            ("Development", "development.md"),
            ("Troubleshooting", "troubleshooting.md"),
        ],
    ),
];

pub fn page() -> Page {
    let hero = Hero::new("Documentation")
        .lead("Guides and reference for SkiMasque, kept in the repository alongside the code.");

    let mut page = SitePage::new(
        "../",
        "docs",
        "Documentation · SkiMasque",
        "Documentation index for SkiMasque: getting started, concepts, CI/CD, deployment, security, reference and architecture.",
    )
    .push(&hero);

    for (i, (title, links)) in INDEX.iter().enumerate() {
        let links: Vec<(&str, String)> = links.iter().map(|(l, f)| (*l, doc(f))).collect();
        let mut s = Section::new(*title).push(&DocLinks::new(&links));
        if i % 2 == 1 {
            s = s.alt();
        }
        page = page.push(&s);
    }

    let more = Section::new("More guides").push(&PlannedBlock::new(
        "guides that are not written yet",
        &Prose::new().p("More guides are planned."),
    ));
    page = page.push(&more);

    Page {
        path: "docs/index.html",
        contents: page.html().as_str().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_linked_guide_exists_in_the_docs_directory() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let mut n = 0;
        for (_, links) in INDEX {
            for (_, file) in *links {
                assert!(dir.join(file).is_file(), "docs/{file} does not exist");
                n += 1;
            }
        }
        assert!(n >= 10);
        let p = page();
        assert_eq!(p.path, "docs/index.html");
        for (_, links) in INDEX {
            for (_, file) in *links {
                assert!(p.contents.contains(&doc(file)), "{file} not linked");
            }
        }
    }

    #[test]
    fn the_index_says_more_guides_are_planned() {
        let s = page().contents;
        assert!(s.contains("More guides are planned.") && s.contains("PLANNED"));
    }
}
