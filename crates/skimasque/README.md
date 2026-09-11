# skimasque

MASQUE proxying over HTTP/3: a CONNECT-UDP client and a `tower`-based proxy
service, on a from-scratch implementation of the wire protocol.

This crate carries [`skimasque-core`](https://crates.io/crates/skimasque-core)'s
wire formats over a real transport — QUIC via
[`quinn`](https://crates.io/crates/quinn), HTTP/3 via
[`h3`](https://crates.io/crates/h3), and HTTP Datagrams framed here. It provides
a `client` for opening tunnels and a `server` that serves them, with the proxy's
authorization decisions expressed as a `tower::Service` stack.

## What is implemented

- **[RFC 9297](https://www.rfc-editor.org/rfc/rfc9297)** HTTP Datagrams and the
  Capsule Protocol, in both encodings: QUIC DATAGRAM frames and DATAGRAM
  capsules on the request stream.
- **[RFC 9298](https://www.rfc-editor.org/rfc/rfc9298)** Proxying UDP in HTTP,
  over HTTP/3 extended CONNECT.
- **`draft-ietf-httpbis-connect-tcp`** classic `CONNECT host:port` tunnels
  (opt-in).
- **[RFC 9484](https://www.rfc-editor.org/rfc/rfc9484)** CONNECT-IP wire
  formats; the transport is behind the `connect-ip` feature and TUN forwarding
  is not wired up yet.

## The enforcement stack

The proxy is a `tower::Service` wrapped in layers that each answer one question
and fail closed:

- `IdentityLayer` — verifies the credential (or OIDC token) in
  `Proxy-Authorization: Bearer`, recovers the `WorkloadIdentity`, and leaves it
  in the request extensions.
- `PolicyLayer` — evaluates the
  [`skimasque-policy`](https://crates.io/crates/skimasque-policy) set and either
  attaches an `AuthorizedDestination` or answers `403` with the reason in
  `Proxy-Status`. Has an observe mode that logs what the policy *would* decide.
- `QuotaLayer` — enforces the decision's `[limits]`: concurrent-tunnel cap,
  bandwidth and packet-rate caps, and per-tunnel transfer ceiling.
- `AddressPolicy` — the SSRF floor, checked against *resolved* addresses:
  loopback, RFC 1918, CGNAT, link-local and multicast are refused unless a
  deployment opts a range in.

The innermost proxy only ever forwards to an `AuthorizedDestination`, never a
bare target, so a forwarding path cannot be written without something having
authorized it.

## Two things worth knowing

The RFC 9297 datagram frame is encoded in this crate rather than by
`h3-datagram`, whose `Datagram::encode` computes the Quarter Stream ID and then
emits zeroes in its place — correct only for stream 0, and silently wrong for
every tunnel after the first.

Tunnels have UDP semantics end to end. Sending does not block and does not
guarantee delivery; receiving may miss datagrams. Adding reliability would put a
second retransmission layer underneath whatever the tunnel carries.

## Features

| Feature | Default | Effect |
|---|---|---|
| `self-signed` | yes | generate throwaway certificates, for development proxies and tests |
| `acme` | no | Let's Encrypt issuance and renewal for the gateway, via TLS-ALPN-01 |
| `connect-ip` | no | the HTTP/3 plumbing and address-pool proxy for CONNECT-IP |

## Part of skimasque

skimasque is identity-aware, least-privilege network access for CI/CD jobs and
developers. The ready-to-run gateway and tunnel binaries are in
[`skimasque-cli`](https://crates.io/crates/skimasque-cli); the full picture is
in the [project repository](https://github.com/skimasque-dev/skimasque).

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option. Unless you explicitly state
otherwise, any contribution intentionally submitted for inclusion in this crate
by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
