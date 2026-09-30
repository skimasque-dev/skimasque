# `skimasque exec` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `skimasque exec [--policy P] [--app A] [--forward …] -- COMMAND`, which runs a command with policy-scoped network access through a gateway and drops the access when the command exits, plus the open `X-Masque-Policy` pin the gateway enforces.

**Architecture:** The policy engine gains a `requested_policy` on `RequestContext` and denies with `PolicyMismatch` when the selected policy is a different one; the gateway fills it from the `X-Masque-Policy` header and audits it. In the CLI, the session-opening code moves out of `skimasque-client` into a shared `session` module; two small front ends (`http_connect`, `forward`) join the existing SOCKS5 relay; `exec` opens one session, starts the front ends on loopback, sets proxy environment variables, optionally preflights against the control plane, runs the child and tears everything down when it exits.

**Tech Stack:** Rust 1.88 workspace, tokio, clap 4 (derive), the in-repo `skimasque` MASQUE client/server, `skimasque-policy`, `skimasque-identity`, askama 0.12 (`skimasque-visual` site pages).

**Spec:** `docs/superpowers/specs/2026-09-29-skimasque-exec-design.md` (amends `docs/superpowers/specs/2026-09-30-product-surface-amendment.md`, which listed exec as Planned).

## Global Constraints

- Work on branch `feat/exec` in worktree `C:\Users\Thor\Documents\src\skimasque-dev\wt-exec`. Never `cd` elsewhere.
- No AI attribution in commit messages (no `Co-Authored-By`, no "Generated with").
- Do **not** run bare `cargo fmt` (the workspace is not rustfmt-clean). Format only the files you changed: `rustfmt --edition 2021 <file> …`.
- Do not edit files with Python or `sed -i` (CRLF/multibyte mangling on Windows); use the editor tools.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` must pass at the end of every task.
- Header name: `X-Masque-Policy`, constant `POLICY_HEADER = "x-masque-policy"`.
- Deny reason summaries, verbatim: `Policy "<requested>" does not apply to this identity; "<selected>" does.` and `Policy "<requested>" does not apply to this identity; no policy does.`
- Exit codes: child's code; `128 + signal` (Unix signal death); `125` exec failed before launch; `126` not executable; `127` not found.
- Cloud control plane `https://control.skimasque.com`; Cloud gateway `gateway.skimasque.com`.
- No-gateway error, verbatim: `No gateway is configured for <url>. Pass --gateway or set SKIMASQUE_GATEWAY.`
- Env vars set for the child: `HTTPS_PROXY https_proxy HTTP_PROXY http_proxy` = `http://127.0.0.1:<p>`; `ALL_PROXY all_proxy` = `socks5h://127.0.0.1:<p>`; `NO_PROXY no_proxy` = `localhost,127.0.0.1,::1`; `SKIMASQUE_PROXY_HTTP`, `SKIMASQUE_PROXY_SOCKS5` (bare `127.0.0.1:<p>`); `SKIMASQUE_FORWARD_<HOST>_<PORT>=127.0.0.1:<p>`.
- The preflight placeholder destination is `preflight.invalid:1`.
- Ruling: the "not signed in" error is the existing shared one from `resolve_bearer` (`not authenticated: run \`skimasque login\`, or pass --github-oidc/--oidc-token/--auth-token`), not a new exec-only wording — one message for `skimasque-client` and exec; spec §7's sentence is the same instruction.
- Control-repo changes (dependency bump, showing `PolicyMismatch`/`requested_policy`, removing the console's exec Planned labels) are a **separate control PR after this merges** — not in this plan.

## Review Focus

1. **A pinned name that selects a broader policy.** A reasonable operator expects `--policy broad` never to widen access past what the identity's most specific policy allows → Task 1 test `a_pin_naming_a_broader_matching_policy_is_still_denied`.
2. **A proxy client that sends bytes right after the CONNECT head** (pipelined TLS ClientHello). Expected: the bytes reach the destination, not dropped → Task 5 test `bytes_sent_with_the_connect_head_reach_the_destination`.
3. **A command that does not exist / is not executable.** Expected: `127`/`126` with a one-line message, no panic, listeners cleaned up → Task 8 test `a_missing_command_exits_127`.
4. **An existing `NO_PROXY` in the user's environment that would route a forward's loopback address or the proxy itself strangely.** Expected: exec replaces it → Task 7 test `child_env_replaces_no_proxy_and_sets_both_cases`.
5. **A self-hosted user who forgets `--gateway`.** Expected: exit 125 with the exact message, before any network use → Task 7 test `a_self_hosted_control_plane_needs_a_gateway` and Task 8 test `a_self_hosted_control_plane_without_a_gateway_exits_125`.

---

### Task 1: Policy pin in the engine

**Files:**
- Modify: `crates/skimasque-policy/src/eval.rs` (struct `RequestContext` ~L21, enum `DenyReason` ~L87, `DenyReason::summary`, `PolicySet::evaluate` ~L154, tests)
- Modify (add `requested_policy: None` to every `RequestContext { … }` literal): `crates/skimasque-policy/src/lib.rs` (doc example ~L58), `crates/skimasque-policy/src/learn.rs` (~L182, ~L230), `crates/skimasque-policy/src/parse.rs` (~L681), `crates/skimasque-policy/src/report.rs` (~L108, ~L195, ~L210), `crates/skimasque/src/audit.rs` (~L234), `crates/skimasque/src/service.rs` (~L1181)
- Modify: `crates/skimasque-cli/src/policy.rs` (`evaluate` ~L206 and its tests), `crates/skimasque-cli/src/bin/skimasque.rs` (`Why` variant ~L145, dispatch ~L431, `run_why` ~L985, `policy check`/`explain` callers ~L461/~L480)

**Interfaces:**
- Produces: `RequestContext.requested_policy: Option<String>`; `DenyReason::PolicyMismatch { requested: String, selected: Option<String> }`; `skimasque_cli::policy::evaluate(loaded, policy_name, application, transport, destination, identity, requested_policy: Option<&str>)`.

- [ ] **Step 1: Write the failing tests** — append to the `tests` module in `crates/skimasque-policy/src/eval.rs`:

```rust
    // --- the policy pin (`X-Masque-Policy`) ---

    const PINNED_SET: [(&str, &str); 2] = [
        (
            "prod.toml",
            r#"
            name = "prod"
            [match]
            repository = "acme/widget"
            [[rules]]
            application = "terraform"
            action = "allow"
            destinations = ["db.prod:5432"]
            "#,
        ),
        (
            "broad.toml",
            r#"
            name = "broad"
            [[rules]]
            application = "terraform"
            action = "allow"
            destinations = ["db.prod:5432", "other.example:443"]
            "#,
        ),
    ];

    fn pinned(requested: Option<&str>, repository: &str, destination: &str) -> RequestContext {
        RequestContext {
            workload: WorkloadIdentity {
                repository: Some(repository.into()),
                ..Default::default()
            },
            application: "terraform".into(),
            transport: Transport::Tcp,
            destination: Destination::parse(destination).unwrap(),
            requested_policy: requested.map(str::to_owned),
        }
    }

    #[test]
    fn a_pin_naming_the_selected_policy_evaluates_normally() {
        let set = PolicySet::from_documents(PINNED_SET).unwrap();
        match set.evaluate(&pinned(Some("prod"), "acme/widget", "db.prod:5432")) {
            Decision::Allow(a) => assert_eq!(a.policy, "prod"),
            other => panic!("expected allow, got {other:?}"),
        }
    }

    #[test]
    fn no_pin_leaves_selection_unchanged() {
        let set = PolicySet::from_documents(PINNED_SET).unwrap();
        match set.evaluate(&pinned(None, "acme/widget", "other.example:443")) {
            Decision::Deny(d) => {
                assert_eq!(d.policy.as_deref(), Some("prod"));
                assert_eq!(d.reason, DenyReason::NoMatchingAllowRule);
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn a_pin_naming_a_broader_matching_policy_is_still_denied() {
        // `broad` matches every identity and would allow this, but `prod` is
        // the more specific match. The pin must not let a workload escape it.
        let set = PolicySet::from_documents(PINNED_SET).unwrap();
        match set.evaluate(&pinned(Some("broad"), "acme/widget", "other.example:443")) {
            Decision::Deny(d) => {
                assert_eq!(
                    d.reason,
                    DenyReason::PolicyMismatch {
                        requested: "broad".into(),
                        selected: Some("prod".into()),
                    }
                );
                assert_eq!(d.policy.as_deref(), Some("prod"));
                assert!(d.closest.is_empty());
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn a_pin_naming_a_missing_policy_is_denied() {
        let set = PolicySet::from_documents(PINNED_SET).unwrap();
        let d = set.evaluate(&pinned(Some("staging"), "acme/widget", "db.prod:5432"));
        assert!(matches!(
            d,
            Decision::Deny(Denied { reason: DenyReason::PolicyMismatch { .. }, .. })
        ));
    }

    #[test]
    fn a_pin_with_no_selected_policy_names_none() {
        let set = PolicySet::from_documents([PINNED_SET[0]]).unwrap();
        match set.evaluate(&pinned(Some("prod"), "someone/else", "db.prod:5432")) {
            Decision::Deny(d) => {
                assert_eq!(
                    d.reason,
                    DenyReason::PolicyMismatch { requested: "prod".into(), selected: None }
                );
                assert_eq!(d.policy, None);
            }
            other => panic!("expected deny, got {other:?}"),
        }
    }

    #[test]
    fn policy_mismatch_summaries_read_as_the_spec_says() {
        assert_eq!(
            DenyReason::PolicyMismatch { requested: "broad".into(), selected: Some("prod".into()) }
                .summary(),
            r#"Policy "broad" does not apply to this identity; "prod" does."#
        );
        assert_eq!(
            DenyReason::PolicyMismatch { requested: "prod".into(), selected: None }.summary(),
            r#"Policy "prod" does not apply to this identity; no policy does."#
        );
    }

    #[test]
    fn evaluate_named_ignores_the_pin() {
        let set = PolicySet::from_documents(PINNED_SET).unwrap();
        let d = set
            .evaluate_named("broad", &pinned(Some("prod"), "acme/widget", "other.example:443"))
            .unwrap();
        assert!(d.is_allow());
    }
```

Also add `requested_policy: None,` to the existing `ctx_on` helper in the same module.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p skimasque-policy`
Expected: compile errors — `RequestContext` has no field `requested_policy`, no variant `PolicyMismatch`.

- [ ] **Step 3: Implement** — in `eval.rs`:

Add to `RequestContext` after `destination`:

```rust
    /// The policy the client pinned with `X-Masque-Policy`, if any. The engine
    /// never uses it to *choose* a policy: when the policy selected for the
    /// identity has another name, the request is denied. A pin can only narrow
    /// access, never widen it.
    pub requested_policy: Option<String>,
```

Add to `DenyReason`:

```rust
    /// The client pinned a policy (`X-Masque-Policy`) other than the one
    /// selected for its identity.
    PolicyMismatch {
        requested: String,
        selected: Option<String>,
    },
```

Extend `summary()`:

```rust
            Self::PolicyMismatch { requested, selected: Some(selected) } => format!(
                "Policy \"{requested}\" does not apply to this identity; \"{selected}\" does."
            ),
            Self::PolicyMismatch { requested, selected: None } => format!(
                "Policy \"{requested}\" does not apply to this identity; no policy does."
            ),
```

Replace `PolicySet::evaluate` with:

```rust
    /// Select the policy for `ctx.workload` and evaluate against it.
    ///
    /// With no matching policy the result is [`DenyReason::NoPolicyMatch`]: the
    /// least-privilege default is that an unrecognised workload reaches
    /// nothing. When `ctx.requested_policy` names a policy other than the
    /// selected one, the result is [`DenyReason::PolicyMismatch`].
    pub fn evaluate(&self, ctx: &RequestContext) -> Decision {
        let selected = self.select(&ctx.workload);
        if let Some(requested) = &ctx.requested_policy {
            let selected_name = selected.map(|p| p.name.clone());
            if selected_name.as_deref() != Some(requested.as_str()) {
                return Decision::Deny(Denied {
                    policy: selected_name.clone(),
                    reason: DenyReason::PolicyMismatch {
                        requested: requested.clone(),
                        selected: selected_name,
                    },
                    suggested_rule: suggested_rule(ctx),
                    closest: Vec::new(),
                });
            }
        }
        match selected {
            Some(policy) => policy.evaluate(ctx),
            None => Decision::Deny(Denied {
                policy: None,
                reason: DenyReason::NoPolicyMatch,
                suggested_rule: suggested_rule(ctx),
                closest: Vec::new(),
            }),
        }
    }
```

Add `requested_policy: None,` to every other `RequestContext { … }` literal listed under **Files** (including the `lib.rs` doc example, whose doctest must compile). In `crates/skimasque/src/service.rs` ~L1181 use `requested_policy: None,` for now (Task 2 wires the header).

In `crates/skimasque-cli/src/policy.rs`, add a last parameter to `evaluate`:

```rust
pub fn evaluate(
    loaded: &Loaded,
    policy_name: Option<&str>,
    application: &str,
    transport: Transport,
    destination: &str,
    identity: WorkloadIdentity,
    requested_policy: Option<&str>,
) -> anyhow::Result<Decision> {
```

and build the context with `requested_policy: requested_policy.map(str::to_owned),`. Update its doc comment: "`requested_policy` is the `X-Masque-Policy` pin; it applies only when the set selects the policy (`policy_name` is `None`)." Update every existing caller in `policy.rs` tests and in `skimasque.rs` (`policy check`, `policy explain`) to pass `None`.

In `skimasque.rs`, add to the `Why` variant (after `request`):

```rust
        /// Simulate `skimasque exec --policy NAME`: deny unless the policy
        /// selected for the identity is NAME. Local policy only.
        #[arg(long, value_name = "NAME", conflicts_with = "control_plane")]
        policy: Option<String>,
```

destructure it in the dispatch, pass `policy.as_deref()` into `run_why(&destination, &request, &source, policy.as_deref())`, and in `run_why` forward it as the new last argument of `policy::evaluate`.

Add this test to `crates/skimasque-cli/src/policy.rs` tests:

```rust
    #[test]
    fn evaluate_applies_a_pin_when_the_set_selects() {
        let identity = WorkloadIdentity {
            repository: Some("acme/widget".into()),
            git_ref: Some("refs/heads/main".into()),
            ..Default::default()
        };
        let d = evaluate(
            &loaded(), None, "terraform", Transport::Tcp,
            "api.production.example.com:443", identity.clone(), Some("staging"),
        )
        .unwrap();
        assert!(d.is_deny());
        let d = evaluate(
            &loaded(), None, "terraform", Transport::Tcp,
            "api.production.example.com:443", identity, Some("production"),
        )
        .unwrap();
        assert!(d.is_allow());
    }
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 crates/skimasque-policy/src/eval.rs crates/skimasque-policy/src/learn.rs crates/skimasque-policy/src/parse.rs crates/skimasque-policy/src/report.rs crates/skimasque/src/audit.rs crates/skimasque/src/service.rs crates/skimasque-cli/src/policy.rs crates/skimasque-cli/src/bin/skimasque.rs
git add -A crates
git commit -m "feat(policy): pin a request to a named policy (deny on mismatch)"
```

(If rustfmt reformats code outside your edits in those files, restore those hunks — keep the diff to your change.)

---

### Task 2: `X-Masque-Policy` on the gateway, in audit, and in the protocol docs

**Files:**
- Modify: `crates/skimasque/src/service.rs` (const near `APPLICATION_HEADER` ~L856, `Enforce::call` ~L1160-1235, `denial_rejection` ~L1260, tests ~L1516+)
- Modify: `crates/skimasque/src/lib.rs` (re-export list ~L56)
- Modify: `crates/skimasque/src/audit.rs` (`AuditEvent` ~L31, `from_decision`'s `common` ~L75, tests)
- Modify: `docs/protocol.md` (new section before `## Health`)

**Interfaces:**
- Consumes: `RequestContext.requested_policy`, `DenyReason::PolicyMismatch` (Task 1).
- Produces: `skimasque::POLICY_HEADER: &str = "x-masque-policy"`; `AuditEvent.requested_policy: Option<String>`.

- [ ] **Step 1: Write the failing tests** — in `service.rs` tests, add a helper and tests:

```rust
    fn pinned_udp_request(target: &str, app: &str, policy: &str) -> TunnelRequest {
        let parts = http::Request::builder()
            .header(APPLICATION_HEADER, app)
            .header(POLICY_HEADER, policy)
            .body(())
            .unwrap()
            .into_parts()
            .0;
        TunnelRequest::new(
            Protocol::ConnectUdp,
            Destination::Udp(Target::parse(target).unwrap()),
            "203.0.113.1:9000".parse().unwrap(),
            parts,
        )
    }

    #[tokio::test]
    async fn a_matching_pin_is_allowed_and_audited_with_the_requested_policy() {
        let sink = Arc::new(RecordingSink::default());
        let mut service = PolicyLayer::new(policy_set(ALLOW_TF))
            .with_audit(sink.clone())
            .layer(Spy::default());
        let _ = service
            .call(pinned_udp_request("api.production.example.com:443", "terraform", "prod"))
            .await;
        let events = sink.0.lock().unwrap();
        assert_eq!(events[0].decision, "allow");
        assert_eq!(events[0].requested_policy.as_deref(), Some("prod"));
    }

    #[tokio::test]
    async fn a_mismatched_pin_is_denied_403_and_audited() {
        let sink = Arc::new(RecordingSink::default());
        let spy = Spy::default();
        let mut service = PolicyLayer::new(policy_set(ALLOW_TF))
            .with_audit(sink.clone())
            .layer(spy.clone());
        let rejection = service
            .call(pinned_udp_request("api.production.example.com:443", "terraform", "staging"))
            .await
            .unwrap_err();
        assert_eq!(rejection.status(), StatusCode::FORBIDDEN);
        assert!(spy.0.lock().unwrap().is_none(), "inner must not be called");
        let events = sink.0.lock().unwrap();
        assert_eq!(events[0].decision, "deny");
        assert_eq!(events[0].requested_policy.as_deref(), Some("staging"));
        assert_eq!(
            events[0].reason.as_deref(),
            Some(r#"Policy "staging" does not apply to this identity; "prod" does."#)
        );
    }

    #[test]
    fn a_mismatch_rejection_carries_no_suggested_rule() {
        let denied = skimasque_policy::Denied {
            policy: Some("prod".into()),
            reason: skimasque_policy::DenyReason::PolicyMismatch {
                requested: "staging".into(),
                selected: Some("prod".into()),
            },
            suggested_rule: "allow terraform x:1".into(),
            closest: Vec::new(),
        };
        let (_, _, detail, _) = denial_rejection(&denied).into_parts();
        assert_eq!(detail, r#"Policy "staging" does not apply to this identity; "prod" does."#);
    }
```

(If `Rejection::into_parts` is not visible from the test module, it is in the same crate; use whatever accessor `server.rs::rejection_response` uses. `detail` may be a `Cow<str>` — compare with `.as_ref()` if needed.)

In `audit.rs` tests add:

```rust
    #[test]
    fn requested_policy_is_omitted_when_absent_and_serialised_when_set() {
        let d = decision(ALLOW_TF, "terraform", "api.production.example.com:443");
        let mut event = AuditEvent::from_decision(
            &d, "connect-tcp", "terraform", "api.production.example.com:443",
            "127.0.0.1:1", &WorkloadIdentity::default(),
        );
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("requested_policy"), "{json}");
        event.requested_policy = Some("prod".into());
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""requested_policy":"prod""#), "{json}");
    }
```

(Use the module's existing `ALLOW_TF` constant; if its destination differs, use one it allows.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p skimasque`
Expected: compile errors — no `POLICY_HEADER`, no field `requested_policy` on `AuditEvent`.

- [ ] **Step 3: Implement**

`service.rs`, beside `APPLICATION_HEADER`:

```rust
/// The header a client pins a policy with. The gateway denies the tunnel
/// unless the policy selected for the client's identity has this name; it
/// never selects a policy by it. Part of the open tunnel protocol.
pub const POLICY_HEADER: &str = "x-masque-policy";
```

Re-export it from `lib.rs` next to `APPLICATION_HEADER`.

In `Enforce::call`, after reading `application`:

```rust
        let requested_policy = request
            .headers()
            .get(POLICY_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
```

set `requested_policy: requested_policy.clone(),` in the `RequestContext` literal, and replace the audit block with:

```rust
        if let Some(sink) = &self.audit {
            let mut event = AuditEvent::from_decision(
                &decision,
                request.protocol().upgrade_token(),
                application.as_str(),
                target.to_string(),
                request.client_addr().to_string(),
                &ctx.workload,
            );
            event.requested_policy = ctx.requested_policy.clone();
            sink.record(&event);
        }
```

`denial_rejection`:

```rust
fn denial_rejection(denied: &skimasque_policy::Denied) -> Rejection {
    // A pin mismatch is not fixed by adding a rule, so it carries no suggestion.
    let detail = match denied.reason {
        skimasque_policy::DenyReason::PolicyMismatch { .. } => denied.reason.summary(),
        _ => format!("{} suggested rule: {}", denied.reason.summary(), denied.suggested_rule),
    };
    Rejection::new(StatusCode::FORBIDDEN, detail).with_proxy_error("destination_prohibited")
}
```

`audit.rs` — add after `suggested_rule`:

```rust
    /// The policy the client pinned with `x-masque-policy`, if it sent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_policy: Option<String>,
```

and `requested_policy: None,` in `from_decision`'s `common` closure.

`docs/protocol.md` — insert before `## Health`:

```markdown
## Tunnel request headers

A client opening a tunnel (`CONNECT` for TCP, extended `CONNECT` with
`:protocol connect-udp` for UDP) may send, besides `Proxy-Authorization`:

| Header | Meaning |
|---|---|
| `X-Masque-Application: <name>` | WHAT — the application the client declares. Policy rules match on it. Session context, not an authenticated fact. |
| `X-Masque-Policy: <name>` | A pin. The gateway selects the policy for the client's identity exactly as it would without the header; if the selected policy is not `<name>` the tunnel is refused `403` with `Proxy-Status: …; error=destination_prohibited; details="Policy \"<name>\" does not apply to this identity; …"`. The header never selects a policy, so it can only narrow access. Audit events carry it as `requested_policy`. |

Both are part of the open protocol: any gateway, SkiMasque Cloud or
self-hosted, honours them the same way.
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 crates/skimasque/src/service.rs crates/skimasque/src/audit.rs crates/skimasque/src/lib.rs
git add crates/skimasque docs/protocol.md
git commit -m "feat(gateway): enforce and audit the X-Masque-Policy pin"
```

---

### Task 3: `peek_identity` for platform credentials

**Files:**
- Modify: `crates/skimasque-identity/src/credential.rs` (next to `peek_org_id` ~L351, tests)
- Modify: `crates/skimasque-identity/src/lib.rs` (re-export ~L44)

**Interfaces:**
- Produces: `skimasque_identity::peek_identity(token: &str) -> Result<WorkloadIdentity, Error>`.

- [ ] **Step 1: Write the failing tests** — in `credential.rs` tests:

```rust
    #[test]
    fn peek_identity_reads_the_claims_without_a_key() {
        let issuer = CredentialIssuer::generate(Duration::from_secs(900));
        let issued = issuer.issue(&identity(), Some("sub")).unwrap();
        assert_eq!(peek_identity(&issued.token).unwrap(), identity());
    }

    #[test]
    fn peek_identity_rejects_something_that_is_not_a_jwt() {
        assert!(matches!(peek_identity("not-a-token"), Err(Error::Malformed(_))));
        assert!(matches!(peek_identity("a.%%%.c"), Err(Error::Malformed(_))));
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p skimasque-identity peek_identity`
Expected: compile error — `peek_identity` not found.

- [ ] **Step 3: Implement** — below `peek_org_id`:

```rust
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
```

Add `peek_identity` to the `pub use credential::{…}` list in `lib.rs`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p skimasque-identity` then `cargo clippy --workspace --all-targets -- -D warnings`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 crates/skimasque-identity/src/credential.rs crates/skimasque-identity/src/lib.rs
git add crates/skimasque-identity
git commit -m "feat(identity): peek a platform credential's identity for display"
```

---

### Task 4: Shared `session` module (move out of `skimasque-client`)

**Files:**
- Create: `crates/skimasque-cli/src/session.rs`
- Modify: `crates/skimasque-cli/src/lib.rs` (add `pub mod session;`)
- Modify: `crates/skimasque-cli/src/bin/skimasque-client.rs` (L50-560: `ConnectionArgs`, `Connected`, `proxy_with_port`, `resolve_proxy`, `open_session`, `Bearer`, `AuthMode`, `auth_mode`, `resolve_bearer`, `OidcExchange`, `SessionExchange`, `RefreshMode`, `REFRESH_RETRY`, `refresh_lead_time`, `refresh_credential`; tests at L686+)

**Interfaces:**
- Produces (all in `skimasque_cli::session`):
  - `#[derive(Debug, Clone, clap::Args)] pub struct TlsArgs { pub authority: Option<String>, pub template: Option<String>, pub ca: Option<PathBuf>, pub insecure: bool }` — same flags, docs and attributes as today.
  - `#[derive(Debug, Clone, clap::Args)] pub struct AuthArgs { pub auth_token: Option<String>, pub github_oidc: bool, pub oidc_token: Option<String>, pub oidc_audience: Option<String>, pub org: Option<String> }` — same flags, env vars, `conflicts_with` and docs as today.
  - `pub struct Login { pub creds: account::Credentials, pub org: String }`
  - `pub struct Connected { pub session: Session, pub refresh: Option<(RefreshMode, Duration)>, pub credential: Option<String>, pub note: Option<String>, pub login: Option<Login> }`
  - `pub async fn open_session(gateway: &str, tls: &TlsArgs, auth: &AuthArgs, headers: http::HeaderMap) -> anyhow::Result<Connected>`
  - `pub async fn refresh_credential(session: Arc<Session>, exchange: RefreshMode, ttl: Duration, announce: bool)`
  - `pub enum RefreshMode` (variants private-field structs as today), `pub fn refresh_lead_time(ttl: Duration) -> Duration`
  - `pub fn refusal_line(destination: &str, error: &skimasque::Error) -> String`

- [ ] **Step 1: Write the failing tests** — create `session.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct Harness {
        #[command(flatten)]
        tls: TlsArgs,
        #[command(flatten)]
        auth: AuthArgs,
    }

    fn auth(extra: &[&str]) -> AuthArgs {
        let mut argv = vec!["x"];
        argv.extend_from_slice(extra);
        Harness::try_parse_from(argv).unwrap().auth
    }

    #[test]
    fn the_harness_is_a_consistent_command() {
        use clap::CommandFactory;
        Harness::command().debug_assert();
    }

    #[test]
    fn auth_mode_picks_oidc_over_everything_else() {
        let a = auth(&["--github-oidc", "--oidc-audience", "https://gw.example"]);
        assert_eq!(auth_mode(&a, true), AuthMode::Oidc);
        assert_eq!(auth_mode(&a, false), AuthMode::Oidc);
    }

    #[test]
    fn auth_mode_picks_static_token_when_no_oidc_flag_is_given() {
        let a = auth(&["--auth-token", "secret"]);
        assert_eq!(auth_mode(&a, true), AuthMode::Static);
        assert_eq!(auth_mode(&a, false), AuthMode::Static);
    }

    #[test]
    fn auth_mode_falls_back_to_a_stored_session_when_nothing_else_is_given() {
        assert_eq!(auth_mode(&auth(&[]), true), AuthMode::Session);
    }

    #[test]
    fn auth_mode_is_none_with_no_flags_and_no_session() {
        assert_eq!(auth_mode(&auth(&[]), false), AuthMode::None);
    }

    #[test]
    fn the_refresh_lead_time_stays_within_bounds() {
        assert_eq!(refresh_lead_time(Duration::from_secs(3600)), Duration::from_secs(900));
        assert_eq!(refresh_lead_time(Duration::from_secs(8)), Duration::from_secs(10));
        assert_eq!(
            refresh_lead_time(Duration::from_secs(24 * 3600)),
            Duration::from_secs(15 * 60)
        );
    }

    #[test]
    fn the_proxy_gets_a_default_port_of_443() {
        assert_eq!(proxy_with_port("gateway.skimasque.com"), "gateway.skimasque.com:443");
        assert_eq!(proxy_with_port("gateway.skimasque.com:8443"), "gateway.skimasque.com:8443");
        assert_eq!(proxy_with_port("10.0.0.1"), "10.0.0.1:443");
        assert_eq!(proxy_with_port("10.0.0.1:4433"), "10.0.0.1:4433");
        assert_eq!(proxy_with_port("::1"), "[::1]:443");
        assert_eq!(proxy_with_port("[::1]"), "[::1]:443");
        assert_eq!(proxy_with_port("[::1]:4433"), "[::1]:4433");
        assert_eq!(proxy_with_port("2001:db8::1"), "[2001:db8::1]:443");
    }

    fn rejected(status: u16, proxy_status: Option<&str>) -> skimasque::Error {
        skimasque::Error::Rejected {
            status: http::StatusCode::from_u16(status).unwrap(),
            proxy_status: proxy_status.map(str::to_owned),
        }
    }

    #[test]
    fn a_policy_denial_reads_as_reason_and_fix() {
        let e = rejected(
            403,
            Some(r#"gw; error=destination_prohibited; details="No matching allow rule. suggested rule: allow psql db.prod:5432""#),
        );
        assert_eq!(
            refusal_line("db.prod:5432", &e),
            "denied db.prod:5432: No matching allow rule. To allow it: allow psql db.prod:5432"
        );
    }

    #[test]
    fn a_pin_mismatch_denial_has_no_fix_and_unescapes_quotes() {
        let e = rejected(
            403,
            Some(r#"gw; error=destination_prohibited; details="Policy \"staging\" does not apply to this identity; \"prod\" does.""#),
        );
        assert_eq!(
            refusal_line("db.prod:5432", &e),
            r#"denied db.prod:5432: Policy "staging" does not apply to this identity; "prod" does."#
        );
    }

    #[test]
    fn other_failures_say_the_destination_could_not_be_reached() {
        let e = rejected(502, Some(r#"gw; error=dns_error; details="nope""#));
        assert!(refusal_line("x:1", &e).starts_with("could not reach x:1: "));
        assert_eq!(refusal_line("x:1", &rejected(403, None)), "denied x:1");
    }
}
```

and add `pub mod session;` to `lib.rs`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p skimasque-cli --lib session`
Expected: compile errors — `TlsArgs`, `AuthArgs`, `auth_mode`, `refusal_line`, … not found.

- [ ] **Step 3: Implement** — move, don't rewrite:

1. Module doc for `session.rs`:
   ```rust
   //! Opening a gateway session from command-line flags — TLS trust, the bearer
   //! credential, and keeping a short-lived credential fresh. Shared by
   //! `skimasque-client` and `skimasque exec`.
   ```
2. Split today's `ConnectionArgs` fields: `authority`, `template`, `ca`, `insecure` → `TlsArgs`; `auth_token`, `github_oidc`, `oidc_token`, `oidc_audience`, `org` → `AuthArgs`. Keep every doc comment and `#[arg(...)]` exactly. Make fields `pub`.
3. Move verbatim (make `pub` only what **Interfaces** lists; keep the rest private): `Connected`, `proxy_with_port`, `resolve_proxy`, `Bearer`, `AuthMode`, `auth_mode`, `resolve_bearer`, `OidcExchange`, `SessionExchange`, `RefreshMode`, `REFRESH_RETRY`, `refresh_lead_time`, `refresh_credential`. Replace `skimasque_cli::` paths with `crate::`.
4. Change `auth_mode(args: &AuthArgs, has_session: bool)` and `resolve_bearer(auth: &AuthArgs, session: &Session)`.
5. `Bearer` gains `note: Option<String>` and `login: Option<Login>`. In `resolve_bearer`, replace each `eprintln!` with setting `note` to the same text (`format!("exchanged an OIDC token for a platform credential (valid {}s)", …)` and `format!("minted a platform credential from your skimasque login session (valid {}s)", …)`). In the `Session` arm set `login: Some(Login { creds: creds.clone(), org: org.clone() })` before building `SessionExchange` (derive/implement `Clone` where needed; `account::Credentials` is already `Clone`).
6. New `open_session`:
   ```rust
   /// Connect to `gateway` (`host[:port]`, port defaulting to 443), resolve the
   /// bearer credential, and return a session that sends it — plus `headers`
   /// (application, policy pin) — on every tunnel.
   pub async fn open_session(
       gateway: &str,
       tls: &TlsArgs,
       auth: &AuthArgs,
       mut headers: HeaderMap,
   ) -> anyhow::Result<Connected> {
   ```
   Body: today's `open_session` with `args.proxy` → `gateway`, `args.ca/insecure/authority/template` → `tls.…`, `resolve_bearer(auth, &session)`; delete the `--app` header code (callers now pass it in `headers`); insert `PROXY_AUTHORIZATION` into `headers` when there is a bearer; always call `session.with_default_headers(headers)` when `headers` is non-empty. Return `Connected { session, refresh: bearer.refresh, credential: bearer.header, note: bearer.note, login: bearer.login }`. Keep the `--insecure` warning `eprintln!`.
7. `refresh_credential(session, exchange, ttl, announce: bool)`: print the "refreshed the platform credential" line only when `announce`; failures always print.
8. Add `refusal_line`:
   ```rust
   /// One line for stderr when the gateway refuses a tunnel: a policy denial
   /// reads `denied db.prod:5432: No matching allow rule. To allow it: allow
   /// psql db.prod:5432`; anything else `could not reach <dest>: <error>`.
   pub fn refusal_line(destination: &str, error: &skimasque::Error) -> String {
       if let skimasque::Error::Rejected { status, proxy_status } = error {
           if *status == http::StatusCode::FORBIDDEN {
               return match proxy_status.as_deref().and_then(proxy_status_details) {
                   Some(details) => match details.split_once(" suggested rule: ") {
                       Some((reason, rule)) => {
                           format!("denied {destination}: {reason} To allow it: {rule}")
                       }
                       None => format!("denied {destination}: {details}"),
                   },
                   None => format!("denied {destination}"),
               };
           }
       }
       format!("could not reach {destination}: {error}")
   }

   /// The `details` string parameter of a `Proxy-Status` value (RFC 9209),
   /// with Structured Fields escapes (`\"`, `\\`) undone.
   fn proxy_status_details(value: &str) -> Option<String> {
       let start = value.find("details=\"")? + "details=\"".len();
       let mut out = String::new();
       let mut chars = value[start..].chars();
       while let Some(c) = chars.next() {
           match c {
               '\\' => out.push(chars.next()?),
               '"' => return Some(out),
               c => out.push(c),
           }
       }
       None
   }
   ```
9. In `skimasque-client.rs`: keep `ConnectionArgs` as
   ```rust
   #[derive(Debug, ClapArgs)]
   struct ConnectionArgs {
       /// (keep today's `--proxy` doc comment)
       #[arg(long, value_name = "HOST[:PORT]", default_value = "gateway.skimasque.com")]
       proxy: String,
       #[command(flatten)]
       tls: TlsArgs,
       #[command(flatten)]
       auth: AuthArgs,
       /// (keep today's `--app` doc comment)
       #[arg(long, value_name = "NAME")]
       app: Option<String>,
   }
   ```
   `main` builds the headers (application header from `--app`, with today's error text), calls `session::open_session(&args.connection.proxy, &args.connection.tls, &args.connection.auth, headers)`, prints `connected.note` with `eprintln!` if set (preserving today's output), then spawns `session::refresh_credential(session.clone(), exchange, ttl, true)`. Delete the moved code and its moved tests (`the_refresh_lead_time_stays_within_bounds`, `the_proxy_gets_a_default_port_of_443`, the four `auth_mode_*` tests, `connection_args`). Keep `the_command_definition_is_consistent` and the probe tests.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p skimasque-cli` then `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo run -q -p skimasque-cli --bin skimasque-client -- --help` and confirm the same flags as before (`--proxy --authority --template --ca --insecure --auth-token --github-oidc --oidc-token --oidc-audience --org --app -v`).
Expected: pass; help lists every flag.

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 crates/skimasque-cli/src/session.rs crates/skimasque-cli/src/lib.rs crates/skimasque-cli/src/bin/skimasque-client.rs
git add crates/skimasque-cli
git commit -m "refactor(cli): share gateway session setup between skimasque-client and exec"
```

---

### Task 5: HTTP CONNECT front end

**Files:**
- Create: `crates/skimasque-cli/src/http_connect.rs`
- Modify: `crates/skimasque-cli/src/lib.rs` (`pub mod http_connect;`)
- Create: `crates/skimasque-cli/tests/http_connect.rs`

**Interfaces:**
- Consumes: `session::refusal_line` (Task 4).
- Produces: `pub async fn serve(listener: TcpListener, session: Arc<Session>) -> std::io::Result<()>`; `pub const MAX_HEAD: usize = 8 * 1024;`; `pub fn parse_head(head: &[u8]) -> Head`; `#[derive(Debug, PartialEq, Eq)] pub enum Head { Connect(String), OtherMethod, Malformed }`.

- [ ] **Step 1: Write the failing tests**

Unit tests at the bottom of `http_connect.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connect_head_yields_its_authority() {
        assert_eq!(
            parse_head(b"CONNECT db.prod:5432 HTTP/1.1\r\nHost: db.prod:5432\r\n\r\n"),
            Head::Connect("db.prod:5432".into())
        );
        assert_eq!(
            parse_head(b"CONNECT [fd00::5]:443 HTTP/1.0\r\n\r\n"),
            Head::Connect("[fd00::5]:443".into())
        );
    }

    #[test]
    fn other_methods_and_garbage_are_told_apart() {
        assert_eq!(parse_head(b"GET http://x/ HTTP/1.1\r\n\r\n"), Head::OtherMethod);
        assert_eq!(parse_head(b"CONNECT x:1\r\n\r\n"), Head::Malformed);
        assert_eq!(parse_head(b"CONNECT x:1 SPDY/3\r\n\r\n"), Head::Malformed);
        assert_eq!(parse_head(b"CONNECT  HTTP/1.1\r\n\r\n"), Head::Malformed);
        assert_eq!(parse_head(&[0xff, 0xfe]), Head::Malformed);
    }
}
```

Integration test `tests/http_connect.rs` (gateway harness modelled on `tests/socks5_bridge.rs` and `crates/skimasque/tests/tcp_roundtrip.rs`):

```rust
//! End-to-end tests of the HTTP CONNECT front end: raw HTTP/1.1 bytes, the
//! front end, a real MASQUE gateway with a policy, and a real TCP echo target.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use skimasque::client::Client;
use skimasque::policy::AddressPolicy;
use skimasque::server::{ProxyConfig, Server};
use skimasque::service::{Dispatch, PolicyLayer, TcpProxy};
use skimasque::tls;
use skimasque_cli::http_connect;
use skimasque_core::UriTemplate;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tower::ServiceBuilder;

const T: Duration = Duration::from_secs(10);

async fn spawn_echo(tag: &'static [u8]) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 {
                        return;
                    }
                    let mut reply = tag.to_vec();
                    reply.extend_from_slice(&buf[..n]);
                    if s.write_all(&reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

/// A gateway allowing `curl` to `allowed` only, and the front end before it.
async fn spawn_front(allowed: SocketAddr) -> SocketAddr {
    let policy = format!(
        "name = \"t\"\n[[rules]]\napplication = \"curl\"\naction = \"allow\"\ndestinations = [\"127.0.0.1:{}\"]\n",
        allowed.port()
    );
    let set = skimasque::policy_engine::PolicySet::from_documents([("p.toml", policy.as_str())])
        .unwrap();
    let service = ServiceBuilder::new()
        .layer(PolicyLayer::new(set))
        .service(Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive())));
    let generated = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();
    let server_tls = tls::server_config_from_pem(
        generated.certificate_pem.as_bytes(),
        generated.key_pem.as_bytes(),
    )
    .unwrap();
    let server = Server::bind(
        "127.0.0.1:0".parse().unwrap(),
        server_tls,
        service,
        ProxyConfig::new("localhost").unwrap(),
    )
    .unwrap();
    let gw = server.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.run().await;
    });

    let client_tls = tls::client_config_with_ca(generated.certificate_pem.as_bytes()).unwrap();
    let template = UriTemplate::default_connect_udp(&format!("localhost:{}", gw.port())).unwrap();
    let mut headers = http::HeaderMap::new();
    headers.insert(skimasque::APPLICATION_HEADER, http::HeaderValue::from_static("curl"));
    let session = Client::new(client_tls)
        .unwrap()
        .connect(gw, template)
        .await
        .unwrap()
        .with_default_headers(headers);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let front = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = http_connect::serve(listener, Arc::new(session)).await;
    });
    front
}

async fn read_head(s: &mut TcpStream) -> String {
    let mut got = Vec::new();
    let mut byte = [0u8; 1];
    while !got.ends_with(b"\r\n\r\n") {
        timeout(T, s.read_exact(&mut byte)).await.expect("timed out").unwrap();
        got.push(byte[0]);
    }
    String::from_utf8(got).unwrap()
}

#[tokio::test]
async fn an_allowed_connect_is_established_and_carries_bytes() {
    let echo = spawn_echo(b"ok:").await;
    let front = spawn_front(echo).await;
    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(format!("CONNECT {echo} HTTP/1.1\r\nHost: {echo}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    assert!(read_head(&mut s).await.starts_with("HTTP/1.1 200 "));
    s.write_all(b"hi").await.unwrap();
    let mut reply = [0u8; 5];
    timeout(T, s.read_exact(&mut reply)).await.unwrap().unwrap();
    assert_eq!(&reply, b"ok:hi");
}

#[tokio::test]
async fn bytes_sent_with_the_connect_head_reach_the_destination() {
    let echo = spawn_echo(b"ok:").await;
    let front = spawn_front(echo).await;
    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(format!("CONNECT {echo} HTTP/1.1\r\n\r\nearly").as_bytes())
        .await
        .unwrap();
    assert!(read_head(&mut s).await.starts_with("HTTP/1.1 200 "));
    let mut reply = [0u8; 8];
    timeout(T, s.read_exact(&mut reply)).await.unwrap().unwrap();
    assert_eq!(&reply, b"ok:early");
}

#[tokio::test]
async fn a_policy_denial_answers_403_with_the_reason() {
    let echo = spawn_echo(b"ok:").await;
    let other = spawn_echo(b"no:").await;
    let front = spawn_front(echo).await;
    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(format!("CONNECT {other} HTTP/1.1\r\n\r\n").as_bytes()).await.unwrap();
    let head = read_head(&mut s).await;
    assert!(head.starts_with("HTTP/1.1 403 "), "{head}");
}

#[tokio::test]
async fn other_methods_get_405_and_garbage_gets_400() {
    let echo = spawn_echo(b"ok:").await;
    let front = spawn_front(echo).await;
    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(b"GET http://example.com/ HTTP/1.1\r\n\r\n").await.unwrap();
    assert!(read_head(&mut s).await.starts_with("HTTP/1.1 405 "));

    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(b"CONNECT nonsense HTTP/1.1\r\n\r\n").await.unwrap();
    assert!(read_head(&mut s).await.starts_with("HTTP/1.1 400 "));

    let mut s = TcpStream::connect(front).await.unwrap();
    s.write_all(&vec![b'a'; http_connect::MAX_HEAD + 1]).await.unwrap();
    assert!(read_head(&mut s).await.starts_with("HTTP/1.1 400 "));
}
```

(If `skimasque::policy_engine` is not the re-export name in this crate, use the one `crates/skimasque/tests/tcp_roundtrip.rs` uses. Add `tower = { workspace = true, features = ["util"] }` to `[dev-dependencies]` only if the build says it is missing — it is already a normal dependency of `skimasque-cli`.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p skimasque-cli --test http_connect`
Expected: compile error — module `http_connect` not found.

- [ ] **Step 3: Implement** `crates/skimasque-cli/src/http_connect.rs`:

```rust
//! A minimal HTTP/1.1 `CONNECT` proxy in front of the gateway session.
//!
//! `HTTPS_PROXY=http://…` is understood by far more software than
//! `socks5h://` (Go, Python, Node, curl, git), so `skimasque exec` points those
//! variables here. Each `CONNECT host:port` becomes a TCP tunnel through the
//! gateway; the name is resolved there, not here. Nothing else is proxied:
//! plain-HTTP requests are refused with `405`.

use std::sync::Arc;

use skimasque::client::Session;
use skimasque_core::target::Target;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::debug;

use crate::session::refusal_line;

/// The largest request head accepted, in bytes.
pub const MAX_HEAD: usize = 8 * 1024;

/// What a request head asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Head {
    /// `CONNECT <authority> HTTP/1.x`.
    Connect(String),
    /// A well-formed request with another method.
    OtherMethod,
    /// Anything that is not an HTTP/1.x request line.
    Malformed,
}

/// Classify a request head (the bytes up to and including the blank line).
pub fn parse_head(head: &[u8]) -> Head {
    let Ok(text) = std::str::from_utf8(head) else {
        return Head::Malformed;
    };
    let line = text.split("\r\n").next().unwrap_or_default();
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Head::Malformed;
    };
    if !version.starts_with("HTTP/1.") || target.is_empty() {
        return Head::Malformed;
    }
    if method != "CONNECT" {
        return Head::OtherMethod;
    }
    Head::Connect(target.to_owned())
}

/// Accept connections on `listener` until it fails, handling each on its own
/// task.
pub async fn serve(listener: TcpListener, session: Arc<Session>) -> std::io::Result<()> {
    loop {
        let (stream, client) = listener.accept().await?;
        let session = session.clone();
        tokio::spawn(async move {
            if let Err(error) = handle(stream, session).await {
                debug!(%client, %error, "HTTP CONNECT connection ended with an error");
            }
        });
    }
}

async fn handle(mut stream: TcpStream, session: Arc<Session>) -> anyhow::Result<()> {
    let Some((head, early)) = read_head(&mut stream).await? else {
        return respond(&mut stream, "400 Bad Request").await;
    };
    let authority = match parse_head(&head) {
        Head::Connect(authority) => authority,
        Head::OtherMethod => return respond(&mut stream, "405 Method Not Allowed\r\nAllow: CONNECT").await,
        Head::Malformed => return respond(&mut stream, "400 Bad Request").await,
    };
    let Ok(target) = Target::parse(&authority) else {
        return respond(&mut stream, "400 Bad Request").await;
    };
    let mut tunnel = match session.connect_tcp(target).await {
        Ok(tunnel) => tunnel,
        Err(error) => {
            eprintln!("skimasque: {}", refusal_line(&authority, &error));
            let forbidden = matches!(
                &error,
                skimasque::Error::Rejected { status, .. } if *status == http::StatusCode::FORBIDDEN
            );
            let status = if forbidden { "403 Forbidden" } else { "502 Bad Gateway" };
            return respond(&mut stream, status).await;
        }
    };
    stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    if !early.is_empty() {
        tunnel.write(&early).await?;
    }
    tunnel.relay(stream).await?;
    Ok(())
}

/// Read up to the end of the request head. Returns the head and any bytes the
/// client sent after it, or `None` if the head is larger than [`MAX_HEAD`] or
/// the client closed first.
async fn read_head(stream: &mut TcpStream) -> std::io::Result<Option<(Vec<u8>, Vec<u8>)>> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let early = buf.split_off(end + 4);
            return Ok(Some((buf, early)));
        }
        if buf.len() > MAX_HEAD {
            return Ok(None);
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

async fn respond(stream: &mut TcpStream, status: &str) -> anyhow::Result<()> {
    stream
        .write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes())
        .await?;
    Ok(())
}
```

(Use whichever `Target` path `socks5.rs` imports — it uses `skimasque_core::connect_udp::Target`; both name the same type.) Add `pub mod http_connect;` to `lib.rs`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p skimasque-cli` then `cargo clippy --workspace --all-targets -- -D warnings`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 crates/skimasque-cli/src/http_connect.rs crates/skimasque-cli/src/lib.rs crates/skimasque-cli/tests/http_connect.rs
git add crates/skimasque-cli
git commit -m "feat(cli): HTTP CONNECT front end over the gateway session"
```

---

### Task 6: `--forward` specs and listeners, and denial lines from SOCKS5

**Files:**
- Create: `crates/skimasque-cli/src/forward.rs`
- Modify: `crates/skimasque-cli/src/lib.rs` (`pub mod forward;`)
- Modify: `crates/skimasque-cli/src/socks5.rs` (the CONNECT refusal branch ~L270)
- Test: unit tests in `forward.rs`; add a forward case to `crates/skimasque-cli/tests/http_connect.rs`

**Interfaces:**
- Consumes: `session::refusal_line` (Task 4).
- Produces: `#[derive(Debug, Clone, PartialEq, Eq)] pub struct ForwardSpec { pub local_port: u16, pub target: Target }` (`local_port == 0` means ephemeral); `ForwardSpec::parse(s: &str) -> Result<ForwardSpec, String>` (clap `value_parser` compatible); `ForwardSpec::env_name(&self) -> String`; `pub async fn bind(spec: &ForwardSpec) -> std::io::Result<TcpListener>`; `pub async fn serve(listener: TcpListener, target: Target, session: Arc<Session>) -> std::io::Result<()>`.

- [ ] **Step 1: Write the failing tests** — `forward.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_destination_gets_an_ephemeral_port() {
        let f = ForwardSpec::parse("db.prod:5432").unwrap();
        assert_eq!(f.local_port, 0);
        assert_eq!(f.target.to_string(), "db.prod:5432");
    }

    #[test]
    fn a_leading_port_is_the_local_one() {
        let f = ForwardSpec::parse("15432:db.prod:5432").unwrap();
        assert_eq!(f.local_port, 15432);
        assert_eq!(f.target.to_string(), "db.prod:5432");
    }

    #[test]
    fn ipv6_destinations_are_bracketed() {
        let f = ForwardSpec::parse("15432:[fd00::5]:5432").unwrap();
        assert_eq!(f.local_port, 15432);
        assert_eq!(f.target.to_string(), "[fd00::5]:5432");
        assert_eq!(ForwardSpec::parse("[fd00::5]:5432").unwrap().local_port, 0);
    }

    #[test]
    fn bad_specs_are_refused_with_a_reason() {
        for bad in ["db.prod", "70000:db.prod:5432", "db.prod:notaport", "", "15432:"] {
            assert!(ForwardSpec::parse(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn env_names_are_upper_snake_case() {
        assert_eq!(
            ForwardSpec::parse("db.prod:5432").unwrap().env_name(),
            "SKIMASQUE_FORWARD_DB_PROD_5432"
        );
        assert_eq!(
            ForwardSpec::parse("[fd00::5]:5432").unwrap().env_name(),
            "SKIMASQUE_FORWARD_FD00__5_5432"
        );
        assert_eq!(
            ForwardSpec::parse("my-db.internal:6379").unwrap().env_name(),
            "SKIMASQUE_FORWARD_MY_DB_INTERNAL_6379"
        );
    }
}
```

Append to `tests/http_connect.rs` (reusing its harness; `spawn_front` must additionally return the session — change it to return `(SocketAddr, Arc<Session>)` and update the four existing callers to `let (front, _) = …`):

```rust
#[tokio::test]
async fn a_forward_listener_splices_to_its_destination() {
    use skimasque_cli::forward::{self, ForwardSpec};
    let echo = spawn_echo(b"fw:").await;
    let (_, session) = spawn_front(echo).await;
    let spec = ForwardSpec::parse(&echo.to_string()).unwrap();
    let listener = forward::bind(&spec).await.unwrap();
    let local = listener.local_addr().unwrap();
    assert!(local.ip().is_loopback());
    tokio::spawn(forward::serve(listener, spec.target.clone(), session));
    let mut s = TcpStream::connect(local).await.unwrap();
    s.write_all(b"hi").await.unwrap();
    let mut reply = [0u8; 5];
    timeout(T, s.read_exact(&mut reply)).await.unwrap().unwrap();
    assert_eq!(&reply, b"fw:hi");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p skimasque-cli forward`
Expected: compile error — module `forward` not found.

- [ ] **Step 3: Implement** `crates/skimasque-cli/src/forward.rs`:

```rust
//! `skimasque exec --forward [LOCAL_PORT:]HOST:PORT`: a loopback TCP listener
//! whose every connection becomes a TCP tunnel to one destination — for tools
//! such as `psql` that ignore proxy settings.

use std::net::Ipv4Addr;
use std::sync::Arc;

use skimasque::client::Session;
use skimasque_core::target::{Target, TargetHost};
use tokio::net::TcpListener;
use tracing::debug;

use crate::session::refusal_line;

/// One `--forward`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardSpec {
    /// The loopback port to listen on; `0` picks a free one.
    pub local_port: u16,
    /// Where each connection is tunnelled to.
    pub target: Target,
}

impl ForwardSpec {
    /// Parse `HOST:PORT` or `LOCAL_PORT:HOST:PORT` (IPv6 hosts bracketed).
    pub fn parse(spec: &str) -> Result<Self, String> {
        let (local_port, destination) = match spec.split_once(':') {
            Some((head, rest))
                if !head.is_empty() && head.bytes().all(|b| b.is_ascii_digit()) && rest.contains(':') =>
            {
                let port = head
                    .parse::<u16>()
                    .map_err(|_| format!("{head:?} is not a local port (1-65535)"))?;
                (port, rest)
            }
            _ => (0, spec),
        };
        let target = Target::parse(destination)
            .map_err(|e| format!("{destination:?} is not HOST:PORT ({e})"))?;
        Ok(Self { local_port, target })
    }

    /// The environment variable exec sets to this forward's local address:
    /// `SKIMASQUE_FORWARD_<HOST>_<PORT>`, upper-cased, every other byte `_`.
    pub fn env_name(&self) -> String {
        let host = match &self.target.host {
            TargetHost::Name(name) => name.clone(),
            TargetHost::Ip(ip) => ip.to_string(),
        };
        let host: String = host
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
            .collect();
        format!("SKIMASQUE_FORWARD_{host}_{}", self.target.port)
    }
}

/// Listen on `127.0.0.1:<local_port>`.
pub async fn bind(spec: &ForwardSpec) -> std::io::Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, spec.local_port)).await
}

/// Tunnel every connection on `listener` to `target` until the listener fails.
pub async fn serve(listener: TcpListener, target: Target, session: Arc<Session>) -> std::io::Result<()> {
    loop {
        let (stream, client) = listener.accept().await?;
        let session = session.clone();
        let target = target.clone();
        tokio::spawn(async move {
            match session.connect_tcp(target.clone()).await {
                Ok(tunnel) => {
                    if let Err(error) = tunnel.relay(stream).await {
                        debug!(%client, %target, %error, "forwarded connection ended with an error");
                    }
                }
                Err(error) => eprintln!("skimasque: {}", refusal_line(&target.to_string(), &error)),
            }
        });
    }
}
```

(`"15432:"` must fail: `rest` is empty so the split arm is skipped and `Target::parse("15432:")` errors. `"db.prod"` fails in `Target::parse`. `"70000:db.prod:5432"` fails the `u16` parse. If `Target::parse` accepts something the tests expect refused, add the explicit check here, not in `Target`.) Add `pub mod forward;` to `lib.rs`.

In `socks5.rs`, in the CONNECT branch where `connect_reply_for(&error)` is computed, add after it:

```rust
            if reply == REPLY_NOT_ALLOWED {
                eprintln!("skimasque: {}", crate::session::refusal_line(&requested.to_string(), &error));
            }
```

(using the destination variable that branch already logs as `requested`).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p skimasque-cli` then `cargo clippy --workspace --all-targets -- -D warnings`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 crates/skimasque-cli/src/forward.rs crates/skimasque-cli/src/socks5.rs crates/skimasque-cli/src/lib.rs crates/skimasque-cli/tests/http_connect.rs
git add crates/skimasque-cli
git commit -m "feat(cli): loopback port forwards over the gateway session"
```

---

### Task 7: `exec` — arguments and the pure decisions

**Files:**
- Create: `crates/skimasque-cli/src/exec.rs`
- Modify: `crates/skimasque-cli/src/lib.rs` (`pub mod exec;`)

**Interfaces:**
- Consumes: `session::{TlsArgs, AuthArgs}` (Task 4), `forward::ForwardSpec` (Task 6), `account::{Credentials, SimulateResult, SimLimit}`, `normalize_base_url`.
- Produces (all `pub` in `skimasque_cli::exec`):
  - `const CLOUD_CONTROL_PLANE: &str = "https://control.skimasque.com"; const CLOUD_GATEWAY: &str = "gateway.skimasque.com"; const PREFLIGHT_PLACEHOLDER: &str = "preflight.invalid:1";`
  - `const EXIT_FAILED: i32 = 125; const EXIT_NOT_EXECUTABLE: i32 = 126; const EXIT_NOT_FOUND: i32 = 127;`
  - `#[derive(Debug, clap::Args)] struct ExecArgs { policy, app, gateway, control_plane, forwards: Vec<ForwardSpec>, quiet, tls: TlsArgs, auth: AuthArgs, command: Vec<OsString> }`
  - `fn resolve_control_plane(explicit: Option<&str>, login: Option<&Credentials>) -> String`
  - `fn resolve_gateway(explicit: Option<&str>, control_plane: &str) -> Result<String, String>`
  - `fn default_app(command: &OsStr) -> String`
  - `fn child_env(http: SocketAddr, socks: SocketAddr, forwards: &[(ForwardSpec, SocketAddr)]) -> Vec<(String, String)>`
  - `fn identity_label(identity: Option<&WorkloadIdentity>) -> String`
  - `struct Checked { pub selected: Option<String>, pub allowed: Vec<bool>, pub session: Option<String> }`
  - `fn judge(pin: Option<&str>, who: &str, probes: &[(String, SimulateResult)], forwards: usize) -> Result<Checked, String>`
  - `struct AccessLine { pub destination: String, pub allowed: Option<bool>, pub local: SocketAddr }`
  - `struct HeaderView { pub identity: String, pub policy: String, pub application: String, pub gateway: String, pub access: Vec<AccessLine>, pub session: Option<String> }`
  - `fn render_header(view: &HeaderView) -> String`
  - `fn spawn_error_code(error: &std::io::Error) -> i32`, `fn status_code(status: std::process::ExitStatus) -> i32`

- [ ] **Step 1: Write the failing tests** — `exec.rs` test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{Credentials, SimLimit, SimulateResult};
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct Harness {
        #[command(flatten)]
        exec: ExecArgs,
    }

    fn creds(cp: &str) -> Credentials {
        Credentials {
            control_plane: cp.into(),
            session_token: "t".into(),
            github_login: "alice".into(),
            user_id: "usr_1".into(),
        }
    }

    fn sim(outcome: &str, policy: Option<&str>, duration: Option<&str>) -> SimulateResult {
        SimulateResult {
            against: "policy revision v1".into(),
            outcome: outcome.into(),
            policy: policy.map(str::to_owned),
            rule: None,
            steps: Vec::new(),
            suggested_rule: Some("allow psql db.prod:5432".into()),
            closest: Vec::new(),
            limits: duration
                .map(|d| vec![SimLimit { label: "duration".into(), value: d.into() }])
                .unwrap_or_default(),
        }
    }

    #[test]
    fn the_command_follows_a_double_dash() {
        let h = Harness::try_parse_from([
            "x", "--policy", "production", "--forward", "15432:db.prod:5432", "--", "psql", "-h", "127.0.0.1",
        ])
        .unwrap();
        assert_eq!(h.exec.policy.as_deref(), Some("production"));
        assert_eq!(h.exec.forwards[0].local_port, 15432);
        assert_eq!(h.exec.command, ["psql", "-h", "127.0.0.1"]);
        assert!(Harness::try_parse_from(["x", "--"]).is_err(), "a command is required");
    }

    #[test]
    fn the_control_plane_defaults_to_the_session_then_cloud() {
        assert_eq!(resolve_control_plane(None, None), CLOUD_CONTROL_PLANE);
        assert_eq!(
            resolve_control_plane(None, Some(&creds("https://cp.example/"))),
            "https://cp.example"
        );
        assert_eq!(
            resolve_control_plane(Some("cp2.example"), Some(&creds("https://cp.example"))),
            "https://cp2.example"
        );
    }

    #[test]
    fn cloud_gets_the_cloud_gateway() {
        assert_eq!(resolve_gateway(None, CLOUD_CONTROL_PLANE).unwrap(), CLOUD_GATEWAY);
        assert_eq!(resolve_gateway(Some("gw.example:8443"), CLOUD_CONTROL_PLANE).unwrap(), "gw.example:8443");
    }

    #[test]
    fn a_self_hosted_control_plane_needs_a_gateway() {
        assert_eq!(
            resolve_gateway(None, "https://cp.example").unwrap_err(),
            "No gateway is configured for https://cp.example. Pass --gateway or set SKIMASQUE_GATEWAY."
        );
        assert_eq!(resolve_gateway(Some("gw.example"), "https://cp.example").unwrap(), "gw.example");
    }

    #[test]
    fn the_app_defaults_to_the_command_stem() {
        assert_eq!(default_app(OsStr::new("terraform")), "terraform");
        assert_eq!(default_app(OsStr::new("/usr/bin/psql.exe")), "psql");
        assert_eq!(default_app(OsStr::new("./tools/kubectl")), "kubectl");
    }

    #[test]
    fn child_env_replaces_no_proxy_and_sets_both_cases() {
        let http: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let socks: SocketAddr = "127.0.0.1:4002".parse().unwrap();
        let fwd = ForwardSpec::parse("db.prod:5432").unwrap();
        let env = child_env(http, socks, &[(fwd, "127.0.0.1:15432".parse().unwrap())]);
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        for k in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
            assert_eq!(get(k), Some("http://127.0.0.1:4001"), "{k}");
        }
        for k in ["ALL_PROXY", "all_proxy"] {
            assert_eq!(get(k), Some("socks5h://127.0.0.1:4002"), "{k}");
        }
        for k in ["NO_PROXY", "no_proxy"] {
            assert_eq!(get(k), Some("localhost,127.0.0.1,::1"), "{k}");
        }
        assert_eq!(get("SKIMASQUE_PROXY_HTTP"), Some("127.0.0.1:4001"));
        assert_eq!(get("SKIMASQUE_PROXY_SOCKS5"), Some("127.0.0.1:4002"));
        assert_eq!(get("SKIMASQUE_FORWARD_DB_PROD_5432"), Some("127.0.0.1:15432"));
    }

    #[test]
    fn the_identity_label_prefers_the_actor() {
        let mut id = WorkloadIdentity { repository: Some("acme/widget".into()), ..Default::default() };
        assert_eq!(identity_label(Some(&id)), "acme/widget");
        id.actor = Some("alice".into());
        assert_eq!(identity_label(Some(&id)), "alice");
        assert_eq!(identity_label(Some(&WorkloadIdentity::default())), "unknown");
        assert_eq!(identity_label(None), "unknown");
    }

    #[test]
    fn a_pin_the_preflight_does_not_select_refuses_to_run() {
        let probes = [("db.prod:5432".to_owned(), sim("allow", Some("staging"), None))];
        assert_eq!(
            judge(Some("production"), "alice", &probes, 1).unwrap_err(),
            r#"Policy "production" does not apply to you (alice). SkiMasque selected "staging" for this identity."#
        );
        let probes = [(PREFLIGHT_PLACEHOLDER.to_owned(), sim("deny", None, None))];
        assert_eq!(
            judge(Some("production"), "alice", &probes, 0).unwrap_err(),
            r#"Policy "production" does not apply to you (alice). SkiMasque selected no policy for this identity."#
        );
    }

    #[test]
    fn every_forward_denied_refuses_to_run_and_lists_the_fixes() {
        let probes = [("db.prod:5432".to_owned(), sim("deny", Some("production"), None))];
        let e = judge(Some("production"), "alice", &probes, 1).unwrap_err();
        assert!(e.starts_with("Every destination would be denied:"), "{e}");
        assert!(e.contains("  db.prod:5432: denied. To allow it: allow psql db.prod:5432"), "{e}");
    }

    #[test]
    fn partly_denied_forwards_run_and_report_the_session() {
        let probes = [
            ("db.prod:5432".to_owned(), sim("allow", Some("production"), Some("20m"))),
            ("cache:6379".to_owned(), sim("deny", Some("production"), None)),
        ];
        let c = judge(None, "alice", &probes, 2).unwrap();
        assert_eq!(c.selected.as_deref(), Some("production"));
        assert_eq!(c.allowed, [true, false]);
        assert_eq!(c.session.as_deref(), Some("20m"));
    }

    #[test]
    fn the_placeholder_probe_only_reveals_the_policy() {
        let probes = [(PREFLIGHT_PLACEHOLDER.to_owned(), sim("deny", Some("production"), None))];
        let c = judge(Some("production"), "alice", &probes, 0).unwrap();
        assert_eq!(c.selected.as_deref(), Some("production"));
        assert!(c.allowed.is_empty());
        assert_eq!(c.session, None);
    }

    #[test]
    fn the_header_matches_the_spec_layout() {
        let view = HeaderView {
            identity: "alice".into(),
            policy: "production".into(),
            application: "terraform".into(),
            gateway: "gateway.skimasque.com".into(),
            access: vec![AccessLine {
                destination: "db.prod:5432".into(),
                allowed: Some(true),
                local: "127.0.0.1:15432".parse().unwrap(),
            }],
            session: Some("20m".into()),
        };
        assert_eq!(
            render_header(&view),
            "SkiMasque\n\n\
             Identity     alice\n\
             Policy       production\n\
             Application  terraform\n\
             Gateway      gateway.skimasque.com\n\n\
             Access\n  db.prod:5432     ✓  → 127.0.0.1:15432\n\n\
             Session\n  20m\n\n\
             Connected.\n"
        );
    }

    #[test]
    fn the_header_omits_empty_sections_and_unknown_marks() {
        let mut view = HeaderView {
            identity: "unknown".into(),
            policy: "(selected by the gateway)".into(),
            application: "curl".into(),
            gateway: "gw:443".into(),
            access: Vec::new(),
            session: None,
        };
        let s = render_header(&view);
        assert!(!s.contains("Access") && !s.contains("Session"), "{s}");
        view.access.push(AccessLine {
            destination: "db:1".into(),
            allowed: None,
            local: "127.0.0.1:2".parse().unwrap(),
        });
        assert!(render_header(&view).contains("\n  db:1             → 127.0.0.1:2\n"));
    }

    #[test]
    fn spawn_errors_map_to_126_and_127() {
        use std::io::{Error, ErrorKind};
        assert_eq!(spawn_error_code(&Error::from(ErrorKind::NotFound)), EXIT_NOT_FOUND);
        assert_eq!(spawn_error_code(&Error::from(ErrorKind::PermissionDenied)), EXIT_NOT_EXECUTABLE);
    }

    #[cfg(unix)]
    #[test]
    fn a_signal_death_is_128_plus_the_signal() {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status_code(std::process::ExitStatus::from_raw(15)), 128 + 15);
        assert_eq!(status_code(std::process::ExitStatus::from_raw(3 << 8)), 3);
    }
}
```

(`SimulateResult`/`SimLimit`/`SimStep` fields must be constructible from here: they are `pub` structs with `pub` fields in `account.rs`; if any field is private, make it `pub`.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p skimasque-cli --lib exec`
Expected: compile error — module `exec` not found.

- [ ] **Step 3: Implement** the non-test part of `exec.rs`:

```rust
//! `skimasque exec`: run a command with policy-scoped network access that ends
//! when the command does.
//!
//! One gateway session; on loopback, an HTTP CONNECT front end, the SOCKS5
//! relay and any `--forward` listeners; proxy variables in the child's
//! environment. When the child exits, the listeners and the session go with it.
//! This module holds the arguments and the decisions; [`run`] wires them up.

use std::ffi::{OsStr, OsString};
use std::net::SocketAddr;
use std::path::Path;

use skimasque_policy::WorkloadIdentity;

use crate::account::{Credentials, SimulateResult};
use crate::forward::ForwardSpec;
use crate::session::{AuthArgs, TlsArgs};

pub const CLOUD_CONTROL_PLANE: &str = "https://control.skimasque.com";
pub const CLOUD_GATEWAY: &str = "gateway.skimasque.com";
/// Simulated when there are no forwards: denied, but it names the policy the
/// identity selects.
pub const PREFLIGHT_PLACEHOLDER: &str = "preflight.invalid:1";

pub const EXIT_FAILED: i32 = 125;
pub const EXIT_NOT_EXECUTABLE: i32 = 126;
pub const EXIT_NOT_FOUND: i32 = 127;

/// `skimasque exec [OPTIONS] -- COMMAND [ARGS...]`.
#[derive(Debug, clap::Args)]
pub struct ExecArgs {
    /// Only run if this is the policy selected for your identity. Sent to the
    /// gateway as `X-Masque-Policy`, which denies every tunnel otherwise.
    #[arg(long, value_name = "NAME")]
    pub policy: Option<String>,

    /// The application to declare (`X-Masque-Application`). Defaults to the
    /// command's file name, e.g. `terraform`.
    #[arg(long, value_name = "NAME")]
    pub app: Option<String>,

    /// The gateway, as `host[:port]`. Defaults to gateway.skimasque.com when the
    /// control plane is SkiMasque Cloud; required for a self-hosted one.
    #[arg(long, env = "SKIMASQUE_GATEWAY", value_name = "HOST[:PORT]")]
    pub gateway: Option<String>,

    /// The control plane. Defaults to the one you signed in to, else SkiMasque
    /// Cloud.
    #[arg(long, env = "SKIMASQUE_CONTROL_PLANE", value_name = "URL")]
    pub control_plane: Option<String>,

    /// Listen on a loopback port and tunnel each connection to HOST:PORT, for
    /// tools that ignore proxy settings. Repeatable. Its address is also in
    /// `$SKIMASQUE_FORWARD_<HOST>_<PORT>`.
    #[arg(long = "forward", value_name = "[LOCAL_PORT:]HOST:PORT", value_parser = ForwardSpec::parse)]
    pub forwards: Vec<ForwardSpec>,

    /// Do not print the access summary.
    #[arg(long, short)]
    pub quiet: bool,

    #[command(flatten)]
    pub tls: TlsArgs,

    #[command(flatten)]
    pub auth: AuthArgs,

    /// The command to run, after `--`.
    #[arg(last = true, required = true, value_name = "COMMAND")]
    pub command: Vec<OsString>,
}

/// `--control-plane` / `$SKIMASQUE_CONTROL_PLANE`, else the signed-in
/// session's, else SkiMasque Cloud — normalised like every other base URL.
pub fn resolve_control_plane(explicit: Option<&str>, login: Option<&Credentials>) -> String {
    crate::normalize_base_url(
        explicit
            .map(str::to_owned)
            .or_else(|| login.map(|c| c.control_plane.clone()))
            .as_deref()
            .unwrap_or(CLOUD_CONTROL_PLANE),
    )
}

/// The gateway to use, or the message explaining that one is needed.
pub fn resolve_gateway(explicit: Option<&str>, control_plane: &str) -> Result<String, String> {
    match explicit {
        Some(gateway) => Ok(gateway.to_owned()),
        None if control_plane == CLOUD_CONTROL_PLANE => Ok(CLOUD_GATEWAY.to_owned()),
        None => Err(format!(
            "No gateway is configured for {control_plane}. Pass --gateway or set SKIMASQUE_GATEWAY."
        )),
    }
}

/// The command's file stem: `terraform` for `terraform`, `psql` for
/// `/usr/bin/psql.exe`.
pub fn default_app(command: &OsStr) -> String {
    Path::new(command)
        .file_stem()
        .unwrap_or(command)
        .to_string_lossy()
        .into_owned()
}

/// The variables added to the child's environment.
pub fn child_env(
    http: SocketAddr,
    socks: SocketAddr,
    forwards: &[(ForwardSpec, SocketAddr)],
) -> Vec<(String, String)> {
    let http_url = format!("http://{http}");
    let socks_url = format!("socks5h://{socks}");
    let mut env = Vec::new();
    for name in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
        env.push((name.to_owned(), http_url.clone()));
    }
    for name in ["ALL_PROXY", "all_proxy"] {
        env.push((name.to_owned(), socks_url.clone()));
    }
    // Replaced, not merged: under exec every non-loopback destination goes
    // through the tunnel, and forward listeners must not be proxied.
    for name in ["NO_PROXY", "no_proxy"] {
        env.push((name.to_owned(), "localhost,127.0.0.1,::1".to_owned()));
    }
    env.push(("SKIMASQUE_PROXY_HTTP".to_owned(), http.to_string()));
    env.push(("SKIMASQUE_PROXY_SOCKS5".to_owned(), socks.to_string()));
    for (spec, local) in forwards {
        env.push((spec.env_name(), local.to_string()));
    }
    env
}

/// Who the credential says you are: the actor, else the repository, else the
/// organisation, else `unknown`.
pub fn identity_label(identity: Option<&WorkloadIdentity>) -> String {
    identity
        .and_then(|id| {
            id.actor
                .clone()
                .or_else(|| id.repository.clone())
                .or_else(|| id.organization.clone())
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

/// What the preflight established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// The policy the control plane selected for the identity.
    pub selected: Option<String>,
    /// Per forward, in order: whether it would be allowed. Empty with no forwards.
    pub allowed: Vec<bool>,
    /// The first allowed probe's `duration` limit.
    pub session: Option<String>,
}

/// Decide from the preflight simulations whether to run. `probes` is one per
/// forward, in order — or the single [`PREFLIGHT_PLACEHOLDER`] probe when
/// `forwards` is 0.
pub fn judge(
    pin: Option<&str>,
    who: &str,
    probes: &[(String, SimulateResult)],
    forwards: usize,
) -> Result<Checked, String> {
    let selected = probes.first().and_then(|(_, r)| r.policy.clone());
    if let Some(pin) = pin {
        if selected.as_deref() != Some(pin) {
            let chose = match &selected {
                Some(s) => format!("\"{s}\""),
                None => "no policy".to_owned(),
            };
            return Err(format!(
                "Policy \"{pin}\" does not apply to you ({who}). SkiMasque selected {chose} for this identity."
            ));
        }
    }
    if forwards == 0 {
        return Ok(Checked { selected, allowed: Vec::new(), session: None });
    }
    let allowed: Vec<bool> = probes.iter().map(|(_, r)| r.outcome == "allow").collect();
    if !allowed.iter().any(|a| *a) {
        let mut message = "Every destination would be denied:".to_owned();
        for (destination, r) in probes {
            message.push_str(&format!("\n  {destination}: denied."));
            if let Some(rule) = &r.suggested_rule {
                message.push_str(&format!(" To allow it: {rule}"));
            }
        }
        return Err(message);
    }
    let session = probes
        .iter()
        .find(|(_, r)| r.outcome == "allow")
        .and_then(|(_, r)| r.limits.iter().find(|l| l.label == "duration"))
        .map(|l| l.value.clone());
    Ok(Checked { selected, allowed, session })
}

/// One `Access` line of the header.
#[derive(Debug, Clone)]
pub struct AccessLine {
    pub destination: String,
    /// `Some` only when the preflight ran.
    pub allowed: Option<bool>,
    pub local: SocketAddr,
}

/// Everything the header shows.
#[derive(Debug, Clone)]
pub struct HeaderView {
    pub identity: String,
    pub policy: String,
    pub application: String,
    pub gateway: String,
    pub access: Vec<AccessLine>,
    pub session: Option<String>,
}

/// The access summary printed to stderr before the command starts.
pub fn render_header(view: &HeaderView) -> String {
    let mut out = String::from("SkiMasque\n\n");
    for (label, value) in [
        ("Identity", &view.identity),
        ("Policy", &view.policy),
        ("Application", &view.application),
        ("Gateway", &view.gateway),
    ] {
        out.push_str(&format!("{label:<13}{value}\n"));
    }
    if !view.access.is_empty() {
        out.push_str("\nAccess\n");
        for line in &view.access {
            match line.allowed {
                Some(ok) => out.push_str(&format!(
                    "  {:<16} {}  → {}\n",
                    line.destination,
                    if ok { "✓" } else { "✗" },
                    line.local
                )),
                None => out.push_str(&format!("  {:<16} → {}\n", line.destination, line.local)),
            }
        }
    }
    if let Some(session) = &view.session {
        out.push_str(&format!("\nSession\n  {session}\n"));
    }
    out.push_str("\nConnected.\n");
    out
}

/// The exit status for a command that could not be started.
pub fn spawn_error_code(error: &std::io::Error) -> i32 {
    match error.kind() {
        std::io::ErrorKind::NotFound => EXIT_NOT_FOUND,
        _ => EXIT_NOT_EXECUTABLE,
    }
}

/// The child's exit status as exec's own: its code, or `128 + signal`.
pub fn status_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    EXIT_FAILED
}
```

Add `pub mod exec;` to `lib.rs`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p skimasque-cli --lib exec` then `cargo clippy --workspace --all-targets -- -D warnings`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 crates/skimasque-cli/src/exec.rs crates/skimasque-cli/src/lib.rs
git add crates/skimasque-cli
git commit -m "feat(cli): exec arguments, gateway resolution, env, preflight and header"
```

---

### Task 8: `exec` — running it, the `skimasque exec` subcommand, end-to-end tests

**Files:**
- Modify: `crates/skimasque-cli/src/exec.rs` (add `run`)
- Modify: `crates/skimasque-cli/Cargo.toml` (tokio feature `process`; unix-only `libc`)
- Modify: `crates/skimasque-cli/src/bin/skimasque.rs` (`Command::Exec`, dispatch, `run_exec`; module doc list of commands if it has one)
- Create: `crates/skimasque-cli/tests/exec.rs`

**Interfaces:**
- Consumes: everything from Tasks 2–7: `skimasque::{APPLICATION_HEADER, POLICY_HEADER}`, `skimasque_identity::peek_identity`, `session::{open_session, refresh_credential, Connected, Login}`, `http_connect::serve`, `socks5::serve`, `forward::{bind, serve}`, `account::{self, Api}`.
- Produces: `pub async fn run(args: ExecArgs) -> i32` in `skimasque_cli::exec`; the `skimasque exec` subcommand.

- [ ] **Step 1: Write the failing end-to-end tests** — `crates/skimasque-cli/tests/exec.rs`:

```rust
//! `skimasque exec` end to end: the real binary, a real gateway with a policy
//! and an audit sink, TCP echo targets, and this test executable re-run as the
//! child (the `child_probe` test, which does nothing unless SKM_EXEC_CHILD is
//! set).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream as StdTcp};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use skimasque::audit::{AuditEvent, AuditSink};
use skimasque::policy::AddressPolicy;
use skimasque::server::{ProxyConfig, Server};
use skimasque::service::{Dispatch, PolicyLayer, TcpProxy};
use skimasque::tls;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tower::ServiceBuilder;

#[derive(Default)]
struct Sink(Mutex<Vec<AuditEvent>>);
impl AuditSink for Sink {
    fn record(&self, event: &AuditEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

async fn spawn_echo(tag: &'static [u8]) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 {
                        return;
                    }
                    let mut reply = tag.to_vec();
                    reply.extend_from_slice(&buf[..n]);
                    if s.write_all(&reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

struct Gateway {
    addr: SocketAddr,
    ca: PathBuf,
    sink: Arc<Sink>,
    _dir: TempDir,
}

/// A throwaway directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("skm-exec-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A gateway whose policy `t` lets `curl` reach `allowed`.
async fn spawn_gateway(tag: &str, allowed: &[SocketAddr]) -> Gateway {
    let list: Vec<String> = allowed.iter().map(|a| format!("\"{a}\"")).collect();
    let policy = format!(
        "name = \"t\"\n[[rules]]\napplication = \"curl\"\naction = \"allow\"\ndestinations = [{}]\n",
        list.join(", ")
    );
    let set = skimasque::policy_engine::PolicySet::from_documents([("p.toml", policy.as_str())])
        .unwrap();
    let sink = Arc::new(Sink::default());
    let service = ServiceBuilder::new()
        .layer(PolicyLayer::new(set).with_audit(sink.clone()))
        .service(Dispatch::new().with_tcp(TcpProxy::new(AddressPolicy::permissive())));
    let generated = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();
    let server_tls = tls::server_config_from_pem(
        generated.certificate_pem.as_bytes(),
        generated.key_pem.as_bytes(),
    )
    .unwrap();
    let server = Server::bind(
        "127.0.0.1:0".parse().unwrap(),
        server_tls,
        service,
        ProxyConfig::new("localhost").unwrap(),
    )
    .unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    let dir = TempDir::new(tag);
    let ca = dir.0.join("gateway.pem");
    std::fs::write(&ca, generated.certificate_pem).unwrap();
    Gateway { addr, ca, sink, _dir: dir }
}

/// Run `skimasque exec <extra> -- <this test binary as child_probe>`.
async fn exec(gw: &Gateway, extra: &[&str], child_env: &[(&str, String)]) -> (i32, String) {
    let config = TempDir::new("config");
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_skimasque"));
    cmd.arg("exec")
        .args(["--gateway", &gw.addr.to_string()])
        .args(["--authority", &format!("localhost:{}", gw.addr.port())])
        .arg("--ca")
        .arg(&gw.ca)
        .args(["--auth-token", "test-token", "--app", "curl"])
        .args(extra)
        .arg("--")
        .arg(std::env::current_exe().unwrap())
        .args(["child_probe", "--exact", "--nocapture", "--test-threads=1"])
        .env("SKIMASQUE_CONFIG_HOME", &config.0)
        .env_remove("SKIMASQUE_GATEWAY")
        .env_remove("SKIMASQUE_CONTROL_PLANE")
        .env_remove("SKIMASQUE_TOKEN");
    for (k, v) in child_env {
        cmd.env(k, v);
    }
    let out = tokio::time::timeout(Duration::from_secs(60), cmd.output())
        .await
        .expect("exec timed out")
        .unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).into_owned())
}

/// The child: talks through `HTTPS_PROXY` and a forward, per SKM_EXEC_CHILD.
#[test]
fn child_probe() {
    let Ok(mode) = std::env::var("SKM_EXEC_CHILD") else {
        return;
    };
    let proxy = std::env::var("SKIMASQUE_PROXY_HTTP").expect("exec sets SKIMASQUE_PROXY_HTTP");
    let target = std::env::var("SKM_ECHO").unwrap();
    if let Ok(path) = std::env::var("SKM_REPORT") {
        std::fs::write(path, &proxy).unwrap();
    }
    let mut s = StdTcp::connect(&proxy).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
    }
    let head = String::from_utf8(head).unwrap();
    if mode == "denied" {
        std::process::exit(if head.starts_with("HTTP/1.1 403 ") { 9 } else { 1 });
    }
    assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
    s.write_all(b"hi").unwrap();
    let mut reply = [0u8; 5];
    s.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"ok:hi");

    let fwd_var = std::env::var("SKM_FORWARD_VAR").unwrap();
    let fwd = std::env::var(&fwd_var).unwrap_or_else(|_| panic!("{fwd_var} is set"));
    let mut f = StdTcp::connect(&fwd).unwrap();
    f.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    f.write_all(b"hi").unwrap();
    let mut reply = [0u8; 5];
    f.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"fw:hi");
    assert_eq!(std::env::var("NO_PROXY").unwrap(), "localhost,127.0.0.1,::1");
    std::process::exit(7);
}

#[tokio::test]
async fn exec_gives_the_command_access_and_passes_its_exit_code() {
    let echo = spawn_echo(b"ok:").await;
    let fwd = spawn_echo(b"fw:").await;
    let gw = spawn_gateway("access", &[echo, fwd]).await;
    let report = gw._dir.0.join("proxy.txt");
    let var = format!("SKIMASQUE_FORWARD_127_0_0_1_{}", fwd.port());
    let (code, stderr) = exec(
        &gw,
        &["--forward", &fwd.to_string()],
        &[
            ("SKM_EXEC_CHILD", "ok".into()),
            ("SKM_ECHO", echo.to_string()),
            ("SKM_FORWARD_VAR", var),
            ("SKM_REPORT", report.display().to_string()),
            ("NO_PROXY", "example.com".into()),
        ],
    )
    .await;
    assert_eq!(code, 7, "{stderr}");
    assert!(stderr.contains("Connected."), "{stderr}");

    // The front end is gone once exec has exited.
    let proxy = std::fs::read_to_string(&report).unwrap();
    assert!(StdTcp::connect(proxy.trim()).is_err(), "listener still open after exit");
}

#[tokio::test]
async fn a_pin_that_does_not_match_is_denied_and_audited() {
    let echo = spawn_echo(b"ok:").await;
    let gw = spawn_gateway("pin", &[echo]).await;
    let (code, stderr) = exec(
        &gw,
        &["--policy", "production"],
        &[("SKM_EXEC_CHILD", "denied".into()), ("SKM_ECHO", echo.to_string())],
    )
    .await;
    assert_eq!(code, 9, "{stderr}");
    assert!(
        stderr.contains(r#"Policy "production" does not apply to this identity; "t" does."#),
        "{stderr}"
    );
    let events = gw.sink.0.lock().unwrap();
    let denied = events.iter().find(|e| e.decision == "deny").expect("a denial was audited");
    assert_eq!(denied.requested_policy.as_deref(), Some("production"));
}

#[tokio::test]
async fn a_missing_command_exits_127() {
    let gw = spawn_gateway("missing", &[]).await;
    let config = TempDir::new("missing-config");
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_skimasque"))
        .args(["exec", "--gateway", &gw.addr.to_string()])
        .args(["--authority", &format!("localhost:{}", gw.addr.port())])
        .arg("--ca")
        .arg(&gw.ca)
        .args(["--auth-token", "t", "--quiet", "--", "skm-definitely-not-a-command"])
        .env("SKIMASQUE_CONFIG_HOME", &config.0)
        .output()
        .await
        .unwrap();
    assert_eq!(out.status.code(), Some(127));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("skm-definitely-not-a-command: command not found"), "{stderr}");
}

#[tokio::test]
async fn a_self_hosted_control_plane_without_a_gateway_exits_125() {
    let config = TempDir::new("nogw-config");
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_skimasque"))
        .args(["exec", "--control-plane", "https://cp.example", "--auth-token", "t", "--", "anything"])
        .env("SKIMASQUE_CONFIG_HOME", &config.0)
        .env_remove("SKIMASQUE_GATEWAY")
        .output()
        .await
        .unwrap();
    assert_eq!(out.status.code(), Some(125));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("No gateway is configured for https://cp.example. Pass --gateway or set SKIMASQUE_GATEWAY."),
        "{stderr}"
    );
}
```

(If `skimasque::policy_engine` / `skimasque::audit` paths differ, use the ones `crates/skimasque/tests/tcp_roundtrip.rs` and `crates/skimasque/src/lib.rs` export. The `--authority localhost:<port>` makes TLS verify against the self-signed `localhost` certificate while dialling `127.0.0.1`.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p skimasque-cli --test exec`
Expected: the tests fail — `skimasque exec` is an unrecognised subcommand (exit 2), so codes do not match. (`child_probe` passes trivially.)

- [ ] **Step 3: Implement**

`Cargo.toml` (`skimasque-cli`): add `"process"` to the tokio features list, and

```toml
[target.'cfg(unix)'.dependencies]
libc = "0.2"
```

(`libc` 0.2 is already in `Cargo.lock`; do not change other versions.)

Append to `exec.rs`:

```rust
/// Run exec to completion and return the process exit status. Every failure
/// before the command starts is printed and returns [`EXIT_FAILED`].
pub async fn run(args: ExecArgs) -> i32 {
    match run_inner(args).await {
        Ok(code) => code,
        Err(Failure(code, message)) => {
            eprintln!("skimasque exec: {message}");
            code
        }
    }
}

struct Failure(i32, String);

fn failed(message: impl std::fmt::Display) -> Failure {
    Failure(EXIT_FAILED, message.to_string())
}

async fn run_inner(args: ExecArgs) -> Result<i32, Failure> {
    use std::sync::Arc;

    use http::{HeaderMap, HeaderValue};
    use tokio::net::TcpListener;

    let login = crate::account::load().map_err(|e| failed(format!("{e:#}")))?;
    let control_plane = resolve_control_plane(args.control_plane.as_deref(), login.as_ref());
    let gateway = resolve_gateway(args.gateway.as_deref(), &control_plane).map_err(failed)?;
    let program = args.command[0].clone();
    let app = args.app.clone().unwrap_or_else(|| default_app(&program));

    let mut headers = HeaderMap::new();
    headers.insert(
        skimasque::APPLICATION_HEADER,
        HeaderValue::from_str(&app)
            .map_err(|_| failed("the application name contains characters a header cannot carry"))?,
    );
    if let Some(pin) = &args.policy {
        headers.insert(
            skimasque::POLICY_HEADER,
            HeaderValue::from_str(pin)
                .map_err(|_| failed("the policy name contains characters a header cannot carry"))?,
        );
    }

    let connected = crate::session::open_session(&gateway, &args.tls, &args.auth, headers)
        .await
        .map_err(|e| failed(format!("{e:#}")))?;
    let identity = connected
        .credential
        .as_deref()
        .and_then(|token| skimasque_identity::peek_identity(token).ok());
    let who = identity_label(identity.as_ref());

    // Preflight: only with a login session on this control plane.
    let mut checked: Option<Checked> = None;
    if let Some(login) = connected.login.as_ref().filter(|l| {
        crate::normalize_base_url(&l.creds.control_plane) == control_plane
    }) {
        let destinations: Vec<String> = if args.forwards.is_empty() {
            vec![PREFLIGHT_PLACEHOLDER.to_owned()]
        } else {
            args.forwards.iter().map(|f| f.target.to_string()).collect()
        };
        match preflight(login, identity.clone().unwrap_or_default(), &app, &destinations).await {
            Ok(probes) => {
                checked = Some(judge(args.policy.as_deref(), &who, &probes, args.forwards.len()).map_err(failed)?);
            }
            Err(error) => eprintln!(
                "skimasque exec: could not check access in advance ({error:#}); the gateway still enforces"
            ),
        }
    }

    let crate::session::Connected { session, refresh, .. } = connected;
    let session = Arc::new(session);
    // Every task is `JoinHandle<()>` so they can be aborted together.
    let mut tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    if let Some((exchange, ttl)) = refresh {
        let s = session.clone();
        tasks.push(tokio::spawn(async move {
            crate::session::refresh_credential(s, exchange, ttl, false).await;
        }));
    }

    let bind = |port: u16| async move {
        TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await
    };
    let http_listener = bind(0).await.map_err(|e| failed(format!("listening on loopback: {e}")))?;
    let socks_listener = bind(0).await.map_err(|e| failed(format!("listening on loopback: {e}")))?;
    let http_addr = http_listener.local_addr().map_err(failed)?;
    let socks_addr = socks_listener.local_addr().map_err(failed)?;
    let mut forwards = Vec::new();
    for spec in &args.forwards {
        let listener = crate::forward::bind(spec).await.map_err(|e| {
            failed(format!("could not listen on 127.0.0.1:{} for {}: {e}", spec.local_port, spec.target))
        })?;
        let local = listener.local_addr().map_err(failed)?;
        let (s, target) = (session.clone(), spec.target.clone());
        tasks.push(tokio::spawn(async move {
            let _ = crate::forward::serve(listener, target, s).await;
        }));
        forwards.push((spec.clone(), local));
    }
    let s = session.clone();
    tasks.push(tokio::spawn(async move {
        let _ = crate::http_connect::serve(http_listener, s).await;
    }));
    let s = session.clone();
    tasks.push(tokio::spawn(async move {
        let _ = crate::socks5::serve(socks_listener, s).await;
    }));

    if !args.quiet {
        let view = HeaderView {
            identity: who.clone(),
            policy: args
                .policy
                .clone()
                .or_else(|| checked.as_ref().and_then(|c| c.selected.clone()))
                .unwrap_or_else(|| "(selected by the gateway)".to_owned()),
            application: app.clone(),
            gateway: gateway.clone(),
            access: forwards
                .iter()
                .enumerate()
                .map(|(i, (spec, local))| AccessLine {
                    destination: spec.target.to_string(),
                    allowed: checked.as_ref().and_then(|c| c.allowed.get(i).copied()),
                    local: *local,
                })
                .collect(),
            session: checked.as_ref().and_then(|c| c.session.clone()),
        };
        eprint!("{}", render_header(&view));
    }

    let mut child = tokio::process::Command::new(&program);
    child.args(&args.command[1..]).envs(child_env(http_addr, socks_addr, &forwards));
    let code = match child.spawn() {
        Err(error) => {
            let code = spawn_error_code(&error);
            let what = if code == EXIT_NOT_FOUND { "command not found".to_owned() } else { error.to_string() };
            eprintln!("skimasque exec: {}: {what}", program.to_string_lossy());
            code
        }
        Ok(mut child) => supervise(&mut child).await.map_err(failed)?,
    };

    for task in &tasks {
        task.abort();
    }
    for task in tasks {
        let _ = task.await;
    }
    if let Ok(session) = Arc::try_unwrap(session) {
        session.close();
    }
    Ok(code)
}

/// Wait for the child. Ctrl-C does not end exec (the child receives it and
/// decides); SIGTERM and SIGHUP are passed on to the child.
async fn supervise(child: &mut tokio::process::Child) -> std::io::Result<i32> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate())?;
        let mut hup = signal(SignalKind::hangup())?;
        let mut int = signal(SignalKind::interrupt())?;
        loop {
            tokio::select! {
                status = child.wait() => return Ok(status_code(status?)),
                _ = int.recv() => {}
                _ = term.recv() => forward_signal(child, libc::SIGTERM),
                _ = hup.recv() => forward_signal(child, libc::SIGHUP),
            }
        }
    }
    #[cfg(not(unix))]
    {
        loop {
            tokio::select! {
                status = child.wait() => return Ok(status_code(status?)),
                _ = tokio::signal::ctrl_c() => {}
            }
        }
    }
}

#[cfg(unix)]
fn forward_signal(child: &tokio::process::Child, signal: libc::c_int) {
    if let Some(pid) = child.id() {
        // SAFETY: `kill` has no memory-safety preconditions; a stale pid at
        // worst signals nothing (the child is still ours until it is reaped).
        unsafe {
            libc::kill(pid as libc::pid_t, signal);
        }
    }
}

/// Simulate each destination on the control plane as the gateway would see it.
async fn preflight(
    login: &crate::session::Login,
    identity: WorkloadIdentity,
    app: &str,
    destinations: &[String],
) -> anyhow::Result<Vec<(String, SimulateResult)>> {
    let api = crate::account::Api::new(&login.creds.control_plane)?;
    let mut probes = Vec::new();
    for destination in destinations {
        let body = serde_json::json!({
            "identity": identity,
            "application": app,
            "destination": destination,
            "transport": "tcp",
        });
        let result = api
            .simulate(&login.creds.session_token, &login.org, None, false, None, &body)
            .await?;
        probes.push((destination.clone(), result));
    }
    Ok(probes)
}
```

`skimasque.rs`: add to `Command`:

```rust
    /// Run a command with the network access your policy grants, through a
    /// gateway, and drop the access when the command exits.
    ///
    /// Sets HTTPS_PROXY / HTTP_PROXY / ALL_PROXY for the command; use
    /// `--forward` for tools that ignore them. Exits with the command's status
    /// (125 if exec itself fails, 126/127 if the command cannot be run).
    Exec(skimasque_cli::exec::ExecArgs),
```

dispatch `Command::Exec(args) => run_exec(args),` and:

```rust
fn run_exec(args: skimasque_cli::exec::ExecArgs) -> anyhow::Result<ExitCode> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    let code = runtime.block_on(skimasque_cli::exec::run(args));
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(skimasque_cli::exec::EXIT_FAILED as u8)))
}
```

(Import `anyhow::Context` if the file does not already.)

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p skimasque-cli --test exec` then `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings`
Expected: all pass. Then a manual check: `cargo run -q -p skimasque-cli --bin skimasque -- exec --help` lists `--policy --app --gateway --control-plane --forward --quiet`, the TLS and auth flags, and `-- <COMMAND>...`.

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 crates/skimasque-cli/src/exec.rs crates/skimasque-cli/src/bin/skimasque.rs crates/skimasque-cli/tests/exec.rs
git add crates/skimasque-cli Cargo.lock
git commit -m "feat(cli): skimasque exec"
```

---

### Task 9: Docs, CI on Windows, and retiring the Planned labels

**Files:**
- Modify: `docs/cli.md` (new `### Run a command with access` under `## skimasque`, after `### Run a gateway or open a tunnel` ~L74)
- Modify: `.github/workflows/ci.yml` (new `windows` job)
- Modify: `docs/superpowers/specs/2026-09-30-product-surface-amendment.md` (L18-25, L147-150)
- Modify: `crates/skimasque-visual/src/site/pages/developers.rs`, `faq.rs`, `use_cases.rs` (module doc + test), `crates/skimasque-visual/src/site/mod.rs` (honesty test ~L509-517)
- Regenerate: `site/` via sitegen

**Interfaces:**
- Consumes: the shipped command's real flags and header (Task 7/8).

- [ ] **Step 1: Write the failing tests** — change the site tests first:

`developers.rs` test `developers_follows_the_canonical_spec`: replace the `"PLANNED"` entry in the `want` list with `"skimasque exec"` and `"--forward"`; replace the last two assertions with

```rust
        assert!(!s.contains("Planned: a command wrapper"), "exec is built now");
        assert_eq!(
            s.matches("<div class=\"v-planned-block\"").count(),
            1,
            "only the control-plane-backed connect note is still planned"
        );
```

`faq.rs` test: rename to `ten_questions_a_deny_flow_and_exec_in_the_developer_answer`; replace the three `skimasque exec`/`planned` lines with

```rust
        assert!(!s.contains("v-planned-block"), "nothing on the FAQ is planned now");
        assert!(s.contains("skimasque exec --policy production -- terraform plan"));
```

`use_cases.rs`: delete `assert!(!s.contains("skimasque exec"), "exec is not built");` and change the module doc's second line to "`skimasque connect`, the real command for one tunnel." 

`site/mod.rs` `honesty_and_voice_rules_hold_on_every_page`: delete the `skimasque exec` assertion and its comment (keep the banned-words loop).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p skimasque-visual --features site`
Expected: `developers_follows_the_canonical_spec` and the FAQ test fail (pages still show exec as Planned).

- [ ] **Step 3: Implement**

`developers.rs`:
- Replace `EXEC_READOUT` with the real header:
  ```rust
  const EXEC_READOUT: &str = "$ skimasque exec --policy production --forward 15432:db.prod:5432 -- psql -h 127.0.0.1 -p 15432\nSkiMasque\n\nIdentity     alice\nPolicy       production\nApplication  psql\nGateway      gateway.skimasque.com\n\nAccess\n  db.prod:5432     ✓  → 127.0.0.1:15432\n\nSession\n  20m\n\nConnected.";
  ```
- In `exec_flow()` change the caption to "A command is checked for identity and policy, given a session, and then run."
- Replace `exec_body`'s first prose with "skimasque exec requests the access a command needs, runs the command, and drops the access when it exits. Tools that honour HTTPS_PROXY or ALL_PROXY just work; for tools that don't, --forward opens a local port to one destination.", change the `CodeExample` labels to "wrap a command" (unchanged) and "what it prints" (was "what the command would print").
- Replace `planned_exec` with a plain section (the `Stack` stays):
  ```rust
  let exec = Section::new("Run a command with access").alt().push(&exec_body);
  ```
  and push `&exec` where `&planned_exec` was. Remove the now-unused `PlannedBlock` import only if nothing else uses it (`cp_note` still does — keep it).

`faq.rs`: the "Can developers use it?" answer becomes `"The CLI supports local policy work, skimasque connect for a single tunnel, and skimasque exec --policy production -- terraform plan to run a command with access."`; delete the trailing `.push(&PlannedBlock::new("a command wrapper for developers", …))` and the `PlannedBlock` import if now unused.

Regenerate and check:

```bash
cargo run -q -p skimasque-visual --features site --bin sitegen
cargo run -q -p skimasque-visual --features site --bin sitegen -- --check
```

`docs/cli.md`, after the `### Run a gateway or open a tunnel` block:

````markdown
### Run a command with access

```
skimasque exec [--policy NAME] [--app NAME] [--gateway HOST[:PORT]]
               [--forward [LOCAL_PORT:]HOST:PORT]... [--org ORG] [--quiet]
               [--auth-token T | --github-oidc --oidc-audience AUD]
               [--ca PEM | --insecure] -- COMMAND [ARGS...]
```

Opens a gateway session, starts an HTTP CONNECT proxy and a SOCKS5 relay on
loopback, and runs `COMMAND` with `HTTPS_PROXY`/`HTTP_PROXY` (HTTP CONNECT),
`ALL_PROXY` (`socks5h://`) and `NO_PROXY=localhost,127.0.0.1,::1` set. Access
ends when the command exits.

| Flag | Meaning |
|---|---|
| `--policy NAME` | Only run if `NAME` is the policy selected for your identity; the gateway enforces it via `X-Masque-Policy`. |
| `--app NAME` | Declared application; defaults to the command's file name. |
| `--gateway HOST[:PORT]` / `$SKIMASQUE_GATEWAY` | Defaults to `gateway.skimasque.com` on SkiMasque Cloud; required with a self-hosted control plane. |
| `--forward [LOCAL_PORT:]HOST:PORT` | A loopback listener tunnelled to one destination, for tools that ignore proxy settings (`psql`). Its address is in `$SKIMASQUE_FORWARD_<HOST>_<PORT>`. Pick `LOCAL_PORT` when your shell needs the port on the command line. |
| `--quiet` | No access summary on stderr. |

With a `skimasque login` session exec checks your access with the control plane
first and refuses to start when `--policy` does not apply or every
`--forward` would be denied. Exit status is the command's; `125` when exec
fails before starting it, `126`/`127` when it cannot be run.

```
skimasque exec --policy production -- terraform apply
skimasque exec --forward 15432:db.prod:5432 -- psql -h 127.0.0.1 -p 15432
```
````

`.github/workflows/ci.yml` — add after the `test` job:

```yaml
  windows:
    # exec spawns processes, handles Ctrl-C and maps exit codes; check that on
    # Windows as well as Linux.
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - name: Cache cargo
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: cargo-${{ runner.os }}-${{ hashFiles('**/Cargo.lock') }}
      - run: cargo test -p skimasque-cli
```

Amendment spec: in the Planned rule (L18-25) replace "Working examples use real commands (`skimasque connect`, the `skimasque-dev/connect` Action); `skimasque exec` appears only in Planned contexts." with "Working examples use real commands (`skimasque exec`, `skimasque connect`, the `skimasque-dev/connect` Action)." and remove `` `skimasque exec`, `` from the "Planned today" list; in `## CLI` replace "`exec` is Planned and is not built by this project." with "`exec` shipped in 2026-09 (`docs/superpowers/specs/2026-09-29-skimasque-exec-design.md`)."

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p skimasque-visual --features site`, `cargo clippy -p skimasque-visual --all-targets --features site -- -D warnings`, `cargo run -q -p skimasque-visual --features site --bin sitegen -- --check`, `cargo test --workspace`
Expected: all pass; `--check` reports the site up to date.

- [ ] **Step 5: Commit**

```bash
rustfmt --edition 2021 crates/skimasque-visual/src/site/pages/developers.rs crates/skimasque-visual/src/site/pages/faq.rs crates/skimasque-visual/src/site/pages/use_cases.rs crates/skimasque-visual/src/site/mod.rs
git add docs .github crates/skimasque-visual site
git commit -m "docs: skimasque exec in the CLI reference and on the site; Windows CI"
```
