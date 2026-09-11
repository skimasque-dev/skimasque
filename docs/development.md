# Development

> **Applies to:** this repository (the open-source components)

Requires Rust **1.88+** (the workspace `rust-version`; an `msrv` CI job builds
`skimasque-cli` on exactly that toolchain so it cannot drift).

```console
$ cargo test --workspace
$ cargo clippy --workspace --all-targets -- -D warnings
$ cargo fmt --all --check
$ cargo build -p skimasque --features connect-ip     # wire up the IP path
$ cargo deny check                                    # advisories, licenses, bans
$ cd fuzz && cargo +nightly fuzz run varint           # fuzz a wire parser
```

## Layout

| Path | What |
|---|---|
| `crates/skimasque-core` | wire formats — QUIC varints, HTTP Datagrams, the Capsule Protocol, CONNECT-UDP/IP payloads. No I/O. |
| `crates/skimasque-policy` | the policy engine — model, TOML+YAML parsers, evaluator, tests, learning mode. No I/O. |
| `crates/skimasque-identity` | CI OIDC verification; the HS256 platform credential. |
| `crates/skimasque-protocol` | the Gateway↔control-plane wire contract. Pure data. |
| `crates/skimasque` | the transport (`quinn` + `h3`), the proxy tower stack, the token-exchange endpoint. |
| `crates/skimasque-cli` | the three binaries — `skimasque`, `skimasque-server`, `skimasque-client`. |
| `fuzz/` | libFuzzer targets for every hand-rolled wire and policy parser. |
| `deploy/` | the gateway's `Dockerfile`, systemd unit, Helm chart, Terraform. |
| `docs/` | this documentation. |

The control-plane server implementation is **not** in this repo (see
[`architecture.md`](architecture.md)). This repo builds and ships the gateway,
client, CLI, and libraries.

## Tests

The end-to-end tests in `crates/skimasque/tests/` and
`crates/skimasque-cli/tests/` run real QUIC connections, real HTTP/3 exchanges,
and real UDP sockets on loopback — no mocks in the transport path.
`.github/workflows/e2e-oidc.yml` runs the whole OIDC path against real GitHub
OIDC on every push. `fuzz/` targets are built and briefly run in CI.

## Dependency notes

- **`h3`** is pinned to a `master` revision via `[patch.crates-io]` (the
  released 0.0.8 cannot express `connect-ip` / `connect-tcp` templates). Bump it
  deliberately in a reviewed PR. Because of the patch, `skimasque` and
  `skimasque-cli` cannot publish to crates.io — they ship as release binaries
  and the gateway image. `skimasque-core` / `-policy` / `-identity` / `-protocol`
  do publish.
- `cargo deny` `[sources]` allows only the pinned `h3` git URL.

## Making changes

- Match the surrounding code's style, naming, and comment density.
- Keep `skimasque-policy` free of I/O and HTTP — it is a pure function from a
  request to a decision.
- Keep `skimasque-protocol` pure data — no HTTP client, no async runtime; it is
  the contract, not an implementation.
- New wire parsing gets a `fuzz/` target.
- Security-relevant changes: read [`threat-model.md`](threat-model.md) first and
  note in the PR which trust boundary the change touches.

## Releasing

A `v*` tag triggers: `release.yml` (multi-arch binaries + the gateway image +
an SPDX SBOM + provenance once public) and `crates-io.yml` (publishes the four
publishable library crates). See [`CONTRIBUTING.md`](../CONTRIBUTING.md).
