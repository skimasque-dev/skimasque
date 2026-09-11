//! The identity a policy is evaluated against: WHO is asking.
//!
//! This is a plain value type. Producing one from a trustworthy source -- a
//! verified GitHub OIDC token, a Kubernetes projected token, a signed platform
//! credential -- is the job of the identity crate that will sit above this one.
//! The policy engine only compares fields, so it takes the identity as data and
//! never asks where it came from.

use serde::{Deserialize, Serialize};

/// A workload's identity, as far as policy cares about it.
///
/// Every field is optional because different sources populate different
/// subsets: a developer running locally has an `actor` and little else, while a
/// GitHub Actions job has a repository, workflow and ref but no human actor.
/// A [`crate::MatchSpec`] only constrains the fields it names, so an identity
/// missing a field simply fails any match that requires it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadIdentity {
    /// The owning organisation or user, e.g. `acme`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    /// The full repository name, e.g. `acme/widget`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// The workflow file, e.g. `deploy.yml`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    /// The full git ref, e.g. `refs/heads/main` or `refs/tags/v1.2.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    /// The deployment environment, e.g. `production`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// The human or bot that triggered the run, e.g. `octocat`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
}

impl WorkloadIdentity {
    /// The branch name, if the ref is a branch ref.
    ///
    /// `refs/heads/main` yields `main`; a tag or other ref yields `None`. This
    /// is what lets a policy write `branch = "main"` instead of the full ref.
    pub fn branch(&self) -> Option<&str> {
        self.git_ref.as_deref()?.strip_prefix("refs/heads/")
    }

    /// The repository name without its owner: `acme/widget` yields `widget`.
    pub fn repository_name(&self) -> Option<&str> {
        let repo = self.repository.as_deref()?;
        Some(repo.rsplit_once('/').map_or(repo, |(_, name)| name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_ref_yields_its_branch_name() {
        let id = WorkloadIdentity {
            git_ref: Some("refs/heads/release/1.x".into()),
            ..Default::default()
        };
        assert_eq!(id.branch(), Some("release/1.x"));

        let tagged = WorkloadIdentity {
            git_ref: Some("refs/tags/v1.0".into()),
            ..Default::default()
        };
        assert_eq!(tagged.branch(), None);
    }

    #[test]
    fn the_repository_name_drops_the_owner() {
        let id = WorkloadIdentity {
            repository: Some("acme/widget".into()),
            ..Default::default()
        };
        assert_eq!(id.repository_name(), Some("widget"));
    }

    #[test]
    fn an_identity_round_trips_through_json_without_its_empty_fields() {
        let id = WorkloadIdentity {
            repository: Some("acme/widget".into()),
            git_ref: Some("refs/heads/main".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&id).unwrap();
        assert!(!json.contains("organization"), "{json}");
        assert_eq!(serde_json::from_str::<WorkloadIdentity>(&json).unwrap(), id);
    }
}
