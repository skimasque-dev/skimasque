//! Getting an issuer's signing keys, and holding onto them.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::JwkSet;
use tokio::sync::RwLock;

use crate::Error;

/// A source of an issuer's JWK Set -- its published signing keys.
///
/// The default implementation ([`HttpJwksProvider`], behind the `remote`
/// feature) does OIDC discovery and an HTTPS fetch. A caller with its own
/// transport, or a test, can implement this instead.
pub trait JwksProvider: Send + Sync + fmt::Debug {
    fn fetch(&self) -> Pin<Box<dyn Future<Output = Result<JwkSet, Error>> + Send + '_>>;
}

/// A time-bounded cache over a [`JwksProvider`].
///
/// Keys are refetched when they age past the TTL, and [`refresh`](Self::refresh)
/// forces it -- the verifier calls that once when a token names a `kid` the
/// cached set does not contain, since the issuer may have rotated.
#[derive(Debug)]
pub struct JwksCache {
    provider: Arc<dyn JwksProvider>,
    ttl: Duration,
    cached: RwLock<Option<Entry>>,
}

#[derive(Debug)]
struct Entry {
    keys: JwkSet,
    fetched: Instant,
}

impl JwksCache {
    /// Cache the output of `provider`, treating it as stale after `ttl`.
    pub fn new(provider: Arc<dyn JwksProvider>, ttl: Duration) -> Self {
        Self {
            provider,
            ttl,
            cached: RwLock::new(None),
        }
    }

    /// The cached keys if they are still fresh, otherwise a fresh fetch.
    pub async fn keys(&self) -> Result<JwkSet, Error> {
        if let Some(entry) = self.cached.read().await.as_ref() {
            if entry.fetched.elapsed() < self.ttl {
                return Ok(entry.keys.clone());
            }
        }
        self.refresh().await
    }

    /// Fetch and store, regardless of what the cache holds.
    pub async fn refresh(&self) -> Result<JwkSet, Error> {
        let keys = self.provider.fetch().await?;
        *self.cached.write().await = Some(Entry {
            keys: keys.clone(),
            fetched: Instant::now(),
        });
        Ok(keys)
    }
}

#[cfg(feature = "remote")]
pub use remote::HttpJwksProvider;

#[cfg(feature = "remote")]
mod remote {
    use super::{Error, Future, JwkSet, JwksProvider, Pin};
    use std::time::Duration;

    use serde::Deserialize;

    /// Fetches keys over HTTPS: OIDC discovery on the issuer, then the
    /// `jwks_uri` it advertises.
    #[derive(Debug)]
    pub struct HttpJwksProvider {
        client: reqwest::Client,
        issuer: String,
    }

    #[derive(Deserialize)]
    struct Discovery {
        jwks_uri: String,
    }

    impl HttpJwksProvider {
        /// Discover and fetch from `issuer` (no trailing slash needed).
        pub fn new(issuer: &str) -> Result<Self, Error> {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(concat!("skimasque-identity/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|e| Error::Jwks(e.to_string()))?;
            Ok(Self {
                client,
                issuer: issuer.trim_end_matches('/').to_owned(),
            })
        }

        async fn discover(&self) -> Result<String, Error> {
            let url = format!("{}/.well-known/openid-configuration", self.issuer);
            let discovery: Discovery = self
                .client
                .get(&url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|e| Error::Discovery(e.to_string()))?
                .json()
                .await
                .map_err(|e| Error::Discovery(e.to_string()))?;
            Ok(discovery.jwks_uri)
        }
    }

    impl JwksProvider for HttpJwksProvider {
        fn fetch(&self) -> Pin<Box<dyn Future<Output = Result<JwkSet, Error>> + Send + '_>> {
            Box::pin(async move {
                let jwks_uri = self.discover().await?;
                let keys: JwkSet = self
                    .client
                    .get(&jwks_uri)
                    .send()
                    .await
                    .and_then(reqwest::Response::error_for_status)
                    .map_err(|e| Error::Jwks(e.to_string()))?
                    .json()
                    .await
                    .map_err(|e| Error::Jwks(e.to_string()))?;
                Ok(keys)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct Counting {
        calls: AtomicUsize,
        json: String,
    }

    impl JwksProvider for Counting {
        fn fetch(&self) -> Pin<Box<dyn Future<Output = Result<JwkSet, Error>> + Send + '_>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let json = self.json.clone();
            Box::pin(
                async move { serde_json::from_str(&json).map_err(|e| Error::Jwks(e.to_string())) },
            )
        }
    }

    fn provider() -> Arc<Counting> {
        Arc::new(Counting {
            calls: AtomicUsize::new(0),
            json: crate::test_support::TestIssuer::new()
                .jwks_json()
                .to_owned(),
        })
    }

    #[tokio::test]
    async fn a_fresh_cache_is_not_refetched() {
        let counting = provider();
        let cache = JwksCache::new(counting.clone(), Duration::from_secs(3600));

        cache.keys().await.unwrap();
        cache.keys().await.unwrap();

        assert_eq!(counting.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn refresh_always_refetches() {
        let counting = provider();
        let cache = JwksCache::new(counting.clone(), Duration::from_secs(3600));

        cache.keys().await.unwrap();
        cache.refresh().await.unwrap();

        assert_eq!(counting.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_zero_ttl_refetches_every_time() {
        let counting = provider();
        let cache = JwksCache::new(counting.clone(), Duration::ZERO);

        cache.keys().await.unwrap();
        cache.keys().await.unwrap();

        assert_eq!(counting.calls.load(Ordering::SeqCst), 2);
    }
}
