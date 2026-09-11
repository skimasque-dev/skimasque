# Fully self-hosted (Mode 3)

> **Applies to:** ✓ Mode 3
>
> Mode 3 is the advanced path. Start with [Mode 1](deployment-modes.md#mode-1--fully-managed)
> and move to [Mode 2](gateways.md) if you need the network path in your own
> infrastructure. Choose Mode 3 only when a dependency on any external service
> is genuinely unacceptable — air-gapped or heavily regulated environments — and
> expect real engineering and operational work.

## What is open

The entire **enforcement edge** is open source (MIT OR Apache-2.0) and unchanged
across all three modes: the gateway (`skimasque-server`), the tunnel client, the
`skimasque` CLI, the policy engine, OIDC verification, the GitHub Action, and
the **control protocol** — the `/v1` HTTP contract a gateway speaks to a control
plane, documented in [`protocol.md`](protocol.md) and released as the
[`skimasque-protocol`](https://crates.io/crates/skimasque-protocol) crate.

## What is not

SkiMasque Cloud — the control-plane *implementation*, the managed policy-review
workflow, the dashboard, the audit store, org/roles/billing — is proprietary.
There is **no open-source reference control-plane server**, and Mode 3 does not
hand you SkiMasque Cloud.

To run Mode 3 you either:

- **implement a control plane** against [`protocol.md`](protocol.md) — the
  gateway is a stock client and does not change; or
- **licence SkiMasque's control-plane distribution** for self-hosting.

Either way, [get in touch](https://github.com/skimasque-dev/skimasque/issues)
before committing to Mode 3.

## What does not change

- The gateway, the CLI, and the Action are identical to Modes 1–2 — they take a
  configurable control-plane endpoint (`--control-plane` /
  `$SKIMASQUE_CONTROL_PLANE` / `SKM_CONTROL_PLANE`), not a hard-coded one.
- The gateway still verifies the runner's OIDC token itself; your control plane
  only signs credentials for an already-verified identity.
- Certificates, ports, and firewall are as in [`gateways.md`](gateways.md). Your
  control plane needs its own TLS and a persistent store on top of that.

## The line

Mode 3 reproduces the *enforcement* capability. It does not reproduce the
operational experience — hosting, availability, upgrades, the dashboard, managed
gateways and egress, and support are what SkiMasque Cloud is for, in every mode.
