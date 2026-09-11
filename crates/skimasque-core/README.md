# skimasque-core

Wire-format types for [IETF MASQUE](https://datatracker.ietf.org/wg/masque/about/):
HTTP Datagrams, the Capsule Protocol, CONNECT-UDP and CONNECT-IP.

This crate is deliberately transport-free. It does no I/O, spawns no tasks, and
depends only on [`bytes`](https://crates.io/crates/bytes). Everything in it is a
codec or a validated value type, so the protocol can be tested exhaustively
without a network, and the same types serve a client, a proxy, and an
intermediary.

| Module | Specification |
|---|---|
| `varint` | QUIC variable-length integers ([RFC 9000](https://www.rfc-editor.org/rfc/rfc9000) §16) |
| `datagram` | HTTP Datagrams and context ids ([RFC 9297](https://www.rfc-editor.org/rfc/rfc9297) §2) |
| `capsule` | The Capsule Protocol ([RFC 9297](https://www.rfc-editor.org/rfc/rfc9297) §3) |
| `template` | Proxy URI Templates ([RFC 6570](https://www.rfc-editor.org/rfc/rfc6570), profiled by [RFC 9298](https://www.rfc-editor.org/rfc/rfc9298)) |
| `target` | The `host:port` target address, shared by CONNECT-UDP and CONNECT-TCP |
| `connect_udp` | Proxying UDP in HTTP ([RFC 9298](https://www.rfc-editor.org/rfc/rfc9298)) |
| `connect_tcp` | Proxying TCP in HTTP ([`draft-ietf-httpbis-connect-tcp`](https://datatracker.ietf.org/doc/draft-ietf-httpbis-connect-tcp/)) |
| `connect_ip` | Proxying IP in HTTP ([RFC 9484](https://www.rfc-editor.org/rfc/rfc9484)) |

The transport that carries these formats — QUIC, HTTP/3, and the proxy service —
lives in the [`skimasque`](https://crates.io/crates/skimasque) crate.

`#![forbid(unsafe_code)]`.

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
