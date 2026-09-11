# deploy/

Deployment artifacts for the **gateway**. See [`docs/gateways.md`](../docs/gateways.md)
for the walkthrough.

| Path | What |
|---|---|
| `docker/Dockerfile` | Builds the gateway image — `skimasque` / `-server` / `-client`. Build from the repo root. |
| `docker/run-gateway.sh` | Launch a gateway container that terminates TLS itself via ACME (Let's Encrypt): sets up a writable ACME cache, publishes UDP+TCP 443, optionally enrols with a control plane (`SKM_CONTROL_PLANE` + `SKM_CONTROL_TOKEN`), and re-runs cleanly to roll the image or change flags. `SKM_HOSTNAME=… deploy/docker/run-gateway.sh`. |
| `docker/publish-gateway.sh` / `.ps1` | Build and push the gateway image to GHCR (or `MODE=save` for a tarball) without CI. |
| `systemd/skimasque-gateway.service` + `gateway.env.example` | Hardened gateway unit for a Linux host. |
| `helm/skimasque-gateway/` | Helm chart for the gateway: hardened Deployment, policy ConfigMap, TLS secret mount, health probes, PDB, optional ServiceMonitor. |
| `terraform/` | Single-instance AWS starting point for a gateway (instance + SG + EIP). Not a production module. |

The gateway's QUIC is UDP (`4433/udp`). Real `*.env` files are git-ignored —
commit only the `*.example`.

The hosted control plane is a separate, private component
([`skimasque-dev/control`](https://github.com/skimasque-dev/control)); a gateway
reaches it with `--control-plane <url>`.
