# deploy/

Deployment artifacts for the **gateway**. See [`docs/gateways.md`](../docs/gateways.md)
for the walkthrough.

| Path | What |
|---|---|
| `docker/Dockerfile` | Builds the gateway image — `skimasque` / `-server` / `-client`. Build from the repo root. |
| `docker/run-gateway.sh` | Launch a gateway container that terminates TLS itself via ACME (Let's Encrypt): sets up a writable ACME cache, publishes UDP+TCP 443, enrols with SkiMasque Cloud's control plane by default (`SKM_CONTROL_TOKEN` on first run; `SKM_CONTROL_PLANE=` opts out), verifies GitHub Actions OIDC by default (`SKM_OIDC=0` opts out), and re-runs cleanly to roll the image or change flags. `SKM_HOSTNAME=… deploy/docker/run-gateway.sh`. |
| `systemd/skimasque-gateway.service` + `gateway.env.example` | Hardened gateway unit for a Linux host. |
| `helm/skimasque-gateway/` | Helm chart for the gateway: hardened Deployment, policy ConfigMap, TLS secret mount, health probes, PDB, optional ServiceMonitor. |
| `agent/` | The pull-based **release agent** (`skimasque-deploy`): a systemd timer that installs the release an operator has promoted, verifies its SHA-256, health-checks it and rolls back on failure; plus an idempotent data-disk helper. See [`agent/README.md`](agent/README.md). |
| `terraform/` | Single-instance AWS starting point for a gateway (instance + SG + EIP). Not a production module. |
| `terraform/gcp-gateway/` | A gateway on a GCP VM: static IP, firewall, persistent state disk, service account, secret container, cloud-init and the release agent. See [`terraform/gcp-gateway/README.md`](terraform/gcp-gateway/README.md). |
| `terraform/agent-files/` | The agent's files as Terraform outputs, so other stacks embed the same copy. |

The gateway's QUIC is UDP (`4433/udp`). Real `*.env` files are git-ignored —
commit only the `*.example`.

The hosted control plane is a separate, private component
([`skimasque-dev/control`](https://github.com/skimasque-dev/control)); a gateway
reaches it with `--control-plane <url>`.
