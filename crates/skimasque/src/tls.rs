//! TLS and QUIC transport configuration.
//!
//! MASQUE runs over HTTP/3, so every configuration here pins ALPN to `h3` and
//! enables QUIC DATAGRAM frames. Getting either wrong fails in a confusing
//! place -- a missing ALPN looks like a handshake rejection, and disabled
//! datagrams looks like a tunnel that opens and then silently drops traffic --
//! so both are set in one place rather than left to callers.

use std::sync::Arc;

use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use crate::Error;

/// The ALPN protocol identifier for HTTP/3 (RFC 9114, Section 3.1).
pub const ALPN_H3: &[u8] = b"h3";

/// Install the ring crypto provider if no provider has been installed yet.
///
/// rustls requires exactly one process-wide default. Calling this repeatedly is
/// harmless: a second call finds a provider already installed and does nothing.
pub fn install_default_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Client TLS trusting the Mozilla root program, for proxies with real certificates.
pub fn client_config_with_webpki_roots() -> rustls::ClientConfig {
    install_default_crypto_provider();
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    finish_client_config(rustls::ClientConfig::builder().with_root_certificates(roots))
}

/// Client TLS trusting exactly the certificates in `ca_pem`.
///
/// This is the right way to talk to a proxy using a self-signed or private-CA
/// certificate: pin the certificate instead of turning verification off.
pub fn client_config_with_ca(ca_pem: &[u8]) -> Result<rustls::ClientConfig, Error> {
    install_default_crypto_provider();
    let mut roots = rustls::RootCertStore::empty();
    let mut added = 0;
    for cert in CertificateDer::pem_slice_iter(ca_pem) {
        roots
            .add(cert.map_err(|e| Error::Tls(format!("reading CA PEM: {e}")))?)
            .map_err(|e| Error::Tls(format!("adding CA certificate: {e}")))?;
        added += 1;
    }
    if added == 0 {
        return Err(Error::Tls("CA file contained no certificates".to_owned()));
    }
    Ok(finish_client_config(
        rustls::ClientConfig::builder().with_root_certificates(roots),
    ))
}

/// Client TLS that accepts **any** certificate, verifying nothing.
///
/// This removes the only protection the client has against a machine-in-the-
/// middle: an attacker who can reach the traffic can read and rewrite
/// everything inside the tunnel. It exists for local development against a
/// throwaway certificate. Prefer [`client_config_with_ca`], which is no harder
/// to use, for anything that leaves your machine.
pub fn dangerous_client_config_without_verification() -> rustls::ClientConfig {
    install_default_crypto_provider();
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(danger::AcceptAnyCertificate))
        .with_no_client_auth();
    finish_built_client_config(config)
}

fn finish_client_config(
    builder: rustls::ConfigBuilder<rustls::ClientConfig, rustls::client::WantsClientCert>,
) -> rustls::ClientConfig {
    finish_built_client_config(builder.with_no_client_auth())
}

fn finish_built_client_config(mut config: rustls::ClientConfig) -> rustls::ClientConfig {
    config.alpn_protocols = vec![ALPN_H3.to_vec()];
    config
}

/// Server TLS from a PEM certificate chain and private key.
pub fn server_config_from_pem(
    cert_chain_pem: &[u8],
    key_pem: &[u8],
) -> Result<rustls::ServerConfig, Error> {
    install_default_crypto_provider();
    let certs = CertificateDer::pem_slice_iter(cert_chain_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Error::Tls(format!("reading certificate PEM: {e}")))?;
    if certs.is_empty() {
        return Err(Error::Tls(
            "certificate file contained no certificates".to_owned(),
        ));
    }
    // `from_pem_slice` finds the first PKCS#8, PKCS#1 or SEC1 key and reports
    // `NoItemsFound` when there is none.
    let key = PrivateKeyDer::from_pem_slice(key_pem)
        .map_err(|e| Error::Tls(format!("reading private key PEM: {e}")))?;
    server_config_from_der(certs, key)
}

/// Server TLS from already-parsed DER material.
pub fn server_config_from_der(
    cert_chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<rustls::ServerConfig, Error> {
    install_default_crypto_provider();
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .map_err(|e| Error::Tls(format!("building server config: {e}")))?;
    config.alpn_protocols = vec![ALPN_H3.to_vec()];
    Ok(config)
}

/// A freshly generated self-signed certificate and the key that signed it.
#[cfg(feature = "self-signed")]
#[derive(Debug, Clone)]
pub struct SelfSigned {
    /// The certificate, DER-encoded.
    pub certificate: CertificateDer<'static>,
    /// The certificate as PEM, so a client can pin it with
    /// [`client_config_with_ca`].
    pub certificate_pem: String,
    /// The private key as PEM.
    pub key_pem: String,
}

/// Generate a self-signed certificate covering `subject_alt_names`.
///
/// Intended for development proxies and tests. Clients should be pointed at
/// [`SelfSigned::certificate_pem`] rather than told to skip verification.
#[cfg(feature = "self-signed")]
pub fn generate_self_signed(subject_alt_names: Vec<String>) -> Result<SelfSigned, Error> {
    let generated = rcgen::generate_simple_self_signed(subject_alt_names)
        .map_err(|e| Error::Tls(format!("generating self-signed certificate: {e}")))?;
    Ok(SelfSigned {
        certificate: generated.cert.der().clone(),
        certificate_pem: generated.cert.pem(),
        key_pem: generated.signing_key.serialize_pem(),
    })
}

/// Wrap client TLS into a QUIC client configuration.
pub fn quic_client_config(tls: rustls::ClientConfig) -> Result<quinn::ClientConfig, Error> {
    let crypto = QuicClientConfig::try_from(tls)
        .map_err(|e| Error::Tls(format!("QUIC requires TLS 1.3: {e}")))?;
    Ok(quinn::ClientConfig::new(Arc::new(crypto)))
}

/// Wrap server TLS into a QUIC server configuration with datagrams enabled.
pub fn quic_server_config(tls: rustls::ServerConfig) -> Result<quinn::ServerConfig, Error> {
    let crypto = QuicServerConfig::try_from(tls)
        .map_err(|e| Error::Tls(format!("QUIC requires TLS 1.3: {e}")))?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    config.transport_config(Arc::new(datagram_transport_config()));
    Ok(config)
}

/// A QUIC transport configuration with DATAGRAM frames explicitly enabled.
///
/// quinn enables datagrams by default, but MASQUE cannot work without them, so
/// this states the requirement rather than inheriting it.
pub fn datagram_transport_config() -> quinn::TransportConfig {
    let mut transport = quinn::TransportConfig::default();
    transport.datagram_receive_buffer_size(Some(1024 * 1024));
    transport.datagram_send_buffer_size(1024 * 1024);
    // Proxied flows are often idle between packets; the default 30s idle
    // timeout would tear down a working DNS tunnel between queries.
    transport.max_idle_timeout(Some(
        std::time::Duration::from_secs(60)
            .try_into()
            .expect("60s is a valid idle timeout"),
    ));
    transport.keep_alive_interval(Some(std::time::Duration::from_secs(15)));
    transport
}

mod danger {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, Error, SignatureScheme};

    /// A verifier that accepts every certificate. See
    /// [`dangerous_client_config_without_verification`](super::dangerous_client_config_without_verification).
    #[derive(Debug)]
    pub(super) struct AcceptAnyCertificate;

    impl ServerCertVerifier for AcceptAnyCertificate {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_configuration_pins_alpn_to_h3() {
        assert_eq!(
            client_config_with_webpki_roots().alpn_protocols,
            vec![ALPN_H3.to_vec()]
        );
        assert_eq!(
            dangerous_client_config_without_verification().alpn_protocols,
            vec![ALPN_H3.to_vec()]
        );
    }

    #[cfg(feature = "self-signed")]
    #[test]
    fn a_generated_certificate_can_be_loaded_by_both_ends() {
        let generated = generate_self_signed(vec!["localhost".to_owned()]).unwrap();
        server_config_from_pem(
            generated.certificate_pem.as_bytes(),
            generated.key_pem.as_bytes(),
        )
        .unwrap();
        let client = client_config_with_ca(generated.certificate_pem.as_bytes()).unwrap();
        assert_eq!(client.alpn_protocols, vec![ALPN_H3.to_vec()]);
    }

    #[test]
    fn empty_pem_input_is_an_error_rather_than_an_empty_trust_store() {
        assert!(client_config_with_ca(b"").is_err());
        assert!(server_config_from_pem(b"", b"").is_err());
    }
}
