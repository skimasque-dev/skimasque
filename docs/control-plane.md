# The control plane

> **Applies to:** ✓ Mode 1 · ✓ Mode 2 · ✓ Mode 3

A **control plane** answers one question: *who is allowed to establish which
tunnel, to where, under what conditions?* It holds identity, the policy
workflow, the fleet view, and the audit trail. It is **never on the traffic
path** — a gateway that loses its control plane keeps enforcing the last policy
it cached.

In **Modes 1 and 2** the control plane is **SkiMasque Cloud** — hosted, kept
available, and upgraded by SkiMasque, at `https://control.skimasque.com`. In
**Mode 3** you run one yourself (see [`self-hosting.md`](self-hosting.md)).

## Responsibilities

| Area | What it does |
|---|---|
| **Identity & organisations** | who your people are, their org and role (owner / member), sign-in (GitHub OAuth), and the per-org Ed25519 keys that sign platform credentials |
| **Access / policy workflow** | policy as reviewed **revisions**: edit a draft, see its diff and the engine's checks, publish it as an authored revision with history; turn a denial into an **access request** a reviewer approves into the policy |
| **Fleet** | gateway registration, the desired-vs-actual view (labels + target revision vs last heartbeat + acked revision), and marking a gateway `offline` when heartbeats lapse |
| **Distribution** | serve each gateway the policy documents its labels match, with an ETag and a long-poll for changes |
| **Audit & usage** | ingest each gateway's hash-chained decision stream, reject a broken chain, and expose the fleet's history and per-org usage |

## The invariant

> **The control plane is authoritative for _desired_ state. Gateways are
> authoritative for _enforcement_.**

Every change flows one way, and none of it reaches into a live tunnel:

```
dashboard / API / CLI
      │
      ▼
   Draft            (mutable, per-org, unvalidated)
      │  validate + test
      ▼
  Revision          (immutable, versioned, authored)
      │  publish
      ▼
 Distribution       (ETag + long-poll; gateways pull)
      │
      ▼
   Gateway          (enforces the revision it holds; fail-static)
```

One decision engine (`skimasque-policy`) is reused everywhere — the gateway hot
path, the "test access" form, `skimasque why`, access-request review — never
re-implemented.

## The gateway ↔ control-plane lifecycle

1. **Register.** Mint a one-time registration token (`skmreg_…`, single-use,
   ~1h) — dashboard → **Gateways → Mint a registration token**, or `skimasque
   gateway register`. The gateway presents it once
   (`--control-plane-token` / `SKIMASQUE_CONTROL_TOKEN`) and receives a
   persistent identity stored under `--control-plane-state`.
2. **Pull policy.** The gateway fetches the documents its labels match (plus
   every untargeted one), caches them, and long-polls for changes. It will not
   start until a revision exists for its org.
3. **Heartbeat.** Every `--control-plane-interval` (default 30s) it reports
   health and usage counters.
4. **Degrade, never stop.** Past the **soft lease**
   (`--control-plane-policy-lease`, default 15m) with the control plane
   unreachable, the gateway reports itself degraded; past the **hard TTL**
   (`--control-plane-cache-ttl`, default 30m) it escalates the alarm — and keeps
   enforcing the cached policy the whole time.

## Credentials

Standalone gateways (policy in a file, no control plane) sign the platform
credential with an HS256 key (`--credential-secret`, shared across a fleet).
Control-plane gateways verify against the org's Ed25519 **public** key (fetched
once, re-checked hourly for rotation) and mint through the control plane at
token exchange, falling back to a locally signed credential during an outage.

## The wire contract

The exact endpoints and JSON shapes are in [`protocol.md`](protocol.md) and the
[`skimasque-protocol`](https://crates.io/crates/skimasque-protocol) crate. A
Mode 3 control plane must serve them; SkiMasque Cloud is one implementation.
