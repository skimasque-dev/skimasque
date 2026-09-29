//! SkiMasque's visual language, rendered on the server.
//!
//! Every component is a plain struct that renders itself with askama. Callers
//! embed the result with `{{ component|safe }}`: the dashboard fills
//! components with live data, the website's generator with examples, and both
//! share [`CSS`]. Library components escape every string they are given (askama's
//! default escaping); the only markup they accept is [`Html`], which a caller can
//! only obtain by rendering a [`Component`].

#![forbid(unsafe_code)]

use std::fmt;

/// The library stylesheet: design tokens, then the component layers.
pub const CSS: &str = include_str!("../static/visual.css");

/// Rendered markup from a [`Component`]. It has no public constructor, so the
/// only way to nest markup inside a library component is to render another
/// component; plain strings stay escaped.
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

/// Anything that renders to [`Html`]. The trait is intentionally open: other
/// crates (the dashboard) implement it for their own askama templates so they
/// can be nested in a [`Flow`] or [`Boundary`]; such a component is responsible
/// for escaping its own output.
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

pub mod access;
pub mod boundary;
pub mod chrome;
pub mod connection;
pub mod content;
pub mod decision;
pub mod diagrams;
pub mod flow;
pub mod icons;
pub mod layout;
pub mod motion;
pub mod motif;
pub mod node;
pub mod policy;
#[cfg(feature = "site")]
pub mod site;
pub mod site_chrome;
pub mod status;

pub use access::{
    GatewayCard, HealthCard, IdentityCard, SessionCard, SessionTimeline, TimelineEvent,
};
pub use boundary::{Boundary, BoundaryKind};
pub use chrome::{
    Block, ComparisonTable, Cta, CtaBand, Faq, Feature, FeatureGrid, Hero, PlannedBlock, Prose,
    Section, TierCard,
};
pub use connection::{ConnKind, Connection};
pub use content::{CodeExample, EmptyState, Planned};
pub use decision::{AuditEventCard, Check, DecisionCard, DecisionExplainer, DIMENSIONS};
pub use flow::{Flow, Step};
pub use icons::Icons;
pub use layout::{Arm, Branch, Compare, LayerRow, Layers, Message, Sequence, Side};
pub use motif::{Contour, Mountain, Route, Run, RunCard, Shape, TrailMarker};
pub use motion::{Expire, Reveal};
pub use node::{Node, NodeKind, Tone};
pub use policy::{Change, ChangeKind, PolicyCard, PolicyDiff, PolicyExplorer, PolicySummary};
pub use site_chrome::{SiteFooter, SiteNav, SitePage, NAV_GROUPS};
pub use status::{DecisionBadge, Status, StatusBadge};
