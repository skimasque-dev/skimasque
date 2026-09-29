//! One module per public page. Each exposes `page() -> Page`.

use super::{Page, DOCS_BASE};

pub fn all() -> Vec<Page> {
    Vec::new()
}

/// A link to a file under the repository's `docs/`.
#[allow(dead_code)]
pub(super) fn doc(file: &str) -> String {
    format!("{DOCS_BASE}{file}")
}
