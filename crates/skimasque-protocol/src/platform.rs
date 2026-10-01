//! The **platform** (multi-tenant) extension of the control protocol: one
//! gateway serving many organisations — SkiMasque Cloud's shared gateway
//! (Mode 1).
//!
//! A platform gateway registers with [`crate::paths::PLATFORM_REGISTER`] and
//! belongs to no single organisation. It learns its tenants from
//! [`crate::paths::platform_tenants`]: each tenant's slug, verified GitHub
//! owners, published policy and signing key. At token exchange it resolves a
//! job to exactly one tenant, from the OIDC audience
//! (`https://<gateway host>/o/<slug>`, see [`slug_from_audience`]) and the
//! job's GitHub owner (`WorkloadIdentity::organization`, compared through
//! [`normalize_owner`]). It then asks the control plane to mint a credential
//! scoped to that org ([`PlatformMintRequest`]).
//!
//! Nothing here replaces the single-org protocol in the crate root; a
//! customer-operated gateway (Mode 2) never uses these types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use skimasque_policy::WorkloadIdentity;

use crate::{PolicyDocument, SigningKey, UsageReport};

/// The longest organisation slug, in bytes (all slug bytes are ASCII).
pub const SLUG_MAX_LEN: usize = 39;

/// Machine-readable codes carried in a [`Refusal`].
pub mod refusal {
    /// The job's GitHub owner is not a verified owner of the organisation.
    pub const OWNER_NOT_VERIFIED: &str = "owner_not_verified";
    /// The organisation has used its plan's shared-gateway allowance.
    pub const OVER_CAP: &str = "over_cap";
    /// No organisation has that id or slug, or it is not a tenant.
    pub const UNKNOWN_ORG: &str = "unknown_org";
    /// A platform endpoint was called by a gateway that is not a platform gateway.
    pub const NOT_PLATFORM_GATEWAY: &str = "not_platform_gateway";
    /// A single-org endpoint was called by a platform gateway.
    pub const PLATFORM_GATEWAY: &str = "platform_gateway";
}

/// The JSON body of every `403` / `422` a platform endpoint returns. The
/// gateway relays `message` verbatim so it reaches the CI log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    /// One of the [`refusal`] codes.
    pub code: String,
    /// A sentence for a person, e.g. "`acme` is not verified for `widgets`."
    pub message: String,
}

/// `POST /v1/platform/register` — register a platform gateway with a one-time,
/// operator-issued platform registration token. Unauthenticated, like the
/// single-org registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformRegisterRequest {
    pub registration_token: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

/// The `200` response to [`PlatformRegisterRequest`]. There is no `org_id`:
/// a platform gateway belongs to no single organisation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformRegisterResponse {
    pub gateway_id: String,
    /// The bearer secret for every later platform call.
    pub secret: String,
}

/// The `200` body of a tenant poll ([`crate::paths::platform_tenants`]).
/// Polled like policy: `If-None-Match: "<version>"` and `?wait=<seconds>`;
/// `304` when unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantList {
    /// Bumped by any policy publish, owner-claim change, slug change, key
    /// rotation, or organisation creation or deletion.
    pub version: u64,
    pub tenants: Vec<Tenant>,
}

/// One organisation served by the platform gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tenant {
    pub org_id: String,
    /// Unique across tenants; see [`is_valid_slug`].
    pub slug: String,
    /// Verified GitHub owners, already normalised with [`normalize_owner`].
    pub owners: Vec<String>,
    /// GitHub's numeric account id for each owner in `owners` that has one
    /// recorded (login → id). A gateway matches the OIDC `repository_owner_id`
    /// against it, so a login renamed away and re-registered by someone else
    /// is not the same owner.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub owner_ids: BTreeMap<String, u64>,
    /// `None` until the organisation publishes a policy; every request is
    /// then denied.
    pub policy: Option<TenantPolicy>,
    /// The organisation's credential-verification key(s).
    pub signing_key: SigningKey,
}

/// A tenant's published policy, already filtered to the documents whose label
/// `target` this platform gateway satisfies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantPolicy {
    pub version: u64,
    pub documents: Vec<PolicyDocument>,
}

/// `POST /v1/platform/gateways/{id}/credentials` — mint a credential scoped to
/// `org_id` for an identity the gateway already verified from OIDC. The
/// control plane re-checks the owner and the plan allowance independently.
/// The response is [`crate::MintResponse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformMintRequest {
    pub org_id: String,
    pub identity: WorkloadIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// GitHub's numeric id for `identity.organization`: the OIDC
    /// `repository_owner_id`. The control plane refuses the mint when the
    /// owner's verified claim records an id and this is different or absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<u64>,
    pub ttl_seconds: u64,
}

/// One event in a platform audit batch: a single-org
/// [`crate::ChainedAuditEvent`] plus the organisation it belongs to. The chain
/// is per gateway and the hash covers `event_json` exactly as in the
/// single-org protocol ([`crate::audit_hash`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformAuditEvent {
    pub org_id: String,
    pub seq: u64,
    pub prev_hash: String,
    pub event_json: String,
}

/// `POST /v1/platform/gateways/{id}/audit`. The response is
/// [`crate::ShipAuditResponse`]. A batch naming any organisation that is not a
/// tenant is rejected whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformShipAuditRequest {
    pub events: Vec<PlatformAuditEvent>,
}

/// `POST /v1/platform/gateways/{id}/heartbeat`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformHeartbeatRequest {
    /// As the single-org heartbeat: `"online"` or `"degraded"`.
    pub status: String,
    /// The [`TenantList::version`] the gateway is enforcing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenants_version: Option<u64>,
    /// Cumulative-since-process-start counters per organisation id. The
    /// control plane diffs successive reports, as for the single-org
    /// `usage`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub usage_by_org: BTreeMap<String, UsageReport>,
}

/// Whether `s` is a well-formed organisation slug: 1–[`SLUG_MAX_LEN`] bytes of
/// lowercase ASCII letters, digits and `-`, not starting or ending with `-`.
pub fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= SLUG_MAX_LEN
        && !s.starts_with('-')
        && !s.ends_with('-')
        && s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The OIDC audience a tenant's workflows use: `<base_url>/o/<slug>`, where
/// `base_url` is the gateway's public origin, e.g.
/// `https://gateway.skimasque.com`.
pub fn tenant_audience(base_url: &str, slug: &str) -> String {
    format!("{}/o/{slug}", base_url.trim_end_matches('/'))
}

/// The slug named by `audience` if it is exactly
/// `<base_url>/o/<slug>` (one trailing `/` tolerated) and the slug is valid.
/// Case is not folded: a person who typed `Acme` gets "unknown organisation",
/// never another org's `acme`.
pub fn slug_from_audience<'a>(base_url: &str, audience: &'a str) -> Option<&'a str> {
    let prefix = format!("{}/o/", base_url.trim_end_matches('/'));
    let rest = audience.strip_prefix(prefix.as_str())?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    is_valid_slug(rest).then_some(rest)
}

/// Normalise a GitHub owner (user or organisation login) for comparison:
/// trimmed and lowercased. GitHub logins are case-insensitive ASCII.
pub fn normalize_owner(owner: &str) -> String {
    owner.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PolicyDocument, SigningKey, UsageReport};

    const BASE: &str = "https://gateway.skimasque.com";

    fn key() -> SigningKey {
        SigningKey {
            org_id: "org_1".into(),
            algorithm: "ed25519".into(),
            public_key_b64: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            previous_public_key_b64: None,
        }
    }

    #[test]
    fn valid_slugs_follow_the_spec_rules() {
        let longest = "a".repeat(SLUG_MAX_LEN);
        let too_long = "a".repeat(SLUG_MAX_LEN + 1);
        for ok in ["a", "acme", "acme-2", "a1-b2", longest.as_str()] {
            assert!(is_valid_slug(ok), "{ok:?} should be valid");
        }
        for bad in [
            "",
            "-acme",
            "acme-",
            "Acme",
            "ac me",
            "acme_co",
            "acmé",
            too_long.as_str(),
        ] {
            assert!(!is_valid_slug(bad), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn tenant_audience_is_base_plus_o_slug() {
        assert_eq!(
            tenant_audience(BASE, "acme"),
            "https://gateway.skimasque.com/o/acme"
        );
        // A trailing slash on the base is not doubled.
        assert_eq!(
            tenant_audience("https://gateway.skimasque.com/", "acme"),
            "https://gateway.skimasque.com/o/acme"
        );
    }

    #[test]
    fn slug_from_audience_round_trips_and_tolerates_a_trailing_slash() {
        assert_eq!(slug_from_audience(BASE, &tenant_audience(BASE, "acme")), Some("acme"));
        assert_eq!(
            slug_from_audience(BASE, "https://gateway.skimasque.com/o/acme/"),
            Some("acme")
        );
    }

    #[test]
    fn slug_from_audience_rejects_other_hosts_case_and_junk() {
        // A different host never yields a slug, even with the right path.
        assert_eq!(slug_from_audience(BASE, "https://evil.example/o/acme"), None);
        // Case is not folded: `Acme` is not silently `acme`.
        assert_eq!(slug_from_audience(BASE, "https://gateway.skimasque.com/o/Acme"), None);
        // No slug, extra segments, or the bare host.
        assert_eq!(slug_from_audience(BASE, "https://gateway.skimasque.com/o/"), None);
        assert_eq!(slug_from_audience(BASE, "https://gateway.skimasque.com/o/acme/x"), None);
        assert_eq!(slug_from_audience(BASE, "https://gateway.skimasque.com"), None);
    }

    #[test]
    fn owners_normalise_to_trimmed_lowercase() {
        assert_eq!(normalize_owner(" Acme "), "acme");
        assert_eq!(normalize_owner("octocat"), "octocat");
    }

    #[test]
    fn a_tenant_without_a_policy_serialises_policy_as_null_and_round_trips() {
        let t = Tenant {
            org_id: "org_1".into(),
            slug: "acme".into(),
            owners: vec!["acme".into()],
            owner_ids: BTreeMap::new(),
            policy: None,
            signing_key: key(),
        };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains(r#""policy":null"#), "{json}");
        let back: Tenant = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn a_tenant_list_round_trips_with_a_policy() {
        let list = TenantList {
            version: 42,
            tenants: vec![Tenant {
                org_id: "org_1".into(),
                slug: "acme".into(),
                owners: vec!["acme".into(), "octocat".into()],
                owner_ids: BTreeMap::new(),
                policy: Some(TenantPolicy {
                    version: 7,
                    documents: vec![PolicyDocument {
                        name: "prod.toml".into(),
                        text: "name = \"prod\"\n".into(),
                    }],
                }),
                signing_key: key(),
            }],
        };
        let back: TenantList =
            serde_json::from_str(&serde_json::to_string(&list).unwrap()).unwrap();
        assert_eq!(back, list);
    }

    #[test]
    fn owner_ids_are_optional_on_the_wire() {
        // Old JSON (before the fields existed) still reads.
        let tenant: Tenant = serde_json::from_value(serde_json::json!({
            "org_id": "org_1",
            "slug": "acme",
            "owners": ["acme"],
            "policy": null,
            "signing_key": key(),
        }))
        .unwrap();
        assert!(tenant.owner_ids.is_empty());
        let mint: PlatformMintRequest = serde_json::from_value(serde_json::json!({
            "org_id": "org_1",
            "identity": {},
            "ttl_seconds": 900,
        }))
        .unwrap();
        assert_eq!(mint.owner_id, None);

        // Empty values are omitted; set values round-trip.
        let json = serde_json::to_value(&tenant).unwrap();
        assert!(json.get("owner_ids").is_none(), "{json}");
        assert!(serde_json::to_value(&mint).unwrap().get("owner_id").is_none());
        let with = Tenant {
            owner_ids: [("acme".to_owned(), 900)].into_iter().collect(),
            ..tenant
        };
        let back: Tenant = serde_json::from_value(serde_json::to_value(&with).unwrap()).unwrap();
        assert_eq!(back.owner_ids.get("acme"), Some(&900));
        let mint = PlatformMintRequest {
            owner_id: Some(900),
            ..mint
        };
        let back: PlatformMintRequest =
            serde_json::from_value(serde_json::to_value(&mint).unwrap()).unwrap();
        assert_eq!(back.owner_id, Some(900));
    }

    #[test]
    fn platform_mint_request_omits_an_absent_subject() {
        let req = PlatformMintRequest {
            org_id: "org_1".into(),
            identity: Default::default(),
            subject: None,
            owner_id: None,
            ttl_seconds: 900,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("subject"), "{json}");
        assert!(json.contains(r#""org_id":"org_1""#), "{json}");
    }

    #[test]
    fn heartbeat_omits_empty_usage_and_accepts_it_missing() {
        let hb = PlatformHeartbeatRequest {
            status: "online".into(),
            tenants_version: None,
            usage_by_org: Default::default(),
        };
        let json = serde_json::to_string(&hb).unwrap();
        assert!(!json.contains("usage_by_org"), "{json}");
        let back: PlatformHeartbeatRequest =
            serde_json::from_str(r#"{"status":"online"}"#).unwrap();
        assert_eq!(back, hb);

        let mut with_usage = hb.clone();
        with_usage.usage_by_org.insert(
            "org_1".into(),
            UsageReport {
                tunnels_opened: 3,
                ..Default::default()
            },
        );
        let json = serde_json::to_string(&with_usage).unwrap();
        assert!(json.contains(r#""usage_by_org":{"org_1""#), "{json}");
    }

    #[test]
    fn audit_events_carry_their_org() {
        let batch = PlatformShipAuditRequest {
            events: vec![PlatformAuditEvent {
                org_id: "org_1".into(),
                seq: 1,
                prev_hash: crate::AUDIT_GENESIS.into(),
                event_json: r#"{"decision":"allow"}"#.into(),
            }],
        };
        let back: PlatformShipAuditRequest =
            serde_json::from_str(&serde_json::to_string(&batch).unwrap()).unwrap();
        assert_eq!(back, batch);
    }

    #[test]
    fn refusal_codes_are_stable_wire_strings() {
        assert_eq!(refusal::OWNER_NOT_VERIFIED, "owner_not_verified");
        assert_eq!(refusal::OVER_CAP, "over_cap");
        assert_eq!(refusal::UNKNOWN_ORG, "unknown_org");
        assert_eq!(refusal::NOT_PLATFORM_GATEWAY, "not_platform_gateway");
        assert_eq!(refusal::PLATFORM_GATEWAY, "platform_gateway");
        let r = Refusal {
            code: refusal::OVER_CAP.into(),
            message: "Monthly limit reached".into(),
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"code":"over_cap","message":"Monthly limit reached"}"#
        );
    }
}
