//! SkiMasque's visual language, rendered on the server.
//!
//! Components implement stucco's [`Render`] trait and compose directly with
//! stucco UI components. The dashboard fills them with live data and the site
//! generator with examples; both share [`CSS`]. Text and attribute values are
//! escaped by stucco. [`Html`] holds trusted output from a [`Component`].

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

/// Anything that renders to [`Html`]. Other crates can implement this for
/// their own stucco components; they are responsible for escaping their output.
pub trait Component: Render {
    fn html(&self) -> Html {
        Html(stucco_core::to_html(self))
    }
}

impl Render for Html {
    fn render(&self, cx: &mut Cx) {
        stucco_core::Raw::trusted(self.as_str()).render(cx);
    }
}

/// Stucco's rendering interface, re-exported for downstream components.
pub use stucco_core::{Cx, Render};

mod render;

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
pub mod motif;
pub mod motion;
pub mod node;
pub mod policy;
#[cfg(feature = "site")]
pub mod site;
pub mod site_chrome;
pub mod status;
pub mod topology;

pub use access::{
    GatewayCard, HealthCard, IdentityCard, SessionCard, SessionTimeline, TimelineEvent,
};
pub use boundary::{Boundary, BoundaryKind};
pub use chrome::{
    Block, ComparisonTable, Cta, CtaBand, Faq, Feature, FeatureGrid, Hero, PlannedBlock, Prose,
    Section, TierCard, TierGrid,
};
pub use connection::{ArrowGeometry, ConnKind, Connection};
pub use content::{CodeExample, EmptyState, Planned};
pub use decision::{AuditEventCard, Check, DecisionCard, DecisionExplainer, DIMENSIONS};
pub use flow::{Flow, Step};
pub use icons::Icons;
pub use layout::{Arm, Branch, Compare, LayerRow, Layers, Message, Sequence, Side};
pub use motif::{Contour, Mountain, Route, Run, RunCard, Shape, TrailMarker};
pub use motion::{Expire, Reveal, WorkflowDemo};
pub use node::{Node, NodeKind, Tone};
pub use policy::{Change, ChangeKind, PolicyCard, PolicyDiff, PolicyExplorer, PolicySummary};
pub use site_chrome::{SiteFooter, SiteNav, SitePage, NAV_GROUPS};
pub use status::{DecisionBadge, Status, StatusBadge};
pub use topology::{NetworkTopology, TopologyNetwork};
