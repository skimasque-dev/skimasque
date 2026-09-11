# The SkiMasque control protocol

> **Applies to:** ✓ Mode 1 · ✓ Mode 2 · ✓ Mode 3 (you implement the server side)

The contract between a gateway (`skimasque-server --control-plane <url>`) and a
control plane. SkiMasque publishes this so that a fully self-hosted deployment
is possible without SkiMasque Cloud: **anything that serves these endpoints with
these JSON shapes can drive a stock open-source gateway.**

The Rust types are the [`skimasque-protocol`](https://crates.io/crates/skimasque-protocol)
crate; this page is the human-readable reference. `PROTOCOL_VERSION` is `"v1"`.

## Shape

- Plain HTTPS, JSON request and response bodies.
- Two audiences:
  - **Gateway ↔ control plane** (documented here) — the enforcement-relevant
    surface.
  - **Client ↔ control plane** — `skimasque login`, org and policy management,
    usage and audit queries. Management-only, off the enforcement path;
    [summarised below](#client--control-plane-endpoints) but not part of the
    stable contract yet.
- After registration a gateway authenticates every call with
  `Authorization: Bearer <secret>`, where `<secret>` is the value it received in
  the registration response. `POST /v1/gateways/register` and
  `GET /v1/orgs/{org}/signing-key` are unauthenticated (a one-time token and a
  public key, respectively).

## Invariant

The control plane decides *desired* state; the gateway enforces. A gateway that
loses the control plane keeps enforcing its last cached policy and never fails
open. Nothing here lets a control plane reach into live enforcement — the most a
control plane does is publish policy the gateway then pulls, and sign
credentials the gateway then verifies.

---

## Gateway ↔ control plane endpoints

### `POST /v1/gateways/register`

Register a new gateway. Unauthenticated; the one-time token is the credential.

Request:

```json
{
  "registration_token": "skmreg_…",
  "name": "gw-eu-west-1",
  "labels": { "env": "prod", "region": "eu-west-1" }
}
```

`labels` may be omitted. Response `200`:

```json
{ "gateway_id": "gw_…", "org_id": "org_…", "secret": "…" }
```

The gateway persists all three under `--control-plane-state`. The token is spent;
a restart reuses the stored identity. A spent or expired token → `403`
`bad_registration_token`.

### `GET /v1/gateways/{id}/policy`

Poll for policy. `Authorization: Bearer <secret>`.

- `If-None-Match: "<version>"` makes it conditional on the revision the gateway
  already holds.
- `?wait=<seconds>` long-polls: the control plane holds the request open up to
  that long, answering as soon as a newer revision is published.

Responses:

| Status | Meaning |
|---|---|
| `200` + body | a newer revision; body below |
| `304` | nothing newer than `If-None-Match` |
| `204` | the org has never published a policy — the gateway **exits** ("the control plane has no policy for this gateway's organisation yet") and restart-loops until one exists |

`200` body:

```json
{
  "version": 7,
  "documents": [
    { "name": "production.toml", "text": "name = \"production\"\n[match]\n…" }
  ]
}
```

`version` is also the next `ETag` value. The gateway writes each document to its
policy-dir cache atomically and enforces the set.

### `POST /v1/gateways/{id}/heartbeat`

Liveness and usage counters. `Authorization: Bearer <secret>`.

```json
{
  "status": "online",
  "policy_version": 7,
  "usage": { "tunnels_opened": 128, "bytes_to_target": 9000000, "bytes_to_client": 4000000 }
}
```

`status` is `"online"` or `"degraded"` (the latter once the gateway has been
past its soft policy lease with the control plane unreachable — it is still
enforcing). `usage` is cumulative-since-start and may be omitted. The control
plane marks a gateway `offline` in the fleet view if heartbeats lapse.

### `PUT /v1/gateways/{id}/labels`

Declare (or update) the gateway's labels for policy targeting. Idempotent.

```json
{ "labels": { "env": "prod", "region": "eu-west-1" } }
```

### `POST /v1/gateways/{id}/credentials`

Ask the control plane to sign a platform credential for a workload identity the
gateway **has already verified** from the runner's OIDC token. `Authorization:
Bearer <secret>`.

```json
{
  "identity": { "organization": "acme", "repository": "acme/widget", "workflow": "deploy.yml", "git_ref": "refs/heads/main" },
  "subject": "repo:acme/widget:ref:refs/heads/main",
  "ttl_seconds": 3600
}
```

Response:

```json
{ "credential": "<JWT>", "expires_in": 3600 }
```

The credential is signed with the org's Ed25519 private key. `expires_in` is the
actual lifetime after any server-side cap. The gateway may fall back to a
locally signed HS256 credential if this endpoint is unreachable (unless
`--control-plane-no-credential-fallback`).

### `GET /v1/orgs/{org}/signing-key`

The org's Ed25519 public key. Unauthenticated. The gateway verifies
control-plane-minted credentials against this **offline**, with no further calls.

```json
{
  "org_id": "org_…",
  "algorithm": "ed25519",
  "public_key_b64": "…",            // raw 32-byte key, standard base64
  "previous_public_key_b64": "…"     // present across a rotation, until old credentials expire
}
```

The gateway re-fetches this on `--control-plane-signing-key-interval` (default
1h) so an in-place rotation is picked up without a restart.

### `GET /v1/gateways/{id}/audit/head`

The tail of this gateway's audit chain, so a restart resumes its sequence.

```json
{ "seq": 4210, "hash": "…" }        // seq 0, hash = 64 zeros, before anything is shipped
```

### `POST /v1/gateways/{id}/audit`

Ship a hash-chained batch of audit events. `Authorization: Bearer <secret>`.

```json
{
  "events": [
    { "seq": 4211, "prev_hash": "<hash of 4210>", "event_json": "{…the audit event verbatim…}" }
  ]
}
```

Each link is `sha256(seq_be || prev_hash || event_json)` as lowercase hex
(`skimasque_protocol::audit_hash`). The control plane recomputes the chain and
**rejects a break** (`409`). Response:

```json
{ "head_seq": 4211 }
```

Audit shipping is fail-static: a ship failure never blocks enforcement, and the
gateway's local JSONL sink stays authoritative.

---

## Client ↔ control plane endpoints

Not part of the stable contract; listed so a self-hosted control plane knows the
surface `skimasque login` / `org` / `audit` / `status` and the dashboard expect.

| Method + path | Purpose |
|---|---|
| `POST /v1/auth/device`, `/v1/auth/device/poll`, `/v1/auth/logout` | the GitHub OAuth device flow, brokered by the control plane |
| `GET /v1/me` | the signed-in developer |
| `POST /v1/orgs`, `GET /v1/orgs`, `GET /v1/orgs/{org}` | organisations |
| `POST/GET /v1/orgs/{org}/members`, `DELETE /v1/orgs/{org}/members/{user}`, `PUT …/role` | membership and roles |
| `POST /v1/orgs/{org}/registration-tokens` | mint a one-time gateway token |
| `POST /v1/orgs/{org}/policy`, `GET …/policy` | publish / read a policy revision |
| `POST /v1/orgs/{org}/policy/simulate` | evaluate a hypothetical request against a revision |
| `GET/POST /v1/orgs/{org}/signing-key`, `…/signing-key/rotate` | the org signing key |
| `GET /v1/orgs/{org}/gateways` | the fleet |
| `GET /v1/orgs/{org}/usage`, `…/usage/history` | usage totals and the daily series |
| `GET /v1/orgs/{org}/audit` | query shipped decisions (`{ events, next_cursor }`) |

## Health

`GET /healthz` and `GET /readyz` are unauthenticated. `GET /metrics` (Prometheus)
is gated by the admin token.
