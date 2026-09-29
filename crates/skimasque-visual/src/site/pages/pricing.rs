//! Pricing (`/pricing`): canonical spec §25. Only the Free tier is live; the
//! paid tiers and their prices are planned, not final (billing is not in place).

use crate::site::Page;
use crate::{Component, Hero, Prose, Section, SitePage, TierCard, TierGrid};

const PAID_NOTE: &str = "paid plans — billing is not available yet";

pub fn page() -> Page {
    let hero = Hero::new("Pay for the platform. Choose where traffic runs.");

    let free = TierCard::new("Free", "$0", "For:")
        .include("evaluation")
        .include("personal projects")
        .include("small workloads")
        .live();
    let team = TierCard::new("Team", "$49 / month", "For small engineering teams.")
        .include("core policies")
        .include("workload identities")
        .include("CI/CD access")
        .include("developer CLI")
        .include("basic audit")
        .include("managed gateway options")
        .planned(PAID_NOTE);
    let business = TierCard::new(
        "Business",
        "$199 / month",
        "For teams with production infrastructure.",
    )
    .include("advanced policy controls")
    .include("multiple gateways")
    .include("richer audit")
    .include("team controls")
    .include("advanced integrations")
    .include("higher limits")
    .planned(PAID_NOTE);
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
            "The Free tier and SkiMasque Cloud are available now. Team, Business and Enterprise are planned: billing is not available yet, and the paid prices shown here are planned, not final.",
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
            .p("Pricing should not be overcomplicated around:")
            .list(&[
                "bandwidth",
                "packet counts",
                "individual users",
                "number of policy rules",
            ])
            .p("The product value is primarily the managed access-control platform."),
    );

    Page {
        path: "pricing/index.html",
        contents: SitePage::new(
            "../",
            "pricing",
            "Pricing · SkiMasque",
            "SkiMasque pricing: a Free tier is available now; Team, Business and Enterprise plans are planned and their prices are not final.",
        )
        .push(&hero)
        .push(&tiers)
        .push(&philosophy)
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
            "$49 / month",
            "$199 / month",
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
            3,
            "Team, Business, Enterprise"
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
