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
    /// The SkiMasque organisation this credential is scoped to, set by a
    /// multi-tenant (platform) mint. Absent on single-org credentials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    org_id: Option<String>,
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
    org_id: Option<&str>,
) -> Result<Issued, Error> {
    let now = unix_now();
    let claims = Claims {
        iss: CREDENTIAL_ISSUER.to_owned(),
        iat: now,
        exp: now + ttl.as_secs() as i64,
        sub: subject.map(str::to_owned),
        org_id: org_id.map(str::to_owned),
        identity: identity.clone(),
    };
    let token = jsonwebtoken::encode(&Header::new(algorithm), &claims, key)
        .map_err(|e| Error::Verification(format!("issuing a credential: {e}")))?;
    Ok(Issued {
        token,
        expires_in: ttl,
    })
}

/// Verify a credential signed under `algorithm` with `key`, recovering its
/// claims.
fn decode_claims(algorithm: Algorithm, key: &DecodingKey, token: &str) -> Result<Claims, Error> {
    let mut validation = Validation::new(algorithm);
    validation.set_issuer(&[CREDENTIAL_ISSUER]);
    validation.leeway = 30;
    validation.set_required_spec_claims(&["exp", "iss"]);

    jsonwebtoken::decode::<Claims>(token, key, &validation)
        .map(|data| data.claims)
        .map_err(|e| Error::Verification(e.to_string()))
}

/// Verify a credential signed under `algorithm` with `key`, recovering the
/// identity it carries.
fn decode(algorithm: Algorithm, key: &DecodingKey, token: &str) -> Result<WorkloadIdentity, Error> {
    decode_claims(algorithm, key, token).map(|c| c.identity)
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
        encode(
            Algorithm::HS256,
            &self.encoding,
            ttl,
            identity,
            subject,
            None,
        )
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
        encode(
            Algorithm::EdDSA,
            &self.encoding,
            self.ttl,
            identity,
            subject,
            None,
        )
    }

    /// Issue a credential scoped to SkiMasque organisation `org_id` -- the
    /// multi-tenant (platform gateway) mint. A verifier recovers the org with
    /// [`CredentialVerifier::verify_claims`]. An empty `org_id` is refused so
    /// a credential can never look org-scoped without naming an org.
    pub fn issue_for_org(
        &self,
        identity: &WorkloadIdentity,
        subject: Option<&str>,
        org_id: &str,
    ) -> Result<Issued, Error> {
        let org_id = org_id.trim();
        if org_id.is_empty() {
            return Err(Error::Verification(
                "issuing an org-scoped credential needs an org id".to_owned(),
            ));
        }
        encode(
            Algorithm::EdDSA,
            &self.encoding,
            self.ttl,
            identity,
            subject,
            Some(org_id),
        )
    }
}

/// What a verified credential says: the workload identity and, for a
/// multi-tenant (platform) credential, the organisation it is scoped to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCredential {
    pub identity: WorkloadIdentity,
    /// `Some` only for a credential minted by
    /// [`CredentialSigner::issue_for_org`].
    pub org_id: Option<String>,
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

    /// As [`verify`](Self::verify), but also returns the organisation the
    /// credential is scoped to, if any.
    ///
    /// The returned `org_id` is only as trustworthy as the keys this verifier
    /// holds: a verifier built from several organisations' keys will accept a
    /// token one org's key signed that *claims* another org. A multi-tenant
    /// gateway must use [`verify_for_org`](Self::verify_for_org) with a
    /// verifier holding only the expected org's keys.
    pub fn verify_claims(&self, token: &str) -> Result<VerifiedCredential, Error> {
        let mut last = Error::Verification("no signing key configured".to_owned());
        for key in &self.decoding {
            match decode_claims(Algorithm::EdDSA, key, token) {
                Ok(claims) => {
                    return Ok(VerifiedCredential {
                        identity: claims.identity,
                        org_id: claims.org_id,
                    })
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Verify a credential scoped to `expected_org`: the signature must check
    /// against this verifier's keys **and** the credential's `org_id` claim
    /// must equal `expected_org`. A missing claim is refused. Build this
    /// verifier from `expected_org`'s signing key(s) alone; pick them with
    /// [`peek_org_id`].
    pub fn verify_for_org(
        &self,
        token: &str,
        expected_org: &str,
    ) -> Result<WorkloadIdentity, Error> {
        let verified = self.verify_claims(token)?;
        match verified.org_id.as_deref() {
            Some(org) if org == expected_org => Ok(verified.identity),
            Some(org) => Err(Error::Verification(format!(
                "credential is scoped to {org:?}, not {expected_org:?}"
            ))),
            None => Err(Error::Verification(
                "credential is not scoped to an organisation".to_owned(),
            )),
        }
    }
}

/// The `org_id` claim of `token`, read **without verifying anything**. Use it
/// only to choose which organisation's keys to verify with (then call
/// [`CredentialVerifier::verify_for_org`] with the same org). `None` for a
/// malformed token or one with no `org_id`.
pub fn peek_org_id(token: &str) -> Option<String> {
    use base64::prelude::{Engine as _, BASE64_URL_SAFE_NO_PAD};
    #[derive(Deserialize)]
    struct OrgOnly {
        org_id: Option<String>,
    }
    let payload = token.split('.').nth(1)?;
    let bytes = BASE64_URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice::<OrgOnly>(&bytes).ok()?.org_id
}

/// The identity a platform credential carries, read **without verifying the
/// signature or expiry**. For display and for asking a control plane what a
/// gateway would decide — never for an authorization decision; a gateway
/// verifies with [`CredentialVerifier`] or [`CredentialIssuer`].
pub fn peek_identity(token: &str) -> Result<WorkloadIdentity, Error> {
    use base64::prelude::{Engine as _, BASE64_URL_SAFE_NO_PAD};
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| Error::Malformed("not a JWT".to_owned()))?;
    let bytes = BASE64_URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|e| Error::Malformed(e.to_string()))?;
    serde_json::from_slice::<Claims>(&bytes)
        .map(|claims| claims.identity)
        .map_err(|e| Error::Malformed(e.to_string()))
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
            org_id: None,
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
            org_id: None,
            identity: identity(),
        };
        let ancient =
            jsonwebtoken::encode(&Header::new(Algorithm::EdDSA), &claims, &signer.encoding)
                .unwrap();
        assert!(verifier.verify(&ancient).is_err());
    }

    #[test]
    fn an_org_scoped_credential_carries_its_org_through_verification() {
        let (pkcs8, public) = ed25519_keypair();
        let signer = CredentialSigner::from_pkcs8_der(&pkcs8, Duration::from_secs(600));
        let verifier = CredentialVerifier::from_ed_public_key(&public);

        let issued = signer
            .issue_for_org(&identity(), Some("repo:acme/widget"), "org_acme")
            .unwrap();
        let verified = verifier.verify_claims(&issued.token).unwrap();
        assert_eq!(verified.identity, identity());
        assert_eq!(verified.org_id.as_deref(), Some("org_acme"));
    }

    #[test]
    fn a_credential_without_an_org_verifies_with_no_org() {
        let (pkcs8, public) = ed25519_keypair();
        let signer = CredentialSigner::from_pkcs8_der(&pkcs8, Duration::from_secs(600));
        let verifier = CredentialVerifier::from_ed_public_key(&public);

        let issued = signer.issue(&identity(), None).unwrap();
        let verified = verifier.verify_claims(&issued.token).unwrap();
        assert_eq!(verified.identity, identity());
        assert_eq!(verified.org_id, None);
    }

    #[test]
    fn the_plain_verify_still_accepts_an_org_scoped_credential() {
        // Backward compatibility: gateways that predate org scoping call
        // `verify`, which must keep returning the identity.
        let (pkcs8, public) = ed25519_keypair();
        let signer = CredentialSigner::from_pkcs8_der(&pkcs8, Duration::from_secs(600));
        let verifier = CredentialVerifier::from_ed_public_key(&public);

        let issued = signer.issue_for_org(&identity(), None, "org_acme").unwrap();
        assert_eq!(verifier.verify(&issued.token).unwrap(), identity());
    }

    #[test]
    fn an_empty_org_id_is_refused_at_issue() {
        let (pkcs8, _public) = ed25519_keypair();
        let signer = CredentialSigner::from_pkcs8_der(&pkcs8, Duration::from_secs(600));
        assert!(signer.issue_for_org(&identity(), None, "").is_err());
        assert!(signer.issue_for_org(&identity(), None, "   ").is_err());
    }

    #[test]
    fn verify_claims_rejects_a_signature_from_an_unknown_key() {
        let (pkcs8_a, _public_a) = ed25519_keypair();
        let (_pkcs8_b, public_b) = ed25519_keypair();
        let signer_a = CredentialSigner::from_pkcs8_der(&pkcs8_a, Duration::from_secs(600));
        let verifier_b = CredentialVerifier::from_ed_public_key(&public_b);

        let issued = signer_a.issue_for_org(&identity(), None, "org_b").unwrap();
        assert!(verifier_b.verify_claims(&issued.token).is_err());
    }

    #[test]
    fn verify_for_org_rejects_a_credential_naming_a_different_org_even_with_a_valid_key() {
        // Invariant 1: a verifier holding org A's key must not accept a token
        // that A's key signed but that claims org B.
        let (pkcs8_a, public_a) = ed25519_keypair();
        let signer_a = CredentialSigner::from_pkcs8_der(&pkcs8_a, Duration::from_secs(600));
        let verifier_a = CredentialVerifier::from_ed_public_key(&public_a);

        let claims_b = signer_a.issue_for_org(&identity(), None, "org_b").unwrap();
        assert!(verifier_a.verify_for_org(&claims_b.token, "org_a").is_err());

        let claims_a = signer_a.issue_for_org(&identity(), None, "org_a").unwrap();
        assert_eq!(
            verifier_a.verify_for_org(&claims_a.token, "org_a").unwrap(),
            identity()
        );
    }

    #[test]
    fn verify_for_org_rejects_a_credential_with_no_org() {
        let (pkcs8, public) = ed25519_keypair();
        let signer = CredentialSigner::from_pkcs8_der(&pkcs8, Duration::from_secs(600));
        let verifier = CredentialVerifier::from_ed_public_key(&public);
        let issued = signer.issue(&identity(), None).unwrap();
        assert!(verifier.verify_for_org(&issued.token, "org_a").is_err());
    }

    #[test]
    fn peek_org_id_reads_the_claim_without_verifying_and_never_panics() {
        let (pkcs8, _public) = ed25519_keypair();
        let signer = CredentialSigner::from_pkcs8_der(&pkcs8, Duration::from_secs(600));
        let scoped = signer.issue_for_org(&identity(), None, "org_acme").unwrap();
        assert_eq!(peek_org_id(&scoped.token).as_deref(), Some("org_acme"));
        let plain = signer.issue(&identity(), None).unwrap();
        assert_eq!(peek_org_id(&plain.token), None);
        for junk in ["", "abc", "a.b.c", "a.!!!.c", "a..c"] {
            assert_eq!(peek_org_id(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn peek_identity_reads_the_claims_without_a_key() {
        let issuer = CredentialIssuer::generate(Duration::from_secs(900));
        let issued = issuer.issue(&identity(), Some("sub")).unwrap();
        assert_eq!(peek_identity(&issued.token).unwrap(), identity());
    }

    #[test]
    fn peek_identity_rejects_something_that_is_not_a_jwt() {
        assert!(matches!(
            peek_identity("not-a-token"),
            Err(Error::Malformed(_))
        ));
        assert!(matches!(peek_identity("a.%%%.c"), Err(Error::Malformed(_))));
    }
}
