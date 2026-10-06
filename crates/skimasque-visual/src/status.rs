//! Status words and decision badges. A dot is never shown without its word.

use stucco_core::Render;

use crate::Tone;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Allow,
    Deny,
    Active,
    Expired,
    Pending,
    Blocked,
    Granted,
    Denied,
    Healthy,
    Degraded,
    Offline,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Allow => "ALLOW",
            Status::Deny => "DENY",
            Status::Active => "ACTIVE",
            Status::Expired => "EXPIRED",
            Status::Pending => "PENDING",
            Status::Blocked => "BLOCKED",
            Status::Granted => "ACCESS GRANTED",
            Status::Denied => "ACCESS DENIED",
            Status::Healthy => "HEALTHY",
            Status::Degraded => "DEGRADED",
            Status::Offline => "OFFLINE",
        }
    }
    pub fn tone(self) -> Tone {
        match self {
            Status::Allow | Status::Active | Status::Granted | Status::Healthy => Tone::Active,
            Status::Deny | Status::Blocked | Status::Denied => Tone::Deny,
            Status::Expired | Status::Offline => Tone::Neutral,
            Status::Pending => Tone::Info,
            Status::Degraded => Tone::Warning,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StatusBadge {
    pub status: Status,
}

impl Render for StatusBadge {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<span class="v-status v-tone-"#);
        crate::render::text(cx, &(self.status.tone().class()));
        crate::render::markup(cx, r#""><span aria-hidden="true">●</span> "#);
        crate::render::text(cx, &self.status.word());
        crate::render::markup(cx, r#"</span>"#);
    }
}
impl crate::Component for StatusBadge {}

#[derive(Debug, Clone)]
pub struct DecisionBadge {
    pub allow: bool,
    /// Policy-editor wording (ALLOW / DENY) instead of customer wording.
    pub technical: bool,
}

impl Render for DecisionBadge {
    fn render(&self, cx: &mut stucco_core::Cx) {
        if self.allow {
            crate::render::markup(
                cx,
                "<span class=\"v-decision v-tone-active\"><span aria-hidden=\"true\">✓</span> ",
            );
            crate::render::text(cx, &self.word());
            crate::render::markup(cx, r#"</span>"#);
        } else {
            crate::render::markup(
                cx,
                r#"<span class="v-decision v-tone-deny"><span aria-hidden="true"><svg class="v-denial-mark" viewBox="0 0 12 12" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true" focusable="false"><path d="M3 3 L9 9 M9 3 L3 9"/></svg>
</span> "#,
            );
            crate::render::text(cx, &self.word());
            crate::render::markup(cx, r#"</span>"#);
        }
    }
}

impl DecisionBadge {
    pub fn new(allow: bool) -> Self {
        Self {
            allow,
            technical: false,
        }
    }
    pub fn technical(mut self) -> Self {
        self.technical = true;
        self
    }
    fn word(&self) -> &'static str {
        match (self.allow, self.technical) {
            (true, false) => "ACCESS GRANTED",
            (false, false) => "ACCESS DENIED",
            (true, true) => "ALLOW",
            (false, true) => "DENY",
        }
    }
}
impl crate::Component for DecisionBadge {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    #[test]
    fn every_status_carries_its_word_beside_the_dot() {
        for (s, word, tone) in [
            (Status::Allow, "ALLOW", "active"),
            (Status::Deny, "DENY", "deny"),
            (Status::Active, "ACTIVE", "active"),
            (Status::Expired, "EXPIRED", "neutral"),
            (Status::Pending, "PENDING", "info"),
            (Status::Blocked, "BLOCKED", "deny"),
            (Status::Granted, "ACCESS GRANTED", "active"),
            (Status::Denied, "ACCESS DENIED", "deny"),
            (Status::Healthy, "HEALTHY", "active"),
            (Status::Degraded, "DEGRADED", "warning"),
            (Status::Offline, "OFFLINE", "neutral"),
        ] {
            let html = StatusBadge { status: s }.html().as_str().to_owned();
            assert_eq!(
                html,
                format!(
                    r#"<span class="v-status v-tone-{tone}"><span aria-hidden="true">●</span> {word}</span>"#
                )
            );
        }
    }

    #[test]
    fn decision_badges_use_customer_wording_unless_technical() {
        let g = DecisionBadge::new(true).html();
        assert!(g.as_str().contains("✓") && g.as_str().contains("ACCESS GRANTED"));
        let d = DecisionBadge::new(false).html();
        assert!(d.as_str().contains("v-denial-mark") && d.as_str().contains("ACCESS DENIED"));
        assert!(d.as_str().contains("v-tone-deny"));
        let a = DecisionBadge::new(true).technical().html();
        assert!(a.as_str().contains("ALLOW") && !a.as_str().contains("ACCESS"));
        let n = DecisionBadge::new(false).technical().html();
        assert!(n.as_str().contains("DENY") && !n.as_str().contains("ACCESS"));
    }
}
