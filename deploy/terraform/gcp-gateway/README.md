# GCP gateway module

One SkiMasque gateway on a GCP VM: a static IP, firewall rules, a persistent
state disk, a service account, a Secret Manager container for the one-time
registration token, and cloud-init that installs the
[release agent](../../agent/README.md) and the hardened systemd unit. The agent
then pulls the release an operator has promoted; the VM never compiles anything.

```hcl
module "gateway" {
  source = "github.com/skimasque-dev/skimasque//deploy/terraform/gcp-gateway?ref=v0.4.0"

  name              = "acme"
  hostname          = "gateway.example.com"
  project_id        = "my-project"
  zone              = "us-central1-a"
  network           = "default"
  control_plane_url = "https://control.example.com"
  release_source    = "gs://my-release-bucket"
  acme_email        = "ops@example.com"
}
```

A full example is in [`examples/basic`](examples/basic/main.tf). Use a **dedicated
VPC** in real deployments: the `default` network's own firewall rules (SSH and RDP
open to the internet, all internal traffic allowed) still apply to the VM alongside
the module's rules.

## What you do around it

1. Point an A record for `hostname` at `module.gateway.address`.
2. Give `module.gateway.service_account_email` read access to the release
   bucket (`roles/storage.objectViewer`, scoped to `gateway/` and
   `channels/gateway`). The module does not manage your bucket.
3. Release layout the agent expects, in `release_source`:
   `channels/gateway` (a version string), `gateway/<version>/skimasque-server` and
   `gateway/<version>/skimasque-server.sha256`.
4. Mint a registration token (the dashboard, or `POST /v1/orgs/{org}/registration-tokens`;
   set `ttl_seconds` generously, the default is an hour) and store it:

   ```bash
   printf '%s' "$TOKEN" | gcloud secrets versions add \
     "$(terraform output -raw registration_token_secret)" --data-file=-
   ```

   Do this **before** the first release is promoted (a gateway with no token and
   no identity cannot enrol, so its first health check fails). The token is needed only
   until the gateway has enrolled; its identity then lives on the state disk.
5. Promote a release. Within about two minutes the agent installs it, starts the
   gateway, and ACME issues the certificate.

## Modes

The gateway runs single-tenant: one organisation, with the GitHub OIDC audience
`https://<hostname>`. SkiMasque's shared multi-tenant mode (`--platform`) is not
offered yet because the released `skimasque-server` has no such flag; the module
will gain a `mode` input when it does. A test compares every flag the module
emits with the server's source, so an unsupported flag is caught in CI.

## State and replacement

The gateway's identity, audit log and ACME account live on a separate 10 GB disk
(`state_disk_gb`) with `prevent_destroy`. A change to the rendered cloud-init
(including a new agent) **replaces the VM**, which is safe: the disk, the IP and
DNS stay. To tear a test environment down, remove the `lifecycle { prevent_destroy }`
block on `google_compute_disk.state` first.

## Not included

Autoscaling, a load balancer, more than one region, and a provider other than
GCP. The inputs and outputs (`name`, `hostname`, `zone`, `address`,
`service_account_email`, `release_source`) are the contract another cloud's
module should keep.
