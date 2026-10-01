//! Motion that communicates state, never decoration. Each wrapper only sets
//! a class; the animation lives in CSS and is off under reduced motion.

use askama::Template;

use crate::{Component, Html};

/// The steps of a decision appear in order (the "authorization reveal").
#[derive(Template, Debug, Clone)]
#[template(path = "reveal.html")]
pub struct Reveal {
    pub inner: Html,
}
impl Reveal {
    pub fn new(c: &impl Component) -> Self {
        Self { inner: c.html() }
    }
}
impl Component for Reveal {}

/// An expired session settles to a faded state (static under reduced motion).
#[derive(Template, Debug, Clone)]
#[template(path = "expire.html")]
pub struct Expire {
    pub inner: Html,
}
impl Expire {
    pub fn new(c: &impl Component) -> Self {
        Self { inner: c.html() }
    }
}
impl Component for Expire {}

/// An illustrative, controllable walkthrough of a workload's access lifecycle.
#[derive(Debug, Clone)]
struct WorkflowStage {
    title: &'static str,
    description: &'static str,
    diagram: Html,
}

#[derive(Template, Debug, Clone)]
#[template(path = "workflow.html")]
pub struct WorkflowDemo {
    stages: Vec<WorkflowStage>,
}
impl WorkflowDemo {
    pub fn new() -> Self {
        use crate::{ConnKind, Connection, Flow, Node, NodeKind, Status};
        let traffic = Flow::new("The gateway forwards the session's traffic to db.prod:5432.")
            .then(&Node::new(NodeKind::Gateway).sub("us-west"))
            .via(
                Connection::new(ConnKind::Active).label("traffic"),
                &Node::new(NodeKind::Database).sub("db.prod:5432"),
            );
        Self {
            stages: vec![
                WorkflowStage { title: "Request access", description: "A GitHub Actions job in acme/widget asks to run terraform against db.prod:5432.", diagram: Node::new(NodeKind::GitHub).sub("acme/widget").html() },
                WorkflowStage { title: "Verify identity", description: "SkiMasque verifies the job's OIDC identity, including its repository and workflow claims.", diagram: Node::new(NodeKind::Identity).sub("deploy-production").html() },
                WorkflowStage { title: "Check policy", description: "A matching allow rule permits this identity and application to reach the requested destination. Without a matching allow rule, the request is denied.", diagram: Node::new(NodeKind::Policy).status(Status::Granted).html() },
                WorkflowStage { title: "Open a session", description: "An authorized request receives a session limited to db.prod:5432 for 20 minutes in this example.", diagram: Node::new(NodeKind::Session).sub("20 minutes").status(Status::Active).html() },
                WorkflowStage { title: "Connect through the gateway", description: "The workload's traffic passes through the us-west gateway to the private database. The control plane makes the access decision; the gateway carries the traffic.", diagram: traffic.html() },
                WorkflowStage { title: "Session expires", description: "When the session expires or ends, its access closes. A later connection needs a new authorized session.", diagram: Node::new(NodeKind::Session).status(Status::Expired).html() },
            ],
        }
    }

    fn script(&self) -> &'static str {
        include_str!("../static/workflow.js")
    }
}
impl Default for WorkflowDemo {
    fn default() -> Self {
        Self::new()
    }
}
impl Component for WorkflowDemo {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Node, NodeKind};

    #[test]
    fn workflow_is_a_complete_simulated_example_without_javascript() {
        let html = super::WorkflowDemo::new().html();
        let s = html.as_str();
        assert_eq!(s.matches("class=\"v-workflow-stage\"").count(), 6);
        let mut at = 0;
        for title in [
            "Request access",
            "Verify identity",
            "Check policy",
            "Open a session",
            "Connect through the gateway",
            "Session expires",
        ] {
            at += s[at..].find(title).expect("ordered workflow stage") + title.len();
        }
        for text in [
            "Simulated example",
            "acme/widget",
            "db.prod:5432",
            "20 minutes",
            "Play",
            "Pause",
            "Replay",
        ] {
            assert!(s.contains(text), "missing {text}");
        }
        assert!(s.contains("class=\"v-workflow-controls\" hidden"));
        assert!(s.contains("aria-live=\"polite\""));
    }

    #[test]
    fn wrappers_wrap_the_component_and_mark_the_motion() {
        let n = Node::new(NodeKind::Policy);
        let r = Reveal::new(&n).html();
        assert!(
            r.as_str().starts_with(r#"<div class="v-reveal">"#) && r.as_str().contains("v-node")
        );
        let e = Expire::new(&n).html();
        assert!(
            e.as_str().starts_with(r#"<div class="v-expire">"#) && e.as_str().contains("v-node")
        );
    }
}
