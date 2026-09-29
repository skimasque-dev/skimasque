//! Policy components: the WHO → WHAT → WHERE → LIMITS pattern in four shapes.

use askama::Template;

use crate::{Component, DecisionBadge, Html, Status, StatusBadge};

#[derive(Template, Debug, Clone)]
#[template(path = "policy_summary.html")]
pub struct PolicySummary {
    pub who: String,
    pub what: String,
    pub target: String,
    pub limits: String,
}
impl PolicySummary {
    pub fn new(
        who: impl Into<String>,
        what: impl Into<String>,
        target: impl Into<String>,
        limits: impl Into<String>,
    ) -> Self {
        Self {
            who: who.into(),
            what: what.into(),
            target: target.into(),
            limits: limits.into(),
        }
    }
}
impl Component for PolicySummary {}

#[derive(Template, Debug, Clone)]
#[template(path = "policy_card.html")]
pub struct PolicyCard {
    pub name: String,
    pub status: Status,
    pub summary: PolicySummary,
    pub meta: Option<String>,
    pub href: Option<String>,
}
impl PolicyCard {
    pub fn new(name: impl Into<String>, status: Status, summary: PolicySummary) -> Self {
        Self {
            name: name.into(),
            status,
            summary,
            meta: None,
            href: None,
        }
    }
    pub fn meta(mut self, meta: impl Into<String>) -> Self {
        self.meta = Some(meta.into());
        self
    }
    pub fn href(mut self, href: impl Into<String>) -> Self {
        self.href = Some(href.into());
        self
    }
    fn status_html(&self) -> Html {
        StatusBadge {
            status: self.status,
        }
        .html()
    }
    fn summary_html(&self) -> Html {
        self.summary.html()
    }
}
impl Component for PolicyCard {}

struct Layer {
    key: &'static str,
    title: &'static str,
    lines: Vec<String>,
}

#[derive(Template, Debug, Clone)]
#[template(path = "policy_explorer.html")]
pub struct PolicyExplorer {
    pub who: Vec<String>,
    pub what: Vec<String>,
    pub target: Vec<String>,
    pub limits: Vec<String>,
    pub allow: bool,
    pub reason: String,
}
impl PolicyExplorer {
    pub fn new(
        who: &[&str],
        what: &[&str],
        target: &[&str],
        limits: &[&str],
        allow: bool,
        reason: impl Into<String>,
    ) -> Self {
        let own = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect();
        Self {
            who: own(who),
            what: own(what),
            target: own(target),
            limits: own(limits),
            allow,
            reason: reason.into(),
        }
    }
    fn layers(&self) -> Vec<Layer> {
        vec![
            Layer {
                key: "who",
                title: "WHO",
                lines: self.who.clone(),
            },
            Layer {
                key: "what",
                title: "WHAT",
                lines: self.what.clone(),
            },
            Layer {
                key: "where",
                title: "WHERE",
                lines: self.target.clone(),
            },
            Layer {
                key: "limits",
                title: "LIMITS",
                lines: self.limits.clone(),
            },
        ]
    }
    fn decision_html(&self) -> Html {
        DecisionBadge::new(self.allow).html()
    }
}
impl Component for PolicyExplorer {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone)]
pub struct Change {
    pub kind: ChangeKind,
    pub field: String,
    pub before: Option<String>,
    pub after: Option<String>,
}
impl Change {
    pub fn added(field: impl Into<String>, after: impl Into<String>) -> Self {
        Self {
            kind: ChangeKind::Added,
            field: field.into(),
            before: None,
            after: Some(after.into()),
        }
    }
    pub fn removed(field: impl Into<String>, before: impl Into<String>) -> Self {
        Self {
            kind: ChangeKind::Removed,
            field: field.into(),
            before: Some(before.into()),
            after: None,
        }
    }
    pub fn changed(
        field: impl Into<String>,
        before: impl Into<String>,
        after: impl Into<String>,
    ) -> Self {
        Self {
            kind: ChangeKind::Changed,
            field: field.into(),
            before: Some(before.into()),
            after: Some(after.into()),
        }
    }
    fn class(&self) -> &'static str {
        match self.kind {
            ChangeKind::Added => "added",
            ChangeKind::Removed => "removed",
            ChangeKind::Changed => "changed",
        }
    }
    fn glyph(&self) -> &'static str {
        match self.kind {
            ChangeKind::Added => "+",
            ChangeKind::Removed => "−",
            ChangeKind::Changed => "~",
        }
    }
}

#[derive(Template, Debug, Clone)]
#[template(path = "policy_diff.html")]
pub struct PolicyDiff {
    pub changes: Vec<Change>,
}
impl PolicyDiff {
    pub fn new(changes: Vec<Change>) -> Self {
        Self { changes }
    }
}
impl Component for PolicyDiff {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Status};

    fn summary() -> PolicySummary {
        PolicySummary::new("acme/widget", "terraform", "db.prod:5432", "20 min")
    }

    #[test]
    fn summary_lists_who_what_where_limits_in_order() {
        let h = summary().html();
        let s = h.as_str();
        let pos = |w: &str| s.find(w).unwrap_or_else(|| panic!("{w}"));
        assert!(
            pos("WHO") < pos("WHAT") && pos("WHAT") < pos("WHERE") && pos("WHERE") < pos("LIMITS")
        );
        for v in ["acme/widget", "terraform", "db.prod:5432", "20 min"] {
            assert!(s.contains(v), "{v}");
        }
    }

    #[test]
    fn card_shows_name_status_word_summary_and_optional_link() {
        let h = PolicyCard::new("production-deploy", Status::Active, summary())
            .meta("updated 2h ago")
            .href("/app/policies/1")
            .html();
        let s = h.as_str();
        assert!(s.contains("production-deploy") && s.contains("ACTIVE") && s.contains("WHO"));
        assert!(s.contains(r#"<a href="/app/policies/1""#) && s.contains("updated 2h ago"));
        assert!(!PolicyCard::new("x", Status::Pending, summary())
            .html()
            .as_str()
            .contains("<a "));
    }

    #[test]
    fn explorer_stacks_four_layers_then_the_decision() {
        let h = PolicyExplorer::new(
            &["acme/widget"],
            &["terraform"],
            &["db.prod:5432"],
            &["20 min", "TCP only"],
            true,
            "policy production-deploy matches",
        )
        .html();
        let s = h.as_str();
        for layer in ["WHO", "WHAT", "WHERE", "LIMITS"] {
            assert!(
                s.contains(&format!(r#"data-layer="{}""#, layer.to_lowercase())),
                "{layer}"
            );
        }
        assert_eq!(
            s.matches("v-layer-link").count(),
            4,
            "a link before each layer after the first and before the result"
        );
        assert!(s.contains("ACCESS GRANTED") && s.contains("policy production-deploy matches"));
        assert!(s.contains("TCP only"));
        let denied =
            PolicyExplorer::new(&[], &[], &[], &[], false, "no matching allow rule").html();
        assert!(denied.as_str().contains("ACCESS DENIED"));
        assert!(denied.as_str().contains("any"), "an empty layer says so");
    }

    #[test]
    fn diff_marks_added_removed_changed_with_glyph_and_word() {
        let h = PolicyDiff::new(vec![
            Change::added("limit", "TCP only"),
            Change::removed("target", "db.old:5432"),
            Change::changed("duration", "20 min", "10 min"),
        ])
        .html();
        let s = h.as_str();
        assert!(
            s.contains("v-diff-added")
                && s.contains("v-diff-removed")
                && s.contains("v-diff-changed")
        );
        assert!(s.contains("+") && s.contains("−") && s.contains("~"));
        for w in ["added", "removed", "changed"] {
            assert!(s.contains(w), "{w}");
        }
        assert!(s.contains("20 min") && s.contains("10 min") && s.contains("→"));
        assert!(PolicyDiff::new(vec![])
            .html()
            .as_str()
            .contains("No changes"));
    }

    #[test]
    fn policy_components_escape_their_strings() {
        let s = PolicySummary::new("<script>", "a", "b", "c").html();
        assert!(!s.as_str().contains("<script>") && s.as_str().contains("&lt;script&gt;"));
        let d = PolicyDiff::new(vec![Change::added("<i>", "\"q\"")]).html();
        assert!(!d.as_str().contains("<i>"));
    }
}
