# Platform Gateway (`--platform`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `skimasque-server --platform`: one gateway process that serves many SkiMasque organisations, routing each CI job to exactly one tenant, checking it against that tenant's policy only, signing with that tenant's key only, and recording audit and usage per tenant.

**Architecture:** Tenant routing lives in the `skimasque` library (a `TenantTable` that alone mints `TenantId`s, a tenant-verifying identity layer, a tenant-aware policy layer, per-tenant quotas and usage meters). The CLI crate owns everything that needs signing keys or the control plane: the platform control-protocol client, the tenant sync loop, the token-exchange minter, the tunnel-side credential verifier, per-org audit shipping and heartbeat usage. `--platform` only wires these together; without it the gateway behaves exactly as today.

**Tech Stack:** Rust 2021 workspace (`skimasque`, `skimasque-cli`, `skimasque-identity`, `skimasque-protocol`, `skimasque-policy`), tokio, tower, reqwest, jsonwebtoken (Ed25519), `metrics`.

**Spec:** `docs/superpowers/specs/2026-09-28-multi-tenant-shared-gateway-design.md` in the private `skimasque-dev/control` repo (branch `docs/multi-tenant-gateway-spec`), sections "Invariants", "2. Platform gateway ↔ control-plane protocol", "3. Gateway platform mode". The wire types (`skimasque_protocol::platform`), `CredentialSigner::issue_for_org`, `CredentialVerifier::verify_for_org` and `peek_org_id` already exist (rollout steps 1-3). This plan is rollout step 4.

**Repository and branch:** `skimasque-dev/skimasque` (public), branch `feat/platform-gateway` off `origin/main` (worktree `../wt-platform-gw`). Do not push or open a PR unless your human partner asks. Verify with `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`.

## Global Constraints (the spec's invariants; each has a test)

1. A credential signed with org A's key is **never** accepted as org B.
2. A request resolved to org A is **never** evaluated against org B's policy.
3. An audit event is **never** filed under an org other than the one its credential names.
4. A job whose `repository_owner` is not a verified owner of the audience's org **never** receives a credential.
5. Removing an owner claim or a tenant takes effect at the next tenant-list refresh; credentials already issued live out their TTL.

Also:

- **No default tenant and no fallback.** A request with no resolved tenant is refused. Platform mode has **no** local (HS256) minting: a control plane that is down is `502`.
- **`TenantId` is minted only by the `TenantTable`.** Its constructor is `pub(crate)` in the `skimasque` crate; nothing else can create one. Every tenant-specific lookup takes a `&TenantId`.
- **Quotas, meters and audit are tenant-scoped.** The existing `QuotaLayer` keys limiters by policy name only, so two orgs that both name a policy `prod` would share a concurrency limit: in tenant mode the key must include the tenant.
- Without `--platform` nothing changes: every existing test must keep passing, and the single-org code paths are not edited except to add optional fields.
- The audience is exactly `https://<--hostname>/o/<slug>` (`skimasque_protocol::platform::slug_from_audience`); case is not folded. `--oidc-audience` and `--credential-secret` are ignored in platform mode.
- The OIDC numeric `repository_owner_id` must match the claim's recorded id when the tenant has one (`Tenant.owner_ids`); a claim with no recorded id is matched by login only.
- Control-plane refusals (`owner_not_verified`, `over_cap`, `unknown_org`) are relayed **verbatim** (code and message) to the CI log.
- No AI-attribution trailers in commit messages.

## Review Focus

1. **Cross-tenant credential confusion.** A credential signed by A's key that names B, a credential with no `org_id`, and an unknown `org_id` are all refused (Task 5 `a_credential_signed_by_a_but_claiming_b_is_refused`).
2. **Quota/limiter sharing across tenants.** Two tenants with the same policy name get independent limiters (Task 3 `two_tenants_with_the_same_policy_name_get_independent_concurrency_limits`).
3. **Tenant removal mid-flight.** New tunnels are refused after a refresh; open tunnels are untouched (Task 3 `a_removed_tenant_is_refused_for_new_tunnels`).
4. **Control-plane outage.** Token exchange answers `502` with no local mint, and enforcement of already-issued credentials continues from the cached tenant table (Tasks 5, 7).
5. **A tenant whose policy fails to parse** must be deny-all for that tenant only, and must not break the others (Task 5 `a_tenant_with_a_broken_policy_is_deny_all_and_others_are_unaffected`).

---

## Task 1: Identity crate: per-call audience, unverified audience peek, numeric owner id

**Files:**
- Modify: `crates/skimasque-identity/src/verify.rs` (add `Verifier::with_audience`)
- Modify: `crates/skimasque-identity/src/provider.rs` (add `Claims::owner_id`)
- Modify: `crates/skimasque-identity/src/lib.rs` (add `peek_audiences`, `OidcVerifier::verify_claims_for_audience`, exports)

**Interfaces:**
- Consumes: the existing `Verifier`, `OidcVerifier`, `Claims`.
- Produces:
  - `pub fn peek_audiences(token: &str) -> Vec<String>`: the JWT's `aud` (a string or an array) read **without verifying anything**; empty for a malformed token. Used only to pick which tenant's audience to verify against.
  - `OidcVerifier::verify_claims_for_audience(&self, token: &str, audience: &str) -> Result<Claims, Error>`: like `verify_claims` (including the one-shot JWKS refresh on `UnknownKey`) but requiring `aud` to contain exactly `audience`, ignoring the verifier's configured audiences.
  - `Claims::owner_id(&self) -> Option<u64>`: GitHub's numeric `repository_owner_id` (a string claim; a JSON number is also accepted).

- [ ] **Step 1: Write the failing tests** in the existing `tests` modules:

  ```rust
  // lib.rs tests
  #[test]
  fn peek_audiences_reads_a_string_or_an_array_and_never_panics() {
      let issuer = TestIssuer::new();
      let one = sign(&issuer, serde_json::json!({"iss": GITHUB_ACTIONS_ISSUER, "aud": "https://g.example/o/acme", "exp": now() + 60}));
      assert_eq!(peek_audiences(&one), vec!["https://g.example/o/acme".to_owned()]);
      let many = sign(&issuer, serde_json::json!({"iss": GITHUB_ACTIONS_ISSUER, "aud": ["a", "b"], "exp": now() + 60}));
      assert_eq!(peek_audiences(&many), vec!["a".to_owned(), "b".to_owned()]);
      for junk in ["", "x", "a.b", "a.!!.c"] {
          assert!(peek_audiences(junk).is_empty(), "{junk:?}");
      }
  }

  #[tokio::test]
  async fn verify_claims_for_audience_requires_that_exact_audience() {
      let issuer = TestIssuer::new();
      // The verifier is configured for a *different* audience than the token's.
      let verifier = verifier_for(&issuer); // configured for https://masque.example
      let tok = sign(&issuer, serde_json::json!({
          "iss": GITHUB_ACTIONS_ISSUER, "aud": "https://g.example/o/acme", "exp": now() + 3600,
          "repository_owner": "acme", "repository": "acme/w",
      }));
      assert!(verifier.verify_claims(&tok).await.is_err());
      assert!(verifier.verify_claims_for_audience(&tok, "https://g.example/o/acme").await.is_ok());
      assert!(verifier.verify_claims_for_audience(&tok, "https://g.example/o/other").await.is_err());
  }

  // provider.rs tests
  #[test]
  fn owner_id_reads_github_numeric_owner_id_as_string_or_number() {
      assert_eq!(claims(serde_json::json!({"repository_owner_id": "4242"})).owner_id(), Some(4242));
      assert_eq!(claims(serde_json::json!({"repository_owner_id": 4242})).owner_id(), Some(4242));
      assert_eq!(claims(serde_json::json!({"repository_owner_id": "abc"})).owner_id(), None);
      assert_eq!(claims(serde_json::json!({})).owner_id(), None);
  }
  ```

- [ ] **Step 2: Run them to verify they fail**

  Run: `cargo test -p skimasque-identity`
  Expected: compile errors (`peek_audiences`, `verify_claims_for_audience`, `owner_id` not found).

- [ ] **Step 3: Implement.** `Verifier::with_audience(&self, audience: &str) -> Verifier` clones the verifier with its audience list replaced by `[audience]` (keep the issuer and leeway). `OidcVerifier::verify_claims_for_audience` mirrors `verify_claims` but calls `self.verifier.with_audience(audience).verify(...)`. `peek_audiences` base64url-decodes the second JWT segment (as `peek_org_id` does) and reads `aud` as string-or-array. `Claims::owner_id` reads `repository_owner_id` as a string parsed to `u64`, or a JSON number.

- [ ] **Step 4: Run the tests to verify they pass**

  Run: `cargo test -p skimasque-identity` (Expected: all pass) and `cargo clippy -p skimasque-identity --all-targets -- -D warnings`.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/skimasque-identity
  git commit -m "feat(identity): verify an OIDC token against a chosen audience; peek audiences; numeric owner id"
  ```

---

## Task 2: Library: the tenant model, `MintError::Refused`, and `AuditEvent.org_id`

**Files:**
- Create: `crates/skimasque/src/tenant.rs`
- Modify: `crates/skimasque/src/lib.rs` (declare and re-export)
- Modify: `crates/skimasque/src/exchange.rs` (`MintError::Refused`)
- Modify: `crates/skimasque/src/server.rs` (map `Refused` to `403` with its own code)
- Modify: `crates/skimasque/src/audit.rs` (optional `org_id` on `AuditEvent`)
- Test: in those files' `tests` modules, and `crates/skimasque/tests/roundtrip.rs` for the exchange refusal

**Interfaces:**
- Produces in `skimasque::tenant`:

  ```rust
  /// An organisation the gateway serves. Minted only by [`TenantTable`]: there is
  /// no public constructor, so a `TenantId` in hand means "this org is a tenant".
  #[derive(Debug, Clone, PartialEq, Eq, Hash)]
  pub struct TenantId(std::sync::Arc<str>);
  impl TenantId { pub fn as_str(&self) -> &str; }

  pub struct TenantSpec {           // what the control plane says about one tenant
      pub org_id: String,
      pub slug: String,
      pub owners: Vec<String>,                       // already normalised (lowercase)
      pub owner_ids: std::collections::BTreeMap<String, u64>,
      pub policy: std::sync::Arc<skimasque_policy::PolicySet>,
  }

  pub struct Tenant { /* id, slug, owners, owner_ids, policy */ }
  impl Tenant {
      pub fn id(&self) -> &TenantId;
      pub fn slug(&self) -> &str;
      pub fn policy(&self) -> &std::sync::Arc<skimasque_policy::PolicySet>;
      /// `owner` is matched case-insensitively; when an id is recorded for that
      /// login, `owner_id` must equal it.
      pub fn owns(&self, owner: &str, owner_id: Option<u64>) -> bool;
  }

  pub struct TenantSnapshot { /* version + by_org + by_slug */ }
  impl TenantSnapshot {
      pub fn version(&self) -> u64;
      pub fn resolve_org(&self, org_id: &str) -> Option<&std::sync::Arc<Tenant>>;
      pub fn resolve_slug(&self, slug: &str) -> Option<&std::sync::Arc<Tenant>>;
      pub fn tenants(&self) -> impl Iterator<Item = &std::sync::Arc<Tenant>>;
  }

  /// The live tenant table: cheap to clone, swapped atomically on each refresh.
  #[derive(Clone)] pub struct TenantTable { /* watch::Sender<Arc<TenantSnapshot>> */ }
  impl TenantTable {
      pub fn new() -> Self;                                   // empty, version 0
      pub fn store(&self, version: u64, tenants: Vec<TenantSpec>);
      pub fn snapshot(&self) -> std::sync::Arc<TenantSnapshot>;
      pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<std::sync::Arc<TenantSnapshot>>;
  }
  ```

  Duplicate `org_id` or `slug` inside one `store` call: the **later entry is dropped and logged** (a slug must never resolve to two orgs).
- Produces: `MintError::Refused { code: String, message: String }` (server answers `403` with `error_body(code, message)`); `AuditEvent.org_id: Option<String>` (`#[serde(skip_serializing_if = "Option::is_none")]`, `None` everywhere it is built today).

- [ ] **Step 1: Write the failing tests**

  ```rust
  // tenant.rs
  fn spec(org: &str, slug: &str, owners: &[&str]) -> TenantSpec { /* PolicySet::default-ish; owners vec */ }

  #[test]
  fn tenants_resolve_by_org_and_by_slug_and_nothing_else_does() {
      let table = TenantTable::new();
      table.store(7, vec![spec("org_a", "acme", &["acme"]), spec("org_b", "beta", &["beta"])]);
      let snap = table.snapshot();
      assert_eq!(snap.version(), 7);
      assert_eq!(snap.resolve_org("org_a").unwrap().slug(), "acme");
      assert_eq!(snap.resolve_slug("beta").unwrap().id().as_str(), "org_b");
      assert!(snap.resolve_org("org_zzz").is_none());
      assert!(snap.resolve_slug("Acme").is_none(), "slugs are case-sensitive");
  }

  #[test]
  fn a_later_duplicate_org_or_slug_is_dropped_so_a_slug_never_resolves_to_two_orgs() {
      let table = TenantTable::new();
      table.store(1, vec![spec("org_a", "acme", &[]), spec("org_b", "acme", &[]), spec("org_a", "other", &[])]);
      let snap = table.snapshot();
      assert_eq!(snap.resolve_slug("acme").unwrap().id().as_str(), "org_a");
      assert!(snap.resolve_slug("other").is_none());
      assert_eq!(snap.tenants().count(), 1);
  }

  #[test]
  fn owns_matches_case_insensitively_and_checks_the_recorded_numeric_id() {
      let mut s = spec("org_a", "acme", &["acme", "octocat"]);
      s.owner_ids.insert("acme".into(), 900);
      let t = /* build the Tenant from `s` via a one-tenant table */;
      assert!(t.owns("ACME", Some(900)));
      assert!(!t.owns("acme", Some(901)), "a recorded id must match");
      assert!(!t.owns("acme", None), "a recorded id must be presented");
      assert!(t.owns("octocat", None), "no recorded id: login only");
      assert!(!t.owns("someone-else", Some(900)));
  }

  #[test]
  fn replacing_the_table_removes_tenants_that_are_gone() { /* store v1 {a,b}; store v2 {a}; b unresolved, version 2 */ }

  // audit.rs
  #[test]
  fn org_id_is_omitted_from_single_org_events_and_present_when_set() { /* serde_json: no "org_id" key vs "org_id":"org_a" */ }

  // tests/roundtrip.rs: extend the existing exchange test with a minter returning
  // MintError::Refused { code: "owner_not_verified", message: "acme is not verified for widgets" }
  // and assert HTTP 403 with body error == "owner_not_verified" and the message verbatim.
  ```

- [ ] **Step 2: Run them to verify they fail**

  Run: `cargo test -p skimasque tenant audit roundtrip`
  Expected: compile errors: `tenant` module, `MintError::Refused`, `AuditEvent::org_id` missing.

- [ ] **Step 3: Implement.** `TenantTable::store` builds both maps, constructs each `TenantId` inside `Tenant::new` (`pub(crate)`), drops duplicates with `tracing::warn!`, and `send_replace`s a new `Arc<TenantSnapshot>`. `owns` lowercases the presented login, checks `owners.contains`, then compares `owner_ids.get(login)` if present. In `server.rs` add the `Refused` arm next to `Unauthorized`. Add `org_id: None` to every `AuditEvent` constructor (`from_decision` and tests).

- [ ] **Step 4: Run the tests to verify they pass**

  Run: `cargo test -p skimasque` and `cargo clippy -p skimasque --all-targets -- -D warnings`
  Expected: pass (the existing 100+ tests unchanged).

- [ ] **Step 5: Commit**

  ```bash
  git add crates/skimasque
  git commit -m "feat(skimasque): tenant table with unforgeable TenantId; MintError::Refused; org_id on audit events"
  ```

---

## Task 3: Library layers: tenant identity, tenant-aware policy, per-tenant quotas and usage

**Files:**
- Modify: `crates/skimasque/src/service.rs` (`TenantLayer`, tenant mode for `PolicyLayer`/`Enforce`, tenant-keyed `QuotaLayer`, per-tenant `TunnelMeter`)
- Modify: `crates/skimasque/src/server.rs` (call the meter where bytes and tunnel opens are counted; ~5 sites)
- Modify: `crates/skimasque/src/tenant.rs` (`TenantUsage`)
- Modify: `crates/skimasque/src/lib.rs` (exports)

**Interfaces:**
- Produces:

  ```rust
  /// Verifies a presented credential and resolves the tenant it belongs to.
  /// The implementation must only return a `TenantId` it obtained from a
  /// `TenantSnapshot` (that is the only place one can come from).
  pub trait TenantVerifier: Send + Sync + std::fmt::Debug {
      fn verify(&self, token: String) -> Pin<Box<dyn Future<Output = Result<(TenantId, WorkloadIdentity), String>> + Send>>;
  }
  pub struct TenantLayer { /* Arc<dyn TenantVerifier> */ }   // like IdentityLayer, but inserts TenantId AND WorkloadIdentity
  impl PolicyLayer { pub fn tenants(table: TenantTable) -> Self; }   // per-tenant policy sets; audits with org_id
  // QuotaLayer::new() is unchanged; it now namespaces its keys with the tenant when present.
  pub struct TunnelMeter { /* atomics: tunnels_opened, bytes_to_target, bytes_to_client */ }
  pub struct TenantUsage { /* Mutex<HashMap<String, Arc<TunnelMeter>>> */ }
  impl TenantUsage { pub fn new() -> Self; pub fn meter_for(&self, id: &TenantId) -> Arc<TunnelMeter>;
                     pub fn snapshot(&self) -> Vec<(String /* org_id */, UsageSnapshot)>; }
  pub struct TenantMeterLayer { /* Arc<TenantUsage> */ }   // attaches the tenant's meter to Accepted
  impl Accepted { pub fn with_meter(self, m: Arc<TunnelMeter>) -> Self; }
  ```

  `Enforce` in tenant mode: no `TenantId` in the request extensions -> `403` ("no organisation resolved for this request"); `TenantId` whose org is no longer in the current snapshot -> `403` ("organisation is not served by this gateway"); otherwise evaluate against **that tenant's** `PolicySet` only and put `org_id` on the audit event.

- [ ] **Step 1: Write the failing tests** (module `tenant_layers` in `service.rs` tests; build two tenants A and B where A's policy allows `10.0.0.1:443` and B's allows `10.0.0.2:443`, each with a policy named `prod`; use the existing stub-inner-service and `TunnelRequest` test helpers in this file, and a `TenantVerifier` stub mapping token `"a"` to tenant A and `"b"` to tenant B via `table.snapshot().resolve_org(..)`):

  - `a_request_resolved_to_a_is_denied_by_as_policy_even_if_bs_policy_would_allow_it`: token `a` to `10.0.0.2:443` is denied (403); token `b` to the same destination is allowed. (Invariant 2)
  - `an_audit_event_is_tagged_with_the_tenant_the_credential_named`: a recording `AuditSink`; one allow for A, one deny for B; `org_id` equals `org_a` / `org_b` respectively. (Invariant 3)
  - `a_request_with_no_resolved_tenant_is_refused_never_defaulted`: `Enforce` called without `TenantId` in extensions -> 403 and no audit event files under any org.
  - `a_removed_tenant_is_refused_for_new_tunnels`: `table.store(2, vec![only B])`; token `a` (already verified, `TenantId` for A in hand) -> 403; B still works.
  - `a_tenant_with_no_policy_denies_everything`: a tenant stored with an empty `PolicySet` -> every request denied.
  - `two_tenants_with_the_same_policy_name_get_independent_concurrency_limits`: both policies `prod` with `concurrent_connections = 1`; A holds its permit; B's request still gets a permit; A's second request is refused `503 connection_limit_reached`.
  - `the_tenant_meter_counts_only_its_own_tenants_traffic`: `TenantUsage::meter_for(A)` and `(B)`; bump A's; `snapshot()` reports A>0, B==0.
  - `the_global_usage_totals_still_count_everything` (in `metrics.rs`/`server.rs` tests): the existing process-wide `usage_snapshot()` keeps moving when a tenant meter is attached.

- [ ] **Step 2: Run them to verify they fail**

  Run: `cargo test -p skimasque tenant_layers`
  Expected: compile errors (`TenantLayer`, `PolicyLayer::tenants`, `TenantUsage`, ... not defined).

- [ ] **Step 3: Implement.**
  - `TenantLayer`/`TenantIdentify<S>` mirror `Identify<S>`: read `Proxy-Authorization: Bearer`, `407` when absent, call the `TenantVerifier`, `403` on error, then `extensions_mut().insert(tenant_id)` **and** `insert(identity)`.
  - `PolicyLayer::tenants(table)` stores a `PolicySource::Tenants(TenantTable)`; `Enforce` holds `enum PolicySource { Single(watch::Receiver<Arc<PolicySet>>), Tenants(watch::Receiver<Arc<TenantSnapshot>>) }`. Keep `PolicyLayer::new/from_handle/handle` and the single-org behaviour byte-for-byte. Keep the `handle: PolicyHandle` field and `handle()`'s signature exactly as they are (existing callers use them): a tenant-mode layer simply seeds that handle with an empty set and ignores it, and the doc comment on `PolicyLayer::tenants` says so.
  - `QuotaLayer`: compute `let scope = request.extensions().get::<TenantId>().map(|t| t.as_str().to_owned())`; when `Some`, use `format!("{scope}/{policy}")` as the limiter key (permits, bandwidth, packets). When `None`, keys are unchanged.
  - `TunnelMeter`/`TenantUsage`/`TenantMeterLayer`: the layer reads `TenantId`, `usage.meter_for(&id)`, increments `tunnels_opened` on success, and calls `accepted.with_meter(meter)`. In `server.rs` the relay loops call `meter.add_to_target(n)` / `add_to_client(n)` beside the existing `crate::metrics::bytes_relayed(...)`.

- [ ] **Step 4: Run the tests to verify they pass**

  Run: `cargo test -p skimasque` and `cargo clippy -p skimasque --all-targets -- -D warnings`
  Expected: pass, including every pre-existing test (single-tenant behaviour unchanged).

- [ ] **Step 5: Commit**

  ```bash
  git add crates/skimasque
  git commit -m "feat(skimasque): tenant identity and policy layers, tenant-scoped quotas and usage meters"
  ```

---

## Task 4: CLI: the platform control-plane client

**Files:**
- Modify: `crates/skimasque-cli/src/control.rs`
- Test: `crates/skimasque-cli/tests/platform_control.rs` (new; a stub HTTP control plane built on `hyper`, which is already a dependency)

**Interfaces:**
- Consumes: `skimasque_protocol::platform::*` and `paths::platform_*`.
- Produces on `ControlPlane`:

  ```rust
  pub async fn register_platform(&self, token: &str, name: &str, labels: &BTreeMap<String,String>) -> Result<GatewayIdentity>;
      // POST PLATFORM_REGISTER; persists gateway.json (org_id = "").
  pub async fn fetch_tenants(&self, identity: &GatewayIdentity, known_version: Option<u64>, wait: Option<Duration>) -> Result<TenantFetch>;
      // GET platform_tenants(id) with If-None-Match/?wait=, like fetch_policy.
  pub enum TenantFetch { Updated(TenantList), Unchanged }
  pub async fn platform_mint(&self, identity: &GatewayIdentity, req: &PlatformMintRequest) -> Result<MintedCredential, PlatformMintError>;
  pub enum PlatformMintError { Refused(Refusal), Unavailable(anyhow::Error) }   // 403/422 with a Refusal body vs everything else
  pub async fn ship_platform_audit(&self, identity: &GatewayIdentity, batch: &PlatformShipAuditRequest) -> Result<ShipAuditResponse>;
  pub async fn platform_audit_head(&self, identity: &GatewayIdentity) -> Result<AuditHead>;
  pub async fn platform_heartbeat(&self, identity: &GatewayIdentity, req: &PlatformHeartbeatRequest) -> Result<()>;
  ```

- [ ] **Step 1: Write the failing tests** against a stub server that records requests and returns canned responses:
  - `register_platform_posts_the_token_and_persists_an_identity_with_no_org`
  - `fetch_tenants_sends_if_none_match_and_wait_and_maps_304_to_unchanged`
  - `a_403_with_a_refusal_body_is_a_refused_mint_carrying_its_code_and_message_verbatim` (code `owner_not_verified`)
  - `a_5xx_or_a_connection_error_is_an_unavailable_mint`
  - `platform_mint_sends_the_org_identity_owner_id_and_ttl`
  - `ship_platform_audit_and_heartbeat_post_to_the_platform_paths_with_the_bearer_secret`

- [ ] **Step 2: Run them to verify they fail**

  Run: `cargo test -p skimasque-cli --test platform_control`
  Expected: compile errors: the methods do not exist.

- [ ] **Step 3: Implement** following `register`, `fetch_policy`, `mint_credential`, `ship_audit` and `heartbeat` already in `control.rs` (same `error_for_status` helper and bearer auth). `platform_mint` parses a `Refusal` from a 403/422 body and returns `PlatformMintError::Refused`; any other failure is `Unavailable`.

- [ ] **Step 4: Run the tests to verify they pass**

  Run: `cargo test -p skimasque-cli` and `cargo clippy -p skimasque-cli --all-targets -- -D warnings`
  Expected: pass.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/skimasque-cli
  git commit -m "feat(cli): platform control-plane client (register, tenants, mint, audit, heartbeat)"
  ```

---

## Task 5: CLI: tenant sync, the tunnel-side verifier, and the token-exchange minter

**Files:**
- Create: `crates/skimasque-cli/src/platform.rs`
- Modify: `crates/skimasque-cli/src/lib.rs` (`pub mod platform;`)
- Test: in `platform.rs`, and `crates/skimasque-cli/tests/platform_exchange.rs`

**Interfaces:**
- Consumes: Tasks 1-4.
- Produces in `skimasque_cli::platform`:

  ```rust
  /// The live platform state: the library's tenant table plus, per org id, that
  /// org's credential-verification keys. The table is the authority: the key map
  /// is only consulted for an org the table currently resolves.
  #[derive(Clone)] pub struct PlatformTenants { pub table: TenantTable, /* keys: watch<Arc<HashMap<String, Arc<CredentialVerifier>>>> */ }
  impl PlatformTenants {
      pub fn new() -> Self;
      pub fn apply(&self, list: &TenantList);          // parse, build verifiers, table.store(...)
      pub fn load_cache(state_dir: &Path) -> Option<TenantList>;
      pub fn write_cache(state_dir: &Path, list: &TenantList) -> Result<()>;   // atomic; <state>/tenants.json
  }
  pub async fn run_tenant_sync(control: ControlPlane, identity: GatewayIdentity, tenants: PlatformTenants, state_dir: PathBuf, interval: Duration);
  pub struct PlatformTenantVerifier { /* PlatformTenants */ }   // impl skimasque::TenantVerifier
  pub struct PlatformMinter { /* OidcVerifier, hostname/base_url, control, identity, tenants, ttl */ }   // impl CredentialMinter
  ```

- `PlatformTenants::apply`: for each tenant parse `policy.documents` into a `PolicySet`; **a parse failure yields an empty (deny-all) set for that tenant, increments `skimasque_platform_tenant_policy_total{outcome="rejected"}` and logs, and never affects other tenants**; a missing policy (`None`) is deny-all. Build each tenant's `CredentialVerifier` from `signing_key.all_public_key_bytes()`; a key that does not decode leaves that tenant with a verifier that rejects everything.
- `PlatformTenantVerifier::verify(token)`: `peek_org_id(token)` (none -> `Err("credential is not scoped to an organisation")`), `snapshot.resolve_org(org)` (none -> `Err("unknown organisation")`), take **only that org's** verifier, `verify_for_org(token, org)`, return `(tenant.id().clone(), identity)`. No HS256 path.
- `PlatformMinter::mint(oidc_token)` (spec §3 "Token exchange"):
  1. `peek_audiences` -> the one audience for which `slug_from_audience(base, aud)` is `Some(slug)`; none -> `Refused{"unknown_org", "Unknown SkiMasque organisation ... Check the `audience` in your workflow."}`. `snapshot.resolve_slug(slug)` none -> same refusal.
  2. `oidc.verify_claims_for_audience(token, &tenant_audience(base, slug))` (Discovery/Jwks errors -> `Unavailable`, others -> `Unauthorized`).
  3. `provider.identify(&claims)`; `owner = normalize_owner(identity.organization)`; `tenant.owns(&owner, claims.owner_id())` else `Refused{"owner_not_verified", "`<owner>` is not verified for `<slug>`. Verify it at <dashboard settings URL>."}`.
  4. `control.platform_mint(...)`; `Refused(r)` -> `MintError::Refused{r.code, r.message}` **verbatim**; `Unavailable` -> `MintError::Unavailable("SkiMasque control plane unreachable; try again shortly.")`. No local minting.

- [ ] **Step 1: Write the failing tests** (two tenants `acme`/`beta`, each with its own Ed25519 key from `CredentialSigner`; a stub control plane; the `skimasque-identity` `test_support`-style signing helper or a fixed RSA test key for OIDC tokens; see how `skimasque-identity`'s tests mint OIDC tokens):
  - **Invariant 1** `a_credential_signed_by_a_but_claiming_b_is_refused`: sign with A's key a credential `issue_for_org(.., "org_b")`; `PlatformTenantVerifier` refuses it; also refused: no `org_id`, unknown `org_id`, and B's real credential resolves to B (not A).
  - **Invariant 4** `a_job_whose_owner_is_not_verified_never_receives_a_credential`: the control plane is **not called**; refusal code `owner_not_verified`; also a recorded numeric id that differs from `repository_owner_id`.
  - `an_unknown_slug_is_refused_with_unknown_org` and `a_wrong_case_slug_is_not_folded_into_another_org`.
  - `a_control_plane_refusal_is_relayed_verbatim` (over_cap message).
  - `a_control_plane_that_is_down_is_a_502_style_unavailable_with_no_local_mint`.
  - `a_tenant_with_a_broken_policy_is_deny_all_and_others_are_unaffected`.
  - `a_tenant_with_a_bad_signing_key_verifies_nothing_but_others_still_do`.
  - `the_tenant_cache_round_trips_and_a_restart_with_the_control_plane_down_still_enforces`: write the cache, build a fresh `PlatformTenants` from `load_cache`, verify a credential.
  - `a_removed_tenant_stops_verifying_at_the_next_apply` (**Invariant 5**).

- [ ] **Step 2: Run them to verify they fail**

  Run: `cargo test -p skimasque-cli platform`
  Expected: compile errors: `platform` module missing.

- [ ] **Step 3: Implement** as specified above. `run_tenant_sync` mirrors `run_control_plane_sync`: long-poll with `known_version`, on `Updated` -> `apply` + `write_cache`, exponential backoff (1s doubling to 60s) on error, never stopping enforcement.

- [ ] **Step 4: Run the tests to verify they pass**

  Run: `cargo test -p skimasque-cli` and `cargo clippy -p skimasque-cli --all-targets -- -D warnings`
  Expected: pass.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/skimasque-cli
  git commit -m "feat(cli): platform tenant sync, tenant-scoped credential verifier and token-exchange minter"
  ```

---

## Task 6: CLI: per-org audit shipping and heartbeat usage

**Files:**
- Modify: `crates/skimasque-cli/src/audit_ship.rs` (extract the chain-and-ship loop so single-org and platform share `chain_batch`, `resume`, `persist`)
- Modify: `crates/skimasque-cli/src/platform.rs` (`run_platform_audit_shipping`, `run_platform_heartbeat`)

**Interfaces:**
- Produces:
  - `run_platform_audit_shipping(control, identity, rx: mpsc::Receiver<AuditEvent>, chain_path: PathBuf)`: like `run_audit_shipping`, but each event becomes a `PlatformAuditEvent { org_id: event.org_id, seq, prev_hash, event_json }`. **An event with no `org_id` is dropped and counted, never shipped** (it must never be filed under an org it does not name). The chain is one per gateway.
  - `run_platform_heartbeat(control, identity, state, tenants_version: impl Fn() -> Option<u64>, usage: Arc<TenantUsage>, interval)`: sends `PlatformHeartbeatRequest { status, tenants_version, usage_by_org }` with cumulative per-org counters from `TenantUsage::snapshot()`.

- [ ] **Step 1: Write the failing tests**
  - `platform_batches_carry_each_events_own_org_and_keep_one_gateway_chain` (interleave A, B, A events; `prev_hash`/`seq` chain across them; each `org_id` preserved; the hash still covers `event_json` exactly as `skimasque_protocol::audit_hash`).
  - `an_event_with_no_org_is_never_shipped`.
  - `the_heartbeat_reports_cumulative_usage_per_org` (A and B counters distinct; `tenants_version` from the table).
  - The existing single-org shipping tests still pass unchanged.

- [ ] **Step 2: Run them to verify they fail**

  Run: `cargo test -p skimasque-cli audit_ship platform`
  Expected: compile errors / failures for the new functions.

- [ ] **Step 3: Implement** by factoring the shared chaining (`chain_batch` takes an iterator of `(Option<&str> org, &AuditEvent)`), keeping the 409-rebase and backoff behaviour identical.

- [ ] **Step 4: Run the tests to verify they pass**

  Run: `cargo test -p skimasque-cli` and clippy.
  Expected: pass.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/skimasque-cli
  git commit -m "feat(cli): ship platform audit per organisation and report per-org usage in the heartbeat"
  ```

---

## Task 7: `skimasque-server --platform`: flags, wiring, and the cross-tenant end-to-end test

**Files:**
- Modify: `crates/skimasque-cli/src/bin/skimasque-server.rs`
- Create: `crates/skimasque-cli/tests/platform_gateway.rs`

**Interfaces:**
- Produces: the `--platform` flag and platform startup.
  - `#[arg(long, requires = "control_plane", requires = "oidc")] platform: bool`.
  - In platform mode: `--oidc-audience` is not required and is ignored; `--credential-secret` is ignored for minting (a warning is printed if set); `--control-plane-token` registers via `register_platform`.
  - Startup: register or reuse the stored identity; `fetch_tenants` (or, when the control plane is unreachable, `PlatformTenants::load_cache`, else fail: a platform gateway with no tenant table refuses to start); build `PlatformTenants`; the service stack is `limit -> TenantLayer(PlatformTenantVerifier) -> [auth] -> PolicyLayer::tenants(table).with_audit(sink) -> QuotaLayer -> TenantMeterLayer -> dispatch`; the token-exchange minter is `PlatformMinter`; spawn `run_tenant_sync`, `run_platform_audit_shipping`, `run_platform_heartbeat`.
  - The audience base URL is `https://<--hostname>` (strip a default `:443`).
  - A startup line lists the tenant count and prints each slug's audience at `-v`.

- [ ] **Step 1: Write the failing tests**
  - Unit tests in the binary (`mod tests`): `platform_requires_control_plane_and_oidc` (clap), `platform_does_not_need_an_oidc_audience`, `platform_audience_base_uses_the_hostname` (with and without `:443`).
  - `tests/platform_gateway.rs`: an in-process service-level end-to-end with a stub control plane that holds two orgs' **real** Ed25519 signers and serves `/tenants` and `/credentials` (signing with `issue_for_org`):
    - org A's job (token exchange through `PlatformMinter`, then a tunnel request with the minted credential) is **allowed to A's destination, denied B's destination**, and the recorded audit events carry `org_a`;
    - org B's job is the mirror image;
    - a job for A with an unverified owner gets `owner_not_verified`;
    - with the stub control plane stopped, exchange returns the unavailable error but a credential minted earlier **still enforces** (cached tenant table).
  - `skimasque-server --platform` (spawned with `assert_cmd`-style `Command`, or by calling the arg parser) refuses to start without `--control-plane`.

- [ ] **Step 2: Run them to verify they fail**

  Run: `cargo test -p skimasque-cli --test platform_gateway` and `cargo test -p skimasque-cli --bin skimasque-server platform`
  Expected: failures: `--platform` unknown.

- [ ] **Step 3: Implement** the flag and the platform branch of `main`/`build_service`/bootstrap, reusing the existing TLS, shutdown, ops and reload wiring untouched. Factor the platform-specific startup into `async fn bootstrap_platform(args: &Args) -> anyhow::Result<PlatformBootstrap>` so `main` stays readable.

- [ ] **Step 4: Run the whole suite to verify everything passes**

  Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings` then `cargo fmt --all --check`
  Expected: all pass, including the pre-existing tests; `skimasque-server --help` lists `--platform`.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/skimasque-cli
  git commit -m "feat(server): --platform, the multi-tenant shared gateway (Mode 1)"
  ```

---

## Task 8: Documentation

**Files:**
- Modify: `docs/gateways.md` (a "Shared platform gateway" section: what `--platform` is, the audience, refusal codes, what to monitor)
- Modify: `docs/threat-model.md` (fold in the spec's "Security notes")
- Modify: `deploy/systemd/gateway.env.example` (a commented `--platform` alternative)

- [ ] **Step 1: Write the docs**, describing: the audience `https://<hostname>/o/<slug>`; the machine codes `unknown_org`, `owner_not_verified`, `over_cap` and where they appear; that there is no local minting in platform mode (a control-plane outage stops *new* exchanges, not enforcement); the tenant cache file `<state>/tenants.json`; the new metrics; and the cross-tenant invariants.
- [ ] **Step 2: Verify** `cargo doc -p skimasque-cli --no-deps` still builds and the markdown links resolve (`grep` the paths).
- [ ] **Step 3: Commit**

  ```bash
  git add docs deploy/systemd/gateway.env.example
  git commit -m "docs: the shared platform gateway (--platform)"
  ```

---

## After this plan

- **Final review.** One fresh-context review of the whole branch on the most capable model, with the five Review Focus items above and the spec's invariants; fix Critical/Important findings test-first.
- **Follow-ups (separate):** (1) once merged and released, re-add a `mode` input (`single-tenant` | `platform`) to the `gcp-gateway` Terraform module in the deployment PR (`--platform` replaces `--oidc-audience`; the CI flag check will then pass), (2) staging: one platform gateway against a staging control plane with two test orgs using real GitHub OIDC (rollout step 5), (3) `e2e-oidc.yml` extended with two orgs.
- **Out of scope** (per the spec): dedicated gateways, per-credential revocation, lazy per-tenant loading, self-service claim disputes.
