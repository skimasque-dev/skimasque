# Live test: TCP + UDP through SkiMasque Cloud

A full, no-CI, live test of the enforcement loop: sign in to the real control
plane at `https://control.skimasque.com`, publish a policy, and open real
tunnels through the real shared gateway at `gateway.skimasque.com` -- one TCP,
one UDP -- proving both actually round-trip and that everything else is denied.

Nothing here is a mock. `control.skimasque.com` and `gateway.skimasque.com`
already exist and run continuously; you provide only an org, a policy, and
your own signed-in identity. The two destinations are public, well-known test
services (a TCP echo and a public DNS resolver), so this needs no
infrastructure of your own and works from a laptop.

```
your workstation                        SkiMasque Cloud            public test targets
  skimasque login ──session──▶  control.skimasque.com
  skimasque-client --org ──credential──▶  gateway.skimasque.com ──policy: ALLOW──▶  tcpbin.com:4242  (TCP)
                                                (deny by default)   ──policy: ALLOW──▶  8.8.8.8:53        (UDP)
                                                                     ✗────▶  anything else
```

## 0. Prerequisites

`skimasque` and `skimasque-client` on your `PATH` (build from this repo with
`cargo build --release -p skimasque-cli`, or use a release binary).

## 1. Sign in and find your identity

```console
$ skimasque login                 # opens the GitHub device flow against control.skimasque.com
$ skimasque whoami
octocat (https://control.skimasque.com)
  user id: ...
```

A credential minted from this session (no OIDC, no CI) carries **only your
GitHub login**, as `actor` -- nothing else. That is what the policy below
matches on.

## 2. Edit and test the policy locally

Open [`.masque/policies/live-test.toml`](.masque/policies/live-test.toml) and
replace `YOUR-GITHUB-LOGIN` with exactly what `skimasque whoami` printed.

From this directory, check it with no network:

```console
$ skimasque policy test
live-test
  ok   probe -> tcpbin.com:4242  (expect allow)
  ok   probe -> 8.8.8.8:53  (expect allow)
  ok   probe -> tcpbin.com:4242  (expect deny)
  ok   probe -> example.com:443  (expect deny)

4 assertions, all passed

$ skimasque policy validate
ok    live-test.toml (live-test)
```

`--strict` (the flag `getting-started.md` recommends for a repository-scoped
CI policy) is deliberately **not** run here: it fails this policy with
`[match-lacks-repository]`, because the lint assumes every `[match]` should
pin a repository. That doesn't apply to a policy scoped to a developer
session — a login-session credential has no repository to pin — so plain
`validate` (no `--strict`) is the right check for this one.

## 3. Create an org and publish the policy

```console
$ skimasque org create "Live test"      # or `skimasque org list` to reuse one you already have
Created Live test (org_...).
...
```

`--org` below always takes the **id** in parentheses (`org_...`), never the
display name — save it:

```console
$ export ORG=org_...
```

Publish the policy: in the dashboard (`https://control.skimasque.com`) open
the org → **Access → Policy editor**, paste `live-test.toml`, and **Publish**
it as revision 1.

## 4. Prove the TCP tunnel

```console
$ skimasque connect tcpbin.com:4242 --proxy gateway.skimasque.com:443 --org "$ORG" --app probe
tunnel open to tcpbin.com:4242; bridging stdin/stdout
hello from skimasque
hello from skimasque
```

Type a line and press enter -- tcpbin echoes it straight back over the same
tunnel. That is a real `CONNECT` round trip through `gateway.skimasque.com`,
authorized by the `tcp-echo` rule. `Ctrl-C` to close it.

(`--org` can be dropped if your account belongs to only one organisation in
total, not just this one — otherwise it's required.)

## 5. Prove the UDP tunnel

```console
$ skimasque-client --proxy gateway.skimasque.com:443 --org "$ORG" --app probe \
    probe --target 8.8.8.8:53 --dns example.com
tunnel to 8.8.8.8:53 open on stream ... (max payload ... bytes)
sending ... bytes:
...
reply 1: ... bytes in ...
dns reply id matches; answers: ...
```

A real DNS answer from `8.8.8.8`, over a UDP tunnel through the same gateway,
authorized by the `dns` rule.

## 6. Prove the deny

```console
$ skimasque connect example.com:443 --proxy gateway.skimasque.com:443 --org "$ORG" --app probe
Error: opening a TCP tunnel to example.com:443

Caused by:
    proxy refused the tunnel: 403 Forbidden (...)
```

`example.com:443` isn't in either rule, so it's refused -- no matching allow
rule means deny, even though the gateway can reach it fine. The refusal
carries the reason in `Proxy-Status`.

Two more denials worth trying, both already covered by `[[tests]]` above and
worth seeing live too: `probe --target tcpbin.com:4242 --text hi` (the
tcp-echo rule is TCP-only, so a UDP probe to the same host is refused), and
the same `connect`/`probe` commands with `--app` set to anything other than
`probe` (the rule's `application` has to match what the client declares).

## 7. Check the decision from both sides

```console
$ skimasque why tcpbin.com:4242 --app probe --actor YOUR-GITHUB-LOGIN --transport tcp \
    --control-plane --org "$ORG"
ALLOW

Identity:    (unspecified)
Actor:       YOUR-GITHUB-LOGIN
Application: probe
Transport:   tcp
Destination: tcpbin.com:4242
Policy:      live-test
Rule:        tcp-echo
```

This evaluates server-side against the org's *published* revision — the same
engine the gateway uses, so it's the same decision the gateway logged for
step 4 above, without re-running the tunnel. Drop `--control-plane` to check
it against the local file instead (that's what `policy test` did in step 2).

And the dashboard's **Activity** view for the org has one line per decision
from every step above — identity, application, destination, the rule or
denial reason, a timestamp; `skimasque audit --org "$ORG"` prints the same
from the terminal.

## Next

| You want… | See |
|---|---|
| The same loop, but from a CI job's OIDC identity instead of a login session | [`../github-actions/managed-postgres-migration.yml`](../github-actions/managed-postgres-migration.yml), [`../../docs/getting-started.md`](../../docs/getting-started.md) |
| The policy DSL in full | [`../../docs/policies.md`](../../docs/policies.md) |
| Every command and flag | [`../../docs/cli.md`](../../docs/cli.md) |
| Something failed | [`../../docs/troubleshooting.md`](../../docs/troubleshooting.md) |
