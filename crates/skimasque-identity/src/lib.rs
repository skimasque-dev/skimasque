//! Workload identity for skimasque: turn a signed CI OIDC token into the
//! [`WorkloadIdentity`] the policy engine evaluates.
//!
//! [`skimasque_policy`] takes a [`WorkloadIdentity`] as data and never asks
//! where it came from. This crate is one place it comes from: an OIDC token from
//! GitHub Actions, GitLab CI, Buildkite, or any issuer, verified against the
//! issuer's published signing keys, with its claims mapped by a [`Provider`]
//! onto the fields a policy matches -- `organization`, `repository`, `workflow`,
//! `ref`, `environment`, `actor`.
//!
//! ```no_run
//! # async fn run() -> Result<(), skimasque_identity::Error> {
//! use skimasque_identity::{OidcVerifier, Provider};
//!
//! // The audience is whatever the workflow asks the issuer to mint the token for.
//! let verifier = OidcVerifier::hosted(Provider::GitHubActions, ["https://masque.example"])?;
//! let identity = verifier.verify("<the OIDC JWT>").await?;
//! assert_eq!(identity.organization.as_deref(), Some("acme"));
//! # Ok(()) }
//! ```
//!
//! Verification is split so the cryptography is testable without a network:
//! [`Verifier::verify`] is a pure function from `(token, keys)` to [`Claims`],
//! [`Provider::identify`] maps those, and [`JwksCache`] over a [`JwksProvider`]
//! handles fetching and caching the keys.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

mod credential;
mod error;
mod jwks;
mod provider;
mod verify;

#[cfg(test)]
mod test_support;

use std::sync::Arc;
use std::time::Duration;

use skimasque_policy::WorkloadIdentity;

pub use credential::{
    CredentialIssuer, CredentialSigner, CredentialVerifier, Issued, CREDENTIAL_ISSUER,
};
pub use error::Error;
pub use jwks::{JwksCache, JwksProvider};
pub use provider::{Claims, ClaimNames, Provider, GITHUB_ACTIONS_ISSUER};
pub use verify::Verifier;

#[cfg(feature = "remote")]
pub use jwks::HttpJwksProvider;

/// Fetch a GitHub Actions OIDC token for `audience` from the runner.
///
/// Reads `ACTIONS_ID_TOKEN_REQUEST_URL` and `ACTIONS_ID_TOKEN_REQUEST_TOKEN`
/// from the environment, which GitHub sets only when the job has
/// `permissions: id-token: write`. Outside a suitably configured Actions run
/// this returns an error rather than a token.
#[cfg(feature = "remote")]
pub async fn github_actions_id_token(audience: &str) -> Result<String, Error> {
    let base = std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL").map_err(|_| {
        Error::Discovery(
            "ACTIONS_ID_TOKEN_REQUEST_URL is not set -- the job needs `permissions: id-token: write`"
                .to_owned(),
        )
    })?;
    let bearer = std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .map_err(|_| Error::Discovery("ACTIONS_ID_TOKEN_REQUEST_TOKEN is not set".to_owned()))?;

    #[derive(serde::Deserialize)]
    struct TokenResponse {
        value: String,
    }

    let response: TokenResponse = reqwest::Client::new()
        .get(&base)
        .query(&[("audience", audience)])
        .bearer_auth(bearer)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| Error::Discovery(format!("requesting a GitHub OIDC token: {e}")))?
        .json()
        .await
        .map_err(|e| Error::Discovery(format!("reading the GitHub OIDC token response: {e}")))?;

    Ok(response.value)
}

/// How long fetched signing keys are trusted before a refetch.
const JWKS_TTL: Duration = Duration::from_secs(3600);

/// An OIDC verifier: a [`Verifier`] and a signing-key cache, paired with the
/// [`Provider`] that maps the verified claims.
///
/// This is the type a gateway holds. [`verify`](Self::verify) does the network
/// I/O (lazily, and cached) and returns a ready-to-evaluate
/// [`WorkloadIdentity`].
#[derive(Debug)]
pub struct OidcVerifier {
    verifier: Verifier,
    jwks: JwksCache,
    provider: Provider,
}

impl OidcVerifier {
    /// Verify tokens from `provider`'s canonical hosted issuer
    /// ([`Provider::default_issuer`]), accepting any of `audiences` as `aud`.
    ///
    /// Errors for [`Provider::Generic`], which has no canonical issuer -- use
    /// [`hosted_at`](Self::hosted_at).
    #[cfg(feature = "remote")]
    pub fn hosted<I, S>(provider: Provider, audiences: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let issuer = provider.default_issuer().ok_or_else(|| {
            Error::Discovery("a generic OIDC provider has no default issuer".to_owned())
        })?;
        Self::hosted_at(provider, issuer, audiences)
    }

    /// As [`hosted`](Self::hosted) but for a specific `issuer` -- a GitLab
    /// self-managed instance, a GitHub Enterprise Server host, a generic issuer.
    #[cfg(feature = "remote")]
    pub fn hosted_at<I, S>(provider: Provider, issuer: &str, audiences: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let keys = HttpJwksProvider::new(issuer)?;
        Ok(Self {
            verifier: Verifier::new(issuer, audiences),
            jwks: JwksCache::new(Arc::new(keys), JWKS_TTL),
            provider,
        })
    }

    /// Build from a [`Verifier`] and a key source directly -- for a non-HTTP
    /// transport, or a test.
    pub fn from_parts(provider: Provider, verifier: Verifier, keys: Arc<dyn JwksProvider>) -> Self {
        Self {
            verifier,
            jwks: JwksCache::new(keys, JWKS_TTL),
            provider,
        }
    }

    /// Verify `token` and map its claims to a [`WorkloadIdentity`].
    ///
    /// If the token names a signing key the cache does not have, the key set is
    /// refreshed once -- the issuer may have rotated -- before giving up.
    pub async fn verify(&self, token: &str) -> Result<WorkloadIdentity, Error> {
        self.verify_claims(token)
            .await
            .map(|claims| self.provider.identify(&claims))
    }

    /// As [`verify`](Self::verify) but returning the raw verified [`Claims`].
    pub async fn verify_claims(&self, token: &str) -> Result<Claims, Error> {
        let keys = self.jwks.keys().await?;
        match self.verifier.verify(token, &keys) {
            Err(Error::UnknownKey) => {
                let keys = self.jwks.refresh().await?;
                self.verifier.verify(token, &keys)
            }
            other => other,
        }
    }

    /// The provider this verifier maps claims with.
    pub fn provider(&self) -> &Provider {
        &self.provider
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{sign, TestIssuer};
    use std::future::Future;
    use std::pin::Pin;

    fn token(issuer: &TestIssuer) -> String {
        sign(
            issuer,
            serde_json::json!({
                "iss": GITHUB_ACTIONS_ISSUER,
                "aud": "https://masque.example",
                "exp": now() + 3600,
                "repository": "acme/widget",
                "repository_owner": "acme",
                "workflow_ref": "acme/widget/.github/workflows/deploy.yml@refs/heads/main",
                "ref": "refs/heads/main",
                "actor": "octocat",
            }),
        )
    }

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn verifier_for(issuer: &TestIssuer) -> OidcVerifier {
        OidcVerifier::from_parts(
            Provider::GitHubActions,
            Verifier::new(GITHUB_ACTIONS_ISSUER, ["https://masque.example"]),
            Arc::new(StaticKeys(issuer.jwks_json().to_owned())),
        )
    }

    #[derive(Debug)]
    struct StaticKeys(String);

    impl JwksProvider for StaticKeys {
        fn fetch(
            &self,
        ) -> Pin<Box<dyn Future<Output = Result<jsonwebtoken::jwk::JwkSet, Error>> + Send + '_>>
        {
            let json = self.0.clone();
            Box::pin(
                async move { serde_json::from_str(&json).map_err(|e| Error::Jwks(e.to_string())) },
            )
        }
    }

    #[tokio::test]
    async fn a_github_token_verifies_into_a_workload_identity() {
        let issuer = TestIssuer::new();
        let identity = verifier_for(&issuer).verify(&token(&issuer)).await.unwrap();

        assert_eq!(identity.organization.as_deref(), Some("acme"));
        assert_eq!(identity.repository.as_deref(), Some("acme/widget"));
        assert_eq!(identity.workflow.as_deref(), Some("deploy.yml"));
        assert_eq!(identity.branch(), Some("main"));
        assert_eq!(identity.actor.as_deref(), Some("octocat"));
    }

    #[tokio::test]
    async fn a_token_signed_by_someone_else_is_rejected() {
        let issuer = TestIssuer::new();
        let impostor = TestIssuer::other();
        let result = verifier_for(&issuer).verify(&token(&impostor)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn a_gitlab_token_maps_through_the_gitlab_provider() {
        let issuer = TestIssuer::new();
        let token = sign(
            &issuer,
            serde_json::json!({
                "iss": "https://gitlab.example",
                "aud": "https://masque.example",
                "exp": now() + 3600,
                "namespace_path": "acme",
                "project_path": "acme/widget",
                "ref": "main",
                "ref_type": "branch",
                "user_login": "octocat",
                "environment": "production",
            }),
        );
        let verifier = OidcVerifier::from_parts(
            Provider::GitLab,
            Verifier::new("https://gitlab.example", ["https://masque.example"]),
            Arc::new(StaticKeys(issuer.jwks_json().to_owned())),
        );

        let identity = verifier.verify(&token).await.unwrap();
        assert_eq!(identity.repository.as_deref(), Some("acme/widget"));
        assert_eq!(identity.git_ref.as_deref(), Some("refs/heads/main"));
        assert_eq!(identity.environment.as_deref(), Some("production"));
    }

    /// GitHub Enterprise Server issues the same claim shapes from its own host.
    #[tokio::test]
    async fn a_github_enterprise_server_token_uses_the_github_provider_at_a_custom_issuer() {
        let issuer = TestIssuer::new();
        let token = sign(
            &issuer,
            serde_json::json!({
                "iss": "https://ghe.acme.example",
                "aud": "https://masque.example",
                "exp": now() + 3600,
                "repository": "acme/widget",
                "repository_owner": "acme",
                "ref": "refs/heads/main",
            }),
        );
        let verifier = OidcVerifier::from_parts(
            Provider::GitHubActions,
            Verifier::new("https://ghe.acme.example", ["https://masque.example"]),
            Arc::new(StaticKeys(issuer.jwks_json().to_owned())),
        );

        let identity = verifier.verify(&token).await.unwrap();
        assert_eq!(identity.repository.as_deref(), Some("acme/widget"));
    }
}
