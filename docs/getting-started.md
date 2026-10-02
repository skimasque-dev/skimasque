# Getting started: test your first CI connection

Create a GitHub Actions job that reaches `http://hello.skimasque.com:8080/`
through `gateway.skimasque.com:443`, then prove that an unlisted destination is
denied. The hello service uses private DNS and is reachable from the managed
gateway; your runner does not need to resolve or reach it directly.

## 1. Prepare your GitHub repository

Use a repository you can commit to and run Actions in. This guide uses
`YOUR_OWNER/YOUR_REPO`, the branch `main`, and the workflow file
`hello-skimasque.yml`. Substitute your real owner and repository everywhere;
replace `main` in both the policy and workflow if your default branch differs.
Enable GitHub Actions in the repository's **Settings → Actions → General** and
allow `skimasque-dev/connect` if your organisation restricts third-party Actions.
Use the GitHub-hosted `ubuntu-24.04` runner for this walkthrough.

No SkiMasque token or GitHub personal access token goes in repository secrets.
The job's `id-token: write` permission lets the Action request GitHub OIDC.

## 2. Create an organisation and verify the GitHub owner

Open the [SkiMasque console](https://control.skimasque.com/), sign in with GitHub,
and create or select your SkiMasque organisation. Under
[identity settings](https://control.skimasque.com/app/settings/identity), verify
the GitHub user or organisation that owns `YOUR_OWNER/YOUR_REPO`, following the
verification flow or installing the GitHub App. Confirm the owner is verified
in the selected SkiMasque organisation. Signing in alone does not associate all
your repositories with an organisation.

Use the shared managed gateway for this test; no gateway VM, private DNS setup
on the runner, or local CLI installation is required. The managed deployment
must have its hello test endpoint enabled.

## 3. Publish the test policy

Open [Policies](https://control.skimasque.com/app/policies) in that organisation
and open the policy editor. Under **Add a document**, enter `hello-ci.toml`,
leave the target field empty for the shared gateway, and paste:

```toml
name = "hello-ci"

[match]
repository = "YOUR_OWNER/YOUR_REPO"
workflow = "hello-skimasque.yml"
branch = "main"
kind = "ci"

[[rules]]
application = "skimasque-test"
transport = "tcp"
action = "allow"
destinations = ["hello.skimasque.com:8080"]

[[tests]]
application = "skimasque-test"
destination = "hello.skimasque.com:8080"
expect = "allow"

[[tests]]
application = "skimasque-test"
destination = "hello.skimasque.com:8081"
expect = "deny"
```

Replace `YOUR_OWNER/YOUR_REPO`, click **Save draft**, and check that validation
and both policy tests pass. Click **Publish revision**. Saving a draft alone
does not change gateway authorization. Under **Gateways**, confirm the shared
gateway is healthy and has acknowledged the published policy revision before
running the job.

New organisations may already have a `skimasque-test-OWNER.toml` starter policy
allowing `skimasque-test` to reach the hello service from that verified owner's
repositories. The policy above gives this walkthrough an explicit repository,
workflow and branch scope. If you want only that scope, remove the broader
starter document from the draft before publishing. Preserve unrelated policies;
ensure none allows `hello.skimasque.com:8081`, which is the denial test below.

## 4. Create the complete workflow

This repository includes the [hello workflow](../.github/workflows/hello-skimasque.yml)
and its [hello CI policy](../.github/policies/hello-ci.toml), scoped to
`skimasque-dev/skimasque` on `main`. Repository CI validates the policy; publish
it in the console before manually running the workflow on `main`.

Create `.github/workflows/hello-skimasque.yml` in your repository and paste:

```yaml
name: Hello through SkiMasque

on:
  workflow_dispatch:

jobs:
  hello:
    runs-on: ubuntu-24.04
    timeout-minutes: 5
    permissions:
      contents: read
      id-token: write
    steps:
      - name: Connect to the managed gateway
        id: network
        uses: skimasque-dev/connect@v2
        with:
          mode: proxy
          proxy: gateway.skimasque.com:443
          audience: https://gateway.skimasque.com
          application: skimasque-test

      - name: Reach the allowed hello service
        shell: bash
        env:
          SKIMASQUE_HTTP_PROXY: ${{ steps.network.outputs.http-proxy }}
        run: |
          curl --fail-with-body --show-error --silent \
            --connect-timeout 10 --max-time 30 \
            --proxy "$SKIMASQUE_HTTP_PROXY" --noproxy '' \
            http://hello.skimasque.com:8080/ | tee hello-response.txt
          grep -Fx 'private endpoint ok' hello-response.txt

      - name: Verify an unlisted port is denied
        shell: bash
        env:
          SKIMASQUE_HTTP_PROXY: ${{ steps.network.outputs.http-proxy }}
        run: |
          status=$(curl --show-error --silent \
            --connect-timeout 10 --max-time 30 \
            --proxy "$SKIMASQUE_HTTP_PROXY" --noproxy '' \
            --output denied-response.txt --write-out '%{http_code}' \
            http://hello.skimasque.com:8081/)
          test "$status" = 403 || {
            echo "Expected gateway policy denial (403), got $status"
            cat denied-response.txt
            exit 1
          }
          echo 'Gateway refused the unlisted port (403).'
```

Commit the file to your default branch (`main` here). A checkout step is not
needed because the job only uses the Action and curl. `mode: proxy` is explicit:
the Action defaults to transparent mode, which needs routes and DNS inputs.
The application name must match the policy; it is declared context, not proof
of the executable. The explicit proxy output and `--noproxy ''` ensure both
requests use the gateway even if the runner has inherited proxy exclusions.
The gateway resolves the hello hostname.

`@v2` selects the moving Action major release and, without `version`, the latest
client release. For reproducible production jobs, pin the Action to a reviewed
commit and set `with.version` to a compatible client release providing
`proxy-ready-v1`. Action and client versions are independent.

## 5. Run the job and check its output

In GitHub, open **Actions → Hello through SkiMasque → Run workflow**, select
`main` (or the branch you placed in the policy), and click **Run workflow**.
Open the run and expand the `hello` job's steps.

- **Connect to the managed gateway** must finish successfully: the Action
  exchanges GitHub OIDC and starts authenticated local proxy listeners.
- **Reach the allowed hello service** must print `private endpoint ok`, followed
  by `host:`, `seen client:` and `path: /`. The client address is the gateway's
  internal address. The body assertion and curl's HTTP error check must pass.
- **Verify an unlisted port is denied** must print
  `Gateway refused the unlisted port (403).` A timeout, DNS failure, connection
  refusal or 502 is a failed test, not evidence of policy denial.

The job should be green, including the Action's post-job cleanup. HTTP port
8080 is intentional; changing the hello URL to HTTPS or omitting the port tests
a different destination. The runner-to-gateway connection uses encrypted QUIC;
the demo service speaks HTTP on the gateway's internal network.

## 6. Verify the gateway's audit decisions

Open [Audit](https://control.skimasque.com/app/audit) in the same organisation.
Find the entries around the workflow run time and inspect the repository,
workflow `hello-skimasque.yml`, branch/ref `main` / `refs/heads/main`, workload
kind `ci`, and application `skimasque-test`. Confirm an **allow** for
`hello.skimasque.com:8080` and a **deny** for `hello.skimasque.com:8081`, with
the policy reason. Audit ingestion can take a short time; refresh if necessary.

Together, the expected response, explicit proxy path, 403 assertion and audit
decisions verify connectivity and enforcement. Curling the service directly
does not test gateway policy.

## 7. Troubleshoot a failed run

| Symptom | What to check |
|---|---|
| Workflow missing or no Run workflow button | Commit to the default branch, keep `workflow_dispatch`, and enable Actions. |
| OIDC or credential exchange fails | Keep job-level `id-token: write`, the exact audience `https://gateway.skimasque.com`, and a verified repository owner in the selected organisation. |
| Action cannot connect or times out | Check managed gateway health and UDP 443 reachability; QUIC requires UDP. Inspect the Action's startup diagnostics. |
| Hello request returns 403 | Check the published revision and gateway acknowledgement, actual repository/branch/workflow filename, `kind = "ci"`, application and port. Read the audit denial reason. |
| Hello request returns 502 | Check gateway-side DNS/reachability and that the managed hello endpoint is enabled. Do not add public runner DNS overrides for this private service. |
| Denial test returns something other than 403 | Check audit and other matching policies for a broader allow. A network failure does not prove authorization was denied. |
| Policy changes have no effect | Publish the saved draft, check the selected organisation and gateway revision, then rerun the job to create new tunnels. |

## 8. Clean up and adapt the job

The Action's post hook stops its client and restores previous proxy variables
on normal job success or failure. Remove `hello-ci.toml` (and any unused starter
test document) from the console draft and **publish** the removal when done.
Delete the workflow file if you no longer need the test.

To reach your own service, replace the destination and port in both policy and
workflow, set the application context consistently, and use a gateway that can
reach that service. The shared gateway does not automatically reach your VPC.
See [GitHub Actions](github-actions.md) for proxy and transparent modes and
[deployment modes](deployment-modes.md) for customer gateways.

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
