//! Ending a session early: the revoked-session set, and the layer that applies
//! it.
//!
//! A credential is a signed token the gateway verifies offline, so nothing
//! about it can be taken back. What *can* be taken back is the session it
//! belongs to: a credential minted for a session carries that session's id as
//! its `sid`, the control plane tells the gateway which session ids have been
//! ended, and the gateway refuses new tunnels for them and closes the ones
//! already open.
//!
//! [`Revocations`] is the gateway's view of that list. The control-plane sync
//! loop replaces it wholesale ([`Revocations::replace`]); a [`RevocationLayer`]
//! reads it on every new tunnel and hands the tunnel a [`TunnelEnd`] that
//! resolves the moment its session is revoked, which the server races against
//! the relay.

use std::collections::HashSet;
use std::sync::Arc;
use std::task::{Context, Poll};

use http::StatusCode;
use tokio::sync::watch;
use tower::{Layer, Service};

use crate::service::{Accepted, Rejection, TunnelFuture, TunnelRequest};

type Set = Arc<HashSet<String>>;

/// The session ids this gateway must refuse, shared by every tunnel.
///
/// Cloning is cheap and clones share state. An empty set (the default) revokes
/// nothing, so a gateway not connected to a control plane behaves as before.
#[derive(Debug, Clone)]
pub struct Revocations {
    tx: Arc<watch::Sender<Set>>,
}

impl Default for Revocations {
    fn default() -> Self {
        Self::new()
    }
}

impl Revocations {
    pub fn new() -> Self {
        Self {
            tx: Arc::new(watch::channel(Set::default()).0),
        }
    }

    /// Make `sids` the complete set of revoked sessions. Tunnels whose session
    /// is in it end; one that has dropped out of the set is not revived.
    ///
    /// Taking the whole set, not a delta, means a missed update can never leave
    /// the gateway permanently out of step.
    pub fn replace(&self, sids: impl IntoIterator<Item = String>) {
        let next: HashSet<String> = sids.into_iter().collect();
        self.tx.send_if_modified(|current| {
            if **current == next {
                return false;
            }
            *current = Arc::new(next);
            true
        });
    }

    /// Whether `sid` has been revoked.
    pub fn is_revoked(&self, sid: &str) -> bool {
        self.tx.borrow().contains(sid)
    }

    /// How many sessions are currently revoked.
    pub fn len(&self) -> usize {
        self.tx.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A signal that resolves when `sid` is revoked.
    pub fn track(&self, sid: &str) -> TunnelEnd {
        TunnelEnd {
            sid: sid.into(),
            rx: self.tx.subscribe(),
        }
    }
}

/// Resolves when one session is revoked. Held by a tunnel for its lifetime.
#[derive(Debug, Clone)]
pub struct TunnelEnd {
    sid: Arc<str>,
    rx: watch::Receiver<Set>,
}

impl TunnelEnd {
    /// Wait until the session is revoked. Never resolves if it is not (and
    /// stays pending, rather than ending the tunnel, if the revocation source
    /// goes away).
    pub async fn ended(mut self) {
        loop {
            if self.rx.borrow_and_update().contains(&*self.sid) {
                return;
            }
            if self.rx.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }
}

/// Refuses a new tunnel whose credential's session has been revoked, and gives
/// every other tunnel with a session a [`TunnelEnd`].
///
/// It reads the [`WorkloadIdentity`](skimasque_policy::WorkloadIdentity) an
/// identity layer left in the request, so it sits below one. An identity with
/// no `sid` (a CI credential, a developer's, a credential from before sessions
/// existed) is passed through untouched.
#[derive(Debug, Clone)]
pub struct RevocationLayer {
    revocations: Revocations,
}

impl RevocationLayer {
    pub fn new(revocations: Revocations) -> Self {
        Self { revocations }
    }
}

impl<S> Layer<S> for RevocationLayer {
    type Service = Revoked<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Revoked {
            inner,
            revocations: self.revocations.clone(),
        }
    }
}

/// The service [`RevocationLayer`] produces.
#[derive(Debug, Clone)]
pub struct Revoked<S> {
    inner: S,
    revocations: Revocations,
}

impl<S> Service<TunnelRequest> for Revoked<S>
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection>,
    S::Future: Send + 'static,
{
    type Response = Accepted;
    type Error = Rejection;
    type Future = TunnelFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: TunnelRequest) -> Self::Future {
        let sid = request
            .extensions()
            .get::<skimasque_policy::WorkloadIdentity>()
            .and_then(|identity| identity.sid.clone());

        let Some(sid) = sid else {
            return Box::pin(self.inner.call(request));
        };
        if self.revocations.is_revoked(&sid) {
            tracing::info!(%sid, "refusing a tunnel for an ended session");
            return Box::pin(std::future::ready(Err(Rejection::new(
                StatusCode::FORBIDDEN,
                "this session has ended; start a new one",
            )
            .with_proxy_error("session_ended"))));
        }

        // Start watching before the inner service runs, so a revocation that
        // lands while the tunnel is being set up is not missed.
        let end = self.revocations.track(&sid);
        let future = self.inner.call(request);
        Box::pin(async move { Ok(future.await?.with_end(end)) })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use skimasque_policy::WorkloadIdentity;
    use tokio::net::UdpSocket;

    use super::*;
    use crate::service::Destination;
    use skimasque_core::target::Target;
    use skimasque_core::Protocol;

    struct AcceptAll;

    impl Service<TunnelRequest> for AcceptAll {
        type Response = Accepted;
        type Error = Rejection;
        type Future = TunnelFuture;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _request: TunnelRequest) -> Self::Future {
            Box::pin(async {
                let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                Ok(Accepted::udp(socket, "127.0.0.1:9".parse().unwrap()))
            })
        }
    }

    fn request(sid: Option<&str>) -> TunnelRequest {
        let parts = http::Request::builder().body(()).unwrap().into_parts().0;
        let mut request = TunnelRequest::new(
            Protocol::ConnectUdp,
            Destination::Udp(Target::parse("10.0.0.2:443").unwrap()),
            "203.0.113.1:9000".parse().unwrap(),
            parts,
        );
        request.extensions_mut().insert(WorkloadIdentity {
            sid: sid.map(str::to_owned),
            ..Default::default()
        });
        request
    }

    async fn pending_after(end: TunnelEnd, wait: Duration) -> bool {
        tokio::time::timeout(wait, end.ended()).await.is_err()
    }

    #[tokio::test]
    async fn a_revoked_session_is_refused_with_a_message_naming_why() {
        let revocations = Revocations::new();
        revocations.replace(["sess_dead".to_owned()]);
        let mut service = RevocationLayer::new(revocations).layer(AcceptAll);

        let rejection = service.call(request(Some("sess_dead"))).await.unwrap_err();
        assert_eq!(rejection.status(), StatusCode::FORBIDDEN);
        assert!(rejection.detail().contains("session has ended"));

        // A different session, and an identity with no session at all, are untouched.
        assert!(service.call(request(Some("sess_live"))).await.is_ok());
        assert!(service.call(request(None)).await.is_ok());
    }

    #[tokio::test]
    async fn an_open_tunnel_ends_when_its_session_is_revoked_and_no_other_does() {
        let revocations = Revocations::new();
        let mut service = RevocationLayer::new(revocations.clone()).layer(AcceptAll);

        let mine = service.call(request(Some("sess_a"))).await.unwrap();
        let theirs = service.call(request(Some("sess_b"))).await.unwrap();
        let mine = mine
            .end()
            .cloned()
            .expect("a session tunnel carries an end signal");
        let theirs = theirs.end().cloned().unwrap();
        assert!(service.call(request(None)).await.unwrap().end().is_none());

        assert!(pending_after(mine.clone(), Duration::from_millis(50)).await);

        revocations.replace(["sess_a".to_owned()]);
        tokio::time::timeout(Duration::from_secs(2), mine.ended())
            .await
            .expect("revoking the session ends its tunnel");
        assert!(pending_after(theirs, Duration::from_millis(50)).await);
    }

    #[tokio::test]
    async fn replacing_the_set_is_a_snapshot_not_a_delta() {
        let revocations = Revocations::new();
        assert!(revocations.is_empty());
        revocations.replace(["a".to_owned(), "b".to_owned()]);
        assert!(revocations.is_revoked("a") && revocations.is_revoked("b"));
        revocations.replace(["b".to_owned()]);
        assert!(
            !revocations.is_revoked("a"),
            "a dropped out of the snapshot"
        );
        assert_eq!(revocations.len(), 1);
    }
}
