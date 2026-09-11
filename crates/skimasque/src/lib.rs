//! MASQUE proxying over HTTP/3.
//!
//! This crate carries [`skimasque_core`]'s wire formats over a real transport:
//! QUIC via `quinn`, HTTP/3 via `h3`, and HTTP Datagrams framed here. It
//! provides a [`client`] for opening CONNECT-UDP tunnels and a [`server`] that
//! serves them, with the proxy's decisions expressed as a [`tower::Service`].
//!
//! # What is implemented
//!
//! - **RFC 9297** HTTP Datagrams and the Capsule Protocol, in both encodings:
//!   QUIC DATAGRAM frames and DATAGRAM capsules on the request stream.
//! - **RFC 9298** Proxying UDP in HTTP, over HTTP/3 extended CONNECT.
//! - **RFC 9484** CONNECT-IP wire formats, in [`skimasque_core::connect_ip`].
//!   The transport is not wired up yet: `h3` 0.0.8 does not recognise
//!   `connect-ip` as a `:protocol` value, so a server built on it rejects such
//!   requests before this crate ever sees them.
//!
//! # Two things worth knowing
//!
//! The RFC 9297 datagram frame is encoded in this crate rather than by
//! `h3-datagram`, because that crate's `Datagram::encode` computes the Quarter
//! Stream ID and then emits zeroes in its place -- correct only for stream 0,
//! and silently wrong for every tunnel after the first.
//!
//! Tunnels have UDP semantics end to end. Sending does not block and does not
//! guarantee delivery; receiving may miss datagrams. Adding reliability would
//! put a second retransmission layer underneath whatever the tunnel carries.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

mod capsules;

#[cfg(feature = "acme")]
pub mod acme;
pub mod audit;
pub mod client;
#[cfg(feature = "connect-ip")]
pub mod connect_ip;
pub mod dgram;
pub mod exchange;
pub mod metrics;
pub mod policy;
pub mod server;
pub mod service;
pub mod tls;

pub use audit::{AuditEvent, AuditSink, JsonlAuditSink, TracingAuditSink};
pub use client::{Client, Credential, Session, TcpTunnel, UdpTunnel};
pub use exchange::{CredentialMinter, MintError, MintedCredential, CREDENTIAL_EXCHANGE_PATH};
pub use policy::AddressPolicy;
pub use server::{ConnectionRate, ProxyConfig, ResourceLimits, Server, TlsReloader};
pub use service::{
    Accepted, AuthorizeLayer, AuthorizedDestination, Dispatch, IdentityLayer, IdentityVerifier,
    PolicyHandle, PolicyLayer, QuotaLayer, RateLimiter, Rejection, TcpProxy, TunnelGuard,
    TunnelLimits, TunnelRequest, UdpProxy, APPLICATION_HEADER,
};

/// Re-exported so a proxy can build a [`PolicyLayer`] without a separate
/// dependency line.
pub use skimasque_policy as policy_engine;

/// Re-exported so callers do not have to depend on `skimasque-core` directly.
pub use skimasque_core as core;

/// Re-exported so callers can build TLS configuration without pinning their own
/// `rustls` version against this crate's.
pub use rustls;

/// Anything that can go wrong setting up or running a tunnel.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("TLS configuration: {0}")]
    Tls(String),

    /// ACME certificate setup failed (bad domain, unwritable cache directory).
    /// Only issuance *setup* fails here; a later ordering or renewal error is
    /// logged and retried, never surfaced as an error.
    #[error("ACME: {0}")]
    Acme(String),

    #[error(transparent)]
    Template(#[from] skimasque_core::template::Error),

    #[error(transparent)]
    Target(#[from] skimasque_core::connect_udp::Error),

    #[error("QUIC connect: {0}")]
    Connect(#[from] quinn::ConnectError),

    #[error("QUIC connection: {0}")]
    Connection(#[from] quinn::ConnectionError),

    #[error("HTTP/3 connection: {0}")]
    H3Connection(#[from] h3::error::ConnectionError),

    #[error("HTTP/3 stream: {0}")]
    H3Stream(#[from] h3::error::StreamError),

    /// The proxy answered with something other than 2xx, which RFC 9298
    /// Section 3.5 defines as a failed request.
    #[error("proxy refused the tunnel: {status}{}", .proxy_status.as_deref().map(|s| format!(" ({s})")).unwrap_or_default())]
    Rejected {
        status: http::StatusCode,
        /// The `Proxy-Status` header, if the proxy explained itself.
        proxy_status: Option<String>,
    },

    /// MASQUE cannot work without QUIC DATAGRAM frames, and the peer did not
    /// offer them.
    #[error("peer does not support QUIC datagrams")]
    DatagramsUnavailable,

    #[error("sending datagram: {0}")]
    Datagram(#[from] dgram::SendError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The gateway's credential-exchange endpoint refused or could not complete.
    #[error("credential exchange failed: {status}{}", .detail.as_deref().map(|d| format!(" ({d})")).unwrap_or_default())]
    ExchangeFailed {
        status: http::StatusCode,
        detail: Option<String>,
    },

    #[error("{0}")]
    Invalid(String),
}
