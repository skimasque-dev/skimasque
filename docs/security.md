# Security model

> **Applies to:** ✓ Mode 1 · ✓ Mode 2 · ✓ Mode 3

SkiMasque is pre-1.0 and has not had an independent security review. It should
not yet be the only control in front of production infrastructure. This page is
the operator-facing summary; [`threat-model.md`](threat-model.md) is the
falsifiable version (every "mitigated" claim points at code or config), and
[`SECURITY.md`](../SECURITY.md) is how to report a vulnerability.

## What SkiMasque enforces

- **Identity is verified, not asserted.** A CI OIDC token is checked (RS256
  against the issuer's published keys, plus `iss` / `aud` / `exp`) before its
  claims become a `WorkloadIdentity`. A tunnel without a verifiable credential
  is refused (`407` missing, `403` unverifiable).
- **Deny by default.** No matching allow rule → DENY. There is no implicit
  "allow the rest", and a policy that fails to parse is ignored in favour of the
  last good one.
- **Short-lived access.** The platform credential defaults to a 1h TTL and is
  re-exchanged proactively; when the job ends nothing is left holding access.
- **The gateway enforces locally.** `identity + application + destination +
  policy + expiry` must all resolve to an allow, checked in the gateway — not
  assumed because a control plane said so. A control-plane outage never becomes
  a data-plane outage and never fails open.
- **An SSRF floor below policy.** Loopback / RFC 1918 / CGNAT / link-local
  (including `169.254.169.254`) / multicast are refused on the **resolved**
  address unless a range was explicitly opted in.
- **The application name is session context, not proof.** The gateway matches on
  it but does not trust it as authenticated. Stronger process identity is on the
  roadmap.
- **A tamper-evident audit trail.** Each decision is recorded; a control-plane
  deployment hash-chains the stream and rejects a break.

## What is your responsibility

| Concern | Yours to handle |
|---|---|
| **Firewall / reachability** | Your firewall is the outer boundary — it decides what the gateway *can* reach. SkiMasque decides *who* may use that. SkiMasque does not bypass your network controls. |
| **Gateway placement (Modes 2–3)** | where the gateway sits, its egress IP, and what that IP is allowlisted to reach |
| **Gateway host (Modes 2–3)** | OS patching, container image freshness, host access, secrets on the box (`--credential-secret`, TLS keys) |
| **`[match]` scoping** | an over-broad `[match]` silently widens access. Run `skimasque policy validate --strict` in CI. |
| **OIDC audience** | choose a stable, gateway-specific `aud` and mint each pipeline's token for exactly it |
| **Control plane (Mode 3)** | its TLS, its store, its admin token, its GitHub OAuth app, its backups; the per-org Ed25519 signing keys are **not recoverable if lost** |
| **Credential-secret rotation** | rotating `--credential-secret` invalidates every issued credential within the TTL — do this on suspected compromise |

## Trust boundaries

```
CI runner ──(OIDC token, trusted only after verification)──▶ gateway
gateway   ──(pulls policy; verifies signed credentials)────▶ control plane
gateway   ──(your firewall is still the boundary)──────────▶ destination
```

- The gateway trusts the **issuer's signing keys** (OIDC) and the **org's
  Ed25519 public key** (control-plane credentials) or its **`--credential-secret`**
  (standalone). It does not trust the client's claimed application, the
  destination, or a credential it cannot verify.
- The control plane is trusted for *policy content* and *credential signing*. It
  is **not** on the traffic path and cannot open or redirect a tunnel.
- In Mode 3 you also own the control plane's trust — its admin token gates org
  and policy management and the `/metrics` endpoint.

## Escape hatches (greppable, on purpose)

- `AuthorizedDestination::trusting(...)` — the only way to forward without a
  policy decision; used when no `PolicyLayer` is in the stack.
- `--allow-private` / `--allow-cidr` — opt ranges past the SSRF floor.
- `--insecure` (client) — accept any gateway certificate; gives up MITM defence.

## Reporting

Do **not** open a public issue. See [`SECURITY.md`](../SECURITY.md) — GitHub
private security advisories, plus a coordinated-disclosure process.
