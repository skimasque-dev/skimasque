//! Rendering decisions and running a policy's tests.
//!
//! The evaluation types stay data-only; the human-facing shapes live here: the
//! `ALLOW` / `DENY` block `masque policy check` prints, and the pass/fail list
//! `masque policy test` produces from a policy's `[[tests]]`.

use std::fmt;

use crate::destination::Destination;
use crate::eval::{Decision, RequestContext};
use crate::identity::WorkloadIdentity;
use crate::model::{Action, Policy, PolicyTest};

impl fmt::Display for Decision {
    /// The block form from the product spec:
    ///
    /// ```text
    /// ALLOW
    ///
    /// Policy: production
    /// Rule: terraform-production-api
    /// Egress: us-west
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Decision::Allow(allowed) => {
                writeln!(f, "ALLOW\n")?;
                writeln!(f, "Policy: {}", allowed.policy)?;
                write!(f, "Rule: {}", allowed.rule)?;
                if let Some(egress) = &allowed.egress {
                    if let Some(region) = &egress.region {
                        write!(f, "\nEgress: {region}")?;
                    } else if let Some(pool) = &egress.ip_pool {
                        write!(f, "\nEgress: {pool}")?;
                    }
                }
                if let Some(duration) = allowed.session.max_duration {
                    write!(f, "\nMax duration: {}", humanize(duration))?;
                }
                Ok(())
            }
            Decision::Deny(denied) => {
                writeln!(f, "DENY\n")?;
                if let Some(policy) = &denied.policy {
                    writeln!(f, "Policy: {policy}")?;
                }
                writeln!(f, "\nReason:\n{}", denied.reason.summary())?;
                if !denied.closest.is_empty() {
                    writeln!(f, "\nClosest rules:")?;
                    for rule in &denied.closest {
                        writeln!(f, "  {rule}")?;
                    }
                }
                write!(f, "\nSuggested rule:\n\n  {}", denied.suggested_rule)
            }
        }
    }
}

fn humanize(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    match () {
        _ if secs != 0 && secs.is_multiple_of(3_600) => format!("{}h", secs / 3_600),
        _ if secs != 0 && secs.is_multiple_of(60) => format!("{}m", secs / 60),
        _ => format!("{secs}s"),
    }
}

/// The result of running one `[[tests]]` assertion.
#[derive(Debug, Clone)]
pub struct TestOutcome {
    pub test: PolicyTest,
    /// What the policy actually decided, or `None` if the test's destination
    /// did not parse.
    pub actual: Option<Action>,
    pub passed: bool,
    /// A one-line explanation, populated when the test failed.
    pub detail: Option<String>,
}

impl Policy {
    /// Run every `[[tests]]` assertion against this policy's rules.
    ///
    /// Tests exercise the rule table only: they synthesise an identity that
    /// this policy's own match accepts, so a test never fails merely because
    /// it forgot to spell out `repository` and `branch`.
    pub fn run_tests(&self) -> Vec<TestOutcome> {
        let workload = self.representative_identity();
        self.tests
            .iter()
            .map(|test| self.run_one_test(test, &workload))
            .collect()
    }

    fn run_one_test(&self, test: &PolicyTest, workload: &WorkloadIdentity) -> TestOutcome {
        let destination = match Destination::parse(&test.destination) {
            Ok(destination) => destination,
            Err(error) => {
                return TestOutcome {
                    test: test.clone(),
                    actual: None,
                    passed: false,
                    detail: Some(format!("the test destination is invalid: {error}")),
                }
            }
        };

        let ctx = RequestContext {
            workload: workload.clone(),
            application: test.application.clone(),
            transport: test.transport(),
            destination,
        };
        let actual = self.evaluate(&ctx).action();
        let passed = actual == test.expect;
        TestOutcome {
            test: test.clone(),
            actual: Some(actual),
            passed,
            detail: (!passed).then(|| {
                format!("expected {}, got {}", test.expect, actual)
            }),
        }
    }

    /// An identity that this policy's [`crate::MatchSpec`] accepts, built from
    /// the match's own constraints. Used only to drive `[[tests]]`.
    fn representative_identity(&self) -> WorkloadIdentity {
        let m = &self.match_spec;
        let git_ref = m.git_ref.clone().or_else(|| {
            m.branch.as_ref().map(|branch| format!("refs/heads/{branch}"))
        });
        WorkloadIdentity {
            organization: m.organization.clone(),
            repository: m.repository.clone(),
            workflow: m.workflow.clone(),
            git_ref,
            environment: m.environment.clone(),
            actor: m.actor.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy::from_toml(
            r#"
            name = "production"
            [match]
            repository = "acme/widget"
            branch = "main"
            [egress]
            region = "us-west"

            [[rules]]
            id = "tf-api"
            application = "terraform"
            action = "allow"
            destinations = ["api.production.example.com:443"]

            [[tests]]
            application = "terraform"
            destination = "api.production.example.com:443"
            expect = "allow"

            [[tests]]
            application = "terraform"
            destination = "google.com:443"
            expect = "deny"

            [[tests]]
            application = "terraform"
            destination = "api.production.example.com:443"
            expect = "deny"
        "#,
        )
        .unwrap()
    }

    #[test]
    fn tests_run_against_the_rule_table_with_a_synthesised_identity() {
        let outcomes = policy().run_tests();
        assert_eq!(outcomes.len(), 3);
        assert!(outcomes[0].passed, "{:?}", outcomes[0].detail);
        assert!(outcomes[1].passed);
        assert!(!outcomes[2].passed, "the deliberately-wrong assertion should fail");
        assert_eq!(outcomes[2].actual, Some(Action::Allow));
    }

    #[test]
    fn an_allow_decision_renders_the_spec_block() {
        let decision = policy().evaluate(&RequestContext {
            workload: WorkloadIdentity::default(),
            application: "terraform".into(),
            transport: crate::model::Transport::Tcp,
            destination: Destination::parse("api.production.example.com:443").unwrap(),
        });
        let text = decision.to_string();
        assert!(text.starts_with("ALLOW\n"), "{text}");
        assert!(text.contains("Policy: production"));
        assert!(text.contains("Rule: tf-api"));
        assert!(text.contains("Egress: us-west"));
    }

    #[test]
    fn a_deny_decision_renders_reason_and_suggested_rule() {
        let decision = policy().evaluate(&RequestContext {
            workload: WorkloadIdentity::default(),
            application: "terraform".into(),
            transport: crate::model::Transport::Tcp,
            destination: Destination::parse("google.com:443").unwrap(),
        });
        let text = decision.to_string();
        assert!(text.starts_with("DENY\n"), "{text}");
        assert!(text.contains("No matching allow rule."));
        assert!(text.contains("allow terraform google.com:443"));
    }
}
