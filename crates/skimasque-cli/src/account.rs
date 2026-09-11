//! Developer sign-in for the CLI: the `skimasque login` device flow and the
//! on-disk credential file it writes.
//!
//! This is the *developer's* view of the control plane —
//! [`control`](crate::control) is the *gateway's*. A session here reaches the
//! management API (orgs, policy, gateways) as the logged-in user.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// The stored result of `skimasque login`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// The control plane this session is for. `https://control.skimasque.com`
    /// (SkiMasque Cloud) is the default; `--control-plane` selects another,
    /// e.g. a self-hosted one.
    pub control_plane: String,
    /// The bearer token for the management API. Sensitive.
    pub session_token: String,
    /// The GitHub login of the signed-in user, for display.
    pub github_login: String,
    /// The control plane's id for the user (`usr_...`).
    pub user_id: String,
}

/// Where the credential file lives:
/// `$SKIMASQUE_CONFIG_HOME`, else `$XDG_CONFIG_HOME/skimasque`, else
/// `~/.config/skimasque` (Unix) or `%APPDATA%\skimasque` (Windows).
pub fn credentials_path() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("SKIMASQUE_CONFIG_HOME") {
        return Ok(PathBuf::from(dir).join("credentials.json"));
    }
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(dir).join("skimasque").join("credentials.json"));
    }
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"));
    let base = base.context("no home directory (set SKIMASQUE_CONFIG_HOME)")?;
    Ok(base.join("skimasque").join("credentials.json"))
}

/// Load the stored credentials, if `skimasque login` has been run.
pub fn load() -> Result<Option<Credentials>> {
    match std::fs::read(credentials_path()?) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&bytes).context("parsing the stored credentials")?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("reading the stored credentials"),
    }
}

/// Load the stored credentials, or fail with a "run `skimasque login`" message.
pub fn require() -> Result<Credentials> {
    load()?.context("not signed in -- run `skimasque login` first")
}

/// Write the credential file, 0600 on Unix.
pub fn save(creds: &Credentials) -> Result<()> {
    let path = credentials_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(creds)?;
    write_private(&path, &json).with_context(|| format!("writing {}", path.display()))
}

/// Remove the credential file. Not an error if it is already gone.
pub fn clear() -> Result<()> {
    match std::fs::remove_file(credentials_path()?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context("removing the stored credentials"),
    }
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

// --- the control-plane management API, as the CLI uses it -----------------

/// A minimal client for the control plane's developer/management surface.
pub struct Api {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
pub struct DeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DevicePoll {
    Pending {
        #[serde(default)]
        slow_down: bool,
    },
    Complete {
        session_token: String,
        user: UserView,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserView {
    pub id: String,
    pub github_id: u64,
    pub github_login: String,
    pub email: Option<String>,
}

/// An organisation, as `GET /v1/orgs` lists it.
#[derive(Debug, Clone, Deserialize)]
pub struct OrgView {
    pub id: String,
    pub name: String,
}

/// A one-time registration token from `POST /v1/orgs/{id}/registration-tokens`.
#[derive(Debug, Clone, Deserialize)]
pub struct RegistrationToken {
    pub registration_token: String,
    pub expires_at_ms: u64,
}

/// A credential the control plane minted for the session's own identity —
/// the developer-side counterpart of [`crate::control::MintedCredential`],
/// which is the gateway-side one.
#[derive(Debug, Clone)]
pub struct MintedCredential {
    pub token: String,
    pub expires_in: Duration,
}

/// A membership row, as the members endpoints return it.
#[derive(Debug, Clone, Deserialize)]
pub struct MemberView {
    pub user_id: String,
    pub github_login: Option<String>,
    /// `owner` | `member`.
    pub role: String,
    pub added_at_ms: u64,
}

/// The result of creating an organisation.
#[derive(Debug, Clone, Deserialize)]
pub struct CreatedOrg {
    pub org: OrgView,
    pub registration_token: String,
}

/// An org's usage totals, as `GET /v1/orgs/{id}/usage` returns them.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct UsageView {
    pub tunnels_total: u64,
    pub bytes_total: u64,
    pub active_gateways: u64,
    pub updated_at_ms: u64,
}

/// One audit event, as `GET /v1/orgs/{id}/audit` returns it.
#[derive(Debug, Clone, Deserialize)]
pub struct AuditView {
    pub gateway_id: String,
    pub seq: u64,
    pub received_at_ms: u64,
    /// The gateway's audit event; `None` if it did not parse control-plane side.
    pub event: Option<serde_json::Value>,
}

/// One page of the audit trail: the events plus a cursor for the next page.
#[derive(Debug, Clone, Deserialize)]
struct AuditPage {
    events: Vec<AuditView>,
    #[serde(default)]
    next_cursor: Option<String>,
}

/// Filters for [`Api::query_audit`].
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    pub gateway: Option<String>,
    pub decision: Option<String>,
    pub since: Option<String>,
    /// Total events to return. The client pages the API (500 per request) to
    /// reach this, so it may exceed one page. `None` uses the API default.
    pub limit: Option<usize>,
}

/// The result of `POST /v1/orgs/{id}/policy/simulate`.
#[derive(Debug, Clone, Deserialize)]
pub struct SimulateResult {
    /// What the evaluation ran against, e.g. `policy revision v3 (as prod-eu)`.
    pub against: String,
    /// `"allow"` or `"deny"`.
    pub outcome: String,
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default)]
    pub rule: Option<String>,
    #[serde(default)]
    pub steps: Vec<SimStep>,
    #[serde(default)]
    pub suggested_rule: Option<String>,
    #[serde(default)]
    pub closest: Vec<String>,
    #[serde(default)]
    pub limits: Vec<SimLimit>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimStep {
    pub ok: bool,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimLimit {
    pub label: String,
    pub value: String,
}

/// A gateway in the fleet, as `GET /v1/orgs/{id}/gateways` lists it.
#[derive(Debug, Clone, Deserialize)]
pub struct GatewayView {
    pub id: String,
    pub name: String,
    /// `registered` | `online` | `degraded` | `offline`.
    pub status: String,
    pub last_seen_ms: Option<u64>,
    pub acked_policy_version: Option<u64>,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
}

impl Api {
    pub fn new(base_url: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("skimasque/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("building the HTTP client")?;
        Ok(Self {
            base_url: crate::normalize_base_url(base_url),
            http,
        })
    }

    async fn error_for_status(response: reqwest::Response, what: &str) -> Result<reqwest::Response> {
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["detail"].as_str().map(str::to_owned))
            .unwrap_or_else(|| body.trim().to_owned());
        bail!("{what} failed: {status} {detail}");
    }

    /// Run the GitHub device flow to completion, printing progress to stderr.
    /// Returns the session token and the user it belongs to.
    pub async fn login(&self) -> Result<(String, UserView)> {
        let start: DeviceStart = {
            let response = self
                .http
                .post(format!("{}/v1/auth/device", self.base_url))
                .send()
                .await
                .context("starting the device flow")?;
            Self::error_for_status(response, "device authorization")
                .await?
                .json()
                .await
                .context("parsing the device response")?
        };

        eprintln!();
        eprintln!("  Open {} and enter code: {}", start.verification_uri, start.user_code);
        eprintln!();
        eprint!("  Waiting for authorization");
        let _ = std::io::stderr().flush();

        let deadline = Instant::now() + Duration::from_secs(start.expires_in.min(900));
        let mut interval = Duration::from_secs(start.interval.max(1));
        loop {
            let response = self
                .http
                .post(format!("{}/v1/auth/device/poll", self.base_url))
                .json(&serde_json::json!({ "device_code": start.device_code }))
                .send()
                .await
                .context("polling the device flow")?;
            let poll: DevicePoll = Self::error_for_status(response, "device poll")
                .await?
                .json()
                .await
                .context("parsing the poll response")?;
            match poll {
                DevicePoll::Pending { slow_down } => {
                    if slow_down {
                        interval += Duration::from_secs(5);
                    }
                }
                DevicePoll::Complete { session_token, user } => {
                    eprintln!();
                    return Ok((session_token, user));
                }
                DevicePoll::Failed { reason } => {
                    eprintln!();
                    bail!("authorization failed: {reason}");
                }
            }
            if Instant::now() + interval >= deadline {
                eprintln!();
                bail!("timed out waiting for authorization");
            }
            tokio::time::sleep(interval).await;
            eprint!(".");
            let _ = std::io::stderr().flush();
        }
    }

    /// `GET /v1/me` with a session token.
    pub async fn me(&self, session: &str) -> Result<UserView> {
        let response = self
            .http
            .get(format!("{}/v1/me", self.base_url))
            .bearer_auth(session)
            .send()
            .await
            .context("requesting /v1/me")?;
        Self::error_for_status(response, "identity check")
            .await?
            .json()
            .await
            .context("parsing /v1/me")
    }

    /// `POST /v1/auth/logout` with a session token. Best effort.
    pub async fn logout(&self, session: &str) -> Result<()> {
        self.http
            .post(format!("{}/v1/auth/logout", self.base_url))
            .bearer_auth(session)
            .send()
            .await
            .context("sending logout")?;
        Ok(())
    }

    /// `POST /v1/orgs/{org}/policy/simulate` — evaluate a hypothetical request.
    /// `body` is `{ identity, application, destination, transport }`.
    pub async fn simulate(
        &self,
        session: &str,
        org: &str,
        version: Option<u64>,
        draft: bool,
        gateway: Option<&str>,
        body: &serde_json::Value,
    ) -> Result<SimulateResult> {
        let mut req = self
            .http
            .post(format!("{}/v1/orgs/{}/policy/simulate", self.base_url, org))
            .bearer_auth(session)
            .json(body);
        if let Some(v) = version {
            req = req.query(&[("version", v.to_string())]);
        }
        if draft {
            req = req.query(&[("draft", "true")]);
        }
        if let Some(g) = gateway {
            req = req.query(&[("gateway", g)]);
        }
        let response = req.send().await.context("simulating the request")?;
        Self::error_for_status(response, "simulating the request")
            .await?
            .json()
            .await
            .context("parsing the simulation result")
    }

    /// `GET /v1/orgs` — the organisations the session's user belongs to.
    pub async fn list_orgs(&self, session: &str) -> Result<Vec<OrgView>> {
        let response = self
            .http
            .get(format!("{}/v1/orgs", self.base_url))
            .bearer_auth(session)
            .send()
            .await
            .context("listing organisations")?;
        Self::error_for_status(response, "listing organisations")
            .await?
            .json()
            .await
            .context("parsing the organisations")
    }

    /// `POST /v1/orgs/{org}/registration-tokens` — mint a one-time token for a
    /// new gateway.
    pub async fn mint_registration_token(
        &self,
        session: &str,
        org: &str,
    ) -> Result<RegistrationToken> {
        let response = self
            .http
            .post(format!(
                "{}/v1/orgs/{}/registration-tokens",
                self.base_url, org
            ))
            .bearer_auth(session)
            .json(&serde_json::json!({}))
            .send()
            .await
            .context("minting a registration token")?;
        Self::error_for_status(response, "minting a registration token")
            .await?
            .json()
            .await
            .context("parsing the registration token")
    }

    /// `POST /v1/orgs/{org}/credentials` — mint a platform credential for the
    /// signed-in session: the control plane asserts the developer's GitHub
    /// login as `actor` (never anything the caller supplies) and signs it
    /// with the org's Ed25519 key, exactly like a gateway-minted credential.
    /// `ttl_seconds` is a request the control plane may cap; `None` asks for
    /// its default.
    pub async fn mint_credential(
        &self,
        session: &str,
        org: &str,
        ttl_seconds: Option<u64>,
    ) -> Result<MintedCredential> {
        let response = self
            .http
            .post(format!("{}/v1/orgs/{}/credentials", self.base_url, org))
            .bearer_auth(session)
            .json(&skimasque_protocol::DeveloperCredentialRequest { ttl_seconds })
            .send()
            .await
            .context("minting a credential")?;
        let body: skimasque_protocol::MintResponse =
            Self::error_for_status(response, "minting a credential")
                .await?
                .json()
                .await
                .context("parsing the minted credential")?;
        Ok(MintedCredential {
            token: body.credential,
            expires_in: Duration::from_secs(body.expires_in),
        })
    }

    /// `POST /v1/orgs` — create an organisation, owned by the session's user.
    pub async fn create_org(&self, session: &str, name: &str) -> Result<CreatedOrg> {
        let response = self
            .http
            .post(format!("{}/v1/orgs", self.base_url))
            .bearer_auth(session)
            .json(&serde_json::json!({ "name": name }))
            .send()
            .await
            .context("creating an organisation")?;
        Self::error_for_status(response, "creating an organisation")
            .await?
            .json()
            .await
            .context("parsing the created organisation")
    }

    /// `GET /v1/orgs/{org}/members`.
    pub async fn list_members(&self, session: &str, org: &str) -> Result<Vec<MemberView>> {
        let response = self
            .http
            .get(format!("{}/v1/orgs/{}/members", self.base_url, org))
            .bearer_auth(session)
            .send()
            .await
            .context("listing members")?;
        Self::error_for_status(response, "listing members")
            .await?
            .json()
            .await
            .context("parsing the members")
    }

    /// `POST /v1/orgs/{org}/members` — add a member by GitHub login.
    pub async fn add_member(
        &self,
        session: &str,
        org: &str,
        github_login: &str,
    ) -> Result<MemberView> {
        let response = self
            .http
            .post(format!("{}/v1/orgs/{}/members", self.base_url, org))
            .bearer_auth(session)
            .json(&serde_json::json!({ "github_login": github_login }))
            .send()
            .await
            .context("adding a member")?;
        Self::error_for_status(response, "adding a member")
            .await?
            .json()
            .await
            .context("parsing the added member")
    }

    /// `PUT /v1/orgs/{org}/members/{user}/role` — promote or demote a member.
    /// Owner-only on the control plane. `role` is `"owner"` or `"member"`.
    pub async fn set_member_role(
        &self,
        session: &str,
        org: &str,
        user_id: &str,
        role: &str,
    ) -> Result<MemberView> {
        let response = self
            .http
            .put(format!(
                "{}/v1/orgs/{}/members/{}/role",
                self.base_url, org, user_id
            ))
            .bearer_auth(session)
            .json(&serde_json::json!({ "role": role }))
            .send()
            .await
            .context("setting a member role")?;
        Self::error_for_status(response, "setting a member role")
            .await?
            .json()
            .await
            .context("parsing the updated member")
    }

    /// `DELETE /v1/orgs/{org}/members/{user}` — remove a member. Owner-only on
    /// the control plane.
    pub async fn remove_member(&self, session: &str, org: &str, user_id: &str) -> Result<()> {
        let response = self
            .http
            .delete(format!(
                "{}/v1/orgs/{}/members/{}",
                self.base_url, org, user_id
            ))
            .bearer_auth(session)
            .send()
            .await
            .context("removing a member")?;
        Self::error_for_status(response, "removing a member").await?;
        Ok(())
    }

    /// `GET /v1/orgs/{org}/audit` — policy decisions across the fleet, newest
    /// first. Follows the API's `next_cursor` to gather up to `filter.limit`
    /// events (which may span many 500-event pages).
    pub async fn query_audit(
        &self,
        session: &str,
        org: &str,
        filter: &AuditFilter,
    ) -> Result<Vec<AuditView>> {
        /// One request's worth; the API clamps to this too.
        const PAGE: usize = 500;
        let want = filter.limit.unwrap_or(50).max(1);
        let mut out: Vec<AuditView> = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let remaining = want - out.len();
            let mut req = self
                .http
                .get(format!("{}/v1/orgs/{}/audit", self.base_url, org))
                .bearer_auth(session)
                .query(&[("limit", remaining.min(PAGE).to_string())]);
            if let Some(gw) = &filter.gateway {
                req = req.query(&[("gateway", gw)]);
            }
            if let Some(decision) = &filter.decision {
                req = req.query(&[("decision", decision)]);
            }
            if let Some(since) = &filter.since {
                req = req.query(&[("since", since)]);
            }
            if let Some(c) = &cursor {
                req = req.query(&[("cursor", c)]);
            }
            let response = req.send().await.context("querying the audit trail")?;
            let page: AuditPage = Self::error_for_status(response, "querying the audit trail")
                .await?
                .json()
                .await
                .context("parsing the audit trail")?;
            out.extend(page.events);
            match page.next_cursor {
                Some(c) if out.len() < want => cursor = Some(c),
                _ => break,
            }
        }
        Ok(out)
    }

    /// `GET /v1/orgs/{org}/usage` — the running usage totals.
    pub async fn usage(&self, session: &str, org: &str) -> Result<UsageView> {
        let response = self
            .http
            .get(format!("{}/v1/orgs/{}/usage", self.base_url, org))
            .bearer_auth(session)
            .send()
            .await
            .context("fetching usage")?;
        Self::error_for_status(response, "fetching usage")
            .await?
            .json()
            .await
            .context("parsing usage")
    }

    /// `GET /v1/orgs/{org}/gateways` — the fleet.
    pub async fn list_gateways(&self, session: &str, org: &str) -> Result<Vec<GatewayView>> {
        let response = self
            .http
            .get(format!("{}/v1/orgs/{}/gateways", self.base_url, org))
            .bearer_auth(session)
            .send()
            .await
            .context("listing gateways")?;
        Self::error_for_status(response, "listing gateways")
            .await?
            .json()
            .await
            .context("parsing the gateways")
    }
}

/// Resolve which org a command should act on: `explicit` if given, else the
/// session's only org, else an error listing the choices. Genuinely `async`
/// (just awaits [`Api::list_orgs`] directly) rather than wrapped in
/// [`block_on`], so an already-async caller (`skimasque-client`, whose `main`
/// runs on a live tokio runtime) can call it directly -- `block_on` would
/// panic there ("cannot start a runtime from within a runtime"). A
/// synchronous caller (`skimasque`'s `main`) wraps this call itself.
pub async fn resolve_org(api: &Api, creds: &Credentials, explicit: Option<String>) -> Result<String> {
    if let Some(org) = explicit {
        return Ok(org);
    }
    let orgs = api.list_orgs(&creds.session_token).await?;
    match orgs.as_slice() {
        [only] => Ok(only.id.clone()),
        [] => bail!(
            "you are not a member of any organisation on {} -- \
             `skimasque org create <name>`",
            creds.control_plane
        ),
        many => {
            eprintln!("you belong to several organisations -- pass --org <id>:");
            for org in many {
                eprintln!("  {}  {}", org.id, org.name);
            }
            bail!("--org is required")
        }
    }
}

/// Run an async block on a fresh current-thread runtime -- the CLI's `main` is
/// synchronous.
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("building a tokio runtime")
        .block_on(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_path_and_file_round_trip_under_the_override() {
        // One test so the process-global env var is not raced between tests.
        let dir = std::env::temp_dir().join(format!("skm-acct-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("SKIMASQUE_CONFIG_HOME", &dir);

        let path = credentials_path().unwrap();
        assert!(path.ends_with("credentials.json"));
        assert!(path.starts_with(&dir));

        assert!(load().unwrap().is_none());
        let creds = Credentials {
            control_plane: "https://control.example".into(),
            session_token: "skmses_secret".into(),
            github_login: "octocat".into(),
            user_id: "usr_1".into(),
        };
        save(&creds).unwrap();
        let back = load().unwrap().unwrap();
        assert_eq!(back.session_token, "skmses_secret");
        assert_eq!(back.github_login, "octocat");
        clear().unwrap();
        assert!(load().unwrap().is_none());

        std::env::remove_var("SKIMASQUE_CONFIG_HOME");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
