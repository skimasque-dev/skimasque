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
        .line("Answer in an issue on GitHub. The question itself tells us what to build next.")
        .cta(Cta::primary("Open an issue", ISSUES_URL))
        .cta(Cta::secondary("View the repository", REPO_URL));

    let form = Section::new("Contact form").alt().push(&PlannedBlock::new(
        "contact form (Name, Email, Company, Role)",
        &Prose::new()
            .p("The intended form asks for a name, an email address, a company and a role, and one open question: how does your infrastructure currently provide network access to CI/CD or developers?")
            .p("There is no contact form yet because nothing is in place to receive it. Until there is, open an issue on GitHub."),
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
