//! Running a command inside Anthropic's sandbox runtime (`srt`), with its only
//! way out being SkiMasque.
//!
//! SkiMasque decides who may reach what; it does not confine a process. `srt`
//! does the confining: it removes the process's direct network access and
//! leaves it a proxy. This module points that proxy at the HTTP CONNECT front
//! end `skimasque exec` already runs (srt's `network.parentProxy`), so every
//! connection the sandboxed command makes -- HTTP(S) and SOCKS alike -- reaches
//! the gateway, and nothing else leaves the machine.
//!
//! srt also keeps an allowlist of its own and will not accept a bare `*`, so
//! the caller names the domains it covers (`--allow-domain`). That list is an
//! outer fence only: the gateway's policy still decides every connection.
//!
//! The pure parts (settings, argv, validation) are here and tested; spawning is
//! in [`crate::exec`].

use std::ffi::{OsStr, OsString};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

/// Environment variables that would make srt dial somewhere other than the
/// parent proxy we give it, or hand the command a credential or proxy of its
/// own. Removed from srt's environment (and therefore the command's).
pub const SCRUBBED_ENV: &[&str] = &[
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "SKIMASQUE_TOKEN",
    "SKIMASQUE_TOKEN_FILE",
    "SKIMASQUE_OIDC_TOKEN",
];

/// What `skimasque sandbox-probe` prints when its direct connection succeeded:
/// the sandbox is not blocking anything.
pub const PROBE_ESCAPED: &str = "connected";
/// What it prints when the connection was refused or timed out.
pub const PROBE_BLOCKED: &str = "blocked";

/// Reject an `--allow-domain` srt would refuse or that means something other
/// than a host pattern.
pub fn check_allow_domain(raw: &str) -> Result<String, String> {
    let domain = raw.trim();
    if domain.is_empty() {
        return Err("an allowed domain cannot be empty".to_owned());
    }
    if domain == "*" || domain.starts_with("*.") && !domain[2..].contains('.') {
        return Err(format!(
            "\"{domain}\" is too broad: the sandbox runtime refuses a bare \"*\" or a whole \
             top-level domain. Name the domains the agent needs, e.g. \"*.acme.dev\"; the \
             gateway's policy still decides each connection"
        ));
    }
    if domain.contains("://") || domain.contains('/') || domain.contains(char::is_whitespace) {
        return Err(format!(
            "\"{domain}\" is not a host pattern: give a host such as \"api.acme.dev\", \
             \"*.acme.dev\" or \"api.acme.dev:443\", without a scheme or path"
        ));
    }
    Ok(domain.to_owned())
}

/// The settings document handed to `srt --settings`.
///
/// `base` is the user's own file (filesystem rules and the like) and is kept as
/// it is, except for what this function owns: the network allowlist and the
/// parent proxy. Anything the base said there is replaced, because a different
/// parent proxy would send the agent's traffic somewhere other than SkiMasque.
pub fn settings(base: Option<Value>, allow: &[String], parent: SocketAddr) -> Result<Value, String> {
    let mut doc = match base {
        None => json!({}),
        Some(Value::Object(map)) => Value::Object(map),
        Some(_) => return Err("the sandbox settings file must be a JSON object".to_owned()),
    };
    if allow.is_empty() {
        return Err(
            "--sandbox needs at least one --allow-domain: the sandbox runtime allows no \
             network access until domains are named, and it will not accept \"*\""
                .to_owned(),
        );
    }
    let object = doc.as_object_mut().expect("checked above");
    let network = object.entry("network").or_insert_with(|| json!({}));
    let network = network
        .as_object_mut()
        .ok_or_else(|| "\"network\" in the sandbox settings must be an object".to_owned())?;
    let url = format!("http://{parent}");
    network.insert("allowedDomains".to_owned(), json!(allow));
    network.insert(
        "parentProxy".to_owned(),
        json!({ "http": url, "https": url }),
    );
    Ok(doc)
}

/// `srt --settings <file> -- <command...>`.
pub fn argv(settings: &Path, command: &[OsString]) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["--settings".into(), settings.into(), "--".into()];
    args.extend(command.iter().cloned());
    args
}

/// A private directory for the generated settings, removed on drop.
pub struct SettingsDir {
    path: PathBuf,
}

impl SettingsDir {
    /// Write `settings.json` into a fresh directory only this user can enter.
    pub fn create(doc: &Value) -> std::io::Result<(Self, PathBuf)> {
        let path = std::env::temp_dir().join(format!(
            "skimasque-sandbox-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        }
        let dir = Self { path };
        let file = dir.path.join("settings.json");
        std::fs::write(&file, serde_json::to_vec_pretty(doc).expect("serializable"))?;
        Ok((dir, file))
    }
}

impl Drop for SettingsDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The address of this host on a non-loopback interface, found without sending
/// anything: connecting a UDP socket only selects a route.
pub fn outside_address() -> std::io::Result<std::net::IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("192.0.2.1:9")?;
    let ip = socket.local_addr()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        return Err(std::io::Error::other("no non-loopback address"));
    }
    Ok(ip)
}

/// Bind a listener on a non-loopback address for the enforcement check to
/// aim at. Hold it for the duration of the probe.
pub fn probe_target() -> std::io::Result<(TcpListener, SocketAddr)> {
    let ip = outside_address()?;
    let listener = TcpListener::bind((ip, 0))?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

/// What the probe command does: try a direct connection and say what happened.
/// Run *inside* the sandbox, where the answer should be [`PROBE_BLOCKED`].
pub fn probe_connect(addr: SocketAddr) -> &'static str {
    match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
        Ok(_) => PROBE_ESCAPED,
        Err(_) => PROBE_BLOCKED,
    }
}

/// Judge the probe's output. Only an explicit "blocked" counts as enforcement:
/// a probe that could not run, or printed anything else, proves nothing.
pub fn judge_probe(stdout: &str, success: bool) -> Result<(), String> {
    match (stdout.trim(), success) {
        (PROBE_BLOCKED, true) => Ok(()),
        (PROBE_ESCAPED, _) => Err(
            "the sandbox did not stop a direct connection to this host's own network address, \
             so it is not confining the command. Refusing to start the agent"
                .to_owned(),
        ),
        _ => Err(
            "could not confirm that the sandbox blocks direct connections (the check did not \
             run to completion inside it). Refusing to start the agent"
                .to_owned(),
        ),
    }
}

/// Where `srt` is, resolving a bare name through `PATH` the way a shell does
/// (on Windows an npm-installed `srt` is `srt.cmd`).
pub fn locate(program: &OsStr, path: Option<&OsStr>, pathext: Option<&OsStr>) -> Option<PathBuf> {
    if Path::new(program).components().count() > 1 {
        return Some(PathBuf::from(program)).filter(|p| p.is_file());
    }
    if let Some(found) = crate::exec::resolve_program(program, path, pathext, |p| p.is_file()) {
        return Some(found);
    }
    std::env::split_paths(path?)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parent() -> SocketAddr {
        "127.0.0.1:41234".parse().unwrap()
    }

    #[test]
    fn broad_domains_are_refused_with_the_reason() {
        for bad in ["*", "*.com", "", "  ", "https://a.dev", "a.dev/x", "a b"] {
            assert!(check_allow_domain(bad).is_err(), "{bad:?}");
        }
        assert!(check_allow_domain("*").unwrap_err().contains("too broad"));
        for good in ["api.acme.dev", "*.acme.dev", "api.acme.dev:443", " github.com "] {
            assert!(check_allow_domain(good).is_ok(), "{good:?}");
        }
        assert_eq!(check_allow_domain(" github.com ").unwrap(), "github.com");
    }

    #[test]
    fn settings_point_both_schemes_at_skimasque_and_keep_the_rest() {
        let base = json!({
            "filesystem": { "denyRead": ["~/.ssh"], "allowWrite": ["."] },
            "network": {
                "deniedDomains": ["evil.example"],
                "parentProxy": { "http": "http://corp:3128", "noProxy": "*.acme.dev" }
            }
        });
        let doc = settings(Some(base), &["api.acme.dev".into()], parent()).unwrap();
        assert_eq!(doc["filesystem"]["denyRead"], json!(["~/.ssh"]), "kept");
        assert_eq!(doc["network"]["deniedDomains"], json!(["evil.example"]), "kept");
        assert_eq!(doc["network"]["allowedDomains"], json!(["api.acme.dev"]));
        assert_eq!(
            doc["network"]["parentProxy"],
            json!({ "http": "http://127.0.0.1:41234", "https": "http://127.0.0.1:41234" }),
            "the user's parent proxy and its noProxy are replaced wholesale"
        );
    }

    #[test]
    fn settings_need_domains_and_an_object() {
        assert!(settings(None, &[], parent()).unwrap_err().contains("--allow-domain"));
        assert!(settings(Some(json!([1])), &["a.dev".into()], parent()).is_err());
        assert!(settings(Some(json!({"network": 3})), &["a.dev".into()], parent()).is_err());
        let doc = settings(None, &["a.dev".into()], parent()).unwrap();
        assert_eq!(doc["network"]["allowedDomains"], json!(["a.dev"]));
    }

    #[test]
    fn the_command_follows_a_double_dash() {
        let args = argv(Path::new("/tmp/s.json"), &["curl".into(), "-sS".into(), "--".into()]);
        assert_eq!(args, ["--settings", "/tmp/s.json", "--", "curl", "-sS", "--"]);
    }

    #[test]
    fn only_an_explicit_blocked_counts_as_enforcement() {
        assert!(judge_probe("blocked\n", true).is_ok());
        assert!(judge_probe("connected", true).unwrap_err().contains("not confining"));
        assert!(judge_probe("connected", false).is_err());
        assert!(judge_probe("", true).unwrap_err().contains("could not confirm"));
        assert!(judge_probe("blocked", false).is_err(), "a crashing probe proves nothing");
    }

    #[test]
    fn the_probe_sees_an_open_port_and_a_closed_one() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let open = listener.local_addr().unwrap();
        assert_eq!(probe_connect(open), PROBE_ESCAPED);
        drop(listener);
        assert_eq!(probe_connect(open), PROBE_BLOCKED);
    }

    #[test]
    fn settings_files_are_private_and_cleaned_up() {
        let (dir, file) = SettingsDir::create(&json!({"a": 1})).unwrap();
        assert!(file.is_file());
        let root = file.parent().unwrap().to_owned();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o700);
        }
        drop(dir);
        assert!(!root.exists());
    }

    #[test]
    fn scrubbed_names_cover_the_proxy_and_credential_variables() {
        for name in ["HTTPS_PROXY", "no_proxy", "SKIMASQUE_TOKEN", "SKIMASQUE_TOKEN_FILE"] {
            assert!(SCRUBBED_ENV.contains(&name), "{name}");
        }
    }
}
