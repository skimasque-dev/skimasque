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
        }
    }
    pub fn tone(self) -> Tone {
        match self {
            Status::Allow | Status::Active => Tone::Active,
            Status::Deny | Status::Blocked => Tone::Deny,
            Status::Expired => Tone::Neutral,
            Status::Pending => Tone::Info,
        }
    }
}

#[derive(Template)]
#[template(path = "status.html")]
pub struct StatusBadge {
    pub status: Status,
}
impl crate::Component for StatusBadge {}

#[derive(Template)]
#[template(path = "decision_badge.html")]
pub struct DecisionBadge {
    pub allow: bool,
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
    fn decision_badges_say_granted_or_denied_in_words() {
        assert_eq!(
            DecisionBadge { allow: true }.html().as_str(),
            r#"<span class="v-decision v-tone-active"><span aria-hidden="true">✓</span> ACCESS GRANTED</span>"#
        );
        assert_eq!(
            DecisionBadge { allow: false }.html().as_str(),
            r#"<span class="v-decision v-tone-deny"><span aria-hidden="true">×</span> ACCESS DENIED</span>"#
        );
    }
}
