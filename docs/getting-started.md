# Getting started

SkiMasque gives CI jobs, developers and coding agents policy-controlled TCP/UDP
access through a MASQUE gateway. Choose a gateway that can reach your resource:
the shared Cloud gateway for reachable destinations, or a customer gateway inside
your network for private services. A Cloud account does not create a route into
your VPC. See [deployment modes](deployment-modes.md).

## Sign in and publish a policy

```console
skimasque login
skimasque org create "Acme"
skimasque init
```

In the console, verify the GitHub owner associated with your organisation before
using its repositories for CI access. Write a policy for your actual identity and
destination, test it locally, then save and publish it under **Policies**.

```toml
name = "staging-api"
[match]
repository = "acme/widget"
workflow = "deploy.yml"
branch = "main"
kind = "ci"

[[rules]]
application = "curl"
action = "allow"
destinations = ["api.staging.example.com:443"]

[[tests]]
application = "curl"
destination = "api.staging.example.com:443"
expect = "allow"

[[tests]]
application = "curl"
destination = "api.production.example.com:443"
expect = "deny"
```

```console
skimasque policy validate --strict
skimasque policy test
```

Replace the repository, workflow and destination. No matching allow rule means
deny when policy enforcement is enabled. Application names are declared context;
verified identity and destination rules provide the authorization boundary.

## Connect a GitHub Actions job

Start with explicit proxy mode for a proxy-aware tool:

```yaml
jobs:
  check-api:
    runs-on: ubuntu-24.04
    permissions:
      id-token: write
      contents: read
    steps:
      - uses: skimasque-dev/connect@v1
        with:
          mode: proxy
          proxy: gateway.skimasque.com:443
          audience: https://gateway.skimasque.com
          application: curl
      - run: curl --fail https://api.staging.example.com/health
```

Use an Action/client release containing the current interface; merging to main
does not move an existing release tag. Pin coordinated releases in production.
For a customer gateway, replace `proxy` and `audience` with its configured values.
Proxy mode exports HTTP, HTTPS and SOCKS proxy variables. It requires tools that
honour them; `psql` does not use `ALL_PROXY`.

For a private database on a dedicated Ubuntu runner, use **transparent mode** with
explicit private CIDRs, DNS servers and DNS routing domains. Ordinary TCP/UDP
sockets then use the tunnel for those routes. DNS and service IPs need matching
policy rules, and the gateway must permit those ranges through its address floor.
See [GitHub Actions](github-actions.md) and the
[Postgres example](../examples/github-actions/managed-postgres-migration.yml).

The Action supervises the client and registers a post-job cleanup hook. Public
traffic outside configured routes remains on the runner's normal network;
transparent mode does not confine the job.

## Verify the decision

Check **Audit** in the console for the identity, application, destination and
allow/deny reason. Test a denied destination through the same proxy or a configured
private route. A direct request outside the tunnel does not test gateway policy.
Publish policy revisions to change authorization for new tunnels.

## Run a local command

After publishing a policy for your developer identity:

```console
skimasque exec --app curl -- curl --fail https://api.staging.example.com/health
skimasque exec --forward 15432:db.internal:5432 -- psql -h 127.0.0.1 -p 15432
```

The second command requires its own matching database rule and a gateway that can
reach the database. Local listeners close when the command exits. For a coding
agent, use `exec --agent` with `--sandbox srt` or an explicit `--unsandboxed` choice;
read [agent sessions](agents.md) before granting access.

## Next

- [GitHub Actions](github-actions.md): modes, prerequisites, identity and cleanup.
- [Policies](policies.md): workload kinds, baselines and enforcement limits.
- [CLI](cli.md): exec, tunnels, agent sessions and management commands.
- [Gateways](gateways.md): deploy inside your network.
- [Self-hosting](self-hosting.md): operate a licensed or compatible control plane.
- [Troubleshooting](troubleshooting.md): diagnose routing and policy decisions.
