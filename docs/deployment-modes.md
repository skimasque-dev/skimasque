# Deployment modes

**Start managed. Own more when you need to.**

SkiMasque is two things sharing one open protocol:

- a **control plane** — identity, the policy workflow, the dashboard, audit —
  which decides *who may open which tunnel, to where, under what conditions*;
- a **gateway** (`skimasque-server`) — MASQUE transport and enforcement — which
  carries an allowed tunnel and denies everything else.

Where each runs is a deployment choice. All three modes use the **same** policy
model, the **same** gateway binary, and the **same** GitHub Action — you are not
learning three products.

```
                Mode 1                Mode 2                Mode 3
            ┌───────────┐         ┌───────────┐         ┌───────────┐
control     │ SkiMasque │         │ SkiMasque │         │    You    │
plane       │   Cloud   │         │   Cloud   │         │           │
            └─────┬─────┘         └─────┬─────┘         └─────┬─────┘
                  │ authorization       │ authorization       │ authorization
            ┌─────▼─────┐         ┌─────▼─────┐         ┌─────▼─────┐
gateway     │ SkiMasque │         │    You    │         │    You    │
            └─────┬─────┘         └─────┬─────┘         └─────┬─────┘
                  ▼                     ▼                     ▼
            your infrastructure   your infrastructure   your infrastructure
```

| | Mode 1 — Fully managed | Mode 2 — Customer gateway | Mode 3 — Fully self-hosted |
|---|---|---|---|
| Control plane | SkiMasque | SkiMasque | **You** |
| Gateway | SkiMasque | **You** | **You** |
| You control the egress IP | No | Yes | Yes |
| You operate a gateway | No | Yes | Yes |
| You operate a control plane | No | No | Yes |
| GitHub Action | `skimasque-dev/connect@v1` | `skimasque-dev/connect@v1` | `skimasque-dev/connect@v1` |
| `--control-plane` endpoint | `control.skimasque.com` (default) | `control.skimasque.com` (default) | your URL |
| Operational load | Lowest | Medium | Highest |
| SkiMasque Cloud required | Yes | Yes | No |
| Best for | Fastest adoption | Traffic path in your network | Maximum control, air-gapped, regulated |

**Start with Mode 1.** Move to Mode 2 when you need the network path in your own
infrastructure. Mode 3 is the advanced path — reach for it only when a
dependency on any external service is genuinely unacceptable, and expect real
work. None of the three is a crippled edition; the difference is operational
load, and it climbs steeply from left to right.

---

## Mode 1 — Fully managed

> **Applies to:** ✓ Mode 1

*"Just use it."*

SkiMasque operates the control plane and the gateway. You point CI at the
SkiMasque endpoint and write policy. There is nothing to provision, install,
register, patch, monitor, or keep online.

```
GitHub Actions ──OIDC──▶ SkiMasque Cloud ──authorized tunnel──▶ your database / API
```

You configure **identity** (which OIDC issuers and claims you trust) and
**policy** (what each identity may reach). That's it.

- Fastest path to a working enforcement loop — a policy file and a workflow
  change is the whole integration.
- Full control-plane feature set: reviewed policy revisions, the dashboard, the
  fleet view, access requests, audit history.
- Per-org isolation: organisations share the endpoint but never see each other's
  traffic or policy.
- Traffic egresses from a shared SkiMasque IP. A **dedicated egress IP** — so you
  can allowlist exactly one address on your firewall — is a higher subscription
  tier (same gateway, a reserved address).

Get started: [`getting-started.md`](getting-started.md).

---

## Mode 2 — Customer gateway

> **Applies to:** ✓ Mode 2

*"Keep the network path in my infrastructure."*

You run the open-source gateway (`skimasque-server`) inside your own network;
SkiMasque Cloud provides the control plane.

```
GitHub Actions ──OIDC──▶ SkiMasque Cloud ──authorization──▶ your gateway (in your VPC) ──▶ private service
```

Choose this when:

- the target is on a **private network** the SkiMasque endpoint cannot reach
  (`10.20.30.15:5432` in a VPC) — the strongest reason;
- your security team **won't allow** SkiMasque infrastructure near their
  production network, or won't expose a private service to inbound traffic even
  from a fixed IP;
- **data residency / compliance** requires the traffic path to stay in a region,
  VPC, or data centre you control;
- you want a predictable, single egress IP under your control.

Responsibilities:

| You own | SkiMasque owns |
|---|---|
| the gateway host / container, its network placement, firewall, outbound connectivity, upgrades, and availability | identity, the policy workflow, authorization decisions, the dashboard, and audit |

The gateway still does full policy enforcement locally and keeps enforcing its
cached policy if the control plane is unreachable — SkiMasque Cloud is never on
the traffic path. Running the gateway: [`gateways.md`](gateways.md).

---

## Mode 3 — Fully self-hosted

> **Applies to:** ✓ Mode 3

*"Operate everything myself."* The advanced path.

You run the gateway **and** a control plane. No dependency on SkiMasque Cloud.

```
GitHub Actions ──OIDC──▶ your control plane ──authorization──▶ your gateway ──▶ your network
```

Choose this only for air-gapped or heavily regulated environments where a
dependency on any external service is genuinely unacceptable. It is real
engineering and operational work, not a configuration choice.

The **enforcement edge** is open source and speaks the protocol in every mode —
the gateway, the client, the CLI, the policy engine, OIDC verification, the
[`skimasque-protocol`](protocol.md) contract, and the GitHub Action. **SkiMasque
Cloud's control-plane implementation is not.** There is no open-source reference
control-plane server; Mode 3 means you implement a control plane against
[`protocol.md`](protocol.md), or licence SkiMasque's distribution for
self-hosting.

See [`self-hosting.md`](self-hosting.md) — and get in touch before committing.

---

## Moving between modes

- **Mode 1 → Mode 2:** deploy a gateway, register it, and remove SkiMasque's
  gateway from the org. Policy and identity config carry over unchanged.
- **Mode 2 → Mode 3:** you need a control plane that speaks the protocol first
  (see above) — then the gateway re-registers against it (`--control-plane
  <your-url>`). The gateway binary and the Action do not change; only the
  endpoint does.
- Nothing about the policy DSL or the Action's inputs is mode-specific.
