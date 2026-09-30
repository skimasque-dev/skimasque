//! The homepage (`/`): canonical spec §11.

use askama::Template;

use super::doc;
use crate::diagrams::public;
use crate::site::Page;
use crate::site_chrome::{link, GET_STARTED_URL};
use crate::{
    Component, ConnKind, Connection, Cta, CtaBand, FeatureGrid, Flow, Hero, Html, Node, NodeKind,
    PolicyExplorer, Prose, Route, Section, SitePage, Status,
};

/// The hero artwork: the session path with the animated route beneath it.
#[derive(Template)]
#[template(
    source = r#"<div class="v-hero-art">{{ flow|safe }}{{ route|safe }}</div>"#,
    ext = "html"
)]
struct HeroArt {
    flow: Html,
    route: Html,
}
impl Component for HeroArt {}

fn hero_flow() -> Flow {
    let active = || Connection::new(ConnKind::Active);
    Flow::new(
        "An identity is checked against policy, a temporary session is opened, and the gateway \
         carries it to the private service.",
    )
    .then(&Node::new(NodeKind::Identity).label("IDENTITY"))
    .via(active(), &Node::new(NodeKind::Policy).label("POLICY"))
    .via(
        active(),
        &Node::new(NodeKind::Session)
            .label("TEMPORARY SESSION")
            .status(Status::Active),
    )
    .via(active(), &Node::new(NodeKind::Gateway).label("GATEWAY"))
    .via(
        active(),
        &Node::new(NodeKind::Service).label("PRIVATE SERVICE"),
    )
}

pub fn page() -> Page {
    let art = HeroArt {
        flow: hero_flow().html(),
        route: Route::new().flowing().html(),
    };
    let hero = Hero::new("Give every workload exactly the network access it needs.")
        .lead(
            "SkiMasque provides identity-aware, least-privilege network access for developers \
             and CI/CD workloads.",
        )
        .lead("No broad VPN membership.")
        .lead("No permanent network credentials.")
        .lead("No standing access.")
        .lead("Just the network access required for the job.")
        .cta(Cta::primary("Get Started", GET_STARTED_URL))
        .cta(Cta::secondary("See How It Works", link("", "how-it-works")))
        .aside(&art);

    let problem = Section::new("Your deployment shouldn't need the whole network.")
        .push(
            &Prose::new()
                .p("A deployment may need to reach one database.")
                .p("That doesn't mean it should automatically gain access to:")
                .list(&[
                    "every internal API",
                    "every database",
                    "administrative services",
                    "monitoring systems",
                    "unrelated production infrastructure",
                ])
                .p("Traditional solutions often solve this by putting the workload somewhere inside the network.")
                .p("SkiMasque solves the problem at the access layer."),
        )
        .push(&public::traditional_vs_skimasque());

    let model = Section::new("Network access as a capability.")
        .alt()
        .push(&Prose::new().p("Every request answers four questions."))
        .push(&public::policy_model())
        .push(&PolicyExplorer::new(
            &["acme/widget"],
            &["terraform"],
            &["db.prod:5432"],
            &["20m", "100 Mbps"],
            true,
            "policy production-deploy matches",
        ));

    let features = Section::new("What you get").push(
        &FeatureGrid::new()
            .feature(
                "Identity-aware",
                "Know what workload or developer is requesting access.",
            )
            .feature(
                "Least privilege",
                "Grant access to specific destinations instead of entire networks.",
            )
            .feature("Short-lived", "Access expires automatically.")
            .feature("Policy-driven", "Define access declaratively.")
            .feature(
                "Developer-friendly",
                "Run a command with the access its policy grants: \
                 skimasque exec --policy production -- terraform apply. \
                 Or use skimasque connect, or the GitHub Action in CI.",
            )
            .feature(
                "Real network access",
                "Built around MASQUE, HTTP/3, and QUIC.",
            ),
    );

    let cta = CtaBand::new("Network access should be temporary.")
        .line("Define the access your workloads need.")
        .line("Give it to them.")
        .line("Let it disappear when the work is done.")
        .cta(Cta::primary("Create Your First Policy", doc("policies.md")));

    Page {
        path: "index.html",
        contents: SitePage::new(
            "",
            "",
            "SkiMasque — identity-aware network access",
            "SkiMasque provides identity-aware, least-privilege network access for developers and CI/CD workloads.",
        )
        .push(&hero)
        .push(&problem)
        .push(&model)
        .push(&features)
        .push(&cta)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_homepage_follows_the_canonical_spec() {
        let p = page();
        assert_eq!(p.path, "index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        for want in [
            "Give every workload exactly the network access it needs.",
            "No broad VPN membership.",
            "No permanent network credentials.",
            "No standing access.",
            "Your deployment shouldn't need the whole network.",
            "Network access as a capability.",
            "WHO",
            "WHAT",
            "WHERE",
            "LIMITS",
            "ACCESS GRANTED",
            "Identity-aware",
            "Least privilege",
            "Short-lived",
            "Policy-driven",
            "Developer-friendly",
            "Real network access",
            "Network access should be temporary.",
            "Create Your First Policy",
            "Get Started",
            "See How It Works",
        ] {
            assert!(
                s.contains(want) || s.contains(&want.replace('\'', "&#39;")),
                "missing {want:?}"
            );
        }
        assert!(s.contains("v-route"), "the hero route motif");
        assert!(
            !s.to_lowercase().contains("command wrapper"),
            "exec is built: no planned command-wrapper card"
        );
        assert!(s.contains(
            "Run a command with the access its policy grants: \
             skimasque exec --policy production -- terraform apply."
        ));
        assert!(s.contains("skimasque connect"));
        assert!(!s.contains('`'), "no literal backticks in rendered copy");
    }
}
