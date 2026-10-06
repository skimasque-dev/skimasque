//! Decision components: why access was granted or denied.

use stucco_core::Render;

use crate::{Component, DecisionBadge, Html};

/// The five dimensions a decision is explained along (canonical §43/§71).
pub const DIMENSIONS: [&str; 5] = [
    "Identity",
    "Application",
    "Destination",
    "Limits",
    "Policy active",
];

#[derive(Debug, Clone)]
pub struct Check {
    pub label: String,
    pub pass: bool,
    pub detail: Option<String>,
    pub narrated: bool,
}
impl Check {
    pub fn pass(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            pass: true,
            detail: None,
            narrated: false,
        }
    }
    pub fn fail(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            pass: false,
            detail: None,
            narrated: false,
        }
    }
    /// One narrated step ("Identity is governed by policy X."): the text is the
    /// whole message, so no "matched" word follows it. A screen reader still hears
    /// whether it passed.
    pub fn step(text: impl Into<String>, pass: bool) -> Self {
        Self {
            label: text.into(),
            pass,
            detail: None,
            narrated: true,
        }
    }
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct DecisionExplainer {
    pub checks: Vec<Check>,
    pub allow: bool,
    pub reason: String,
    pub technical: bool,
}

impl Render for DecisionExplainer {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<div class="v-explain">"#);
        if !self.checks.is_empty() {
            crate::render::markup(cx, r#"<ul class="v-checks">"#);
            for c in &self.checks {
                crate::render::markup(cx, r#"<li class="v-check "#);
                if c.pass {
                    crate::render::markup(cx, r#"v-check-pass"#);
                } else {
                    crate::render::markup(cx, r#"v-check-fail"#);
                }
                crate::render::markup(cx, r#""><span class="v-check-glyph" aria-hidden="true">"#);
                if c.pass {
                    crate::render::markup(cx, r#"✓"#);
                } else {
                    crate::render::markup(
                        cx,
                        r#"<svg class="v-denial-mark" viewBox="0 0 12 12" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true" focusable="false"><path d="M3 3 L9 9 M9 3 L3 9"/></svg>
"#,
                    );
                }
                crate::render::markup(cx, r#"</span><span class="v-check-label">"#);
                crate::render::text(cx, &c.label);
                crate::render::markup(cx, r#"</span>"#);
                if c.narrated {
                    crate::render::markup(cx, r#"<span class="v-sr">"#);
                    if c.pass {
                        crate::render::markup(cx, r#"passed"#);
                    } else {
                        crate::render::markup(cx, r#"failed"#);
                    }
                    crate::render::markup(cx, r#"</span>"#);
                } else {
                    crate::render::markup(cx, r#"<span class="v-check-word">"#);
                    if c.pass {
                        crate::render::markup(cx, r#"matched"#);
                    } else {
                        crate::render::markup(cx, r#"did not match"#);
                    }
                    crate::render::markup(cx, r#"</span>"#);
                }
                if let Some(d) = &c.detail {
                    crate::render::markup(cx, r#"<span class="v-check-detail">"#);
                    crate::render::text(cx, &d);
                    crate::render::markup(cx, r#"</span>"#);
                }
                crate::render::markup(cx, r#"</li>"#);
            }
            crate::render::markup(cx, r#"</ul>"#);
        }
        crate::render::markup(cx, r#"<div class="v-explain-result">"#);
        self.decision_html().render(cx);
        crate::render::markup(cx, r#"<p class="v-explain-reason">"#);
        crate::render::text(cx, &self.reason);
        crate::render::markup(cx, r#"</p></div></div>"#);
    }
}
impl DecisionExplainer {
    pub fn new(checks: Vec<Check>, allow: bool, reason: impl Into<String>) -> Self {
        Self {
            checks,
            allow,
            reason: reason.into(),
            technical: false,
        }
    }
    pub fn technical(mut self) -> Self {
        self.technical = true;
        self
    }
    fn decision_html(&self) -> Html {
        let b = DecisionBadge::new(self.allow);
        if self.technical { b.technical() } else { b }.html()
    }
}
impl Component for DecisionExplainer {}

#[derive(Debug, Clone)]
pub struct DecisionCard {
    pub allow: bool,
    pub who: String,
    pub target: String,
    pub policy: Option<String>,
    pub when: String,
    pub technical: bool,
}

impl Render for DecisionCard {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            "<article class=\"v-card v-decision-card\"><header class=\"v-card-head\">",
        );
        self.decision_html().render(cx);
        crate::render::markup(cx, r#"<time class="v-card-meta">"#);
        crate::render::text(cx, &self.when);
        crate::render::markup(
            cx,
            "</time></header><p class=\"v-decision-line\"><span class=\"v-decision-who\">",
        );
        crate::render::text(cx, &self.who);
        crate::render::markup(
            cx,
            r#"</span> <span aria-hidden="true">→</span> <span class="v-sr">to</span> <span class="v-decision-target">"#,
        );
        crate::render::text(cx, &self.target);
        crate::render::markup(cx, r#"</span></p>"#);
        if let Some(p) = &self.policy {
            crate::render::markup(cx, r#"<p class="v-card-meta">Policy "#);
            crate::render::text(cx, &p);
            crate::render::markup(cx, r#"</p>"#);
        }
        crate::render::markup(cx, r#"</article>"#);
    }
}
impl DecisionCard {
    pub fn new(
        allow: bool,
        who: impl Into<String>,
        target: impl Into<String>,
        when: impl Into<String>,
    ) -> Self {
        Self {
            allow,
            who: who.into(),
            target: target.into(),
            policy: None,
            when: when.into(),
            technical: false,
        }
    }
    pub fn policy(mut self, policy: impl Into<String>) -> Self {
        self.policy = Some(policy.into());
        self
    }
    pub fn technical(mut self) -> Self {
        self.technical = true;
        self
    }
    fn decision_html(&self) -> Html {
        let b = DecisionBadge::new(self.allow);
        if self.technical { b.technical() } else { b }.html()
    }
}
impl Component for DecisionCard {}

#[derive(Debug, Clone)]
pub struct AuditEventCard {
    pub card: DecisionCard,
    pub reason: String,
    pub explainer: Option<DecisionExplainer>,
    pub technical: bool,
}

impl Render for AuditEventCard {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<div class="v-audit-event">"#);
        self.card_html().render(cx);
        crate::render::markup(cx, r#"<p class="v-audit-reason">"#);
        crate::render::text(cx, &self.reason);
        crate::render::markup(cx, r#"</p>"#);
        if let Some(e) = &self.explainer_html() {
            crate::render::markup(
                cx,
                r#"<details class="v-audit-why"><summary>Why?</summary>"#,
            );
            e.render(cx);
            crate::render::markup(cx, r#"</details>"#);
        }
        crate::render::markup(cx, r#"</div>"#);
    }
}
impl AuditEventCard {
    pub fn new(card: DecisionCard, reason: impl Into<String>) -> Self {
        Self {
            card,
            reason: reason.into(),
            explainer: None,
            technical: false,
        }
    }
    pub fn explainer(mut self, e: DecisionExplainer) -> Self {
        self.explainer = Some(e);
        self
    }
    /// Audit's technical detail: ALLOW / DENY wording on the card and the explainer.
    pub fn technical(mut self) -> Self {
        self.technical = true;
        self
    }
    fn card_html(&self) -> Html {
        let c = self.card.clone();
        if self.technical { c.technical() } else { c }.html()
    }
    fn explainer_html(&self) -> Option<Html> {
        self.explainer
            .clone()
            .map(|e| if self.technical { e.technical() } else { e }.html())
    }
}
impl Component for AuditEventCard {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    fn granted() -> DecisionExplainer {
        DecisionExplainer::new(
            DIMENSIONS.iter().map(|d| Check::pass(*d)).collect(),
            true,
            "identity, application, destination and limits all match production-deploy",
        )
    }

    #[test]
    fn explainer_shows_a_glyph_and_a_word_per_check() {
        let s = granted().html().as_str().to_owned();
        assert_eq!(s.matches("v-check-pass").count(), 5);
        assert!(s.matches("✓").count() >= 5);
        for d in DIMENSIONS {
            assert!(s.contains(d), "{d}");
        }
        assert!(s.contains("ACCESS GRANTED") && s.contains("all match production-deploy"));
    }

    #[test]
    fn a_failed_check_reads_as_failed_and_carries_its_detail() {
        let e = DecisionExplainer::new(
            vec![
                Check::pass("Identity"),
                Check::fail("Destination").detail("db.staging:5432 is not in this policy"),
            ],
            false,
            "destination not allowed",
        );
        let s = e.html().as_str().to_owned();
        assert!(
            s.contains("v-check-fail")
                && s.contains("v-denial-mark")
                && s.contains("did not match")
        );
        assert!(s.contains("db.staging:5432 is not in this policy") && s.contains("ACCESS DENIED"));
    }

    #[test]
    fn an_explainer_with_no_checks_still_states_the_decision() {
        let s = DecisionExplainer::new(vec![], false, "no policy applies")
            .html()
            .as_str()
            .to_owned();
        assert!(
            s.contains("ACCESS DENIED") && s.contains("no policy applies") && !s.contains("<li")
        );
    }

    #[test]
    fn decision_card_lists_who_target_policy_and_time() {
        let s = DecisionCard::new(true, "acme/widget", "db.prod:5432", "2026-09-29 14:02 UTC")
            .policy("production-deploy")
            .html()
            .as_str()
            .to_owned();
        for v in [
            "acme/widget",
            "db.prod:5432",
            "production-deploy",
            "2026-09-29 14:02 UTC",
            "ACCESS GRANTED",
        ] {
            assert!(s.contains(v), "{v}");
        }
        assert!(!DecisionCard::new(false, "a", "b", "c")
            .html()
            .as_str()
            .contains("Policy"));
    }

    #[test]
    fn audit_card_expands_to_the_explainer_and_can_use_technical_wording() {
        let card = DecisionCard::new(true, "acme/widget", "db.prod:5432", "14:02");
        let plain = AuditEventCard::new(card.clone(), "matched production-deploy")
            .html()
            .as_str()
            .to_owned();
        assert!(!plain.contains("<details"));
        let full = AuditEventCard::new(card.clone(), "matched production-deploy")
            .explainer(granted())
            .technical()
            .html()
            .as_str()
            .to_owned();
        assert!(full.contains("<details") && full.contains("v-check-pass"));
        assert!(full.contains("ALLOW"));
    }

    #[test]
    fn audit_technical_wording_does_not_depend_on_call_order() {
        let card = DecisionCard::new(true, "a", "b", "c");
        let s = AuditEventCard::new(card, "r")
            .technical()
            .explainer(granted())
            .html()
            .as_str()
            .to_owned();
        assert!(s.contains("ALLOW") && !s.contains("ACCESS"), "{s}");
    }

    #[test]
    fn decision_components_escape_their_strings() {
        let s = DecisionCard::new(true, "<script>", "\"><img>", "t")
            .html()
            .as_str()
            .to_owned();
        assert!(!s.contains("<script>") && !s.contains("<img>"));
    }

    #[test]
    fn a_narrated_check_reads_as_a_sentence_with_a_hidden_state_word() {
        let e = DecisionExplainer::new(
            vec![
                Check::step("Identity is governed by policy `production-db`.", true),
                Check::step("No rule allows curl to db.prod:5432.", false),
            ],
            false,
            "No matching allow rule.",
        );
        let h = e.html();
        let s = h.as_str();
        assert!(
            s.contains("Identity is governed by policy `production-db`."),
            "{s}"
        );
        assert!(!s.contains("matched"), "{s}");
        assert!(!s.contains("did not match"), "{s}");
        assert!(s.contains("<span class=\"v-sr\">passed</span>"), "{s}");
        assert!(s.contains("<span class=\"v-sr\">failed</span>"), "{s}");
        assert_eq!(s.matches("<li class=\"v-check ").count(), 2);
    }

    #[test]
    fn a_dimension_check_still_shows_its_word() {
        let e = DecisionExplainer::new(vec![Check::pass("Identity")], true, "ok");
        assert!(e.html().as_str().contains("matched"));
    }

    #[test]
    fn a_narrated_check_with_an_empty_sentence_still_renders_one_item() {
        let e = DecisionExplainer::new(vec![Check::step("", true)], true, "ok");
        assert_eq!(e.html().as_str().matches("<li class=\"v-check ").count(), 1);
    }
}
