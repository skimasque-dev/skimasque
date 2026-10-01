//! The platform gateway's tenant resolution: the tunnel-side credential
//! verifier ([`PlatformTenantVerifier`]), the token-exchange minter
//! ([`PlatformMinter`]) against a stub control plane, and the tenant table's
//! apply / cache / sync behaviour. These are the checks that decide which
//! customer a credential or a CI job belongs to.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::prelude::{Engine as _, BASE64_STANDARD};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use ring::signature::KeyPair as _;
use skimasque::{CredentialMinter, MintError, TenantVerifier};
use skimasque_cli::control::{ControlPlane, SyncState};
use skimasque_cli::platform::{
    run_tenant_sync, PlatformMinter, PlatformTenantVerifier, PlatformTenants,
};
use skimasque_identity::{
    CredentialSigner, Error as IdentityError, JwksProvider, OidcVerifier, Provider, Verifier,
    GITHUB_ACTIONS_ISSUER,
};
use skimasque_policy::WorkloadIdentity;
use skimasque_protocol::platform::{Tenant, TenantList, TenantPolicy};
use skimasque_protocol::{GatewayIdentity, PolicyDocument, SigningKey};

const BASE: &str = "https://gw.example";
const UNAVAILABLE: &str = "SkiMasque control plane unreachable; try again shortly.";

// ---------------------------------------------------------------------------
// Org signing keys (Ed25519) and credentials.
// ---------------------------------------------------------------------------

struct OrgKey {
    signer: CredentialSigner,
    public_b64: String,
}

fn org_key() -> OrgKey {
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    OrgKey {
        signer: CredentialSigner::from_pkcs8_der(pkcs8.as_ref(), Duration::from_secs(600)),
        public_b64: BASE64_STANDARD.encode(pair.public_key().as_ref()),
    }
}

fn workload(owner: &str) -> WorkloadIdentity {
    WorkloadIdentity {
        organization: Some(owner.into()),
        repository: Some(format!("{owner}/widget")),
        ..Default::default()
    }
}

/// A credential signed by `key` that claims `org_id`.
fn credential(key: &OrgKey, org_id: &str) -> String {
    key.signer
        .issue_for_org(&workload("acme"), None, org_id)
        .unwrap()
        .token
}

fn signing_key(org_id: &str, public_b64: &str) -> SigningKey {
    SigningKey {
        org_id: org_id.into(),
        algorithm: "ed25519".into(),
        public_key_b64: public_b64.into(),
        previous_public_key_b64: None,
    }
}

const GOOD_POLICY: &str = "name = \"p\"\n[[rules]]\napplication = \"curl\"\naction = \"allow\"\ndestinations = [\"example.com:443\"]\n";

fn tenant(org_id: &str, slug: &str, owners: &[&str], key: &OrgKey) -> Tenant {
    Tenant {
        org_id: org_id.into(),
        slug: slug.into(),
        owners: owners.iter().map(|o| (*o).to_owned()).collect(),
        owner_ids: BTreeMap::new(),
        policy: Some(TenantPolicy {
            version: 1,
            documents: vec![PolicyDocument {
                name: "p.toml".into(),
                text: GOOD_POLICY.into(),
            }],
        }),
        signing_key: signing_key(org_id, &key.public_b64),
    }
}

struct Fleet {
    a: OrgKey,
    b: OrgKey,
    list: TenantList,
}

/// Two tenants: `acme` (org_a, owner `acme` with numeric id 900 recorded) and
/// `beta` (org_b, owner `beta-corp`, no id recorded), each with its own key.
fn fleet() -> Fleet {
    let a = org_key();
    let b = org_key();
    let mut acme = tenant("org_a", "acme", &["acme"], &a);
    acme.owner_ids.insert("acme".into(), 900);
    let beta = tenant("org_b", "beta", &["beta-corp"], &b);
    Fleet {
        list: TenantList {
            version: 1,
            tenants: vec![acme, beta],
        },
        a,
        b,
    }
}

fn tenants_for(list: &TenantList) -> PlatformTenants {
    let tenants = PlatformTenants::new();
    tenants.apply(list);
    tenants
}

async fn verify(
    tenants: &PlatformTenants,
    token: &str,
) -> Result<(String, WorkloadIdentity), String> {
    let verifier = PlatformTenantVerifier::new(tenants.clone());
    verifier
        .verify(token.to_owned())
        .await
        .map(|(id, identity)| (id.as_str().to_owned(), identity))
}

// ---------------------------------------------------------------------------
// OIDC: a fixed RS256 test key and an in-memory JWK Set.
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

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn sign_oidc(claims: serde_json::Value) -> String {
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(TEST_KID.into());
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(TEST_RSA_PEM.as_bytes()).unwrap();
    jsonwebtoken::encode(&header, &claims, &key).unwrap()
}

/// A GitHub Actions OIDC token for `aud`, from a repository owned by `owner`.
fn oidc_token(aud: &str, owner: &str, owner_id: Option<u64>) -> String {
    let mut claims = serde_json::json!({
        "iss": GITHUB_ACTIONS_ISSUER,
        "aud": aud,
        "exp": now() + 3600,
        "sub": format!("repo:{owner}/widget:ref:refs/heads/main"),
        "repository": format!("{owner}/widget"),
        "repository_owner": owner,
        "ref": "refs/heads/main",
    });
    if let Some(id) = owner_id {
        claims["repository_owner_id"] = serde_json::Value::String(id.to_string());
    }
    sign_oidc(claims)
}

fn oidc() -> Arc<OidcVerifier> {
    Arc::new(OidcVerifier::from_parts(
        Provider::GitHubActions,
        // The configured audience is deliberately none of the tenants': the
        // minter must verify against the tenant's audience it derived.
        Verifier::new(GITHUB_ACTIONS_ISSUER, ["https://not-a-tenant.example"]),
        Arc::new(StaticJwks),
    ))
}

// ---------------------------------------------------------------------------
// A stub control plane that records each request and answers one canned reply.
// ---------------------------------------------------------------------------

struct Stub {
    base_url: String,
    seen: Arc<Mutex<Vec<(String, String)>>>,
}

impl Stub {
    /// `(path, body)` of every request received.
    fn requests(&self) -> Vec<(String, String)> {
        self.seen.lock().unwrap().clone()
    }
}

async fn stub(status: u16, body: &str) -> Stub {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let log = seen.clone();
    let body = body.to_owned();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            let body = body.clone();
            tokio::spawn(async move {
                let service = service_fn(move |req: Request<Incoming>| {
                    let log = log.clone();
                    let body = body.clone();
                    async move {
                        let (parts, incoming) = req.into_parts();
                        let got = incoming.collect().await.unwrap().to_bytes();
                        log.lock().unwrap().push((
                            parts.uri.path().to_owned(),
                            String::from_utf8(got.to_vec()).unwrap(),
                        ));
                        let mut response = Response::new(Full::new(Bytes::from(body)));
                        *response.status_mut() = StatusCode::from_u16(status).unwrap();
                        Ok::<_, Infallible>(response)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    Stub {
        base_url: format!("http://{addr}"),
        seen,
    }
}

fn state_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("skmcp-pgx-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn gateway() -> GatewayIdentity {
    GatewayIdentity {
        gateway_id: "gw_p1".into(),
        org_id: String::new(),
        secret: "s3cret".into(),
    }
}

fn minter(control_url: &str, tenants: &PlatformTenants) -> PlatformMinter {
    let control = ControlPlane::new(control_url, state_dir("minter")).unwrap();
    PlatformMinter::new(
        oidc(),
        BASE,
        control,
        gateway(),
        tenants.clone(),
        Duration::from_secs(900),
    )
}

fn refused(result: Result<skimasque::MintedCredential, MintError>) -> (String, String) {
    match result {
        Err(MintError::Refused { code, message }) => (code, message),
        Err(other) => panic!("expected a refusal, got {other}"),
        Ok(_) => panic!("expected a refusal, got a credential"),
    }
}

const MINTED: &str = r#"{"credential":"jwt.minted.cred","expires_in":600}"#;

// ---------------------------------------------------------------------------
// The tunnel-side verifier.
// ---------------------------------------------------------------------------

/// Invariant 1: a credential is verified with only the claimed org's keys.
#[tokio::test]
async fn a_credential_signed_by_a_but_claiming_b_is_refused() {
    let f = fleet();
    let tenants = tenants_for(&f.list);

    // Signed by A's key, claiming B.
    let forged = credential(&f.a, "org_b");
    let err = verify(&tenants, &forged).await.unwrap_err();
    assert!(!err.contains("org_a") && !err.contains("acme"), "{err}");

    // No org_id at all (a single-org credential signed by A's key).
    let unscoped = f.a.signer.issue(&workload("acme"), None).unwrap().token;
    assert_eq!(
        verify(&tenants, &unscoped).await.unwrap_err(),
        "credential is not scoped to an organisation"
    );

    // An org the table does not hold, signed by a key of its own.
    let stranger = org_key();
    assert_eq!(
        verify(&tenants, &credential(&stranger, "org_zzz"))
            .await
            .unwrap_err(),
        "unknown organisation"
    );
    // ...and one signed by A's key.
    assert_eq!(
        verify(&tenants, &credential(&f.a, "org_zzz"))
            .await
            .unwrap_err(),
        "unknown organisation"
    );

    // Garbage is refused, not a panic.
    assert!(verify(&tenants, "not.a.jwt").await.is_err());

    // B's genuine credential resolves to B, and A's to A.
    let (org, identity) = verify(&tenants, &credential(&f.b, "org_b")).await.unwrap();
    assert_eq!(org, "org_b");
    assert_eq!(identity.repository.as_deref(), Some("acme/widget"));
    let (org, _) = verify(&tenants, &credential(&f.a, "org_a")).await.unwrap();
    assert_eq!(org, "org_a");
}

#[tokio::test]
async fn a_tenant_with_a_bad_signing_key_verifies_nothing_but_others_still_do() {
    let mut f = fleet();
    f.list.tenants[0].signing_key.public_key_b64 = "!!! not base64 !!!".into();
    let tenants = tenants_for(&f.list);

    assert!(verify(&tenants, &credential(&f.a, "org_a")).await.is_err());
    // The tenant itself is still in the table (its policy is still there);
    // only its credentials are refused.
    assert!(tenants.table.snapshot().resolve_org("org_a").is_some());
    let (org, _) = verify(&tenants, &credential(&f.b, "org_b")).await.unwrap();
    assert_eq!(org, "org_b");

    // A bad *previous* key poisons the tenant's keys the same way.
    let mut f = fleet();
    f.list.tenants[1].signing_key.previous_public_key_b64 = Some("%%%".into());
    let tenants = tenants_for(&f.list);
    assert!(verify(&tenants, &credential(&f.b, "org_b")).await.is_err());
    assert!(verify(&tenants, &credential(&f.a, "org_a")).await.is_ok());
}

#[tokio::test]
async fn a_tenant_with_a_broken_policy_is_deny_all_and_others_are_unaffected() {
    let mut f = fleet();
    f.list.tenants[0].policy = Some(TenantPolicy {
        version: 2,
        documents: vec![PolicyDocument {
            name: "broken.toml".into(),
            text: "this is [[[ not a policy".into(),
        }],
    });
    // A tenant with no policy published yet is deny-all too.
    let c = org_key();
    let mut gamma = tenant("org_c", "gamma", &["gamma"], &c);
    gamma.policy = None;
    f.list.tenants.push(gamma);
    let tenants = tenants_for(&f.list);

    let snap = tenants.table.snapshot();
    let acme = snap
        .resolve_org("org_a")
        .expect("a broken policy keeps the tenant");
    assert!(acme.policy().policies().is_empty(), "deny-all");
    assert!(snap
        .resolve_org("org_c")
        .unwrap()
        .policy()
        .policies()
        .is_empty());
    let beta = snap.resolve_org("org_b").unwrap();
    assert_eq!(beta.policy().policies().len(), 1, "beta is unaffected");

    // Credentials still verify for all three; the policy layer denies A and C.
    assert!(verify(&tenants, &credential(&f.a, "org_a")).await.is_ok());
    assert!(verify(&tenants, &credential(&f.b, "org_b")).await.is_ok());
    assert!(verify(&tenants, &credential(&c, "org_c")).await.is_ok());
}

/// Invariant 5: removal takes effect at the next apply.
#[tokio::test]
async fn a_removed_tenant_stops_verifying_at_the_next_apply() {
    let f = fleet();
    let tenants = tenants_for(&f.list);
    let b_cred = credential(&f.b, "org_b");
    assert!(verify(&tenants, &b_cred).await.is_ok());

    let mut without_b = f.list.clone();
    without_b.version = 2;
    without_b.tenants.retain(|t| t.org_id != "org_b");
    tenants.apply(&without_b);

    assert_eq!(
        verify(&tenants, &b_cred).await.unwrap_err(),
        "unknown organisation"
    );
    assert!(verify(&tenants, &credential(&f.a, "org_a")).await.is_ok());
    assert_eq!(tenants.table.snapshot().version(), 2);
}

/// A later duplicate org_id or slug is dropped from the table; its key must be
/// dropped with it, or the table's org would verify against the wrong key.
#[tokio::test]
async fn a_duplicate_tenant_entry_never_lends_its_key_to_the_tenant_that_was_kept() {
    let f = fleet();
    let intruder = org_key();
    let mut list = f.list.clone();
    // Same org_id as acme, different slug and key.
    list.tenants
        .push(tenant("org_a", "other", &["x"], &intruder));
    // Same slug as beta (dropped), then the same org_id again under a new slug.
    list.tenants
        .push(tenant("org_d", "beta", &["y"], &intruder));
    let d = org_key();
    list.tenants.push(tenant("org_d", "delta", &["z"], &d));
    let tenants = tenants_for(&list);

    assert!(verify(&tenants, &credential(&intruder, "org_a"))
        .await
        .is_err());
    assert!(verify(&tenants, &credential(&f.a, "org_a")).await.is_ok());
    let snap = tenants.table.snapshot();
    assert_eq!(snap.resolve_slug("beta").unwrap().id().as_str(), "org_b");
    // org_d resolves (to the `delta` entry) and verifies only with delta's key.
    assert_eq!(snap.resolve_org("org_d").unwrap().slug(), "delta");
    assert!(verify(&tenants, &credential(&intruder, "org_d"))
        .await
        .is_err());
    assert!(verify(&tenants, &credential(&d, "org_d")).await.is_ok());
}

#[tokio::test]
async fn the_tenant_cache_round_trips_and_a_restart_with_the_control_plane_down_still_enforces() {
    let f = fleet();
    let dir = state_dir("cache");
    assert!(
        PlatformTenants::load_cache(&dir).is_none(),
        "nothing cached yet"
    );

    PlatformTenants::write_cache(&dir, &f.list).unwrap();
    assert!(dir.join("tenants.json").exists());
    let loaded = PlatformTenants::load_cache(&dir).expect("cache reads back");
    assert_eq!(loaded, f.list);

    // A "restarted" gateway: fresh state, built only from the cache.
    let tenants = tenants_for(&loaded);
    let (org, _) = verify(&tenants, &credential(&f.b, "org_b")).await.unwrap();
    assert_eq!(org, "org_b");
    assert!(verify(&tenants, &credential(&f.a, "org_b")).await.is_err());

    // Overwriting replaces the cache; a corrupt one reads as nothing.
    let mut v2 = f.list.clone();
    v2.version = 2;
    PlatformTenants::write_cache(&dir, &v2).unwrap();
    assert_eq!(PlatformTenants::load_cache(&dir).unwrap().version, 2);
    std::fs::write(dir.join("tenants.json"), b"{ not json").unwrap();
    assert!(PlatformTenants::load_cache(&dir).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn tenant_sync_applies_the_polled_list_and_writes_the_cache() {
    let f = fleet();
    let control = stub(200, &serde_json::to_string(&f.list).unwrap()).await;
    let dir = state_dir("sync");
    let tenants = PlatformTenants::new();
    let task = tokio::spawn(run_tenant_sync(
        ControlPlane::new(&control.base_url, &dir).unwrap(),
        gateway(),
        tenants.clone(),
        sync_state(),
        dir.clone(),
        Duration::from_secs(1),
    ));

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tenants.table.snapshot().version() != 1 || PlatformTenants::load_cache(&dir).is_none() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "sync never applied the list"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    task.abort();
    assert!(verify(&tenants, &credential(&f.a, "org_a")).await.is_ok());
    assert_eq!(PlatformTenants::load_cache(&dir).unwrap(), f.list);
    let (path, _) = &control.requests()[0];
    assert_eq!(path, "/v1/platform/gateways/gw_p1/tenants");
    std::fs::remove_dir_all(&dir).ok();
}

fn sync_state() -> Arc<SyncState> {
    Arc::new(SyncState::new(
        None,
        Some(Duration::ZERO),
        Duration::from_secs(900),
        Duration::from_secs(1800),
    ))
}

/// The tenant sync feeds the heartbeat's health: contact and the enforced
/// version on success, unhealthy on failure.
#[tokio::test]
async fn tenant_sync_reports_contact_version_and_failure_to_the_sync_state() {
    let f = fleet();
    let control = stub(200, &serde_json::to_string(&f.list).unwrap()).await;
    let dir = state_dir("sync-state");
    let state = sync_state();
    state.mark_unreachable();
    let task = tokio::spawn(run_tenant_sync(
        ControlPlane::new(&control.base_url, &dir).unwrap(),
        gateway(),
        PlatformTenants::new(),
        state.clone(),
        dir.clone(),
        Duration::from_secs(1),
    ));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while state.version() != Some(1) || !state.healthy() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "sync never reported"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    task.abort();

    // A control plane that answers 5xx: unhealthy, the version kept.
    let down = stub(503, "down").await;
    let task = tokio::spawn(run_tenant_sync(
        ControlPlane::new(&down.base_url, &dir).unwrap(),
        gateway(),
        PlatformTenants::new(),
        state.clone(),
        dir.clone(),
        Duration::from_secs(1),
    ));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while state.healthy() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "failure never reported"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    task.abort();
    assert_eq!(state.version(), Some(1));
    std::fs::remove_dir_all(&dir).ok();
}

/// A control plane that answers every poll at once (a `304` that ignores
/// `?wait=`) must not be polled in a hot loop.
#[tokio::test]
async fn tenant_sync_waits_between_polls_even_when_the_control_plane_answers_at_once() {
    let control = stub(304, "").await;
    let dir = state_dir("sync-floor");
    let task = tokio::spawn(run_tenant_sync(
        ControlPlane::new(&control.base_url, &dir).unwrap(),
        gateway(),
        PlatformTenants::new(),
        sync_state(),
        dir.clone(),
        Duration::from_secs(30),
    ));
    tokio::time::sleep(Duration::from_millis(2500)).await;
    task.abort();
    let polls = control.requests().len();
    assert!((1..=4).contains(&polls), "{polls} polls in 2.5s");
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// The token-exchange minter.
// ---------------------------------------------------------------------------

/// A JWKS source that always fails with an internal detail.
#[derive(Debug)]
struct BrokenJwks;

impl JwksProvider for BrokenJwks {
    fn fetch(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<jsonwebtoken::jwk::JwkSet, IdentityError>> + Send + '_>>
    {
        Box::pin(async {
            Err(IdentityError::Jwks(
                "GET http://10.1.2.3/internal/jwks: connection refused".to_owned(),
            ))
        })
    }
}

/// When GitHub's keys cannot be fetched the job is told to retry, with a fixed
/// message: the fetch error's detail stays in the gateway's log.
#[tokio::test]
async fn an_oidc_key_outage_is_unavailable_with_a_fixed_message() {
    let f = fleet();
    let tenants = tenants_for(&f.list);
    let control = stub(200, MINTED).await;
    let minter = PlatformMinter::new(
        Arc::new(OidcVerifier::from_parts(
            Provider::GitHubActions,
            Verifier::new(GITHUB_ACTIONS_ISSUER, ["https://not-a-tenant.example"]),
            Arc::new(BrokenJwks),
        )),
        BASE,
        ControlPlane::new(&control.base_url, state_dir("jwks")).unwrap(),
        gateway(),
        tenants,
        Duration::from_secs(900),
    );
    match minter
        .mint(oidc_token(&format!("{BASE}/o/acme"), "acme", Some(900)))
        .await
    {
        Err(MintError::Unavailable(message)) => {
            assert_eq!(message, "GitHub OIDC keys unavailable; try again shortly.")
        }
        Err(other) => panic!("expected unavailable, got {other}"),
        Ok(_) => panic!("minted without OIDC keys"),
    }
    assert!(control.requests().is_empty());
}

#[tokio::test]
async fn a_verified_owner_is_minted_a_credential_by_the_control_plane_for_its_org() {
    let f = fleet();
    let tenants = tenants_for(&f.list);
    let control = stub(200, MINTED).await;
    let minter = minter(&control.base_url, &tenants);

    // Owner case is folded; the recorded numeric id matches.
    let minted = minter
        .mint(oidc_token(&format!("{BASE}/o/acme"), "Acme", Some(900)))
        .await
        .unwrap();
    assert_eq!(minted.credential, "jwt.minted.cred");
    assert_eq!(minted.expires_in, Duration::from_secs(600));

    let seen = control.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "/v1/platform/gateways/gw_p1/credentials");
    let body: serde_json::Value = serde_json::from_str(&seen[0].1).unwrap();
    assert_eq!(body["org_id"], "org_a");
    assert_eq!(body["owner_id"], 900);
    assert_eq!(body["ttl_seconds"], 900);
    assert_eq!(body["identity"]["repository"], "Acme/widget");

    // beta records no numeric id: the login alone is enough. A trailing slash
    // on the audience is tolerated.
    minter
        .mint(oidc_token(&format!("{BASE}/o/beta/"), "beta-corp", None))
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&control.requests()[1].1).unwrap();
    assert_eq!(body["org_id"], "org_b");
}

/// Invariant 4: an owner who is not verified for the audience's org never
/// reaches the control plane's mint.
#[tokio::test]
async fn a_job_whose_owner_is_not_verified_never_receives_a_credential() {
    let f = fleet();
    let tenants = tenants_for(&f.list);
    let control = stub(200, MINTED).await;
    let minter = minter(&control.base_url, &tenants);
    let acme_aud = format!("{BASE}/o/acme");

    // beta's owner asking for acme's audience.
    let (code, message) = refused(
        minter
            .mint(oidc_token(&acme_aud, "beta-corp", Some(900)))
            .await,
    );
    assert_eq!(code, "owner_not_verified");
    assert_eq!(
        message,
        "`beta-corp` is not verified for `acme`. Verify it at \
         https://control.skimasque.com/settings/owners."
    );

    // The right login but a different numeric id (a renamed-away login).
    let (code, _) = refused(minter.mint(oidc_token(&acme_aud, "acme", Some(901))).await);
    assert_eq!(code, "owner_not_verified");
    // ...or no numeric id at all where one is recorded.
    let (code, _) = refused(minter.mint(oidc_token(&acme_aud, "acme", None)).await);
    assert_eq!(code, "owner_not_verified");

    // A token with no repository owner at all.
    let no_owner = sign_oidc(serde_json::json!({
        "iss": GITHUB_ACTIONS_ISSUER, "aud": acme_aud, "exp": now() + 3600,
    }));
    let (code, message) = refused(minter.mint(no_owner).await);
    assert_eq!(code, "owner_not_verified");
    assert_eq!(
        message,
        "The job's token names no repository owner, so it cannot be verified for `acme`."
    );

    assert!(
        control.requests().is_empty(),
        "the control plane was called"
    );
}

#[tokio::test]
async fn an_unknown_slug_is_refused_with_unknown_org() {
    let f = fleet();
    let tenants = tenants_for(&f.list);
    let control = stub(200, MINTED).await;
    let minter = minter(&control.base_url, &tenants);

    let (code, message) = refused(
        minter
            .mint(oidc_token(&format!("{BASE}/o/nobody"), "acme", Some(900)))
            .await,
    );
    assert_eq!(code, "unknown_org");
    assert_eq!(
        message,
        "Unknown SkiMasque organisation `nobody`. Check the `audience` in your workflow."
    );

    // Another host's audience, the bare host, and no audience at all.
    for aud in [
        serde_json::json!("https://evil.example/o/acme"),
        serde_json::json!(BASE),
        serde_json::json!([]),
    ] {
        let token = sign_oidc(serde_json::json!({
            "iss": GITHUB_ACTIONS_ISSUER, "aud": aud, "exp": now() + 3600,
            "repository_owner": "acme", "repository_owner_id": "900",
        }));
        let (code, message) = refused(minter.mint(token).await);
        assert_eq!(code, "unknown_org", "{aud}");
        assert_eq!(
            message,
            "Unknown SkiMasque organisation. Check the `audience` in your workflow. It must be \
             `https://gw.example/o/<your-org-slug>`.",
            "{aud}"
        );
    }

    // Two different tenants' audiences in one token: ambiguous, refused.
    let both = sign_oidc(serde_json::json!({
        "iss": GITHUB_ACTIONS_ISSUER,
        "aud": [format!("{BASE}/o/acme"), format!("{BASE}/o/beta")],
        "exp": now() + 3600, "repository_owner": "acme", "repository_owner_id": "900",
    }));
    let (code, message) = refused(minter.mint(both).await);
    assert_eq!(code, "unknown_org");
    assert_eq!(
        message,
        "The token names more than one SkiMasque organisation. Check the `audience` in your \
         workflow."
    );

    assert!(control.requests().is_empty());
}

#[tokio::test]
async fn a_wrong_case_slug_is_not_folded_into_another_org() {
    let f = fleet();
    let tenants = tenants_for(&f.list);
    let control = stub(200, MINTED).await;
    let minter = minter(&control.base_url, &tenants);

    for slug in ["Acme", "ACME", "aCme"] {
        let (code, _) = refused(
            minter
                .mint(oidc_token(&format!("{BASE}/o/{slug}"), "acme", Some(900)))
                .await,
        );
        assert_eq!(code, "unknown_org", "{slug}");
    }
    assert!(control.requests().is_empty());
}

#[tokio::test]
async fn a_token_that_does_not_verify_is_unauthorized_and_never_reaches_the_control_plane() {
    let f = fleet();
    let tenants = tenants_for(&f.list);
    let control = stub(200, MINTED).await;
    let minter = minter(&control.base_url, &tenants);

    // Wrong issuer.
    let token = sign_oidc(serde_json::json!({
        "iss": "https://evil.example", "aud": format!("{BASE}/o/acme"), "exp": now() + 3600,
        "repository_owner": "acme", "repository_owner_id": "900",
    }));
    assert!(matches!(
        minter.mint(token).await,
        Err(MintError::Unauthorized(_))
    ));
    // Expired.
    let token = sign_oidc(serde_json::json!({
        "iss": GITHUB_ACTIONS_ISSUER, "aud": format!("{BASE}/o/acme"), "exp": now() - 3600,
        "repository_owner": "acme", "repository_owner_id": "900",
    }));
    assert!(matches!(
        minter.mint(token).await,
        Err(MintError::Unauthorized(_))
    ));
    assert!(control.requests().is_empty());
}

#[tokio::test]
async fn a_control_plane_refusal_is_relayed_verbatim() {
    let f = fleet();
    let tenants = tenants_for(&f.list);
    let over_cap =
        "Shared gateway tunnels this month: 500 / 500. Upgrade at https://control.example/billing.";
    let control = stub(
        403,
        &serde_json::json!({"code": "over_cap", "message": over_cap}).to_string(),
    )
    .await;
    let minter = minter(&control.base_url, &tenants);

    let (code, message) = refused(
        minter
            .mint(oidc_token(&format!("{BASE}/o/acme"), "acme", Some(900)))
            .await,
    );
    assert_eq!(code, "over_cap");
    assert_eq!(message, over_cap);
    assert_eq!(control.requests().len(), 1);
}

#[tokio::test]
async fn a_control_plane_that_is_down_is_a_502_style_unavailable_with_no_local_mint() {
    let f = fleet();
    let tenants = tenants_for(&f.list);
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let minter = minter(&format!("http://127.0.0.1:{port}"), &tenants);
    match minter
        .mint(oidc_token(&format!("{BASE}/o/acme"), "acme", Some(900)))
        .await
    {
        Err(MintError::Unavailable(message)) => assert_eq!(message, UNAVAILABLE),
        Err(other) => panic!("expected unavailable, got {other}"),
        Ok(_) => panic!("a credential was minted with the control plane down"),
    }

    // A 5xx is the same.
    let control = stub(503, "down for maintenance").await;
    let minter = self::minter(&control.base_url, &tenants);
    match minter
        .mint(oidc_token(&format!("{BASE}/o/acme"), "acme", Some(900)))
        .await
    {
        Err(MintError::Unavailable(message)) => assert_eq!(message, UNAVAILABLE),
        other => panic!(
            "expected unavailable, got {:?}",
            other.map(|m| m.credential)
        ),
    }
}
