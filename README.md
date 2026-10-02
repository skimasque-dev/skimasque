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

## Walkthrough: test your first CI connection

Create a GitHub Actions job that reaches `http://hello.skimasque.com:8080/`
through `gateway.skimasque.com:443`, then prove that an unlisted destination is
denied. The hello service uses private DNS and is reachable from the managed
gateway; your runner does not need to resolve or reach it directly.

### 1. Prepare your GitHub repository

Use a repository you can commit to and run Actions in. This guide uses
`YOUR_OWNER/YOUR_REPO`, the branch `main`, and the workflow file
`hello-skimasque.yml`. Substitute your real owner and repository everywhere;
replace `main` in both the policy and workflow if your default branch differs.
Enable GitHub Actions in the repository's **Settings → Actions → General** and
allow `skimasque-dev/connect` if your organisation restricts third-party Actions.
Use the GitHub-hosted `ubuntu-24.04` runner for this walkthrough.

No SkiMasque token or GitHub personal access token goes in repository secrets.
The job's `id-token: write` permission lets the Action request GitHub OIDC.

### 2. Create an organisation and verify the GitHub owner

Open the [SkiMasque console](https://control.skimasque.com/), sign in with GitHub,
and create or select your SkiMasque organisation. Under
[identity settings](https://control.skimasque.com/app/settings/identity), verify
the GitHub user or organisation that owns `YOUR_OWNER/YOUR_REPO`, following the
verification flow or installing the GitHub App. Confirm the owner is verified
in the selected SkiMasque organisation. Signing in alone does not associate all
your repositories with an organisation.

Use the shared managed gateway for this test; no gateway VM, private DNS setup
on the runner, or local CLI installation is required. The managed deployment
must have its hello test endpoint enabled.

### 3. Publish the test policy

Open [Policies](https://control.skimasque.com/app/policies) in that organisation
and open the policy editor. Under **Add a document**, enter `hello-ci.toml`,
leave the target field empty for the shared gateway, and paste:

```toml
name = "hello-ci"

[match]
repository = "YOUR_OWNER/YOUR_REPO"
workflow = "hello-skimasque.yml"
branch = "main"
kind = "ci"

[[rules]]
application = "skimasque-test"
transport = "tcp"
action = "allow"
destinations = ["hello.skimasque.com:8080"]

[[tests]]
application = "skimasque-test"
destination = "hello.skimasque.com:8080"
expect = "allow"

[[tests]]
application = "skimasque-test"
destination = "hello.skimasque.com:8081"
expect = "deny"
```

Replace `YOUR_OWNER/YOUR_REPO`, click **Save draft**, and check that validation
and both policy tests pass. Click **Publish revision**. Saving a draft alone
does not change gateway authorization. Under **Gateways**, confirm the shared
gateway is healthy and has acknowledged the published policy revision before
running the job.

New organisations may already have a `skimasque-test-OWNER.toml` starter policy
allowing `skimasque-test` to reach the hello service from that verified owner's
repositories. The policy above gives this walkthrough an explicit repository,
workflow and branch scope. If you want only that scope, remove the broader
starter document from the draft before publishing. Preserve unrelated policies;
ensure none allows `hello.skimasque.com:8081`, which is the denial test below.

### 4. Create the complete workflow

This repository includes the [hello workflow](.github/workflows/hello-skimasque.yml)
and its [hello CI policy](.github/policies/hello-ci.toml), scoped to
`skimasque-dev/skimasque` on `main`. Repository CI validates the policy; publish
it in the console before manually running the workflow on `main`.

Create `.github/workflows/hello-skimasque.yml` in your repository and paste:

```yaml
name: Hello through SkiMasque

on:
  workflow_dispatch:

jobs:
  hello:
    runs-on: ubuntu-24.04
    timeout-minutes: 5
    permissions:
      contents: read
      id-token: write
    steps:
      - name: Connect to the managed gateway
        id: network
        uses: skimasque-dev/connect@v2
        with:
          mode: proxy
          proxy: gateway.skimasque.com:443
          audience: https://gateway.skimasque.com
          application: skimasque-test

      - name: Reach the allowed hello service
        shell: bash
        env:
          SKIMASQUE_HTTP_PROXY: ${{ steps.network.outputs.http-proxy }}
        run: |
          curl --fail-with-body --show-error --silent \
            --connect-timeout 10 --max-time 30 \
            --proxy "$SKIMASQUE_HTTP_PROXY" --noproxy '' \
            http://hello.skimasque.com:8080/ | tee hello-response.txt
          grep -Fx 'private endpoint ok' hello-response.txt

      - name: Verify an unlisted port is denied
        shell: bash
        env:
          SKIMASQUE_HTTP_PROXY: ${{ steps.network.outputs.http-proxy }}
        run: |
          status=$(curl --show-error --silent \
            --connect-timeout 10 --max-time 30 \
            --proxy "$SKIMASQUE_HTTP_PROXY" --noproxy '' \
            --output denied-response.txt --write-out '%{http_code}' \
            http://hello.skimasque.com:8081/)
          test "$status" = 403 || {
            echo "Expected gateway policy denial (403), got $status"
            cat denied-response.txt
            exit 1
          }
          echo 'Gateway refused the unlisted port (403).'
```

Commit the file to your default branch (`main` here). A checkout step is not
needed because the job only uses the Action and curl. `mode: proxy` is explicit:
the Action defaults to transparent mode, which needs routes and DNS inputs.
The application name must match the policy; it is declared context, not proof
of the executable. The explicit proxy output and `--noproxy ''` ensure both
requests use the gateway even if the runner has inherited proxy exclusions.
The gateway resolves the hello hostname.

`@v2` selects the moving Action major release and, without `version`, the latest
client release. For reproducible production jobs, pin the Action to a reviewed
commit and set `with.version` to a compatible client release providing
`proxy-ready-v1`. Action and client versions are independent.

### 5. Run the job and check its output

In GitHub, open **Actions → Hello through SkiMasque → Run workflow**, select
`main` (or the branch you placed in the policy), and click **Run workflow**.
Open the run and expand the `hello` job's steps.

- **Connect to the managed gateway** must finish successfully: the Action
  exchanges GitHub OIDC and starts authenticated local proxy listeners.
- **Reach the allowed hello service** must print `private endpoint ok`, followed
  by `host:`, `seen client:` and `path: /`. The client address is the gateway's
  internal address. The body assertion and curl's HTTP error check must pass.
- **Verify an unlisted port is denied** must print
  `Gateway refused the unlisted port (403).` A timeout, DNS failure, connection
  refusal or 502 is a failed test, not evidence of policy denial.

The job should be green, including the Action's post-job cleanup. HTTP port
8080 is intentional; changing the hello URL to HTTPS or omitting the port tests
a different destination. The runner-to-gateway connection uses encrypted QUIC;
the demo service speaks HTTP on the gateway's internal network.

### 6. Verify the gateway's audit decisions

Open [Audit](https://control.skimasque.com/app/audit) in the same organisation.
Find the entries around the workflow run time and inspect the repository,
workflow `hello-skimasque.yml`, branch/ref `main` / `refs/heads/main`, workload
kind `ci`, and application `skimasque-test`. Confirm an **allow** for
`hello.skimasque.com:8080` and a **deny** for `hello.skimasque.com:8081`, with
the policy reason. Audit ingestion can take a short time; refresh if necessary.

Together, the expected response, explicit proxy path, 403 assertion and audit
decisions verify connectivity and enforcement. Curling the service directly
does not test gateway policy.

### 7. Troubleshoot a failed run

| Symptom | What to check |
|---|---|
| Workflow missing or no Run workflow button | Commit to the default branch, keep `workflow_dispatch`, and enable Actions. |
| OIDC or credential exchange fails | Keep job-level `id-token: write`, the exact audience `https://gateway.skimasque.com`, and a verified repository owner in the selected organisation. |
| Action cannot connect or times out | Check managed gateway health and UDP 443 reachability; QUIC requires UDP. Inspect the Action's startup diagnostics. |
| Hello request returns 403 | Check the published revision and gateway acknowledgement, actual repository/branch/workflow filename, `kind = "ci"`, application and port. Read the audit denial reason. |
| Hello request returns 502 | Check gateway-side DNS/reachability and that the managed hello endpoint is enabled. Do not add public runner DNS overrides for this private service. |
| Denial test returns something other than 403 | Check audit and other matching policies for a broader allow. A network failure does not prove authorization was denied. |
| Policy changes have no effect | Publish the saved draft, check the selected organisation and gateway revision, then rerun the job to create new tunnels. |

### 8. Clean up and adapt the job

The Action's post hook stops its client and restores previous proxy variables
on normal job success or failure. Remove `hello-ci.toml` (and any unused starter
test document) from the console draft and **publish** the removal when done.
Delete the workflow file if you no longer need the test.

To reach your own service, replace the destination and port in both policy and
workflow, set the application context consistently, and use a gateway that can
reach that service. The shared gateway does not automatically reach your VPC.
See [GitHub Actions](docs/github-actions.md) for proxy and transparent modes and
[deployment modes](docs/deployment-modes.md) for customer gateways.

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
