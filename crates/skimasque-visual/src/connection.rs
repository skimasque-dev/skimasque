//! Connections between nodes. Solid = data, dashed = control plane, thick
//! mint = an active session, dotted = a potential route, broken with × =
//! denied. The meaning is also spoken for screen readers.

use askama::Template;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnKind {
    Normal,
    Control,
    Active,
    Potential,
    Denied,
}

impl ConnKind {
    pub fn slug(self) -> &'static str {
        match self {
            ConnKind::Normal => "normal",
            ConnKind::Control => "control",
            ConnKind::Active => "active",
            ConnKind::Potential => "potential",
            ConnKind::Denied => "denied",
        }
    }
    pub fn spoken(self) -> &'static str {
        match self {
            ConnKind::Normal => "",
            ConnKind::Control => "control plane",
            ConnKind::Active => "active",
            ConnKind::Potential => "potential",
            ConnKind::Denied => "denied",
        }
    }
    fn is_denied(self) -> bool {
        self == ConnKind::Denied
    }
}

#[derive(Template, Debug, Clone)]
#[template(path = "connection.html")]
pub struct Connection {
    pub kind: ConnKind,
    pub label: Option<String>,
}

impl Connection {
    pub fn new(kind: ConnKind) -> Self {
        Connection { kind, label: None }
    }
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl crate::Component for Connection {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    #[test]
    fn each_kind_has_its_class_and_spoken_word() {
        for (k, slug, word) in [
            (ConnKind::Normal, "normal", ""),
            (ConnKind::Control, "control", "control plane"),
            (ConnKind::Active, "active", "active"),
            (ConnKind::Potential, "potential", "potential"),
            (ConnKind::Denied, "denied", "denied"),
        ] {
            let h = Connection::new(k).html().as_str().to_owned();
            assert!(
                h.contains(&format!(r#"class="v-conn v-conn-{slug}""#)),
                "{h}"
            );
            if word.is_empty() {
                assert!(!h.contains("v-sr"), "{h}");
            } else {
                assert!(
                    h.contains(&format!(r#"<span class="v-sr">{word}</span>"#)),
                    "{h}"
                );
            }
        }
        assert!(Connection::new(ConnKind::Denied)
            .html()
            .as_str()
            .contains(r#"<span class="v-conn-x" aria-hidden="true">×</span>"#));
    }

    #[test]
    fn labels_are_shown_and_escaped() {
        let h = Connection::new(ConnKind::Control)
            .label("OIDC <tok>")
            .html();
        assert!(
            h.as_str()
                .contains(r#"<span class="v-conn-label">OIDC &lt;tok&gt;</span>"#),
            "{h}"
        );
    }
}
