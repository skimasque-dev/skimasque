//! The policy data model: what a `.masque/policies/*.toml` file becomes once it
//! is parsed and validated.
//!
//! These types are the engine's own representation, not the file format. The
//! mapping from TOML lives in [`crate::parse`]; here everything is already a
//! `Duration`, a validated [`DestinationSpec`], a resolved rate. The separation
//! means evaluation never has to think about spelling.

use std::time::Duration;

use crate::destination::DestinationSpec;
use crate::identity::WorkloadIdentity;
use crate::units::Rate;

/// One policy: an identity to match, and the rules that apply once it does.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// The policy's name, e.g. `production`. Unique within a [`PolicySet`].
    pub name: String,
    /// Which workloads this policy governs. An empty match governs every
    /// workload -- useful for a single-tenant deployment, dangerous otherwise.
    pub match_spec: MatchSpec,
    /// Session-wide constraints.
    pub session: SessionSpec,
    /// Where traffic this policy allows should leave from.
    pub egress: Option<EgressSpec>,
    /// Resource ceilings applied to every session under this policy.
    pub limits: Limits,
    /// The allow/deny rules, evaluated in order. First match wins.
    pub rules: Vec<Rule>,
    /// Assertions that travel with the policy and run in CI.
    pub tests: Vec<PolicyTest>,
}

/// Which workloads a policy governs. Every field is a constraint: a `Some`
/// field must be present and equal on the identity, a `None` field is not
/// checked. An all-`None` match matches everything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchSpec {
    /// Compared case-insensitively, as GitHub treats organisation names.
    pub organization: Option<String>,
    /// Compared case-insensitively; may be the full `owner/name` or just the
    /// name.
    pub repository: Option<String>,
    pub workflow: Option<String>,
    /// The full ref, e.g. `refs/heads/main`.
    pub git_ref: Option<String>,
    /// Sugar for `git_ref = "refs/heads/<branch>"`, kept separate so a denial
    /// can echo whichever the author wrote.
    pub branch: Option<String>,
    pub environment: Option<String>,
    pub actor: Option<String>,
}

impl MatchSpec {
    /// Whether `identity` satisfies every constraint this match names.
    pub fn matches(&self, identity: &WorkloadIdentity) -> bool {
        let ci_eq = |want: &str, got: Option<&str>| got.is_some_and(|g| g.eq_ignore_ascii_case(want));
        let eq = |want: &str, got: Option<&str>| got == Some(want);

        if let Some(org) = &self.organization {
            if !ci_eq(org, identity.organization.as_deref()) {
                return false;
            }
        }
        if let Some(repo) = &self.repository {
            let full = identity.repository.as_deref();
            let name = identity.repository_name();
            if !(ci_eq(repo, full) || ci_eq(repo, name)) {
                return false;
            }
        }
        if let Some(workflow) = &self.workflow {
            if !eq(workflow, identity.workflow.as_deref()) {
                return false;
            }
        }
        if let Some(git_ref) = &self.git_ref {
            if !eq(git_ref, identity.git_ref.as_deref()) {
                return false;
            }
        }
        if let Some(branch) = &self.branch {
            if identity.branch() != Some(branch.as_str()) {
                return false;
            }
        }
        if let Some(environment) = &self.environment {
            if !eq(environment, identity.environment.as_deref()) {
                return false;
            }
        }
        if let Some(actor) = &self.actor {
            if !eq(actor, identity.actor.as_deref()) {
                return false;
            }
        }
        true
    }

    /// How many fields this match constrains. A policy that constrains more
    /// fields is the more specific one when a [`PolicySet`] has to choose.
    pub fn specificity(&self) -> usize {
        [
            self.organization.is_some(),
            self.repository.is_some(),
            self.workflow.is_some(),
            self.git_ref.is_some(),
            self.branch.is_some(),
            self.environment.is_some(),
            self.actor.is_some(),
        ]
        .iter()
        .filter(|set| **set)
        .count()
    }
}

/// Session-wide constraints, distinct from the per-second [`Limits`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSpec {
    /// The longest a session under this policy may run before the credential
    /// expires and the tunnels are torn down.
    pub max_duration: Option<Duration>,
}

/// Where allowed traffic should egress.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EgressSpec {
    pub region: Option<String>,
    pub ip_pool: Option<String>,
}

/// Resource ceilings for a session. Every field is optional; `None` means the
/// gateway's own default applies, not "unlimited".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Limits {
    /// Bits per second, aggregate across the session's tunnels.
    pub bandwidth_bits_per_sec: Option<u64>,
    /// Total bytes the session may transfer before it is cut off.
    pub total_bytes: Option<u64>,
    /// Tunnels open at once.
    pub concurrent_connections: Option<u32>,
    /// Distinct destinations reachable at once.
    pub concurrent_destinations: Option<u32>,
    /// How fast new connections may be opened.
    pub connection_rate: Option<Rate>,
    /// Packets per second, aggregate.
    pub packets_per_sec: Option<u64>,
}

/// The transport a tunnel carries: TCP (`CONNECT`) or UDP (CONNECT-UDP).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Tcp,
    Udp,
}

impl Transport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which transport(s) a rule applies to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum TransportPattern {
    /// `any`, or omitted -- both TCP and UDP.
    #[default]
    Any,
    /// Only `tcp`.
    Tcp,
    /// Only `udp`.
    Udp,
}

impl TransportPattern {
    pub fn matches(self, transport: Transport) -> bool {
        matches!(
            (self, transport),
            (Self::Any, _) | (Self::Tcp, Transport::Tcp) | (Self::Udp, Transport::Udp)
        )
    }

    /// The word to write in a policy, or `None` for [`Any`](Self::Any) (which is
    /// the default and left off).
    pub fn as_word(self) -> Option<&'static str> {
        match self {
            Self::Any => None,
            Self::Tcp => Some("tcp"),
            Self::Udp => Some("udp"),
        }
    }
}

impl From<Option<Transport>> for TransportPattern {
    fn from(value: Option<Transport>) -> Self {
        match value {
            None => Self::Any,
            Some(Transport::Tcp) => Self::Tcp,
            Some(Transport::Udp) => Self::Udp,
        }
    }
}

/// One allow-or-deny rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// An optional identifier, surfaced in `ALLOW`/`DENY` output so an operator
    /// can find the rule that fired.
    pub id: Option<String>,
    /// Which application this rule is about.
    pub application: AppPattern,
    /// Which transport this rule is about. `Any` unless the rule says otherwise.
    pub transport: TransportPattern,
    pub action: Action,
    /// The destinations the rule covers. A rule with no destinations matches
    /// nothing and is a parse error.
    pub destinations: Vec<DestinationSpec>,
}

impl Rule {
    /// Whether this rule speaks to `(application, transport, ...)`, ignoring the
    /// destination. Used to decide which rules to draw closest-match
    /// suggestions from.
    pub fn covers(&self, application: &str, transport: Transport) -> bool {
        self.application.matches(application) && self.transport.matches(transport)
    }
}

/// Which application a rule applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppPattern {
    /// `*` -- every application.
    Any,
    /// An exact application name, compared case-insensitively.
    Name(String),
}

impl AppPattern {
    pub fn matches(&self, application: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Name(name) => name.eq_ignore_ascii_case(application),
        }
    }
}

/// A rule's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Allow,
    Deny,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A `[[tests]]` entry: an assertion about what the rules do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyTest {
    pub application: String,
    /// The destination as written; parsed when the test runs so a bad address
    /// is reported as a failing test rather than a load error.
    pub destination: String,
    /// The transport to test with. `None` means TCP -- the common case, and
    /// what a test written before transport granularity assumes.
    pub transport: Option<Transport>,
    pub expect: Action,
    pub description: Option<String>,
}

impl PolicyTest {
    /// The transport this test runs with (`transport` or TCP).
    pub fn transport(&self) -> Transport {
        self.transport.unwrap_or(Transport::Tcp)
    }
}

/// An ordered collection of policies, as loaded from `.masque/policies/`.
///
/// Order is the tiebreak of last resort. [`PolicySet::evaluate`] prefers the
/// most *specific* matching policy (the one whose [`MatchSpec`] constrains the
/// most fields); among equally specific matches, the earlier one wins, so the
/// file layout stays meaningful.
#[derive(Debug, Clone, PartialEq)]
pub struct PolicySet {
    policies: Vec<Policy>,
}

impl PolicySet {
    pub fn new(policies: Vec<Policy>) -> Self {
        Self { policies }
    }

    pub fn policies(&self) -> &[Policy] {
        &self.policies
    }

    /// The policy with this name, if the set has one.
    pub fn get(&self, name: &str) -> Option<&Policy> {
        self.policies.iter().find(|p| p.name == name)
    }

    /// The most specific policy whose match accepts `identity`.
    pub fn select(&self, identity: &WorkloadIdentity) -> Option<&Policy> {
        self.policies
            .iter()
            .filter(|p| p.match_spec.matches(identity))
            .enumerate()
            .max_by_key(|(index, p)| (p.match_spec.specificity(), std::cmp::Reverse(*index)))
            .map(|(_, p)| p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> WorkloadIdentity {
        WorkloadIdentity {
            organization: Some("Acme".into()),
            repository: Some("acme/widget".into()),
            workflow: Some("deploy.yml".into()),
            git_ref: Some("refs/heads/main".into()),
            environment: Some("production".into()),
            actor: Some("octocat".into()),
        }
    }

    #[test]
    fn an_empty_match_matches_everything() {
        assert!(MatchSpec::default().matches(&id()));
        assert!(MatchSpec::default().matches(&WorkloadIdentity::default()));
    }

    #[test]
    fn organization_and_repository_compare_case_insensitively() {
        let spec = MatchSpec {
            organization: Some("acme".into()),
            repository: Some("WIDGET".into()),
            ..Default::default()
        };
        assert!(spec.matches(&id()));
    }

    #[test]
    fn the_branch_sugar_matches_a_branch_ref() {
        let spec = MatchSpec {
            branch: Some("main".into()),
            ..Default::default()
        };
        assert!(spec.matches(&id()));

        let other = MatchSpec {
            branch: Some("develop".into()),
            ..Default::default()
        };
        assert!(!other.matches(&id()));
    }

    #[test]
    fn every_named_field_is_a_constraint() {
        let spec = MatchSpec {
            environment: Some("staging".into()),
            ..Default::default()
        };
        assert!(!spec.matches(&id()), "production is not staging");
    }

    #[test]
    fn select_prefers_the_more_specific_match() {
        let broad = Policy {
            name: "broad".into(),
            match_spec: MatchSpec {
                organization: Some("acme".into()),
                ..Default::default()
            },
            session: SessionSpec::default(),
            egress: None,
            limits: Limits::default(),
            rules: vec![],
            tests: vec![],
        };
        let narrow = Policy {
            name: "narrow".into(),
            match_spec: MatchSpec {
                organization: Some("acme".into()),
                repository: Some("acme/widget".into()),
                branch: Some("main".into()),
                ..Default::default()
            },
            ..broad.clone()
        };
        let set = PolicySet::new(vec![broad, narrow]);
        assert_eq!(set.select(&id()).unwrap().name, "narrow");
    }
}
