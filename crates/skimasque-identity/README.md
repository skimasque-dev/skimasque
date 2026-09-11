# skimasque-identity

Verify workload-identity tokens (GitHub Actions OIDC and other CI issuers) and
map their claims onto the `WorkloadIdentity` a policy engine evaluates.

[`skimasque-policy`](https://crates.io/crates/skimasque-policy) takes a
`WorkloadIdentity` as data and never asks where it came from. This crate is one
place it comes from: an OIDC token from GitHub Actions, GitLab CI, Buildkite, or
any issuer, verified against the issuer's published signing keys (RS256, with
`iss` / `aud` / `exp` checks), with its claims mapped by a `Provider` onto the
fields a policy matches — `organization`, `repository`, `workflow`, `ref`,
`environment`, `actor`.

```rust
use skimasque_identity::{OidcVerifier, Provider};

// The audience is whatever the workflow asks the issuer to mint the token for.
let verifier = OidcVerifier::hosted(Provider::GitHubActions, ["https://masque.example"])?;
let identity = verifier.verify("<the OIDC JWT>").await?;
assert_eq!(identity.organization.as_deref(), Some("acme"));
```

Verification is split so the cryptography is testable without a network:
`Verifier::verify` is a pure function from `(token, keys)` to `Claims`,
`Provider::identify` maps those, and `JwksCache` over a `JwksProvider` handles
fetching and caching the keys. The default `remote` feature supplies an HTTPS
`JwksProvider`; disable it (`default-features = false`) to bring your own.

This crate also issues and verifies the short-lived platform credential a
gateway hands back after token exchange, so a tunnel can be authorized locally
with no network round-trip.

## Part of skimasque

skimasque is identity-aware, least-privilege network access for CI/CD jobs and
developers, built on a from-scratch implementation of IETF MASQUE in Rust. See
the [project repository](https://github.com/skimasque-dev/skimasque).

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option. Unless you explicitly state
otherwise, any contribution intentionally submitted for inclusion in this crate
by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
