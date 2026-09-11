# Threat model

> **Applies to:** ✓ Mode 1 · ✓ Mode 2 · ✓ Mode 3 (the gateway trust boundaries
> are the same; Mode 3 adds the control plane's own trust — see
> [`security.md`](security.md))

A working security model for a **GitHub Actions egress gateway**. It names what
the system protects, who might attack it, where the trust boundaries are, and
what stops each attack today. It is the scoping document for an independent
security review, and it is written to be falsifiable: every "mitigated" claim
points at code or config a reviewer can check.

First written against the tree at the `feat/release-image` branch point
(`20a7941`) and kept current since; code references still cite `20a7941` where
the mechanism has not changed.

## What the system is

One stateless process, `skimasque-server`, sits at a network egress point. A CI
job proves its identity with a GitHub (or GitLab / Buildkite / generic) OIDC
token, trades it once for a short-lived credential, and then opens MASQUE
tunnels that carry its real traffic. A policy — a set of files the gateway
re-reads in place — decides per tunnel whether it opens, to where, and under
what rate and volume limits.

```
                    ┌─────────────────────── gateway host (trusted) ────────────────────────┐
                    │                                                                       │
 CI runner          │   skimasque-server                                                    │
 (semi-trusted)     │   ┌───────────────┐   ┌──────────────────────────────────────────┐    │
   │                │   │ token exchange│   │ data plane (QUIC / HTTP/3)                │    │
   │ OIDC token ────┼──▶│  verify RS256 │   │  IdentityLayer  → verify HS256 credential │    │
   │                │   │  iss/aud/exp  │   │  AuthorizeLayer                           │    │
   │ ◀── credential ┼───│  → HS256 JWT  │   │  PolicyLayer    → evaluate policy set     │    │
   │  (HS256, ~1h)  │   └───────────────┘   │  QuotaLayer     → rate / volume limits    │    │
   │                │                       │  AddressPolicy  → SSRF floor (resolved IP)│    │
   │ tunnels ───────┼──────────────────────▶│  Dispatch       → UdpProxy / TcpProxy     │    │
   │                │                       └───────────────────────┬──────────────────┘    │
   │                │                                               │                       │
   │                │   policy files ─┐   TLS cert/key ─┐           │ egress                │
   │                │   audit log ────┤   (files, hot-reloaded)     │                       │
   │                └─────────────────┼────────────────────────────┼───────────────────────┘
   │                                  │                            │
 OIDC issuer JWKS                operator / CD                private resources
 (external, trusted to           (trusted)                   (db.prod:5432, …)
  sign only its own tokens)
```

## Assets

| Asset | Why it matters | Where it lives |
|---|---|---|
| **Reachability of private resources** | The whole point of the gateway is to be a controlled path to things a runner otherwise cannot reach. | The gateway's network position. |
| **The credential-signing secret** (`--credential-secret` / `SKIMASQUE_CREDENTIAL_SECRET`) | Anyone who holds it can mint a credential for any identity and get that identity's access. | Gateway process env / memory; operator's secret store; shared across a fleet. |
| **The TLS private key** | Impersonating the gateway lets an attacker collect OIDC tokens and MITM tunnels. | `--key` file on the host, hot-reloaded. |
| **The policy set** | It *is* the authorization decision. A bad edit widens access silently. | Files under `--policy-dir` / `--policy-file`, or a ConfigMap. |
| **The audit log** | The only record of what was allowed and denied. | `--audit-log` file, or the `masque::audit` tracing target. |
| **In-flight tunnel traffic** | Contains the job's own secrets (registry creds, DB passwords, source). | QUIC streams/datagrams between runner and gateway; plaintext on the egress side. |
| **A runner's OIDC token** | A bearer token for the exchange endpoint; valid for its short lifetime and broad `aud`. | Runner memory; in transit to the gateway. |

## Actors and trust levels

| Actor | Trust | What we assume they can do |
|---|---|---|
| **Operator / CD pipeline** | Trusted | Deploy the gateway, write policy, hold the secrets. Out of scope as an attacker. |
| **OIDC issuer** (GitHub, …) | Trusted to sign only tokens it should | Publishes a JWK Set; signs tokens with the claims the CI platform asserts. Not assumed to be malicious, but *is* assumed to sign tokens for **every** repo/workflow on the platform, including an attacker's. |
| **Legitimate CI job** | Semi-trusted | Runs code from a repository. Holds a valid OIDC token and, after exchange, a credential scoped to its identity. **May be running attacker-controlled code** (a malicious PR, a compromised dependency, a supply-chain attack in the job itself). |
| **Attacker with their own GitHub repo** | Untrusted | Can obtain a validly-signed OIDC token for *their* `repository` / `workflow_ref` and hit the exchange endpoint. |
| **Network attacker** | Untrusted | On-path between runner and gateway, or between gateway and the OIDC issuer; can send packets to any listener. |
| **Compromised gateway host** | Out of scope | Per [`SECURITY.md`](../SECURITY.md); if the host is owned, the model does not hold. |

The central tension: **a legitimate CI job is the primary threat agent.** The
gateway exists precisely because you cannot fully trust the code running in a
build. Every boundary below is drawn with "the job is hostile" in mind.

## Trust boundaries and the threats at each

### B1 — Runner → token-exchange endpoint

A caller presents an OIDC token; the gateway returns a credential.

| Threat | Mitigation | Residual |
|---|---|---|
| Forged or altered token | RS256 verified against the issuer's JWK Set (`skimasque-identity`); `kid` matched, one refetch on an unknown `kid`. | Trust in the issuer's key management. |
| Token from a different audience (a token minted for another service, replayed here) | `aud` must equal one of `--oidc-audience`; the operator picks a gateway-specific value. | An operator who reuses a common audience string weakens this. |
| Expired / not-yet-valid token | `exp` / `nbf` checked with 60 s leeway. | 60 s replay window on a leaked token. |
| Wrong issuer | `iss` must equal `--oidc-issuer`. | — |
| **Attacker's own repo requests a credential** | The credential carries *their* identity (`repository`, `workflow_ref`, `ref`, …). Policy then denies it — deny-by-default means an unknown repo matches nothing. | The operator must not write an over-broad `[match]` (e.g. matching only `repository_owner`). Policy tests (`[[tests]]`) are the guard. |
| Token exfiltrated from a runner and replayed from elsewhere | Short OIDC lifetime; the credential it yields still carries the original job's identity, so stolen access is bounded by that job's policy. | A token stolen mid-job can be exchanged by the attacker for a credential with the same access the job legitimately has. This is inherent to bearer tokens; mitigation is short TTLs and policy scoping. |
| DoS on the exchange (RSA verify + JWKS fetch per call) | A per-source-IP token bucket (`--max-exchange-rate`, default 10/s burst 30) refuses excess with `429` *before* the verify; JWKS cached for an hour; the exchange is meant to be called once per job. `--max-concurrent-requests` bounds concurrent verification. | Per-source key is the QUIC peer IP; a NAT'd fleet shares one bucket. Global (all-source) exchange volume is bounded only by `--max-concurrent-requests`. |

### B2 — Runner → data plane (tunnel open)

Every tunnel carries `Proxy-Authorization: Bearer <credential>`.

| Threat | Mitigation | Residual |
|---|---|---|
| No credential | `407` — `IdentityLayer` is the authentication boundary when present, fail-closed. | If the gateway is run without `--oidc` / `--auth-token`, there is no auth. Documented; the exchange endpoint and `--oidc` are the supported path. |
| Forged credential | HS256 verified locally with `--credential-secret`; no network, no external trust at tunnel time. | Strength is the secret. A weak or leaked secret is total compromise of the identity layer. |
| Expired credential | `exp` checked; the client proactively re-exchanges before expiry. | — |
| Credential minted by a *different* gateway in the fleet | Accepted by design when `--credential-secret` is shared. | A compromise of one gateway's secret compromises the fleet. Operators who want isolation run separate secrets. |
| Replaying a captured credential from another runner | The credential is a bearer token; its identity and its policy are what bound the damage, plus `[session] max_duration`. | Same bearer-token caveat as B1. No proof-of-possession binding to the QUIC connection. |
| Lying about the application (`--app terraform`) | **Not trusted.** The README is explicit: application name is session context, matched but not authenticated. Policy authors treat it as a convenience, not a control. | A hostile job picks whatever `--app` string opens the most doors. Mitigation is writing policy where the *destination* set is the real constraint. Strong process identity is roadmap (Phase 5). |

### B3 — PolicyLayer → egress (destination authorization)

| Threat | Mitigation | Residual |
|---|---|---|
| Reaching a destination no rule allows | Deny-by-default; no implicit allow. Every `DENY` is logged with a reason. | Only as good as the policy. |
| SSRF to internal / metadata addresses (`169.254.169.254`, RFC 1918, loopback, CGNAT, link-local, multicast) | `AddressPolicy` — the SSRF floor — checked against the **resolved** IP, not the requested name, so DNS rebinding does not bypass it. Refused unless the deployment opts in. The opt-in is per-range: `--allow-cidr 10.0.5.0/24` names exactly what is reachable and leaves the rest of private space, and the metadata address, refused. `--allow-private` still exists but is the blunt instrument. | An operator can still use `--allow-private` and re-expose everything; the docs steer to `--allow-cidr`. A CIDR that is itself too wide is the operator's mistake to make. |
| DNS rebinding / TOCTOU between resolve and connect | Policy and `AddressPolicy` run on resolved addresses; the proxy forwards only to an `AuthorizedDestination`, never a bare target. | A name that resolves to different IPs on two lookups: the one that is connected is the one that was checked, because the checked address is what is dialed. |
| Forwarding path constructed without authorization | Structural: the inner proxy takes an `AuthorizedDestination` only. The single bypass, `AuthorizedDestination::trusting`, is named to be greppable and is not on the request path. | Code review item — verify no new call site. |
| Policy file tampered on disk | Out of scope (host compromise) — but a malformed revision is logged and **skipped**, so a broken edit fails closed to the last good set rather than open. | A *valid* but wrong edit applies silently after `--policy-reload-interval`. Mitigation: policy in version control, `skimasque policy check` / `diff` / `[[tests]]` in the CD pipeline. |

### B4 — Gateway → OIDC issuer (JWKS fetch)

| Threat | Mitigation | Residual |
|---|---|---|
| MITM of the JWKS fetch serving attacker keys | HTTPS with the system trust store; rustls, no OpenSSL. | Trust in the host CA bundle. |
| Issuer unreachable → fail open? | No — verification fails closed; no cached key for the `kid` means the token is rejected. | A JWKS outage stops *new* exchanges (existing credentials keep working for their TTL). Availability, not integrity. |
| Malicious `kid` forcing unbounded refetches | One refetch per unknown `kid`, then cached. | Light amplification only. |

### B5 — Network attacker on the runner↔gateway path

| Threat | Mitigation | Residual |
|---|---|---|
| Eavesdrop tunnel traffic | QUIC / TLS 1.3, always on. | Egress side is plaintext by nature (that is the point of a proxy) — the private resource sees the gateway's IP. |
| Impersonate the gateway to harvest OIDC tokens | The client verifies the gateway cert against its configured CA; `--insecure` disables this and is explicitly out of scope. | An operator who ships `--insecure` to production. Documented as dev-only. |
| Spoofed / flooded QUIC packets | QUIC address validation (Retry); `ResourceLimits` — concurrent-connection cap, a **per-source-IP** new-connection rate limit checked before the global one (a bounded LRU map of token buckets), per-connection tunnel cap, per-tunnel idle timeout — refuse before a handshake task is spawned. | The per-source key comes from the not-yet-validated peer address, so a spoofed-source flood can still churn the bucket map — but only within its fixed bound. A fleet behind one NAT egress IP counts as a single source, so the per-source rate must be raised or disabled there. The token-exchange endpoint is still only globally rate-limited (see gaps). |

### B6 — Cross-tenant isolation on one gateway

Multiple repos/workflows share one process.

| Threat | Mitigation | Residual |
|---|---|---|
| One job's traffic leaking into another's tunnel | Each tunnel is a distinct QUIC stream/flow with its own `AuthorizedDestination`; no shared relay buffer between tunnels (64 KB per relay task). | Code review item. |
| One job exhausting shared resources to deny others | Per-policy `[limits]` (bandwidth, packets/s, concurrent connections) and per-tunnel `bytes`; global `ResourceLimits` as the backstop. | Limits are **aggregate per policy**, not per session/job — two jobs under the same policy share one bandwidth bucket. Per-session limits need the credential to carry a session id (roadmap). |
| Learning-mode output leaking one tenant's destinations into another's draft | Learning groups by `(application, transport, destination)` and emits a draft for review; it is never auto-applied. | Operator process — review the draft. |

## The two authorization escape hatches

Both are named to be greppable, and an audit against `20a7941`'s successors
found neither reachable on an enforced request path by accident.

### `AuthorizedDestination::trusting`

The inner `UdpProxy` / `TcpProxy` forwards only to an `AuthorizedDestination`.
`trusting` is the one constructor that makes one with no policy decision behind
it. It is reached in exactly three places:

- **No `PolicyLayer` in the stack** (`open_tunnel` / `open_tcp_tunnel` fall
  back to it). This is the deliberate "network floor only" mode for a
  single-tenant gateway. `skimasque-server` now warns at startup when an
  identity source is configured but no policy is — the case where an operator
  might wrongly believe destinations are constrained.
- **Observe mode** (`--policy-observe`), which by definition enforces nothing;
  it evaluates, logs `masque::observe`, and forwards. Opt-in, and the audit log
  is deliberately not written in this mode.
- Tests.

`Enforce::call` itself is fail-closed: an `Allow` attaches a real
`AuthorizedDestination::from_decision`, a `Deny` returns `403`, and a request it
cannot turn into a policy question (a non-UDP/TCP target) is denied, not passed
through. There is no path from `Enforce` to the inner service that skips
`authorize`.

### `--insecure`

A `skimasque-client` flag that swaps certificate verification for
`AcceptAnyCertificate`. It is CLI-only (no env var), `conflicts_with` `--ca`,
prints a warning, and is not exposed by the composite GitHub Action (which only
takes an optional `--ca`). The default client path uses the webpki roots. The
library function is `pub` but named `dangerous_client_config_without_verification`
and documented as dev-only.

A gateway started with `--acme` serves a publicly-trusted Let's Encrypt
certificate for `--hostname`, so its clients need neither `--ca` nor
`--insecure` — the default webpki path just works (and `skimasque-client
--proxy` defaults to `gateway.skimasque.com`). This removes the operational
pressure that
leads to `--insecure` (a self-signed gateway whose certificate was not
distributed). The ACME account key and issued certificate sit in
`--acme-cache`; they are ordinary key material, protected as assumption 2
requires, and a leaked *account* key lets an attacker who also controls the
domain's DNS or port 443 issue certificates — i.e. it grants nothing they did
not already have.

## Security assumptions

1. The gateway host and its operator are trusted. Host compromise is out of scope.
2. `--credential-secret` and the TLS key are stored and distributed by the
   operator with at least the care of any other production secret.
3. The OIDC issuer signs tokens honestly and manages its keys. It *will* sign a
   token for an attacker's own repository — policy, not token validity, is what
   keeps that attacker out.
4. Policy is treated as code: version-controlled, reviewed, tested with
   `[[tests]]`, and checked in CI before it reaches `--policy-dir`.
5. Clients verify the gateway certificate — the default webpki path, which an
   `--acme` gateway makes zero-config. `--insecure` is for local development
   only.
6. TLS is terminated at the gateway; the egress hop runs on a network the
   operator already controls with a firewall (skimasque decides *who*, the
   firewall decides *what is reachable at all*).

## Non-goals (today)

- **Authenticating the application name.** Session context, not proof, until
  Phase 5 (cgroups / eBPF process identity).
- **Proof-of-possession credentials.** Credentials and OIDC tokens are bearer
  tokens; a token stolen from a running job grants that job's access for the
  token's lifetime.
- **Protecting against a malicious operator or a compromised host.**
- **Confidentiality of traffic past the gateway.** The proxy exists to make that
  hop; the private resource sees gateway-sourced plaintext (or its own TLS).
- **A hosted multi-org control plane.** One gateway, one operator, files on disk.

## Known gaps feeding the review

These are the questions the independent review should press on:

1. **The per-source rate limiters.** The QUIC new-connection path and the
   token-exchange path both now have per-source-IP token buckets
   (`PerSourceRate` / `ExchangeRateLimiter` in `crates/skimasque/src/server.rs`),
   bounded by a `MAX_SOURCES` LRU map. Is that map the right structure, is
   keying on an unvalidated QUIC peer address acceptable, and is the residual —
   a NAT'd fleet sharing one bucket, and no *global* cap on exchange volume
   beyond `--max-concurrent-requests` — the right trade?
2. **Per-session resource limits.** `[limits]` are aggregate per policy; a
   session id in the credential would let them be per job.
3. **Bearer-token theft.** Is there a practical binding of the credential to the
   QUIC connection or the runner that would not break mid-session refresh?
4. **Policy misconfiguration is the likeliest real-world failure.**
   `skimasque policy validate` now lints for the broad-`[match]` cases
   (`match-governs-every-workload`, `match-lacks-repository`,
   `match-lacks-ref`), `--strict` fails CI on them, and the gateway logs them
   on load (`Policy::lints` in `crates/skimasque-policy/src/lint.rs`). Are those
   three heuristics the right set, is warn-not-fail the right default, and
   should `match-lacks-ref` be louder given it is the pull-request-from-a-fork
   vector?
5. **`AddressPolicy` opt-in granularity.** Addressed: `--allow-cidr` /
   `AddressPolicy::allowed_cidrs` is a per-range allow-list that overrides the
   category floor for exactly the networks listed (`allowed_cidrs` in
   `crates/skimasque/src/policy.rs`). The review should decide whether
   `--allow-private` should be deprecated outright, and whether a CIDR that
   spans a category (e.g. `10.0.0.0/8`) deserves a warning.
6. **The `trusting` / `--insecure` escape hatches.** Audited — see
   [The two authorization escape hatches](#the-two-authorization-escape-hatches).
   The internal review should re-confirm the three `trusting` call sites and
   that `--insecure` stays out of the Action, and decide whether the
   identity-without-policy mode should be a hard error rather than a warning.
7. **Fleet secret sharing.** The `--credential-secret` model trades isolation
   for fleet mobility. `SECURITY.md` now spells out the blast radius and
   rotation; the review should still sanity-check the wording. The control
   plane's D1 scheme narrows this: with `--control-plane --oidc`, the private
   signing key stays on the control plane, gateways hold only the org public
   key and verify credentials against it offline, and issuance goes through
   `POST /v1/gateways/{id}/credentials`. A gateway compromise no longer leaks a
   fleet-wide minting secret. The org key rotates via
   `POST /v1/orgs/{id}/signing-key/rotate` — gracefully (previous key kept for
   the credential TTL) or `revoke_previous` for an immediate revocation of every
   outstanding credential. Residual exposure: `--credential-secret` still signs
   the 5-minute fallback credentials during a control-plane outage (and
   `--control-plane-no-credential-fallback` removes even that); rotation is
   operator-triggered with no automatic cadence yet, and there is no
   per-credential revocation. The review should check the outage/fallback
   window and the still-shared HS256 path.
8. **Dependency surface.** `quinn`, `h3`, `rustls`, `jsonwebtoken`. The `h3`
   git pin was reviewed (see the notes in the workspace `Cargo.toml`): the rev
   is `hyperium/h3` `master` HEAD, `master` is developed actively but the
   `h3-v*` tags have not moved in over a year, so there is no imminent release
   to move to; the pin also carries correctness fixes we want. `deny.toml`
   restricts git sources to that one URL. Open items for the review: `cargo
   audit` does not see the git dependency (an `h3` advisory needs a manual
   watch), and `h3-quinn` runs from its registry release against patched `h3`
   with no lockstep guarantee. The clean exit is `h3` 0.0.9, or dropping
   `connect-ip` as a goal and reverting to `h3` 0.0.8.
