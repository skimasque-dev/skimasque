//! Evaluating a request against a policy.
//!
//! [`Policy::evaluate`] is a pure function: same context, same [`Decision`],
//! every time, with no I/O. It assumes the identity has already been matched --
//! either because the caller looked the policy up by name (`masque policy
//! check production ...`) or because [`PolicySet::evaluate`] selected it. What
//! it does here is walk the rules in order and apply the first that speaks to
//! both the application and the destination.
//!
//! Deny is the default and the fallthrough. A [`Denied`] result carries a
//! machine-readable [`DenyReason`], a suggested rule that would have allowed the
//! request, and the closest existing rules, so the CLI can print the
//! self-explaining denial the product calls for.

use crate::destination::Destination;
use crate::identity::WorkloadIdentity;
use crate::model::{Action, EgressSpec, Limits, Policy, PolicySet, Rule, SessionSpec, Transport};

/// Everything an evaluation needs: WHO, WHAT and WHERE.
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// WHO -- the (ideally verified) workload identity.
    pub workload: WorkloadIdentity,
    /// WHAT -- the application name. Treated as session context: the engine
    /// matches on it, but it is only as trustworthy as whatever supplied it.
    pub application: String,
    /// HOW -- the transport the tunnel carries (TCP or UDP).
    pub transport: Transport,
    /// WHERE -- the destination the client named, before DNS.
    pub destination: Destination,
}

/// The outcome of an evaluation.
#[derive(Debug, Clone)]
pub enum Decision {
    Allow(Allowed),
    Deny(Denied),
}

impl Decision {
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow(_))
    }

    pub fn is_deny(&self) -> bool {
        matches!(self, Self::Deny(_))
    }

    /// The [`Action`] this decision corresponds to, for comparing against a
    /// policy test's expectation.
    pub fn action(&self) -> Action {
        match self {
            Self::Allow(_) => Action::Allow,
            Self::Deny(_) => Action::Deny,
        }
    }
}

/// An allowed request, and the terms it is allowed under.
#[derive(Debug, Clone)]
pub struct Allowed {
    pub policy: String,
    /// The rule that allowed it: its `id` if it has one, else `rule #<n>`.
    pub rule: String,
    pub egress: Option<EgressSpec>,
    pub limits: Limits,
    pub session: SessionSpec,
}

/// A denied request, with enough context to explain and fix it.
#[derive(Debug, Clone)]
pub struct Denied {
    /// The policy that was consulted, if one was.
    pub policy: Option<String>,
    pub reason: DenyReason,
    /// The rule text that would have allowed this request:
    /// `allow terraform registry.terraform.io:443`.
    pub suggested_rule: String,
    /// Existing allow-rule destinations most like the one requested, nearest
    /// first, for the "closest rules" hint. Empty when the policy has no allow
    /// rules for this application.
    pub closest: Vec<String>,
}

/// Why a request was denied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenyReason {
    /// No policy in the set matched the workload identity.
    NoPolicyMatch,
    /// A policy matched, but no rule allowed the destination.
    NoMatchingAllowRule,
    /// A `deny` rule matched the request explicitly.
    ExplicitDeny { rule: String },
}

impl DenyReason {
    /// A one-line explanation, as the denial message shows it.
    pub fn summary(&self) -> String {
        match self {
            Self::NoPolicyMatch => "No policy matches this workload identity.".into(),
            Self::NoMatchingAllowRule => "No matching allow rule.".into(),
            Self::ExplicitDeny { rule } => format!("Denied by rule {rule}."),
        }
    }
}

impl Policy {
    /// Whether this policy's match accepts `identity`.
    pub fn identity_matches(&self, identity: &WorkloadIdentity) -> bool {
        self.match_spec.matches(identity)
    }

    /// Evaluate `ctx` against this policy's rules. Does not check the identity.
    pub fn evaluate(&self, ctx: &RequestContext) -> Decision {
        for (index, rule) in self.rules.iter().enumerate() {
            if !rule.covers(&ctx.application, ctx.transport) {
                continue;
            }
            if !rule.destinations.iter().any(|d| d.matches(&ctx.destination)) {
                continue;
            }
            let label = rule_label(rule, index);
            return match rule.action {
                Action::Allow => Decision::Allow(Allowed {
                    policy: self.name.clone(),
                    rule: label,
                    egress: self.egress.clone(),
                    limits: self.limits.clone(),
                    session: self.session.clone(),
                }),
                Action::Deny => Decision::Deny(self.deny(ctx, DenyReason::ExplicitDeny { rule: label })),
            };
        }

        Decision::Deny(self.deny(ctx, DenyReason::NoMatchingAllowRule))
    }

    fn deny(&self, ctx: &RequestContext, reason: DenyReason) -> Denied {
        Denied {
            policy: Some(self.name.clone()),
            reason,
            suggested_rule: suggested_rule(ctx),
            closest: closest_destinations(&self.rules, ctx),
        }
    }
}

impl PolicySet {
    /// Select the policy for `ctx.workload` and evaluate against it.
    ///
    /// With no matching policy the result is [`DenyReason::NoPolicyMatch`]: the
    /// least-privilege default is that an unrecognised workload reaches
    /// nothing.
    pub fn evaluate(&self, ctx: &RequestContext) -> Decision {
        match self.select(&ctx.workload) {
            Some(policy) => policy.evaluate(ctx),
            None => Decision::Deny(Denied {
                policy: None,
                reason: DenyReason::NoPolicyMatch,
                suggested_rule: suggested_rule(ctx),
                closest: Vec::new(),
            }),
        }
    }

    /// Evaluate against the named policy specifically, as `masque policy check
    /// <name>` does. Returns `None` if the set has no such policy.
    pub fn evaluate_named(&self, name: &str, ctx: &RequestContext) -> Option<Decision> {
        self.get(name).map(|policy| policy.evaluate(ctx))
    }
}

fn rule_label(rule: &Rule, index: usize) -> String {
    rule.id.clone().unwrap_or_else(|| format!("rule #{index}"))
}

fn suggested_rule(ctx: &RequestContext) -> String {
    let app = if ctx.application.is_empty() { "<app>" } else { &ctx.application };
    // A `tcp` request needs no `transport` in the suggested rule -- the default
    // rule matches it -- but a `udp` request does.
    match ctx.transport {
        Transport::Tcp => format!("allow {app} {}", ctx.destination),
        Transport::Udp => format!("allow {app} udp {}", ctx.destination),
    }
}

/// Rank the allow-rule destinations for this application by how close they are
/// to the one requested, nearest first, and return the top few as text.
///
/// "Close" is deliberately simple: a shared port counts, and so does a shared
/// run of trailing domain labels (`api.production.example.com` and
/// `db.production.example.com` share `production.example.com`). It only has to
/// be good enough to point a developer at the rule they meant to match.
fn closest_destinations(rules: &[Rule], ctx: &RequestContext) -> Vec<String> {
    let mut scored: Vec<(i32, &str)> = rules
        .iter()
        .filter(|rule| {
            rule.action == Action::Allow && rule.covers(&ctx.application, ctx.transport)
        })
        .flat_map(|rule| rule.destinations.iter())
        .map(|spec| (proximity(spec.as_str(), &ctx.destination), spec.as_str()))
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored.dedup_by_key(|(_, text)| *text);
    scored.into_iter().take(3).map(|(_, text)| text.to_owned()).collect()
}

fn proximity(spec: &str, destination: &Destination) -> i32 {
    let want = destination.to_string();
    let (spec_host, spec_port) = spec.rsplit_once(':').unwrap_or((spec, ""));
    let (want_host, want_port) = want.rsplit_once(':').unwrap_or((want.as_str(), ""));

    let mut score = 0;
    if !spec_port.is_empty() && spec_port == want_port {
        score += 3;
    }
    let shared = spec_host
        .rsplit('.')
        .zip(want_host.rsplit('.'))
        .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
        .count();
    score += shared as i32 * 2;
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(application: &str, destination: &str) -> RequestContext {
        ctx_on(application, Transport::Tcp, destination)
    }

    fn ctx_on(application: &str, transport: Transport, destination: &str) -> RequestContext {
        RequestContext {
            workload: WorkloadIdentity::default(),
            application: application.into(),
            transport,
            destination: Destination::parse(destination).unwrap(),
        }
    }

    fn policy() -> Policy {
        Policy::from_toml(
            r#"
            name = "production"
            [session]
            max_duration = "20m"

            [[rules]]
            id = "tf-api"
            application = "terraform"
            action = "allow"
            destinations = [
                "api.production.example.com:443",
                "registry.terraform.io:443",
                "db.production.example.com:5432",
            ]

            [[rules]]
            id = "no-metadata"
            application = "*"
            action = "deny"
            destinations = ["169.254.169.254:80"]
        "#,
        )
        .unwrap()
    }

    #[test]
    fn an_allowed_request_reports_the_rule_and_the_limits() {
        let decision = policy().evaluate(&ctx("terraform", "api.production.example.com:443"));
        let Decision::Allow(allowed) = decision else {
            panic!("expected allow, got {decision:?}");
        };
        assert_eq!(allowed.policy, "production");
        assert_eq!(allowed.rule, "tf-api");
        assert_eq!(allowed.session.max_duration.unwrap().as_secs(), 1_200);
    }

    #[test]
    fn an_unlisted_destination_is_denied_by_default_with_a_suggested_rule() {
        let decision = policy().evaluate(&ctx("terraform", "google.com:443"));
        let Decision::Deny(denied) = decision else {
            panic!("expected deny");
        };
        assert_eq!(denied.reason, DenyReason::NoMatchingAllowRule);
        assert_eq!(denied.suggested_rule, "allow terraform google.com:443");
    }

    #[test]
    fn the_closest_rules_are_the_ones_sharing_a_domain_and_port() {
        let decision = policy().evaluate(&ctx("terraform", "cache.production.example.com:443"));
        let Decision::Deny(denied) = decision else {
            panic!("expected deny");
        };
        assert_eq!(
            denied.closest.first().map(String::as_str),
            Some("api.production.example.com:443"),
            "got {:?}",
            denied.closest
        );
    }

    #[test]
    fn an_explicit_deny_rule_wins_and_is_named() {
        let decision = policy().evaluate(&ctx("terraform", "169.254.169.254:80"));
        let Decision::Deny(denied) = decision else {
            panic!("expected deny");
        };
        assert_eq!(denied.reason, DenyReason::ExplicitDeny { rule: "no-metadata".into() });
    }

    #[test]
    fn a_wrong_application_does_not_match_the_allow_rule() {
        assert!(policy().evaluate(&ctx("kubectl", "api.production.example.com:443")).is_deny());
    }

    #[test]
    fn a_transport_scoped_rule_only_matches_that_transport() {
        let policy = Policy::from_toml(
            r#"
            name = "p"
            [[rules]]
            application = "dns"
            transport = "udp"
            action = "allow"
            destinations = ["1.1.1.1:53"]
        "#,
        )
        .unwrap();

        assert!(policy
            .evaluate(&ctx_on("dns", Transport::Udp, "1.1.1.1:53"))
            .is_allow());
        let Decision::Deny(denied) = policy.evaluate(&ctx_on("dns", Transport::Tcp, "1.1.1.1:53"))
        else {
            panic!("a tcp request must not match a udp-only rule");
        };
        assert_eq!(denied.suggested_rule, "allow dns 1.1.1.1:53");
    }

    #[test]
    fn a_denied_udp_request_suggests_a_udp_rule() {
        let denied = match policy().evaluate(&ctx_on("terraform", Transport::Udp, "8.8.8.8:53")) {
            Decision::Deny(denied) => denied,
            other => panic!("expected deny, got {other:?}"),
        };
        assert_eq!(denied.suggested_rule, "allow terraform udp 8.8.8.8:53");
    }

    #[test]
    fn an_untyped_rule_still_matches_both_transports() {
        assert!(policy()
            .evaluate(&ctx_on("terraform", Transport::Tcp, "api.production.example.com:443"))
            .is_allow());
        assert!(policy()
            .evaluate(&ctx_on("terraform", Transport::Udp, "api.production.example.com:443"))
            .is_allow());
    }

    #[test]
    fn a_policy_set_denies_an_unrecognised_workload() {
        let set = PolicySet::new(vec![{
            let mut p = policy();
            p.match_spec.organization = Some("acme".into());
            p
        }]);
        let mut context = ctx("terraform", "api.production.example.com:443");
        context.workload.organization = Some("someone-else".into());
        let decision = set.evaluate(&context);
        assert!(matches!(
            decision,
            Decision::Deny(Denied { reason: DenyReason::NoPolicyMatch, .. })
        ));
    }
}
