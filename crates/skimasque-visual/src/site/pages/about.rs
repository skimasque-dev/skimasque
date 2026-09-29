//! About (`/about`): canonical spec §53. Philosophy only; no claims beyond the
//! identity, intent, policy, capability, expiration model the docs describe.

use crate::site::Page;
use crate::{Component, Hero, Layers, Prose, Section, SitePage};

pub fn page() -> Page {
    let hero = Hero::new("Network access should work the way modern infrastructure works.")
        .lead("SkiMasque was built around a simple observation.")
        .lead("Infrastructure is increasingly automated, ephemeral, and identity-aware.")
        .lead("Network access should be too.");

    let philosophy = Section::new("Philosophy")
        .push(&Prose::new().p(
            "Access starts from who or what is asking, is judged against policy, and ends on its own.",
        ))
        .push(
            &Layers::new("Identity, intent, policy, capability and expiration.")
                .row("Identity")
                .row("Intent")
                .row("Policy")
                .row("Capability")
                .row("Expiration"),
        );

    Page {
        path: "about/index.html",
        contents: SitePage::new(
            "../",
            "about",
            "About · SkiMasque",
            "Why SkiMasque exists: network access that is identity-aware, policy-checked and short-lived, like the infrastructure around it.",
        )
        .push(&hero)
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
    fn about_shows_the_philosophy_layers_in_order() {
        let p = page();
        assert_eq!(p.path, "about/index.html");
        let s = &p.contents;
        assert!(s.contains("Network access should work the way modern infrastructure works."));
        let mut at = s.find("Philosophy").expect("Philosophy");
        for w in ["Identity", "Intent", "Policy", "Capability", "Expiration"] {
            let i = s[at..]
                .find(w)
                .unwrap_or_else(|| panic!("missing {w} after {at}"));
            at += i + w.len();
        }
        assert!(s.contains("v-layers") && s.contains("↓"));
    }
}
