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
//! A session's tunnels also end when its credential expires. A credential is
//! checked once, as a tunnel opens, so without this a tunnel opened a minute
//! before expiry would outlive the session by as long as it stayed busy.
//!
//! [`Revocations`] is the gateway's view of that list. The control-plane sync
//! loop replaces it wholesale ([`Revocations::replace`]); a [`RevocationLayer`]
//! reads it on every new tunnel and hands the tunnel a [`TunnelEnd`] that
//! resolves the moment its session is revoked, which the server races against
//! the relay.

use std::collections::HashSet;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::prelude::{Engine as _, BASE64_URL_SAFE_NO_PAD};
use http::StatusCode;
use tokio::sync::watch;
use tokio::time::Instant;
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

    /// A signal that resolves when `sid` is revoked, or at `deadline` if given.
    pub fn track(&self, sid: &str, deadline: Option<Instant>) -> TunnelEnd {
        TunnelEnd {
            sid: sid.into(),
            rx: self.tx.subscribe(),
            deadline,
        }
    }
}

/// Why a tunnel was ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// Its session was ended early.
    Revoked,
    /// Its credential ran out.
    Expired,
}

/// Resolves when one session is revoked or its credential expires. Held by a
/// tunnel for its lifetime.
#[derive(Debug, Clone)]
pub struct TunnelEnd {
    sid: Arc<str>,
    rx: watch::Receiver<Set>,
    deadline: Option<Instant>,
}

impl TunnelEnd {
    /// Wait until the tunnel must close, and say why. Never resolves while the
    /// session is live and unexpired (and stays pending, rather than ending the
    /// tunnel, if the revocation source goes away).
    pub async fn ended(mut self) -> EndReason {
        let revoked = async {
            loop {
                if self.rx.borrow_and_update().contains(&*self.sid) {
                    return;
                }
                if self.rx.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        };
        match self.deadline {
            Some(deadline) => tokio::select! {
                () = revoked => EndReason::Revoked,
                () = tokio::time::sleep_until(deadline) => EndReason::Expired,
            },
            None => {
                revoked.await;
                EndReason::Revoked
            }
        }
    }
}

/// When the bearer credential on `request` expires, if it says.
///
/// Read from the token's own `exp` claim **without checking the signature**.
/// That is sound only because this runs below an identity layer that has
/// already verified the very same token, and it can only shorten a tunnel's
/// life: a token with no readable `exp` simply gets no deadline.
fn credential_expiry(request: &TunnelRequest) -> Option<SystemTime> {
    #[derive(serde::Deserialize)]
    struct Exp {
        exp: u64,
    }
    let token = request
        .headers()
        .get(http::header::PROXY_AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    let payload = BASE64_URL_SAFE_NO_PAD
        .decode(token.split('.').nth(1)?)
        .ok()?;
    let exp = serde_json::from_slice::<Exp>(&payload).ok()?.exp;
    Some(UNIX_EPOCH + Duration::from_secs(exp))
}

/// Refuses a new tunnel whose credential's session has been revoked or whose
/// credential has expired, and gives every other tunnel with a session a
/// [`TunnelEnd`] that fires on revocation or at expiry.
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

        let deadline = match credential_expiry(&request) {
            None => None,
            Some(expiry) => match expiry.duration_since(SystemTime::now()) {
                Ok(left) if !left.is_zero() => Some(Instant::now() + left),
                _ => {
                    tracing::info!(%sid, "refusing a tunnel for an expired session");
                    return Box::pin(std::future::ready(Err(Rejection::new(
                        StatusCode::FORBIDDEN,
                        "this session has expired; start a new one",
                    )
                    .with_proxy_error("session_expired"))));
                }
            },
        };

        // Start watching before the inner service runs, so a revocation that
        // lands while the tunnel is being set up is not missed.
        let end = self.revocations.track(&sid, deadline);
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

    /// A bearer credential that expires `secs_from_now` seconds from now
    /// (negative: already). Unsigned: the layer only reads the claim.
    fn bearer_expiring(secs_from_now: i64) -> String {
        let exp = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + secs_from_now) as u64;
        let payload = BASE64_URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#));
        format!("Bearer aaa.{payload}.sig")
    }

    fn request_with_credential(sid: &str, authorization: Option<String>) -> TunnelRequest {
        let mut builder = http::Request::builder();
        if let Some(value) = authorization {
            builder = builder.header(http::header::PROXY_AUTHORIZATION, value);
        }
        let parts = builder.body(()).unwrap().into_parts().0;
        let mut request = TunnelRequest::new(
            Protocol::ConnectUdp,
            Destination::Udp(Target::parse("10.0.0.2:443").unwrap()),
            "203.0.113.1:9000".parse().unwrap(),
            parts,
        );
        request.extensions_mut().insert(WorkloadIdentity {
            sid: Some(sid.to_owned()),
            ..Default::default()
        });
        request
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

    #[tokio::test(start_paused = true)]
    async fn a_session_tunnel_ends_when_its_credential_expires() {
        let revocations = Revocations::new();
        let mut service = RevocationLayer::new(revocations.clone()).layer(AcceptAll);

        let accepted = service
            .call(request_with_credential("sess_a", Some(bearer_expiring(60))))
            .await
            .unwrap();
        let end = accepted.end().cloned().unwrap();

        // Live and unexpired: still pending a minute-less-a-second in.
        let early = tokio::time::timeout(Duration::from_secs(50), end.clone().ended()).await;
        assert!(early.is_err(), "ended before the credential expired");

        // Then it fires, saying why.
        let reason = tokio::time::timeout(Duration::from_secs(30), end.ended())
            .await
            .expect("the tunnel outlived its credential");
        assert_eq!(reason, EndReason::Expired);
    }

    #[tokio::test(start_paused = true)]
    async fn revocation_wins_over_a_later_expiry_and_says_so() {
        let revocations = Revocations::new();
        let mut service = RevocationLayer::new(revocations.clone()).layer(AcceptAll);
        let accepted = service
            .call(request_with_credential(
                "sess_a",
                Some(bearer_expiring(3600)),
            ))
            .await
            .unwrap();
        let end = accepted.end().cloned().unwrap();
        revocations.replace(["sess_a".to_owned()]);
        assert_eq!(end.ended().await, EndReason::Revoked);
    }

    #[tokio::test]
    async fn an_expired_credential_is_refused_at_setup_and_one_without_a_readable_exp_gets_no_deadline(
    ) {
        let mut service = RevocationLayer::new(Revocations::new()).layer(AcceptAll);

        let rejection = service
            .call(request_with_credential("sess_a", Some(bearer_expiring(-5))))
            .await
            .unwrap_err();
        assert_eq!(rejection.status(), StatusCode::FORBIDDEN);
        assert!(rejection.detail().contains("expired"));

        // No header, a non-JWT bearer, and a payload without `exp` all just get
        // the revocation signal and no deadline: the identity layer above
        // already decided whether the token is acceptable.
        for authorization in [
            None,
            Some("Bearer opaque".to_owned()),
            Some("Bearer a.e30.c".to_owned()),
        ] {
            let accepted = service
                .call(request_with_credential("sess_a", authorization))
                .await
                .unwrap();
            let end = accepted.end().cloned().unwrap();
            assert!(pending_after(end, Duration::from_millis(30)).await);
        }
    }
}
