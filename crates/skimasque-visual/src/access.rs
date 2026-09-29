//! Access components: sessions, gateways, health, identities.

use askama::Template;

use crate::{Component, Html, Planned, Status, StatusBadge, Tone};

#[derive(Template, Debug, Clone)]
#[template(path = "session_card.html")]
pub struct SessionCard {
    pub identity: String,
    pub target: String,
    pub gateway: String,
    pub status: Status,
    pub remaining_pct: u32,
    pub remaining_label: String,
}
impl SessionCard {
    pub fn new(
        identity: impl Into<String>,
        target: impl Into<String>,
        gateway: impl Into<String>,
        status: Status,
        remaining_pct: u32,
        remaining_label: impl Into<String>,
    ) -> Self {
        Self {
            identity: identity.into(),
            target: target.into(),
            gateway: gateway.into(),
            status,
            remaining_pct,
            remaining_label: remaining_label.into(),
        }
    }
    fn pct(&self) -> u32 {
        self.remaining_pct.min(100)
    }
    fn status_html(&self) -> Html {
        StatusBadge {
            status: self.status,
        }
        .html()
    }
}
impl Component for SessionCard {}

#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub time: String,
    pub label: String,
    pub tone: Tone,
}
impl TimelineEvent {
    pub fn new(time: impl Into<String>, label: impl Into<String>, tone: Tone) -> Self {
        Self {
            time: time.into(),
            label: label.into(),
            tone,
        }
    }
}

#[derive(Template, Debug, Clone)]
#[template(path = "session_timeline.html")]
pub struct SessionTimeline {
    pub events: Vec<TimelineEvent>,
}
impl SessionTimeline {
    pub fn new(events: Vec<TimelineEvent>) -> Self {
        Self { events }
    }
}
impl Component for SessionTimeline {}

#[derive(Template, Debug, Clone)]
#[template(path = "gateway_card.html")]
pub struct GatewayCard {
    pub name: String,
    pub region: String,
    pub status: Status,
    pub last_heartbeat: String,
    pub sessions: u32,
    pub egress_ip: Option<String>,
}
impl GatewayCard {
    pub fn new(
        name: impl Into<String>,
        region: impl Into<String>,
        status: Status,
        last_heartbeat: impl Into<String>,
        sessions: u32,
    ) -> Self {
        Self {
            name: name.into(),
            region: region.into(),
            status,
            last_heartbeat: last_heartbeat.into(),
            sessions,
            egress_ip: None,
        }
    }
    pub fn egress_ip(mut self, ip: impl Into<String>) -> Self {
        self.egress_ip = Some(ip.into());
        self
    }
    fn status_html(&self) -> Html {
        StatusBadge {
            status: self.status,
        }
        .html()
    }
    fn planned_html(&self) -> Html {
        Planned::new()
            .note("per-gateway egress IP is not available yet")
            .html()
    }
}
impl Component for GatewayCard {}

#[derive(Template, Debug, Clone)]
#[template(path = "health_card.html")]
pub struct HealthCard {
    pub title: String,
    pub status: Status,
    pub detail: String,
}
impl HealthCard {
    pub fn new(title: impl Into<String>, status: Status, detail: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            status,
            detail: detail.into(),
        }
    }
    fn status_html(&self) -> Html {
        StatusBadge {
            status: self.status,
        }
        .html()
    }
}
impl Component for HealthCard {}

#[derive(Template, Debug, Clone)]
#[template(path = "identity_card.html")]
pub struct IdentityCard {
    pub name: String,
    pub source: String,
    pub last_seen: Option<String>,
    pub policies: u32,
}
impl IdentityCard {
    pub fn new(name: impl Into<String>, source: impl Into<String>, policies: u32) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
            last_seen: None,
            policies,
        }
    }
    pub fn last_seen(mut self, when: impl Into<String>) -> Self {
        self.last_seen = Some(when.into());
        self
    }
    fn policy_count(&self) -> String {
        match self.policies {
            1 => "1 policy".to_owned(),
            n => format!("{n} policies"),
        }
    }
}
impl Component for IdentityCard {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Status, Tone};

    #[test]
    fn session_card_shows_status_word_target_and_remaining_time() {
        let s = SessionCard::new(
            "acme/widget",
            "db.prod:5432",
            "us-west",
            Status::Active,
            40,
            "12 min left",
        )
        .html()
        .as_str()
        .to_owned();
        for v in [
            "acme/widget",
            "db.prod:5432",
            "us-west",
            "ACTIVE",
            "12 min left",
        ] {
            assert!(s.contains(v), "{v}");
        }
        assert!(
            s.contains(r#"role="progressbar""#)
                && s.contains(r#"aria-valuenow="40""#)
                && s.contains("width: 40%")
        );
    }

    #[test]
    fn the_remaining_time_bar_clamps() {
        let over = SessionCard::new("a", "b", "c", Status::Active, 400, "x")
            .html()
            .as_str()
            .to_owned();
        assert!(over.contains("width: 100%") && over.contains(r#"aria-valuenow="100""#));
        let none = SessionCard::new("a", "b", "c", Status::Expired, 0, "expired")
            .html()
            .as_str()
            .to_owned();
        assert!(none.contains("width: 0%") && none.contains("EXPIRED") && none.contains("expired"));
    }

    #[test]
    fn timeline_lists_events_in_order_with_their_words() {
        let t = SessionTimeline::new(vec![
            TimelineEvent::new("14:02:01", "Requested", Tone::Neutral),
            TimelineEvent::new("14:02:01", "Authorized", Tone::Active),
            TimelineEvent::new("14:22:01", "Expires", Tone::Neutral),
        ])
        .html()
        .as_str()
        .to_owned();
        assert!(t.find("Requested").unwrap() < t.find("Authorized").unwrap());
        assert!(t.find("Authorized").unwrap() < t.find("Expires").unwrap());
        assert!(t.contains("<ol") && t.contains("v-tone-active") && t.contains("14:22:01"));
        assert!(SessionTimeline::new(vec![])
            .html()
            .as_str()
            .contains("No events"));
    }

    #[test]
    fn gateway_egress_ip_is_planned_unless_known() {
        let g = GatewayCard::new("gw-us-west", "us-west-2", Status::Healthy, "12 s ago", 3)
            .html()
            .as_str()
            .to_owned();
        assert!(g.contains("HEALTHY") && g.contains("12 s ago") && g.contains("gw-us-west"));
        assert!(g.contains("Egress IP") && g.contains("PLANNED"));
        let k = GatewayCard::new("g", "r", Status::Degraded, "9 min ago", 0)
            .egress_ip("203.0.113.7")
            .html()
            .as_str()
            .to_owned();
        assert!(k.contains("203.0.113.7") && !k.contains("PLANNED") && k.contains("DEGRADED"));
    }

    #[test]
    fn health_and_identity_cards_carry_words_and_escape() {
        let h = HealthCard::new("Control plane", Status::Offline, "no heartbeat for 10 min")
            .html()
            .as_str()
            .to_owned();
        assert!(h.contains("OFFLINE") && h.contains("no heartbeat"));
        let i = IdentityCard::new("acme/widget", "GitHub Actions", 2)
            .last_seen("2 h ago")
            .html()
            .as_str()
            .to_owned();
        assert!(i.contains("GitHub Actions") && i.contains("2 h ago") && i.contains("2 policies"));
        assert!(IdentityCard::new("x", "y", 1)
            .html()
            .as_str()
            .contains("1 policy"));
        assert!(IdentityCard::new("x", "y", 0)
            .html()
            .as_str()
            .contains("never seen"));
        let e = IdentityCard::new("<script>", "<b>", 0)
            .html()
            .as_str()
            .to_owned();
        assert!(!e.contains("<script>") && !e.contains("<b>"));
    }
}
