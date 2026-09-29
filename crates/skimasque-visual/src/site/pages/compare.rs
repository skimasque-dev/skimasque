//! How SkiMasque compares (`/compare`): canonical spec §17. Descriptive, never
//! a claim that one approach is universally better.

use crate::diagrams::public;
use crate::site::Page;
use crate::{ComparisonTable, Component, Hero, Prose, Section, SitePage};

pub fn page() -> Page {
    let hero = Hero::new("Network access doesn't have to mean network membership.")
        .lead("Different tools solve different problems.")
        .lead(
            "SkiMasque is specifically designed around identity-aware network access for \
             workloads and developer commands.",
        );

    let matrix = Section::new("Comparison matrix").push(
        &ComparisonTable::new(&["Approach", "Primary abstraction", "Typical access model"])
            .row(&["Traditional VPN", "Network", "Join a private network"])
            .row(&["Mesh VPN", "Machines/networks", "Connect trusted nodes"])
            .row(&[
                "ZTNA",
                "Applications/users",
                "Identity-based application access",
            ])
            .row(&[
                "Bastion",
                "Infrastructure",
                "Connect through a controlled host",
            ])
            .row(&["PAM", "Privileged access", "Manage privileged sessions"])
            .row(&[
                "SkiMasque",
                "Workload network access",
                "Temporary policy-controlled network capability",
            ])
            .highlight_last(),
    );

    let vpn = Section::new("VPN")
        .alt()
        .push(&Prose::new().sub("Network first. Policy afterward."))
        .push(
            &Prose::new()
                .p("VPNs are designed around network connectivity.")
                .p(
                    "A user or machine joins a network and can then reach resources permitted by \
                     network configuration.",
                )
                .p("SkiMasque starts with an access request."),
        )
        .push(&public::traditional_vs_skimasque());

    let mesh = Section::new("Mesh VPN").push(
        &Prose::new()
            .p(
                "Mesh VPNs are useful when the fundamental requirement is connecting machines or \
                 networks.",
            )
            .p(
                "SkiMasque instead focuses on making the workload and access request the unit of \
                 authorization.",
            ),
    );

    let ztna = Section::new("ZTNA").alt().push(
        &Prose::new()
            .p("ZTNA systems commonly focus on identity-aware access to applications.")
            .p(
                "SkiMasque is designed around network-level connectivity for developer and \
                 workload workflows where arbitrary protocols may matter.",
            ),
    );

    let bastions = Section::new("Bastions").push(
        &Prose::new()
            .p("A bastion provides a controlled entry point into infrastructure.")
            .p(
                "SkiMasque can provide temporary network connectivity without requiring every \
                 workflow to become an interactive bastion session.",
            ),
    );

    let runners = Section::new("Self-hosted runners").alt().push(
        &Prose::new()
            .p("Putting CI runners inside a private network is a straightforward architecture.")
            .p("But it makes network placement part of the security model.")
            .p(
                "SkiMasque allows workloads to remain outside the private network while \
                 receiving narrowly scoped access.",
            ),
    );

    let closing = Section::new("What should the unit of network access be?").push(
        &Prose::new()
            .p("The important question is:")
            .quote("What should the unit of network access be?")
            .p("For SkiMasque:")
            .quote("The workload and its request."),
    );

    Page {
        path: "compare/index.html",
        contents: SitePage::new(
            "../",
            "compare",
            "How SkiMasque compares · SkiMasque",
            "VPNs, mesh VPNs, ZTNA, bastions and PAM solve different problems. SkiMasque makes the workload and its request the unit of network access.",
        )
        .push(&hero)
        .push(&matrix)
        .push(&vpn)
        .push(&mesh)
        .push(&ztna)
        .push(&bastions)
        .push(&runners)
        .push(&closing)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_is_a_descriptive_six_row_table_with_skimasque_highlighted() {
        let p = page();
        assert_eq!(p.path, "compare/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        assert!(s.contains("Network access doesn't have to mean network membership."));
        let main = &s[s.find("<main").unwrap()..s.find("</main>").unwrap()];
        let body = &main[main.find("<tbody>").unwrap()..main.find("</tbody>").unwrap()];
        assert_eq!(body.matches("<tr").count(), 6);
        assert_eq!(body.matches("v-row-highlight").count(), 1);
        let hl = &body[body.find("v-row-highlight").unwrap()..];
        assert!(hl.contains("SkiMasque") && hl.contains("Temporary policy-controlled"));
        for row in [
            "Traditional VPN",
            "Mesh VPN",
            "ZTNA",
            "Bastion",
            "PAM",
            "Machines/networks",
        ] {
            assert!(body.contains(row), "{row}");
        }
        for h in [
            "Network first. Policy afterward.",
            "Self-hosted runners",
            "Bastions",
        ] {
            assert!(s.contains(h), "{h}");
        }
        assert!(s.contains("v-compare-side"), "VPN versus SkiMasque diagram");
        assert!(s.contains("The workload and its request."));
        assert!(!s.contains("This table should remain descriptive"));
        assert!(!main.contains("PLANNED"));
    }
}
