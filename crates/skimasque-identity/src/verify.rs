//! The pure half of verification: signature and claim checks against a key set
//! the caller supplies. No I/O happens here, so it is exhaustively testable.

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};

use crate::{Claims, Error};

/// Checks a token's RS256 signature and its `iss` / `aud` / `exp` claims, and
/// returns the verified claims for a [`Provider`](crate::Provider) to map.
///
/// It is deliberately not responsible for *obtaining* the signing keys -- that
/// is [`JwksCache`](crate::JwksCache)'s job -- so that this part can be tested
/// with a key set built in the test itself.
#[derive(Debug, Clone)]
pub struct Verifier {
    issuer: String,
    audiences: Vec<String>,
    leeway_secs: u64,
}

impl Verifier {
    /// Require `iss` to equal `issuer` and `aud` to contain one of `audiences`.
    ///
    /// An empty `audiences` disables the audience check, which is only sensible
    /// when some other layer constrains who may reach the verifier.
    pub fn new<I, S>(issuer: impl Into<String>, audiences: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            issuer: issuer.into(),
            audiences: audiences.into_iter().map(Into::into).collect(),
            leeway_secs: 60,
        }
    }

    /// Clock-skew tolerance for the time-based claims, in seconds. Default 60.
    pub fn with_leeway_secs(mut self, secs: u64) -> Self {
        self.leeway_secs = secs;
        self
    }

    /// The issuer this verifier trusts.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Verify `token` against `keys` and return its claims.
    pub fn verify(&self, token: &str, keys: &JwkSet) -> Result<Claims, Error> {
        let header = decode_header(token).map_err(|e| Error::Malformed(e.to_string()))?;
        if header.alg != Algorithm::RS256 {
            return Err(Error::UnsupportedAlgorithm(format!("{:?}", header.alg)));
        }
        let kid = header
            .kid
            .ok_or_else(|| Error::Malformed("the token header carries no key id".to_owned()))?;
        let jwk = keys.find(&kid).ok_or(Error::UnknownKey)?;
        let key = DecodingKey::from_jwk(jwk)
            .map_err(|e| Error::Verification(format!("unusable signing key: {e}")))?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.leeway = self.leeway_secs;
        if self.audiences.is_empty() {
            validation.validate_aud = false;
        } else {
            validation.set_audience(&self.audiences);
        }

        decode::<Claims>(token, &key, &validation)
            .map(|data| data.claims)
            .map_err(|e| Error::Verification(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{sign, TestIssuer};

    #[test]
    fn a_well_formed_token_verifies_and_yields_its_claims() {
        let issuer = TestIssuer::new();
        let token = sign(
            &issuer,
            serde_json::json!({
                "iss": crate::GITHUB_ACTIONS_ISSUER,
                "aud": "https://masque.example",
                "exp": far_future(),
                "repository": "acme/widget",
                "repository_owner": "acme",
                "ref": "refs/heads/main",
            }),
        );

        let verifier = Verifier::new(crate::GITHUB_ACTIONS_ISSUER, ["https://masque.example"]);
        let claims = verifier.verify(&token, &issuer.jwks()).unwrap();
        assert_eq!(claims.get("repository"), Some("acme/widget"));
    }

    #[test]
    fn a_token_for_the_wrong_audience_is_rejected() {
        let issuer = TestIssuer::new();
        let token = sign(
            &issuer,
            serde_json::json!({
                "iss": crate::GITHUB_ACTIONS_ISSUER,
                "aud": "https://someone-else.example",
                "exp": far_future(),
            }),
        );
        let verifier = Verifier::new(crate::GITHUB_ACTIONS_ISSUER, ["https://masque.example"]);
        assert!(matches!(
            verifier.verify(&token, &issuer.jwks()),
            Err(Error::Verification(_))
        ));
    }

    #[test]
    fn an_expired_token_is_rejected() {
        let issuer = TestIssuer::new();
        let token = sign(
            &issuer,
            serde_json::json!({
                "iss": crate::GITHUB_ACTIONS_ISSUER,
                "aud": "https://masque.example",
                "exp": 1_000i64,
            }),
        );
        let verifier = Verifier::new(crate::GITHUB_ACTIONS_ISSUER, ["https://masque.example"]);
        assert!(verifier.verify(&token, &issuer.jwks()).is_err());
    }

    #[test]
    fn a_token_from_another_issuer_is_rejected() {
        let issuer = TestIssuer::new();
        let token = sign(
            &issuer,
            serde_json::json!({
                "iss": "https://evil.example",
                "aud": "https://masque.example",
                "exp": far_future(),
            }),
        );
        let verifier = Verifier::new(crate::GITHUB_ACTIONS_ISSUER, ["https://masque.example"]);
        assert!(verifier.verify(&token, &issuer.jwks()).is_err());
    }

    #[test]
    fn a_signature_from_an_unknown_key_is_an_unknown_key_error() {
        let real = TestIssuer::new();
        let impostor = TestIssuer::other();
        let token = sign(
            &impostor,
            serde_json::json!({
                "iss": crate::GITHUB_ACTIONS_ISSUER,
                "aud": "https://masque.example",
                "exp": far_future(),
            }),
        );
        let verifier = Verifier::new(crate::GITHUB_ACTIONS_ISSUER, ["https://masque.example"]);
        // `impostor` signs with a different key under a different `kid`, so the
        // lookup in the real key set misses entirely.
        let outcome = verifier.verify(&token, &real.jwks());
        assert!(
            matches!(outcome, Err(Error::UnknownKey)),
            "expected UnknownKey, got {outcome:?}"
        );
    }

    fn far_future() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 3600
    }
}
