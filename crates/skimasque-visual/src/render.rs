//! Helpers for the custom visual grammar. Markup is always a source literal;
//! dynamic values use stucco's escaping, including quotes in attributes.

use std::fmt::Display;
use stucco_core::{escape::escape_attr, Cx, Raw, Render};

pub(crate) fn markup(cx: &mut Cx, html: &'static str) {
    Raw::trusted(html).render(cx);
}

pub(crate) fn text(cx: &mut Cx, value: &impl Display) {
    let mut escaped = String::new();
    escape_attr(&value.to_string(), &mut escaped);
    Raw::trusted(escaped).render(cx);
}
