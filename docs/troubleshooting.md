# Troubleshooting

> **Applies to:** ✓ Mode 1 · ✓ Mode 2 · ✓ Mode 3

Turn up logging with `-v` (repeatable). The gateway's `--audit-log` and the
client's stderr carry the decision detail.

## OIDC exchange fails

**Symptom:** the client exits at "exchange", or the gateway logs a `401`/`403`
at the token-exchange endpoint.

- **`aud` mismatch** — the token was minted for a different audience. The
  workflow's `permissions: id-token: write` plus the action's `audience:` (or
  `--oidc-audience`) must equal one of the gateway's `--oidc-audience` values,
  exactly, including scheme.
- **Wrong issuer** — self-managed GitLab / GitHub Enterprise Server needs
  `--oidc-issuer https://<host>` on the gateway.
- **`permissions: id-token: write` missing** — the runner can't mint a token;
  the action fails with *"no OIDC token available"*.
- **Clock skew** — `exp` / `nbf` are checked with 60s leeway; a badly skewed
  runner clock fails verification.

## Policy denied the tunnel

**Symptom:** the tunnel is refused; `Proxy-Status` carries the reason; a SOCKS
client sees `REPLY_NOT_ALLOWED`; the job step fails.

- Run `skimasque why <host:port> --app <name> --repository <r> --branch <b>`
  (add `--control-plane` for the org's published policy). It prints the same
  decision the gateway logged, plus the closest rules and a suggested rule.
- **`[match]` didn't match** — a fork's PR branch, a different workflow, or the
  wrong `repository`. Check the audit line's identity fields.
- **`--app` differs** — the rule's `application` must equal what the client
  declared.
- **Transport** — a rule with `transport = "udp"` won't allow a TCP tunnel (and
  vice versa).

## Gateway unreachable

- **QUIC is UDP.** Every firewall, security group, and load balancer between CI
  and the gateway must forward the **UDP** port (443 or 4433). A TCP-only LB
  will not work.
- `--acme` also needs inbound **TCP 443** for the Let's Encrypt challenge —
  publishing the port with `-p` is not enough if a cloud firewall blocks it.
- Check `curl -sS https://<gateway>/readyz` if `--metrics-listen` is set.

## Destination unreachable, but policy allowed it

- **The SSRF floor.** An internal target (`10.x`, `192.168.x`, `172.16-31.x`,
  `169.254.x`, loopback) is refused on the *resolved* address unless you named
  its range with `--allow-cidr 10.0.5.0/24`. `--allow-private` opens all of it.
- **Your firewall.** The gateway needs an outbound path to the destination.
  SkiMasque does not bypass your network controls — allowlist the gateway's
  egress IP on the target's side (Modes 2–3).

## Certificate / ACME failure

- **`--acme` pending forever** — `--hostname` must resolve to the box and TCP
  443 must be reachable *from the internet*. Watch `docker logs` for
  `ACME TLS-ALPN-01 challenge listener up` then `ACME certificate issued`.
- **Rate-limited** — Let's Encrypt production has tight limits. Use
  `--acme-staging` while testing, and keep `--acme-cache` on a **persistent**
  volume so a restart doesn't re-order.
- **`account cache store: Permission denied`** — the image runs as uid 10001; a
  fresh named volume is root-owned. Bind-mount a host dir you `chown 10001:10001`
  first, or use `run-gateway.sh`.
- **Wrong environment served** — `run-gateway.sh` / `run-control.sh` clear the
  ACME cache when the staging↔production marker changes; a manual `docker run`
  does not.

## Tunnel expired mid-job

The credential TTL (`--credential-ttl`, default 1h) is re-exchanged a quarter of
the way before expiry by `skimasque-client`. If a job still loses the tunnel,
the client process was killed or the relay was restarted — keep the
`skimasque-client … socks5 &` process alive for the whole job.

## Control plane unavailable

**This does not stop enforcement.** The gateway keeps serving its cached policy:

- past `--control-plane-policy-lease` (15m) it reports `degraded` in heartbeats;
- past `--control-plane-cache-ttl` (30m) it logs an error and sets the
  `skimasque_control_plane_policy_expired` gauge;
- it never fails open and never halts the data plane.

Once the control plane returns, the next poll resyncs. If token exchange starts
returning `502`, the mint endpoint is down and
`--control-plane-no-credential-fallback` is set — remove it to allow local
signing during an outage.

## Self-hosted gateway registration failed

- **`403 bad_registration_token`** — the token is spent (single-use) or expired
  (~1h). Mint a fresh one and, if the gateway half-registered, clear
  `--control-plane-state` (`rm -rf ~/skimasque/state/*`) before retrying.
- **`the control plane has no policy for this gateway's organisation yet`** —
  publish a policy revision before the first `--control-plane` boot. The
  container restart-loops until one exists.
- **`relative URL without a base`** — `--control-plane` needs a URL; a bare host
  gets `https://` automatically, but an empty value or a typo like `://x` does
  not.

## `skimasque login` inside a container fails

Don't. `skimasque login` is a workstation command — the gateway container runs
one server process and has no writable home. Enrol the gateway with a
registration token instead (`--control-plane-token` / `SKM_CONTROL_TOKEN`).
