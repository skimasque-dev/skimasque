//! A linear flow of nodes. Horizontal on wide screens, vertical on narrow
//! ones (CSS), and an ordered list either way so the sequence survives
//! without styles. The caption is the flow's text equivalent.

use askama::Template;

use crate::{Component, ConnKind, Connection, Html};

#[derive(Debug, Clone)]
pub struct Step {
    /// Any rendered component: a node, a boundary, another flow.
    pub body: Html,
    pub next: Option<Connection>,
}

#[derive(Template, Debug, Clone)]
#[template(path = "flow.html")]
pub struct Flow {
    pub steps: Vec<Step>,
    pub caption: String,
}

impl Flow {
    pub fn new(caption: impl Into<String>) -> Self {
        Flow {
            steps: Vec::new(),
            caption: caption.into(),
        }
    }

    pub fn then(self, c: &impl Component) -> Self {
        self.push(None, c)
    }

    pub fn via(self, conn: Connection, c: &impl Component) -> Self {
        self.push(Some(conn), c)
    }

    fn push(mut self, conn: Option<Connection>, c: &impl Component) -> Self {
        if let Some(prev) = self.steps.last_mut() {
            prev.next = Some(conn.unwrap_or_else(|| Connection::new(ConnKind::Normal)));
        }
        self.steps.push(Step {
            body: c.html(),
            next: None,
        });
        self
    }

    fn conn_html(step: &Step) -> Option<Html> {
        step.next.as_ref().map(|c| c.html())
    }
}

impl Component for Flow {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Boundary, Component, ConnKind, Node, NodeKind};

    #[test]
    fn a_flow_is_an_ordered_list_with_connections_between_steps_only() {
        let f = Flow::new("GitHub Actions reaches the database through a session.")
            .then(&Node::new(NodeKind::GitHub))
            .via(
                Connection::new(ConnKind::Control).label("OIDC"),
                &Node::new(NodeKind::Identity),
            )
            .via(
                Connection::new(ConnKind::Active),
                &Node::new(NodeKind::Database),
            );
        let h = f.html().as_str().to_owned();
        assert!(h.starts_with(r#"<figure class="v-flow-wrap">"#), "{h}");
        assert_eq!(h.matches(r#"<li class="v-flow-step">"#).count(), 3, "{h}");
        assert_eq!(
            h.matches(r#"class="v-conn "#).count(),
            2,
            "two connections for three steps: {h}"
        );
        assert!(
            h.contains("v-conn-control") && h.contains("v-conn-active"),
            "{h}"
        );
        assert!(h.contains(r#"<figcaption class="v-sr">GitHub Actions reaches the database through a session.</figcaption>"#), "{h}");
    }

    #[test]
    fn a_single_step_flow_has_no_dangling_connector() {
        let h = Flow::new("Just a gateway.")
            .then(&Node::new(NodeKind::Gateway))
            .html();
        assert!(!h.as_str().contains("v-conn"), "{h}");
    }

    #[test]
    fn the_last_steps_connection_never_renders() {
        let h = Flow {
            steps: vec![Step {
                body: Node::new(NodeKind::Gateway).html(),
                next: Some(Connection::new(ConnKind::Active)),
            }],
            caption: "x".into(),
        }
        .html();
        assert!(!h.as_str().contains("v-conn"), "{h}");
    }

    #[test]
    fn then_defaults_the_link_to_a_normal_connection() {
        let h = Flow::new("x")
            .then(&Node::new(NodeKind::Workload))
            .then(&Node::new(NodeKind::Policy))
            .html();
        assert!(h.as_str().contains("v-conn-normal"), "{h}");
    }

    #[test]
    fn a_step_can_hold_any_component() {
        let h = Flow::new("x")
            .then(&Node::new(NodeKind::Developer))
            .via(
                Connection::new(ConnKind::Active),
                &Boundary::region("YOUR VPC").child(&Node::new(NodeKind::Database)),
            )
            .html();
        let s = h.as_str();
        assert!(
            s.contains("v-boundary-region") && s.contains("v-conn-active"),
            "{s}"
        );
        assert_eq!(s.matches(r#"<li class="v-flow-step">"#).count(), 2, "{s}");
    }
}
