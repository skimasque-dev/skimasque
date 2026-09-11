# Fuzz targets

`cargo-fuzz` / libFuzzer harnesses for the parsers that run on untrusted input:
the hand-rolled wire-format decoders in `skimasque-core` (client-controlled
bytes, decoded before authorization) and the `skimasque-policy` document parser
(operator files, re-read on hot-reload).

| Target | Exercises |
|---|---|
| `varint` | QUIC variable-length integer decode (RFC 9000 §16) |
| `http3_datagram` | QUIC DATAGRAM frame → quarter-stream-id + payload |
| `proxying_payload` | HTTP Datagram Payload (context id + inner bytes) |
| `capsule_decoder` | Capsule Protocol framing (RFC 9297), fed in arbitrary chunks |
| `connect_udp_payload` | CONNECT-UDP datagram payload decode (RFC 9298) |
| `connect_ip` | CONNECT-IP packet payload + ADDRESS_* capsule decode (RFC 9484) |
| `uri_template` | URI Template parse (RFC 6570 subset) |
| `target_parse` | `host:port` and bare-host parsing |
| `policy_document` | `Policy::from_toml` / `from_yaml` and the `[limits]` unit parsers |

Targets that decode-then-re-encode also assert the round trip.

## Running

Needs a nightly toolchain and `cargo-fuzz` (`cargo install cargo-fuzz`). This is
its own workspace, so run from `fuzz/` — or with `--fuzz-dir fuzz` from the root.

```console
$ cargo +nightly fuzz list
$ cargo +nightly fuzz run varint
$ cargo +nightly fuzz run policy_document -- -max_total_time=120
```

Findings land in `fuzz/artifacts/<target>/`; reproduce one with
`cargo +nightly fuzz run <target> fuzz/artifacts/<target>/<crash-file>`.

CI (`fuzz` job) builds every target on each push and gives each a short run;
longer campaigns and a persistent corpus are a follow-up (OSS-Fuzz).
