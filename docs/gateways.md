# Running a gateway

> **Applies to:** ✓ Mode 2 · ✓ Mode 3 · (Mode 1 — SkiMasque runs the gateway)

The gateway (`skimasque-server`) is one stateless process. It holds no database;
its policy and TLS material are files it re-reads in place, and every tunnel is
ephemeral. That makes it cheap to run several and to replace them without
cutting a running job.

The onboarding flow, whichever target you pick:

```
deploy the gateway
      ↓
allow the gateway → the resources it should reach   (your firewall)
      ↓
give it an identity source        (--github-oidc, or --auth-token)
      ↓
policy                            (a file, or --control-plane)
      ↓
point CI at it                    (skimasque-dev/connect@v1)
```

Your firewall stays the outer boundary — it decides what the gateway *can*
reach. SkiMasque decides *who* may use that reachability.

## Ports

| Port | For |
|---|---|
| `443/udp` (or `4433/udp` in examples) | MASQUE over QUIC. **UDP** — any load balancer must forward UDP. |
| `443/tcp` | only with `--acme`: the Let's Encrypt TLS-ALPN-01 challenge. |
| `<metrics>/tcp` | `/healthz`, `/readyz`, `/metrics` when `--metrics-listen` is set. Keep on an internal interface. |

## TLS

Easiest first:

- **`--acme` + `--hostname <name>`** (default `gateway.skimasque.com`) — the
  gateway obtains a Let's Encrypt certificate over TLS-ALPN-01 and renews it
  ~30 days before expiry. Needs `--hostname` to resolve to the box, TCP+UDP 443
  reachable, and `--acme-cache <dir>` on a persistent volume. Clients then need
  no `--ca`.
- **`--cert`/`--key` + `--tls-reload`** — bring your own certificate
  (cert-manager, an internal CA); the gateway re-reads it in place on change.
- **neither** — a throwaway self-signed certificate, dev only; the client pins
  it with `--ca`.

## The SSRF floor

Independent of policy, the gateway refuses tunnels to loopback, RFC 1918, CGNAT,
link-local (including `169.254.169.254`), and multicast — checked against the
**resolved** address, so a hostname that resolves into private space does not
walk past it. To reach an internal target, name its range:

```
--allow-cidr 10.0.5.0/24        # repeatable; the precise instrument
--allow-private                 # opens every private range at once; the blunt one
```

## Container

Every release publishes a `linux/amd64` gateway image (`linux/arm64` is
dropped for now — QEMU-emulated compilation of this workspace's release
profile did not finish in a reasonable time on the Actions runner; a
native-arm64 runner is the likely fix) to `ghcr.io/skimasque-dev/skimasque`,
tagged `vX.Y.Z` / `X.Y` / `X` / `latest`. While the repo is private the
package is private too — make the
package public (it is only compiled binaries on a Debian base), authenticate
with a classic `read:packages` PAT, or `docker load` the
`skimasque-image-amd64.tar.gz` attached to the release. Once the repo is public,
each image carries a Sigstore provenance attestation.

### Standalone gateway, policy in a file

```console
$ mkdir -p "$PWD/acme" && sudo chown 10001:10001 "$PWD/acme"   # the image runs as uid 10001

$ docker run -d --name skimasque-gateway --restart unless-stopped \
    -p 443:443/udp -p 443:443/tcp \
    -v "$PWD/acme:/acme" \
    -v "$PWD/.masque/policies:/etc/skimasque/policies:ro" \
    ghcr.io/skimasque-dev/skimasque:latest \
      --listen 0.0.0.0:443 --hostname gw.example.com \
      --acme --acme-email you@example.com --acme-cache /acme \
      --github-oidc --oidc-audience https://gw.example.com \
      --allow-cidr 10.0.0.0/8 \
      --policy-dir /etc/skimasque/policies --policy-reload \
      --audit-log /dev/stdout
```

`docker logs -f skimasque-gateway` shows `ACME certificate issued/renewed and
now being served` within a minute (issuance needs inbound TCP 443 to the box — a
cloud firewall has to allow it, not just `-p`).

### With a control plane (Mode 2/3)

[`deploy/docker/run-gateway.sh`](../deploy/docker/run-gateway.sh) wraps the ACME
setup (the `chown`, both port publishes, a staging/prod guard) and the
control-plane enrolment:

```console
$ SKM_HOSTNAME=gw.example.com \
  SKM_CONTROL_PLANE=https://control.skimasque.com \    # or your own (Mode 3)
  SKM_CONTROL_TOKEN=skmreg_… \                          # from `skimasque gateway register`
  ./deploy/docker/run-gateway.sh
$ docker logs -f skimasque-gateway     # -> "registered …" then "pulled policy version 1"
```

Or add `--control-plane <url> --control-plane-state <persistent-dir>` and
`SKIMASQUE_CONTROL_TOKEN` to your own `docker run`. The token goes in as an env
var so it stays out of `docker inspect`.

- **Do not** run `skimasque login` inside the gateway container — it runs one
  server process and has no writable home. Enrol with a token instead.
- Publish a policy revision before the first `--control-plane` boot, or the
  gateway exits with *"the control plane has no policy for this gateway's
  organisation yet"* and restart-loops until one exists.

## Linux host (systemd)

[`deploy/systemd/skimasque-gateway.service`](../deploy/systemd/skimasque-gateway.service)
— a hardened unit (`DynamicUser`, `ProtectSystem=strict`, no capabilities,
seccomp `@system-service`). Flags go in
[`gateway.env.example`](../deploy/systemd/gateway.env.example) →
`/etc/skimasque/gateway.env`. No `systemctl reload` needed: `--policy-reload`
and `--tls-reload` watch `/etc/skimasque/`, and `SIGTERM` drains gracefully.

## Kubernetes (Helm)

[`deploy/helm/skimasque-gateway`](../deploy/helm/skimasque-gateway) — a hardened
Deployment (`runAsNonRoot`, `readOnlyRootFilesystem`, drop all capabilities,
seccomp `RuntimeDefault`), liveness/readiness against `/healthz` and `/readyz`,
a PodDisruptionBudget, an optional `ServiceMonitor`, policy from a ConfigMap
picked up by `--policy-reload`, and TLS from a `kubernetes.io/tls` secret
rotated in place by `--tls-reload`.

```console
$ helm install gw deploy/helm/skimasque-gateway -f gateway-values.yaml
```

The QUIC Service is `ClusterIP` by default (in-cluster runners). For runners
outside the cluster, use `service.type=LoadBalancer` with a UDP-capable cloud
LB, or a NodePort.

## AWS (Terraform)

[`deploy/terraform`](../deploy/terraform) — a single-instance starting point
(instance + security group + Elastic IP). Its README lists what a production
module would add (an autoscaling group behind a UDP NLB, secrets from SSM).

## Shared platform gateway (`--platform`)

> Operated by SkiMasque (or a self-hosted control plane operator); customers do
> not run this.

`skimasque-server --platform` runs one process that serves many SkiMasque
organisations ("tenants"), each isolated from the others. Without the flag
nothing changes.

```console
$ skimasque-server --platform --github-oidc \
    --hostname gateway.skimasque.com \
    --control-plane https://control.skimasque.com \
    --control-plane-state /var/lib/skimasque/platform \
    --audit-log /var/lib/skimasque/audit.jsonl
```

The first start needs a **platform registration token** in
`SKIMASQUE_CONTROL_TOKEN` (`--control-plane-token`); the identity it yields is
stored in `--control-plane-state`. A platform gateway needs its own state
directory: it refuses to start on one that holds a single-organisation
gateway's identity.

### Flags

- Required: `--control-plane` (with `--control-plane-state`) and `--github-oidc`
  (`--oidc`). The server will not start without them.
- Refused: `--oidc-issuer`; `--oidc-provider` other than `github` (owners are
  verified on github.com, so a token from another issuer, such as GitHub
  Enterprise Server, could name a colliding owner); `--policy-dir`,
  `--policy-file`, `--policy-reload`, `--policy-observe`; and `--auth-token`.
  Policy comes from the tenant list only.
- Ignored, with a startup warning: `--oidc-audience` and `--credential-secret`.
- Ignored in platform mode: `--control-plane-signing-key-interval` and
  `--control-plane-no-credential-fallback` (there is no local minting to fall
  back to, and tenant keys arrive with the tenant list).
- Still honoured: `--control-plane-interval` (tenant poll and heartbeat),
  `--control-plane-name`, `--audit-log`, `--metrics-listen`, the TLS and ACME
  flags and the SSRF-floor flags. `--control-plane-label` is sent only when the
  gateway first registers.

### The audience

Each organisation has its own OIDC audience:

```
https://<--hostname>/o/<org-slug>
```

For example `https://gateway.skimasque.com/o/acme`. A `:443` suffix on
`--hostname` is dropped; any other port stays. The slug is matched exactly:
**case is not folded**. `--hostname` must be the name CI jobs request
tokens for, since the audience is built from it. At startup the gateway logs how many organisations it
serves and the audience pattern (`-v` lists each slug's audience).

### Token exchange and refusals

An exchange resolves the slug in the audience to a tenant, verifies the GitHub
token for that audience, checks the job's `repository_owner` (and its numeric
id, when the control plane has one recorded) against the tenant's verified
owners, and then asks the control plane to mint the credential with that
organisation's key. There is no default tenant: an audience that names no
tenant is refused.

| Code | HTTP | Raised by | Meaning |
|---|---|---|---|
| `unknown_org` | 403 | gateway | no tenant has that slug in the audience |
| `owner_not_verified` | 403 | gateway; also relayed from the control plane | the job's GitHub owner is not verified for the org; the gateway's message points at the owner-settings page |
| `over_cap` | 403 | control plane, relayed | the org used its plan's shared-gateway allowance |
| `temporarily_unavailable` | 502 | gateway | the control plane (or GitHub's signing keys) could not be reached; retry |

The gateway raises `unknown_org` and `owner_not_verified` itself; any refusal
the control plane returns (`over_cap`, and others) is relayed verbatim, code and
message, to the CI log. A wrong or expired OIDC token is a
403 `invalid_grant`.

### Outages: no local minting

A platform gateway never signs a credential itself. If the control plane is
down, **new** exchanges fail with `502 temporarily_unavailable`; credentials
already issued keep verifying and tunnels keep being enforced against the last
tenant list. A gateway that restarts while the control plane is down enforces
the cached list, and refuses to start only if it has neither.

### The tenant list and its cache

The gateway pulls the tenant list (slug, verified owners, signing keys and
policy per organisation) from the control plane and keeps polling it. The
current list is written atomically to `<--control-plane-state>/tenants.json`.
A **removed tenant** is cut off at the next refresh: every tunnel resolves its
organisation from the current list, so its existing credentials stop working
with no TTL grace. A **removed owner** takes effect for new exchanges at the
next refresh (owners are checked at exchange time); credentials that owner's
jobs already hold stay valid until they expire (`--credential-ttl`).

### Isolation

Every tunnel is verified against the key of the organisation its credential
names, evaluated against that organisation's policy only, limited per tenant
(two orgs that both name a policy `prod` do not share a limit), and audited and
metered under it. The audit trail is one hash chain per gateway, with each
event filed under the org it names; an event naming no org is not shipped. The
local `--audit-log` stays authoritative. See
[`threat-model.md`](threat-model.md#b7--shared-platform-gateway---platform).

### What to monitor

| Metric | Labels | Watch for |
|---|---|---|
| `skimasque_platform_tenants` | | tenants in force; an unexpected drop |
| `skimasque_platform_tenant_sync_total` | `outcome` = `applied`, `error` | rising `error`: the control plane is unreachable and the list is going stale |
| `skimasque_platform_tenant_policy_total` | `outcome` = `applied`, `rejected` | `rejected`: a tenant's policy did not load |
| `skimasque_platform_mint_total` | `outcome` = `minted`, `owner_not_verified`, `refused`, `unavailable`, `oidc_keys_unavailable` | `unavailable` is a control-plane outage; `refused` carries the control plane's refusals, such as `over_cap` |
| `skimasque_control_plane_audit_total` | `outcome` = `shipped`, `error`, `dropped`, `dropped_no_org` | `error` and `dropped` are lost audit events |
| `skimasque_control_plane_heartbeat_total` | `outcome` | the control plane seeing this gateway |
| `skimasque_control_plane_policy_age_seconds`, `skimasque_control_plane_policy_expired` | | the tenant list's age against `--control-plane-policy-lease` and `--control-plane-cache-ttl` |

## Firewall and egress IP

The gateway needs an outbound path to the destinations your policy allows. In
Modes 2–3 you control its egress IP — allowlist it on the target's firewall.
SkiMasque does not bypass your network controls; it decides *who* may use the
reachability your firewall already grants.

## Draining and rollout

On `SIGINT` / `SIGTERM` the gateway stops accepting connections, sends every
live HTTP/3 connection a GOAWAY, and lets in-flight tunnels finish until
`--shutdown-grace` (default `10s`) elapses. Set the orchestrator's grace period
a little higher (the systemd unit and Helm chart already do) so a routine
deploy never cuts a running job.

## Ops

`--metrics-listen <addr>` serves Prometheus `/metrics` (connection and tunnel
counters, bytes relayed, reload outcomes, ACME lifecycle events), `/healthz`,
and `/readyz` on a plain-HTTP listener separate from the QUIC data plane.
`--audit-log <path>` appends one JSON line per policy decision.

## Full flag reference

[`configuration.md`](configuration.md) and `skimasque-server --help`.
