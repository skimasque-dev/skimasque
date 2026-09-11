# skimasque-protocol

The **SkiMasque control protocol** — the wire types and endpoint paths a
gateway (`skimasque-server --control-plane <url>`) and a control plane exchange:
registration, policy distribution (ETag + long-poll), heartbeats, label
declaration, credential minting, audit shipping, and the org signing key.

This crate is the *contract*, not an implementation. SkiMasque publishes it so
that a fully self-hosted deployment ("Mode 3") — your own control plane, your
own gateway — is possible without SkiMasque Cloud. Anything that serves these
endpoints with these JSON shapes can drive an open-source gateway.

It is pure data: `serde` shapes, the audit-chain hash, and the signing-key
encoding. No HTTP client, no async runtime.

## Part of skimasque

See the [project repository](https://github.com/skimasque-dev/skimasque),
`docs/protocol.md` for the endpoint reference, and `docs/deployment-modes.md`.

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option.
