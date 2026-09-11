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
