//! Learning mode: turning observed traffic into a policy draft.
//!
//! The workflow the product calls for is *observe, generate, review, lock down,
//! edit*. This module is the "generate" step, and only that step: given a list
//! of [`Observation`]s -- one per `(application, destination)` a workload was
//! seen reaching -- it produces a [`Policy`] that allows exactly those and,
//! by the engine's deny-by-default rule, nothing else.
//!
//! What it does not do is decide the observations are safe. They are a
//! suggestion for a human to read, trim and commit; an observed destination is
//! not a trusted one.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::destination::Destination;
use crate::identity::WorkloadIdentity;
use crate::model::{Action, AppPattern, MatchSpec, Policy, Rule, Transport, TransportPattern};

/// One thing a workload was observed doing: reaching `destination` while
/// running `application`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub application: String,
    /// The transport the tunnel carried, if the collector recorded it. `None`
    /// produces a transport-agnostic rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<Transport>,
    /// The destination as the client named it, `host:port`.
    pub destination: String,
    /// How many times it was seen. Optional; carried through for the reviewer's
    /// benefit, not used in generation.
    #[serde(default)]
    pub count: u64,
}

/// The result of [`suggest_policy`]: the draft, plus any observations that
/// could not be used.
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub policy: Policy,
    /// `(observation, why)` for observations whose destination did not parse.
    pub skipped: Vec<(Observation, String)>,
}

/// Build a policy draft from observations.
///
/// One `allow` rule is emitted per distinct `(application, transport)` seen,
/// listing its observed destinations as exact `host:port` matches, sorted and
/// de-duplicated. Observations with no `transport` produce a transport-agnostic
/// rule. The `identity` becomes the policy's `[match]`, so the draft governs the
/// same workload that was observed.
pub fn suggest_policy(
    name: &str,
    identity: &WorkloadIdentity,
    observations: &[Observation],
) -> Suggestion {
    let mut skipped = Vec::new();

    // (application, transport) -> set of "host:port" strings, normalised via a parse.
    let mut by_key: std::collections::BTreeMap<(String, TransportPattern), BTreeSet<String>> =
        std::collections::BTreeMap::new();

    for observation in observations {
        match Destination::parse(&observation.destination) {
            Ok(destination) => {
                by_key
                    .entry((
                        observation.application.clone(),
                        TransportPattern::from(observation.transport),
                    ))
                    .or_default()
                    .insert(destination.to_string());
            }
            Err(error) => skipped.push((observation.clone(), error.to_string())),
        }
    }

    let rules = by_key
        .into_iter()
        .map(|((application, transport), destinations)| {
            let app_pattern = if application.is_empty() || application == "*" {
                AppPattern::Any
            } else {
                AppPattern::Name(application.clone())
            };
            let app_id = if application.is_empty() { "any" } else { &application };
            let id = match transport.as_word() {
                Some(word) => format!("{app_id}-{word}-observed"),
                None => format!("{app_id}-observed"),
            };
            Rule {
                id: Some(id),
                application: app_pattern,
                transport,
                action: Action::Allow,
                destinations: destinations
                    .iter()
                    .map(|d| {
                        crate::destination::DestinationSpec::parse(d)
                            .expect("a string that already parsed as a Destination")
                    })
                    .collect(),
            }
        })
        .collect();

    Suggestion {
        policy: Policy {
            name: name.to_owned(),
            match_spec: match_from_identity(identity),
            session: Default::default(),
            egress: None,
            limits: Default::default(),
            rules,
            tests: Vec::new(),
        },
        skipped,
    }
}

fn match_from_identity(identity: &WorkloadIdentity) -> MatchSpec {
    // Prefer the branch sugar when the ref is a branch, so the draft reads the
    // way a person would write it.
    let (branch, git_ref) = match identity.branch() {
        Some(branch) => (Some(branch.to_owned()), None),
        None => (None, identity.git_ref.clone()),
    };
    MatchSpec {
        organization: identity.organization.clone(),
        repository: identity.repository.clone(),
        workflow: identity.workflow.clone(),
        git_ref,
        branch,
        environment: identity.environment.clone(),
        actor: identity.actor.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Destination, RequestContext};

    fn obs(app: &str, dest: &str) -> Observation {
        Observation {
            application: app.to_owned(),
            transport: None,
            destination: dest.to_owned(),
            count: 1,
        }
    }

    fn obs_on(app: &str, transport: Transport, dest: &str) -> Observation {
        Observation {
            transport: Some(transport),
            ..obs(app, dest)
        }
    }

    #[test]
    fn a_draft_allows_exactly_what_was_observed() {
        let identity = WorkloadIdentity {
            repository: Some("acme/widget".into()),
            git_ref: Some("refs/heads/main".into()),
            ..Default::default()
        };
        let observations = [
            obs("terraform", "api.production.example.com:443"),
            obs("terraform", "api.production.example.com:443"), // duplicate
            obs("terraform", "registry.terraform.io:443"),
            obs("kubectl", "k8s.production.example.com:443"),
        ];

        let suggestion = suggest_policy("production", &identity, &observations);
        assert!(suggestion.skipped.is_empty());
        assert_eq!(suggestion.policy.match_spec.branch.as_deref(), Some("main"));
        assert_eq!(suggestion.policy.rules.len(), 2, "one rule per application");

        // Evaluate the draft: observed destinations allowed, others denied.
        let ctx = |app: &str, dest: &str| RequestContext {
            workload: identity.clone(),
            application: app.into(),
            transport: Transport::Tcp,
            destination: Destination::parse(dest).unwrap(),
        };
        assert!(suggestion
            .policy
            .evaluate(&ctx("terraform", "registry.terraform.io:443"))
            .is_allow());
        assert!(suggestion
            .policy
            .evaluate(&ctx("terraform", "evil.example.com:443"))
            .is_deny());
        assert!(suggestion
            .policy
            .evaluate(&ctx("kubectl", "api.production.example.com:443"))
            .is_deny());
    }

    #[test]
    fn an_unparseable_destination_is_skipped_not_fatal() {
        let suggestion = suggest_policy(
            "x",
            &WorkloadIdentity::default(),
            &[obs("terraform", "not a host"), obs("terraform", "ok.example.com:443")],
        );
        assert_eq!(suggestion.skipped.len(), 1);
        assert_eq!(suggestion.policy.rules[0].destinations.len(), 1);
    }

    #[test]
    fn observed_transports_become_separate_rules() {
        let observations = [
            obs_on("dns", Transport::Udp, "1.1.1.1:53"),
            obs_on("dns", Transport::Tcp, "1.1.1.1:53"),
            obs_on("curl", Transport::Tcp, "api.example.com:443"),
            obs("legacy", "other.example.com:443"), // no transport -> agnostic
        ];
        let suggestion = suggest_policy("p", &WorkloadIdentity::default(), &observations);

        let rule = |id: &str| suggestion.policy.rules.iter().find(|r| r.id.as_deref() == Some(id));
        assert_eq!(rule("dns-udp-observed").unwrap().transport, TransportPattern::Udp);
        assert_eq!(rule("dns-tcp-observed").unwrap().transport, TransportPattern::Tcp);
        assert_eq!(rule("curl-tcp-observed").unwrap().transport, TransportPattern::Tcp);
        assert_eq!(rule("legacy-observed").unwrap().transport, TransportPattern::Any);

        // The draft evaluates the way it was observed.
        let ctx = |app: &str, t: Transport, d: &str| RequestContext {
            workload: WorkloadIdentity::default(),
            application: app.into(),
            transport: t,
            destination: Destination::parse(d).unwrap(),
        };
        assert!(suggestion.policy.evaluate(&ctx("dns", Transport::Udp, "1.1.1.1:53")).is_allow());
        assert!(suggestion.policy.evaluate(&ctx("curl", Transport::Udp, "api.example.com:443")).is_deny());
    }

    #[test]
    fn a_draft_round_trips_through_toml() {
        let suggestion = suggest_policy(
            "production",
            &WorkloadIdentity {
                repository: Some("acme/widget".into()),
                ..Default::default()
            },
            &[obs("terraform", "api.example.com:443")],
        );
        let toml = suggestion.policy.to_toml();
        assert_eq!(Policy::from_toml(&toml).unwrap(), suggestion.policy);
    }
}
