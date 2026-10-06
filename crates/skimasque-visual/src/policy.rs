//! Policy components: the WHO → WHAT → WHERE → LIMITS pattern in four shapes.

use stucco_core::Render;

use crate::{Component, DecisionBadge, Html, Status, StatusBadge};

#[derive(Debug, Clone)]
pub struct PolicySummary {
    pub who: String,
    pub what: String,
    pub target: String,
    pub limits: String,
}

impl Render for PolicySummary {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<dl class="v-summary"><div><dt>WHO</dt><dd>"#);
        crate::render::text(cx, &self.who);
        crate::render::markup(cx, r#"</dd></div><div><dt>WHAT</dt><dd>"#);
        crate::render::text(cx, &self.what);
        crate::render::markup(cx, r#"</dd></div><div><dt>WHERE</dt><dd>"#);
        crate::render::text(cx, &self.target);
        crate::render::markup(cx, r#"</dd></div><div><dt>LIMITS</dt><dd>"#);
        crate::render::text(cx, &self.limits);
        crate::render::markup(cx, r#"</dd></div></dl>"#);
    }
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

#[derive(Debug, Clone)]
pub struct PolicyCard {
    pub name: String,
    pub status: Status,
    pub summary: PolicySummary,
    pub meta: Option<String>,
    /// Must be an app-built path. It is HTML-escaped but not scheme-checked;
    /// never pass user input.
    pub href: Option<String>,
    pub level: u8,
}

impl Render for PolicyCard {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            "<article class=\"v-card v-policy-card\"><header class=\"v-card-head\"><h",
        );
        crate::render::text(cx, &self.level);
        crate::render::markup(cx, r#" class="v-card-title">"#);
        if let Some(h) = &self.href {
            crate::render::markup(cx, r#"<a href=""#);
            crate::render::text(cx, &h);
            crate::render::markup(cx, r#"">"#);
            crate::render::text(cx, &self.name);
            crate::render::markup(cx, r#"</a>"#);
        } else {
            crate::render::text(cx, &self.name);
        }
        crate::render::markup(cx, r#"</h"#);
        crate::render::text(cx, &self.level);
        crate::render::markup(cx, r#">"#);
        self.status_html().render(cx);
        crate::render::markup(cx, r#"</header>"#);
        self.summary_html().render(cx);
        if let Some(m) = &self.meta {
            crate::render::markup(cx, r#"<footer class="v-card-meta">"#);
            crate::render::text(cx, &m);
            crate::render::markup(cx, r#"</footer>"#);
        }
        crate::render::markup(cx, r#"</article>"#);
    }
}
impl PolicyCard {
    pub fn new(name: impl Into<String>, status: Status, summary: PolicySummary) -> Self {
        Self {
            name: name.into(),
            status,
            summary,
            meta: None,
            href: None,
            level: 3,
        }
    }
    /// The heading level of the card title, `2..=6` (default 3). Pick the level
    /// that follows the page's own headings so levels never skip.
    pub fn level(mut self, n: u8) -> Self {
        self.level = n.clamp(2, 6);
        self
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

#[derive(Debug, Clone)]
pub struct PolicyExplorer {
    pub who: Vec<String>,
    pub what: Vec<String>,
    pub target: Vec<String>,
    pub limits: Vec<String>,
    pub allow: bool,
    pub reason: String,
    pub technical: bool,
}

impl Render for PolicyExplorer {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            "<div class=\"v-explorer\" role=\"group\" aria-label=\"Policy explorer\">",
        );
        for l in (self.layers()).iter() {
            crate::render::markup(cx, r#"<div class="v-layer" role="group" aria-label=""#);
            crate::render::text(cx, &l.title);
            crate::render::markup(cx, r#"" data-layer=""#);
            crate::render::text(cx, &l.key);
            crate::render::markup(cx, r#""><p class="v-layer-title" aria-hidden="true">"#);
            crate::render::text(cx, &l.title);
            crate::render::markup(cx, r#"</p>"#);
            if l.lines.is_empty() {
                crate::render::markup(cx, r#"<p class="v-layer-line v-layer-any">any</p>"#);
            } else {
                crate::render::markup(cx, r#"<ul class="v-layer-lines">"#);
                for line in l.lines.iter() {
                    crate::render::markup(cx, r#"<li>"#);
                    crate::render::text(cx, &line);
                    crate::render::markup(cx, r#"</li>"#);
                }
                crate::render::markup(cx, r#"</ul>"#);
            }
            crate::render::markup(
                cx,
                "</div><span class=\"v-layer-link\" aria-hidden=\"true\">↓</span>",
            );
        }
        crate::render::markup(
            cx,
            "<div class=\"v-layer v-explorer-result\" role=\"group\" aria-label=\"Decision\">",
        );
        self.decision_html().render(cx);
        crate::render::markup(cx, r#"<p class="v-layer-line">"#);
        crate::render::text(cx, &self.reason);
        crate::render::markup(cx, r#"</p></div></div>"#);
    }
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
            technical: false,
        }
    }
    /// ALLOW / DENY wording instead of ACCESS GRANTED / DENIED.
    pub fn technical(mut self) -> Self {
        self.technical = true;
        self
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
        let b = DecisionBadge::new(self.allow);
        if self.technical { b.technical() } else { b }.html()
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

#[derive(Debug, Clone)]
pub struct PolicyDiff {
    pub changes: Vec<Change>,
}

impl Render for PolicyDiff {
    fn render(&self, cx: &mut stucco_core::Cx) {
        if self.changes.is_empty() {
            crate::render::markup(cx, r#"<p class="v-diff-empty">No changes</p>"#);
        } else {
            crate::render::markup(cx, r#"<ul class="v-diff">"#);
            for c in &self.changes {
                crate::render::markup(cx, r#"<li class="v-diff-"#);
                crate::render::text(cx, &c.class());
                crate::render::markup(cx, r#""><span class="v-diff-glyph" aria-hidden="true">"#);
                crate::render::text(cx, &c.glyph());
                crate::render::markup(cx, r#"</span><span class="v-sr">"#);
                crate::render::text(cx, &c.class());
                crate::render::markup(cx, r#" </span><span class="v-diff-field">"#);
                crate::render::text(cx, &c.field);
                crate::render::markup(cx, r#"</span>"#);
                if let Some(b) = &c.before {
                    crate::render::markup(cx, r#"<span class="v-diff-before">"#);
                    crate::render::text(cx, &b);
                    crate::render::markup(cx, r#"</span>"#);
                }
                if c.before.is_some() && c.after.is_some() {
                    crate::render::markup(cx, r#"<span aria-hidden="true">→</span>"#);
                }
                if let Some(a) = &c.after {
                    crate::render::markup(cx, r#"<span class="v-diff-after">"#);
                    crate::render::text(cx, &a);
                    crate::render::markup(cx, r#"</span>"#);
                }
                crate::render::markup(cx, r#"</li>"#);
            }
            crate::render::markup(cx, r#"</ul>"#);
        }
    }
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
    fn explorer_technical_uses_allow_deny_wording() {
        let s = PolicyExplorer::new(&[], &[], &[], &[], true, "r")
            .technical()
            .html()
            .as_str()
            .to_owned();
        assert!(s.contains("ALLOW") && !s.contains("ACCESS"), "{s}");
        let d = PolicyExplorer::new(&[], &[], &[], &[], false, "r")
            .technical()
            .html()
            .as_str()
            .to_owned();
        assert!(d.contains("DENY") && !d.contains("ACCESS"));
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

    #[test]
    fn a_policy_card_takes_a_heading_level() {
        let c = PolicyCard::new(
            "prod",
            Status::Active,
            PolicySummary::new("a", "b", "c", "d"),
        )
        .href("/app/policies/prod")
        .level(2);
        let h = c.html();
        assert!(
            h.as_str().contains(
                "<h2 class=\"v-card-title\"><a href=\"/app/policies/prod\">prod</a></h2>"
            ),
            "{}",
            h.as_str()
        );
    }

    #[test]
    fn explorer_groups_are_named_and_carry_no_ids() {
        let e = PolicyExplorer::new(
            &["acme/widget"],
            &["terraform"],
            &["db:5432"],
            &["20 min"],
            true,
            "ok",
        );
        let h = e.html();
        let s = h.as_str();
        assert!(!s.contains("<section"), "{s}");
        for name in ["WHO", "WHAT", "WHERE", "LIMITS", "Decision"] {
            assert!(
                s.contains(&format!("role=\"group\" aria-label=\"{name}\"")),
                "{name}: {s}"
            );
        }
        assert!(!s.contains(" id="), "{s}");
    }
}
