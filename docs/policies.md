# Policies

> **Applies to:** ✓ Mode 1 · ✓ Mode 2 · ✓ Mode 3

A policy answers `WHO → WHAT → WHERE → LIMITS`, and its first principle is:

> **No matching allow rule means DENY.** There is no implicit "allow the rest".

The policy engine (`skimasque-policy`) does no I/O and speaks no HTTP — it is a
pure function from a request to a decision, so policies are testable and
diffable in CI, the same way in every deployment mode.

## The document

Two formats over one model — an ordered rule table (TOML) or one application
with an allow-list (YAML). They produce the same `Policy`.

```toml
name = "production"

# --- WHO: the identity this policy governs. This IS the authorization
#     boundary — an over-broad match silently widens access.
[match]
repository = "acme/widget"
workflow = "deploy.yml"     # optional
branch = "main"             # a fork's PR branch will not match

# --- WHAT + WHERE: an ordered list of rules over application + destinations.
[[rules]]
id = "terraform-prod"       # optional, shows up in decisions and audit
application = "terraform"
action = "allow"            # allow | deny
destinations = [
    "api.production.example.com:443",
    "*.terraform.io:443",
    "db.production.example.com:5432",
]

[[rules]]
application = "terraform"
transport = "udp"           # tcp | udp | any (default any) — scopes the rule
action = "allow"
destinations = ["10.0.0.2:53"]

# --- LIMITS
[session]
max_duration = "20m"

[limits]
bandwidth = "100Mbps"
packets_per_second = 20000
connections = 50
bytes = "5GiB"

# --- [[tests]]: assertions that travel with the policy and run in CI
[[tests]]
application = "terraform"
destination = "api.production.example.com:443"
expect = "allow"

[[tests]]
application = "terraform"
destination = "secrets.internal:443"
expect = "deny"
```

The YAML form of the same idea:

```yaml
name: production
identity:
  repository: acme/widget
  branch: main
application:
  name: terraform
network:
  transport: any           # applies to both lists
  allow:
    - api.production.example.com:443
    - "*.terraform.io:443"
limits:
  bandwidth: 100Mbps
  connections: 50
```

## WHO — the `[match]` block

Fields, matched against the `WorkloadIdentity` the gateway derives from the
runner's OIDC token:

| Field | From (GitHub Actions) | Example |
|---|---|---|
| `organization` | `repository_owner` | `acme` |
| `repository` | `repository` | `acme/widget` |
| `workflow` | `workflow_ref` (file name) | `deploy.yml` |
| `branch` / `ref` | `ref` | `main` / `refs/heads/main` |
| `environment` | `environment` | `production` |
| `actor` | `actor` | `octocat` |

Because `[match]` *is* the authorization boundary, `skimasque policy validate`
flags the common mistakes — an empty `[match]`, no `repository`, or a
`repository` pinned without a `branch` / `ref` (so a fork's PR branch matches).
`--strict` fails CI on them; the gateway logs the same warnings on load.

## WHERE — destinations

Each destination is `host:port`. The host accepts:

- an exact name — `db.prod.example.com`
- a `*.suffix` wildcard — `*.terraform.io`
- a bare IP — `10.0.0.2`
- a CIDR — `10.0.0.0/8`
- `*` — any

The port accepts an exact number, a range (`8000-8100`), or `*`.

The match is on the name the client asked for, **before DNS**. Independently,
the gateway's SSRF floor (`AddressPolicy`) re-checks the *resolved* address and
refuses loopback / RFC 1918 / CGNAT / link-local / multicast unless the
deployment opted that range in with `--allow-cidr` — see
[`gateways.md`](gateways.md#the-ssrf-floor).

## WHAT — application

The name the client declares (`--app terraform`, or the action's `application:`
input). The gateway matches on it but **does not trust it as an authenticated
fact** — it is session context. Stronger process identity is on the roadmap.

## Transport

A rule with no `transport` covers both TCP and UDP tunnels — which matches how
operators think about "what may this workload reach". `transport = "tcp"` or
`"udp"` narrows a rule to one tunnel kind, so a rule written for UDP DNS does
not silently also permit TCP to that host.

## LIMITS

| Key | Scope | Meaning |
|---|---|---|
| `[session] max_duration` | per session | the credential lifetime cap |
| `[limits] bandwidth` | aggregate across the policy | a shared token bucket the relay paces every tunnel against |
| `[limits] packets_per_second` | aggregate | as above |
| `[limits] connections` | aggregate | concurrent-tunnel cap (a semaphore) |
| `[limits] bytes` | per tunnel | the relay closes a tunnel at this transfer ceiling |

## Working with policy

```console
$ skimasque init                         # scaffold .masque/policies/
$ skimasque policy check <policy> <host:port> --app <name>   # one decision, no network
$ skimasque policy test                  # run every [[tests]] assertion
$ skimasque policy validate --strict     # lint [match]; fail CI on a broad one
$ skimasque policy explain <policy>       # human-readable summary of the rules
$ skimasque policy diff <old-dir> <new-dir>   # what changed between two policy sets
$ skimasque policy learn observations.jsonl --name production --repository acme/widget --branch main
```

`skimasque why <host:port> --app <name> --repository <r> --branch <b>` prints the
decision a gateway would log for that exact request — locally, or with
`--control-plane` against the org's published policy.

### Learning mode

Run a gateway in observe mode (`--policy-observe`), collect the `masque::observe`
events into a file of
`{"application": "…", "transport": "tcp", "destination": "host:port"}` objects,
then `skimasque policy learn` turns them into a reviewable draft — one rule per
distinct `(application, transport, destination)`. Review before committing: an
*observed* destination is not a *trusted* one.

## In CI

```yaml
- run: skimasque policy validate --strict
- run: skimasque policy test
# review the change against what's deployed:
- run: git worktree add /tmp/base origin/main
- run: skimasque policy diff /tmp/base/.masque/policies .masque/policies
```
