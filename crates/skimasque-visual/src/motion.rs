//! Motion that communicates state, never decoration. Each wrapper only sets
//! a class; the animation lives in CSS and is off under reduced motion.

use stucco_core::Render;

use crate::{Component, Html};

/// The steps of a decision appear in order (the "authorization reveal").
#[derive(Debug, Clone)]
pub struct Reveal {
    pub inner: Html,
}

impl Render for Reveal {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<div class="v-reveal">"#);
        self.inner.render(cx);
        crate::render::markup(cx, r#"</div>"#);
    }
}
impl Reveal {
    pub fn new(c: &impl Component) -> Self {
        Self { inner: c.html() }
    }
}
impl Component for Reveal {}

/// An expired session settles to a faded state (static under reduced motion).
#[derive(Debug, Clone)]
pub struct Expire {
    pub inner: Html,
}

impl Render for Expire {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<div class="v-expire">"#);
        self.inner.render(cx);
        crate::render::markup(cx, r#"</div>"#);
    }
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

#[derive(Debug, Clone)]
pub struct WorkflowDemo {
    stages: Vec<WorkflowStage>,
}

impl Render for WorkflowDemo {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            r#"<div class="v-workflow">
  <p class="v-workflow-note"><strong>Simulated example</strong> · Playback compresses a 20-minute session into a few seconds. It does not open a connection or show live activity.</p>
  <div class="v-workflow-controls" hidden>
    <button type="button" data-action="play">Play</button>
    <button type="button" data-action="pause" disabled>Pause</button>
    <button type="button" data-action="replay">Replay</button>
  </div>
  <p class="v-workflow-progress" role="status" aria-live="polite" aria-atomic="true">Read the six steps below to follow the session from request to expiry.</p>
  <ol class="v-workflow-stages">
    "#,
        );
        for (index_0, stage) in self.stages.iter().enumerate() {
            crate::render::markup(
                cx,
                r#"<li class="v-workflow-stage">
      <p class="v-workflow-title"><span class="v-workflow-number" aria-hidden="true">"#,
            );
            crate::render::text(cx, &(index_0 + 1));
            crate::render::markup(cx, r#"</span><strong>"#);
            crate::render::text(cx, &stage.title);
            crate::render::markup(
                cx,
                "</strong></p>\n      <div class=\"v-workflow-diagram\">",
            );
            stage.diagram.render(cx);
            crate::render::markup(
                cx,
                r#"</div>
      <p class="v-workflow-description">"#,
            );
            crate::render::text(cx, &stage.description);
            crate::render::markup(
                cx,
                r#"</p>
    </li>"#,
            );
        }
        crate::render::markup(
            cx,
            r#"
  </ol>
</div>
<script>"#,
        );
        stucco_core::Raw::trusted(self.script()).render(cx);
        crate::render::markup(
            cx,
            r#"</script>
"#,
        );
    }
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
