//! The **SkiMasque control protocol** — the contract between a gateway
//! (`skimasque-server --control-plane <url>`) and a control plane.
//!
//! SkiMasque opens this protocol, not the SkiMasque Cloud implementation of it.
//! Anything that speaks these endpoints and wire shapes can drive an
//! open-source gateway; that is what makes a fully self-hosted deployment
//! ("Mode 3") possible without SkiMasque Cloud.
//!
//! # Shape
//!
//! Plain HTTPS with JSON bodies. Two audiences:
//!
//! - **Gateway ↔ control plane** (this crate): registration, policy
//!   distribution (ETag + long-poll), heartbeats, label declaration, credential
//!   minting, audit shipping, and the org signing key. A registered gateway
//!   authenticates every call after registration with
//!   `Authorization: Bearer <secret>`, the value it received at registration.
//! - **Client ↔ control plane** (not modelled here): `skimasque login`, org and
//!   policy management, usage and audit queries. That surface is
//!   management-only and off the enforcement path.
//!
//! # Invariant
//!
//! The control plane decides *desired* state; the gateway enforces. A gateway
//! that loses the control plane keeps enforcing the last policy it cached and
//! never fails open. Nothing here lets the control plane reach into live
//! enforcement.

use serde::{Deserialize, Serialize};
use skimasque_policy::WorkloadIdentity;

/// The protocol revision this crate implements. Bumped only on a
/// wire-incompatible change; a control plane may advertise the versions it
/// accepts out of band.
pub const PROTOCOL_VERSION: &str = "v1";

/// The `prev_hash` of a gateway's very first audit event (see [`audit_hash`]):
/// 64 hex zeros.
pub const AUDIT_GENESIS: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// The gateway-facing endpoint paths, relative to the control plane's base URL.
///
/// `{id}` is the gateway id from [`RegisterResponse::gateway_id`]; `{org}` is
/// its [`RegisterResponse::org_id`].
pub mod paths {
    /// `POST` — register a new gateway with a one-time token. Unauthenticated.
    pub const REGISTER: &str = "/v1/gateways/register";

    /// `GET` — poll for policy. `If-None-Match: "<version>"` for a conditional
    /// fetch; `?wait=<seconds>` to long-poll. `304` unchanged, `204` no policy
    /// published yet, `200` + [`crate::PolicyResponse`] otherwise.
    pub fn gateway_policy(id: &str) -> String {
        format!("/v1/gateways/{id}/policy")
    }

    /// `POST` [`crate::HeartbeatRequest`] — liveness and usage counters.
    pub fn gateway_heartbeat(id: &str) -> String {
        format!("/v1/gateways/{id}/heartbeat")
    }

    /// `PUT` [`crate::LabelsRequest`] — declare labels for policy targeting.
    pub fn gateway_labels(id: &str) -> String {
        format!("/v1/gateways/{id}/labels")
    }

    /// `POST` [`crate::MintRequest`] — ask the control plane to sign a platform
    /// credential for an already-verified workload identity.
    pub fn gateway_credentials(id: &str) -> String {
        format!("/v1/gateways/{id}/credentials")
    }

    /// `POST` [`crate::ShipAuditRequest`] — ship a hash-chained batch of audit
    /// events.
    pub fn gateway_audit(id: &str) -> String {
        format!("/v1/gateways/{id}/audit")
    }

    /// `GET` — the tail ([`crate::AuditHead`]) of this gateway's audit chain,
    /// so a restart resumes its sequence.
    pub fn gateway_audit_head(id: &str) -> String {
        format!("/v1/gateways/{id}/audit/head")
    }

    /// `GET` — the org's Ed25519 signing key ([`crate::SigningKey`]).
    /// Unauthenticated; it is a public key.
    pub fn org_signing_key(org: &str) -> String {
        format!("/v1/orgs/{org}/signing-key")
    }
}

/// The identity the control plane issues at registration, and the gateway
/// persists so a restart does not re-register.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayIdentity {
    pub gateway_id: String,
    pub org_id: String,
    /// The bearer secret for every authenticated call after registration.
    pub secret: String,
}

/// `POST /v1/gateways/register`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub registration_token: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub labels: std::collections::BTreeMap<String, String>,
}

/// The `200` response to [`RegisterRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub gateway_id: String,
    pub org_id: String,
    pub secret: String,
}

/// One policy document: the file name it should have on disk and its verbatim
/// text (TOML or YAML). The gateway writes each to its `--policy-dir` cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDocument {
    pub name: String,
    pub text: String,
}

/// The `200` body of a policy poll (see [`paths::gateway_policy`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyResponse {
    /// The revision number. Also the `ETag` value the gateway echoes in
    /// `If-None-Match` on the next poll.
    pub version: u64,
    pub documents: Vec<PolicyDocument>,
}

/// `POST /v1/gateways/{id}/heartbeat`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    /// `"online"` or `"degraded"` — the latter once the gateway has been past
    /// its soft policy lease with the control plane unreachable. Enforcement
    /// continues either way.
    pub status: String,
    pub policy_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageReport>,
}

/// Cumulative-since-start counters a gateway reports in its heartbeat for
/// per-org usage accounting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageReport {
    pub tunnels_opened: u64,
    pub bytes_to_target: u64,
    pub bytes_to_client: u64,
}

/// `PUT /v1/gateways/{id}/labels`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelsRequest {
    pub labels: std::collections::BTreeMap<String, String>,
}

/// `POST /v1/gateways/{id}/credentials` — the gateway has already verified
/// `identity` from the runner's OIDC token; the control plane signs a platform
/// credential for it with the org's private key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintRequest {
    pub identity: WorkloadIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Requested lifetime in seconds. The control plane may cap it.
    pub ttl_seconds: u64,
}

/// The response to [`MintRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintResponse {
    /// The signed credential (a JWT), for `Proxy-Authorization: Bearer`.
    pub credential: String,
    /// Its actual lifetime in seconds (after any cap).
    pub expires_in: u64,
}

/// The org's Ed25519 signing key, served by [`paths::org_signing_key`]. A
/// gateway verifies control-plane-minted credentials against this offline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningKey {
    pub org_id: String,
    /// Always `"ed25519"` today.
    pub algorithm: String,
    /// The raw 32-byte public key, standard base64.
    pub public_key_b64: String,
    /// The key from before the last rotation, still accepted until the
    /// credentials it signed expire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_public_key_b64: Option<String>,
}

/// A key-material decode failure.
#[derive(Debug, thiserror::Error)]
#[error("decoding an Ed25519 signing key: {0}")]
pub struct KeyDecodeError(#[from] base64::DecodeError);

impl SigningKey {
    /// The current public-key bytes, for a credential verifier.
    pub fn public_key_bytes(&self) -> Result<Vec<u8>, KeyDecodeError> {
        Ok(decode_key(&self.public_key_b64)?)
    }

    /// Every public key a presented credential may have been signed with: the
    /// current one, plus the previous one across a rotation.
    pub fn all_public_key_bytes(&self) -> Result<Vec<Vec<u8>>, KeyDecodeError> {
        let mut keys = vec![self.public_key_bytes()?];
        if let Some(prev) = &self.previous_public_key_b64 {
            keys.push(decode_key(prev)?);
        }
        Ok(keys)
    }
}

fn decode_key(b64: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::prelude::{Engine as _, BASE64_STANDARD};
    BASE64_STANDARD.decode(b64)
}

/// `GET /v1/gateways/{id}/audit/head` — the tail of a gateway's audit chain as
/// the control plane holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditHead {
    pub seq: u64,
    pub hash: String,
}

impl AuditHead {
    /// The head a gateway that has never shipped starts from.
    pub fn genesis() -> Self {
        Self {
            seq: 0,
            hash: AUDIT_GENESIS.to_owned(),
        }
    }
}

/// One event in a ship batch, already sequenced and hashed by the gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainedAuditEvent {
    pub seq: u64,
    pub prev_hash: String,
    /// The audit event verbatim as a JSON string — hashed exactly as sent, so
    /// the control plane can recompute the chain.
    pub event_json: String,
}

/// `POST /v1/gateways/{id}/audit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipAuditRequest {
    pub events: Vec<ChainedAuditEvent>,
}

/// The response to [`ShipAuditRequest`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipAuditResponse {
    /// The new chain-head sequence number after the batch.
    pub head_seq: u64,
}

/// `sha256(seq_be || prev_hash || event_json)` as lowercase hex — the audit
/// chain link. The control plane recomputes this and rejects a break.
pub fn audit_hash(seq: u64, prev_hash: &str, event_json: &str) -> String {
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        &[&seq.to_be_bytes()[..], prev_hash.as_bytes(), event_json.as_bytes()].concat(),
    );
    let mut hex = String::with_capacity(64);
    for byte in digest.as_ref() {
        hex.push(char::from_digit(u32::from(byte >> 4), 16).unwrap());
        hex.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap());
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_hash_is_stable_and_chains() {
        let h1 = audit_hash(1, AUDIT_GENESIS, r#"{"decision":"allow"}"#);
        assert_eq!(h1.len(), 64);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
        // Deterministic.
        assert_eq!(h1, audit_hash(1, AUDIT_GENESIS, r#"{"decision":"allow"}"#));
        // Position matters.
        assert_ne!(h1, audit_hash(2, AUDIT_GENESIS, r#"{"decision":"allow"}"#));
        assert_ne!(h1, audit_hash(1, &h1, r#"{"decision":"allow"}"#));
    }

    #[test]
    fn register_request_omits_empty_labels() {
        let req = RegisterRequest {
            registration_token: "skmreg_x".into(),
            name: "gw-1".into(),
            labels: Default::default(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("labels"), "{json}");
    }

    #[test]
    fn signing_key_round_trips_and_decodes() {
        let key = SigningKey {
            org_id: "org_1".into(),
            algorithm: "ed25519".into(),
            public_key_b64: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            previous_public_key_b64: None,
        };
        let round: SigningKey =
            serde_json::from_str(&serde_json::to_string(&key).unwrap()).unwrap();
        assert_eq!(key, round);
        assert_eq!(key.public_key_bytes().unwrap().len(), 32);
    }

    #[test]
    fn paths_are_versioned() {
        assert_eq!(paths::REGISTER, "/v1/gateways/register");
        assert_eq!(paths::gateway_policy("gw_1"), "/v1/gateways/gw_1/policy");
        assert_eq!(paths::org_signing_key("org_1"), "/v1/orgs/org_1/signing-key");
    }
}
