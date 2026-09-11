//! ACME (Let's Encrypt) certificate issuance and renewal for the gateway,
//! using the **TLS-ALPN-01** challenge.
//!
//! [`rustls_acme`] runs the ACME protocol; this module wires it to the
//! gateway's QUIC endpoint. Build a [`rustls::ServerConfig`] with
//! [`Acme::server_config`] and hand it to [`Server::bind`](crate::Server::bind)
//! — it carries the ACME certificate *resolver*, which swaps in a fresh
//! certificate in place as one is issued or renewed. New QUIC handshakes then
//! present the new certificate with no endpoint reconfigure and no dropped
//! tunnels — the same swap model as [`TlsReloader`](crate::TlsReloader).
//!
//! Let's Encrypt validates TLS-ALPN-01 over **TCP** on port 443, while the
//! gateway's real traffic is QUIC over **UDP**. [`Acme::run`] therefore also
//! binds a TCP listener on the same address that answers *only* the
//! `acme-tls/1` challenge handshake; nothing else legitimately connects there.
//!
//! **Renewal is proactive.** `rustls_acme` re-orders once the current
//! certificate has a third of its lifetime left — roughly 30 days before a
//! 90-day Let's Encrypt certificate expires — for as long as [`Acme::run`] is
//! polled. A transient failure therefore has weeks of retries before the old
//! certificate lapses. Every ACME error is logged and metered
//! (`skimasque_acme_events_total{kind="error"}`) and retried with backoff; it
//! never stops the gateway.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use futures::StreamExt;
use rustls_acme::caches::DirCache;
use rustls_acme::{AcmeConfig, AcmeState, EventOk};
use tokio::net::TcpListener;
use tokio_util::compat::TokioAsyncReadCompatExt;

use crate::tls::{install_default_crypto_provider, ALPN_H3};
use crate::Error;

/// How long to wait before re-binding the challenge listener after a bind
/// failure (e.g. the port is briefly held during a restart).
const REBIND_BACKOFF: Duration = Duration::from_secs(30);

/// How the gateway should obtain and renew its certificate.
#[derive(Debug, Clone)]
pub struct AcmeParams {
    /// The DNS names the certificate must cover; the first is the primary.
    pub domains: Vec<String>,
    /// The ACME account contact, e.g. `mailto:ops@example.com`. Optional but
    /// recommended — Let's Encrypt uses it for expiry warnings.
    pub contact: Option<String>,
    /// Directory for the cached ACME account key and issued certificate.
    /// Persistence is required: without it every restart re-orders and soon
    /// trips Let's Encrypt's rate limits.
    pub cache_dir: PathBuf,
    /// Use the Let's Encrypt **production** directory. `false` uses staging —
    /// certificates browsers do not trust, but with generous rate limits, for
    /// testing.
    pub production: bool,
    /// Address for the TCP TLS-ALPN-01 challenge listener. Normally the same
    /// `ip:443` as the QUIC endpoint; Let's Encrypt only validates on port 443.
    pub challenge_addr: SocketAddr,
}

/// A running ACME certificate manager: build it with [`start`], hand
/// [`server_config`](Self::server_config) to
/// [`Server::bind`](crate::Server::bind), and spawn [`run`](Self::run).
pub struct Acme {
    state: AcmeState<std::io::Error>,
    challenge_addr: SocketAddr,
    domains: Vec<String>,
    production: bool,
}

impl std::fmt::Debug for Acme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Acme")
            .field("domains", &self.domains)
            .field("production", &self.production)
            .field("challenge_addr", &self.challenge_addr)
            .finish_non_exhaustive()
    }
}

/// Set up ACME certificate management. Does no network I/O — ordering begins
/// the first time [`Acme::run`] is polled.
pub fn start(params: AcmeParams) -> Result<Acme, Error> {
    install_default_crypto_provider();
    if params.domains.is_empty() {
        return Err(Error::Acme("at least one --acme domain is required".to_owned()));
    }
    if let Some(contact) = &params.contact {
        if !contact.contains(':') {
            return Err(Error::Acme(format!(
                "ACME contact {contact:?} must be a URI such as mailto:you@example.com"
            )));
        }
    }

    let mut config = AcmeConfig::new(params.domains.iter().map(String::as_str))
        .cache(DirCache::new(params.cache_dir))
        .directory_lets_encrypt(params.production);
    if let Some(contact) = &params.contact {
        config = config.contact_push(contact);
    }

    Ok(Acme {
        state: config.state(),
        challenge_addr: params.challenge_addr,
        domains: params.domains,
        production: params.production,
    })
}

impl Acme {
    /// The QUIC [`rustls::ServerConfig`] to serve. It presents whatever
    /// certificate the manager currently holds, refreshed in place on renewal;
    /// ALPN is pinned to `h3`.
    pub fn server_config(&self) -> rustls::ServerConfig {
        let mut config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(self.state.resolver());
        config.alpn_protocols = vec![ALPN_H3.to_vec()];
        config
    }

    /// The DNS names the certificate covers.
    pub fn domains(&self) -> &[String] {
        &self.domains
    }

    /// Whether this targets the Let's Encrypt production directory.
    pub fn is_production(&self) -> bool {
        self.production
    }

    /// Drive certificate ordering, renewal, and the TLS-ALPN-01 challenge
    /// listener. Never returns — spawn it. Fail-soft: every error is logged,
    /// metered, and retried.
    pub async fn run(self) {
        let Acme {
            mut state,
            challenge_addr,
            ..
        } = self;

        // `AcmeState::acceptor` is marked deprecated in favour of the
        // fully-managed `Incoming` stream, which we cannot use here: it logs
        // ACME events through the `log` facade (the gateway is on `tracing`)
        // and gives no hook for metrics. Driving `state` and the challenge
        // acceptor separately keeps both.
        #[allow(deprecated)]
        let acceptor = state.acceptor();

        let listener = loop {
            match TcpListener::bind(challenge_addr).await {
                Ok(listener) => break listener,
                Err(error) => {
                    crate::metrics::acme_event("error");
                    tracing::error!(
                        %error, addr = %challenge_addr,
                        "binding the ACME challenge listener; retrying"
                    );
                    tokio::time::sleep(REBIND_BACKOFF).await;
                }
            }
        };
        tracing::info!(addr = %challenge_addr, "ACME TLS-ALPN-01 challenge listener up");

        loop {
            tokio::select! {
                event = state.next() => match event {
                    Some(Ok(ok)) => on_event(ok),
                    Some(Err(error)) => {
                        crate::metrics::acme_event("error");
                        tracing::warn!(%error, "ACME event error (will retry)");
                    }
                    None => break, // `AcmeState` is an infinite stream.
                },
                accepted = listener.accept() => match accepted {
                    Ok((tcp, _peer)) => {
                        let acceptor = acceptor.clone();
                        tokio::spawn(async move {
                            // `Ok(None)` = a challenge handshake was completed.
                            // `Ok(Some(_))` / `Err(_)` = a normal ClientHello or
                            // a failed handshake; the gateway serves no real
                            // traffic over TCP, so it is just dropped here.
                            if let Ok(None) = acceptor.accept(tcp.compat()).await {
                                crate::metrics::acme_event("challenge");
                                tracing::debug!("served a TLS-ALPN-01 validation request");
                            }
                        });
                    }
                    Err(error) => tracing::debug!(%error, "ACME challenge accept error"),
                },
            }
        }
    }
}

fn on_event(ok: EventOk) {
    match ok {
        EventOk::DeployedNewCert => {
            crate::metrics::acme_event("deployed_new");
            tracing::info!("ACME certificate issued/renewed and now being served");
        }
        EventOk::DeployedCachedCert => {
            crate::metrics::acme_event("deployed_cached");
            tracing::info!("ACME certificate loaded from the cache");
        }
        EventOk::CertCacheStore | EventOk::AccountCacheStore => {
            crate::metrics::acme_event("cache_store");
            tracing::debug!(?ok, "ACME cache write");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(domains: &[&str]) -> AcmeParams {
        AcmeParams {
            domains: domains.iter().map(|s| s.to_string()).collect(),
            contact: None,
            cache_dir: std::env::temp_dir().join(format!("skm-acme-test-{}", std::process::id())),
            production: false,
            challenge_addr: "127.0.0.1:0".parse().unwrap(),
        }
    }

    #[test]
    fn at_least_one_domain_is_required() {
        assert!(matches!(start(params(&[])), Err(Error::Acme(_))));
    }

    #[test]
    fn a_bare_contact_without_a_scheme_is_rejected() {
        let mut p = params(&["gw.example.com"]);
        p.contact = Some("ops@example.com".to_owned());
        assert!(matches!(start(p), Err(Error::Acme(_))));
    }

    #[test]
    fn the_served_config_advertises_h3_and_no_client_auth() {
        let acme = start(params(&["gw.example.com"])).expect("valid params");
        let config = acme.server_config();
        assert_eq!(config.alpn_protocols, vec![ALPN_H3.to_vec()]);
        assert_eq!(acme.domains(), ["gw.example.com"]);
        assert!(!acme.is_production());
        // It also has to satisfy quinn's QUIC (TLS 1.3) requirements.
        crate::tls::quic_server_config(config).expect("usable as a QUIC server config");
    }
}
