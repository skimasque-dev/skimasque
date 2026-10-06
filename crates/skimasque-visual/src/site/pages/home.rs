//! The homepage (`/`): canonical spec Â§11.

use stucco_core::Render;

use super::{ci_cd::WORKFLOW, DocLinks};
use crate::diagrams::public;
use crate::site::Page;
use crate::site_chrome::link;
use crate::{
    CodeExample, Component, Cta, CtaBand, FeatureGrid, Hero, PolicyExplorer, Prose, Section,
    SitePage,
};

/// A self-contained network illustration with progressive enhancement.
struct HeroArt;

impl Render for HeroArt {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, include_str!("../../../static/network_demo.html"));
        stucco_core::Raw::trusted(self.script()).render(cx);
        crate::render::markup(
            cx,
            r#"</script>
"#,
        );
    }
}
impl HeroArt {
    fn script(&self) -> &'static str {
        include_str!("../../../static/network_demo.js")
    }
}
impl Component for HeroArt {}

pub fn page() -> Page {
    let art = HeroArt;
    let hero = Hero::new("Give every workload exactly the network access it needs.")
        .lead(
            "SkiMasque provides identity-aware, least-privilege network access for developers, \
             CI/CD workloads and coding agents.",
        )
        .lead("Grant a policy-bounded session for the destinations the job needs.")
        .cta(Cta::primary("Get Started", "docs/getting-started/"))
        .cta(Cta::secondary(
            "Use SkiMasque Connect",
            "docs/github-actions/",
        ))
        .cta(Cta::secondary(
            "Watch the workflow",
            link("", "how-it-works"),
        ))
        .aside(&art);

    let connect = Section::new("Connect GitHub Actions to your private infrastructure.")
        .eyebrow("SkiMasque Connect")
        .alt()
        .push(&Prose::new()
            .lead("Give your workflow the access it needs with skimasque-dev/connect@v2.")
            .p("Authenticate with GitHub OIDC and reach the destinations your policy allows. No long-lived access token to store in repository secrets.")
            .p("This example uses proxy mode for Terraform. Replace the gateway and audience with your own, and configure a policy for your workflow before running it."))
        .push(&CodeExample::new(".github/workflows/deploy.yml", WORKFLOW))
        .push(&DocLinks {
            links: vec![
                ("Set Up Connect".into(), "docs/github-actions/".into()),
                ("View on GitHub".into(), "https://github.com/skimasque-dev/connect".into()),
            ],
        });

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
                .p("SkiMasque checks the workload's identity and requested destination against policy before the gateway opens a tunnel."),
        )
        .push(&public::traditional_vs_skimasque());

    let model = Section::new("Network access as a capability.")
        .alt()
        .push(&Prose::new().p("Specify who is asking, what application they name, where they need to connect, and the session limits."))
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
            .feature(
                "Short-lived",
                "Credentials expire; agent sessions also close active tunnels at expiry.",
            )
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
        .line("Test the policy, then run a command with the access it grants.")
        .cta(Cta::primary(
            "Create Your First Policy",
            "https://control.skimasque.com/app/policy/new",
        ));

    Page {
        path: "index.html",
        contents: SitePage::new(
            "",
            "",
            "SkiMasque — identity-aware network access",
            "SkiMasque provides identity-aware, least-privilege network access for developers, CI/CD workloads and coding agents.",
        )
        .push(&hero)
        .push(&connect)
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
            "Watch the workflow",
        ] {
            assert!(
                s.contains(want) || s.contains(&want.replace('\'', "&#39;")),
                "missing {want:?}"
            );
        }
        assert!(s.contains("v-network-demo"), "the animated network diagram");
        assert!(s.contains("Other services"));
        assert!(s.contains("Replay"));
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
