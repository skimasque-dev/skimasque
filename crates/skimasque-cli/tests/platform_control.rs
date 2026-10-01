//! The platform control-plane client against a stub HTTP control plane that
//! records each request and returns canned responses.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use skimasque_cli::control::{ControlPlane, PlatformMintError, TenantFetch};
use skimasque_policy::WorkloadIdentity;
use skimasque_protocol::platform::{
    PlatformAuditEvent, PlatformHeartbeatRequest, PlatformMintRequest, PlatformShipAuditRequest,
    TenantList,
};
use skimasque_protocol::GatewayIdentity;

#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    query: String,
    authorization: Option<String>,
    if_none_match: Option<String>,
    body: String,
}

#[derive(Clone)]
struct Canned {
    status: u16,
    body: String,
}

struct Stub {
    base_url: String,
    seen: Arc<Mutex<Vec<Recorded>>>,
}

impl Stub {
    fn requests(&self) -> Vec<Recorded> {
        self.seen.lock().unwrap().clone()
    }
}

/// Serve `canned` for every request, recording each one.
async fn stub(canned: Canned) -> Stub {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<Recorded>>> = Arc::default();
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            let canned = canned.clone();
            tokio::spawn(async move {
                let service = service_fn(move |req: Request<Incoming>| {
                    let log = log.clone();
                    let canned = canned.clone();
                    async move {
                        let (parts, body) = req.into_parts();
                        let body = body.collect().await.unwrap().to_bytes();
                        let header = |name: &str| {
                            parts
                                .headers
                                .get(name)
                                .map(|v| v.to_str().unwrap().to_owned())
                        };
                        log.lock().unwrap().push(Recorded {
                            method: parts.method.to_string(),
                            path: parts.uri.path().to_owned(),
                            query: parts.uri.query().unwrap_or_default().to_owned(),
                            authorization: header("authorization"),
                            if_none_match: header("if-none-match"),
                            body: String::from_utf8(body.to_vec()).unwrap(),
                        });
                        let mut response = Response::new(Full::new(Bytes::from(canned.body)));
                        *response.status_mut() = StatusCode::from_u16(canned.status).unwrap();
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
    let dir = std::env::temp_dir().join(format!("skmcp-platform-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn identity() -> GatewayIdentity {
    GatewayIdentity {
        gateway_id: "gw_p1".into(),
        org_id: String::new(),
        secret: "s3cret".into(),
    }
}

fn mint_request() -> PlatformMintRequest {
    PlatformMintRequest {
        org_id: "org_acme".into(),
        identity: WorkloadIdentity {
            organization: Some("acme".into()),
            repository: Some("acme/widget".into()),
            ..Default::default()
        },
        subject: None,
        owner_id: Some(900),
        ttl_seconds: 900,
    }
}

#[tokio::test]
async fn register_platform_posts_the_token_and_persists_an_identity_with_no_org() {
    let stub = stub(Canned {
        status: 200,
        body: r#"{"gateway_id":"gw_p1","secret":"s3cret"}"#.into(),
    })
    .await;
    let dir = state_dir("register");
    let cp = ControlPlane::new(&stub.base_url, &dir).unwrap();
    let labels: BTreeMap<String, String> = [("region".to_owned(), "eu".to_owned())].into();

    let id = cp
        .register_platform("tok_abc", "shared-1", &labels)
        .await
        .unwrap();
    assert_eq!(id.gateway_id, "gw_p1");
    assert_eq!(id.secret, "s3cret");
    assert_eq!(id.org_id, "");

    let seen = stub.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].path, "/v1/platform/register");
    let body: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body["registration_token"], "tok_abc");
    assert_eq!(body["name"], "shared-1");
    assert_eq!(body["labels"]["region"], "eu");

    let stored = cp.load_identity().unwrap().expect("identity persisted");
    assert_eq!(stored.gateway_id, "gw_p1");
    assert_eq!(stored.org_id, "");
    assert_eq!(stored.secret, "s3cret");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn fetch_tenants_sends_if_none_match_and_wait_and_maps_304_to_unchanged() {
    let list = TenantList {
        version: 5,
        tenants: vec![],
    };
    let ok = stub(Canned {
        status: 200,
        body: serde_json::to_string(&list).unwrap(),
    })
    .await;
    let dir = state_dir("tenants-ok");
    let cp = ControlPlane::new(&ok.base_url, &dir).unwrap();
    let fetched = cp
        .fetch_tenants(&identity(), Some(4), Some(Duration::from_secs(30)))
        .await
        .unwrap();
    match fetched {
        TenantFetch::Updated(got) => assert_eq!(got, list),
        TenantFetch::Unchanged => panic!("expected an update"),
    }
    let seen = ok.requests();
    assert_eq!(seen[0].method, "GET");
    assert_eq!(seen[0].path, "/v1/platform/gateways/gw_p1/tenants");
    assert_eq!(seen[0].query, "wait=30");
    assert_eq!(seen[0].if_none_match.as_deref(), Some("\"4\""));
    assert_eq!(seen[0].authorization.as_deref(), Some("Bearer s3cret"));

    let not_modified = stub(Canned {
        status: 304,
        body: String::new(),
    })
    .await;
    let cp = ControlPlane::new(&not_modified.base_url, &dir).unwrap();
    let fetched = cp.fetch_tenants(&identity(), Some(5), None).await.unwrap();
    assert!(matches!(fetched, TenantFetch::Unchanged));
    let seen = not_modified.requests();
    assert_eq!(seen[0].query, "");
    assert_eq!(seen[0].if_none_match.as_deref(), Some("\"5\""));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_403_with_a_refusal_body_is_a_refused_mint_carrying_its_code_and_message_verbatim() {
    for status in [403, 422] {
        let stub = stub(Canned {
            status,
            body:
                r#"{"code":"owner_not_verified","message":"`acme` is not verified for `widgets`."}"#
                    .into(),
        })
        .await;
        let cp = ControlPlane::new(&stub.base_url, state_dir("refused")).unwrap();
        let err = cp
            .platform_mint(&identity(), &mint_request())
            .await
            .unwrap_err();
        match err {
            PlatformMintError::Refused(refusal) => {
                assert_eq!(refusal.code, "owner_not_verified");
                assert_eq!(refusal.message, "`acme` is not verified for `widgets`.");
            }
            PlatformMintError::Unavailable(e) => panic!("expected a refusal, got {e:#}"),
        }
    }
}

#[tokio::test]
async fn a_5xx_or_a_connection_error_is_an_unavailable_mint() {
    let stub_5xx = stub(Canned {
        status: 503,
        body: "upstream down".into(),
    })
    .await;
    let cp = ControlPlane::new(&stub_5xx.base_url, state_dir("5xx")).unwrap();
    let err = cp
        .platform_mint(&identity(), &mint_request())
        .await
        .unwrap_err();
    assert!(matches!(err, PlatformMintError::Unavailable(_)), "{err:?}");

    // A 403 whose body is not a Refusal is also just unavailable.
    let garbled = stub(Canned {
        status: 403,
        body: "<html>forbidden</html>".into(),
    })
    .await;
    let cp = ControlPlane::new(&garbled.base_url, state_dir("garbled")).unwrap();
    let err = cp
        .platform_mint(&identity(), &mint_request())
        .await
        .unwrap_err();
    assert!(matches!(err, PlatformMintError::Unavailable(_)), "{err:?}");

    // Nothing is listening: bind a port, then drop the listener.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let cp = ControlPlane::new(&format!("http://127.0.0.1:{port}"), state_dir("conn")).unwrap();
    let err = cp
        .platform_mint(&identity(), &mint_request())
        .await
        .unwrap_err();
    assert!(matches!(err, PlatformMintError::Unavailable(_)), "{err:?}");
}

#[tokio::test]
async fn platform_mint_sends_the_org_identity_owner_id_and_ttl() {
    let stub = stub(Canned {
        status: 200,
        body: r#"{"credential":"jwt.abc.def","expires_in":600}"#.into(),
    })
    .await;
    let cp = ControlPlane::new(&stub.base_url, state_dir("mint")).unwrap();
    let minted = cp
        .platform_mint(&identity(), &mint_request())
        .await
        .unwrap();
    assert_eq!(minted.token, "jwt.abc.def");
    assert_eq!(minted.expires_in, Duration::from_secs(600));

    let seen = stub.requests();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].path, "/v1/platform/gateways/gw_p1/credentials");
    assert_eq!(seen[0].authorization.as_deref(), Some("Bearer s3cret"));
    let body: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body["org_id"], "org_acme");
    assert_eq!(body["identity"]["organization"], "acme");
    assert_eq!(body["identity"]["repository"], "acme/widget");
    assert_eq!(body["owner_id"], 900);
    assert_eq!(body["ttl_seconds"], 900);
}

#[tokio::test]
async fn ship_platform_audit_and_heartbeat_post_to_the_platform_paths_with_the_bearer_secret() {
    let audit = stub(Canned {
        status: 200,
        body: r#"{"head_seq":7}"#.into(),
    })
    .await;
    let cp = ControlPlane::new(&audit.base_url, state_dir("audit")).unwrap();
    let batch = PlatformShipAuditRequest {
        events: vec![PlatformAuditEvent {
            org_id: "org_acme".into(),
            seq: 7,
            prev_hash: skimasque_protocol::AUDIT_GENESIS.into(),
            event_json: r#"{"decision":"allow"}"#.into(),
        }],
    };
    let shipped = cp.ship_platform_audit(&identity(), &batch).await.unwrap();
    assert_eq!(shipped.head_seq, 7);
    let seen = audit.requests();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].path, "/v1/platform/gateways/gw_p1/audit");
    assert_eq!(seen[0].authorization.as_deref(), Some("Bearer s3cret"));
    let sent: PlatformShipAuditRequest = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(sent, batch);

    let head = stub(Canned {
        status: 200,
        body: r#"{"seq":7,"hash":"abc"}"#.into(),
    })
    .await;
    let cp = ControlPlane::new(&head.base_url, state_dir("head")).unwrap();
    let got = cp.platform_audit_head(&identity()).await.unwrap();
    assert_eq!((got.seq, got.hash.as_str()), (7, "abc"));
    let seen = head.requests();
    assert_eq!(seen[0].method, "GET");
    assert_eq!(seen[0].path, "/v1/platform/gateways/gw_p1/audit/head");
    assert_eq!(seen[0].authorization.as_deref(), Some("Bearer s3cret"));

    let hb = stub(Canned {
        status: 204,
        body: String::new(),
    })
    .await;
    let cp = ControlPlane::new(&hb.base_url, state_dir("hb")).unwrap();
    let req = PlatformHeartbeatRequest {
        status: "online".into(),
        tenants_version: Some(5),
        usage_by_org: Default::default(),
    };
    cp.platform_heartbeat(&identity(), &req).await.unwrap();
    let seen = hb.requests();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].path, "/v1/platform/gateways/gw_p1/heartbeat");
    assert_eq!(seen[0].authorization.as_deref(), Some("Bearer s3cret"));
    let sent: PlatformHeartbeatRequest = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(sent, req);
}
