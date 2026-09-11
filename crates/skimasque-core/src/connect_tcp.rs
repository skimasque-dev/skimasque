//! Proxying TCP in HTTP (`draft-ietf-httpbis-connect-tcp`).
//!
//! There is deliberately very little here. A CONNECT-TCP tunnel carries a raw,
//! ordered byte stream, and the QUIC request stream already provides exactly
//! that: there is no HTTP Datagram framing and no Capsule Protocol on the
//! stream, so the success response must **not** carry `Capsule-Protocol`.
//!
//! Two request shapes reach the same tunnel:
//!
//! * **Classic `CONNECT`** — method `CONNECT`, the `host:port` target in
//!   `:authority`, no `:scheme` / `:path` / `:protocol`. This is what the
//!   current `h3` release accepts, and what skimasque implements first.
//! * **Template-driven** — extended `CONNECT` with `:protocol = connect-tcp`
//!   and the target in the path, exactly like CONNECT-UDP. This uses
//!   [`UPGRADE_TOKEN`] and [`crate::template::UriTemplate::default_connect_tcp`].
//!
//! The [`Target`] type is shared with CONNECT-UDP; both specs encode the target
//! as `target_host` / `target_port`.

/// The target address of a CONNECT-TCP request, shared with CONNECT-UDP.
pub use crate::target::{Error, Target, TargetHost};

/// The HTTP upgrade token, used as the `:protocol` pseudo-header for the
/// template-driven variant. Classic `CONNECT` carries no token on the wire.
pub const UPGRADE_TOKEN: &str = "connect-tcp";
