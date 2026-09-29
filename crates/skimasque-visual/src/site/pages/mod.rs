//! One module per public page. Each exposes `page() -> Page`.

use askama::Template;

use super::{Page, DOCS_BASE};
use crate::Component;

mod architecture;
mod ci_cd;
mod compare;
mod deployment;
mod developers;
mod gateways;
mod home;
mod how_it_works;
mod identities;
mod masque;
mod policies;
mod security;
mod use_cases;

pub fn all() -> Vec<Page> {
    vec![
        home::page(),
        how_it_works::page(),
        identities::page(),
        policies::page(),
        ci_cd::page(),
        developers::page(),
        compare::page(),
        deployment::page(),
        gateways::page(),
        security::page(),
        architecture::page(),
        masque::page(),
        use_cases::page(),
    ]
}

/// A link to a file under the repository's `docs/`.
pub(super) fn doc(file: &str) -> String {
    format!("{DOCS_BASE}{file}")
}

/// A row of documentation buttons: `(label, href)`.
#[derive(Template)]
#[template(
    source = r#"<p class="v-cta-row">{% for (label, href) in links %}<a class="v-btn" href="{{ href }}">{{ label }}</a>{% endfor %}</p>"#,
    ext = "html"
)]
pub(super) struct DocLinks {
    links: Vec<(String, String)>,
}
impl DocLinks {
    pub(super) fn new(links: &[(&str, String)]) -> Self {
        Self {
            links: links
                .iter()
                .map(|(l, h)| ((*l).to_owned(), h.clone()))
                .collect(),
        }
    }
}
impl Component for DocLinks {}
