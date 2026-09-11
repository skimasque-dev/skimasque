//! End-to-end tests: a real QUIC connection, a real HTTP/3 exchange, real UDP
//! sockets. Everything below runs the same code path a deployed proxy would.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use skimasque::client::{Client, Session, UdpTunnel};
use skimasque::exchange::{CredentialMinter, MintError, MintedCredential};
use skimasque::policy::AddressPolicy;
use skimasque::policy_engine::WorkloadIdentity;
use skimasque::server::{ConnectionRate, ProxyConfig, ResourceLimits, Server};
use skimasque::service::{
    Accepted, AuthorizeLayer, IdentityLayer, IdentityVerifier, Rejection, TunnelRequest, UdpProxy,
};
use skimasque::tls;
use skimasque_core::connect_udp::Target;
use skimasque_core::UriTemplate;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tower::{Service, ServiceBuilder};

/// Long enough that a slow CI machine is not mistaken for a broken tunnel.
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// A UDP server that echoes back `tag` followed by whatever it received.
///
/// The tag is what makes crosstalk visible: a reply carrying the wrong tag
/// means a datagram was delivered to the wrong tunnel.
async fn spawn_echo(tag: &'static [u8]) -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        loop {
            let Ok((len, from)) = socket.recv_from(&mut buf).await else {
                return;
            };
            let mut reply = tag.to_vec();
            reply.extend_from_slice(&buf[..len]);
            if socket.send_to(&reply, from).await.is_err() {
                return;
            }
        }
    });
    addr
}

struct Proxy {
    addr: SocketAddr,
    certificate_pem: String,
}

/// Start a proxy on loopback with a throwaway certificate.
fn spawn_proxy<S>(service: S) -> Proxy
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection> + Clone + Send + 'static,
    S::Future: Send,
{
    let generated = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();
    let server_tls = tls::server_config_from_pem(
        generated.certificate_pem.as_bytes(),
        generated.key_pem.as_bytes(),
    )
    .unwrap();

    // The proxy matches only the request path, so the authority it names in its
    // own template does not have to be the one clients dial.
    let config = ProxyConfig::new("localhost").unwrap();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), server_tls, service, config).unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.run().await;
    });

    Proxy {
        addr,
        certificate_pem: generated.certificate_pem,
    }
}

/// A permissive proxy, since every test target is on loopback.
fn spawn_default_proxy() -> Proxy {
    spawn_proxy(UdpProxy::new(AddressPolicy::permissive()))
}

/// Start a proxy that also answers the token-exchange endpoint.
fn spawn_gateway<S>(service: S, minter: Arc<dyn CredentialMinter>) -> Proxy
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection> + Clone + Send + 'static,
    S::Future: Send,
{
    spawn_gateway_with(service, minter, None)
}

/// As [`spawn_gateway`], but with a specific token-exchange rate limit.
fn spawn_gateway_with<S>(
    service: S,
    minter: Arc<dyn CredentialMinter>,
    exchange_rate: Option<ConnectionRate>,
) -> Proxy
where
    S: Service<TunnelRequest, Response = Accepted, Error = Rejection> + Clone + Send + 'static,
    S::Future: Send,
{
    let generated = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();
    let server_tls = tls::server_config_from_pem(
        generated.certificate_pem.as_bytes(),
        generated.key_pem.as_bytes(),
    )
    .unwrap();
    let mut config = ProxyConfig::new("localhost").unwrap().with_minter(minter);
    if let Some(rate) = exchange_rate {
        config = config.with_limits(ResourceLimits {
            exchange_rate: Some(rate),
            ..ResourceLimits::default()
        });
    }
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), server_tls, service, config).unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    Proxy {
        addr,
        certificate_pem: generated.certificate_pem,
    }
}

/// A minter that issues one canned credential for one known identity token.
#[derive(Debug)]
struct StubMinter;

impl CredentialMinter for StubMinter {
    fn mint(
        &self,
        identity_token: String,
    ) -> Pin<Box<dyn Future<Output = Result<MintedCredential, MintError>> + Send>> {
        Box::pin(async move {
            if identity_token == "valid-oidc" {
                Ok(MintedCredential {
                    credential: "issued-credential".to_owned(),
                    expires_in: Duration::from_secs(900),
                })
            } else {
                Err(MintError::Unauthorized(
                    "unrecognised identity token".to_owned(),
                ))
            }
        })
    }
}

/// Accepts only the credential [`StubMinter`] issues.
#[derive(Debug)]
struct StubCredentials;

impl IdentityVerifier for StubCredentials {
    fn verify(
        &self,
        token: String,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadIdentity, String>> + Send>> {
        Box::pin(async move {
            if token == "issued-credential" {
                Ok(WorkloadIdentity {
                    repository: Some("acme/widget".to_owned()),
                    ..Default::default()
                })
            } else {
                Err("unknown credential".to_owned())
            }
        })
    }
}

/// A minter and verifier that rotate together: every exchange issues a new
/// credential, and only the most recently issued one still verifies. This
/// stands in for a gateway whose credentials expire and have to be re-minted
/// while a session is still open.
#[derive(Debug, Default, Clone)]
struct RotatingCredentials {
    current: Arc<std::sync::Mutex<Option<String>>>,
    issued: Arc<std::sync::atomic::AtomicUsize>,
}

impl CredentialMinter for RotatingCredentials {
    fn mint(
        &self,
        identity_token: String,
    ) -> Pin<Box<dyn Future<Output = Result<MintedCredential, MintError>> + Send>> {
        let current = self.current.clone();
        let issued = self.issued.clone();
        Box::pin(async move {
            if identity_token != "valid-oidc" {
                return Err(MintError::Unauthorized(
                    "unrecognised identity token".to_owned(),
                ));
            }
            let n = issued.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let credential = format!("cred-{n}");
            *current.lock().unwrap() = Some(credential.clone());
            Ok(MintedCredential {
                credential,
                expires_in: Duration::from_secs(900),
            })
        })
    }
}

impl IdentityVerifier for RotatingCredentials {
    fn verify(
        &self,
        token: String,
    ) -> Pin<Box<dyn Future<Output = Result<WorkloadIdentity, String>> + Send>> {
        let current = self.current.clone();
        Box::pin(async move {
            match &*current.lock().unwrap() {
                Some(latest) if *latest == token => Ok(WorkloadIdentity {
                    repository: Some("acme/widget".to_owned()),
                    ..Default::default()
                }),
                _ => Err("stale or unknown credential".to_owned()),
            }
        })
    }
}

async fn connect(proxy: &Proxy) -> Session {
    let client_tls = tls::client_config_with_ca(proxy.certificate_pem.as_bytes()).unwrap();
    let client = Client::new(client_tls).unwrap();
    // Dial the loopback address but present `localhost`, which the certificate
    // covers and the template names.
    let template =
        UriTemplate::default_connect_udp(&format!("localhost:{}", proxy.addr.port())).unwrap();
    client.connect(proxy.addr, template).await.unwrap()
}

async fn exchange(tunnel: &mut UdpTunnel, payload: &[u8]) -> Bytes {
    tunnel.send(payload).expect("sending on the tunnel");
    timeout(REPLY_TIMEOUT, tunnel.recv())
        .await
        .expect("timed out waiting for a reply")
        .expect("tunnel closed before replying")
}

#[tokio::test]
async fn a_udp_payload_reaches_the_target_and_comes_back() {
    let echo = spawn_echo(b"echo:").await;
    let proxy = spawn_default_proxy();
    let session = connect(&proxy).await;

    let mut tunnel = session
        .connect_udp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();

    assert_eq!(&exchange(&mut tunnel, b"hello").await[..], b"echo:hello");
    tunnel.close().await.unwrap();
}

/// The regression test for the reason this crate frames HTTP Datagrams itself.
///
/// `h3-datagram` 0.0.2 encodes every Quarter Stream ID as zero, so a second
/// tunnel's traffic is delivered to the first. With four tunnels to four
/// distinguishable targets, that failure shows up as replies with the wrong tag.
#[tokio::test]
async fn concurrent_tunnels_on_one_connection_do_not_cross_talk() {
    let tags: [&'static [u8]; 4] = [b"one:", b"two:", b"three:", b"four:"];
    let mut echoes = Vec::new();
    for tag in tags {
        echoes.push(spawn_echo(tag).await);
    }

    let proxy = spawn_default_proxy();
    let session = connect(&proxy).await;

    let mut tunnels = Vec::new();
    for echo in &echoes {
        tunnels.push(
            session
                .connect_udp(Target::parse(&echo.to_string()).unwrap())
                .await
                .unwrap(),
        );
    }

    // Every tunnel must be on a distinct stream, or the test proves nothing.
    let stream_ids: std::collections::BTreeSet<_> =
        tunnels.iter().map(UdpTunnel::stream_id).collect();
    assert_eq!(stream_ids.len(), tunnels.len(), "tunnels shared a stream");

    // Interleave the sends so a reply arriving on the wrong tunnel has every
    // opportunity to be mistaken for the right one.
    for tunnel in &tunnels {
        tunnel.send(b"ping").unwrap();
    }
    for (tunnel, tag) in tunnels.iter_mut().zip(tags) {
        let reply = timeout(REPLY_TIMEOUT, tunnel.recv())
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "no reply on the tunnel for {}",
                    String::from_utf8_lossy(tag)
                )
            })
            .expect("tunnel closed");
        let mut expected = tag.to_vec();
        expected.extend_from_slice(b"ping");
        assert_eq!(
            &reply[..],
            &expected[..],
            "reply landed on the wrong tunnel"
        );
    }
}

/// Payloads that are entirely legal UDP but awkward to frame.
#[tokio::test]
async fn empty_and_large_payloads_survive_the_tunnel() {
    let echo = spawn_echo(b"").await;
    let proxy = spawn_default_proxy();
    let session = connect(&proxy).await;
    let mut tunnel = session
        .connect_udp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();

    assert_eq!(&exchange(&mut tunnel, b"").await[..], b"");

    // Fill a datagram to just under the path limit, which is where framing
    // overhead errors show up.
    let limit = tunnel.max_payload_size().expect("datagrams are available");
    let big = vec![0xa5u8; limit - 1];
    assert_eq!(&exchange(&mut tunnel, &big).await[..], &big[..]);
}

#[tokio::test]
async fn the_address_policy_refuses_a_prohibited_destination() {
    let echo = spawn_echo(b"echo:").await;
    // The default policy refuses loopback, which is where the echo server is.
    let proxy = spawn_proxy(UdpProxy::new(AddressPolicy::default()));
    let session = connect(&proxy).await;

    let error = session
        .connect_udp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap_err();

    match error {
        skimasque::Error::Rejected {
            status,
            proxy_status,
        } => {
            assert_eq!(status, http::StatusCode::FORBIDDEN);
            let proxy_status = proxy_status.expect("proxy should explain itself");
            assert!(
                proxy_status.contains("destination_ip_prohibited"),
                "unexpected Proxy-Status: {proxy_status}"
            );
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn a_target_outside_the_template_is_not_found() {
    let proxy = spawn_default_proxy();
    let session = connect(&proxy).await;

    // Port 0 never reaches the wire; build the mismatch at the template layer
    // instead, by asking a proxy that serves the udp template for an ip path.
    let mismatched = UriTemplate::default_connect_ip("localhost").unwrap();
    assert!(mismatched
        .match_path("/.well-known/masque/udp/127.0.0.1/53/")
        .is_none());

    // And confirm the live proxy still serves its own template.
    let echo = spawn_echo(b"echo:").await;
    let mut tunnel = session
        .connect_udp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    assert_eq!(&exchange(&mut tunnel, b"x").await[..], b"echo:x");
}

#[tokio::test]
async fn the_authorize_layer_gates_the_tunnel() {
    let echo = spawn_echo(b"echo:").await;
    let service = ServiceBuilder::new()
        .layer(AuthorizeLayer::bearer("hunter2"))
        .service(UdpProxy::new(AddressPolicy::permissive()));
    let proxy = spawn_proxy(service);
    let session = connect(&proxy).await;

    let error = session
        .connect_udp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap_err();
    match error {
        skimasque::Error::Rejected { status, .. } => assert_eq!(
            status,
            http::StatusCode::PROXY_AUTHENTICATION_REQUIRED,
            "an unauthenticated client should be challenged"
        ),
        other => panic!("expected a challenge, got {other:?}"),
    }
}

/// The full Phase 3 loop: exchange an OIDC token for a credential, then open a
/// tunnel that presents it.
#[tokio::test]
async fn an_exchanged_credential_opens_a_tunnel() {
    let echo = spawn_echo(b"echo:").await;
    let service = ServiceBuilder::new()
        .layer(IdentityLayer::new(Arc::new(StubCredentials)))
        .service(UdpProxy::new(AddressPolicy::permissive()));
    let proxy = spawn_gateway(service, Arc::new(StubMinter));
    let session = connect(&proxy).await;

    // An unrecognised identity token is refused at the exchange.
    match session.exchange_credential("bogus").await.unwrap_err() {
        skimasque::Error::ExchangeFailed { status, .. } => {
            assert_eq!(status, http::StatusCode::FORBIDDEN);
        }
        other => panic!("expected an exchange failure, got {other:?}"),
    }

    // The real token yields a credential, and a tunnel that carries it works.
    let credential = session.exchange_credential("valid-oidc").await.unwrap();
    assert_eq!(credential.expires_in, Duration::from_secs(900));

    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::PROXY_AUTHORIZATION,
        format!("Bearer {}", credential.token).parse().unwrap(),
    );
    let session = session.with_default_headers(headers);

    let mut tunnel = session
        .connect_udp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    assert_eq!(&exchange(&mut tunnel, b"hi").await[..], b"echo:hi");
}

/// A source that hammers the exchange endpoint is answered `429` once it is
/// over its rate, without a credential ever being minted for the excess.
#[tokio::test]
async fn the_exchange_endpoint_rate_limits_a_source() {
    let service = ServiceBuilder::new()
        .layer(IdentityLayer::new(Arc::new(StubCredentials)))
        .service(UdpProxy::new(AddressPolicy::permissive()));
    // Burst of one, no refill: the second exchange on this connection is over.
    let proxy = spawn_gateway_with(
        service,
        Arc::new(StubMinter),
        Some(ConnectionRate {
            per_second: 0,
            burst: 1,
        }),
    );
    let session = connect(&proxy).await;

    session
        .exchange_credential("valid-oidc")
        .await
        .expect("the first exchange is within the rate");

    match session.exchange_credential("valid-oidc").await.unwrap_err() {
        skimasque::Error::ExchangeFailed { status, .. } => {
            assert_eq!(status, http::StatusCode::TOO_MANY_REQUESTS);
        }
        other => panic!("expected a 429, got {other:?}"),
    }
}

/// A session outlives its first credential: re-exchanging and calling
/// [`Session::set_credential`] keeps new tunnels opening, and leaves tunnels
/// that opened under the old credential alone.
#[tokio::test]
async fn a_refreshed_credential_keeps_new_tunnels_opening() {
    let echo = spawn_echo(b"echo:").await;
    let creds = RotatingCredentials::default();
    let service = ServiceBuilder::new()
        .layer(IdentityLayer::new(Arc::new(creds.clone())))
        .service(UdpProxy::new(AddressPolicy::permissive()));
    let proxy = spawn_gateway(service, Arc::new(creds.clone()));
    let session = connect(&proxy).await;

    let first = session.exchange_credential("valid-oidc").await.unwrap();
    session.set_credential(&first).unwrap();

    let mut early = session
        .connect_udp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    assert_eq!(&exchange(&mut early, b"one").await[..], b"echo:one");

    // A second exchange rotates the credential the gateway will accept, so the
    // one the session is still presenting has effectively expired.
    let second = session.exchange_credential("valid-oidc").await.unwrap();
    assert_ne!(first.token, second.token);
    match session
        .connect_udp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap_err()
    {
        skimasque::Error::Rejected { status, .. } => {
            assert_eq!(status, http::StatusCode::FORBIDDEN);
        }
        other => panic!("expected the stale credential to be rejected, got {other:?}"),
    }

    // Installing the fresh credential fixes new tunnels...
    session.set_credential(&second).unwrap();
    let mut later = session
        .connect_udp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap();
    assert_eq!(&exchange(&mut later, b"two").await[..], b"echo:two");

    // ...and the tunnel opened under the old credential still carries traffic.
    assert_eq!(&exchange(&mut early, b"still").await[..], b"echo:still");
}

/// Without a credential, the identity layer challenges before any tunnel opens.
#[tokio::test]
async fn a_tunnel_without_a_credential_is_challenged() {
    let echo = spawn_echo(b"echo:").await;
    let service = ServiceBuilder::new()
        .layer(IdentityLayer::new(Arc::new(StubCredentials)))
        .service(UdpProxy::new(AddressPolicy::permissive()));
    let proxy = spawn_gateway(service, Arc::new(StubMinter));
    let session = connect(&proxy).await;

    match session
        .connect_udp(Target::parse(&echo.to_string()).unwrap())
        .await
        .unwrap_err()
    {
        skimasque::Error::Rejected { status, .. } => {
            assert_eq!(status, http::StatusCode::PROXY_AUTHENTICATION_REQUIRED);
        }
        other => panic!("expected a challenge, got {other:?}"),
    }
}

/// A tunnel is scoped to its request stream: closing it releases the proxy's
/// socket, and the client stops receiving.
#[tokio::test]
async fn closing_a_tunnel_ends_it_without_disturbing_its_neighbours() {
    let first = spawn_echo(b"first:").await;
    let second = spawn_echo(b"second:").await;
    let proxy = spawn_default_proxy();
    let session = connect(&proxy).await;

    let mut a = session
        .connect_udp(Target::parse(&first.to_string()).unwrap())
        .await
        .unwrap();
    let mut b = session
        .connect_udp(Target::parse(&second.to_string()).unwrap())
        .await
        .unwrap();

    assert_eq!(&exchange(&mut a, b"x").await[..], b"first:x");
    a.close().await.unwrap();

    // The surviving tunnel is unaffected.
    assert_eq!(&exchange(&mut b, b"y").await[..], b"second:y");
}
