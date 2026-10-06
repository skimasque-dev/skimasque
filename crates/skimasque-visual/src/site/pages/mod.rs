//! One module per public page. Each exposes `page() -> Page`.

use stucco_core::Render;

use super::{Page, DOCS_BASE};
use crate::Component;

mod about;
mod architecture;
mod ci_cd;
mod compare;
mod contact;
mod deployment;
mod developers;
mod docs;
mod faq;
mod gateways;
mod home;
mod how_it_works;
mod identities;
mod masque;
mod open_source;
mod policies;
mod pricing;
mod security;
mod status;
mod trust;
mod use_cases;

pub fn all() -> Vec<Page> {
    let mut pages = vec![
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
        open_source::page(),
        pricing::page(),
        docs::page(),
        faq::page(),
        trust::page(),
        about::page(),
        contact::page(),
        status::page(),
    ];
    pages.extend(docs::guides());
    pages
}

/// A public guide hosted in the website.
pub(super) fn doc(file: &str) -> String {
    format!("{DOCS_BASE}{}/", file.trim_end_matches(".md"))
}

/// A row of documentation buttons: `(label, href)`.
pub(super) struct DocLinks {
    links: Vec<(String, String)>,
}

impl Render for DocLinks {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<p class="v-cta-row">"#);
        for (label, href) in self.links.iter() {
            crate::render::markup(cx, r#"<a class="v-btn v-btn-quiet" href=""#);
            crate::render::text(cx, &href);
            crate::render::markup(cx, r#"">"#);
            crate::render::text(cx, &label);
            crate::render::markup(cx, r#"</a>"#);
        }
        crate::render::markup(cx, r#"</p>"#);
    }
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
