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
use skimasque::audit::AuditSink;
use skimasque::service::{
    Accepted, Dispatch, PolicyLayer, QuotaLayer, Rejection, TenantLayer, TenantMeterLayer,
    TunnelRequest,
};
use skimasque::{
    CredentialMinter, MintError, MintedCredential, TenantId, TenantSpec, TenantTable, TenantUsage,
    TenantVerifier,
};
use skimasque_identity::{peek_audiences, peek_org_id, CredentialVerifier, OidcVerifier};
use skimasque_policy::{PolicySet, WorkloadIdentity};
use skimasque_protocol::platform::{
    normalize_owner, refusal, slug_from_audience, tenant_audience, PlatformAuditEvent,
    PlatformHeartbeatRequest, PlatformMintRequest, PlatformShipAuditRequest, Tenant, TenantList,
};
use skimasque_protocol::UsageReport;
use tokio::sync::{mpsc, watch};
use tower::limit::GlobalConcurrencyLimitLayer;
use tower::util::BoxCloneService;
use tower::ServiceBuilder;

use crate::audit_ship::{chain_with, persist, resume_from, BATCH_MAX, MAX_PENDING};
use crate::control::{
    ControlPlane, Freshness, GatewayIdentity, PlatformMintError, SyncState, TenantFetch,
};

/// The file, under the state directory, that caches the last tenant list.
const CACHE_FILE: &str = "tenants.json";

/// What a token exchange answers when the control plane cannot mint.
const UNAVAILABLE: &str = "SkiMasque control plane unreachable; try again shortly.";

/// What a token exchange answers when the OIDC issuer's keys cannot be
/// fetched. Fixed, so no fetch detail reaches the client.
const OIDC_KEYS_UNAVAILABLE: &str = "GitHub OIDC keys unavailable; try again shortly.";

/// The least time between two tenant polls, however fast the control plane
/// answers.
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(1);

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
    /// Held across the key-map and table swaps in [`apply`](Self::apply), so
    /// two concurrent applies cannot leave one list's keys beside another's
    /// table.
    apply_lock: Arc<std::sync::Mutex<()>>,
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
            apply_lock: Arc::default(),
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
        // The lock makes the pair one step for any other apply. A poisoned
        // lock guards no data, so it is taken regardless.
        let _serialised = self
            .apply_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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

/// Where a starting platform gateway's first tenant list came from.
#[derive(Debug)]
pub enum InitialTenants {
    /// Pulled from the control plane just now (and cached).
    Fetched(TenantList),
    /// The control plane could not be reached (`error`); this is the list
    /// cached in `<state>/tenants.json`, last written `age` ago if known.
    Cached {
        list: TenantList,
        age: Option<Duration>,
        error: anyhow::Error,
    },
}

/// The tenant list a platform gateway starts with: pulled from the control
/// plane and cached, or, when the control plane cannot be reached, the cached
/// one. With neither this is an error: a platform gateway with no tenant table
/// must not serve.
pub async fn initial_tenants(
    control: &ControlPlane,
    identity: &GatewayIdentity,
    state_dir: &Path,
) -> Result<InitialTenants> {
    let error = match control.fetch_tenants(identity, None, None).await {
        Ok(TenantFetch::Updated(list)) => {
            if let Err(error) = PlatformTenants::write_cache(state_dir, &list) {
                tracing::warn!(%error, "could not cache the tenant list");
            }
            return Ok(InitialTenants::Fetched(list));
        }
        // Asked with no known version, "unchanged" means nothing usable.
        Ok(TenantFetch::Unchanged) => {
            anyhow::anyhow!("the control plane answered the first tenant poll with no list")
        }
        Err(error) => error,
    };
    match PlatformTenants::load_cache(state_dir) {
        Some(list) => Ok(InitialTenants::Cached {
            list,
            age: cache_age(state_dir),
            error,
        }),
        None => Err(error.context(format!(
            "the control plane is unreachable and there is no cached tenant list ({}); a \
             platform gateway will not start without a tenant table",
            state_dir.join(CACHE_FILE).display()
        ))),
    }
}

/// How long ago the tenant cache was written, from its modification time.
fn cache_age(state_dir: &Path) -> Option<Duration> {
    let modified = std::fs::metadata(state_dir.join(CACHE_FILE))
        .and_then(|m| m.modified())
        .ok()?;
    std::time::SystemTime::now().duration_since(modified).ok()
}

/// Where a customer verifies a GitHub owner on the control plane at
/// `control_plane_url`: its identity settings page. Quoted in an
/// `owner_not_verified` refusal.
pub fn owner_settings_url(control_plane_url: &str) -> String {
    format!(
        "{}/app/settings/identity",
        crate::normalize_base_url(control_plane_url)
    )
}

/// The tunnel service a platform gateway serves, outer to inner: the global
/// concurrency cap, the tenant credential check ([`PlatformTenantVerifier`],
/// which puts the `TenantId` and the workload identity in the request), that
/// tenant's policy with every decision audited under its org, the
/// tenant-scoped quotas, the tenant usage meter, then `dispatch` (whose
/// address floor still runs after DNS).
///
/// There is no static-token layer: `--oidc` (which `--platform` requires)
/// excludes `--auth-token`.
pub fn platform_service(
    dispatch: Dispatch,
    max_concurrent_requests: usize,
    tenants: &PlatformTenants,
    audit: Arc<dyn AuditSink>,
    usage: Arc<TenantUsage>,
) -> BoxCloneService<TunnelRequest, Accepted, Rejection> {
    BoxCloneService::new(
        ServiceBuilder::new()
            .layer(GlobalConcurrencyLimitLayer::new(max_concurrent_requests))
            .layer(TenantLayer::new(Arc::new(PlatformTenantVerifier::new(
                tenants.clone(),
            ))))
            .layer(PolicyLayer::tenants(tenants.table.clone()).with_audit(audit))
            .layer(QuotaLayer::new())
            .layer(TenantMeterLayer::new(usage))
            .service(dispatch),
    )
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
///
/// `state` is what [`run_platform_heartbeat`] reports from: every answered poll
/// marks contact, every failed one marks the control plane unreachable, and an
/// applied list records its version.
pub async fn run_tenant_sync(
    control: ControlPlane,
    identity: GatewayIdentity,
    tenants: PlatformTenants,
    state: Arc<SyncState>,
    state_dir: PathBuf,
    interval: Duration,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        // A control plane that answers at once (ignoring `?wait=`) must not
        // turn this into a hot loop.
        let earliest_next = tokio::time::Instant::now() + MIN_POLL_INTERVAL;
        let known = match tenants.table.snapshot().version() {
            0 => None,
            v => Some(v),
        };
        match control
            .fetch_tenants(&identity, known, Some(interval))
            .await
        {
            Ok(TenantFetch::Updated(list)) => {
                state.mark_contact();
                tenants.apply(&list);
                state.set_version(list.version);
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
                state.mark_contact();
                backoff = Duration::from_secs(1);
            }
            Err(error) => {
                state.mark_unreachable();
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
        tokio::time::sleep_until(earliest_next).await;
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
                    "The token names more than one SkiMasque organisation. Check the \
                     `audience` in your workflow."
                        .to_owned(),
                ))
            }
        }
    }
    found.ok_or_else(|| {
        refused(
            refusal::UNKNOWN_ORG,
            format!(
                "Unknown SkiMasque organisation. Check the `audience` in your workflow. It must \
                 be `{}`.",
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
                    | skimasque_identity::Error::Jwks(_) => {
                        // The detail (URLs, transport errors) is for the
                        // operator's log, not the client.
                        metrics::counter!("skimasque_platform_mint_total", "outcome" => "oidc_keys_unavailable")
                            .increment(1);
                        tracing::warn!(error = %e, "could not fetch the OIDC signing keys");
                        MintError::Unavailable(OIDC_KEYS_UNAVAILABLE.to_owned())
                    }
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

/// Sequence and hash `events` into the gateway's one chain, each carrying the
/// organisation it names. An event with no `org_id` is skipped: it consumes no
/// sequence number and is never filed under an org it does not name.
fn chain_platform_batch(
    events: &[skimasque::audit::AuditEvent],
    first_seq: u64,
    prev_hash: &str,
) -> (Vec<PlatformAuditEvent>, String) {
    chain_with(
        events,
        first_seq,
        prev_hash,
        |event, seq, prev_hash, event_json| {
            Some(PlatformAuditEvent {
                org_id: event.org_id.clone()?,
                seq,
                prev_hash,
                event_json,
            })
        },
    )
}

/// The next batch to ship from the front of `pending`.
struct PlatformBatch {
    /// The chained, org-tagged events to send.
    events: Vec<PlatformAuditEvent>,
    /// The chain tail after `events`.
    tail_hash: String,
    /// The sequence number after `events`, once they are accepted. Advanced by
    /// the events actually chained, never by how many were taken, so a skipped
    /// event cannot open a gap.
    next_seq: u64,
    /// How many of `pending` this batch consumes.
    taken: usize,
}

fn platform_batch(
    pending: &[skimasque::audit::AuditEvent],
    next_seq: u64,
    prev_hash: &str,
) -> PlatformBatch {
    let taken = pending.len().min(BATCH_MAX);
    let (events, tail_hash) = chain_platform_batch(&pending[..taken], next_seq, prev_hash);
    PlatformBatch {
        next_seq: next_seq + events.len() as u64,
        events,
        tail_hash,
        taken,
    }
}

/// Queue `event` for shipping unless it names no organisation, in which case it
/// is dropped, counted and logged. Returns whether it was queued.
fn admit_platform_event(
    pending: &mut Vec<skimasque::audit::AuditEvent>,
    event: skimasque::audit::AuditEvent,
) -> bool {
    if event.org_id.is_none() {
        metrics::counter!("skimasque_control_plane_audit_total", "outcome" => "dropped_no_org")
            .increment(1);
        tracing::warn!(
            decision = event.decision,
            "audit event names no organisation; not shipping it to the control plane"
        );
        return false;
    }
    pending.push(event);
    true
}

/// Drain the receiver, hash-chain batches (one chain for the whole gateway) and
/// ship them, each event tagged with its own organisation. Runs until the sender
/// is dropped. Same 409-rebase, backoff, backlog cap and persistence as
/// [`crate::audit_ship::run_audit_shipping`].
pub async fn run_platform_audit_shipping(
    control: ControlPlane,
    identity: GatewayIdentity,
    mut rx: mpsc::Receiver<skimasque::audit::AuditEvent>,
    chain_path: PathBuf,
) {
    let head = resume_from(control.platform_audit_head(&identity).await, &chain_path);
    let mut next_seq = head.seq + 1;
    let mut prev_hash = head.hash;
    let mut pending: Vec<skimasque::audit::AuditEvent> = Vec::new();
    let mut backoff = Duration::from_secs(1);

    loop {
        if pending.is_empty() {
            match rx.recv().await {
                Some(event) => {
                    admit_platform_event(&mut pending, event);
                }
                None => return, // the gateway is shutting down
            }
            if pending.is_empty() {
                continue;
            }
        }
        while pending.len() < BATCH_MAX {
            match rx.try_recv() {
                Ok(event) => {
                    admit_platform_event(&mut pending, event);
                }
                Err(_) => break,
            }
        }

        let PlatformBatch {
            events,
            tail_hash,
            next_seq: seq_after,
            taken,
        } = platform_batch(&pending, next_seq, &prev_hash);
        if events.is_empty() {
            // Nothing in this slice names an org (admit keeps this from
            // happening); consume it without a request.
            pending.drain(..taken);
            continue;
        }
        let shipped = events.len() as u64;

        match control
            .ship_platform_audit(&identity, &PlatformShipAuditRequest { events })
            .await
        {
            Ok(_) => {
                next_seq = seq_after;
                prev_hash = tail_hash;
                persist(&chain_path, next_seq - 1, &prev_hash);
                pending.drain(..taken);
                metrics::counter!("skimasque_control_plane_audit_total", "outcome" => "shipped")
                    .increment(shipped);
                backoff = Duration::from_secs(1);
            }
            Err(error) => {
                metrics::counter!("skimasque_control_plane_audit_total", "outcome" => "error")
                    .increment(1);
                let conflict = error.to_string().contains("409");
                tracing::warn!(
                    %error,
                    "shipping platform audit events failed; the local sink still has them"
                );
                if conflict {
                    // Our sequence disagrees with the control plane's chain --
                    // re-base on its head and rebuild the batch next loop.
                    if let Ok(head) = control.platform_audit_head(&identity).await {
                        next_seq = head.seq + 1;
                        prev_hash = head.hash;
                        continue;
                    }
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
                while pending.len() < MAX_PENDING {
                    match rx.try_recv() {
                        Ok(event) => {
                            admit_platform_event(&mut pending, event);
                        }
                        Err(_) => break,
                    }
                }
                if pending.len() > MAX_PENDING {
                    let overflow = pending.len() - MAX_PENDING;
                    pending.drain(..overflow);
                    metrics::counter!("skimasque_control_plane_audit_total", "outcome" => "dropped")
                        .increment(overflow as u64);
                    tracing::warn!(
                        overflow,
                        "audit ship backlog full; dropped oldest events from the control-plane \
                         copy (the local sink still has them)"
                    );
                }
            }
        }
    }
}

/// The heartbeat body: `"online"` when `healthy`, else `"degraded"`, the tenant
/// list version being enforced, and every org's cumulative usage.
fn platform_heartbeat_request(
    healthy: bool,
    tenants_version: Option<u64>,
    usage: &skimasque::TenantUsage,
) -> PlatformHeartbeatRequest {
    PlatformHeartbeatRequest {
        status: if healthy { "online" } else { "degraded" }.to_owned(),
        tenants_version,
        usage_by_org: usage
            .snapshot()
            .into_iter()
            .map(|(org, s)| {
                (
                    org,
                    UsageReport {
                        tunnels_opened: s.tunnels_opened,
                        bytes_to_target: s.bytes_to_target,
                        bytes_to_client: s.bytes_to_client,
                    },
                )
            })
            .collect(),
    }
}

/// Send a platform heartbeat every `interval`: health from `state` (the tenant
/// sync's freshness), the tenant list version from `tenants_version`, and each
/// org's cumulative usage. Never returns; a failed heartbeat is counted and
/// retried on the next tick.
pub async fn run_platform_heartbeat(
    control: ControlPlane,
    identity: GatewayIdentity,
    state: Arc<SyncState>,
    tenants_version: impl Fn() -> Option<u64>,
    usage: Arc<skimasque::TenantUsage>,
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_freshness = Freshness::Fresh;
    loop {
        ticker.tick().await;

        let age = state.policy_age();
        let freshness = state.freshness();
        metrics::gauge!("skimasque_control_plane_policy_age_seconds").set(age.as_secs_f64());
        metrics::gauge!("skimasque_control_plane_policy_expired").set(
            if freshness == Freshness::Expired {
                1.0
            } else {
                0.0
            },
        );
        if freshness != last_freshness {
            let age_secs = age.as_secs();
            match freshness {
                Freshness::Fresh => tracing::info!(
                    age_secs,
                    "control plane reachable again; the cached tenants are fresh"
                ),
                Freshness::Stale => tracing::warn!(
                    age_secs,
                    "tenant lease expired and the control plane is unreachable; still \
                     enforcing the cached tenants, management is degraded"
                ),
                Freshness::Expired => tracing::error!(
                    age_secs,
                    "tenant cache TTL exceeded and the control plane is still unreachable; \
                     still enforcing the last tenants but they may be badly out of date"
                ),
            }
            last_freshness = freshness;
        }

        let healthy = state.healthy() && freshness == Freshness::Fresh;
        let request = platform_heartbeat_request(healthy, tenants_version(), &usage);
        let outcome = match control.platform_heartbeat(&identity, &request).await {
            Ok(()) => "ok",
            Err(error) => {
                tracing::debug!(%error, "platform heartbeat failed");
                "error"
            }
        };
        metrics::counter!("skimasque_control_plane_heartbeat_total", "outcome" => outcome)
            .increment(1);
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

    fn audit_event(org: Option<&str>, app: &str) -> skimasque::audit::AuditEvent {
        skimasque::audit::AuditEvent {
            timestamp: "2026-03-01T00:00:00.000Z".into(),
            decision: "allow",
            protocol: "connect-tcp",
            application: app.into(),
            destination: "db:5432".into(),
            client: "10.0.0.1:5000".into(),
            identity: Default::default(),
            policy: None,
            rule: None,
            reason: None,
            suggested_rule: None,
            requested_policy: None,
            org_id: org.map(str::to_owned),
        }
    }

    #[test]
    fn platform_batches_carry_each_events_own_org_and_keep_one_gateway_chain() {
        use skimasque_protocol::audit_hash;
        let genesis = skimasque_protocol::AUDIT_GENESIS;
        let events = [
            audit_event(Some("org_a"), "one"),
            audit_event(Some("org_b"), "two"),
            audit_event(Some("org_a"), "three"),
        ];
        let (batch, tail) = chain_platform_batch(&events, 5, genesis);
        let orgs: Vec<&str> = batch.iter().map(|e| e.org_id.as_str()).collect();
        assert_eq!(orgs, ["org_a", "org_b", "org_a"]);
        assert_eq!(
            batch.iter().map(|e| e.seq).collect::<Vec<_>>(),
            [5, 6, 7],
            "one sequence across orgs"
        );
        assert_eq!(batch[0].prev_hash, genesis);
        let mut hash = genesis.to_owned();
        for (event, source) in batch.iter().zip(&events) {
            assert_eq!(event.prev_hash, hash, "one chain across orgs");
            assert_eq!(event.event_json, serde_json::to_string(source).unwrap());
            hash = audit_hash(event.seq, &event.prev_hash, &event.event_json);
        }
        assert_eq!(tail, hash);
    }

    #[test]
    fn an_event_with_no_org_is_never_shipped() {
        let genesis = skimasque_protocol::AUDIT_GENESIS;
        let events = [
            audit_event(Some("org_a"), "one"),
            audit_event(None, "orphan"),
            audit_event(Some("org_b"), "two"),
        ];
        let (batch, _) = chain_platform_batch(&events, 1, genesis);
        assert_eq!(batch.len(), 2);
        assert!(batch.iter().all(|e| !e.event_json.contains("orphan")));
        assert_eq!(
            batch.iter().map(|e| e.seq).collect::<Vec<_>>(),
            [1, 2],
            "a dropped event leaves no gap in the chain"
        );

        let mut pending = Vec::new();
        assert!(!admit_platform_event(&mut pending, audit_event(None, "x")));
        assert!(pending.is_empty());
        assert!(admit_platform_event(
            &mut pending,
            audit_event(Some("org_a"), "x")
        ));
        assert_eq!(pending.len(), 1);
    }

    /// The sequence advances by the events actually shipped, so an event that
    /// is skipped (it names no org) can never open a gap in the chain.
    #[test]
    fn a_shipped_batch_advances_the_sequence_by_the_events_it_carries() {
        let genesis = skimasque_protocol::AUDIT_GENESIS;
        let pending = [
            audit_event(Some("org_a"), "one"),
            audit_event(None, "orphan"),
            audit_event(Some("org_b"), "two"),
        ];
        let batch = platform_batch(&pending, 10, genesis);
        assert_eq!(batch.taken, 3, "every pending event is consumed");
        assert_eq!(batch.events.len(), 2);
        assert_eq!(
            batch.next_seq, 12,
            "only shipped events consume a sequence number"
        );
        assert_eq!(batch.events.last().unwrap().seq, 11);
    }

    /// Concurrent applies must not leave the key map from one list beside the
    /// table from another.
    #[test]
    fn concurrent_applies_never_mix_two_lists() {
        let tenants = PlatformTenants::new();
        let lists: Vec<TenantList> = (1..=8u64)
            .map(|v| TenantList {
                version: v,
                tenants: (0..16)
                    .map(|i| tenant(&format!("org_{v}_{i}"), &format!("s{v}-{i}")))
                    .collect(),
            })
            .collect();
        for _ in 0..300 {
            let start = std::sync::Barrier::new(lists.len());
            std::thread::scope(|scope| {
                for list in &lists {
                    let (tenants, start) = (tenants.clone(), &start);
                    scope.spawn(move || {
                        start.wait();
                        tenants.apply(list)
                    });
                }
            });
            let snapshot = tenants.table.snapshot();
            let mut in_table: Vec<String> = snapshot
                .tenants()
                .map(|t| t.id().as_str().to_owned())
                .collect();
            let mut in_keys: Vec<String> = tenants.keys.borrow().keys().cloned().collect();
            in_table.sort();
            in_keys.sort();
            assert_eq!(in_keys, in_table, "version {}", snapshot.version());
        }
    }

    #[test]
    fn the_heartbeat_reports_cumulative_usage_per_org() {
        let tenants = PlatformTenants::new();
        tenants.apply(&TenantList {
            version: 9,
            tenants: vec![tenant("org_a", "a"), tenant("org_b", "b")],
        });
        let snapshot = tenants.table.snapshot();
        let a = snapshot.resolve_org("org_a").unwrap().id().clone();
        let b = snapshot.resolve_org("org_b").unwrap().id().clone();
        let usage = skimasque::TenantUsage::new();
        usage.meter_for(&a).tunnel_opened();
        usage.meter_for(&a).add_to_target(10);
        usage.meter_for(&b).add_to_client(4);

        let version = tenants.table.snapshot().version();
        let req = platform_heartbeat_request(true, Some(version), &usage);
        assert_eq!(req.status, "online");
        assert_eq!(req.tenants_version, Some(9));
        assert_eq!(req.usage_by_org.len(), 2);
        let ua = req.usage_by_org["org_a"];
        assert_eq!(
            (ua.tunnels_opened, ua.bytes_to_target, ua.bytes_to_client),
            (1, 10, 0)
        );
        let ub = req.usage_by_org["org_b"];
        assert_eq!(
            (ub.tunnels_opened, ub.bytes_to_target, ub.bytes_to_client),
            (0, 0, 4)
        );

        // Cumulative: a later report includes the earlier traffic.
        usage.meter_for(&a).add_to_target(5);
        let again = platform_heartbeat_request(false, None, &usage);
        assert_eq!(again.status, "degraded");
        assert_eq!(again.tenants_version, None);
        assert_eq!(again.usage_by_org["org_a"].bytes_to_target, 15);
    }
}
