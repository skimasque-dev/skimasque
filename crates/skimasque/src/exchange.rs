//! Token exchange: a stronger identity token in, a short-lived platform
//! credential out.
//!
//! A gateway that verifies GitHub OIDC does it once, here, rather than on every
//! tunnel. A client POSTs its OIDC token to
//! [`CREDENTIAL_EXCHANGE_PATH`] on the same authority it opens tunnels on, with
//! `Authorization: Bearer <oidc>`, and gets back a JSON body carrying a
//! credential the gateway signed itself. Tunnels then present *that* in
//! `Proxy-Authorization`, and the gateway verifies it locally.
//!
//! The gateway side is a [`CredentialMinter`]; `skimasque-cli` builds one from
//! `skimasque-identity`. The client side is
//! [`Session::exchange_credential`](crate::client::Session::exchange_credential).

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Where a client POSTs an identity token to exchange it for a credential.
///
/// Not a tunnel path, so it does not go through the URI Template; it is a fixed
/// well-known location on the gateway's authority.
pub const CREDENTIAL_EXCHANGE_PATH: &str = "/.well-known/masque/skimasque-credential";

/// Verifies a presented identity token and issues a platform credential for it.
///
/// A gateway holds one when it is started with identity verification enabled.
/// `mint` takes an owned token and returns a `'static` future so the server can
/// drive it without borrowing the request.
pub trait CredentialMinter: Send + Sync + std::fmt::Debug {
    fn mint(
        &self,
        identity_token: String,
    ) -> Pin<Box<dyn Future<Output = Result<MintedCredential, MintError>> + Send>>;
}

/// A credential a [`CredentialMinter`] has issued.
#[derive(Debug, Clone)]
pub struct MintedCredential {
    /// The credential, opaque to the client.
    pub credential: String,
    /// How long it is valid from now.
    pub expires_in: Duration,
}

/// Why an exchange did not produce a credential.
#[derive(Debug)]
pub enum MintError {
    /// The presented identity token did not verify. Answered `403`.
    Unauthorized(String),
    /// The minter could not complete -- a key fetch failed, say. Answered
    /// `502`, since a retry might succeed.
    Unavailable(String),
}

impl std::fmt::Display for MintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized(detail) => write!(f, "unauthorized: {detail}"),
            Self::Unavailable(detail) => write!(f, "unavailable: {detail}"),
        }
    }
}

impl std::error::Error for MintError {}

/// The JSON body of a successful exchange -- OAuth token-response shaped.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct ExchangeBody {
    pub credential: String,
    pub token_type: String,
    pub expires_in: u64,
}
