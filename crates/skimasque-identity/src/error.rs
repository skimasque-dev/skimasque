//! One error type for every way verification can fail.

/// Why a token could not be turned into a trusted identity.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The token is not a well-formed JWT.
    #[error("the token is malformed: {0}")]
    Malformed(String),

    /// The token is signed with an algorithm this verifier will not accept.
    /// CI OIDC tokens (GitHub, GitLab, Buildkite) are RS256; anything else is
    /// refused rather than negotiated.
    #[error("the token's signing algorithm is {0}, not RS256")]
    UnsupportedAlgorithm(String),

    /// No key in the issuer's published set matches the token's `kid`. The
    /// caller may refresh the key set once -- the issuer could have rotated --
    /// before treating this as final.
    #[error("no published signing key matches the token's key id")]
    UnknownKey,

    /// The signature did not check out, or a claim (`iss`, `aud`, `exp`, ...)
    /// failed validation.
    #[error("token verification failed: {0}")]
    Verification(String),

    /// The issuer's OIDC discovery document could not be fetched or parsed.
    #[error("could not fetch the issuer's OIDC metadata: {0}")]
    Discovery(String),

    /// The issuer's JWK Set could not be fetched or parsed.
    #[error("could not fetch the issuer's signing keys: {0}")]
    Jwks(String),
}
