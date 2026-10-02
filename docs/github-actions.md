# GitHub Actions

For a complete first run, follow [the hello CI walkthrough](getting-started.md):
verify your GitHub owner, publish a policy, create a workflow reaching
`hello.skimasque.com:8080` through `gateway.skimasque.com`, and check both an
allowed request and a policy denial.

> **Applies to:** ✓ Mode 1 · ✓ Mode 2 · ✓ Mode 3 — the workflow is identical in
> all three; only the gateway's `proxy` / `audience` change.

A workflow job proves its identity to a SkiMasque gateway with a **GitHub OIDC
token**, and exchanges it for a short-lived **platform credential** that
its tunnels present. The gateway evaluates policy against the identity in that
credential, so a policy can say "the `deploy.yml` workflow on `main` of
`acme/widget` may reach `db.production:5432`" and nothing else can borrow the
access.

GitHub Actions is the first-class path; GitLab CI, Buildkite, and generic OIDC
issuers work the same way — see [Other CI systems](#other-ci-systems).

## The flow

```
runner  ──OIDC token──▶  gateway /.well-known/masque/skimasque-credential
                              │  verify RS256 · iss · aud · exp
                              │  claims ─▶ WorkloadIdentity
                         ◀──credential──  standalone: HS256; managed: org Ed25519 credential
runner  ──credential──▶  gateway  (every tunnel, in Proxy-Authorization: Bearer)
                              │  verify locally — no network
                              │  WorkloadIdentity ─▶ policy ─▶ ALLOW / DENY
```

Verifying OIDC costs a JWKS lookup and an RSA check, and the token is minted for
a broad audience. The exchange pays that once; tunnels then carry a credential
the gateway verifies locally.

### What the gateway checks at the exchange

`skimasque-server --oidc` (or `--github-oidc`):

1. reads the `kid` from the JWT header and finds the matching key in the
   issuer's JWK Set (discovered from `--oidc-issuer`, cached for an hour,
   refetched once on an unknown `kid`);
2. verifies the RS256 signature;
3. checks `iss` equals `--oidc-issuer`
   (`https://token.actions.githubusercontent.com` by default);
4. checks `aud` is one of the `--oidc-audience` values;
5. checks `exp` / `nbf` with 60s of leeway.

On success the claims become a `WorkloadIdentity`:

| Claim | Identity field | Example |
|---|---|---|
| `repository_owner` | `organization` | `acme` |
| `repository` | `repository` | `acme/widget` |
| `workflow_ref` (file name) | `workflow` | `deploy.yml` |
| `ref` | `git_ref` (→ `branch`) | `refs/heads/main` |
| `environment` | `environment` | `production` |
| `actor` | `actor` | `octocat` |

## Running the gateway

```console
$ skimasque-server \
    --listen 0.0.0.0:4433 --authority masque.example:4433 \
    --github-oidc --oidc-audience https://masque.example \
    --policy-dir .masque/policies
```

TCP tunnels (classic `CONNECT`, which is what carries HTTPS, git and database
traffic) are served by default alongside UDP; pass `--no-connect-tcp` for a
UDP-only gateway.

`--github-oidc` replaces `--auth-token` and turns on the exchange endpoint. A
tunnel without a verifiable credential is refused.

- **`--oidc-audience <AUD>`** (repeatable, required) — the value the OIDC token's
  `aud` must carry. Pick a stable name for the gateway; its public URL is a good
  choice. Every workflow requests its token for exactly this string.
- **`--credential-secret <HEX>`** (or `SKIMASQUE_CREDENTIAL_SECRET`) — the HS256
  key credentials are signed with. Set the same value on every gateway in a
  fleet so a credential one issues is accepted by another. Unset means a random
  key per start: fine for a single gateway, and clients just re-exchange after a
  restart.
- **`--credential-ttl <DURATION>`** — credential lifetime, default `1h`. The
  client re-runs the exchange ahead of each expiry and installs the new
  credential on the live session, so a job that outlasts the TTL keeps working;
  a longer TTL just means fewer refreshes.
- **`--oidc-issuer <URL>`** — defaults to the provider's hosted issuer. For
  GitHub Enterprise Server, point it at `https://<host>/_services/token`.

`--github-oidc` is an alias for `--oidc` (with `--oidc-provider github`, the
default).

## Other CI systems

The gateway maps a different set of claims for each provider; verification (the
RS256 signature and `iss` / `aud` / `exp` checks) is identical.

```console
$ skimasque-server --oidc --oidc-provider gitlab \
    --oidc-audience https://masque.example ...
```

| `--oidc-provider` | Default issuer | Identity from |
|---|---|---|
| `github` | `token.actions.githubusercontent.com` | `repository_owner` / `repository` / `workflow_ref` / `ref` / `environment` / `actor` |
| `gitlab` | `gitlab.com` | `namespace_path` / `project_path` / `ci_config_ref_uri` / `ref`+`ref_type` / `environment` / `user_login` |
| `buildkite` | `agent.buildkite.com` | `organization_slug` / `pipeline_slug` / `build_branch` |
| `generic` | *(none — pass `--oidc-issuer`)* | the claims named by `--oidc-claim FIELD=CLAIM` |

- **Self-managed GitLab / GitHub Enterprise Server:** the provider stays the
  same, pass `--oidc-issuer https://<your-host>`.
- **Generic:** `--oidc-provider generic --oidc-issuer <URL> --oidc-claim
  repository=<claim> --oidc-claim ref=<claim> ...` for any OIDC issuer. Fields:
  `organization`, `repository`, `workflow`, `ref`, `environment`, `actor`.

The client side: `--github-oidc` auto-fetches the runner's token. For anything
else, hand `skimasque-client` the token you obtained from your CI's OIDC feature:

```yaml
# GitLab .gitlab-ci.yml
deploy:
  id_tokens:
    SKIMASQUE_OIDC: { aud: https://masque.example }
  script:
    - skimasque-client --proxy masque.example:4433
        --oidc-token "$SKIMASQUE_OIDC" --oidc-audience https://masque.example
        --app terraform socks5 --listen 127.0.0.1:1080 &
    - ALL_PROXY=socks5h://127.0.0.1:1080 terraform apply
```

## In the workflow

The [connect Action](https://github.com/skimasque-dev/connect) is a Node 24 action
with a supervised client and an always-running post hook. Use a runner supporting
Node 24 and coordinated Action/client releases with `proxy-ready-v1`; transparent
mode also requires `proxy-tun-v1`.

### Proxy-aware tools

```yaml
jobs:
  check-api:
    runs-on: ubuntu-24.04
    permissions:
      id-token: write
      contents: read
    steps:
      - uses: skimasque-dev/connect@v2
        with:
          mode: proxy
          proxy: gateway.skimasque.com:443
          audience: https://gateway.skimasque.com
          application: curl
      - run: curl --fail https://api.staging.example.com/health
```

Proxy mode exports uppercase/lowercase HTTP_PROXY, HTTPS_PROXY, ALL_PROXY and
NO_PROXY. HTTP/HTTPS use the HTTP proxy (`http://127.0.0.1:8080`); SOCKS uses
`socks5h://127.0.0.1:1080` with gateway-side hostname resolution. Raw sockets are
not intercepted. Use transparent mode or `skimasque exec --forward` for `psql`.

### Private TCP/UDP without proxy support

Transparent mode is the default. Configure all private destination CIDRs and
split DNS explicitly:

```yaml
- uses: skimasque-dev/connect@v2
  with:
    mode: transparent
    proxy: gateway.example.com:443
    audience: https://gateway.example.com
    application: psql
    routes: 10.42.0.0/16,fd42::/48
    dns-servers: 10.43.0.53,fd43::53
    dns-domains: ~internal.example
    probe-target: db.internal.example:5432
- run: psql -h db.internal.example -d app -f migrations/latest.sql
```

Replace every network setting and configure a gateway inside, or connected to,
that network. The gateway currently needs an IPv4 address. Destinations can use
IPv4 and IPv6: list both families where services have A and AAAA records.

Use a dedicated Ubuntu runner with `/dev/net/tun`, `ip`, `unzip`, `flock`, a
working systemd-resolved stub at `127.0.0.53`, and root or passwordless sudo.
Remove inherited proxy variables. Unsupported transparent runners fail before
network changes; select `mode: proxy` explicitly to use proxies instead.

The Action owns the TUN link, routing rules/table and per-link DNS. DNS servers
receive routes even outside `routes`. Default routes, `~.`, loopback, link-local
and gateway-overlapping ranges are rejected. Native forwarding carries IP
destinations: allow the private IP/CIDR and ports in policy, including DNS over
TCP/UDP, and permit these ranges with the gateway's `--allow-cidr` address floor.
Hostname-only rules cannot authorize native IP flows. ICMP and arbitrary IP
protocols are unsupported. Public traffic remains on ordinary runner routing.

### Readiness, releases and cleanup

The Action checks client capabilities, authentication and listener readiness.
Transparent mode also checks TUN attachment, routes and DNS; `probe-target` adds
a real TCP connection check. It supervises the client through the job. Private
routes become unreachable if the client dies, until cleanup removes them.

The post hook cleans up owned processes, DNS, rules, routes and links, and restores
previous proxy variables in proxy mode. Startup failures roll back. Forced runner
kills can prevent hooks; use ephemeral runners or the Action's `stop.cjs` and
`state-file` manifest before reusing a persistent runner.

Inputs and defaults are listed in [configuration](configuration.md). Outputs are
`mode`, `http-proxy`, `socks-proxy` and `state-file`. Pin `version` to a compatible
client release or pass `client-bin` for a local build. Named connection profiles
(`connection: staging`) are planned; current workflows use explicit inputs.

### Other clients

The client fetches GitHub OIDC with `--github-oidc`; other providers supply
`--oidc-token`. Run `proxy` for HTTP/SOCKS listeners, `connect --target HOST:PORT`
for a raw TCP stream, or `probe` for UDP diagnostics. `probe --text ping` is not
a Postgres connectivity test. See [CLI](cli.md).

## Policy

The identity fields above are what `[match]` constrains:

```toml
name = "widget-production"

[match]
repository = "acme/widget"
workflow = "deploy.yml"
branch = "main"

[[rules]]
application = "terraform"
action = "allow"
destinations = ["db.production.example.com:5432", "*.terraform.io:443"]

[[rules]]
application = "terraform"
transport = "udp"                       # a separate rule for DNS over the tunnel
action = "allow"
destinations = ["10.0.0.2:53"]
```

A rule with no `transport` matches both TCP and UDP tunnels; add
`transport = "tcp"` or `"udp"` to scope it. A token from a pull-request branch,
a different workflow, or another repository does not match this policy, and —
with no other policy matching — is denied.

Because the `[match]` block *is* the authorization boundary, a match that is
too loose silently widens access. `skimasque policy validate` flags the common
cases — an empty `[match]`, one with no `repository`, or one that pins a
`repository` but no `branch` / `ref` (so a fork's pull-request branch would
match) — and the gateway logs the same warnings when it loads a policy set. Run
`skimasque policy validate --strict` in the pipeline that publishes policies to
fail the build on them.

## Proving it

`.github/workflows/e2e-oidc.yml` runs the whole path on every push: it starts a
gateway with `--github-oidc`, which does real discovery and JWKS verification
against `token.actions.githubusercontent.com`, and drives traffic through it
with the runner's own OIDC token — including the negative cases (wrong audience,
unlisted destination).
