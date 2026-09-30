//! The platform (multi-tenant) gateway's tenant resolution: the live tenant
//! table and its sync from the control plane, the tunnel-side credential
//! verifier, and the token-exchange minter.
//!
//! This is the code that decides which customer a credential or a CI job
//! belongs to, so it is deliberately narrow:
//!
//! - a presented credential is verified with **only** the keys of the org it
//!   claims ([`PlatformTenantVerifier`]), never with a verifier that holds
//!   several orgs' keys;
//! - a CI job is resolved to exactly one tenant from its OIDC audience, and
//!   its GitHub owner must be one of that tenant's verified owners before the
//!   control plane is asked for anything ([`PlatformMinter`]);
//! - the gateway never mints a credential itself: a control-plane outage is a
//!   retryable error, not a local fallback.
//!
//! The tenant table survives restarts through `<state>/tenants.json`, so a
//! gateway that comes back up while the control plane is down keeps enforcing
//! the tenants it last knew.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use skimasque::{
    CredentialMinter, MintError, MintedCredential, TenantId, TenantSpec, TenantTable,
    TenantVerifier,
};
use skimasque_identity::{peek_audiences, peek_org_id, CredentialVerifier, OidcVerifier};
use skimasque_policy::{PolicySet, WorkloadIdentity};
use skimasque_protocol::platform::{
    normalize_owner, refusal, slug_from_audience, tenant_audience, PlatformMintRequest, Tenant,
    TenantList,
};
use tokio::sync::watch;

use crate::control::{ControlPlane, GatewayIdentity, PlatformMintError, TenantFetch};

/// The file, under the state directory, that caches the last tenant list.
const CACHE_FILE: &str = "tenants.json";

/// What a token exchange answers when the control plane cannot mint.
const UNAVAILABLE: &str = "SkiMasque control plane unreachable; try again shortly.";

/// Where a customer verifies a GitHub owner, quoted in an `owner_not_verified`
/// refusal. Override with [`PlatformMinter::with_owner_settings_url`].
pub const DEFAULT_OWNER_SETTINGS_URL: &str = "https://control.skimasque.com/settings/owners";

type KeyMap = HashMap<String, Arc<CredentialVerifier>>;

/// The live platform state: the library's tenant table plus, per org id, that
/// org's credential-verification keys. The table is the authority: the key map
/// is only consulted for an org the table currently resolves.
#[derive(Clone, Debug)]
pub struct PlatformTenants {
    pub table: TenantTable,
    keys: watch::Sender<Arc<KeyMap>>,
}

impl Default for PlatformTenants {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformTenants {
    /// No tenants: every credential and every exchange is refused until the
    /// first [`apply`](Self::apply).
    pub fn new() -> Self {
        let (keys, _) = watch::channel(Arc::new(KeyMap::new()));
        Self {
            table: TenantTable::new(),
            keys,
        }
    }

    /// Replace the tenants with `list`.
    ///
    /// Each tenant's policy is parsed on its own: a policy that does not parse
    /// leaves that tenant with an empty (deny-all) set and never affects the
    /// others, and a tenant with no published policy is deny-all. Each
    /// tenant's verifier holds only its own key(s); a key that does not decode
    /// leaves that tenant with a verifier that rejects everything.
    ///
    /// A later entry repeating an earlier `org_id` or `slug` is dropped here,
    /// before either the table or the key map sees it, so the two always
    /// describe the same tenants.
    pub fn apply(&self, list: &TenantList) {
        let mut specs = Vec::with_capacity(list.tenants.len());
        let mut keys = KeyMap::with_capacity(list.tenants.len());
        for tenant in dedup(&list.tenants) {
            keys.insert(tenant.org_id.clone(), Arc::new(verifier_for(tenant)));
            specs.push(TenantSpec {
                org_id: tenant.org_id.clone(),
                slug: tenant.slug.clone(),
                owners: tenant.owners.iter().map(|o| normalize_owner(o)).collect(),
                owner_ids: tenant.owner_ids.clone(),
                policy: Arc::new(policy_for(tenant)),
            });
        }
        // Keys first, then the table: in between, an org the old table still
        // resolves but the new list dropped has no key and is refused, and an
        // org only the new list has is not yet resolvable. Both fail closed.
        self.keys.send_replace(Arc::new(keys));
        self.table.store(list.version, specs);
        metrics::gauge!("skimasque_platform_tenants")
            .set(self.table.snapshot().tenants().count() as f64);
    }

    /// Only the verifier of `org_id`, whose table entry the caller already
    /// resolved.
    fn verifier(&self, org_id: &str) -> Option<Arc<CredentialVerifier>> {
        self.keys.borrow().get(org_id).cloned()
    }

    /// The tenant list cached by [`write_cache`](Self::write_cache), if there
    /// is one that parses.
    pub fn load_cache(state_dir: &Path) -> Option<TenantList> {
        let path = state_dir.join(CACHE_FILE);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "could not read the tenant cache");
                return None;
            }
        };
        match serde_json::from_slice(&bytes) {
            Ok(list) => Some(list),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "the tenant cache does not parse; ignoring it");
                None
            }
        }
    }

    /// Write `list` to `<state_dir>/tenants.json`, atomically: a crash
    /// mid-write leaves the previous cache in place, never a torn one.
    pub fn write_cache(state_dir: &Path, list: &TenantList) -> Result<()> {
        std::fs::create_dir_all(state_dir).context("creating the state directory")?;
        let json = serde_json::to_vec_pretty(list).context("serialising the tenant list")?;
        let staging = state_dir.join(format!("{CACHE_FILE}.new"));
        std::fs::write(&staging, json).context("writing the tenant cache")?;
        std::fs::rename(&staging, state_dir.join(CACHE_FILE))
            .context("swapping in the new tenant cache")?;
        Ok(())
    }
}

/// The tenants that survive de-duplication, in order: the first entry for an
/// `org_id` or a `slug` wins, and an entry with an empty `org_id` is dropped.
fn dedup(tenants: &[Tenant]) -> Vec<&Tenant> {
    let mut orgs = HashSet::new();
    let mut slugs = HashSet::new();
    let mut kept = Vec::with_capacity(tenants.len());
    for tenant in tenants {
        if tenant.org_id.trim().is_empty() {
            tracing::warn!(slug = %tenant.slug, "tenant with an empty org_id; dropping it");
            continue;
        }
        if orgs.contains(tenant.org_id.as_str()) {
            tracing::warn!(org_id = %tenant.org_id, "duplicate org_id in tenant list; dropping the later entry");
            continue;
        }
        if slugs.contains(tenant.slug.as_str()) {
            tracing::warn!(org_id = %tenant.org_id, slug = %tenant.slug, "duplicate slug in tenant list; dropping the later entry");
            continue;
        }
        orgs.insert(tenant.org_id.as_str());
        slugs.insert(tenant.slug.as_str());
        kept.push(tenant);
    }
    kept
}

/// `tenant`'s own key(s), or a verifier that rejects everything if they do not
/// decode.
fn verifier_for(tenant: &Tenant) -> CredentialVerifier {
    let key = &tenant.signing_key;
    if !key.algorithm.eq_ignore_ascii_case("ed25519") {
        tracing::warn!(org_id = %tenant.org_id, algorithm = %key.algorithm, "tenant signing key is not Ed25519; its credentials will be refused");
        return CredentialVerifier::from_ed_public_keys::<Vec<u8>>(&[]);
    }
    match key.all_public_key_bytes() {
        Ok(keys) => CredentialVerifier::from_ed_public_keys(&keys),
        Err(error) => {
            tracing::warn!(org_id = %tenant.org_id, %error, "tenant signing key does not decode; its credentials will be refused");
            CredentialVerifier::from_ed_public_keys::<Vec<u8>>(&[])
        }
    }
}

/// `tenant`'s policy, or an empty (deny-all) set if it has none or it does not
/// parse.
fn policy_for(tenant: &Tenant) -> PolicySet {
    let Some(policy) = &tenant.policy else {
        return PolicySet::new(Vec::new());
    };
    match PolicySet::from_documents(
        policy
            .documents
            .iter()
            .map(|d| (d.name.as_str(), d.text.as_str())),
    ) {
        Ok(set) => {
            metrics::counter!("skimasque_platform_tenant_policy_total", "outcome" => "applied")
                .increment(1);
            set
        }
        Err(error) => {
            metrics::counter!("skimasque_platform_tenant_policy_total", "outcome" => "rejected")
                .increment(1);
            tracing::warn!(
                org_id = %tenant.org_id,
                version = policy.version,
                %error,
                "tenant policy does not load; denying everything for this tenant"
            );
            PolicySet::new(Vec::new())
        }
    }
}

/// Long-poll the control plane for tenant-list changes, apply each one and
/// cache it to `<state_dir>/tenants.json`. On any error the current tenants
/// stay in force; the loop backs off (1s doubling to 60s) and retries. Never
/// returns.
pub async fn run_tenant_sync(
    control: ControlPlane,
    identity: GatewayIdentity,
    tenants: PlatformTenants,
    state_dir: PathBuf,
    interval: Duration,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        let known = match tenants.table.snapshot().version() {
            0 => None,
            v => Some(v),
        };
        match control
            .fetch_tenants(&identity, known, Some(interval))
            .await
        {
            Ok(TenantFetch::Updated(list)) => {
                tenants.apply(&list);
                if let Err(error) = PlatformTenants::write_cache(&state_dir, &list) {
                    tracing::warn!(%error, "could not cache the tenant list");
                }
                metrics::counter!("skimasque_platform_tenant_sync_total", "outcome" => "applied")
                    .increment(1);
                tracing::info!(
                    version = list.version,
                    tenants = list.tenants.len(),
                    "control plane pushed a new tenant list"
                );
                backoff = Duration::from_secs(1);
            }
            Ok(TenantFetch::Unchanged) => {
                backoff = Duration::from_secs(1);
            }
            Err(error) => {
                metrics::counter!("skimasque_platform_tenant_sync_total", "outcome" => "error")
                    .increment(1);
                tracing::warn!(
                    %error,
                    "control plane tenant poll failed; still enforcing the current tenants"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
}

/// The tunnel-side verifier for a platform gateway: the credential must name
/// an org the table holds, and verify against that org's key(s) alone. There
/// is no HS256 path and no other org's keys are ever tried.
///
/// The `Err` strings reach the client, so they are generic and never name an
/// org.
#[derive(Debug, Clone)]
pub struct PlatformTenantVerifier {
    tenants: PlatformTenants,
}

impl PlatformTenantVerifier {
    pub fn new(tenants: PlatformTenants) -> Self {
        Self { tenants }
    }

    fn verify_now(&self, token: &str) -> Result<(TenantId, WorkloadIdentity), String> {
        let org = peek_org_id(token)
            .ok_or_else(|| "credential is not scoped to an organisation".to_owned())?;
        let snapshot = self.tenants.table.snapshot();
        let tenant = snapshot
            .resolve_org(&org)
            .ok_or_else(|| "unknown organisation".to_owned())?;
        let verifier = self
            .tenants
            .verifier(tenant.id().as_str())
            .ok_or_else(|| "credential did not verify".to_owned())?;
        let identity = verifier
            .verify_for_org(token, tenant.id().as_str())
            .map_err(|error| {
                tracing::debug!(%error, "platform credential did not verify");
                "credential did not verify".to_owned()
            })?;
        Ok((tenant.id().clone(), identity))
    }
}

impl TenantVerifier for PlatformTenantVerifier {
    fn verify(
        &self,
        token: String,
    ) -> Pin<Box<dyn Future<Output = Result<(TenantId, WorkloadIdentity), String>> + Send>> {
        let result = self.verify_now(&token);
        Box::pin(async move { result })
    }
}

/// The token-exchange minter for a platform gateway: resolve the job to one
/// tenant from its OIDC audience, verify the token for exactly that audience,
/// check the job's GitHub owner is verified for the tenant, then ask the
/// control plane to mint. It never mints locally.
pub struct PlatformMinter {
    oidc: Arc<OidcVerifier>,
    base_url: String,
    control: ControlPlane,
    identity: GatewayIdentity,
    tenants: PlatformTenants,
    ttl: Duration,
    owner_settings_url: String,
}

impl std::fmt::Debug for PlatformMinter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformMinter")
            .field("base_url", &self.base_url)
            .field("gateway_id", &self.identity.gateway_id)
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl PlatformMinter {
    /// `base_url` is the gateway's public origin (`https://<hostname>`); a
    /// tenant's audience is `<base_url>/o/<slug>`. `ttl` is the credential
    /// lifetime requested of the control plane, which may cap it.
    pub fn new(
        oidc: Arc<OidcVerifier>,
        base_url: impl Into<String>,
        control: ControlPlane,
        identity: GatewayIdentity,
        tenants: PlatformTenants,
        ttl: Duration,
    ) -> Self {
        Self {
            oidc,
            base_url: base_url.into(),
            control,
            identity,
            tenants,
            ttl,
            owner_settings_url: DEFAULT_OWNER_SETTINGS_URL.to_owned(),
        }
    }

    /// The URL an `owner_not_verified` refusal points the customer at.
    pub fn with_owner_settings_url(mut self, url: impl Into<String>) -> Self {
        self.owner_settings_url = url.into();
        self
    }
}

fn refused(code: &str, message: String) -> MintError {
    MintError::Refused {
        code: code.to_owned(),
        message,
    }
}

/// The one slug `token`'s audiences name under `base_url`, with the audience
/// that named it. Several audiences naming the same slug (say with and
/// without a trailing `/`) are fine; naming two different slugs is ambiguous
/// and refused.
fn slug_of(base_url: &str, token: &str) -> Result<(String, String), MintError> {
    let mut found: Option<(String, String)> = None;
    for aud in peek_audiences(token) {
        let Some(slug) = slug_from_audience(base_url, &aud).map(str::to_owned) else {
            continue;
        };
        match &found {
            None => found = Some((slug, aud)),
            Some((seen, _)) if *seen == slug => {}
            Some(_) => {
                return Err(refused(
                    refusal::UNKNOWN_ORG,
                    "The token names more than one SkiMasque organisation. Check the                      `audience` in your workflow."
                        .to_owned(),
                ))
            }
        }
    }
    found.ok_or_else(|| {
        refused(
            refusal::UNKNOWN_ORG,
            format!(
                "Unknown SkiMasque organisation. Check the `audience` in your workflow. It must                  be `{}`.",
                tenant_audience(base_url, "<your-org-slug>")
            ),
        )
    })
}

impl CredentialMinter for PlatformMinter {
    fn mint(
        &self,
        identity_token: String,
    ) -> Pin<Box<dyn Future<Output = Result<MintedCredential, MintError>> + Send>> {
        let oidc = self.oidc.clone();
        let base_url = self.base_url.clone();
        let control = self.control.clone();
        let gateway = self.identity.clone();
        let snapshot = self.tenants.table.snapshot();
        let ttl = self.ttl;
        let settings_url = self.owner_settings_url.clone();
        Box::pin(async move {
            // 1. The audience names exactly one tenant.
            let (slug, audience) = slug_of(&base_url, &identity_token)?;
            let tenant = snapshot.resolve_slug(&slug).cloned().ok_or_else(|| {
                refused(
                    refusal::UNKNOWN_ORG,
                    format!(
                        "Unknown SkiMasque organisation `{slug}`. Check the `audience` in your \
                         workflow."
                    ),
                )
            })?;

            // 2. The token verifies for the audience that named the tenant:
            //    `<base_url>/o/<slug>` (or with a trailing `/`) for a valid
            //    slug, so never empty.
            let claims = oidc
                .verify_claims_for_audience(&identity_token, &audience)
                .await
                .map_err(|e| match e {
                    skimasque_identity::Error::Discovery(_)
                    | skimasque_identity::Error::Jwks(_) => MintError::Unavailable(e.to_string()),
                    other => MintError::Unauthorized(other.to_string()),
                })?;
            let workload = oidc.provider().identify(&claims);

            // 3. The job's GitHub owner is verified for this tenant.
            let owner = workload
                .organization
                .as_deref()
                .map(normalize_owner)
                .unwrap_or_default();
            if owner.is_empty() || !tenant.owns(&owner, claims.owner_id()) {
                metrics::counter!("skimasque_platform_mint_total", "outcome" => "owner_not_verified")
                    .increment(1);
                let message = if owner.is_empty() {
                    format!(
                        "The job's token names no repository owner, so it cannot be verified \
                         for `{slug}`."
                    )
                } else {
                    format!("`{owner}` is not verified for `{slug}`. Verify it at {settings_url}.")
                };
                return Err(refused(refusal::OWNER_NOT_VERIFIED, message));
            }

            // 4. The control plane mints (and re-checks) it. Never locally.
            let request = PlatformMintRequest {
                org_id: tenant.id().as_str().to_owned(),
                identity: workload,
                subject: claims.subject().map(str::to_owned),
                owner_id: claims.owner_id(),
                ttl_seconds: ttl.as_secs(),
            };
            match control.platform_mint(&gateway, &request).await {
                Ok(minted) => {
                    metrics::counter!("skimasque_platform_mint_total", "outcome" => "minted")
                        .increment(1);
                    Ok(MintedCredential {
                        credential: minted.token,
                        expires_in: minted.expires_in,
                    })
                }
                Err(PlatformMintError::Refused(r)) => {
                    metrics::counter!("skimasque_platform_mint_total", "outcome" => "refused")
                        .increment(1);
                    Err(MintError::Refused {
                        code: r.code,
                        message: r.message,
                    })
                }
                Err(PlatformMintError::Unavailable(error)) => {
                    metrics::counter!("skimasque_platform_mint_total", "outcome" => "unavailable")
                        .increment(1);
                    tracing::warn!(error = %format!("{error:#}"), "control plane could not mint a platform credential");
                    Err(MintError::Unavailable(UNAVAILABLE.to_owned()))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skimasque_protocol::SigningKey;

    fn tenant(org: &str, slug: &str) -> Tenant {
        Tenant {
            org_id: org.into(),
            slug: slug.into(),
            owners: vec![],
            owner_ids: Default::default(),
            policy: None,
            signing_key: SigningKey {
                org_id: org.into(),
                algorithm: "ed25519".into(),
                public_key_b64: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
                previous_public_key_b64: None,
            },
        }
    }

    #[test]
    fn dedup_keeps_the_first_org_and_slug_and_drops_an_empty_org() {
        let list = [
            tenant("org_a", "acme"),
            tenant("org_b", "acme"),
            tenant("org_a", "other"),
            tenant("", "blank"),
            tenant("org_b", "beta"),
        ];
        let kept: Vec<(&str, &str)> = dedup(&list)
            .into_iter()
            .map(|t| (t.org_id.as_str(), t.slug.as_str()))
            .collect();
        assert_eq!(kept, [("org_a", "acme"), ("org_b", "beta")]);
    }

    #[test]
    fn the_table_and_the_key_map_hold_the_same_orgs_after_apply() {
        let tenants = PlatformTenants::new();
        tenants.apply(&TenantList {
            version: 3,
            tenants: vec![
                tenant("org_a", "acme"),
                tenant("org_b", "acme"),
                tenant("org_c", "c"),
            ],
        });
        let snapshot = tenants.table.snapshot();
        let mut in_table: Vec<String> = snapshot
            .tenants()
            .map(|t| t.id().as_str().to_owned())
            .collect();
        let mut in_keys: Vec<String> = tenants.keys.borrow().keys().cloned().collect();
        in_table.sort();
        in_keys.sort();
        assert_eq!(in_table, ["org_a", "org_c"]);
        assert_eq!(in_keys, in_table);
    }

    #[test]
    fn a_non_ed25519_key_verifies_nothing() {
        let mut t = tenant("org_a", "acme");
        t.signing_key.algorithm = "hs256".into();
        assert!(verifier_for(&t).verify("a.b.c").is_err());
    }

    #[test]
    fn the_audience_must_name_exactly_one_slug_under_the_base() {
        let token = |aud: serde_json::Value| {
            use base64::prelude::{Engine as _, BASE64_URL_SAFE_NO_PAD};
            let payload =
                BASE64_URL_SAFE_NO_PAD.encode(serde_json::json!({ "aud": aud }).to_string());
            format!("e30.{payload}.sig")
        };
        let base = "https://gw.example";
        let pair = |slug: &str, aud: &str| (slug.to_owned(), aud.to_owned());
        assert_eq!(
            slug_of(base, &token("https://gw.example/o/acme".into())).unwrap(),
            pair("acme", "https://gw.example/o/acme")
        );
        // The same slug twice is not ambiguous; the first audience is kept.
        assert_eq!(
            slug_of(
                base,
                &token(serde_json::json!([
                    "https://gw.example/o/acme/",
                    "https://gw.example/o/acme"
                ]))
            )
            .unwrap(),
            pair("acme", "https://gw.example/o/acme/")
        );
        // Audiences that name no slug are ignored.
        assert_eq!(
            slug_of(
                base,
                &token(serde_json::json!(["other", "https://gw.example/o/acme"]))
            )
            .unwrap(),
            pair("acme", "https://gw.example/o/acme")
        );
        for bad in [
            serde_json::json!("https://gw.example/o/Acme"),
            serde_json::json!("https://evil.example/o/acme"),
            serde_json::json!(["https://gw.example/o/acme", "https://gw.example/o/beta"]),
            serde_json::json!(""),
        ] {
            match slug_of(base, &token(bad.clone())) {
                Err(MintError::Refused { code, .. }) => assert_eq!(code, "unknown_org", "{bad}"),
                other => panic!("{bad}: {:?}", other.map(|_| ())),
            }
        }
    }
}
