//! One module per public page. Each exposes `page() -> Page`.

use super::{Page, DOCS_BASE};

mod ci_cd;
mod compare;
mod deployment;
mod developers;
mod gateways;
mod home;
mod how_it_works;
mod identities;
mod policies;

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
    ]
}

/// A link to a file under the repository's `docs/`.
pub(super) fn doc(file: &str) -> String {
    format!("{DOCS_BASE}{file}")
}
