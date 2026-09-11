# CLI reference

> **Applies to:** ✓ Mode 1 · ✓ Mode 2 · ✓ Mode 3

A companion to `--help`, not a replacement — every command prints its own full
help, and this page groups the surface so you can find things. Three binaries
ship in `skimasque-cli`:

| Binary | Role |
|---|---|
| **`skimasque`** | workstation front door — policy, sign-in, fleet management |
| **`skimasque-server`** | the gateway data plane |
| **`skimasque-client`** | opens tunnels through a gateway |

Most flags also read a `SKIMASQUE_*` environment variable (noted where it
matters); secrets are marked so `--help` hides their values.

---

## `skimasque`

### Policy, locally (no network)

| Command | Does |
|---|---|
| `skimasque init [--policy-dir DIR]` | scaffold `.masque/policies/` (default `.masque/policies`) |
| `skimasque policy check <POLICY> <DEST>` | ALLOW/DENY for one request; exit 0 = allow, 1 = deny (composes in scripts) |
| `skimasque policy explain <POLICY> <DEST>` | same, but always exit 0 and print the full reasoning |
| `skimasque policy test` | run every policy's `[[tests]]` assertions |
| `skimasque policy validate [--strict]` | parse every file and lint `[match]` blocks; `--strict` exits non-zero on a lint, for a CI gate |
| `skimasque policy diff <OLD> <NEW>` | what changed between two policy dirs or files |
| `skimasque policy learn <OBSERVATIONS> [--name N]` | turn observed `{application,destination}` tuples into a reviewable draft |
| `skimasque why <DEST>` | pick the policy whose `[match]` accepts the identity, evaluate, and print the decision |
| `skimasque status` | summarise the local policy set — what loads, whether its tests pass |

`check` / `explain` / `why` / `learn` take identity flags — `--repository
owner/name`, `--branch <b>` (or `--ref refs/heads/<b>`), `--workflow`,
`--environment`, `--actor`, `--organization` — plus `--app <name>` and
`--transport tcp|udp` (default `tcp`). Policy source is `--policy-dir` (default
`.masque/policies`) or repeatable `--policy-file`.

```console
$ skimasque why db.internal:5432 --app psql --repository acme/web --branch main
```

### Sign in to a control plane

| Command | Does |
|---|---|
| `skimasque login [--control-plane URL]` | GitHub device flow; stores a 30-day session. Defaults to `https://control.skimasque.com`, then the last one used. A bare host gains `https://`. `$SKIMASQUE_CONTROL_PLANE` also works. |
| `skimasque logout` | revoke the session and delete the credential file |
| `skimasque whoami` | show the signed-in developer and control plane |

The credential file lives at `$SKIMASQUE_CONFIG_HOME`, else
`$XDG_CONFIG_HOME/skimasque/`, else `~/.config/skimasque/` (Unix) /
`%APPDATA%\skimasque\` (Windows), mode `0600`.

### Manage an organisation (needs `skimasque login`)

| Command | Does |
|---|---|
| `skimasque org create <NAME>` | create an org; you become its owner |
| `skimasque org list` | the orgs you belong to |
| `skimasque org members` | list members and roles |
| `skimasque org add-member <github-login>` | add a member (they must have signed in once) |
| `skimasque org set-role <github-login> <owner\|member>` | promote/demote (owner-only) |
| `skimasque org remove-member <github-login>` | remove a member (owner-only) |
| `skimasque gateway register [--org ID] [--hostname H] [--acme] [--label k=v]…` | mint a registration token and print (or, with `--run`, execute) the ready `skimasque-server --control-plane …` command |
| `skimasque audit [--gateway ID] [--decision allow\|deny] [--since RFC3339] [--limit N]` | recent decisions across the fleet; the client pages the API so `--limit` has no ceiling |
| `skimasque why <DEST> --control-plane [--org ID] [--revision N \| --draft] [--gateway G]` | evaluate server-side against the published policy (or a revision / the draft), through the same engine the gateway uses |

`--org` is inferred when you belong to exactly one.

### Run a gateway or open a tunnel

- `skimasque gateway <args…>` execs `skimasque-server` with the args — see below.
- `skimasque connect <DEST> [client args…]` execs `skimasque-client … connect
  --target <DEST>`, bridging the tunnel to stdin/stdout (an SSH `ProxyCommand`,
  a pipeline). Needs at least `--proxy <gateway>`. Not yet control-plane-aware —
  pass the gateway address explicitly.

---

## `skimasque-server`

`--listen <ADDR>` (default `0.0.0.0:4433`; use `0.0.0.0:443` in production).
Repeat `-v` for more logging — the ACME lifecycle and the policy audit trail
show at the default level.

### TLS — pick one

| Flags | Effect |
|---|---|
| *(none)* | a throwaway self-signed cert (`--self-signed-name`, `--write-cert` to save it); dev only |
| `--acme` + `--hostname <name>` | in-process Let's Encrypt over TLS-ALPN-01, auto-renewing. `--acme` is a **flag**; the cert name is `--hostname` (default `gateway.skimasque.com`). Also: `--acme-email`, `--acme-extra-domain`, `--acme-staging`, `--acme-cache <dir>` (must persist — default `skimasque-acme` is relative). Needs inbound **TCP 443** from the internet, not just a published port. |
| `--cert <pem> --key <pem>` + `--tls-reload` | a certificate you supply; `--tls-reload` re-reads it in place on change (or `SIGHUP`), interval `--tls-reload-interval` (5s) |

### Identity

| Flag | Effect |
|---|---|
| `--oidc` (alias `--github-oidc`) | verify CI OIDC tokens and enable the exchange endpoint; a tunnel without a verifiable credential is refused. Replaces `--auth-token`. |
| `--oidc-provider <github\|gitlab\|buildkite\|generic>` | which CI system's tokens (default `github`) |
| `--oidc-audience <AUD>` | **required with `--oidc`**, repeatable — the `aud` each pipeline requests its token for |
| `--oidc-issuer <URL>` | override the provider's hosted issuer (GHES, self-managed GitLab, `generic`) |
| `--oidc-claim FIELD=CLAIM` | for `generic`: map an identity field to a token claim |
| `--auth-token <TOKEN>` (`SKIMASQUE_TOKEN`) | pre-OIDC static bearer; prefer `--oidc` |
| `--credential-secret <HEX>` (`SKIMASQUE_CREDENTIAL_SECRET`) | HS256 key for platform credentials; set the same value fleet-wide. Unset = random per start. |
| `--credential-ttl <DUR>` | credential lifetime (default `1h`); the client refreshes ahead of it |

### Policy

`--policy-dir <DIR>` or repeatable `--policy-file <PATH>` — without a source the
gateway applies only the address floor. `--policy-reload` swaps the set on a
file change (or `SIGHUP`) with no dropped tunnel; `--policy-reload-interval`
(5s). `--policy-observe` logs what policy *would* decide but allows every tunnel
(feeds `skimasque policy learn`). `--audit-log <PATH>` appends a JSON-lines
record of every decision (conflicts with `--policy-observe`).

### Control plane (instead of local policy)

| Flag | Effect |
|---|---|
| `--control-plane <URL>` | pull policy from skimasque's control plane (`https://control.skimasque.com`); requires `--control-plane-state` |
| `--control-plane-state <DIR>` | **persistent** — the registered identity and the cached policy |
| `--control-plane-token <TOKEN>` (`SKIMASQUE_CONTROL_TOKEN`) | one-time registration token; not needed once registered |
| `--control-plane-name <NAME>` | fleet display name (default: the listen address) |
| `--control-plane-label KEY=VALUE` | a label for D3 policy targeting; repeatable, re-sent every start |
| `--control-plane-interval <DUR>` | long-poll / heartbeat interval (default `30s`) |
| `--control-plane-policy-lease <DUR>` | soft lease — degrade after this offline (default `15m`) |
| `--control-plane-cache-ttl <DUR>` | hard TTL — escalate the alarm after this, still enforcing (default `30m`, ≥ lease) |
| `--control-plane-no-credential-fallback` | fail token exchange (502) during an outage instead of signing locally |
| `--control-plane-signing-key-interval <DUR>` | re-fetch the org Ed25519 key (default `1h`) |

### Address floor, limits, ops

- `--allow-cidr <CIDR>` (repeatable) — open one private/loopback range;
  `--allow-private` opens all of them; `--allow-port <PORT>` (repeatable)
  restricts to named ports.
- Resource caps, each `0` to disable: `--max-connections` (1024),
  `--max-connection-rate` / `--max-connection-burst` (50 / 200),
  `--max-source-connection-rate` / `-burst` (20 / 60), `--max-exchange-rate` /
  `-burst` (10 / 30), `--max-concurrent-requests` (1024),
  `--max-tunnels-per-connection` (256), `--tunnel-idle-timeout` (`120s`).
- `--no-connect-tcp` — UDP only. TCP tunnels (what a SOCKS front end needs for
  `curl` / `git` / databases) are served by default; this refuses them.
- `--metrics-listen <ADDR>` — `/healthz`, `/readyz`, Prometheus `/metrics` on a
  plain-HTTP listener; keep it on a private interface.
- `--shutdown-grace <DUR>` — how long in-flight tunnels get after `SIGTERM`
  before the endpoint closes (default `10s`).
- `--authority <HOST[:PORT]>` / `--template <T>` — the URI Template served
  (defaults from `--hostname`).

---

## `skimasque-client`

`skimasque-client [connection flags] <socks5 | connect | probe>`

### Connection flags

| Flag | Effect |
|---|---|
| `--proxy <HOST[:PORT]>` | the gateway (default `gateway.skimasque.com`, port `443`) |
| `--authority <HOST[:PORT]>` | TLS server name / `:authority` (default: the `--proxy` host) |
| `--ca <PATH>` | trust these PEM certs instead of the system roots |
| `--insecure` | accept any certificate — gives up MITM defence; use `--ca` instead |
| `--auth-token <TOKEN>` (`SKIMASQUE_TOKEN`) | static bearer, for a `--auth-token` gateway |
| `--github-oidc` | fetch the runner's OIDC token and exchange it (needs `--oidc-audience`) |
| `--oidc-token <JWT>` (`SKIMASQUE_OIDC_TOKEN`) | supply a token you fetched another way |
| `--oidc-audience <AUD>` | must match one of the gateway's `--oidc-audience` values |
| `--org <ORG>` | with none of the above: mint a credential from a `skimasque login` session instead, for this org (auto-resolved if the session belongs to only one) |
| `--app <NAME>` | the application to declare (`X-Masque-Application`); policy matches on it |

### Subcommands

| Command | Does |
|---|---|
| `socks5 [--listen ADDR]` | run a SOCKS5 relay (default `127.0.0.1:1080`); `CONNECT` (TCP, refused by a `--no-connect-tcp` gateway) and `UDP ASSOCIATE`. Point `ALL_PROXY=socks5h://…` at it. |
| `connect --target <HOST:PORT>` | one raw TCP tunnel bridged to stdin/stdout (SSH `ProxyCommand`, interop) |
| `probe --target <HOST:PORT> [--dns NAME \| --text T \| --hex H] [--count N] [--timeout MS]` | send a payload and print the replies |

```console
$ skimasque-client --proxy gw.example.com --github-oidc \
    --oidc-audience https://gw.example.com --app psql \
    socks5 --listen 127.0.0.1:1080
```
