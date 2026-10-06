//! Network boundaries: a labelled region ("YOUR VPC") or a firewall rule
//! with the protected side below it. Children are other components.

use stucco_core::Render;

use crate::{Component, Html};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryKind {
    Region,
    Firewall,
}

#[derive(Debug, Clone)]
pub struct Boundary {
    pub label: String,
    pub kind: BoundaryKind,
    pub children: Vec<Html>,
}

impl Render for Boundary {
    fn render(&self, cx: &mut stucco_core::Cx) {
        if self.is_firewall() {
            crate::render::markup(
                cx,
                "<div class=\"v-boundary v-boundary-firewall\" role=\"group\" aria-label=\"",
            );
            crate::render::text(cx, &self.label);
            crate::render::markup(
                cx,
                r#""><div class="v-firewall-rule"><span class="v-boundary-label" aria-hidden="true">"#,
            );
            crate::render::text(cx, &self.label);
            crate::render::markup(cx, r#"</span></div><div class="v-boundary-body">"#);
            for c in &self.children {
                c.render(cx);
            }
            crate::render::markup(cx, r#"</div></div>"#);
        } else {
            crate::render::markup(
                cx,
                "<div class=\"v-boundary v-boundary-region\" role=\"group\" aria-label=\"",
            );
            crate::render::text(cx, &self.label);
            crate::render::markup(
                cx,
                "\"><span class=\"v-boundary-label\" aria-hidden=\"true\">",
            );
            crate::render::text(cx, &self.label);
            crate::render::markup(cx, r#"</span><div class="v-boundary-body">"#);
            for c in &self.children {
                c.render(cx);
            }
            crate::render::markup(cx, r#"</div></div>"#);
        }
    }
}

impl Boundary {
    pub fn region(label: impl Into<String>) -> Self {
        Boundary {
            label: label.into(),
            kind: BoundaryKind::Region,
            children: Vec::new(),
        }
    }

    pub fn firewall(label: impl Into<String>) -> Self {
        Boundary {
            label: label.into(),
            kind: BoundaryKind::Firewall,
            children: Vec::new(),
        }
    }

    pub fn child(mut self, c: &impl Component) -> Self {
        self.children.push(c.html());
        self
    }

    fn is_firewall(&self) -> bool {
        self.kind == BoundaryKind::Firewall
    }
}

impl Component for Boundary {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Node, NodeKind};

    #[test]
    fn a_region_frames_its_children_under_a_label() {
        let h = Boundary::region("YOUR VPC")
            .child(&Node::new(NodeKind::Gateway))
            .child(&Node::new(NodeKind::Database))
            .html();
        let s = h.as_str();
        assert!(
            s.starts_with(
                r#"<div class="v-boundary v-boundary-region" role="group" aria-label="YOUR VPC">"#
            ),
            "{s}"
        );
        assert!(
            s.contains(r#"<span class="v-boundary-label" aria-hidden="true">YOUR VPC</span>"#),
            "{s}"
        );
        assert_eq!(s.matches(r#"class="v-node "#).count(), 2, "{s}");
    }

    #[test]
    fn a_firewall_draws_a_labelled_rule_above_the_protected_side() {
        let s = Boundary::firewall("YOUR FIREWALL")
            .child(&Node::new(NodeKind::Network))
            .html()
            .as_str()
            .to_owned();
        assert!(
            s.contains(r#"class="v-boundary v-boundary-firewall" role="group""#),
            "{s}"
        );
        assert!(!s.contains("<section"), "a group, not a landmark: {s}");
        let rule = s.find("v-firewall-rule").unwrap();
        let node = s.find("v-node").unwrap();
        assert!(rule < node, "rule before the protected side: {s}");
    }

    #[test]
    fn the_label_is_escaped() {
        let s = Boundary::region("<b>vpc</b>").html().as_str().to_owned();
        assert!(!s.contains("<b>") && s.contains("&lt;b&gt;"), "{s}");
    }
}
