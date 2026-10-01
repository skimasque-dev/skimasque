# SkiMasque

**Identity-aware network access for CI jobs, developers and coding agents.**

Give a CI/CD job exactly the network access it needs — and nothing else, only
for as long as it needs it — without putting it on a broad VPN or handing it a
standing set of credentials.

A workload proves *who* it is (a GitHub Actions job proves its repository,
workflow, and branch with an OIDC token), declares *what* it is running, and
asks to reach a *destination*. A policy decides — **no matching allow rule means
DENY** — and if it allows, the job gets a short-lived, identity-bound tunnel to
that one destination through an enforcement gateway. The client closes its tunnels when it stops; agent sessions also have enforced
expiry and revocation. Traffic must use the gateway for its policy to apply.

```
GitHub Actions job
      │  workload identity  (OIDC: acme/widget, deploy.yml, refs/heads/main)
      ▼
identity + policy            WHO → WHAT → WHERE → LIMITS
      │  short-lived authorization
      ▼
MASQUE gateway               enforces; deny by default
      │
      ▼
db.prod:5432                 (allowed 20m, 100 Mbps — and nothing else)
```

---

## Why

- A CI job that needs one database gets a VPN that reaches your whole network.
- Static credentials in CI secrets don't expire and are hard to rotate.
- IP allowlists say *where from*, never *who* or *which workflow*.
- "Temporary" access is rarely torn down.
- Different repos and workflows need different access, and that's tedious to
  express with the tools above.

SkiMasque replaces all of that with a policy over workload identity and a tunnel
that only opens when the policy says so.

---

## Quick start

Sign in with `skimasque login`, create an organisation, verify its GitHub owner,
and publish a policy in the console. Then run a command with its allowed access:

```console
skimasque exec --app curl -- curl https://api.staging.example.com/health
skimasque exec --forward 15432:db.internal:5432 -- psql -h 127.0.0.1 -p 15432
```

For CI, the [connect Action](https://github.com/skimasque-dev/connect) exchanges
GitHub OIDC for a short-lived credential and renews it while the job runs:

```yaml
permissions:
  id-token: write
  contents: read
steps:
  - uses: skimasque-dev/connect@v1
    with:
      mode: proxy
      proxy: gateway.skimasque.com:443
      audience: https://gateway.skimasque.com
      application: curl
  - run: curl --fail https://api.staging.example.com/health
```

Use coordinated Action/client releases that include these interfaces. Proxy mode
sets HTTP, HTTPS and SOCKS variables for tools that support them. Transparent mode
on dedicated Ubuntu runners routes configured private TCP/UDP and split DNS
through the client's native TUN stack; it requires explicit networks and DNS.
`psql` needs transparent routing or a local forward, not `ALL_PROXY`.

Choose a gateway that can reach the destination. The shared Cloud gateway does
not automatically reach your private VPC. Follow [getting started](docs/getting-started.md)
and [GitHub Actions](docs/github-actions.md) for policy and network setup.

### Coding agents

`skimasque exec --agent` starts a revocable session for one command. Choose
`--sandbox srt` with allowed domains, or explicitly use `--unsandboxed`.
Standalone `agent-session start/list/end` commands support external sandboxes and
delegation. Sessions default to 30 minutes, are capped at 4 hours (or a lower org
limit), and ending a parent ends its descendants. Gateway revocation depends on
receiving control-plane updates; credential expiry bounds access during outages.
See [agents](docs/agents.md).

---

## The three deployment modes

**Start managed. Own more when you need to.** All three use the same policy
model, the same gateway, and the same GitHub Action.

|  | Mode 1 — Fully managed | Mode 2 — Customer gateway | Mode 3 — Fully self-hosted |
|---|---|---|---|
| Control plane | SkiMasque | SkiMasque | **You** |
| Gateway | SkiMasque | **You** | **You** |
| You control the egress IP | No | Yes | Yes |
| You operate a gateway | No | Yes | Yes |
| You operate a control plane | No | No | Yes |
| Operational load | Lowest | Medium | Highest |
| SkiMasque Cloud required | Yes | Yes | No |
| Best for | Fastest adoption | Traffic path in your network | Maximum control / air-gapped |

- **Mode 1** — *"Just use it."* SkiMasque runs everything; you configure identity
  and policy. Start here.
- **Mode 2** — *"Keep the network path in my infrastructure."* You run the
  open-source gateway (`skimasque-server`) in your VPC; SkiMasque Cloud provides
  identity, policy, and the dashboard.
- **Mode 3** — *"Operate everything myself."* The advanced path, for air-gapped
  or heavily regulated environments. You run the gateway **and** a control plane
  that speaks the [control protocol](docs/protocol.md) — which you implement, or
  licence from SkiMasque. Real work; not a config switch.

See [`docs/deployment-modes.md`](docs/deployment-modes.md).

---

## Open source vs SkiMasque Cloud

SkiMasque's **enforcement edge** is open source — the gateway, the client, the
CLI, the Action, the policy engine, and the protocol a gateway speaks to a
control plane. **SkiMasque Cloud** — the control-plane implementation, the
managed policy workflow, the dashboard, audit, support — is proprietary, and is
what almost every team should use (Modes 1 and 2).

| Open source (this repo, MIT OR Apache-2.0) | SkiMasque Cloud (proprietary) |
|---|---|
| The MASQUE transport stack (`skimasque-core`, `skimasque`) | The managed control plane — hosted, kept available, upgraded |
| The gateway (`skimasque-server`) | Managed gateways and global egress (Mode 1) |
| The tunnel client (`skimasque-client`) and CLI (`skimasque`) | The web dashboard: who has access, is it working, why was this denied |
| The policy **engine** (`skimasque-policy`) — evaluation, deny-by-default, `check` / `test` / `explain` / `learn` | The managed policy **workflow** — reviewed revisions, diffs, simulation, access requests, history |
| OIDC verification (`skimasque-identity`) | Organizations, teams, roles, membership |
| The **control protocol** (`skimasque-protocol`) — the Gateway↔control-plane contract | Audit history, usage metering, notifications, support |
| The GitHub Action ([`skimasque-dev/connect`](https://github.com/skimasque-dev/connect)) | |

The open-source gateway is **not** crippled — it does full policy enforcement,
deny-by-default, and the SSRF floor on its own, and it keeps enforcing through a
control-plane outage. What SkiMasque Cloud sells is *operating* the control
plane: hosting, availability, upgrades, the dashboard, managed gateways and
egress, and support. Mode 3 (running your own control plane) is possible because
the [protocol](docs/protocol.md) is documented — but there is no open-source
control-plane server, and it is the advanced path, not a default.

---

## Principles

- **Least privilege by default.** No matching allow rule means DENY. There is no
  implicit "allow the rest".
- **Fail closed.** If authentication, authorization, or destination validation
  can't be established, no tunnel is created.
- **The gateway enforces; it does not trust.** A valid tunnel credential is not
  enough — identity + application + destination + policy + expiry must all
  produce an allow, checked in the gateway, not assumed because a control plane
  said so.
- **Policy is separate from MASQUE.** The engine (`skimasque-policy`) does no I/O
  and speaks no HTTP; it is a pure function from a request to a decision, so
  policies are testable and diffable in CI.
- **Don't make developers learn networking.** The questions are *who is running,
  what application, and what does it need to reach.*

---

## Policy

A policy is `WHO → WHAT → WHERE → LIMITS`. The same policy in both supported
formats — deny-by-default is implicit in both:

<table>
<tr><th>TOML (ordered rule table)</th><th>YAML (one application, an allow-list)</th></tr>
<tr><td>

```toml
name = "production"

[match]                       # WHO
repository = "acme/widget"
branch = "main"

[[rules]]
application = "terraform"     # WHAT
action = "allow"
destinations = [              # WHERE
    "api.production.example.com:443",
    "*.terraform.io:443",
]

[[rules]]
application = "terraform"
transport = "udp"
action = "allow"
destinations = ["10.0.0.2:53"]

[session]                     # LIMITS
max_duration = "20m"
[limits]
bandwidth = "100Mbps"
connections = 50

[[tests]]                     # travels with the policy, runs in CI
application = "terraform"
destination = "api.production.example.com:443"
expect = "allow"
```

</td><td>

```yaml
name: production

identity:
  repository: acme/widget
  branch: main

application:
  name: terraform

network:
  transport: any
  allow:
    - api.production.example.com:443
    - "*.terraform.io:443"

limits:
  bandwidth: 100Mbps
  connections: 50
```

</td></tr>
</table>

Destinations take an exact host, a `*.suffix` wildcard, an IP, a CIDR, or `*`,
each with an exact port, a range, or `*`. Check policy locally, with no network:

```console
$ skimasque policy check production google.com:443 --app terraform
DENY

Reason:
No matching allow rule.

Suggested rule:
  allow terraform google.com:443

$ skimasque policy test          # run the [[tests]] in CI
```

Full DSL: [`docs/policies.md`](docs/policies.md).

---

## What works today

### MASQUE transport

| Specification | Status |
|---|---|
| [RFC 9297](https://www.rfc-editor.org/rfc/rfc9297) — HTTP Datagrams and the Capsule Protocol | Complete, in both encodings |
| [RFC 9298](https://www.rfc-editor.org/rfc/rfc9298) — Proxying UDP in HTTP | Complete over HTTP/3 extended `CONNECT` |
| [`draft-ietf-httpbis-connect-tcp`](https://datatracker.ietf.org/doc/draft-ietf-httpbis-connect-tcp/) — Proxying TCP in HTTP | Classic `CONNECT host:port`, on by default (`--no-connect-tcp` for UDP-only); the template-driven variant waits on `h3` support |
| [RFC 9484](https://www.rfc-editor.org/rfc/rfc9484) — Proxying IP in HTTP | Wire formats complete; transport behind the `connect-ip` feature, TUN forwarding not wired |
| [RFC 1928](https://www.rfc-editor.org/rfc/rfc1928) — SOCKS5 | `CONNECT` (TCP) and `UDP ASSOCIATE`, as a front end for applications |

### Policy engine, identity, and gateway enforcement

Deny-by-default evaluation with self-explaining denials; `[[tests]]` that run in
CI; `[match]` linting; learning mode. CI OIDC verification (GitHub Actions incl.
Enterprise Server, GitLab, Buildkite, generic) → `WorkloadIdentity` → a
short-lived platform credential the gateway verifies locally with no network. An
`AddressPolicy` SSRF floor checked against *resolved* addresses. `[limits]`
enforcement (bandwidth / packets-per-second / connection / byte caps). Policy and
certificate hot-reload without dropping tunnels. In-process ACME (Let's Encrypt).
Prometheus `/metrics` + `/healthz` + `/readyz`.

Full detail: [`docs/architecture.md`](docs/architecture.md).

---

## Crates

| Crate | What |
|---|---|
| `skimasque-core` | wire formats: QUIC varints, HTTP Datagrams, the Capsule Protocol, CONNECT-UDP / CONNECT-IP payloads. No I/O. |
| `skimasque-policy` | the policy engine: `WorkloadIdentity`, `Policy` / `PolicySet`, TOML+YAML parsers, the evaluator, policy tests, learning mode. No I/O. |
| `skimasque-identity` | verifies a CI OIDC token and maps its claims to a `WorkloadIdentity`; issues/verifies the platform credential. |
| `skimasque-protocol` | the Gateway↔control-plane contract: wire types, endpoint paths, `PROTOCOL_VERSION`. Pure data. |
| `skimasque` | the transport (`quinn` + `h3`), the proxy as a `tower::Service` (`IdentityLayer` / `PolicyLayer` / `QuotaLayer` / `AuthorizeLayer`), the token-exchange endpoint. |
| `skimasque-cli` | three binaries — `skimasque` (CLI), `skimasque-server` (gateway), `skimasque-client` (tunnel client). |

---

## Current capabilities and limits

- HTTP/3 over QUIC with TCP CONNECT and CONNECT-UDP; HTTP/HTTPS and SOCKS5
  local proxies, command-scoped access and local TCP forwards.
- Native Linux TUN forwarding for configured TCP/UDP routes. This translates
  flows into MASQUE TCP/UDP tunnels; it is separate from RFC 9484 CONNECT-IP.
- Verified workload kinds (`ci`, `developer`, `agent`), TOML/YAML policy,
  baseline guardrails, offline policy checks and explanations.
- Control-plane policy revisions, shared/customer gateways, audit, organisation
  membership, GitHub owner verification and per-org signing-key rotation.
- Agent sessions, delegation, per-kind credential ceilings, revocation and
  optional integration with an external sandbox runtime.
- Usage/plan enforcement and Stripe checkout, portal and webhooks in the private
  control plane, enabled by deployment configuration.

Not implemented: strong process identity, connection profiles, general IP/ICMP
forwarding, HTTP/1.1 or HTTP/2 MASQUE transports, and RFC 9484 TUN forwarding.
`[session] max_duration` is policy metadata, not a gateway-enforced timeout.
CI/developer credential expiry gates new tunnels; agent expiry also closes active
tunnels. Resource quotas are per tunnel or policy, not a single shared session
budget. Dedicated Cloud egress and public incident reporting remain planned.

## Documentation

Start with [`docs/getting-started.md`](docs/getting-started.md), then
[`docs/deployment-modes.md`](docs/deployment-modes.md) to pick a mode.
[`docs/`](docs/README.md) is the full index.

## Development

Requires Rust **1.88+**. See [`docs/development.md`](docs/development.md).

```console
$ cargo test --workspace
$ cargo clippy --workspace --all-targets
$ cargo deny check
```

## Website

The website in [`site/`](site/) is generated by
`cargo run -p skimasque-visual --features site --bin sitegen`; CI runs it with `--check` and fails if `site/` is out of date. It holds the 21 public pages (the home page and 20 routes), the component gallery under `site/components/`, `robots.txt` (there is no `sitemap.xml`; it would need an absolute base URL) and the shared stylesheet. `site/favicon.svg` is hand-written and left alone.

## Security

See [`SECURITY.md`](SECURITY.md) for reporting, and
[`docs/security.md`](docs/security.md) / [`docs/threat-model.md`](docs/threat-model.md)
for the trust boundaries.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Contributions are dual-licensed the
same way; see [`CONTRIBUTING.md`](CONTRIBUTING.md).
