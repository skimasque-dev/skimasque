//! The platform (multi-tenant) gateway end to end, in process: a stub control
//! plane holding two organisations' real Ed25519 signing keys, the tenant table
//! bootstrapped from it, and the exact service stack `skimasque-server
//! --platform` serves, behind a real QUIC listener.
//!
//! A CI job exchanges its OIDC token through [`PlatformMinter`] (the control
//! plane signs the credential with that org's key), then opens tunnels with the
//! credential. Each org reaches its own destination only, and every audit event
//! is filed under the org the credential names.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::prelude::{Engine as _, BASE64_STANDARD};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use ring::signature::KeyPair as _;
use skimasque::audit::{AuditEvent, AuditSink};
use skimasque::client::{Client, Credential, Session};
use skimasque::policy::AddressPolicy;
use skimasque::server::{ProxyConfig, Server};
use skimasque::service::{Dispatch, TcpProxy, UdpProxy};
use skimasque::{tls, TenantUsage};
use skimasque_cli::control::ControlPlane;
use skimasque_cli::platform::{
    initial_tenants, owner_settings_url, platform_service, InitialTenants, PlatformMinter,
    PlatformTenants,
};
use skimasque_core::connect_udp::Target;
use skimasque_core::UriTemplate;
use skimasque_identity::{
    CredentialSigner, Error as IdentityError, JwksProvider, OidcVerifier, Provider, Verifier,
    GITHUB_ACTIONS_ISSUER,
};
use skimasque_protocol::platform::{PlatformMintRequest, Tenant, TenantList, TenantPolicy};
use skimasque_protocol::{GatewayIdentity, PolicyDocument, SigningKey};
use tokio::net::UdpSocket;
use tokio::time::timeout;

/// The gateway's public origin: tenant audiences are `<BASE>/o/<slug>`.
const BASE: &str = "https://gw.example";
const UNAVAILABLE: &str = "SkiMasque control plane unreachable; try again shortly.";
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// Two organisations, each with a real Ed25519 signing key and a policy that
// allows exactly one destination.
// ---------------------------------------------------------------------------

struct Org {
    org_id: &'static str,
    slug: &'static str,
    owner: &'static str,
    signer: CredentialSigner,
    public_b64: String,
}

fn org(org_id: &'static str, slug: &'static str, owner: &'static str) -> Org {
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    Org {
        org_id,
        slug,
        owner,
        signer: CredentialSigner::from_pkcs8_der(pkcs8.as_ref(), Duration::from_secs(600)),
        public_b64: BASE64_STANDARD.encode(pair.public_key().as_ref()),
    }
}

/// `org`'s tenant entry: its owner verified, its key, and a policy allowing
/// `curl` to `allowed` only.
fn tenant(org: &Org, allowed: SocketAddr) -> Tenant {
    Tenant {
        org_id: org.org_id.into(),
        slug: org.slug.into(),
        owners: vec![org.owner.into()],
        owner_ids: BTreeMap::new(),
        policy: Some(TenantPolicy {
            version: 1,
            documents: vec![PolicyDocument {
                // Both orgs name their policy the same: nothing may be shared
                // because of it.
                name: "prod.toml".into(),
                text: format!(
                    "name = \"prod\"\n[[rules]]\napplication = \"curl\"\naction = \"allow\"\n\
                     destinations = [\"{allowed}\"]\n"
                ),
            }],
        }),
        signing_key: SigningKey {
            org_id: org.org_id.into(),
            algorithm: "ed25519".into(),
            public_key_b64: org.public_b64.clone(),
            previous_public_key_b64: None,
        },
    }
}

// ---------------------------------------------------------------------------
// A stub control plane: serves the tenant list and mints credentials with the
// requested org's real key, as the real control plane does.
// ---------------------------------------------------------------------------

struct StubControlPlane {
    base_url: String,
    mints: Arc<Mutex<Vec<PlatformMintRequest>>>,
    tasks: Arc<Mutex<Vec<tokio::task::AbortHandle>>>,
}

impl StubControlPlane {
    /// Stop answering: the listener and every open connection go away, so
    /// the next request is refused.
    fn stop(&self) {
        for task in self.tasks.lock().unwrap().drain(..) {
            task.abort();
        }
    }

    fn mints(&self) -> Vec<PlatformMintRequest> {
        self.mints.lock().unwrap().clone()
    }
}

async fn stub_control_plane(list: TenantList, orgs: Vec<Arc<Org>>) -> StubControlPlane {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mints: Arc<Mutex<Vec<PlatformMintRequest>>> = Arc::default();
    let tasks: Arc<Mutex<Vec<tokio::task::AbortHandle>>> = Arc::default();
    let list = Arc::new(list);
    let orgs = Arc::new(orgs);

    let (log, conns) = (mints.clone(), tasks.clone());
    let accept = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (log, list, orgs) = (log.clone(), list.clone(), orgs.clone());
            let conn = tokio::spawn(async move {
                let service = service_fn(move |req: Request<Incoming>| {
                    let (log, list, orgs) = (log.clone(), list.clone(), orgs.clone());
                    async move {
                        let (parts, body) = req.into_parts();
                        let body = body.collect().await.unwrap().to_bytes();
                        let path = parts.uri.path();
                        let (status, reply) = if parts.method == Method::GET
                            && path == "/v1/platform/gateways/gw_p1/tenants"
                        {
                            (200, serde_json::to_string(&*list).unwrap())
                        } else if parts.method == Method::POST
                            && path == "/v1/platform/gateways/gw_p1/credentials"
                        {
                            let req: PlatformMintRequest = serde_json::from_slice(&body).unwrap();
                            let org = orgs.iter().find(|o| o.org_id == req.org_id).unwrap();
                            let issued = org
                                .signer
                                .issue_for_org(&req.identity, req.subject.as_deref(), &req.org_id)
                                .unwrap();
                            log.lock().unwrap().push(req);
                            let reply = serde_json::json!({
                                "credential": issued.token,
                                "expires_in": issued.expires_in.as_secs(),
                            });
                            (200, reply.to_string())
                        } else {
                            (404, String::new())
                        };
                        let mut response = Response::new(Full::new(Bytes::from(reply)));
                        *response.status_mut() = StatusCode::from_u16(status).unwrap();
                        Ok::<_, Infallible>(response)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
            conns.lock().unwrap().push(conn.abort_handle());
        }
    });
    tasks.lock().unwrap().push(accept.abort_handle());

    StubControlPlane {
        base_url: format!("http://{addr}"),
        mints,
        tasks,
    }
}

fn gateway_identity() -> GatewayIdentity {
    GatewayIdentity {
        gateway_id: "gw_p1".into(),
        org_id: String::new(),
        secret: "s3cret".into(),
    }
}

fn state_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("skm-pgw-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

// ---------------------------------------------------------------------------
// OIDC: a fixed RS256 test key standing in for GitHub's.
// ---------------------------------------------------------------------------

/// A throwaway 2048-bit RSA key used only to sign test OIDC tokens.
const TEST_RSA_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCpwseOwMfcl9Mf
XcL37ImHCf4AISAQoTUzxw8zQ2bhz4bbOaaKYygeNVqz93+BBnWVXLF57a2pkWJX
br3bzE/eiWHzdEjms0KqNWuoPaM9sVB2Mm2cKJj/Wsx4e1HKRnAzi72kLw6mqh9M
WhdFMr2yEGqW2M/UKj9FvU2vTzk//UM6/Jb+gQSmecNa7a9dvUovVKzOjZ1mVGlg
DhGW8UA8z3wV/XrSCS5QHJ2PuyCyEGt4UyBZ6liYEvVJWeGjc9ddKjwGFGV7rZtv
d5Rj9I6ks4j1smHyieNkN+VmeLP7W3aUAgb2+riA//bUNpeXEWd/6g3McLCGdtA0
yrdEabrFAgMBAAECggEACBmSGf6ayqy31xxLLDcuDLJuWyU5eXGnrzPFRuXlZ8rW
HWmvx5SZmm9jR4j8nXRocdr9YNr1WUzli1RuXKlv+idF9C7NN6y/9o0b+OgP/OaT
9z/KuRO60RxM+0avDV61BdCaGNZh9ZPScHsv9w5HvNJJs67eo7nsUPCKX14BVks0
TcB7LtOrDKtXztylEX3P95AKxvDYlcRH2NCCWqkIQ6lPfNdBDsQmmW7Ou7vd05oM
Uxl4A/SBSSkMQ8qxeiaaQah6gjPbRGfgwpy/O2H7w5i8J/tZPI9OP8qkLFVN6jLt
wzEG1vgH1+rHyEK2iIkJ9fDlv1lDRUns26HGQQ3KVQKBgQDi/CxQNBJOj29hkCE4
nhvJyjQ6xvE0j/Ccg1SAiruRxyuR6/3XaD6uX7XglujJE9iH4n93zobIGv0bqmsr
UkrurEPMbwv6v43FUsp6m1GIoxFStlbCW8y+qQPOHkBjtxsw72g8JCXrI+QMSyjZ
Iam/pIZ0tN7PuukTUPi7h4ap7wKBgQC/dgKMb1N1ofAQPDrosnW/KCJ23H9ipbCK
tJShaEdz8wOwBpIwjSQIckq50REWvZV4PqearJWlzTTX1TbQxeV/oi+Hr2ZH1nlN
2ByJB/wVWZm3yQSUi1CaSQHTvlDaf/ICgG66NG2VH/wYIdwmKh8piXOBfnoBAX0G
XIXwC3nqiwKBgQCvRMM+5wZfzRfXQQC2BDg218D+xdFIogDMCgi8/OMbDK0TDyPC
KgeEg/kfw8daRM3FF1sP+tROPbDFpRD9sZyUsUXk3LZmV3U0MdqRU89gb3IX6R4T
E+mEK2P5y5gypxgC8EoPbmYtLFiSOZMAHqNBjNwZz/PgeVYyCSsXOu371wKBgDiV
vxEUQd1NO+8AbgSh4azaRr1MU5WrFG8aCadec2ewVdGrT39r509bv/wE7wECjO9Z
zR0ojp3O9SQozqeLJVXAcD2wuBDZMUaxbVWOd5EzxvuLPIBOYEcI9rJG2AyLrdHR
dWgw4IYnStEzCKZ64nTbO7j00UgE0ZeUtr0IF0MbAoGARtenkyMV1cXOiaMFgVgs
2fnh87f1ycftqWqJFOvuxyk3moDYLD4v2jb+06l5d0RwaFe5uKEbh2hAK/icDzdG
m1P2Ls9f8e2W53cFGtvA8Hsa5eCK6VVIaOYl1RMShZPPC3KeerQjTmN/dkXu5N+u
Lr2xvQ9kHg8wAlJPGbyH+0o=
-----END PRIVATE KEY-----
";

/// The modulus of [`TEST_RSA_PEM`], base64url without padding.
const TEST_RSA_N: &str = "qcLHjsDH3JfTH13C9-yJhwn-ACEgEKE1M8cPM0Nm4c-G2zmmimMoHjVas_d_gQZ1lVyxee2tqZFiV26928xP3olh83RI5rNCqjVrqD2jPbFQdjJtnCiY_1rMeHtRykZwM4u9pC8OpqofTFoXRTK9shBqltjP1Co_Rb1Nr085P_1DOvyW_oEEpnnDWu2vXb1KL1Sszo2dZlRpYA4RlvFAPM98Ff160gkuUBydj7sgshBreFMgWepYmBL1SVnho3PXXSo8BhRle62bb3eUY_SOpLOI9bJh8onjZDflZniz-1t2lAIG9vq4gP_21DaXlxFnf-oNzHCwhnbQNMq3RGm6xQ";
const TEST_KID: &str = "skimasque-cli-test";

#[derive(Debug)]
struct StaticJwks;

impl JwksProvider for StaticJwks {
    fn fetch(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<jsonwebtoken::jwk::JwkSet, IdentityError>> + Send + '_>>
    {
        Box::pin(async {
            serde_json::from_value(serde_json::json!({
                "keys": [{
                    "kty": "RSA", "use": "sig", "alg": "RS256", "kid": TEST_KID,
                    "n": TEST_RSA_N, "e": "AQAB",
                }]
            }))
            .map_err(|e| IdentityError::Jwks(e.to_string()))
        })
    }
}

/// A GitHub Actions OIDC token for `aud`, from a repository owned by `owner`.
fn oidc_token(aud: &str, owner: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let claims = serde_json::json!({
        "iss": GITHUB_ACTIONS_ISSUER,
        "aud": aud,
        "exp": now + 3600,
        "sub": format!("repo:{owner}/widget:ref:refs/heads/main"),
        "repository": format!("{owner}/widget"),
        "repository_owner": owner,
        "ref": "refs/heads/main",
    });
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(TEST_KID.into());
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(TEST_RSA_PEM.as_bytes()).unwrap();
    jsonwebtoken::encode(&header, &claims, &key).unwrap()
}

fn oidc() -> Arc<OidcVerifier> {
    Arc::new(OidcVerifier::from_parts(
        Provider::GitHubActions,
        // No configured audience: the platform minter verifies against the
        // tenant audience it derives, never these.
        Verifier::new(GITHUB_ACTIONS_ISSUER, Vec::<String>::new()),
        Arc::new(StaticJwks),
    ))
}

// ---------------------------------------------------------------------------
// The gateway under test.
// ---------------------------------------------------------------------------

/// Every audit event the policy layer records.
#[derive(Debug, Default)]
struct RecordingSink(Mutex<Vec<AuditEvent>>);

impl AuditSink for RecordingSink {
    fn record(&self, event: &AuditEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

impl RecordingSink {
    fn take(&self) -> Vec<AuditEvent> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

struct Gateway {
    addr: SocketAddr,
    certificate_pem: String,
    audit: Arc<RecordingSink>,
    usage: Arc<TenantUsage>,
}

/// Bind the platform service stack and `minter` on loopback. The echo
/// destinations are on loopback, so the address floor is permissive here; the
/// tenant policies are what keep each org to its own destination.
fn spawn_gateway(tenants: &PlatformTenants, minter: PlatformMinter) -> Gateway {
    let audit = Arc::new(RecordingSink::default());
    let usage = Arc::new(TenantUsage::new());
    let dispatch = Dispatch::new()
        .with_udp(UdpProxy::new(AddressPolicy::permissive()))
        .with_tcp(TcpProxy::new(AddressPolicy::permissive()));
    let service = platform_service(
        dispatch,
        64,
        tenants,
        audit.clone(),
        usage.clone(),
        skimasque::Revocations::new(),
    );

    let generated = tls::generate_self_signed(vec!["localhost".to_owned()]).unwrap();
    let server_tls = tls::server_config_from_pem(
        generated.certificate_pem.as_bytes(),
        generated.key_pem.as_bytes(),
    )
    .unwrap();
    let config = ProxyConfig::new("localhost")
        .unwrap()
        .with_minter(Arc::new(minter));
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), server_tls, service, config).unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    Gateway {
        addr,
        certificate_pem: generated.certificate_pem,
        audit,
        usage,
    }
}

/// A client session that names its application `curl`.
async fn connect(gateway: &Gateway) -> Session {
    let client_tls = tls::client_config_with_ca(gateway.certificate_pem.as_bytes()).unwrap();
    let client = Client::new(client_tls).unwrap();
    let template =
        UriTemplate::default_connect_udp(&format!("localhost:{}", gateway.addr.port())).unwrap();
    let session = client.connect(gateway.addr, template).await.unwrap();
    let mut headers = http::HeaderMap::new();
    headers.insert(
        skimasque::service::APPLICATION_HEADER,
        http::HeaderValue::from_static("curl"),
    );
    session.with_default_headers(headers)
}

/// A UDP server that echoes back `tag` followed by whatever it received.
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

/// Open a UDP tunnel to `target` and round-trip one payload.
async fn reach(session: &Session, target: SocketAddr) -> Result<Bytes, skimasque::Error> {
    let mut tunnel = session
        .connect_udp(Target::parse(&target.to_string()).unwrap())
        .await?;
    tunnel.send(b"ping").unwrap();
    let reply = timeout(REPLY_TIMEOUT, tunnel.recv())
        .await
        .expect("timed out waiting for a reply")
        .expect("tunnel closed before replying");
    Ok(reply)
}

fn assert_forbidden(result: Result<Bytes, skimasque::Error>, what: &str) {
    match result {
        Err(skimasque::Error::Rejected { status, .. }) => {
            assert_eq!(status, http::StatusCode::FORBIDDEN, "{what}")
        }
        Err(other) => panic!("{what}: expected a 403, got {other:?}"),
        Ok(reply) => panic!("{what}: the tunnel opened and replied {reply:?}"),
    }
}

/// The exchange error body as `(status, error, error_description)`.
fn exchange_error(error: skimasque::Error) -> (http::StatusCode, String, String) {
    match error {
        skimasque::Error::ExchangeFailed { status, detail } => {
            let body: serde_json::Value = serde_json::from_str(&detail.unwrap()).unwrap();
            (
                status,
                body["error"].as_str().unwrap().to_owned(),
                body["error_description"].as_str().unwrap().to_owned(),
            )
        }
        other => panic!("expected an exchange failure, got {other:?}"),
    }
}

/// The destinations each tenant reached or was refused, from the audit trail.
fn audited(events: &[AuditEvent]) -> Vec<(Option<String>, &'static str, String)> {
    events
        .iter()
        .map(|e| (e.org_id.clone(), e.decision, e.destination.clone()))
        .collect()
}

struct World {
    a: Arc<Org>,
    b: Arc<Org>,
    a_dest: SocketAddr,
    b_dest: SocketAddr,
    control: StubControlPlane,
    control_plane: ControlPlane,
    state: PathBuf,
}

async fn world(name: &str) -> World {
    let a = Arc::new(org("org_a", "acme", "acme"));
    let b = Arc::new(org("org_b", "beta", "beta-corp"));
    let a_dest = spawn_echo(b"a:").await;
    let b_dest = spawn_echo(b"b:").await;
    let list = TenantList {
        version: 7,
        tenants: vec![tenant(&a, a_dest), tenant(&b, b_dest)],
    };
    let control = stub_control_plane(list, vec![a.clone(), b.clone()]).await;
    let state = state_dir(name);
    let control_plane = ControlPlane::new(&control.base_url, &state).unwrap();
    World {
        a,
        b,
        a_dest,
        b_dest,
        control,
        control_plane,
        state,
    }
}

/// The tenants as a starting gateway builds them: from the control plane,
/// applied before anything serves.
async fn bootstrap(w: &World) -> PlatformTenants {
    let initial = initial_tenants(&w.control_plane, &gateway_identity(), &w.state)
        .await
        .expect("the control plane is up");
    let list = match initial {
        InitialTenants::Fetched(list) => list,
        InitialTenants::Cached { .. } => panic!("fetched, not cached"),
    };
    assert_eq!(list.version, 7);
    let tenants = PlatformTenants::new();
    tenants.apply(&list);
    tenants
}

fn minter(w: &World, tenants: &PlatformTenants) -> PlatformMinter {
    PlatformMinter::new(
        oidc(),
        BASE,
        w.control_plane.clone(),
        gateway_identity(),
        tenants.clone(),
        Duration::from_secs(900),
    )
    .with_owner_settings_url(owner_settings_url(&w.control.base_url))
}

/// A job for `org`: exchange an OIDC token for the org's audience, then
/// present the minted credential.
async fn job(gateway: &Gateway, org: &Org) -> (Session, Credential) {
    let session = connect(gateway).await;
    let token = oidc_token(&format!("{BASE}/o/{}", org.slug), org.owner);
    let credential = session.exchange_credential(&token).await.unwrap();
    session.set_credential(&credential).unwrap();
    (session, credential)
}

// ---------------------------------------------------------------------------
// The tests.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn each_orgs_job_reaches_only_its_own_destination_and_is_audited_under_its_org() {
    let w = world("isolation").await;
    let tenants = bootstrap(&w).await;
    let gateway = spawn_gateway(&tenants, minter(&w, &tenants));

    // Org A: minted by the control plane with A's key, for org_a.
    let (a, _) = job(&gateway, &w.a).await;
    assert_eq!(&reach(&a, w.a_dest).await.unwrap()[..], b"a:ping");
    assert_forbidden(reach(&a, w.b_dest).await, "A's job to B's destination");
    assert_eq!(
        audited(&gateway.audit.take()),
        [
            (Some("org_a".into()), "allow", w.a_dest.to_string()),
            (Some("org_a".into()), "deny", w.b_dest.to_string()),
        ]
    );

    // Org B: the mirror image.
    let (b, _) = job(&gateway, &w.b).await;
    assert_eq!(&reach(&b, w.b_dest).await.unwrap()[..], b"b:ping");
    assert_forbidden(reach(&b, w.a_dest).await, "B's job to A's destination");
    assert_eq!(
        audited(&gateway.audit.take()),
        [
            (Some("org_b".into()), "allow", w.b_dest.to_string()),
            (Some("org_b".into()), "deny", w.a_dest.to_string()),
        ]
    );

    // The control plane was asked for exactly one credential per org, each
    // for the org its audience named.
    let mints: Vec<(String, Option<String>)> = w
        .control
        .mints()
        .into_iter()
        .map(|m| (m.org_id, m.identity.organization))
        .collect();
    assert_eq!(
        mints,
        [
            ("org_a".to_owned(), Some("acme".to_owned())),
            ("org_b".to_owned(), Some("beta-corp".to_owned())),
        ]
    );

    // Usage is metered per org: one tunnel each.
    let usage: BTreeMap<String, u64> = gateway
        .usage
        .snapshot()
        .into_iter()
        .map(|(org, s)| (org, s.tunnels_opened))
        .collect();
    assert_eq!(usage.get("org_a"), Some(&1), "{usage:?}");
    assert_eq!(usage.get("org_b"), Some(&1), "{usage:?}");
    std::fs::remove_dir_all(&w.state).ok();
}

#[tokio::test]
async fn a_credential_for_one_org_cannot_be_replayed_as_the_other() {
    let w = world("replay").await;
    let tenants = bootstrap(&w).await;
    let gateway = spawn_gateway(&tenants, minter(&w, &tenants));

    // A credential signed with A's key that claims org_b: refused before any
    // policy runs, so nothing is audited.
    let session = connect(&gateway).await;
    let forged =
        w.a.signer
            .issue_for_org(&Default::default(), None, "org_b")
            .unwrap();
    let mut headers = http::HeaderMap::new();
    headers.insert(
        skimasque::service::APPLICATION_HEADER,
        http::HeaderValue::from_static("curl"),
    );
    headers.insert(
        http::header::PROXY_AUTHORIZATION,
        format!("Bearer {}", forged.token).parse().unwrap(),
    );
    let session = session.with_default_headers(headers);
    match reach(&session, w.b_dest).await {
        Err(skimasque::Error::Rejected { status, .. }) => assert!(
            status == http::StatusCode::FORBIDDEN
                || status == http::StatusCode::PROXY_AUTHENTICATION_REQUIRED,
            "{status}"
        ),
        other => panic!("a forged credential opened a tunnel: {other:?}"),
    }
    assert!(gateway.audit.take().is_empty());
    std::fs::remove_dir_all(&w.state).ok();
}

#[tokio::test]
async fn a_job_whose_owner_is_not_verified_gets_owner_not_verified() {
    let w = world("unverified").await;
    let tenants = bootstrap(&w).await;
    let gateway = spawn_gateway(&tenants, minter(&w, &tenants));

    let session = connect(&gateway).await;
    // B's owner asking for A's audience.
    let token = oidc_token(&format!("{BASE}/o/acme"), "beta-corp");
    let (status, code, message) =
        exchange_error(session.exchange_credential(&token).await.unwrap_err());
    assert_eq!(status, http::StatusCode::FORBIDDEN);
    assert_eq!(code, "owner_not_verified");
    assert_eq!(
        message,
        format!(
            "`beta-corp` is not verified for `acme`. Verify it at {}/app/settings/identity.",
            w.control.base_url
        )
    );
    assert!(
        w.control.mints().is_empty(),
        "the control plane was asked to mint"
    );
    std::fs::remove_dir_all(&w.state).ok();
}

#[tokio::test]
async fn with_the_control_plane_down_exchange_fails_but_issued_credentials_still_enforce() {
    let w = world("outage").await;
    let tenants = bootstrap(&w).await;
    let gateway = spawn_gateway(&tenants, minter(&w, &tenants));
    let (a, a_credential) = job(&gateway, &w.a).await;
    assert_eq!(&reach(&a, w.a_dest).await.unwrap()[..], b"a:ping");

    w.control.stop();

    // A new exchange cannot be minted, and nothing is minted locally.
    let session = connect(&gateway).await;
    let token = oidc_token(&format!("{BASE}/o/acme"), "acme");
    let (status, code, message) =
        exchange_error(session.exchange_credential(&token).await.unwrap_err());
    assert_eq!(status, http::StatusCode::BAD_GATEWAY);
    assert_eq!(code, "temporarily_unavailable");
    assert_eq!(message, UNAVAILABLE);

    // The credential minted before the outage still enforces: A's
    // destination only.
    assert_eq!(&reach(&a, w.a_dest).await.unwrap()[..], b"a:ping");
    assert_forbidden(reach(&a, w.b_dest).await, "A's job to B's destination");

    // A gateway restarting now starts from the cached tenant table and
    // enforces the same credential the same way.
    let restarted = match initial_tenants(&w.control_plane, &gateway_identity(), &w.state)
        .await
        .expect("the tenant cache stands in for the control plane")
    {
        InitialTenants::Cached { list, .. } => list,
        InitialTenants::Fetched(_) => panic!("the control plane is down"),
    };
    assert_eq!(restarted.version, 7);
    let tenants = PlatformTenants::new();
    tenants.apply(&restarted);
    let second = spawn_gateway(&tenants, minter(&w, &tenants));
    let session = connect(&second).await;
    let token = oidc_token(&format!("{BASE}/o/acme"), "acme");
    assert_eq!(
        exchange_error(session.exchange_credential(&token).await.unwrap_err()).0,
        http::StatusCode::BAD_GATEWAY
    );
    // Present the credential A was minted before the outage.
    session.set_credential(&a_credential).unwrap();
    assert_eq!(&reach(&session, w.a_dest).await.unwrap()[..], b"a:ping");
    assert_forbidden(
        reach(&session, w.b_dest).await,
        "A's credential to B's destination",
    );
    std::fs::remove_dir_all(&w.state).ok();
}

#[tokio::test]
async fn a_gateway_with_no_control_plane_and_no_cache_refuses_to_start() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let state = state_dir("nocache");
    let control = ControlPlane::new(&format!("http://127.0.0.1:{port}"), &state).unwrap();
    let error = match initial_tenants(&control, &gateway_identity(), &state).await {
        Err(error) => format!("{error:#}"),
        Ok(_) => panic!("a tenant table appeared from nowhere"),
    };
    assert!(error.contains("no cached tenant list"), "{error}");
    std::fs::remove_dir_all(&state).ok();
}

#[test]
fn the_owner_settings_url_is_the_control_planes_identity_page() {
    assert_eq!(
        owner_settings_url("https://control.skimasque.com/"),
        "https://control.skimasque.com/app/settings/identity"
    );
    assert_eq!(
        owner_settings_url("control.example"),
        "https://control.example/app/settings/identity"
    );
}
