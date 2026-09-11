# Getting started

> **Applies to:** ✓ Mode 1 (this page) · Mode 2 → [`gateways.md`](gateways.md) · Mode 3 → [`self-hosting.md`](self-hosting.md)

By the end you will have a GitHub Actions job reaching **one protected
destination** through a policy — and denying everything else. About 5 minutes
with [SkiMasque Cloud](deployment-modes.md#mode-1--fully-managed) (Mode 1): no
infrastructure to run.

```
GitHub Actions job                      SkiMasque Cloud               your resource
  OIDC token  ──exchange──▶  gateway.skimasque.com ──policy: ALLOW──▶  db.internal:5432
  credential  ──tunnel────▶     (deny by default)          ✗────▶  anything else
```

## 1. Sign in and create an org

```console
$ skimasque login                 # opens the GitHub device flow; SkiMasque Cloud by default
$ skimasque org create "Acme"
```

## 2. Write and publish a policy

Scaffold one, edit it for your repo and target, and check it locally — no
network:

```console
$ skimasque init                  # writes .masque/policies/example.toml
```

```toml
# .masque/policies/production.toml
name = "production"

[match]
repository = "your-org/your-repo"   # who this policy governs
branch = "main"                     # a fork's PR branch will not match

[session]
max_duration = "15m"

[[rules]]
id = "db"
application = "psql"                # the name the job declares
action = "allow"
destinations = ["db.internal:5432"]

[[tests]]                           # travels with the policy, runs in CI
application = "psql"
destination = "db.internal:5432"
expect = "allow"

[[tests]]
application = "psql"
destination = "secrets.internal:443"
expect = "deny"
```

```console
$ skimasque policy test
production
  ok   psql -> db.internal:5432        (expect allow)
  ok   psql -> secrets.internal:443    (expect deny)

$ skimasque policy validate --strict   # fail CI if a [match] is too broad
```

Publish it: in the dashboard (`https://control.skimasque.com`) open your org →
**Access → Policy editor**, paste the document, and **Publish** it as revision 1.
(Or keep it in `.masque/policies/` and let CI publish it — see
[`github-actions.md`](github-actions.md).)

## 3. Wire the workflow

The job needs `id-token: write` to mint an OIDC token.

```yaml
jobs:
  migrate:
    runs-on: ubuntu-latest
    permissions:
      id-token: write
      contents: read
    steps:
      - uses: actions/checkout@v4

      - uses: skimasque-dev/connect@v1
        with:
          proxy: gateway.skimasque.com:443
          audience: https://gateway.skimasque.com
          application: psql

      - name: Run the migration through the gateway
        env:
          ALL_PROXY: socks5h://127.0.0.1:1080
        run: psql "postgresql://ci@db.internal:5432/app" -f migrations/latest.sql
```

`socks5h://` (with the `h`) resolves the destination name gateway-side, so DNS
also goes through policy. Tools that honour `ALL_PROXY` — `psql`, `curl`, `git`,
most cloud SDKs — now egress through the gateway, subject to policy.

## 4. Verify

- The **"Run the migration"** step succeeds.
- Point a step at something not in the policy (`secrets.internal:443`) — the
  tunnel is refused with the reason in `Proxy-Status`, and the step fails.
- The dashboard's **Activity** view has one line per decision: identity,
  application, destination, the rule or the denial reason, a timestamp.

That is the whole enforcement loop. Iterate on the policy; publish a new
revision and gateways pick it up.

---

## Without the composite action

`skimasque-client` does the exchange itself — useful for GitLab, Buildkite, or
debugging:

```yaml
      - name: Install skimasque-client
        run: curl -sSL "$SKIMASQUE_CLIENT_URL" | tar xz -C /usr/local/bin skimasque-client

      - name: Open the tunnel
        run: |
          skimasque-client --proxy gateway.skimasque.com:443 \
            --github-oidc --oidc-audience https://gateway.skimasque.com \
            --app psql \
            socks5 --listen 127.0.0.1:1080 &
          for _ in $(seq 20); do nc -z 127.0.0.1 1080 && break; sleep 0.5; done
```

For a one-off check instead of a relay:

```console
$ skimasque-client --proxy gateway.skimasque.com:443 \
    --github-oidc --oidc-audience https://gateway.skimasque.com --app psql \
    probe --target db.internal:5432 --text ping
```

## Local development, no CI

`skimasque` and `skimasque-client` also work from a laptop against a gateway you
can reach — a static token, or a signed-in session. See
[`cli.md`](cli.md#skimasque-client) and [`policies.md`](policies.md).

## Next

| You want… | See |
|---|---|
| To run the gateway in your own network | [`gateways.md`](gateways.md) (Mode 2) |
| To run everything yourself | [`self-hosting.md`](self-hosting.md) (Mode 3) |
| The policy DSL in full | [`policies.md`](policies.md) |
| OIDC details, other CI systems | [`github-actions.md`](github-actions.md) |
| Every command and flag | [`cli.md`](cli.md), [`configuration.md`](configuration.md) |
| Something failed | [`troubleshooting.md`](troubleshooting.md) |
