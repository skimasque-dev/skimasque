# Configuration

> **Applies to:** ✓ Mode 1 (Action inputs) · ✓ Mode 2 · ✓ Mode 3

Every flag also reads an environment variable where noted. This page is a
companion to `--help`, not a replacement — run `skimasque-server --help`,
`skimasque-client --help`, `skimasque <cmd> --help` for the authoritative text.

## `skimasque-server` (the gateway)

### Listen and authority

| Flag | Default | Notes |
|---|---|---|
| `--listen <ADDR>` | `0.0.0.0:4433` | the QUIC (UDP) listener |
| `--hostname <HOST[:PORT]>` | `gateway.skimasque.com` | public name; the `--acme` certificate domain and the default `--authority` |
| `--authority <HOST[:PORT]>` | `--hostname` | the authority in the URI Template |
| `--template <TEMPLATE>` | — | a complete URI Template, overriding `--authority` |

### TLS

| Flag | Default | Notes |
|---|---|---|
| `--acme` | off | obtain + renew a Let's Encrypt cert for `--hostname` over TLS-ALPN-01. Needs TCP 443 reachable and `--acme-cache`. Mutually exclusive with `--cert`. |
| `--acme-email <EMAIL>` | — | Let's Encrypt expiry-warning contact |
| `--acme-cache <DIR>` | `skimasque-acme` | **must persist** or rate limits bite |
| `--acme-staging` | off | Let's Encrypt staging (untrusted certs, lax limits) — for testing |
| `--acme-extra-domain <DOMAIN>` | — | additional SAN; repeatable |
| `--acme-challenge-port <PORT>` | `--listen` port | only for a local test rig; Let's Encrypt only validates on 443 |
| `--cert <PATH>` / `--key <PATH>` | — | bring your own PEM |
| `--tls-reload` | off | re-read `--cert`/`--key` on change; `SIGHUP` forces it |
| `--tls-reload-interval <DUR>` | `5s` | |
| `--self-signed-name <NAME>` | `localhost` | subject for a generated dev cert; repeatable |
| `--write-cert <PATH>` | — | write the served cert for a client to `--ca`-pin |

### Identity

| Flag | Env | Notes |
|---|---|---|
| `--oidc` / `--github-oidc` | — | verify CI OIDC tokens and enable the exchange endpoint. Replaces `--auth-token`. |
| `--oidc-provider <NAME>` | — | `github` (default), `gitlab`, `buildkite`, `generic` |
| `--oidc-audience <AUD>` | — | **required** with `--oidc`; repeatable; the `aud` each pipeline requests |
| `--oidc-issuer <URL>` | — | override for self-managed GitLab / GHES / `generic` |
| `--oidc-claim <FIELD=CLAIM>` | — | for `generic`: map `organization`/`repository`/`workflow`/`ref`/`environment`/`actor` |
| `--auth-token <TOKEN>` | `SKIMASQUE_TOKEN` | a shared bearer token (instead of `--oidc`) |
| `--credential-secret <HEX>` | `SKIMASQUE_CREDENTIAL_SECRET` | HS256 key for signing platform credentials; **same value across a fleet** |
| `--credential-ttl <DUR>` | — | credential lifetime (default `1h`) |

### Policy

| Flag | Default | Notes |
|---|---|---|
| `--policy-dir <DIR>` | — | enforce `*.toml` / `*.yaml` here |
| `--policy-file <PATH>` | — | specific files instead of scanning a dir |
| `--policy-reload` | off | swap the policy set on file change; `SIGHUP` forces it; a bad revision is logged and ignored |
| `--policy-reload-interval <DUR>` | `5s` | |
| `--policy-observe` | off | log what policy *would* decide, allow every tunnel; feeds `skimasque policy learn` |
| `--audit-log <PATH>` | — | one JSON line per decision; the compliance artifact. Conflicts with `--policy-observe`. |

### Control plane (Modes 2–3)

| Flag | Env | Default | Notes |
|---|---|---|---|
| `--control-plane <URL>` | — | — | pull policy from a control plane instead of files |
| `--control-plane-state <DIR>` | — | — | **required** with `--control-plane`; holds the identity + cached policy; **must persist** |
| `--control-plane-token <TOKEN>` | `SKIMASQUE_CONTROL_TOKEN` | — | one-time `skmreg_…`; not needed after first registration |
| `--control-plane-name <NAME>` | — | listen address | fleet-view display name |
| `--control-plane-label <KEY=VALUE>` | — | — | attributes for policy targeting; repeatable; re-sent every start |
| `--control-plane-interval <DUR>` | — | `30s` | long-poll wait + heartbeat interval |
| `--control-plane-policy-lease <DUR>` | — | `15m` | soft lease: report degraded past this, keep enforcing |
| `--control-plane-cache-ttl <DUR>` | — | `30m` | hard TTL: escalate the alarm past this, still keep enforcing. ≥ the lease. |
| `--control-plane-signing-key-interval <DUR>` | — | `1h` | re-fetch the org Ed25519 key (picks up rotation) |
| `--control-plane-no-credential-fallback` | — | off | with `--oidc`: fail exchange (502) during an outage instead of local-signing |

### Address floor (SSRF)

| Flag | Notes |
|---|---|
| `--allow-cidr <CIDR>` | permit one range past the private/loopback floor; repeatable; **the precise instrument** |
| `--allow-private` | permit all loopback / RFC 1918 / link-local; **the blunt one** |
| `--allow-port <PORT>` | restrict destinations to this port; repeatable |

### Transport and resource limits

| Flag | Default | Notes |
|---|---|---|
| `--connect-tcp` | off | also serve TCP tunnels (classic `CONNECT`) — needed for `curl`/`git`/databases via SOCKS |
| `--max-concurrent-requests <N>` | `1024` | tunnels opening at once, across connections |
| `--max-connections <N>` | `1024` | open QUIC connections; `0` = no cap |
| `--max-connection-rate <N>` | `50` | new connections/s, global; `0` = off |
| `--max-connection-burst <N>` | `200` | burst allowance |
| `--max-source-connection-rate <N>` | `20` | new connections/s per remote IP — raise when runners share a NAT |
| `--max-source-connection-burst <N>` | `60` | |
| `--max-exchange-rate <N>` | `10` | token-exchange requests/s per IP (with `--oidc`) |
| `--max-exchange-burst <N>` | `30` | |
| `--max-tunnels-per-connection <N>` | `256` | tunnels on one connection; over → `503` |
| `--tunnel-idle-timeout <DUR>` | `120s` | reclaim an idle tunnel; `0` = off; matters for `--connect-tcp` |
| `--shutdown-grace <DUR>` | `10s` | let in-flight tunnels finish after `SIGTERM` |

### Ops

| Flag | Notes |
|---|---|
| `--metrics-listen <ADDR>` | Prometheus `/metrics` + `/healthz` + `/readyz` on a plain-HTTP listener; keep internal |
| `-v` / `--verbose` | repeatable |

## `skimasque-client` (the tunnel client)

| Flag | Env | Default | Notes |
|---|---|---|---|
| `--proxy <HOST[:PORT]>` | — | `gateway.skimasque.com` | the gateway; port defaults to 443 |
| `--authority <HOST[:PORT]>` | — | proxy host | TLS server name + `:authority` |
| `--template <TEMPLATE>` | — | well-known CONNECT-UDP | |
| `--ca <PATH>` | — | system roots | trust this PEM instead |
| `--insecure` | — | off | accept any cert — **gives up MITM defence**; use `--ca` |
| `--auth-token <TOKEN>` | `SKIMASQUE_TOKEN` | — | bearer token |
| `--github-oidc` | — | off | fetch the runner's OIDC token and exchange it; needs `--oidc-audience` |
| `--oidc-token <JWT>` | `SKIMASQUE_OIDC_TOKEN` | — | supply a token instead of fetching one |
| `--oidc-audience <AUD>` | — | — | must match a gateway `--oidc-audience` |
| `--app <NAME>` | — | — | declared application (`X-Masque-Application`); policy matches on it |

Subcommands: `socks5 --listen <ADDR>`, `connect --target <HOST:PORT>`,
`probe --target <HOST:PORT> [--text <S> | --dns <NAME>]`.

## The GitHub Action (`skimasque-dev/connect@v1`)

| Input | Required | Default | Notes |
|---|---|---|---|
| `proxy` | yes | — | gateway address `host:port` |
| `audience` | yes | — | OIDC audience; must equal a gateway `--oidc-audience` |
| `authority` | no | proxy host | TLS server name / `:authority` |
| `application` | no | — | declared application, matched by policy |
| `ca` | no | — | PEM the gateway's cert chains to (private CA) |
| `listen` | no | `127.0.0.1:1080` | local SOCKS5 relay address |
| `version` | no | the action's ref | `skimasque` release to install; a moving ref resolves to latest |
| `repository` | no | `skimasque-dev/skimasque` | where to download the release from |
| `client-bin` | no | — | use this binary instead of downloading |

It writes `ALL_PROXY=socks5h://<listen>` to `GITHUB_ENV`.

## `run-gateway.sh` environment (Docker)

| Var | Default | Notes |
|---|---|---|
| `SKM_HOSTNAME` | — | **required** — the gateway's public DNS name |
| `SKM_ACME_EMAIL` | — | Let's Encrypt contact |
| `SKM_STAGING` | — | use Let's Encrypt staging |
| `SKM_IMAGE` | `ghcr.io/skimasque-dev/skimasque:latest` | |
| `SKM_ACME_DIR` | `$HOME/skimasque/acme` | bind-mounted, chowned to 10001 |
| `SKM_STATE_DIR` | `$HOME/skimasque/state` | control-plane identity + cache |
| `SKM_PORT` | `443` | host port → container 443 |
| `SKM_CONTAINER` | `skimasque-gateway` | |
| `SKM_CONTROL_PLANE` | `https://control.skimasque.com` *(if `SKM_CONTROL_TOKEN` is set)* | control-plane URL — set for Mode 3 |
| `SKM_CONTROL_TOKEN` | — | `skmreg_…`, first run only; passed as `SKIMASQUE_CONTROL_TOKEN` env, not a flag |
| `SKM_CONTROL_NAME` | `SKM_HOSTNAME` | fleet display name |
| `SKM_CONTROL_LABELS` | — | space-separated `key=value` |

`./run-gateway.sh -- <args>` passes `<args>` straight to `skimasque-server`.

## `skimasque-control` environment (Mode 3 control plane)

All `SKIMASQUE_CONTROL_*`: `STORE`, `CACHE_MB`, `LISTEN`, `ADMIN_TOKEN`,
`GITHUB_CLIENT_ID` / `GITHUB_CLIENT_SECRET`, `DASHBOARD_URL`, `HOSTNAME`,
`ACME` / `ACME_EMAIL` / `ACME_CACHE` / `ACME_STAGING`, `TLS_CERT` / `TLS_KEY`.
See [`self-hosting.md`](self-hosting.md) and the control-plane distribution's
own docs.

## The `skimasque` CLI credential file

`skimasque login` stores a session (mode `0600`) at, in order of preference:
`$SKIMASQUE_CONFIG_HOME`, `$XDG_CONFIG_HOME/skimasque/`,
`~/.config/skimasque/` (Unix), `%APPDATA%\skimasque\` (Windows).
`--control-plane` / `$SKIMASQUE_CONTROL_PLANE` selects the control plane
(default: SkiMasque Cloud).
