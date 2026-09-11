//! The audit log: a structured record of every policy decision.
//!
//! [`PolicyLayer`](crate::service::PolicyLayer), when given a sink with
//! [`with_audit`](crate::service::PolicyLayer::with_audit), emits one
//! [`AuditEvent`] for every tunnel it allows or denies -- who asked, what
//! application, where to, which rule decided, and when. This is the trail a
//! deployment keeps for compliance, distinct from the operational `tracing`
//! output: it is append-only, one JSON object per line, and it records the
//! *policy decision*, not whether the tunnel then opened (a later quota,
//! resolution or address-floor failure leaves an `allow` here with no tunnel).
//!
//! Two sinks ship with the crate: [`JsonlAuditSink`] appends JSON Lines to any
//! writer (a file, most often), and [`TracingAuditSink`] emits each event on
//! the `masque::audit` [`tracing`] target for a subscriber to route. A
//! deployment that needs both, or a different destination entirely, implements
//! [`AuditSink`].

use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use skimasque_policy::{Decision, WorkloadIdentity};

/// One audited policy decision.
///
/// Serialised as a flat JSON object; identity fields that a source did not
/// populate are omitted, as are the fields that do not apply to the outcome
/// (`rule` on a deny, `reason` / `suggested_rule` on an allow).
#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    /// When the decision was made, as an RFC 3339 UTC timestamp with
    /// milliseconds (`2026-09-07T14:03:11.482Z`).
    pub timestamp: String,
    /// `"allow"` or `"deny"`.
    pub decision: &'static str,
    /// The MASQUE protocol the tunnel used: `connect-udp` or `connect-tcp`.
    pub protocol: &'static str,
    /// The application the client named in `x-masque-application`. Empty when
    /// the client sent none.
    pub application: String,
    /// The `host:port` the tunnel was asked for, before DNS.
    pub destination: String,
    /// The client address as the gateway saw it.
    pub client: String,
    /// WHO -- the verified workload identity. Unset fields are omitted.
    pub identity: WorkloadIdentity,
    /// The policy that decided, if one matched the identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    /// On an allow: the rule that permitted the tunnel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// On a deny: the one-line reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// On a deny: the rule text that would have allowed the tunnel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_rule: Option<String>,
}

impl AuditEvent {
    /// Build an event from a policy [`Decision`] and the request it answered.
    /// The timestamp is taken now, from the system clock.
    pub fn from_decision(
        decision: &Decision,
        protocol: &'static str,
        application: impl Into<String>,
        destination: impl Into<String>,
        client: impl Into<String>,
        identity: &WorkloadIdentity,
    ) -> Self {
        let common = |decision: &'static str| Self {
            timestamp: rfc3339(SystemTime::now()),
            decision,
            protocol,
            application: String::new(),
            destination: String::new(),
            client: String::new(),
            identity: identity.clone(),
            policy: None,
            rule: None,
            reason: None,
            suggested_rule: None,
        };

        let mut event = match decision {
            Decision::Allow(allowed) => Self {
                policy: Some(allowed.policy.clone()),
                rule: Some(allowed.rule.clone()),
                ..common("allow")
            },
            Decision::Deny(denied) => Self {
                policy: denied.policy.clone(),
                reason: Some(denied.reason.summary()),
                suggested_rule: Some(denied.suggested_rule.clone()),
                ..common("deny")
            },
        };
        event.application = application.into();
        event.destination = destination.into();
        event.client = client.into();
        event
    }
}

/// Where audited decisions go.
///
/// [`record`](Self::record) must not block the caller for long and must not
/// panic: it runs inline on the request path, once per tunnel decision. A sink
/// that cannot persist an event should log the failure and return, not
/// propagate it -- a broken audit destination denies no tunnels, it just leaves
/// a gap the operator has to notice.
pub trait AuditSink: Send + Sync + std::fmt::Debug {
    fn record(&self, event: &AuditEvent);
}

/// Appends each event to a writer as one line of JSON.
///
/// The writer is flushed after every event, so a crash loses at most the
/// decision in flight. Wrap a [`std::fs::File`] opened for append; a
/// `BufWriter` is counter-productive here, since every line is flushed anyway.
pub struct JsonlAuditSink<W> {
    writer: Mutex<W>,
}

impl<W: Write + Send> JsonlAuditSink<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer: Mutex::new(writer),
        }
    }

    /// Consume the sink and recover its writer.
    pub fn into_inner(self) -> W {
        self.writer.into_inner().unwrap_or_else(|e| e.into_inner())
    }
}

impl<W> std::fmt::Debug for JsonlAuditSink<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonlAuditSink").finish_non_exhaustive()
    }
}

impl<W: Write + Send> AuditSink for JsonlAuditSink<W> {
    fn record(&self, event: &AuditEvent) {
        let mut line = match serde_json::to_vec(event) {
            Ok(line) => line,
            Err(error) => {
                tracing::error!(%error, "could not serialise an audit event");
                return;
            }
        };
        line.push(b'\n');

        let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(error) = writer.write_all(&line).and_then(|()| writer.flush()) {
            tracing::error!(%error, "could not write an audit event");
        }
    }
}

/// Emits each event on the `masque::audit` [`tracing`] target.
///
/// The default sink: it needs no configuration and lets a subscriber decide
/// where the record lands. `skimasque-server` selects it when `--audit-log` is
/// not given, so `RUST_LOG=masque::audit=info` (or `-v`) surfaces decisions.
#[derive(Debug, Default, Clone, Copy)]
pub struct TracingAuditSink;

impl AuditSink for TracingAuditSink {
    fn record(&self, event: &AuditEvent) {
        tracing::info!(
            target: "masque::audit",
            timestamp = %event.timestamp,
            decision = event.decision,
            protocol = event.protocol,
            application = %event.application,
            destination = %event.destination,
            client = %event.client,
            organization = event.identity.organization.as_deref().unwrap_or_default(),
            repository = event.identity.repository.as_deref().unwrap_or_default(),
            workflow = event.identity.workflow.as_deref().unwrap_or_default(),
            git_ref = event.identity.git_ref.as_deref().unwrap_or_default(),
            environment = event.identity.environment.as_deref().unwrap_or_default(),
            actor = event.identity.actor.as_deref().unwrap_or_default(),
            policy = event.policy.as_deref().unwrap_or_default(),
            rule = event.rule.as_deref().unwrap_or_default(),
            reason = event.reason.as_deref().unwrap_or_default(),
            suggested_rule = event.suggested_rule.as_deref().unwrap_or_default(),
            "policy decision",
        );
    }
}

/// Format a [`SystemTime`] as RFC 3339 UTC with milliseconds.
///
/// Hand-rolled to keep a date library out of the dependency tree for one
/// timestamp: the civil-date conversion is Howard Hinnant's `days -> y/m/d`
/// algorithm. Times before the Unix epoch are clamped to it.
fn rfc3339(time: SystemTime) -> String {
    let since_epoch = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since_epoch.as_secs();
    let millis = since_epoch.subsec_millis();

    let days = (secs / 86_400) as i64;
    let day_secs = secs % 86_400;
    let (hour, minute, second) = (day_secs / 3600, (day_secs % 3600) / 60, day_secs % 60);

    // days since 1970-01-01 -> proleptic Gregorian year / month / day.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // day of era, [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year, [0, 365]
    let mp = (5 * doy + 2) / 153; // month, shifted so March = 0
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use skimasque_policy::RequestContext;

    fn decision(toml: &str, application: &str, destination: &str) -> Decision {
        let set = skimasque_policy::PolicySet::from_documents([("p.toml", toml)]).unwrap();
        set.evaluate(&RequestContext {
            workload: WorkloadIdentity::default(),
            application: application.to_owned(),
            transport: skimasque_policy::Transport::Tcp,
            destination: skimasque_policy::Destination::parse(destination).unwrap(),
        })
    }

    const ALLOW_TF: &str = r#"
        name = "prod"
        [[rules]]
        id = "tf-api"
        application = "terraform"
        action = "allow"
        destinations = ["api.production.example.com:443"]
    "#;

    #[test]
    fn the_epoch_is_the_start_of_1970() {
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn a_known_instant_formats_as_expected() {
        let t = UNIX_EPOCH + Duration::from_millis(1_700_000_000_482);
        assert_eq!(rfc3339(t), "2023-11-14T22:13:20.482Z");
    }

    #[test]
    fn a_time_before_the_epoch_clamps_rather_than_panics() {
        assert_eq!(
            rfc3339(UNIX_EPOCH - Duration::from_secs(1)),
            "1970-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn an_allow_event_carries_the_rule_and_no_denial_fields() {
        let event = AuditEvent::from_decision(
            &decision(ALLOW_TF, "terraform", "api.production.example.com:443"),
            "connect-udp",
            "terraform",
            "api.production.example.com:443",
            "203.0.113.1:9000",
            &WorkloadIdentity::default(),
        );
        assert_eq!(event.decision, "allow");
        assert_eq!(event.policy.as_deref(), Some("prod"));
        assert_eq!(event.rule.as_deref(), Some("tf-api"));
        assert!(event.reason.is_none());
        assert!(event.suggested_rule.is_none());
    }

    #[test]
    fn a_deny_event_carries_the_reason_and_the_suggested_rule() {
        let event = AuditEvent::from_decision(
            &decision(ALLOW_TF, "terraform", "evil.example.com:443"),
            "connect-udp",
            "terraform",
            "evil.example.com:443",
            "203.0.113.1:9000",
            &WorkloadIdentity::default(),
        );
        assert_eq!(event.decision, "deny");
        assert!(event.rule.is_none());
        assert_eq!(event.reason.as_deref(), Some("No matching allow rule."));
        assert_eq!(
            event.suggested_rule.as_deref(),
            Some("allow terraform evil.example.com:443")
        );
    }

    #[test]
    fn the_jsonl_sink_writes_one_object_per_line_with_empty_fields_dropped() {
        let sink = JsonlAuditSink::new(Vec::<u8>::new());
        sink.record(&AuditEvent::from_decision(
            &decision(ALLOW_TF, "terraform", "api.production.example.com:443"),
            "connect-udp",
            "terraform",
            "api.production.example.com:443",
            "203.0.113.1:9000",
            &WorkloadIdentity::default(),
        ));
        sink.record(&AuditEvent::from_decision(
            &decision(ALLOW_TF, "terraform", "evil.example.com:443"),
            "connect-udp",
            "terraform",
            "evil.example.com:443",
            "203.0.113.1:9000",
            &WorkloadIdentity::default(),
        ));

        let written = String::from_utf8(sink.into_inner()).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 2);

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["decision"], "allow");
        assert_eq!(first["rule"], "tf-api");
        assert!(first.get("reason").is_none());
        // An unset identity serialises to `{}`, not a wall of nulls.
        assert_eq!(first["identity"], serde_json::json!({}));

        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["decision"], "deny");
        assert_eq!(second["reason"], "No matching allow rule.");
    }
}
