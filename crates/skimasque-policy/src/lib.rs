//! The network-access policy engine.
//!
//! This crate answers one question: *may this workload reach this destination,
//! and under what limits?* It is deliberately independent of MASQUE. Nothing
//! here opens a socket, resolves a name, or speaks HTTP/3; a policy is a value,
//! an evaluation is a pure function, and the result is a [`Decision`] the
//! transport layer acts on. Keeping it separate is what lets a policy be parsed,
//! tested and diffed in CI with no network at all.
//!
//! # The model
//!
//! Every authorization decision has four parts:
//!
//! | Part | Type | Example |
//! |---|---|---|
//! | **WHO** | [`WorkloadIdentity`] | `acme/widget`, workflow `deploy.yml`, ref `refs/heads/main` |
//! | **WHAT** | the application name | `terraform` |
//! | **WHERE** | [`Destination`] | `db.production.example.com:5432` |
//! | **LIMITS** | [`Limits`] and [`SessionSpec`] | 20m, 100 Mbps, 50 connections |
//!
//! A [`Policy`] binds an identity match to a set of [`Rule`]s over applications
//! and destinations. A [`PolicySet`] is the ordered collection loaded from
//! `.masque/policies/`.
//!
//! # Two principles the code enforces
//!
//! **Deny by default.** [`Policy::evaluate`] returns [`Decision::Deny`] unless a
//! rule explicitly allows the destination. There is no implicit trailing
//! `deny *`; the absence of an allow *is* the denial. A denial always carries a
//! reason and, where it can, a suggested rule and the closest rules that do
//! exist.
//!
//! **The application name is session context, not proof.** The engine matches on
//! whatever application string it is handed, but the string is only as
//! trustworthy as its source. Until the platform can identify the real process,
//! callers should treat it as a hint the developer supplied, not an
//! authenticated fact, and policies should not lean on it for their only
//! defence.
//!
//! # Example
//!
//! ```
//! use skimasque_policy::{Destination, Policy, RequestContext, WorkloadIdentity};
//!
//! let policy = Policy::from_toml(r#"
//!     name = "production"
//!
//!     [match]
//!     repository = "acme/widget"
//!     branch = "main"
//!
//!     [[rules]]
//!     application = "terraform"
//!     action = "allow"
//!     destinations = ["api.production.example.com:443"]
//! "#).unwrap();
//!
//! let ctx = RequestContext {
//!     workload: WorkloadIdentity {
//!         repository: Some("acme/widget".into()),
//!         git_ref: Some("refs/heads/main".into()),
//!         ..Default::default()
//!     },
//!     application: "terraform".into(),
//!     transport: skimasque_policy::Transport::Tcp,
//!     destination: Destination::parse("api.production.example.com:443").unwrap(),
//! };
//!
//! assert!(policy.identity_matches(&ctx.workload));
//! assert!(policy.evaluate(&ctx).is_allow());
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

mod destination;
mod eval;
mod identity;
mod learn;
mod lint;
mod model;
mod parse;
mod render;
mod report;
mod units;

pub use destination::{Destination, DestinationSpec, Host, HostPattern, ParseDestinationError, PortPattern};
pub use eval::{Allowed, Decision, DenyReason, Denied, RequestContext};
pub use identity::WorkloadIdentity;
pub use learn::{suggest_policy, Observation, Suggestion};
pub use lint::{Lint, LintCode};
pub use model::{
    Action, AppPattern, EgressSpec, Limits, MatchSpec, Policy, PolicySet, PolicyTest, Rule,
    SessionSpec, Transport, TransportPattern,
};
pub use parse::{ParseError, SetError};
pub use report::TestOutcome;
pub use units::{
    humanize_decimal, humanize_duration, parse_bitrate, parse_bytes, parse_duration, parse_rate,
    ParseUnitError, Rate,
};
