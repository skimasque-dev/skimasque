//! Node kinds and the visual grammar that colours them (spec: Components → Node).

use askama::Template;

use crate::{Component, Html, Status, StatusBadge};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Active,
    Structure,
    Edge,
    Neutral,
    Deny,
    Info,
}

impl Tone {
    pub fn class(self) -> &'static str {
        match self {
            Tone::Active => "active",
            Tone::Structure => "structure",
            Tone::Edge => "edge",
            Tone::Neutral => "neutral",
            Tone::Deny => "deny",
            Tone::Info => "info",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Workload,
    Developer,
    GitHub,
    CiJob,
    Application,
    Identity,
    Policy,
    Session,
    Gateway,
    Network,
    Service,
    Database,
    Api,
    Kubernetes,
    Cloud,
    Firewall,
    Internet,
    Allow,
    Deny,
}

impl NodeKind {
    pub const ALL: [NodeKind; 19] = [
        NodeKind::Workload,
        NodeKind::Developer,
        NodeKind::GitHub,
        NodeKind::CiJob,
        NodeKind::Application,
        NodeKind::Identity,
        NodeKind::Policy,
        NodeKind::Session,
        NodeKind::Gateway,
        NodeKind::Network,
        NodeKind::Service,
        NodeKind::Database,
        NodeKind::Api,
        NodeKind::Kubernetes,
        NodeKind::Cloud,
        NodeKind::Firewall,
        NodeKind::Internet,
        NodeKind::Allow,
        NodeKind::Deny,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            NodeKind::Workload => "workload",
            NodeKind::Developer => "developer",
            NodeKind::GitHub => "github",
            NodeKind::CiJob => "ci-job",
            NodeKind::Application => "application",
            NodeKind::Identity => "identity",
            NodeKind::Policy => "policy",
            NodeKind::Session => "session",
            NodeKind::Gateway => "gateway",
            NodeKind::Network => "network",
            NodeKind::Service => "service",
            NodeKind::Database => "database",
            NodeKind::Api => "api",
            NodeKind::Kubernetes => "kubernetes",
            NodeKind::Cloud => "cloud",
            NodeKind::Firewall => "firewall",
            NodeKind::Internet => "internet",
            NodeKind::Allow => "allow",
            NodeKind::Deny => "deny",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            NodeKind::Session => "i-clock",
            NodeKind::Allow => "i-check",
            NodeKind::Deny => "i-cross",
            NodeKind::Workload => "i-workload",
            NodeKind::Developer => "i-developer",
            NodeKind::GitHub => "i-github",
            NodeKind::CiJob => "i-ci-job",
            NodeKind::Application => "i-application",
            NodeKind::Identity => "i-identity",
            NodeKind::Policy => "i-policy",
            NodeKind::Gateway => "i-gateway",
            NodeKind::Network => "i-network",
            NodeKind::Service => "i-service",
            NodeKind::Database => "i-database",
            NodeKind::Api => "i-api",
            NodeKind::Kubernetes => "i-kubernetes",
            NodeKind::Cloud => "i-cloud",
            NodeKind::Firewall => "i-firewall",
            NodeKind::Internet => "i-internet",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            NodeKind::Network
            | NodeKind::Service
            | NodeKind::Database
            | NodeKind::Api
            | NodeKind::Kubernetes
            | NodeKind::Cloud => Tone::Structure,
            NodeKind::Gateway | NodeKind::Firewall | NodeKind::Internet => Tone::Edge,
            NodeKind::Session | NodeKind::Allow => Tone::Active,
            NodeKind::Deny => Tone::Deny,
            _ => Tone::Neutral,
        }
    }

    pub fn default_label(self) -> &'static str {
        match self {
            NodeKind::Workload => "Workload",
            NodeKind::Developer => "Developer",
            NodeKind::GitHub => "GitHub Actions",
            NodeKind::CiJob => "CI job",
            NodeKind::Application => "Application",
            NodeKind::Identity => "Identity",
            NodeKind::Policy => "Policy",
            NodeKind::Session => "Session",
            NodeKind::Gateway => "Gateway",
            NodeKind::Network => "Private network",
            NodeKind::Service => "Service",
            NodeKind::Database => "Database",
            NodeKind::Api => "Internal API",
            NodeKind::Kubernetes => "Kubernetes API",
            NodeKind::Cloud => "SkiMasque Cloud",
            NodeKind::Firewall => "Firewall",
            NodeKind::Internet => "Internet",
            NodeKind::Allow => "Allow",
            NodeKind::Deny => "Deny",
        }
    }
}

/// One thing in a diagram: a workload, a policy, a gateway, a database…
#[derive(Template, Debug, Clone)]
#[template(path = "node.html")]
pub struct Node {
    pub kind: NodeKind,
    pub label: String,
    pub sub: Option<String>,
    pub status: Option<Status>,
}

impl Node {
    pub fn new(kind: NodeKind) -> Self {
        Node {
            kind,
            label: kind.default_label().to_owned(),
            sub: None,
            status: None,
        }
    }
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
    #[allow(clippy::should_implement_trait)]
    pub fn sub(mut self, sub: impl Into<String>) -> Self {
        self.sub = Some(sub.into());
        self
    }
    pub fn status(mut self, status: Status) -> Self {
        self.status = Some(status);
        self
    }

    fn status_html(&self) -> Option<Html> {
        self.status.map(|status| StatusBadge { status }.html())
    }
}

impl Component for Node {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Status};

    #[test]
    fn a_node_shows_icon_label_sub_and_status_in_its_tone() {
        let html = Node::new(NodeKind::GitHub)
            .sub("acme/widget")
            .status(Status::Active)
            .html();
        let h = html.as_str();
        assert!(h.contains(r#"class="v-node v-tone-neutral""#), "{h}");
        assert!(h.contains(r#"data-kind="github""#), "{h}");
        assert!(h.contains(r##"<use href="#i-github"></use>"##), "{h}");
        assert!(
            h.contains(r#"<span class="v-node-label">GitHub Actions</span>"#),
            "{h}"
        );
        assert!(
            h.contains(r#"<span class="v-node-sub">acme/widget</span>"#),
            "{h}"
        );
        assert!(h.contains("ACTIVE"), "status word: {h}");
    }

    #[test]
    fn node_text_is_escaped() {
        let h = Node::new(NodeKind::Service)
            .label("<script>x</script>")
            .sub("a \"b\" & c")
            .html();
        assert!(!h.as_str().contains("<script>"), "{h}");
        assert!(h.as_str().contains("&lt;script&gt;"), "{h}");
        assert!(
            h.as_str().contains("&quot;b&quot; &amp; c")
                || h.as_str().contains("&#34;b&#34; &amp; c"),
            "{h}"
        );
    }
}
