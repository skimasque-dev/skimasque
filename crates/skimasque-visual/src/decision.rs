//! Decision components: why access was granted or denied.

use askama::Template;

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
}
impl Check {
    pub fn pass(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            pass: true,
            detail: None,
        }
    }
    pub fn fail(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            pass: false,
            detail: None,
        }
    }
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Template, Debug, Clone)]
#[template(path = "decision_explainer.html")]
pub struct DecisionExplainer {
    pub checks: Vec<Check>,
    pub allow: bool,
    pub reason: String,
    pub technical: bool,
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

#[derive(Template, Debug, Clone)]
#[template(path = "decision_card.html")]
pub struct DecisionCard {
    pub allow: bool,
    pub who: String,
    pub target: String,
    pub policy: Option<String>,
    pub when: String,
    pub technical: bool,
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

#[derive(Template, Debug, Clone)]
#[template(path = "audit_event_card.html")]
pub struct AuditEventCard {
    pub card: DecisionCard,
    pub reason: String,
    pub explainer: Option<DecisionExplainer>,
}
impl AuditEventCard {
    pub fn new(card: DecisionCard, reason: impl Into<String>) -> Self {
        Self {
            card,
            reason: reason.into(),
            explainer: None,
        }
    }
    pub fn explainer(mut self, e: DecisionExplainer) -> Self {
        self.explainer = Some(e);
        self
    }
    /// Audit's technical detail: ALLOW / DENY wording on the card and the explainer.
    pub fn technical(mut self) -> Self {
        self.card = self.card.technical();
        self.explainer = self.explainer.map(DecisionExplainer::technical);
        self
    }
    fn card_html(&self) -> Html {
        self.card.html()
    }
    fn explainer_html(&self) -> Option<Html> {
        self.explainer.as_ref().map(|e| e.html())
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
        assert!(s.contains("v-check-fail") && s.contains("×") && s.contains("did not match"));
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
    fn decision_components_escape_their_strings() {
        let s = DecisionCard::new(true, "<script>", "\"><img>", "t")
            .html()
            .as_str()
            .to_owned();
        assert!(!s.contains("<script>") && !s.contains("<img>"));
    }
}
