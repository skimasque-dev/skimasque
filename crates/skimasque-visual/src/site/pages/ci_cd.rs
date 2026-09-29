//! CI/CD (`/ci-cd`): canonical spec §15.

use super::doc;
use crate::diagrams::{platform, public};
use crate::site::Page;
use crate::site_chrome::GET_STARTED_URL;
use crate::{
    CodeExample, Compare, Component, Cta, DecisionBadge, Hero, PolicySummary, Prose, Section,
    SitePage, Status, StatusBadge, Tone,
};

/// The real workflow step from the Action's documentation.
const WORKFLOW: &str = "permissions:\n  id-token: write          # the job mints its own OIDC token\n  contents: read\n\nsteps:\n  - uses: skimasque-dev/connect@v1\n    with:\n      proxy: gateway.skimasque.com:443\n      audience: https://gateway.skimasque.com\n      application: terraform\n\n  - run: terraform apply -auto-approve   # egresses through the gateway";

pub fn page() -> Page {
    let hero = Hero::new(
        "Give CI jobs access to private infrastructure without giving them the whole network.",
    )
    .lead("CI/CD is one of the clearest use cases for SkiMasque.")
    .cta(Cta::primary("Get Started", GET_STARTED_URL))
    .cta(Cta::secondary("Read the Docs", doc("github-actions.md")));

    let problem = Section::new("The problem")
        .push(
            &Prose::new()
                .p("A deployment job may need to reach:")
                .list(&[
                    "database",
                    "private API",
                    "Kubernetes API",
                    "internal service",
                    "cloud control endpoint",
                ])
                .p("The traditional answer is often a VPN or VPC route from the CI runner into the private network."),
        )
        .push(&public::traditional_vs_skimasque())
        .push(&Prose::new().p("SkiMasque makes the access specific to the job."));

    let flow = Section::new("GitHub Actions flow")
        .alt()
        .push(&public::github_actions());

    let example = Section::new("Example")
        .push(&Prose::new().p("A Terraform deployment requests:"))
        .push(&PolicySummary::new(
            "acme/infrastructure · deploy-production · main",
            "terraform",
            "db.prod:5432",
            "20 minutes · 100 Mbps",
        ))
        .push(&Prose::new().p("Result:"))
        .push(&DecisionBadge::new(true))
        .push(&Prose::new().p("After 20 minutes: ACCESS EXPIRED"))
        .push(&StatusBadge {
            status: Status::Expired,
        });

    let branches = Section::new("Pull requests vs production")
        .alt()
        .push(
            &Compare::new("Three workflows in the same repository reach different destinations.")
                .side(
                    "Feature branch",
                    Tone::Neutral,
                    &PolicySummary::new("feature/*", "—", "staging-api:443", "—"),
                )
                .side(
                    "Main",
                    Tone::Info,
                    &PolicySummary::new("main", "—", "staging-api:443 · db.prod:5432", "—"),
                )
                .side(
                    "Production workflow",
                    Tone::Active,
                    &PolicySummary::new(
                        "deploy-production",
                        "—",
                        "db.prod:5432 · api.prod:443",
                        "—",
                    ),
                ),
        )
        .push(&Prose::new().p("The identity context can change the applicable policy."));

    let lifecycle = Section::new("Lifecycle of a CI session").push(&platform::ci_lifecycle());

    let action = Section::new("The GitHub Action")
        .alt()
        .push(&Prose::new().p(
            "The action exchanges the runner's OIDC token for a short-lived credential and sets ALL_PROXY to a local SOCKS5 relay. Tools that honour ALL_PROXY can then reach, through the gateway, only what your policy allows.",
        ))
        .push(&CodeExample::new(".github/workflows/deploy.yml", WORKFLOW));

    let integrations = Section::new("CI/CD integrations").push(
        &Prose::new()
            .p("Supported today:")
            .list(&["GitHub Actions", "generic OIDC", "GitLab CI", "Buildkite"])
            .p("Terraform and other command-line applications are the typical use case: any tool that honours ALL_PROXY can run behind the action.")
            .p("Architecture should remain extensible to:")
            .list(&["CircleCI", "Jenkins", "other workload identity providers"]),
    );

    Page {
        path: "ci-cd/index.html",
        contents: SitePage::new(
            "../",
            "ci-cd",
            "CI/CD · SkiMasque",
            "Give CI jobs access to private infrastructure without giving them the whole network: identity-aware, short-lived access for GitHub Actions and other CI systems.",
        )
        .push(&hero)
        .push(&problem)
        .push(&flow)
        .push(&example)
        .push(&branches)
        .push(&lifecycle)
        .push(&action)
        .push(&integrations)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_cd_follows_the_canonical_spec() {
        let p = page();
        assert_eq!(p.path, "ci-cd/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        for want in [
            "Give CI jobs access to private infrastructure without giving them the whole network.",
            "CI/CD is one of the clearest use cases for SkiMasque.",
            "Kubernetes API",
            "GitHub Actions flow",
            "acme/infrastructure",
            "deploy-production",
            "20 minutes · 100 Mbps",
            "ACCESS GRANTED",
            "ACCESS EXPIRED",
            "Pull requests vs production",
            "feature/*",
            "staging-api:443",
            "db.prod:5432",
            "api.prod:443",
            "The identity context can change the applicable policy.",
            "skimasque-dev/connect@v1",
            "CI/CD integrations",
            "GitLab CI",
            "Buildkite",
            "generic OIDC",
            "CircleCI",
            "Jenkins",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
    }
}
