//! Workload-identity providers: which OIDC issuer a token comes from, and how to
//! read its claims into a [`WorkloadIdentity`].
//!
//! Every CI system that issues OIDC tokens spells the identity differently --
//! GitHub has `repository` / `repository_owner` / `workflow_ref`, GitLab has
//! `project_path` / `namespace_path` / `ref` + `ref_type`, Buildkite has
//! `pipeline_slug` / `organization_slug` / `build_branch`. Verification (the
//! signature and `iss` / `aud` / `exp` checks) is identical for all of them; only
//! the mapping differs, and that lives here.

use serde::Deserialize;
use skimasque_policy::WorkloadIdentity;

/// GitHub's hosted Actions OIDC issuer.
///
/// A GitHub Enterprise Server instance issues from its own host; pass that as
/// the issuer instead, still with [`Provider::GitHubActions`].
pub const GITHUB_ACTIONS_ISSUER: &str = "https://token.actions.githubusercontent.com";

/// The verified claims of an OIDC token, as raw JSON.
///
/// [`Verifier::verify`](crate::Verifier::verify) has already checked the
/// signature and the registered claims by the time you hold one of these; the
/// only thing left is to read the provider-specific fields out.
#[derive(Debug, Clone, Deserialize)]
pub struct Claims(serde_json::Map<String, serde_json::Value>);

impl Claims {
    /// A string claim, or `None` if it is absent or not a string.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(serde_json::Value::as_str)
    }

    /// The `sub` claim -- a stable per-token subject, used as the issued
    /// credential's identifier.
    pub fn subject(&self) -> Option<&str> {
        self.get("sub")
    }

    /// The `iss` claim.
    pub fn issuer(&self) -> Option<&str> {
        self.get("iss")
    }

    #[cfg(test)]
    pub(crate) fn from_value(value: serde_json::Value) -> Self {
        Self(value.as_object().expect("a JSON object").clone())
    }
}

/// The claim names a [`Provider::Generic`] mapping reads each identity field
/// from. An unset field is left unset on the identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimNames {
    pub organization: Option<String>,
    pub repository: Option<String>,
    pub workflow: Option<String>,
    pub git_ref: Option<String>,
    pub environment: Option<String>,
    pub actor: Option<String>,
}

/// The CI system whose OIDC tokens a gateway accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Provider {
    /// GitHub Actions -- `github.com` or a GitHub Enterprise Server host.
    GitHubActions,
    /// GitLab CI/CD `id_tokens` -- `gitlab.com` or a self-managed instance.
    GitLab,
    /// Buildkite agent OIDC.
    Buildkite,
    /// Any other OIDC issuer, with the claim names given explicitly.
    Generic(ClaimNames),
}

impl Provider {
    /// Parse a built-in provider name (`github`, `gitlab`, `buildkite`).
    /// [`Generic`](Self::Generic) has no name -- it is built from claim flags.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "github" | "github-actions" => Some(Self::GitHubActions),
            "gitlab" => Some(Self::GitLab),
            "buildkite" => Some(Self::Buildkite),
            _ => None,
        }
    }

    /// The issuer these tokens come from by default, when there is a canonical
    /// one. `Generic` has none, and GitHub Enterprise Server overrides it.
    pub fn default_issuer(&self) -> Option<&'static str> {
        match self {
            Self::GitHubActions => Some(GITHUB_ACTIONS_ISSUER),
            Self::GitLab => Some("https://gitlab.com"),
            Self::Buildkite => Some("https://agent.buildkite.com"),
            Self::Generic(_) => None,
        }
    }

    /// Map verified `claims` onto the fields a policy matches.
    pub fn identify(&self, claims: &Claims) -> WorkloadIdentity {
        match self {
            Self::GitHubActions => github(claims),
            Self::GitLab => gitlab(claims),
            Self::Buildkite => buildkite(claims),
            Self::Generic(names) => generic(names, claims),
        }
    }
}

fn owned(value: Option<&str>) -> Option<String> {
    value.map(str::to_owned)
}

fn github(c: &Claims) -> WorkloadIdentity {
    WorkloadIdentity {
        organization: owned(c.get("repository_owner")),
        repository: owned(c.get("repository")),
        // `workflow_ref` is `owner/repo/.github/workflows/deploy.yml@refs/heads/main`;
        // report the file name, which is what policies and humans use.
        workflow: file_from_ref_uri(c.get("workflow_ref")).or_else(|| owned(c.get("workflow"))),
        git_ref: owned(c.get("ref")),
        environment: owned(c.get("environment")),
        actor: owned(c.get("actor")),
    }
}

fn gitlab(c: &Claims) -> WorkloadIdentity {
    WorkloadIdentity {
        organization: owned(c.get("namespace_path")),
        repository: owned(c.get("project_path")),
        workflow: file_from_ref_uri(c.get("ci_config_ref_uri")),
        git_ref: gitlab_ref(c),
        environment: owned(c.get("environment")),
        actor: owned(c.get("user_login")),
    }
}

/// GitLab gives a bare `ref` (`main`) plus a `ref_type` (`branch` / `tag`);
/// policies match the full `refs/heads/...` form, so reconstruct it.
fn gitlab_ref(c: &Claims) -> Option<String> {
    let name = c.get("ref")?;
    Some(match c.get("ref_type") {
        Some("tag") => format!("refs/tags/{name}"),
        _ => format!("refs/heads/{name}"),
    })
}

fn buildkite(c: &Claims) -> WorkloadIdentity {
    WorkloadIdentity {
        organization: owned(c.get("organization_slug")),
        // A Buildkite pipeline is the closest thing to a repository.
        repository: owned(c.get("pipeline_slug")),
        workflow: None,
        git_ref: c
            .get("build_branch")
            .map(|b| format!("refs/heads/{b}"))
            .or_else(|| c.get("build_tag").map(|t| format!("refs/tags/{t}"))),
        environment: None,
        actor: None,
    }
}

fn generic(names: &ClaimNames, c: &Claims) -> WorkloadIdentity {
    let pick = |field: &Option<String>| owned(field.as_deref().and_then(|name| c.get(name)));
    WorkloadIdentity {
        organization: pick(&names.organization),
        repository: pick(&names.repository),
        workflow: pick(&names.workflow),
        git_ref: pick(&names.git_ref),
        environment: pick(&names.environment),
        actor: pick(&names.actor),
    }
}

/// The file name from a `path/to/file.yml@ref` style claim.
fn file_from_ref_uri(uri: Option<&str>) -> Option<String> {
    let uri = uri?;
    let path = uri.split('@').next().unwrap_or(uri);
    let file = path.rsplit('/').next().unwrap_or(path);
    (!file.is_empty()).then(|| file.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(value: serde_json::Value) -> Claims {
        Claims::from_value(value)
    }

    #[test]
    fn provider_names_parse() {
        assert_eq!(Provider::from_name("GitHub"), Some(Provider::GitHubActions));
        assert_eq!(Provider::from_name("gitlab"), Some(Provider::GitLab));
        assert_eq!(Provider::from_name("buildkite"), Some(Provider::Buildkite));
        assert_eq!(Provider::from_name("jenkins"), None);
    }

    #[test]
    fn github_claims_map_to_owner_repo_workflow_ref() {
        let id = Provider::GitHubActions.identify(&claims(serde_json::json!({
            "repository": "acme/widget",
            "repository_owner": "acme",
            "workflow_ref": "acme/widget/.github/workflows/deploy.yml@refs/heads/main",
            "workflow": "Deploy to production",
            "ref": "refs/heads/main",
            "environment": "production",
            "actor": "octocat",
        })));
        assert_eq!(id.organization.as_deref(), Some("acme"));
        assert_eq!(id.repository.as_deref(), Some("acme/widget"));
        assert_eq!(id.workflow.as_deref(), Some("deploy.yml"));
        assert_eq!(id.branch(), Some("main"));
        assert_eq!(id.environment.as_deref(), Some("production"));
        assert_eq!(id.actor.as_deref(), Some("octocat"));
    }

    #[test]
    fn github_falls_back_to_the_display_name_without_a_workflow_ref() {
        let id = Provider::GitHubActions.identify(&claims(serde_json::json!({
            "workflow": "Deploy to production",
        })));
        assert_eq!(id.workflow.as_deref(), Some("Deploy to production"));
    }

    #[test]
    fn gitlab_claims_map_and_the_ref_is_reconstructed() {
        let id = Provider::GitLab.identify(&claims(serde_json::json!({
            "namespace_path": "acme",
            "project_path": "acme/widget",
            "ci_config_ref_uri": "gitlab.com/acme/widget//.gitlab-ci.yml@refs/heads/main",
            "ref": "main",
            "ref_type": "branch",
            "environment": "production",
            "user_login": "octocat",
        })));
        assert_eq!(id.organization.as_deref(), Some("acme"));
        assert_eq!(id.repository.as_deref(), Some("acme/widget"));
        assert_eq!(id.git_ref.as_deref(), Some("refs/heads/main"));
        assert_eq!(id.branch(), Some("main"));
        assert_eq!(id.actor.as_deref(), Some("octocat"));

        let tag = Provider::GitLab.identify(&claims(serde_json::json!({
            "ref": "v1.2.0", "ref_type": "tag",
        })));
        assert_eq!(tag.git_ref.as_deref(), Some("refs/tags/v1.2.0"));
    }

    #[test]
    fn buildkite_claims_map_the_slugs_and_branch() {
        let id = Provider::Buildkite.identify(&claims(serde_json::json!({
            "organization_slug": "acme",
            "pipeline_slug": "widget-deploy",
            "build_branch": "main",
        })));
        assert_eq!(id.organization.as_deref(), Some("acme"));
        assert_eq!(id.repository.as_deref(), Some("widget-deploy"));
        assert_eq!(id.git_ref.as_deref(), Some("refs/heads/main"));
    }

    #[test]
    fn a_generic_mapping_reads_the_named_claims_verbatim() {
        let names = ClaimNames {
            organization: Some("org".to_owned()),
            repository: Some("proj".to_owned()),
            git_ref: Some("branch_ref".to_owned()),
            ..Default::default()
        };
        let id = Provider::Generic(names).identify(&claims(serde_json::json!({
            "org": "acme",
            "proj": "acme/widget",
            "branch_ref": "refs/heads/main",
            "actor": "ignored -- not mapped",
        })));
        assert_eq!(id.organization.as_deref(), Some("acme"));
        assert_eq!(id.repository.as_deref(), Some("acme/widget"));
        assert_eq!(id.git_ref.as_deref(), Some("refs/heads/main"));
        assert_eq!(id.actor, None);
    }
}
