//! Status words and decision badges. A dot is never shown without its word.

use askama::Template;

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

#[derive(Template, Debug, Clone)]
#[template(path = "status.html")]
pub struct StatusBadge {
    pub status: Status,
}
impl crate::Component for StatusBadge {}

#[derive(Template, Debug, Clone)]
#[template(path = "decision_badge.html")]
pub struct DecisionBadge {
    pub allow: bool,
    /// Policy-editor wording (ALLOW / DENY) instead of customer wording.
    pub technical: bool,
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
