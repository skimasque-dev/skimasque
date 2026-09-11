# GitHub Actions

> **Applies to:** ✓ Mode 1 · ✓ Mode 2 · ✓ Mode 3 — the workflow is identical in
> all three; only the gateway's `proxy` / `audience` change.

A workflow job proves its identity to a SkiMasque gateway with a **GitHub OIDC
token**, and trades it — once — for a short-lived **platform credential** that
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
                         ◀──credential──  HS256 JWT, ~1h, signed by the gateway
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

The job needs `id-token: write`. Use the
[`skimasque-dev/connect`](https://github.com/skimasque-dev/connect) composite
action:

```yaml
jobs:
  deploy:
    runs-on: ubuntu-latest
    permissions:
      id-token: write
      contents: read
    steps:
      - uses: skimasque-dev/connect@v1
        with:
          proxy: masque.example:4433
          audience: https://masque.example
          application: terraform

      # ALL_PROXY now points at a local SOCKS5 relay through the gateway.
      - run: terraform apply -auto-approve
```

Pin an exact client version with `with: { version: vX.Y.Z }`; `@v1` otherwise
tracks the latest `skimasque` release.

The action downloads `skimasque-client` for the runner (from the
`skimasque-dev/skimasque` release), then runs it with `--github-oidc`: the client
fetches the OIDC token from the runner, exchanges it, starts
`skimasque-client … socks5` in the background, and exports
`ALL_PROXY=socks5h://127.0.0.1:1080`. Tools that honour `ALL_PROXY` (curl, git,
most cloud SDKs) egress through the gateway, subject to policy.

The SOCKS5 relay serves both `CONNECT` (TCP — HTTPS, git, database and cloud-SDK
traffic) and `UDP ASSOCIATE` (DNS and other UDP). Both are on by default; a
gateway started with **`--no-connect-tcp`** answers `CONNECT` with a SOCKS
`command not supported` reply and only UDP egresses.

Action inputs: `proxy`, `audience` (required); `authority`, `application`, `ca`,
`listen`, `version`, `repository`, `client-bin` (optional). Pass `client-bin` to
use a binary you built or installed yourself and skip the download.

A complete, runnable example against the live SkiMasque Cloud gateway
(`gateway.skimasque.com`, governed from `https://control.skimasque.com`) is
[`examples/github-actions/managed-postgres-migration.yml`](../examples/github-actions/managed-postgres-migration.yml)
— copy it, edit the two marked lines, publish the matching policy, done.

### Without the action

`skimasque-client` does the whole exchange itself:

```yaml
      - name: Open the tunnel
        run: |
          skimasque-client --proxy masque.example:4433 \
            --github-oidc --oidc-audience https://masque.example --app terraform \
            probe --target db.production.example.com:5432 --text ping
```

`--github-oidc` reads `ACTIONS_ID_TOKEN_REQUEST_URL` / `…_TOKEN` from the runner.
Pass `--oidc-token <jwt>` instead to supply a token you fetched another way.

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
