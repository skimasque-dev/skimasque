//! The gateway's HTTP client for the SkiMasque control protocol.
//!
//! `skimasque-server --control-plane <url>` registers once, pulls its policy,
//! then long-polls for changes and sends heartbeats. If the control plane is
//! unreachable the gateway keeps enforcing the last policy it cached to disk —
//! a control-plane outage degrades management, never enforcement.
//!
//! The wire types and endpoint paths live in [`skimasque_protocol`] (the open
//! contract); this module is one implementation of the gateway side of it,
//! plus the on-disk fail-static cache.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use skimasque_policy::WorkloadIdentity;
use skimasque_protocol::{paths, HeartbeatRequest, LabelsRequest, MintRequest, RegisterRequest};

// The protocol's wire types, re-exported so existing `crate::control::…` paths
// keep resolving.
pub use skimasque_protocol::{
    audit_hash, AuditHead, ChainedAuditEvent, GatewayIdentity, PolicyDocument, PolicyResponse,
    RegisterResponse, ShipAuditRequest, ShipAuditResponse, SigningKey, UsageReport, AUDIT_GENESIS,
    PROTOCOL_VERSION,
};

/// How stale the enforced policy is, relative to the control plane.
/// Enforcement continues in every state — a control-plane outage never stops
/// the data plane — but management degrades loudly as the policy ages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// The control plane answered within the policy-lease window.
    Fresh,
    /// Past the soft policy lease: the control plane has been unreachable long
    /// enough that management is degraded. The cached policy is still enforced.
    Stale,
    /// Past the hard cache TTL: escalated alarm. The cached policy is *still*
    /// enforced (never fail-open, and an outage never becomes a data-plane
    /// outage), but it may be badly out of date.
    Expired,
}

/// State the policy-sync task and the heartbeat task share: the policy version
/// the gateway is currently enforcing, whether the last control-plane poll
/// succeeded, and how long it has been since the control plane last answered.
/// A heartbeat reports all three.
#[derive(Debug)]
pub struct SyncState {
    /// The enforced policy version, or `0` for "none yet".
    version: AtomicU64,
    /// `false` once a poll has failed, back to `true` on the next success.
    healthy: AtomicBool,
    /// Fixed reference for the monotonic age clock. Placed in the past at
    /// construction so `policy_age` starts from the cache's age at boot.
    origin: Instant,
    /// Seconds since `origin` at the last poll the control plane answered.
    last_contact_secs: AtomicU64,
    /// Soft lease: at or past this age, the policy is [`Freshness::Stale`].
    policy_lease: Duration,
    /// Hard cache TTL: at or past this age, the policy is [`Freshness::Expired`].
    cache_ttl: Duration,
}

impl SyncState {
    /// `cache_age` is how old the cached policy already was when the gateway
    /// started — `Some(Duration::ZERO)` right after a fresh pull, `Some(age)`
    /// from the persisted cache metadata when starting offline, `None` when it
    /// is unknown (treated as zero).
    pub fn new(
        initial_version: Option<u64>,
        cache_age: Option<Duration>,
        policy_lease: Duration,
        cache_ttl: Duration,
    ) -> Self {
        let cache_age = cache_age.unwrap_or(Duration::ZERO);
        Self {
            version: AtomicU64::new(initial_version.unwrap_or(0)),
            healthy: AtomicBool::new(true),
            origin: Instant::now()
                .checked_sub(cache_age)
                .unwrap_or_else(Instant::now),
            last_contact_secs: AtomicU64::new(0),
            policy_lease,
            cache_ttl,
        }
    }

    pub fn set_version(&self, version: u64) {
        self.version.store(version, Ordering::Relaxed);
    }

    pub fn version(&self) -> Option<u64> {
        match self.version.load(Ordering::Relaxed) {
            0 => None,
            v => Some(v),
        }
    }

    /// Record that the control plane answered a poll (whether or not it carried
    /// a new revision): reset the age clock and clear the unhealthy flag.
    pub fn mark_contact(&self) {
        self.last_contact_secs
            .store(self.origin.elapsed().as_secs(), Ordering::Relaxed);
        self.healthy.store(true, Ordering::Relaxed);
    }

    /// Record that a poll failed. The age clock keeps running.
    pub fn mark_unreachable(&self) {
        self.healthy.store(false, Ordering::Relaxed);
    }

    pub fn healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    /// How long since the control plane last answered a poll.
    pub fn policy_age(&self) -> Duration {
        let now = self.origin.elapsed().as_secs();
        let last = self.last_contact_secs.load(Ordering::Relaxed);
        Duration::from_secs(now.saturating_sub(last))
    }

    /// Classify the current policy age against the soft lease and hard TTL.
    pub fn freshness(&self) -> Freshness {
        let age = self.policy_age();
        if age >= self.cache_ttl {
            Freshness::Expired
        } else if age >= self.policy_lease {
            Freshness::Stale
        } else {
            Freshness::Fresh
        }
    }
}

/// Where the gateway keeps its control-plane identity and its cached policy.
///
/// ```text
/// <state>/gateway.json        the registered id and bearer secret
/// <state>/policies/*.toml     the last policy revision pulled, the fail-static cache
/// <state>/policy-cache.json   that revision's version and when it was fetched
/// ```
#[derive(Debug, Clone)]
pub struct ControlPlane {
    base_url: String,
    state_dir: PathBuf,
    http: reqwest::Client,
}

/// Freshness metadata written alongside the on-disk policy cache, so the
/// gateway knows how stale the cache is after a restart during a control-plane
/// outage rather than resetting the staleness clock to zero. Local only — not
/// part of the protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMeta {
    /// The revision version cached.
    pub version: u64,
    /// Wall-clock time the revision was fetched, Unix milliseconds.
    pub fetched_at_ms: u64,
}

impl CacheMeta {
    /// How long ago the cache was fetched, from the current wall clock. `None`
    /// if the clock has gone backwards since (treat as fresh).
    pub fn age(&self) -> Option<Duration> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now_ms
            .checked_sub(self.fetched_at_ms)
            .map(Duration::from_millis)
    }
}

/// The outcome of a policy poll.
#[derive(Debug)]
pub enum PolicyFetch {
    /// The control plane has nothing newer than the version asked about.
    Unchanged,
    /// A newer revision, already written to the on-disk cache.
    Updated {
        version: u64,
        documents: Vec<PolicyDocument>,
    },
    /// The org has never published a policy.
    None,
}

/// A credential the control plane minted with the org's private key — the
/// client-side view of [`skimasque_protocol::MintResponse`].
#[derive(Debug, Clone)]
pub struct MintedCredential {
    pub token: String,
    pub expires_in: Duration,
}

impl ControlPlane {
    /// `base_url` is the control plane's root (`https://control.skimasque.com`).
    pub fn new(base_url: &str, state_dir: impl Into<PathBuf>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .user_agent(concat!("skimasque-server/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("building the HTTP client")?;
        Ok(Self {
            base_url: crate::normalize_base_url(base_url),
            state_dir: state_dir.into(),
            http,
        })
    }

    fn identity_path(&self) -> PathBuf {
        self.state_dir.join("gateway.json")
    }

    /// The directory the cached policy is written to; feed this to
    /// `--policy-dir`'s loader.
    pub fn policy_dir(&self) -> PathBuf {
        self.state_dir.join("policies")
    }

    /// Where the audit-shipping task persists its chain tail.
    pub fn audit_chain_path(&self) -> PathBuf {
        self.state_dir.join("audit-chain.json")
    }

    /// Load the persisted identity, if this gateway has registered before.
    pub fn load_identity(&self) -> Result<Option<GatewayIdentity>> {
        match std::fs::read(self.identity_path()) {
            Ok(bytes) => Ok(Some(
                serde_json::from_slice(&bytes).context("parsing the stored gateway identity")?,
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("reading the stored gateway identity"),
        }
    }

    /// Register with `token`, persist the identity, and return it. `labels` are
    /// the attributes the gateway declares about itself for policy targeting.
    pub async fn register(
        &self,
        token: &str,
        name: &str,
        labels: &BTreeMap<String, String>,
    ) -> Result<GatewayIdentity> {
        let response = self
            .http
            .post(format!("{}{}", self.base_url, paths::REGISTER))
            .json(&RegisterRequest {
                registration_token: token.to_owned(),
                name: name.to_owned(),
                labels: labels.clone(),
            })
            .send()
            .await
            .context("sending the registration request")?;
        let response = error_for_status(response, "registration").await?;
        let body: RegisterResponse = response.json().await.context("parsing the registration response")?;

        let identity = GatewayIdentity {
            gateway_id: body.gateway_id,
            org_id: body.org_id,
            secret: body.secret,
        };
        std::fs::create_dir_all(&self.state_dir).context("creating the control-plane state directory")?;
        let json = serde_json::to_vec_pretty(&identity)?;
        write_private(&self.identity_path(), &json).context("persisting the gateway identity")?;
        Ok(identity)
    }

    /// Poll for a policy newer than `known_version`. With `wait` set the request
    /// is held open that long server-side before a `304`.
    pub async fn fetch_policy(
        &self,
        identity: &GatewayIdentity,
        known_version: Option<u64>,
        wait: Option<Duration>,
    ) -> Result<PolicyFetch> {
        let mut url = format!(
            "{}{}",
            self.base_url,
            paths::gateway_policy(&identity.gateway_id)
        );
        if let Some(wait) = wait {
            url.push_str(&format!("?wait={}", wait.as_secs()));
        }
        let mut request = self.http.get(&url).bearer_auth(&identity.secret);
        if let Some(version) = known_version {
            request = request.header(reqwest::header::IF_NONE_MATCH, format!("\"{version}\""));
        }

        let response = request.send().await.context("polling for policy")?;
        match response.status() {
            reqwest::StatusCode::NOT_MODIFIED => Ok(PolicyFetch::Unchanged),
            reqwest::StatusCode::NO_CONTENT => Ok(PolicyFetch::None),
            _ => {
                let response = error_for_status(response, "policy poll").await?;
                let body: PolicyResponse = response.json().await.context("parsing the policy response")?;
                self.cache_policy(body.version, &body.documents)
                    .context("writing the policy cache")?;
                Ok(PolicyFetch::Updated {
                    version: body.version,
                    documents: body.documents,
                })
            }
        }
    }

    /// Send a heartbeat, optionally carrying the gateway's usage totals.
    pub async fn heartbeat(
        &self,
        identity: &GatewayIdentity,
        healthy: bool,
        policy_version: Option<u64>,
        usage: Option<UsageReport>,
    ) -> Result<()> {
        let response = self
            .http
            .post(format!(
                "{}{}",
                self.base_url,
                paths::gateway_heartbeat(&identity.gateway_id)
            ))
            .bearer_auth(&identity.secret)
            .json(&HeartbeatRequest {
                status: if healthy { "online" } else { "degraded" }.to_owned(),
                policy_version,
                usage,
            })
            .send()
            .await
            .context("sending a heartbeat")?;
        error_for_status(response, "heartbeat").await?;
        Ok(())
    }

    /// The tail of this gateway's audit chain, as the control plane holds it.
    /// A restarting gateway resumes its sequence from here.
    pub async fn audit_head(&self, identity: &GatewayIdentity) -> Result<AuditHead> {
        let response = self
            .http
            .get(format!(
                "{}{}",
                self.base_url,
                paths::gateway_audit_head(&identity.gateway_id)
            ))
            .bearer_auth(&identity.secret)
            .send()
            .await
            .context("fetching the audit head")?;
        error_for_status(response, "audit head")
            .await?
            .json()
            .await
            .context("parsing the audit head")
    }

    /// Ship a hash-chained batch of audit events. Returns the new head sequence.
    pub async fn ship_audit(
        &self,
        identity: &GatewayIdentity,
        events: &[ChainedAuditEvent],
    ) -> Result<u64> {
        let response = self
            .http
            .post(format!(
                "{}{}",
                self.base_url,
                paths::gateway_audit(&identity.gateway_id)
            ))
            .bearer_auth(&identity.secret)
            .json(&ShipAuditRequest {
                events: events.to_vec(),
            })
            .send()
            .await
            .context("shipping audit events")?;
        let response = error_for_status(response, "audit ship").await?;
        Ok(response
            .json::<ShipAuditResponse>()
            .await
            .context("parsing the audit-ship response")?
            .head_seq)
    }

    /// Declare (or update) this gateway's labels for policy targeting. Idempotent
    /// server-side; a no-op change costs nothing.
    pub async fn set_labels(
        &self,
        identity: &GatewayIdentity,
        labels: &BTreeMap<String, String>,
    ) -> Result<()> {
        let response = self
            .http
            .put(format!(
                "{}{}",
                self.base_url,
                paths::gateway_labels(&identity.gateway_id)
            ))
            .bearer_auth(&identity.secret)
            .json(&LabelsRequest {
                labels: labels.clone(),
            })
            .send()
            .await
            .context("sending the gateway labels")?;
        error_for_status(response, "setting gateway labels").await?;
        Ok(())
    }

    fn signing_key_path(&self) -> PathBuf {
        self.state_dir.join("signing-key.json")
    }

    /// Fetch the org's Ed25519 public key and cache it to disk. Unauthenticated
    /// -- it is a public key. The gateway verifies control-plane-minted
    /// credentials against this with no further network calls.
    pub async fn fetch_signing_key(&self, org_id: &str) -> Result<SigningKey> {
        let response = self
            .http
            .get(format!(
                "{}{}",
                self.base_url,
                paths::org_signing_key(org_id)
            ))
            .send()
            .await
            .context("fetching the org signing key")?;
        let response = error_for_status(response, "signing-key fetch").await?;
        let key: SigningKey = response
            .json()
            .await
            .context("parsing the signing-key response")?;
        if !key.algorithm.eq_ignore_ascii_case("ed25519") {
            anyhow::bail!("the control plane served a {} signing key, expected Ed25519", key.algorithm);
        }
        // Validate before caching, so a corrupt key is caught here.
        key.public_key_bytes().context("the org signing key")?;

        std::fs::create_dir_all(&self.state_dir)
            .context("creating the control-plane state directory")?;
        if let Ok(json) = serde_json::to_vec_pretty(&key) {
            let _ = std::fs::write(self.signing_key_path(), json);
        }
        Ok(key)
    }

    /// The last org signing key fetched, if one was cached.
    pub fn load_cached_signing_key(&self) -> Option<SigningKey> {
        let bytes = std::fs::read(self.signing_key_path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Ask the control plane to mint a platform credential for a workload
    /// identity the gateway has already verified from its OIDC token (D1). The
    /// control plane signs it with the org's private key; `ttl` is a request
    /// the control plane may cap.
    pub async fn mint_credential(
        &self,
        identity: &GatewayIdentity,
        workload: &WorkloadIdentity,
        subject: Option<&str>,
        ttl: Duration,
    ) -> Result<MintedCredential> {
        let response = self
            .http
            .post(format!(
                "{}{}",
                self.base_url,
                paths::gateway_credentials(&identity.gateway_id)
            ))
            .bearer_auth(&identity.secret)
            .json(&MintRequest {
                identity: workload.clone(),
                subject: subject.map(str::to_owned),
                ttl_seconds: ttl.as_secs(),
            })
            .send()
            .await
            .context("requesting a minted credential")?;
        let response = error_for_status(response, "credential mint").await?;
        let body: skimasque_protocol::MintResponse = response
            .json()
            .await
            .context("parsing the mint response")?;
        Ok(MintedCredential {
            token: body.credential,
            expires_in: Duration::from_secs(body.expires_in),
        })
    }

    fn cache_meta_path(&self) -> PathBuf {
        self.state_dir.join("policy-cache.json")
    }

    /// Replace the on-disk policy cache with revision `version`'s `documents`
    /// and record when it was fetched.
    pub fn cache_policy(&self, version: u64, documents: &[PolicyDocument]) -> Result<()> {
        let dir = self.policy_dir();
        // Write into a fresh temp dir and swap, so a crash mid-write cannot
        // leave a half-written policy set the next start would load.
        let staging = self.state_dir.join(".policies.new");
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).context("creating the policy staging directory")?;
        for doc in documents {
            let name = sanitize(&doc.name);
            std::fs::write(staging.join(&name), &doc.text)
                .with_context(|| format!("writing {name}"))?;
        }
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&staging, &dir).context("swapping in the new policy cache")?;

        // Advisory freshness metadata: written after the swap, best effort. A
        // missing or stale file only costs us precision in the age clock.
        let meta = CacheMeta {
            version,
            fetched_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        if let Ok(json) = serde_json::to_vec_pretty(&meta) {
            let _ = std::fs::write(self.cache_meta_path(), json);
        }
        Ok(())
    }

    /// Whether a cached policy set exists from a previous run.
    pub fn has_cached_policy(&self) -> bool {
        std::fs::read_dir(self.policy_dir())
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false)
    }

    /// The freshness metadata for the cached policy, if it was recorded.
    pub fn load_cache_meta(&self) -> Option<CacheMeta> {
        let bytes = std::fs::read(self.cache_meta_path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// How stale the on-disk policy cache is, best effort: from the recorded
    /// fetch time, falling back to the cache directory's modification time,
    /// and `None` if neither is available.
    pub fn cached_policy_age(&self) -> Option<Duration> {
        if let Some(age) = self.load_cache_meta().and_then(|m| m.age()) {
            return Some(age);
        }
        let modified = std::fs::metadata(self.policy_dir()).ok()?.modified().ok()?;
        modified.elapsed().ok()
    }
}

/// Keep a document name to a single path component, so a control plane cannot
/// steer a write outside the policy directory.
fn sanitize(name: &str) -> String {
    let stem: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    let stem = stem.trim_matches('.');
    if stem.is_empty() {
        "policy.toml".to_owned()
    } else if Path::new(stem)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e, "toml" | "yaml" | "yml"))
    {
        stem.to_owned()
    } else {
        format!("{stem}.toml")
    }
}

async fn error_for_status(response: reqwest::Response, what: &str) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    anyhow::bail!("{what} failed: {status} {}", body.trim());
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    std::io::Write::write_all(&mut file, bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_names_are_confined_to_one_path_component() {
        assert_eq!(sanitize("prod.toml"), "prod.toml");
        assert_eq!(sanitize("prod.yaml"), "prod.yaml");
        assert_eq!(sanitize("../../etc/passwd"), "etcpasswd.toml");
        assert_eq!(sanitize("a/b/c"), "abc.toml");
        assert_eq!(sanitize(""), "policy.toml");
        assert_eq!(sanitize("..."), "policy.toml");
    }

    #[test]
    fn the_policy_cache_round_trips_and_swaps_atomically() {
        let dir = std::env::temp_dir().join(format!("skmcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cp = ControlPlane::new("https://example.invalid", &dir).unwrap();

        assert!(!cp.has_cached_policy());
        cp.cache_policy(
            1,
            &[PolicyDocument {
                name: "prod.toml".into(),
                text: "name = \"p\"\n".into(),
            }],
        )
        .unwrap();
        assert!(cp.has_cached_policy());
        assert_eq!(
            std::fs::read_to_string(cp.policy_dir().join("prod.toml")).unwrap(),
            "name = \"p\"\n"
        );
        // The freshness metadata is written alongside, and reads back young.
        let meta = cp.load_cache_meta().expect("cache meta written");
        assert_eq!(meta.version, 1);
        assert!(cp.cached_policy_age().unwrap() < Duration::from_secs(60));

        // A second publish replaces the set rather than merging.
        cp.cache_policy(
            2,
            &[PolicyDocument {
                name: "staging.toml".into(),
                text: "name = \"s\"\n".into(),
            }],
        )
        .unwrap();
        assert!(!cp.policy_dir().join("prod.toml").exists());
        assert!(cp.policy_dir().join("staging.toml").exists());
        assert_eq!(cp.load_cache_meta().unwrap().version, 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn freshness_tracks_the_age_against_the_lease_and_ttl() {
        let lease = Duration::from_secs(900);
        let ttl = Duration::from_secs(1800);

        // A gateway that has just pulled: fresh, no matter the version.
        let fresh = SyncState::new(Some(3), Some(Duration::ZERO), lease, ttl);
        assert_eq!(fresh.freshness(), Freshness::Fresh);
        assert!(fresh.healthy());

        // Booted from a cache that was already 20 minutes old: past the lease,
        // not yet past the TTL.
        let stale = SyncState::new(Some(3), Some(Duration::from_secs(1200)), lease, ttl);
        assert_eq!(stale.freshness(), Freshness::Stale);

        // ...and 40 minutes old: past the hard TTL.
        let expired = SyncState::new(Some(3), Some(Duration::from_secs(2400)), lease, ttl);
        assert_eq!(expired.freshness(), Freshness::Expired);

        // A successful poll resets the clock regardless of how stale it was.
        expired.mark_contact();
        assert_eq!(expired.freshness(), Freshness::Fresh);
        assert!(expired.healthy());

        // A failed poll degrades health but leaves the age clock running.
        stale.mark_unreachable();
        assert!(!stale.healthy());
        assert_eq!(stale.freshness(), Freshness::Stale);
    }

    #[test]
    fn identity_persistence_round_trips() {
        let dir = std::env::temp_dir().join(format!("skmcp-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cp = ControlPlane::new("https://example.invalid", &dir).unwrap();

        assert!(cp.load_identity().unwrap().is_none());
        let identity = GatewayIdentity {
            gateway_id: "gw_1".into(),
            org_id: "org_1".into(),
            secret: "s3cret".into(),
        };
        std::fs::write(
            cp.identity_path(),
            serde_json::to_vec(&identity).unwrap(),
        )
        .unwrap();
        assert_eq!(cp.load_identity().unwrap().unwrap().gateway_id, "gw_1");

        std::fs::remove_dir_all(&dir).ok();
    }
}
