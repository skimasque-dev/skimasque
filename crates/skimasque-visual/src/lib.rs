//! SkiMasque's visual language, rendered on the server.
//!
//! Every component is a plain struct that renders itself with askama. Callers
//! embed the result with `{{ component|safe }}`: the dashboard fills
//! components with live data, the website's generator with examples, and both
//! share [`CSS`]. Components escape every string they are given; the only
//! markup a component accepts is [`Html`] produced by another component.

#![forbid(unsafe_code)]

use std::fmt;

/// The library stylesheet: design tokens, then the component layers.
pub const CSS: &str = include_str!("../static/visual.css");

/// Rendered markup from a library component. Only this crate can create one,
/// so a caller cannot smuggle unescaped strings into a component that nests
/// others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Html(String);

impl Html {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Html {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A library component: anything that renders to [`Html`].
pub trait Component: askama::Template {
    fn html(&self) -> Html {
        // Component templates only format owned strings; rendering cannot
        // fail except on a formatter error, which would be a bug here.
        Html(
            self.render()
                .expect("component templates render infallibly"),
        )
    }
}

pub mod boundary;
pub mod connection;
pub mod flow;
pub mod icons;
pub mod node;
pub mod site;
pub mod status;

pub use boundary::{Boundary, BoundaryKind};
pub use connection::{ConnKind, Connection};
pub use flow::{Flow, Step};
pub use icons::Icons;
pub use node::{Node, NodeKind, Tone};
pub use status::{DecisionBadge, Status, StatusBadge};
