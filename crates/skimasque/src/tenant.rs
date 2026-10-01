//! The tenant model for a multi-tenant gateway.
//!
//! A [`TenantTable`] holds the organisations a shared gateway serves. The only
//! way to obtain a [`TenantId`] is through a table, so holding one proves that
//! the organisation is a tenant: there is deliberately no public constructor.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use skimasque_policy::PolicySet;
use tokio::sync::watch;
use tracing::warn;

use crate::metrics::UsageSnapshot;
use crate::service::TunnelMeter;

/// An organisation the gateway serves. Minted only by [`TenantTable`]: there is
/// no public constructor, so a `TenantId` in hand means "this org is a tenant".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TenantId(Arc<str>);

impl TenantId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the control plane says about one tenant.
#[derive(Debug, Clone)]
pub struct TenantSpec {
    pub org_id: String,
    pub slug: String,
    /// Already normalised (lowercase).
    pub owners: Vec<String>,
    pub owner_ids: BTreeMap<String, u64>,
    pub policy: Arc<PolicySet>,
}

/// One tenant in a [`TenantSnapshot`].
#[derive(Debug)]
pub struct Tenant {
    id: TenantId,
    slug: String,
    owners: Vec<String>,
    owner_ids: BTreeMap<String, u64>,
    policy: Arc<PolicySet>,
}

impl Tenant {
    /// Owner logins and the keys of `owner_ids` are lowercased here, whatever
    /// the control plane sent: [`owns`](Self::owns) looks both up by the
    /// lowercased login, and a mixed-case `owner_ids` key it could not find
    /// would silently weaken the check to login-only.
    pub(crate) fn new(spec: TenantSpec) -> Self {
        Self {
            id: TenantId(Arc::from(spec.org_id)),
            slug: spec.slug,
            owners: spec.owners.iter().map(|o| o.to_lowercase()).collect(),
            owner_ids: spec
                .owner_ids
                .into_iter()
                .map(|(login, id)| (login.to_lowercase(), id))
                .collect(),
            policy: spec.policy,
        }
    }

    pub fn id(&self) -> &TenantId {
        &self.id
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn policy(&self) -> &Arc<PolicySet> {
        &self.policy
    }

    /// `owner` is matched case-insensitively; when an id is recorded for that
    /// login, `owner_id` must equal it.
    pub fn owns(&self, owner: &str, owner_id: Option<u64>) -> bool {
        let login = owner.to_lowercase();
        if !self.owners.contains(&login) {
            return false;
        }
        match self.owner_ids.get(&login) {
            Some(recorded) => owner_id == Some(*recorded),
            None => true,
        }
    }
}

/// An immutable view of the tenants at one control-plane version.
#[derive(Debug, Default)]
pub struct TenantSnapshot {
    version: u64,
    by_org: HashMap<String, Arc<Tenant>>,
    by_slug: HashMap<String, Arc<Tenant>>,
}

impl TenantSnapshot {
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn resolve_org(&self, org_id: &str) -> Option<&Arc<Tenant>> {
        self.by_org.get(org_id)
    }

    /// Slugs are case-sensitive.
    pub fn resolve_slug(&self, slug: &str) -> Option<&Arc<Tenant>> {
        self.by_slug.get(slug)
    }

    pub fn tenants(&self) -> impl Iterator<Item = &Arc<Tenant>> {
        self.by_org.values()
    }
}

/// The live tenant table: cheap to clone, swapped atomically on each refresh.
#[derive(Clone, Debug)]
pub struct TenantTable {
    tx: watch::Sender<Arc<TenantSnapshot>>,
}

impl Default for TenantTable {
    fn default() -> Self {
        Self::new()
    }
}

impl TenantTable {
    /// An empty table, version 0.
    pub fn new() -> Self {
        let (tx, _) = watch::channel(Arc::new(TenantSnapshot::default()));
        Self { tx }
    }

    /// Replace the table. A later entry that repeats an earlier `org_id` or
    /// `slug` is dropped and logged: a slug must never resolve to two orgs.
    pub fn store(&self, version: u64, tenants: Vec<TenantSpec>) {
        let mut by_org: HashMap<String, Arc<Tenant>> = HashMap::new();
        let mut by_slug: HashMap<String, Arc<Tenant>> = HashMap::new();
        for spec in tenants {
            if by_org.contains_key(&spec.org_id) {
                warn!(org_id = %spec.org_id, "duplicate org_id in tenant list; dropping the later entry");
                continue;
            }
            if by_slug.contains_key(&spec.slug) {
                warn!(org_id = %spec.org_id, slug = %spec.slug, "duplicate slug in tenant list; dropping the later entry");
                continue;
            }
            let tenant = Arc::new(Tenant::new(spec));
            by_org.insert(tenant.id.as_str().to_owned(), tenant.clone());
            by_slug.insert(tenant.slug.clone(), tenant);
        }
        self.tx.send_replace(Arc::new(TenantSnapshot {
            version,
            by_org,
            by_slug,
        }));
    }

    pub fn snapshot(&self) -> Arc<TenantSnapshot> {
        self.tx.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Arc<TenantSnapshot>> {
        self.tx.subscribe()
    }
}

/// Per-tenant usage: one [`TunnelMeter`] per organisation, created on first
/// use and kept for the life of the process.
///
/// Like the process-wide totals in [`crate::metrics`], the counts are
/// cumulative since process start; the control plane turns successive
/// snapshots into its own accumulator. A tenant removed from the table keeps
/// its meter, so traffic it relayed before removal is still reported.
#[derive(Debug, Default)]
pub struct TenantUsage {
    meters: Mutex<HashMap<String, Arc<TunnelMeter>>>,
}

impl TenantUsage {
    pub fn new() -> Self {
        Self::default()
    }

    /// The meter for `id`'s organisation. Every call for the same org returns
    /// the same meter.
    pub fn meter_for(&self, id: &TenantId) -> Arc<TunnelMeter> {
        self.meters
            .lock()
            .expect("tenant usage table is not poisoned")
            .entry(id.as_str().to_owned())
            .or_default()
            .clone()
    }

    /// Every metered org's totals, ordered by `org_id`.
    pub fn snapshot(&self) -> Vec<(String, UsageSnapshot)> {
        let mut report: Vec<(String, UsageSnapshot)> = self
            .meters
            .lock()
            .expect("tenant usage table is not poisoned")
            .iter()
            .map(|(org, meter)| (org.clone(), meter.snapshot()))
            .collect();
        report.sort_by(|a, b| a.0.cmp(&b.0));
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(org: &str, slug: &str, owners: &[&str]) -> TenantSpec {
        TenantSpec {
            org_id: org.into(),
            slug: slug.into(),
            owners: owners.iter().map(|o| (*o).to_owned()).collect(),
            owner_ids: BTreeMap::new(),
            policy: Arc::new(PolicySet::new(Vec::new())),
        }
    }

    #[test]
    fn tenants_resolve_by_org_and_by_slug_and_nothing_else_does() {
        let table = TenantTable::new();
        table.store(
            7,
            vec![
                spec("org_a", "acme", &["acme"]),
                spec("org_b", "beta", &["beta"]),
            ],
        );
        let snap = table.snapshot();
        assert_eq!(snap.version(), 7);
        assert_eq!(snap.resolve_org("org_a").unwrap().slug(), "acme");
        assert_eq!(snap.resolve_slug("beta").unwrap().id().as_str(), "org_b");
        assert!(snap.resolve_org("org_zzz").is_none());
        assert!(
            snap.resolve_slug("Acme").is_none(),
            "slugs are case-sensitive"
        );
    }

    #[test]
    fn a_later_duplicate_org_or_slug_is_dropped_so_a_slug_never_resolves_to_two_orgs() {
        let table = TenantTable::new();
        table.store(
            1,
            vec![
                spec("org_a", "acme", &[]),
                spec("org_b", "acme", &[]),
                spec("org_a", "other", &[]),
            ],
        );
        let snap = table.snapshot();
        assert_eq!(snap.resolve_slug("acme").unwrap().id().as_str(), "org_a");
        assert!(snap.resolve_slug("other").is_none());
        assert_eq!(snap.tenants().count(), 1);
    }

    #[test]
    fn owns_matches_case_insensitively_and_checks_the_recorded_numeric_id() {
        let mut s = spec("org_a", "acme", &["acme", "octocat"]);
        s.owner_ids.insert("acme".into(), 900);
        let table = TenantTable::new();
        table.store(1, vec![s]);
        let t = table.snapshot().resolve_org("org_a").unwrap().clone();
        assert!(t.owns("ACME", Some(900)));
        assert!(!t.owns("acme", Some(901)), "a recorded id must match");
        assert!(!t.owns("acme", None), "a recorded id must be presented");
        assert!(t.owns("octocat", None), "no recorded id: login only");
        assert!(!t.owns("someone-else", Some(900)));
    }

    #[test]
    fn a_mixed_case_owner_id_key_still_enforces_the_recorded_numeric_id() {
        // The control plane sends a key that is not lowercase. Were it kept
        // as-is, the lowercased lookup in `owns` would miss it and silently
        // fall back to login-only.
        let mut s = spec("org_a", "acme", &["Acme"]);
        s.owner_ids.insert("Acme".into(), 900);
        let table = TenantTable::new();
        table.store(1, vec![s]);
        let t = table.snapshot().resolve_org("org_a").unwrap().clone();
        assert!(!t.owns("acme", Some(901)), "a wrong id must be refused");
        assert!(!t.owns("acme", None), "a recorded id must be presented");
        assert!(t.owns("ACME", Some(900)));
    }

    #[test]
    fn usage_is_metered_per_org_and_reported_in_org_order() {
        let table = TenantTable::new();
        table.store(1, vec![spec("org_b", "b", &[]), spec("org_a", "a", &[])]);
        let snap = table.snapshot();
        let a = snap.resolve_org("org_a").unwrap().id().clone();
        let b = snap.resolve_org("org_b").unwrap().id().clone();

        let usage = TenantUsage::new();
        assert!(
            usage.snapshot().is_empty(),
            "nothing is reported before use"
        );
        let meter = usage.meter_for(&a);
        meter.add_to_target(7);
        assert!(
            Arc::ptr_eq(&meter, &usage.meter_for(&a)),
            "one meter per org, reused"
        );
        usage.meter_for(&b).add_to_client(3);

        let report = usage.snapshot();
        let orgs: Vec<&str> = report.iter().map(|(org, _)| org.as_str()).collect();
        assert_eq!(orgs, ["org_a", "org_b"]);
        assert_eq!(report[0].1.bytes_to_target, 7);
        assert_eq!(report[0].1.bytes_to_client, 0);
        assert_eq!(report[1].1.bytes_to_client, 3);
    }

    #[test]
    fn replacing_the_table_removes_tenants_that_are_gone() {
        let table = TenantTable::new();
        table.store(1, vec![spec("org_a", "a", &[]), spec("org_b", "b", &[])]);
        assert!(table.snapshot().resolve_org("org_b").is_some());
        table.store(2, vec![spec("org_a", "a", &[])]);
        let snap = table.snapshot();
        assert_eq!(snap.version(), 2);
        assert!(snap.resolve_org("org_b").is_none());
        assert!(snap.resolve_slug("b").is_none());
        assert!(snap.resolve_org("org_a").is_some());
    }
}
