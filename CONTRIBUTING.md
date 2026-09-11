# Contributing to SkiMasque

Thanks for your interest. SkiMasque gives CI/CD jobs identity-aware,
least-privilege network access — a from-scratch Rust implementation of IETF
MASQUE plus a policy layer.

## Scope

This repository is the **open-source edge**: the gateway (`skimasque-server`),
the tunnel client (`skimasque-client`), the `skimasque` CLI, the policy engine
(`skimasque-policy`), the wire formats (`skimasque-core`), OIDC identity
(`skimasque-identity`), and the control protocol (`skimasque-protocol`).

SkiMasque Cloud's control-plane *implementation* is a separate, proprietary
component. A gateway talks to a control plane over the **open** protocol
(`--control-plane <url>`), documented in
[`docs/protocol.md`](docs/protocol.md) — so a fully self-hosted deployment
(Mode 3) is possible. See [`docs/deployment-modes.md`](docs/deployment-modes.md)
and [`docs/architecture.md`](docs/architecture.md).

## Before you open a PR

```console
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo deny check                 # advisories, licenses, bans
```

Requires Rust **1.88+** (the workspace `rust-version`; the `msrv` CI job builds
on exactly that toolchain). The end-to-end tests in `crates/*/tests/` run real
QUIC / HTTP/3 / UDP on loopback — no transport mocks.

- Match the surrounding code's style, naming, and comment density.
- Keep the policy engine (`skimasque-policy`) free of I/O and HTTP — it is a
  pure function from a request to a decision.
- Keep `skimasque-protocol` pure data — it is the contract, not an
  implementation. A wire change is a `PROTOCOL_VERSION` decision.
- Security-relevant changes: read [`docs/threat-model.md`](docs/threat-model.md)
  first and note in the PR which trust boundary the change touches.
- New wire parsing gets a `fuzz/` target.

More detail: [`docs/development.md`](docs/development.md).

## Reporting security issues

Do **not** open a public issue. See [`SECURITY.md`](SECURITY.md).

## Licensing

Contributions are dual-licensed under MIT OR Apache-2.0, matching the project.
By submitting a PR you agree to license your work under those terms.
