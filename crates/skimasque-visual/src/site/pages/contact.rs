//! Contact (`/contact`): canonical spec §54, amended: there is no working
//! backend, so contact goes to GitHub Issues. No form, no inputs, no email.

use crate::site::{Page, ISSUES_URL, REPO_URL};
use crate::{Component, Cta, CtaBand, Hero, PlannedBlock, Prose, Section, SitePage};

pub fn page() -> Page {
    let hero = Hero::new("Talk to us about your network access problem.");

    let start = CtaBand::new("Start a conversation")
        .line(
            "How does your infrastructure currently provide network access to CI/CD or developers?",
        )
        .line("Open a GitHub issue with your workflow, the destinations it needs, and the access controls you use today.")
        .cta(Cta::primary("Open an issue", ISSUES_URL))
        .cta(Cta::secondary("View the repository", REPO_URL));

    let form = Section::new("Contact form").alt().push(&PlannedBlock::new(
        "contact form",
        &Prose::new()
            .p("A contact form is planned for deployment and product enquiries.")
            .p("For now, use GitHub issues for questions and feedback. Issues are public; leave out credentials and sensitive infrastructure details."),
    ));

    Page {
        path: "contact/index.html",
        contents: SitePage::new(
            "../",
            "contact",
            "Contact · SkiMasque",
            "Talk to the SkiMasque team about network access for CI/CD and developers. Contact is through GitHub issues for now.",
        )
        .push(&hero)
        .push(&start)
        .push(&form)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_points_to_github_issues_with_no_form_or_email() {
        let p = page();
        assert_eq!(p.path, "contact/index.html");
        let s = &p.contents;
        assert!(s.contains("github.com/skimasque-dev/skimasque/issues"));
        assert!(s.contains("Talk to us about your network access problem."));
        assert!(s.contains("How does your infrastructure currently provide network access"));
        for banned in ["<form", "<input", "<textarea", "mailto:"] {
            assert!(!s.contains(banned), "{banned}");
        }
        assert!(s.contains("PLANNED"));
        assert!(s.contains("v-planned-block"));
    }
}
