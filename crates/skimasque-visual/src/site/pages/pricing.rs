//! Pricing: entitlements and Stripe integration exist; availability is deployment-specific.

use crate::site::Page;
use crate::site_chrome::{link, SIGN_IN_URL};
use crate::{Component, Cta, CtaBand, Hero, Prose, Section, SitePage, TierCard, TierGrid};

pub fn page() -> Page {
    let hero = Hero::new("Start free. Choose where traffic runs.");

    let free = TierCard::new("Free", "$0", "For:")
        .include("evaluation")
        .include("personal projects")
        .include("small workloads")
        .live();
    let team = TierCard::new("Team", "See console", "For small engineering teams.")
        .include("core policies")
        .include("workload identities")
        .include("CI/CD access")
        .include("developer CLI")
        .include("basic audit")
        .include("managed gateway options; check console availability");
    let business = TierCard::new(
        "Business",
        "See console",
        "For teams with production infrastructure.",
    )
    .include("advanced policy controls")
    .include("multiple gateways")
    .include("richer audit")
    .include("team controls")
    .include("policy revision and audit workflows")
    .include("higher limits; check console availability");
    let enterprise = TierCard::new("Enterprise", "Custom", "Potential capabilities:")
        .include("SSO (Planned)")
        .include("advanced RBAC (Planned)")
        .include("compliance requirements")
        .include("dedicated infrastructure")
        .include("support agreements")
        .include("custom deployment")
        .include("contractual requirements")
        .planned("enterprise plans");

    let tiers = Section::new("Plans")
        .push(&Prose::new().p(
            "Free, Team and Business entitlements and Stripe billing are implemented. Online upgrades, invoices and plan management appear in Settings → Billing when enabled on your control plane. Paid pricing is not final; check the console for current availability and pricing. Enterprise capabilities remain planned.",
        ))
        .push(
            &TierGrid::new()
                .card(&free)
                .card(&team)
                .card(&business)
                .card(&enterprise),
        );

    let philosophy = Section::new("Pricing philosophy").alt().push(
        &Prose::new()
            .p("Plans use resource entitlements and usage counters. Review the console for included limits; the service tracks:")
            .list(&[
                "organisation members",
                "registered gateways",
                "published policy documents",
                "shared-gateway tunnels per calendar month",
            ])
            .p("Payment integration does not make every proposed capability available. Dedicated Cloud egress, SSO and advanced RBAC remain planned. Current entitlements are shown in Settings → Billing."),
    );

    let cta = CtaBand::new("Start with the Free tier.")
        .line("Start free and check Settings → Billing for available upgrades.")
        .cta(Cta::primary("Try SkiMasque", SIGN_IN_URL))
        .cta(Cta::secondary("Talk to Us", link("../", "contact")));

    Page {
        path: "pricing/index.html",
        contents: SitePage::new(
            "../",
            "pricing",
            "Pricing · SkiMasque",
            "SkiMasque plans: Free, Team and Business entitlements, deployment-configured billing, and planned Enterprise capabilities. Check the console for current pricing.",
        )
        .push(&hero)
        .push(&tiers)
        .push(&philosophy)
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
    fn four_tiers_with_prices_and_only_the_paid_ones_planned() {
        let p = page();
        assert_eq!(p.path, "pricing/index.html");
        let s = &p.contents;
        for want in [
            "Free",
            "Team",
            "Business",
            "Enterprise",
            "$0",
            "See console",
            "See console",
            "Custom",
            "Available now",
            "not final",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
        assert_eq!(s.matches("<article class=\"v-card v-tier\"").count(), 4);
        let main = s
            .split("<main")
            .nth(1)
            .expect("main")
            .split("</main>")
            .next()
            .unwrap();
        assert_eq!(
            main.matches("PLANNED").count(),
            1,
            "Only Enterprise remains a planned tier"
        );
        let free = main
            .split("<article")
            .nth(1)
            .unwrap()
            .split("</article>")
            .next()
            .unwrap();
        assert!(free.contains("Free") && !free.contains("PLANNED"));
        assert!(main.contains("SSO (Planned)") && main.contains("advanced RBAC (Planned)"));
    }
}
