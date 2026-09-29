//! Policies (`/policies`): canonical spec §14.

use super::doc;
use crate::diagrams::public;
use crate::site::Page;
use crate::site_chrome::GET_STARTED_URL;
use crate::{
    CodeExample, Component, ConnKind, Connection, Cta, Flow, Hero, Node, NodeKind, PlannedBlock,
    Prose, Section, SitePage,
};

/// The real production policy (`docs/policies.md`), as a TOML document.
const PRODUCTION_TOML: &str = r#"name = "production"

[match]                       # WHO
repository = "acme/widget"
branch = "main"

[[rules]]
application = "terraform"     # WHAT
action = "allow"
destinations = [              # WHERE
    "api.production.example.com:443",
    "*.terraform.io:443",
]

[session]                     # LIMITS
max_duration = "20m"

[limits]
bandwidth = "100Mbps"
connections = 50

[[tests]]                     # runs in CI
application = "terraform"
destination = "api.production.example.com:443"
expect = "allow""#;

/// Developer database access. `[match]` uses a documented field (`organization`).
const DEVELOPER_DB_TOML: &str = r#"name = "developer-db"

[match]                       # WHO
organization = "acme"

[[rules]]
application = "psql"          # WHAT
action = "allow"
destinations = ["dev-db.internal:5432"]   # WHERE

[session]                     # LIMITS
max_duration = "60m""#;

const DENIAL: &str = "$ skimasque policy check production google.com:443 --app terraform\nDENY\n\nReason:\nNo matching allow rule.\n\nSuggested rule:\n  allow terraform google.com:443";

fn default_deny_flow() -> Flow {
    Flow::new("A request that matches no allow rule is denied.")
        .then(&Node::new(NodeKind::Workload).label("REQUEST"))
        .via(
            Connection::new(ConnKind::Normal),
            &Node::new(NodeKind::Policy).label("NO MATCH"),
        )
        .via(
            Connection::new(ConnKind::Denied),
            &Node::new(NodeKind::Deny).label("ACCESS DENIED"),
        )
}

pub fn page() -> Page {
    let hero = Hero::new("Turn workload identity into network access.")
        .lead("Policies define exactly what authenticated identities can access.")
        .cta(Cta::primary("Get Started", GET_STARTED_URL))
        .cta(Cta::secondary("Read the Docs", doc("policies.md")));

    let model =
        Section::new("Policy model")
            .push(&Prose::new().p(
                "Every policy answers four questions: who, what, where, and within what limits.",
            ))
            .push(&public::policy_model())
            .push(
                &Prose::new()
                    .kv("WHO", "acme/widget · main · deploy-production")
                    .kv("WHAT", "terraform")
                    .kv("WHERE", "db.prod:5432")
                    .kv("LIMITS", "20m · 100Mbps · us-west"),
            );

    let decision = Section::new("Policy decision")
        .alt()
        .push(&public::policy_decision());

    let default_deny = Section::new("Default deny")
        .push(&Prose::new().quote("No matching allow rule means DENY."))
        .push(&default_deny_flow());

    let examples = Section::new("Policy examples")
        .alt()
        .push(&Prose::new().sub("Production deployment"))
        .push(&CodeExample::new(
            ".masque/policies/production.toml",
            PRODUCTION_TOML,
        ))
        .push(&Prose::new().sub("Developer database access"))
        .push(&CodeExample::new(
            ".masque/policies/developer-db.toml",
            DEVELOPER_DB_TOML,
        ))
        .push(&Prose::new().p(
            "Policies are TOML or YAML files in your repository, so they can be reviewed, diffed and tested in CI.",
        ));

    let wizard = Prose::new()
        .p("The control plane should never force users to understand the entire policy model immediately.")
        .p("Start with:")
        .kv("Who needs access?", "acme/widget")
        .kv("What are they running?", "terraform")
        .kv("Where do they need to go?", "db.prod:5432")
        .kv("How long?", "20 minutes")
        .kv("Bandwidth limit?", "100 Mbps")
        .kv("Egress region?", "us-west")
        .p("Then show the generated policy.");
    let ux = Section::new("Policy UX")
        .push(&PlannedBlock::new(
            "interactive policy creation wizard in the dashboard",
            &wizard,
        ))
        .push(&Prose::new().p(
            "Today you write the policy file, then ask why a request would be allowed or denied. The engine explains the decision and suggests a rule.",
        ))
        .push(&CodeExample::new("Explain a denial, offline", DENIAL));

    Page {
        path: "policies/index.html",
        contents: SitePage::new(
            "../",
            "policies",
            "Policies · SkiMasque",
            "Policies define exactly what authenticated identities can access: who, what, where and within what limits. No matching allow rule means DENY.",
        )
        .push(&hero)
        .push(&model)
        .push(&decision)
        .push(&default_deny)
        .push(&examples)
        .push(&ux)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policies_follows_the_canonical_spec() {
        let p = page();
        assert_eq!(p.path, "policies/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        for want in [
            "Turn workload identity into network access.",
            "Policies define exactly what authenticated identities can access.",
            "Policy model",
            "Policy decision",
            "Default deny",
            "No matching allow rule means DENY.",
            "ACCESS DENIED",
            "Policy examples",
            "Production deployment",
            "Developer database access",
            "production.toml",
            "developer-db",
            "dev-db.internal:5432",
            "max_duration = &quot;60m&quot;",
            "Policy UX",
            "Who needs access?",
            "PLANNED",
            "skimasque policy check production google.com:443 --app terraform",
            "No matching allow rule.",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
        let main = &s[s.find("<main").unwrap()..s.find("</main>").unwrap()];
        assert!(main.contains("v-planned-block"));
        assert!(!s.contains("group ="), "no invented policy fields");
    }
}
