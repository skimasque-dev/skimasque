//! A linear flow of nodes. Horizontal on wide screens, vertical on narrow
//! ones (CSS), and an ordered list either way so the sequence survives
//! without styles. The caption is the flow's text equivalent.

use stucco_core::Render;

use crate::{Component, ConnKind, Connection, Html};

#[derive(Debug, Clone)]
pub struct Step {
    /// Any rendered component: a node, a boundary, another flow.
    pub body: Html,
    pub next: Option<Connection>,
}

#[derive(Debug, Clone)]
pub struct Flow {
    pub steps: Vec<Step>,
    pub caption: String,
}

impl Render for Flow {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            "<figure class=\"v-flow-wrap\"><ol class=\"v-flow\" data-layout=\"",
        );
        crate::render::text(cx, &self.layout());
        crate::render::markup(cx, r#"">"#);
        for (index_0, s) in self.steps.iter().enumerate() {
            crate::render::markup(cx, r#"<li class="v-flow-step">"#);
            s.body.render(cx);
            if index_0 + 1 != self.steps.len() {
                if let Some(c) = &(Self::conn_html(s)) {
                    c.render(cx);
                }
            }
            crate::render::markup(cx, r#"</li>"#);
        }
        crate::render::markup(cx, r#"</ol><figcaption class="v-sr">"#);
        crate::render::text(cx, &self.caption);
        crate::render::markup(
            cx,
            r#"</figcaption></figure>
"#,
        );
    }
}

impl Flow {
    pub fn new(caption: impl Into<String>) -> Self {
        Flow {
            steps: Vec::new(),
            caption: caption.into(),
        }
    }

    /// Nest containers only inside `Compare` sides and `Branch` arms, which have a definite width: a `Flow`, `Branch` or `Compare` placed in a content-sized slot (a `Flow` step body, a `Branch` root) may collapse.
    pub fn then(self, c: &impl Component) -> Self {
        self.push(None, c)
    }

    /// Nest containers only inside `Compare` sides and `Branch` arms, which have a definite width: a `Flow`, `Branch` or `Compare` placed in a content-sized slot (a `Flow` step body, a `Branch` root) may collapse.
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

    /// Complex component bodies retain the wide breakpoint; simple node chains
    /// can use a row as soon as their node count permits it.
    fn layout(&self) -> &'static str {
        if !self
            .steps
            .iter()
            .all(|step| step.body.as_str().starts_with("<div class=\"v-node "))
        {
            return "full";
        }
        match self.steps.len() {
            0..=3 => "compact",
            4..=5 => "medium",
            _ => "full",
        }
    }
}

impl Component for Flow {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Boundary, Component, ConnKind, Node, NodeKind};

    #[test]
    fn flows_choose_breakpoints_for_their_content() {
        for (count, want) in [
            (1, "compact"),
            (3, "compact"),
            (4, "medium"),
            (5, "medium"),
            (6, "full"),
            (7, "full"),
        ] {
            let mut flow = Flow::new("A sequence of nodes.");
            for _ in 0..count {
                flow = flow.then(&Node::new(NodeKind::Gateway));
            }
            assert!(
                flow.html()
                    .as_str()
                    .contains(&format!("data-layout=\"{want}\"")),
                "{count} nodes should use {want}"
            );
        }
        let complex = Flow::new("A gateway enters a network.")
            .then(&Node::new(NodeKind::Gateway))
            .then(&Boundary::region("YOUR VPC").child(&Node::new(NodeKind::Database)));
        assert!(
            complex.html().as_str().contains("data-layout=\"full\""),
            "complex bodies keep the conservative breakpoint"
        );
    }

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
