//! Platform credentials: the short-lived token a gateway issues once it has
//! verified a stronger identity token.
//!
//! A GitHub OIDC token proves who a workload is, but verifying it costs a JWKS
//! lookup and an RSA check, and it is minted for a broad audience. So the
//! gateway verifies it *once*, at [token exchange](crate), and hands back a
//! platform credential carrying the mapped [`WorkloadIdentity`] and a short
//! expiry. Tunnels present that, and the gateway verifies it locally with no
//! network and no third-party keys.
//!
//! Two signing schemes, same claim shape:
//!
//! - **Symmetric (HS256), [`CredentialIssuer`].** One key both issues and
//!   verifies. A single-process gateway generates one at startup
//!   ([`CredentialIssuer::generate`]); a self-hosted fleet shares a configured
//!   secret so any member can verify what another issued.
//! - **Asymmetric (Ed25519), [`CredentialSigner`] + [`CredentialVerifier`].**
//!   D1 of the control-plane design: the control plane holds the private key
//!   and mints credentials; each gateway holds only the org's public key and
//!   verifies offline. A gateway cannot mint.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use skimasque_policy::WorkloadIdentity;

use crate::Error;

/// The `iss` claim on every platform credential, however it was signed.
pub const CREDENTIAL_ISSUER: &str = "urn:skimasque:gateway";

/// A freshly issued credential and how long it is good for.
#[derive(Debug, Clone)]
pub struct Issued {
    /// The credential, a signed JWT.
    pub token: String,
    /// Its lifetime from now.
    pub expires_in: Duration,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iss: String,
    iat: i64,
    exp: i64,
    /// The original identity token's subject, kept for audit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sub: Option<String>,
    #[serde(flatten)]
    identity: WorkloadIdentity,
}

/// Encode a credential for `identity`, signed with `key` under `algorithm`.
fn encode(
    algorithm: Algorithm,
    key: &EncodingKey,
    ttl: Duration,
    identity: &WorkloadIdentity,
    subject: Option<&str>,
) -> Result<Issued, Error> {
    let now = unix_now();
    let claims = Claims {
        iss: CREDENTIAL_ISSUER.to_owned(),
        iat: now,
        exp: now + ttl.as_secs() as i64,
        sub: subject.map(str::to_owned),
        identity: identity.clone(),
    };
    let token = jsonwebtoken::encode(&Header::new(algorithm), &claims, key)
        .map_err(|e| Error::Verification(format!("issuing a credential: {e}")))?;
    Ok(Issued {
        token,
        expires_in: ttl,
    })
}

/// Verify a credential signed under `algorithm` with `key`, recovering the
/// identity it carries.
fn decode(
    algorithm: Algorithm,
    key: &DecodingKey,
    token: &str,
) -> Result<WorkloadIdentity, Error> {
    let mut validation = Validation::new(algorithm);
    validation.set_issuer(&[CREDENTIAL_ISSUER]);
    validation.leeway = 30;
    validation.set_required_spec_claims(&["exp", "iss"]);

    jsonwebtoken::decode::<Claims>(token, key, &validation)
        .map(|data| data.claims.identity)
        .map_err(|e| Error::Verification(e.to_string()))
}

/// Issues and verifies platform credentials with a symmetric HS256 key.
#[derive(Clone)]
pub struct CredentialIssuer {
    encoding: EncodingKey,
    decoding: DecodingKey,
    ttl: Duration,
}

impl std::fmt::Debug for CredentialIssuer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialIssuer")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl CredentialIssuer {
    /// Use `secret` for HS256, issuing credentials that live for `ttl`.
    pub fn new(secret: &[u8], ttl: Duration) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            ttl,
        }
    }

    /// As [`new`](Self::new), with a 32-byte secret from the OS RNG.
    ///
    /// Credentials issued by one process cannot be verified by another, and do
    /// not survive a restart -- the client just exchanges again.
    pub fn generate(ttl: Duration) -> Self {
        let mut secret = [0u8; 32];
        getrandom::getrandom(&mut secret).expect("the OS RNG is available");
        Self::new(&secret, ttl)
    }

    /// The credential lifetime this issuer stamps.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Issue a credential for `identity`. `subject` is recorded for audit only.
    pub fn issue(
        &self,
        identity: &WorkloadIdentity,
        subject: Option<&str>,
    ) -> Result<Issued, Error> {
        self.issue_for(identity, subject, self.ttl)
    }

    /// As [`issue`](Self::issue) but with an explicit lifetime, overriding this
    /// issuer's default -- used to cap a fallback credential well below the
    /// normal TTL.
    pub fn issue_for(
        &self,
        identity: &WorkloadIdentity,
        subject: Option<&str>,
        ttl: Duration,
    ) -> Result<Issued, Error> {
        encode(Algorithm::HS256, &self.encoding, ttl, identity, subject)
    }

    /// Verify a credential and recover the identity it carries.
    pub fn verify(&self, token: &str) -> Result<WorkloadIdentity, Error> {
        decode(Algorithm::HS256, &self.decoding, token)
    }
}

/// Signs platform credentials with an Ed25519 private key -- the control
/// plane's half of D1's asymmetric issuance. It cannot verify; that is
/// [`CredentialVerifier`], which every gateway holds.
#[derive(Clone)]
pub struct CredentialSigner {
    encoding: EncodingKey,
    ttl: Duration,
}

impl std::fmt::Debug for CredentialSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialSigner")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl CredentialSigner {
    /// `pkcs8_der` is a PKCS#8 v2 Ed25519 private key -- exactly what `ring`'s
    /// `Ed25519KeyPair::generate_pkcs8` produces, which is what the control
    /// plane stores per organisation.
    pub fn from_pkcs8_der(pkcs8_der: &[u8], ttl: Duration) -> Self {
        Self {
            encoding: EncodingKey::from_ed_der(pkcs8_der),
            ttl,
        }
    }

    /// The credential lifetime this signer stamps.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Issue a credential for `identity`. `subject` is recorded for audit only.
    pub fn issue(
        &self,
        identity: &WorkloadIdentity,
        subject: Option<&str>,
    ) -> Result<Issued, Error> {
        encode(Algorithm::EdDSA, &self.encoding, self.ttl, identity, subject)
    }
}

/// Verifies platform credentials against one or more Ed25519 *public* keys,
/// offline -- the gateway's half of D1. It cannot issue.
///
/// More than one key is accepted so that, across a signing-key rotation, a
/// credential minted just before the rotation still verifies against the
/// outgoing key until it expires.
#[derive(Clone)]
pub struct CredentialVerifier {
    decoding: Vec<DecodingKey>,
}

impl std::fmt::Debug for CredentialVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialVerifier")
            .field("keys", &self.decoding.len())
            .finish()
    }
}

impl CredentialVerifier {
    /// `public_key` is the raw 32-byte Ed25519 public key the control plane
    /// serves at `GET /v1/orgs/{id}/signing-key`.
    pub fn from_ed_public_key(public_key: &[u8]) -> Self {
        Self {
            decoding: vec![DecodingKey::from_ed_der(public_key)],
        }
    }

    /// As [`from_ed_public_key`](Self::from_ed_public_key) but with several
    /// candidate keys (e.g. the current and previous signing keys). `verify`
    /// accepts a credential that any one of them validates. An empty slice
    /// makes a verifier that rejects everything.
    pub fn from_ed_public_keys<K: AsRef<[u8]>>(public_keys: &[K]) -> Self {
        Self {
            decoding: public_keys
                .iter()
                .map(|k| DecodingKey::from_ed_der(k.as_ref()))
                .collect(),
        }
    }

    /// Verify a credential and recover the identity it carries. Tries each key
    /// in turn.
    pub fn verify(&self, token: &str) -> Result<WorkloadIdentity, Error> {
        let mut last = Error::Verification("no signing key configured".to_owned());
        for key in &self.decoding {
            match decode(Algorithm::EdDSA, key, token) {
                Ok(identity) => return Ok(identity),
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> WorkloadIdentity {
        WorkloadIdentity {
            organization: Some("acme".to_owned()),
            repository: Some("acme/widget".to_owned()),
            workflow: Some("deploy.yml".to_owned()),
            git_ref: Some("refs/heads/main".to_owned()),
            ..Default::default()
        }
    }

    /// A fresh Ed25519 keypair as `(pkcs8_der, raw_public_key)`, the same
    /// encoding the control plane's `signing` module produces and stores.
    fn ed25519_keypair() -> (Vec<u8>, Vec<u8>) {
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        (pkcs8.as_ref().to_vec(), pair.public_key().as_ref().to_vec())
    }

    #[test]
    fn a_credential_round_trips_through_issue_and_verify() {
        let issuer = CredentialIssuer::generate(Duration::from_secs(900));
        let issued = issuer
            .issue(&identity(), Some("repo:acme/widget:ref:refs/heads/main"))
            .unwrap();
        assert_eq!(issued.expires_in, Duration::from_secs(900));

        let recovered = issuer.verify(&issued.token).unwrap();
        assert_eq!(recovered, identity());
    }

    #[test]
    fn another_issuer_cannot_verify_this_ones_credential() {
        let a = CredentialIssuer::generate(Duration::from_secs(900));
        let b = CredentialIssuer::generate(Duration::from_secs(900));
        let issued = a.issue(&identity(), None).unwrap();
        assert!(b.verify(&issued.token).is_err());
    }

    #[test]
    fn a_shared_secret_lets_a_second_issuer_verify() {
        let secret = b"the-fleet-shares-this-32byte-key!";
        let minter = CredentialIssuer::new(secret, Duration::from_secs(900));
        let checker = CredentialIssuer::new(secret, Duration::from_secs(60));
        let issued = minter.issue(&identity(), None).unwrap();
        assert_eq!(checker.verify(&issued.token).unwrap(), identity());
    }

    #[test]
    fn an_expired_credential_is_rejected() {
        let issuer = CredentialIssuer::new(b"secret", Duration::from_secs(900));
        let now = unix_now();
        let claims = Claims {
            iss: CREDENTIAL_ISSUER.to_owned(),
            iat: now - 10_000,
            exp: now - 3_600,
            sub: None,
            identity: identity(),
        };
        let ancient =
            jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &issuer.encoding)
                .unwrap();
        assert!(issuer.verify(&ancient).is_err());
    }

    #[test]
    fn a_garbage_string_is_rejected_not_panicked() {
        let issuer = CredentialIssuer::generate(Duration::from_secs(900));
        assert!(issuer.verify("not.a.jwt").is_err());
        assert!(issuer.verify("").is_err());
    }

    #[test]
    fn an_ed25519_credential_round_trips_from_signer_to_verifier() {
        let (pkcs8, public) = ed25519_keypair();
        let signer = CredentialSigner::from_pkcs8_der(&pkcs8, Duration::from_secs(600));
        let verifier = CredentialVerifier::from_ed_public_key(&public);

        let issued = signer
            .issue(&identity(), Some("repo:acme/widget:ref:refs/heads/main"))
            .unwrap();
        assert_eq!(issued.expires_in, Duration::from_secs(600));
        assert_eq!(verifier.verify(&issued.token).unwrap(), identity());
    }

    #[test]
    fn a_verifier_rejects_a_credential_from_a_different_org_key() {
        let (pkcs8_a, _public_a) = ed25519_keypair();
        let (_pkcs8_b, public_b) = ed25519_keypair();
        let signer = CredentialSigner::from_pkcs8_der(&pkcs8_a, Duration::from_secs(600));
        let other_org = CredentialVerifier::from_ed_public_key(&public_b);

        let issued = signer.issue(&identity(), None).unwrap();
        assert!(other_org.verify(&issued.token).is_err());
    }

    #[test]
    fn a_multi_key_verifier_accepts_a_credential_from_any_key() {
        let (pkcs8_old, public_old) = ed25519_keypair();
        let (pkcs8_new, public_new) = ed25519_keypair();
        let (pkcs8_x, _public_x) = ed25519_keypair();

        // The verifier holds the new (current) and old (previous) keys.
        let verifier = CredentialVerifier::from_ed_public_keys(&[public_new, public_old]);
        for pkcs8 in [&pkcs8_new, &pkcs8_old] {
            let signer = CredentialSigner::from_pkcs8_der(pkcs8, Duration::from_secs(600));
            let issued = signer.issue(&identity(), None).unwrap();
            assert_eq!(verifier.verify(&issued.token).unwrap(), identity());
        }

        // A credential from an unrelated key is still refused.
        let stranger = CredentialSigner::from_pkcs8_der(&pkcs8_x, Duration::from_secs(600));
        assert!(verifier
            .verify(&stranger.issue(&identity(), None).unwrap().token)
            .is_err());

        // An empty verifier rejects everything.
        let empty = CredentialVerifier::from_ed_public_keys::<Vec<u8>>(&[]);
        let signer = CredentialSigner::from_pkcs8_der(&pkcs8_new, Duration::from_secs(600));
        assert!(empty
            .verify(&signer.issue(&identity(), None).unwrap().token)
            .is_err());
    }

    #[test]
    fn an_ed25519_verifier_rejects_an_hs256_credential() {
        let (_pkcs8, public) = ed25519_keypair();
        let verifier = CredentialVerifier::from_ed_public_key(&public);
        // A well-formed HS256 credential must not verify against the Ed25519
        // path -- the algorithm is pinned, so an attacker cannot downgrade.
        let hs = CredentialIssuer::generate(Duration::from_secs(600));
        let issued = hs.issue(&identity(), None).unwrap();
        assert!(verifier.verify(&issued.token).is_err());
    }

    #[test]
    fn an_expired_ed25519_credential_is_rejected() {
        let (pkcs8, public) = ed25519_keypair();
        let signer = CredentialSigner::from_pkcs8_der(&pkcs8, Duration::from_secs(600));
        let verifier = CredentialVerifier::from_ed_public_key(&public);
        let now = unix_now();
        let claims = Claims {
            iss: CREDENTIAL_ISSUER.to_owned(),
            iat: now - 10_000,
            exp: now - 3_600,
            sub: None,
            identity: identity(),
        };
        let ancient =
            jsonwebtoken::encode(&Header::new(Algorithm::EdDSA), &claims, &signer.encoding).unwrap();
        assert!(verifier.verify(&ancient).is_err());
    }
}
