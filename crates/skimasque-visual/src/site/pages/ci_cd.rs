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
const WORKFLOW: &str = "permissions:\n  id-token: write          # the job mints its own OIDC token\n  contents: read\n\nsteps:\n  - uses: skimasque-dev/connect@v2\n    with:\n      proxy: gateway.skimasque.com:443\n      audience: https://gateway.skimasque.com\n      mode: proxy\n      application: terraform\n\n  - run: terraform apply -auto-approve   # egresses through the gateway";

pub fn page() -> Page {
    let hero = Hero::new(
        "Give CI jobs access to private infrastructure without giving them the whole network.",
    )
    .lead("Authenticate the job with OIDC, apply a policy, and reach only the destinations it allows.")
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
        .push(
            &Prose::new()
                .p("Agent sessions close at expiry; CI credentials renew while the job runs."),
        )
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
        .push(&Prose::new().p("These are illustrative policies, not automatic permissions. Configure repository, branch and workflow matches to grant each job the access it needs."));

    let lifecycle = Section::new("Lifecycle of a CI session").push(&platform::ci_lifecycle());

    let action = Section::new("The GitHub Action")
        .alt()
        .push(&Prose::new().p(
            "The Action exchanges GitHub OIDC for a renewable credential. Transparent mode routes configured private TCP/UDP and split DNS on dedicated Ubuntu runners. Explicit proxy mode sets HTTP, HTTPS and SOCKS variables for tools that support them. Policy applies to traffic through the gateway; public traffic outside configured routes keeps its normal path.",
        ))
        .push(&CodeExample::new(".github/workflows/deploy.yml", WORKFLOW));

    let integrations = Section::new("CI/CD integrations").push(
        &Prose::new()
            .p("Supported today:")
            .list(&["GitHub Actions", "generic OIDC", "GitLab CI", "Buildkite"])
            .p("Use proxy mode for proxy-aware tools. For psql and other raw-socket tools, configure transparent routes and private DNS, with IP/CIDR policy and a gateway that can reach those networks. Use compatible Action/client releases.")
            .p("Additional integrations under consideration:")
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
            "Authenticate the job with OIDC, apply a policy, and reach only the destinations it allows.",
            "Kubernetes API",
            "GitHub Actions flow",
            "acme/infrastructure",
            "deploy-production",
            "20 minutes · 100 Mbps",
            "ACCESS GRANTED",
            "Agent sessions close at expiry",
            "Pull requests vs production",
            "feature/*",
            "staging-api:443",
            "db.prod:5432",
            "api.prod:443",
            "These are illustrative policies, not automatic permissions. Configure repository, branch and workflow matches to grant each job the access it needs.",
            "skimasque-dev/connect@v2",
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
