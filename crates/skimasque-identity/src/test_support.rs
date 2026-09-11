//! Test-only: a throwaway RSA issuer that can sign tokens and publish a
//! matching JWK Set, so verification can be exercised end to end without a
//! network or a checked-in key.

use std::sync::{Arc, LazyLock};

use base64::Engine as _;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::RsaPrivateKey;

struct Inner {
    kid: String,
    encoding_key: EncodingKey,
    jwks_json: String,
}

/// A cheap handle to a shared generated keypair. Cloning is free; the RSA key
/// generation happens once per test binary.
#[derive(Clone)]
pub struct TestIssuer(Arc<Inner>);

static PRIMARY: LazyLock<TestIssuer> = LazyLock::new(|| TestIssuer::generate("skimasque-test-0"));
static SECONDARY: LazyLock<TestIssuer> = LazyLock::new(|| TestIssuer::generate("skimasque-test-1"));

impl TestIssuer {
    /// The primary issuer. Every call shares one keypair.
    pub fn new() -> Self {
        PRIMARY.clone()
    }

    /// A second issuer with an unrelated key and a different `kid`.
    pub fn other() -> Self {
        SECONDARY.clone()
    }

    fn generate(kid: &str) -> Self {
        let mut rng = rand::thread_rng();
        let key = RsaPrivateKey::new(&mut rng, 2048).expect("generate a test RSA key");

        let pem = key
            .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
            .expect("encode the test key as PKCS#1 PEM");
        let encoding_key =
            EncodingKey::from_rsa_pem(pem.as_bytes()).expect("load the test signing key");

        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let public = key.to_public_key();
        let jwks_json = serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": kid,
                "n": b64.encode(public.n().to_bytes_be()),
                "e": b64.encode(public.e().to_bytes_be()),
            }]
        })
        .to_string();

        Self(Arc::new(Inner {
            kid: kid.to_owned(),
            encoding_key,
            jwks_json,
        }))
    }

    /// The issuer's published signing keys.
    pub fn jwks(&self) -> JwkSet {
        serde_json::from_str(&self.0.jwks_json).expect("the test JWKS is valid")
    }

    /// The issuer's JWK Set as it would be served over HTTP.
    pub fn jwks_json(&self) -> &str {
        &self.0.jwks_json
    }
}

/// Sign `claims` as an RS256 JWT from `issuer`.
pub fn sign(issuer: &TestIssuer, claims: serde_json::Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(issuer.0.kid.clone());
    jsonwebtoken::encode(&header, &claims, &issuer.0.encoding_key).expect("sign the test token")
}
