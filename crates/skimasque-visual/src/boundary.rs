//! Network boundaries: a labelled region ("YOUR VPC") or a firewall rule
//! with the protected side below it. Children are other components.

use askama::Template;

use crate::{Component, Html};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryKind {
    Region,
    Firewall,
}

#[derive(Template, Debug, Clone)]
#[template(path = "boundary.html")]
pub struct Boundary {
    pub label: String,
    pub kind: BoundaryKind,
    pub children: Vec<Html>,
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
                r#"<section class="v-boundary v-boundary-region" aria-label="YOUR VPC">"#
            ),
            "{s}"
        );
        assert!(
            s.contains(r#"<span class="v-boundary-label">YOUR VPC</span>"#),
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
            s.contains(r#"class="v-boundary v-boundary-firewall""#),
            "{s}"
        );
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
