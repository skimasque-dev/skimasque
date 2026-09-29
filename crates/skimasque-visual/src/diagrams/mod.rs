//! The diagram sets. Each diagram is a function composing library
//! components; the registries list them for the gallery and the site
//! generator. Control-plane diagrams take real data as arguments.

use crate::{Component, Html};

pub mod control;
pub mod platform;
pub mod public;

/// A named diagram, rendered.
pub struct Entry {
    pub slug: &'static str,
    pub title: &'static str,
    pub html: Html,
}

fn entry(slug: &'static str, title: &'static str, c: &impl Component) -> Entry {
    Entry {
        slug,
        title,
        html: c.html(),
    }
}

/// The canonical public diagrams (marketing examples: `acme/widget`,
/// `db.prod:5432`, `20 min`).
pub fn public_set() -> Vec<Entry> {
    vec![
        entry(
            "identity-policy-access",
            "Identity → policy → access",
            &public::identity_policy_access(),
        ),
        entry(
            "policy-model",
            "Who, what, where, limits",
            &public::policy_model(),
        ),
        entry(
            "traditional-vs-skimasque",
            "Traditional vs SkiMasque",
            &public::traditional_vs_skimasque(),
        ),
        entry(
            "access-lifecycle",
            "Access lifecycle",
            &public::access_lifecycle(),
        ),
        entry(
            "policy-decision",
            "Policy decision",
            &public::policy_decision(),
        ),
        entry(
            "github-actions",
            "GitHub Actions",
            &public::github_actions(),
        ),
        entry("developer-cli", "Developer CLI", &public::developer_cli()),
        entry(
            "same-command-different-policy",
            "Same command, different policy",
            &public::same_command_different_policy(),
        ),
        entry("gateway", "Gateway", &public::gateway()),
        entry("customer-vpc", "Customer VPC", &public::customer_vpc()),
    ]
}

/// Control-plane diagrams rendered with example data (the dashboard calls
/// the `control` functions with real data).
pub fn control_set() -> Vec<Entry> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Any dotted quad (a fabricated IP address) in the markup.
    fn has_dotted_quad(s: &str) -> bool {
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_digit() && b[i - 1] != b'.') {
                let mut j = i;
                let mut groups = 0;
                loop {
                    let s0 = j;
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j == s0 {
                        break;
                    }
                    groups += 1;
                    if j < b.len() && b[j] == b'.' && j + 1 < b.len() && b[j + 1].is_ascii_digit() {
                        j += 1
                    } else {
                        break;
                    }
                }
                if groups >= 4 {
                    return true;
                }
                i = j.max(i + 1);
            } else {
                i += 1;
            }
        }
        false
    }

    #[test]
    fn dotted_quad_detector_works() {
        assert!(has_dotted_quad("egress 203.0.113.7 ok"));
        assert!(!has_dotted_quad("db.prod:5432 and v1.2.3 and 100 Mbps"));
    }

    #[test]
    fn every_diagram_is_named_captioned_honest_and_unique() {
        let all: Vec<Entry> = public_set().into_iter().chain(control_set()).collect();
        assert!(!all.is_empty());
        let mut slugs: Vec<_> = all.iter().map(|e| e.slug).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), all.len(), "slugs are unique");
        for e in &all {
            let s = e.html.as_str();
            assert!(
                s.contains(r#"class="v-sr""#),
                "{}: no text equivalent",
                e.slug
            );
            assert!(
                !s.to_lowercase().contains("exec"),
                "{}: mentions exec",
                e.slug
            );
            assert!(!has_dotted_quad(s), "{}: fabricated IP address", e.slug);
            assert!(!e.title.is_empty());
        }
    }

    #[test]
    fn the_public_set_covers_the_canonical_list() {
        let slugs: Vec<_> = public_set().iter().map(|e| e.slug).collect();
        for want in [
            "identity-policy-access",
            "policy-model",
            "traditional-vs-skimasque",
            "access-lifecycle",
            "policy-decision",
            "github-actions",
            "developer-cli",
            "same-command-different-policy",
            "gateway",
            "customer-vpc",
        ] {
            assert!(slugs.contains(&want), "{want}");
        }
    }
}
