//! Identities (`/identities`): canonical spec §13.

use crate::diagrams::public;
use crate::site::Page;
use crate::{Compare, Component, FeatureGrid, Hero, PolicySummary, Prose, Section, SitePage, Tone};

pub fn page() -> Page {
    let hero = Hero::new("Give network access an identity.").aside(
        &Prose::new()
            .p("Traditional network access often starts with:")
            .quote("“Where are you connecting from?”")
            .p("SkiMasque starts with:")
            .quote("“Who or what is requesting access?”"),
    );

    let types = Section::new("Identity and request context").push(
        &FeatureGrid::new()
            .feature("Developer", "developer: alice")
            .feature("Repository", "repository: acme/widget")
            .feature("Workflow", "workflow: deploy-production")
            .feature("Ref", "ref: main")
            .feature("Application", "application: terraform")
            .feature(
                "Workload",
                "repository: acme/widget · workflow: deploy-production · ref: main · application: terraform",
            ),
    );

    let not_authz = Section::new("Identity is not authorization")
        .alt()
        .push(
            &Prose::new()
                .p("Authentication answers:")
                .quote("Who are you?")
                .p("Authorization answers:")
                .quote("What may you access?"),
        )
        .push(&public::policy_decision());

    let actions = Section::new("GitHub Actions").push(&public::github_actions());
    let developers = Section::new("Developer identities")
        .alt()
        .push(&public::developer_cli());

    let together = Section::new("Identity + application")
        .push(&Prose::new().p(
            "A developer running Terraform and a developer running arbitrary networking tools can represent different access requests.",
        ))
        .push(
            &Compare::new("The same developer and destination, with two different applications.")
                .side(
                    "alice running terraform",
                    Tone::Active,
                    &PolicySummary::new("alice", "terraform", "db.prod:5432", "—"),
                )
                .side(
                    "versus alice running curl",
                    Tone::Neutral,
                    &PolicySummary::new("alice", "curl", "db.prod:5432", "—"),
                ),
        )
        .push(
            &Prose::new()
                .p("Policies can distinguish these contexts, but the application name is supplied by the caller. It is not authenticated proof of which program is running. Constrain destinations and verified identity claims as well.")
                .p("Identity tells SkiMasque who is asking.")
                .p("Policy determines what happens next."),
        );

    Page {
        path: "identities/index.html",
        contents: SitePage::new(
            "../",
            "identities",
            "Identities · SkiMasque",
            "Traditional network access often starts with “Where are you connecting from?”; SkiMasque starts with “Who or what is requesting access?”.",
        )
        .push(&hero)
        .push(&types)
        .push(&not_authz)
        .push(&actions)
        .push(&developers)
        .push(&together)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_follows_the_canonical_spec() {
        let p = page();
        assert_eq!(p.path, "identities/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        for want in [
            "Give network access an identity.",
            "Where are you connecting from?",
            "Who or what is requesting access?",
            "Developer",
            "Repository",
            "Workflow",
            "Ref",
            "Application",
            "Workload",
            "developer: alice",
            "Identity is not authorization",
            "Who are you?",
            "What may you access?",
            "GitHub Actions",
            "Developer identities",
            "Identity + application",
            "terraform",
            "curl",
            "db.prod:5432",
            "Identity tells SkiMasque who is asking.",
            "Policy determines what happens next.",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
        assert!(
            s.contains("skimasque connect"),
            "the developer flow uses connect"
        );
        assert!(s.find("terraform").unwrap() < s.rfind("curl").unwrap());
    }
}
