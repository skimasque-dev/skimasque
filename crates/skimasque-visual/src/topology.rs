//! Structured network diagrams assembled from existing visual primitives.
use crate::{Boundary, Branch, Component, Connection, Html, Node};
use stucco_core::Render;

/// A gateway and the services it can reach inside a named private network.
#[derive(Debug, Clone)]
pub struct TopologyNetwork {
    boundary: String,
    gateway: Node,
    control: Connection,
    services: Vec<(Connection, Node)>,
}
impl TopologyNetwork {
    pub fn new(boundary: impl Into<String>, gateway: Node, control: Connection) -> Self {
        Self {
            boundary: boundary.into(),
            gateway,
            control,
            services: Vec::new(),
        }
    }
    pub fn service(mut self, connection: Connection, service: Node) -> Self {
        self.services.push((connection, service));
        self
    }
    fn html(&self) -> Html {
        let branch = self.services.iter().fold(
            Branch::new(format!("Services behind {}.", self.boundary), &self.gateway),
            |branch, (connection, service)| branch.arm(connection.clone(), service),
        );
        Boundary::region(&self.boundary).child(&branch).html()
    }
}

#[derive(Debug, Clone)]
struct TopologyArm {
    connection: Html,
    body: Html,
}

/// Requesters → control plane → private networks, with independently styled links.
/// Layout measures its container; callers never supply pixel coordinates.
/// Emit [`crate::Icons`] once per page, as with other node-based diagrams.
///
/// ```
/// use skimasque_visual::{Component, ConnKind, Connection, NetworkTopology, Node, NodeKind, TopologyNetwork};
/// let topology = NetworkTopology::new(
///     "The control plane authorizes the gateway to reach a database.",
///     Node::new(NodeKind::ControlPlane),
/// ).network(TopologyNetwork::new(
///     "Production VPC", Node::new(NodeKind::Gateway),
///     Connection::new(ConnKind::Control),
/// ).service(Connection::new(ConnKind::Active), Node::new(NodeKind::Database)));
/// let html = topology.html();
/// assert!(html.as_str().contains("Production VPC"));
/// ```
#[derive(Debug, Clone)]
pub struct NetworkTopology {
    caption: String,
    control_plane: Html,
    workloads: Vec<TopologyArm>,
    networks: Vec<TopologyArm>,
}

impl Render for NetworkTopology {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<figure class="v-topology">"#);
        if !self.workloads.is_empty() {
            crate::render::markup(cx, r#"<ul class="v-topology-workloads">"#);
            for arm in &self.workloads {
                crate::render::markup(cx, r#"<li>"#);
                arm.body.render(cx);
                arm.connection.render(cx);
                crate::render::markup(cx, r#"</li>"#);
            }
            crate::render::markup(cx, r#"</ul>"#);
        }
        crate::render::markup(cx, r#"<div class="v-topology-control">"#);
        self.control_plane.render(cx);
        crate::render::markup(cx, r#"</div>"#);
        if !self.networks.is_empty() {
            crate::render::markup(cx, r#"<ul class="v-topology-networks">"#);
            for arm in &self.networks {
                crate::render::markup(cx, r#"<li>"#);
                arm.connection.render(cx);
                arm.body.render(cx);
                crate::render::markup(cx, r#"</li>"#);
            }
            crate::render::markup(cx, r#"</ul>"#);
        } else {
            crate::render::markup(
                cx,
                "<p class=\"v-topology-empty\">No private networks configured.</p>",
            );
        }
        crate::render::markup(cx, r#"<figcaption class="v-sr">"#);
        crate::render::text(cx, &self.caption);
        crate::render::markup(cx, r#"</figcaption></figure>"#);
    }
}
impl NetworkTopology {
    pub fn new(caption: impl Into<String>, control_plane: Node) -> Self {
        Self {
            caption: caption.into(),
            control_plane: control_plane.html(),
            workloads: Vec::new(),
            networks: Vec::new(),
        }
    }
    pub fn workload(mut self, workload: Node, connection: Connection) -> Self {
        self.workloads.push(TopologyArm {
            connection: connection.html(),
            body: workload.html(),
        });
        self
    }
    pub fn network(mut self, network: TopologyNetwork) -> Self {
        self.networks.push(TopologyArm {
            connection: network.control.html(),
            body: network.html(),
        });
        self
    }
}
impl Component for NetworkTopology {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConnKind, NodeKind};
    #[test]
    fn topology_preserves_link_states_boundaries_and_escaped_labels() {
        let html = NetworkTopology::new(
            "A job reaches one database.",
            Node::new(NodeKind::ControlPlane),
        )
        .workload(
            Node::new(NodeKind::CiJob),
            Connection::new(ConnKind::Control),
        )
        .network(
            TopologyNetwork::new(
                "<private>",
                Node::new(NodeKind::Gateway),
                Connection::new(ConnKind::Control),
            )
            .service(
                Connection::new(ConnKind::Active),
                Node::new(NodeKind::Database),
            )
            .service(Connection::new(ConnKind::Denied), Node::new(NodeKind::Api)),
        )
        .html()
        .as_str()
        .to_owned();
        for expected in [
            "v-topology",
            "i-control-plane",
            "i-gateway",
            "v-conn-active",
            "v-conn-denied",
            "&lt;private&gt;",
            "A job reaches one database.",
        ] {
            assert!(html.contains(expected), "{expected}");
        }
        assert!(!html.contains("<private>"));
    }
}
