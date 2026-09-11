//! Turning a `.toml` policy file into a validated [`Policy`].
//!
//! The rule-oriented TOML layout from the product spec:
//!
//! ```toml
//! name = "production"
//!
//! [match]
//! repository = "acme/widget"
//! workflow = "deploy.yml"
//! branch = "main"
//!
//! [session]
//! max_duration = "20m"
//!
//! [egress]
//! region = "us-west"
//! ip_pool = "production"
//!
//! [limits]
//! bandwidth = "100Mbps"
//! connections = 50
//!
//! [[rules]]
//! id = "terraform-production"
//! application = "terraform"
//! action = "allow"
//! destinations = [
//!     "api.production.example.com:443",
//!     "registry.terraform.io:443",
//! ]
//!
//! [[tests]]
//! application = "terraform"
//! destination = "api.production.example.com:443"
//! expect = "allow"
//! ```
//!
//! Deny-by-default is implicit: a file with no `deny` rule still denies
//! everything its `allow` rules do not name. There is no need to write a
//! trailing `application = "*" / action = "deny"`, though one is accepted.

use serde::Deserialize;

use crate::destination::{DestinationSpec, ParseDestinationError};
use crate::model::{
    Action, AppPattern, EgressSpec, Limits, MatchSpec, Policy, PolicyTest, Rule, SessionSpec,
    Transport, TransportPattern,
};
use crate::units::{self, ParseUnitError};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("this is not valid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("this is not valid YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("set the session duration once: session.max_duration or limits.duration, not both")]
    DurationSetTwice,
    #[error("the policy has no name")]
    MissingName,
    #[error("rule {rule}: {source}")]
    Destination {
        rule: usize,
        #[source]
        source: ParseDestinationError,
    },
    #[error("rule {0} has no destinations")]
    EmptyRule(usize),
    #[error("{field}: {source}")]
    Unit {
        field: &'static str,
        #[source]
        source: ParseUnitError,
    },
    #[error("{field} must be \"allow\" or \"deny\", got {value:?}")]
    BadAction { field: String, value: String },
    #[error("{field} must be \"tcp\", \"udp\" or \"any\", got {value:?}")]
    BadTransport { field: String, value: String },
}

impl Policy {
    /// Parse a single policy from a TOML document (the rule-oriented layout).
    pub fn from_toml(input: &str) -> Result<Self, ParseError> {
        let raw: RawPolicy = toml::from_str(input)?;
        raw.into_policy()
    }

    /// Parse a single policy from a YAML document (the `identity` /
    /// `application` / `network` layout). Both formats produce the same model
    /// and share all of the validation below.
    pub fn from_yaml(input: &str) -> Result<Self, ParseError> {
        let doc: YamlDoc = serde_yaml::from_str(input)?;
        doc.into_raw()?.into_policy()
    }

    /// Parse a document whose format is inferred from `source_name`: a `.yaml`
    /// or `.yml` name is YAML, anything else is TOML.
    pub fn from_named_document(source_name: &str, input: &str) -> Result<Self, ParseError> {
        let lower = source_name.to_ascii_lowercase();
        if lower.ends_with(".yaml") || lower.ends_with(".yml") {
            Self::from_yaml(input)
        } else {
            Self::from_toml(input)
        }
    }
}

/// Something wrong with a whole set of policy documents, as loaded from
/// `.masque/policies/`.
#[derive(Debug, thiserror::Error)]
pub enum SetError {
    #[error("{source_name}: {source}")]
    Parse {
        source_name: String,
        #[source]
        source: ParseError,
    },
    #[error("two policies are both named {name:?} ({first} and {second})")]
    DuplicateName {
        name: String,
        first: String,
        second: String,
    },
}

impl crate::model::PolicySet {
    /// Assemble a set from named policy documents -- typically one per file
    /// under `.masque/policies/`. Each document's format is inferred from its
    /// name (`.yaml`/`.yml` is YAML, otherwise TOML). The engine stays free of
    /// the filesystem; the caller reads the files and hands over
    /// `(name, contents)` pairs. Names are also used to point at the offending
    /// document in an error.
    pub fn from_documents<I, N, S>(documents: I) -> Result<Self, SetError>
    where
        I: IntoIterator<Item = (N, S)>,
        N: Into<String>,
        S: AsRef<str>,
    {
        let mut policies: Vec<(String, Policy)> = Vec::new();
        for (source_name, contents) in documents {
            let source_name = source_name.into();
            let policy = Policy::from_named_document(&source_name, contents.as_ref())
                .map_err(|source| SetError::Parse {
                    source_name: source_name.clone(),
                    source,
                })?;
            if let Some((existing_source, _)) =
                policies.iter().find(|(_, p)| p.name == policy.name)
            {
                return Err(SetError::DuplicateName {
                    name: policy.name,
                    first: existing_source.clone(),
                    second: source_name,
                });
            }
            policies.push((source_name, policy));
        }
        Ok(Self::new(policies.into_iter().map(|(_, p)| p).collect()))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    name: Option<String>,
    #[serde(default, rename = "match")]
    match_spec: RawMatch,
    #[serde(default)]
    session: RawSession,
    #[serde(default)]
    egress: Option<RawEgress>,
    #[serde(default)]
    limits: RawLimits,
    #[serde(default)]
    rules: Vec<RawRule>,
    #[serde(default)]
    tests: Vec<RawTest>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMatch {
    organization: Option<String>,
    repository: Option<String>,
    workflow: Option<String>,
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    branch: Option<String>,
    environment: Option<String>,
    actor: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSession {
    max_duration: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEgress {
    region: Option<String>,
    ip_pool: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    bandwidth: Option<String>,
    bytes: Option<String>,
    connections: Option<u32>,
    concurrent_destinations: Option<u32>,
    connection_rate: Option<String>,
    packets_per_second: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    id: Option<String>,
    application: String,
    /// `tcp`, `udp`, `any` (the default), or omitted.
    transport: Option<String>,
    action: String,
    #[serde(default)]
    destinations: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTest {
    application: String,
    destination: String,
    transport: Option<String>,
    expect: String,
    description: Option<String>,
}

/// The YAML layout from the product spec: one application and an allow-list,
/// rather than an ordered rule table. It is lowered onto [`RawPolicy`] so the
/// two formats share every line of validation below.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct YamlDoc {
    name: Option<String>,
    #[serde(default)]
    identity: RawMatch,
    #[serde(default)]
    application: Option<YamlApplication>,
    #[serde(default)]
    session: RawSession,
    #[serde(default)]
    egress: Option<RawEgress>,
    #[serde(default)]
    network: YamlNetwork,
    #[serde(default)]
    limits: YamlLimits,
    #[serde(default)]
    tests: Vec<RawTest>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct YamlApplication {
    name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct YamlNetwork {
    /// Applies to both the allow and deny lists below.
    transport: Option<String>,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    deny: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct YamlLimits {
    /// The session duration lives under `limits` in the YAML schema; it is
    /// routed to `session.max_duration` here.
    duration: Option<String>,
    bandwidth: Option<String>,
    bytes: Option<String>,
    connections: Option<u32>,
    concurrent_destinations: Option<u32>,
    connection_rate: Option<String>,
    packets_per_second: Option<u64>,
}

impl YamlDoc {
    fn into_raw(self) -> Result<RawPolicy, ParseError> {
        let application = self
            .application
            .map(|app| app.name)
            .unwrap_or_else(|| "*".to_owned());

        // A deny list is emitted first so it wins over the allow list for the
        // same application, matching first-match evaluation.
        let transport = self.network.transport;
        let mut rules = Vec::new();
        if !self.network.deny.is_empty() {
            rules.push(RawRule {
                id: Some(format!("{application}-deny")),
                application: application.clone(),
                transport: transport.clone(),
                action: "deny".to_owned(),
                destinations: self.network.deny,
            });
        }
        if !self.network.allow.is_empty() {
            rules.push(RawRule {
                id: Some(format!("{application}-allow")),
                application,
                transport,
                action: "allow".to_owned(),
                destinations: self.network.allow,
            });
        }

        let max_duration = match (self.session.max_duration, self.limits.duration) {
            (Some(_), Some(_)) => return Err(ParseError::DurationSetTwice),
            (a, b) => a.or(b),
        };

        Ok(RawPolicy {
            name: self.name,
            match_spec: self.identity,
            session: RawSession { max_duration },
            egress: self.egress,
            limits: RawLimits {
                bandwidth: self.limits.bandwidth,
                bytes: self.limits.bytes,
                connections: self.limits.connections,
                concurrent_destinations: self.limits.concurrent_destinations,
                connection_rate: self.limits.connection_rate,
                packets_per_second: self.limits.packets_per_second,
            },
            rules,
            tests: self.tests,
        })
    }
}

impl RawPolicy {
    fn into_policy(self) -> Result<Policy, ParseError> {
        let name = self.name.filter(|n| !n.is_empty()).ok_or(ParseError::MissingName)?;

        let match_spec = MatchSpec {
            organization: self.match_spec.organization,
            repository: self.match_spec.repository,
            workflow: self.match_spec.workflow,
            git_ref: self.match_spec.git_ref,
            branch: self.match_spec.branch,
            environment: self.match_spec.environment,
            actor: self.match_spec.actor,
        };

        let session = SessionSpec {
            max_duration: parse_field("session.max_duration", self.session.max_duration, units::parse_duration)?,
        };

        let egress = self.egress.map(|e| EgressSpec {
            region: e.region,
            ip_pool: e.ip_pool,
        });

        let limits = Limits {
            bandwidth_bits_per_sec: parse_field("limits.bandwidth", self.limits.bandwidth, units::parse_bitrate)?,
            total_bytes: parse_field("limits.bytes", self.limits.bytes, units::parse_bytes)?,
            concurrent_connections: self.limits.connections,
            concurrent_destinations: self.limits.concurrent_destinations,
            connection_rate: parse_field("limits.connection_rate", self.limits.connection_rate, units::parse_rate)?,
            packets_per_sec: self.limits.packets_per_second,
        };

        let rules = self
            .rules
            .into_iter()
            .enumerate()
            .map(|(index, raw)| raw.into_rule(index))
            .collect::<Result<Vec<_>, _>>()?;

        let tests = self
            .tests
            .into_iter()
            .map(RawTest::into_test)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Policy {
            name,
            match_spec,
            session,
            egress,
            limits,
            rules,
            tests,
        })
    }
}

impl RawRule {
    fn into_rule(self, index: usize) -> Result<Rule, ParseError> {
        if self.destinations.is_empty() {
            return Err(ParseError::EmptyRule(index));
        }
        let action = parse_action(&format!("rule {index} action"), &self.action)?;
        let transport =
            parse_transport_pattern(&format!("rule {index} transport"), self.transport.as_deref())?;
        let application = match self.application.as_str() {
            "*" => AppPattern::Any,
            other => AppPattern::Name(other.to_owned()),
        };
        let destinations = self
            .destinations
            .iter()
            .map(|d| DestinationSpec::parse(d))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| ParseError::Destination { rule: index, source })?;

        Ok(Rule {
            id: self.id,
            application,
            transport,
            action,
            destinations,
        })
    }
}

impl RawTest {
    fn into_test(self) -> Result<PolicyTest, ParseError> {
        let expect = parse_action("test expect", &self.expect)?;
        let transport = match self.transport.as_deref() {
            None => None,
            Some(word) => match parse_transport_pattern("test transport", Some(word))? {
                TransportPattern::Any => {
                    return Err(ParseError::BadTransport {
                        field: "test transport".to_owned(),
                        value: word.to_owned(),
                    })
                }
                TransportPattern::Tcp => Some(Transport::Tcp),
                TransportPattern::Udp => Some(Transport::Udp),
            },
        };
        Ok(PolicyTest {
            application: self.application,
            destination: self.destination,
            transport,
            expect,
            description: self.description,
        })
    }
}

fn parse_action(field: &str, value: &str) -> Result<Action, ParseError> {
    match value {
        "allow" => Ok(Action::Allow),
        "deny" => Ok(Action::Deny),
        other => Err(ParseError::BadAction {
            field: field.to_owned(),
            value: other.to_owned(),
        }),
    }
}

fn parse_transport_pattern(
    field: &str,
    value: Option<&str>,
) -> Result<TransportPattern, ParseError> {
    match value {
        None => Ok(TransportPattern::Any),
        Some("any") => Ok(TransportPattern::Any),
        Some("tcp") => Ok(TransportPattern::Tcp),
        Some("udp") => Ok(TransportPattern::Udp),
        Some(other) => Err(ParseError::BadTransport {
            field: field.to_owned(),
            value: other.to_owned(),
        }),
    }
}

fn parse_field<T>(
    field: &'static str,
    value: Option<String>,
    parser: impl FnOnce(&str) -> Result<T, ParseUnitError>,
) -> Result<Option<T>, ParseError> {
    value
        .map(|v| parser(&v).map_err(|source| ParseError::Unit { field, source }))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Rate;
    use std::time::Duration;

    const SAMPLE: &str = r#"
        name = "production"

        [match]
        organization = "acme"
        repository = "acme/widget"
        workflow = "deploy.yml"
        branch = "main"
        environment = "production"

        [session]
        max_duration = "20m"

        [egress]
        region = "us-west"
        ip_pool = "production"

        [limits]
        bandwidth = "100Mbps"
        bytes = "10GB"
        connections = 50
        connection_rate = "10/s"

        [[rules]]
        id = "terraform-production-api"
        application = "terraform"
        action = "allow"
        destinations = [
            "api.production.example.com:443",
            "registry.terraform.io:443",
            "db.production.example.com:5432",
        ]

        [[rules]]
        application = "kubectl"
        action = "allow"
        destinations = ["kubernetes.production.example.com:443"]

        [[tests]]
        application = "terraform"
        destination = "api.production.example.com:443"
        expect = "allow"

        [[tests]]
        application = "terraform"
        destination = "google.com:443"
        expect = "deny"
    "#;

    #[test]
    fn the_sample_policy_parses_into_the_model() {
        let policy = Policy::from_toml(SAMPLE).unwrap();
        assert_eq!(policy.name, "production");
        assert_eq!(policy.match_spec.branch.as_deref(), Some("main"));
        assert_eq!(policy.session.max_duration, Some(Duration::from_secs(1_200)));
        assert_eq!(policy.egress.as_ref().unwrap().region.as_deref(), Some("us-west"));
        assert_eq!(policy.limits.bandwidth_bits_per_sec, Some(100_000_000));
        assert_eq!(policy.limits.total_bytes, Some(10_000_000_000));
        assert_eq!(policy.limits.concurrent_connections, Some(50));
        assert_eq!(policy.limits.connection_rate, Some(Rate { count: 10, per: Duration::from_secs(1) }));
        assert_eq!(policy.rules.len(), 2);
        assert_eq!(policy.rules[0].id.as_deref(), Some("terraform-production-api"));
        assert_eq!(policy.rules[0].destinations.len(), 3);
        assert_eq!(policy.tests.len(), 2);
    }

    #[test]
    fn a_minimal_policy_is_just_a_name() {
        let policy = Policy::from_toml(r#"name = "dev""#).unwrap();
        assert_eq!(policy.name, "dev");
        assert!(policy.match_spec.matches(&Default::default()));
        assert!(policy.rules.is_empty());
    }

    #[test]
    fn a_nameless_policy_is_rejected() {
        assert!(matches!(
            Policy::from_toml("[match]\norganization = \"acme\""),
            Err(ParseError::MissingName)
        ));
    }

    #[test]
    fn a_rule_with_a_bad_destination_names_the_rule() {
        let err = Policy::from_toml(
            r#"
            name = "x"
            [[rules]]
            application = "terraform"
            action = "allow"
            destinations = ["not a host:443"]
        "#,
        )
        .unwrap_err();
        assert!(matches!(err, ParseError::Destination { rule: 0, .. }), "{err:?}");
    }

    #[test]
    fn an_empty_rule_is_rejected() {
        let err = Policy::from_toml(
            r#"
            name = "x"
            [[rules]]
            application = "terraform"
            action = "allow"
            destinations = []
        "#,
        )
        .unwrap_err();
        assert!(matches!(err, ParseError::EmptyRule(0)));
    }

    #[test]
    fn an_unknown_key_is_rejected_rather_than_ignored() {
        // A typo'd limit that is silently dropped is a security bug.
        assert!(Policy::from_toml(
            r#"
            name = "x"
            [limits]
            bandwith = "100Mbps"
        "#
        )
        .is_err());
    }

    const SAMPLE_YAML: &str = r#"
name: production

identity:
  repository: acme/widget
  workflow: deploy.yml
  ref: refs/heads/main
  environment: production

application:
  name: terraform

network:
  allow:
    - api.production.example.com:443
    - registry.terraform.io:443
    - db.production.example.com:5432
  deny:
    - 169.254.169.254:80

limits:
  duration: 20m
  bandwidth: 100Mbps
  connections: 50

tests:
  - application: terraform
    destination: api.production.example.com:443
    expect: allow
"#;

    #[test]
    fn the_yaml_layout_lowers_onto_the_same_model() {
        let policy = Policy::from_yaml(SAMPLE_YAML).unwrap();
        assert_eq!(policy.name, "production");
        assert_eq!(policy.match_spec.repository.as_deref(), Some("acme/widget"));
        assert_eq!(policy.match_spec.git_ref.as_deref(), Some("refs/heads/main"));
        assert_eq!(policy.session.max_duration, Some(Duration::from_secs(1_200)));
        assert_eq!(policy.limits.bandwidth_bits_per_sec, Some(100_000_000));
        assert_eq!(policy.limits.concurrent_connections, Some(50));
        // deny is emitted before allow so it wins for the same application.
        assert_eq!(policy.rules.len(), 2);
        assert_eq!(policy.rules[0].action, Action::Deny);
        assert_eq!(policy.rules[1].action, Action::Allow);
        assert_eq!(policy.rules[1].destinations.len(), 3);
        assert_eq!(policy.tests.len(), 1);
    }

    #[test]
    fn a_yaml_policy_evaluates_like_its_toml_twin() {
        use crate::model::Transport;
        use crate::{Destination, RequestContext, WorkloadIdentity};
        let policy = Policy::from_yaml(SAMPLE_YAML).unwrap();
        let ctx = |dest: &str| RequestContext {
            workload: WorkloadIdentity::default(),
            application: "terraform".into(),
            transport: Transport::Tcp,
            destination: Destination::parse(dest).unwrap(),
        };
        assert!(policy.evaluate(&ctx("registry.terraform.io:443")).is_allow());
        assert!(policy.evaluate(&ctx("169.254.169.254:80")).is_deny());
        assert!(policy.evaluate(&ctx("google.com:443")).is_deny());
    }

    #[test]
    fn setting_the_duration_in_both_places_is_an_error() {
        let err = Policy::from_yaml(
            "name: x\nsession:\n  max_duration: 10m\nlimits:\n  duration: 20m\n",
        )
        .unwrap_err();
        assert!(matches!(err, ParseError::DurationSetTwice));
    }

    #[test]
    fn a_set_infers_format_from_the_document_name() {
        let set = crate::PolicySet::from_documents([
            ("prod.yaml", SAMPLE_YAML),
            ("dev.toml", "name = \"dev\"\n"),
        ])
        .unwrap();
        assert_eq!(set.policies().len(), 2);
    }

    #[test]
    fn a_set_rejects_two_policies_with_the_same_name() {
        let err = crate::PolicySet::from_documents([
            ("a.toml", r#"name = "prod""#),
            ("b.toml", r#"name = "prod""#),
        ])
        .unwrap_err();
        assert!(matches!(err, SetError::DuplicateName { .. }), "{err:?}");
    }

    #[test]
    fn a_set_reports_which_file_failed_to_parse() {
        let err = crate::PolicySet::from_documents([
            ("ok.toml", r#"name = "dev""#),
            ("bad.toml", "name = "),
        ])
        .unwrap_err();
        assert!(matches!(err, SetError::Parse { source_name, .. } if source_name == "bad.toml"));
    }

    #[test]
    fn a_bad_action_is_reported() {
        let err = Policy::from_toml(
            r#"
            name = "x"
            [[rules]]
            application = "terraform"
            action = "permit"
            destinations = ["api.example.com:443"]
        "#,
        )
        .unwrap_err();
        assert!(matches!(err, ParseError::BadAction { .. }));
    }

    #[test]
    fn a_rule_and_test_carry_a_transport() {
        use crate::model::{Transport, TransportPattern};
        let policy = Policy::from_toml(
            r#"
            name = "x"
            [[rules]]
            application = "dns"
            transport = "udp"
            action = "allow"
            destinations = ["1.1.1.1:53"]
            [[rules]]
            application = "curl"
            action = "allow"
            destinations = ["api.example.com:443"]
            [[tests]]
            application = "dns"
            transport = "udp"
            destination = "1.1.1.1:53"
            expect = "allow"
        "#,
        )
        .unwrap();
        assert_eq!(policy.rules[0].transport, TransportPattern::Udp);
        assert_eq!(policy.rules[1].transport, TransportPattern::Any);
        assert_eq!(policy.tests[0].transport, Some(Transport::Udp));
        assert!(policy.run_tests()[0].passed);
    }

    #[test]
    fn a_bad_transport_is_reported() {
        assert!(matches!(
            Policy::from_toml(
                "name = \"x\"\n[[rules]]\napplication = \"a\"\ntransport = \"sctp\"\naction = \"allow\"\ndestinations = [\"x:1\"]\n"
            ),
            Err(ParseError::BadTransport { .. })
        ));
        // A test's transport must be concrete, not `any`.
        assert!(matches!(
            Policy::from_toml(
                "name = \"x\"\n[[tests]]\napplication = \"a\"\ntransport = \"any\"\ndestination = \"x:1\"\nexpect = \"deny\"\n"
            ),
            Err(ParseError::BadTransport { .. })
        ));
    }

    #[test]
    fn the_yaml_network_transport_applies_to_both_lists() {
        use crate::model::TransportPattern;
        let policy = Policy::from_yaml(
            "name: p\napplication:\n  name: dns\nnetwork:\n  transport: udp\n  allow: [\"1.1.1.1:53\"]\n  deny: [\"8.8.8.8:53\"]\n",
        )
        .unwrap();
        assert!(policy.rules.iter().all(|r| r.transport == TransportPattern::Udp));
    }
}
