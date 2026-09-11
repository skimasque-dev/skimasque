# AWS reference deployment

A single-file starting point: one instance running the gateway container, a
security group admitting QUIC from your CI egress, and a stable Elastic IP.

```hcl
module "gateway" {
  source          = "github.com/skimasque-dev/skimasque//deploy/terraform"
  vpc_id          = "vpc-..."
  subnet_id       = "subnet-..."           # public
  image           = "ghcr.io/you/skimasque-gateway:0.1.0"
  authority       = "gateway.example.com"
  ci_egress_cidrs = ["203.0.113.0/24"]     # your runners' egress
  extra_server_args = join(" ", [
    "--github-oidc", "--oidc-audience", "https://gateway.example.com",
    "--policy-dir", "/etc/skimasque/policies",
    "--metrics-listen", "127.0.0.1:9090",
  ])
}
```

`module.gateway.gateway_address` is what clients connect to (`<address>:4433`).

## Deliberately not included

- Autoscaling and a UDP network load balancer. One instance is a single point
  of failure; run at least two behind an NLB with UDP target groups for real use.
- Policy file delivery. Bake them into a custom image, or fetch them in
  `user_data` from S3 / an artifact store, and run the gateway with
  `--policy-reload`.
- Secrets. Pull `--credential-secret` / `--auth-token` from SSM Parameter Store
  or Secrets Manager rather than passing them on the command line.
- TLS. Give the gateway a real certificate: add `--acme` to `extra_server_args`
  (the instance must be reachable on TCP+UDP 443 at `authority`, and the ACME
  cache needs a persistent path), or mount your own and pass
  `--cert`/`--key --tls-reload`. The container's self-signed default is dev-only.
