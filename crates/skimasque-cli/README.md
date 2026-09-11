# skimasque-cli

The `skimasque` command-line tool, plus the `skimasque-server` gateway and
`skimasque-client` tunnel binaries it drives.

```console
$ cargo install skimasque-cli
```

installs three binaries:

| Binary | Role |
|---|---|
| **`skimasque`** | the front door: scaffold and check policy locally, run a gateway or open a tunnel, sign in to skimasque's control plane, and manage an organisation's fleet |
| **`skimasque-server`** | the gateway data plane — verifies identity, enforces policy, and relays authorized tunnels |
| **`skimasque-client`** | opens tunnels: a one-shot probe, a raw `CONNECT` stream, or a local SOCKS5 relay in front of the tunnel |

## Check a policy without a network

```console
$ skimasque policy check production db.production.example.com:5432 --app terraform
ALLOW

Policy: production
Rule: terraform-production
Max duration: 20m

$ skimasque why db.production.example.com:5432 --app terraform \
      --repository acme/widget --branch main
ALLOW
...
```

`skimasque policy test` runs the `[[tests]]` assertions that travel with a
policy; `skimasque policy learn` turns observed traffic into a reviewable draft.

## Run an enforcing gateway

```console
$ skimasque gateway --listen 0.0.0.0:443 \
      --hostname gw.example.com --acme --acme-email ops@example.com \
      --policy-dir .masque/policies --policy-reload
```

`--acme` (a flag; the certificate name is `--hostname`) obtains and renews a
Let's Encrypt certificate in process, so clients connect with no pinned CA.
`--policy-reload` swaps the policy set on a file change without dropping a live
tunnel.

## With skimasque's control plane

`skimasque login` runs the GitHub device flow against
`https://control.skimasque.com` and stores a session;
`skimasque org`, `skimasque gateway register`, `skimasque audit`, and
`skimasque status` then manage organisations, enrol gateways, query the fleet's
decisions, and show the fleet. `skimasque why --control-plane` evaluates a
request server-side against the org's published policy.

## Part of skimasque

skimasque is identity-aware, least-privilege network access for CI/CD jobs and
developers, built on a from-scratch implementation of IETF MASQUE in Rust. See
the [project repository](https://github.com/skimasque-dev/skimasque) for the
policy DSL, the GitHub Actions integration, deployment artifacts, and the
roadmap.

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option. Unless you explicitly state
otherwise, any contribution intentionally submitted for inclusion in this crate
by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
