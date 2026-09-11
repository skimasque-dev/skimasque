//! The `skimasque policy` subcommands.
//!
//! This is the CI-facing half of the policy engine: load `.masque/policies/`,
//! then `check` a request against it, `test` its embedded assertions,
//! `validate` that it parses, `explain` a decision in full, or `diff` two
//! revisions. The engine itself ([`skimasque_policy`]) has no filesystem and no
//! opinions about output; both live here.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use skimasque_policy::{
    suggest_policy, Decision, Destination, Observation, Policy, PolicySet, RequestContext, Transport,
    WorkloadIdentity,
};

/// A policy set together with the files it came from, so errors and `diff` can
/// name a source.
#[derive(Debug)]
pub struct Loaded {
    pub set: PolicySet,
    pub sources: Vec<PathBuf>,
}

/// Where an enforcing gateway reads its policy from, kept so the gateway can
/// re-read it while running.
///
/// A gateway is started with either `--policy-dir` (re-scanned each time, so a
/// file added or removed is picked up) or an explicit `--policy-file` list
/// (re-read as given). Hot-reload has to reproduce whichever it was.
#[derive(Debug, Clone)]
pub enum SourceSpec {
    /// Every policy file directly under this directory, in filename order.
    Dir(PathBuf),
    /// This exact list of files.
    Files(Vec<PathBuf>),
}

/// A cheap value that changes whenever a set of files' contents might have: each
/// path with its length and modification time. Comparing two of these is how a
/// reloader (policy, TLS material) decides whether a re-read is worth doing.
///
/// A missing or unreadable file contributes `None` metadata rather than an
/// error, so a file that briefly disappears mid-write still registers as a
/// change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint(Vec<(PathBuf, Option<std::time::SystemTime>, Option<u64>)>);

impl Fingerprint {
    /// Fingerprint an explicit list of files, in a stable order.
    pub fn of<I>(paths: I) -> Self
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let mut paths: Vec<PathBuf> = paths.into_iter().collect();
        paths.sort();
        Self(
            paths
                .into_iter()
                .map(|path| {
                    let meta = std::fs::metadata(&path).ok();
                    let mtime = meta.as_ref().and_then(|m| m.modified().ok());
                    let len = meta.as_ref().map(|m| m.len());
                    (path, mtime, len)
                })
                .collect(),
        )
    }
}

impl SourceSpec {
    /// From the two mutually exclusive CLI inputs. `None` when neither is set.
    pub fn from_args(policy_dir: Option<&Path>, policy_files: &[PathBuf]) -> Option<Self> {
        match (policy_dir, policy_files) {
            (Some(dir), _) => Some(Self::Dir(dir.to_path_buf())),
            (None, files) if !files.is_empty() => Some(Self::Files(files.to_vec())),
            (None, _) => None,
        }
    }

    /// A one-line description for logs.
    pub fn describe(&self) -> String {
        match self {
            Self::Dir(dir) => dir.display().to_string(),
            Self::Files(files) => files
                .iter()
                .map(|f| f.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    /// Re-read the source into a fresh [`Loaded`].
    pub fn load(&self) -> anyhow::Result<Loaded> {
        match self {
            Self::Dir(dir) => load_dir(dir),
            Self::Files(files) => load_files(files),
        }
    }

    /// The current [`Fingerprint`] of the source's files.
    pub fn fingerprint(&self) -> Fingerprint {
        let paths: Vec<PathBuf> = match self {
            Self::Dir(dir) => std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| is_policy_file(path))
                .collect(),
            Self::Files(files) => files.clone(),
        };
        Fingerprint::of(paths)
    }
}

/// A policy file: `.toml`, `.yaml` or `.yml`.
pub fn is_policy_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "toml" | "yaml" | "yml"))
}

/// Load every policy file directly under `dir`, in filename order.
pub fn load_dir(dir: &Path) -> anyhow::Result<Loaded> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading the policy directory {}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_policy_file(path))
        .collect();
    files.sort();
    if files.is_empty() {
        bail!("no .toml/.yaml policy files in {}", dir.display());
    }
    load_files(&files)
}

/// Load an explicit list of policy files.
pub fn load_files(paths: &[PathBuf]) -> anyhow::Result<Loaded> {
    let documents = paths
        .iter()
        .map(|path| {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            Ok((display_name(path), text))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let set = PolicySet::from_documents(documents).context("loading the policy set")?;
    Ok(Loaded {
        set,
        sources: paths.to_vec(),
    })
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Emit a `tracing` warning for every lint in `set`. The gateway calls this
/// when it loads or hot-reloads a policy set, so an over-broad `[match]` is
/// visible in the logs even though it does not stop the gateway from enforcing.
pub fn log_lints(set: &PolicySet) {
    for lint in set.lints() {
        tracing::warn!(
            target: "masque::policy",
            policy = %lint.policy,
            code = %lint.code,
            "{}",
            lint.message
        );
    }
}

/// The identity a `check` / `explain` invocation is testing, from its flags.
#[derive(Debug, Default, Clone)]
pub struct IdentityArgs {
    pub organization: Option<String>,
    pub repository: Option<String>,
    pub workflow: Option<String>,
    pub git_ref: Option<String>,
    pub branch: Option<String>,
    pub environment: Option<String>,
    pub actor: Option<String>,
}

impl IdentityArgs {
    pub fn into_identity(self) -> WorkloadIdentity {
        let git_ref = self
            .git_ref
            .or_else(|| self.branch.as_ref().map(|b| format!("refs/heads/{b}")));
        WorkloadIdentity {
            organization: self.organization,
            repository: self.repository,
            workflow: self.workflow,
            git_ref,
            environment: self.environment,
            actor: self.actor,
        }
    }
}

/// Evaluate one request. When `policy_name` is given, that policy is used
/// directly (as `masque policy check production ...` does); otherwise the set
/// selects one from the identity.
pub fn evaluate(
    loaded: &Loaded,
    policy_name: Option<&str>,
    application: &str,
    transport: Transport,
    destination: &str,
    identity: WorkloadIdentity,
) -> anyhow::Result<Decision> {
    let destination = Destination::parse(destination)
        .with_context(|| format!("parsing the destination {destination:?}"))?;
    let ctx = RequestContext {
        workload: identity,
        application: application.to_owned(),
        transport,
        destination,
    };

    match policy_name {
        Some(name) => loaded
            .set
            .evaluate_named(name, &ctx)
            .with_context(|| format!("no policy named {name:?} in the set")),
        None => Ok(loaded.set.evaluate(&ctx)),
    }
}

/// Run every policy's `[[tests]]`. Returns `true` when all passed; the report
/// is written to `out`.
pub fn run_tests(loaded: &Loaded, out: &mut String) -> bool {
    let mut all_passed = true;
    let mut total = 0usize;

    for policy in loaded.set.policies() {
        let outcomes = policy.run_tests();
        if outcomes.is_empty() {
            continue;
        }
        let _ = writeln!(out, "{}", policy.name);
        for outcome in outcomes {
            total += 1;
            let mark = if outcome.passed { "ok  " } else { "FAIL" };
            let _ = writeln!(
                out,
                "  {mark} {} -> {}  (expect {})",
                outcome.test.application, outcome.test.destination, outcome.test.expect
            );
            if let Some(detail) = &outcome.detail {
                let _ = writeln!(out, "       {detail}");
            }
            all_passed &= outcome.passed;
        }
    }

    if total == 0 {
        let _ = writeln!(out, "no [[tests]] in any policy");
    } else {
        let _ = writeln!(out, "\n{total} assertions, {}", if all_passed { "all passed" } else { "FAILURES" });
    }
    all_passed
}

/// Parse observations from a file: either a JSON array of objects, or one JSON
/// object per line (JSONL), which is what a streaming collector would write.
pub fn parse_observations(text: &str) -> anyhow::Result<Vec<Observation>> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('[') {
        return serde_json::from_str(trimmed).context("parsing the observations as a JSON array");
    }
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(n, line)| {
            serde_json::from_str(line).with_context(|| format!("parsing observation on line {}", n + 1))
        })
        .collect()
}

/// Generate a policy draft from an observations file, returning the rendered
/// TOML with any skipped observations noted as comments at the top.
pub fn learn(
    observations_path: &Path,
    name: &str,
    identity: WorkloadIdentity,
) -> anyhow::Result<String> {
    let text = std::fs::read_to_string(observations_path)
        .with_context(|| format!("reading {}", observations_path.display()))?;
    let observations = parse_observations(&text)?;
    if observations.is_empty() {
        bail!("no observations in {}", observations_path.display());
    }

    let suggestion = suggest_policy(name, &identity, &observations);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# generated by `skimasque policy learn` from {} observation(s)",
        observations.len()
    );
    let _ = writeln!(out, "# review before committing: an observed destination is not a trusted one");
    for (observation, why) in &suggestion.skipped {
        let _ = writeln!(out, "# skipped {:?}: {why}", observation.destination);
    }
    let _ = writeln!(out);
    out.push_str(&suggestion.policy.to_toml());
    Ok(out)
}

/// Parse every source, report the outcome, and list any lints (see
/// [`skimasque_policy::Policy::lints`]). Returns `true` when every file parsed
/// and -- when `strict` -- when nothing was linted.
pub fn validate(paths: &[PathBuf], strict: bool, out: &mut String) -> bool {
    let mut ok = true;
    let mut linted = false;
    for path in paths {
        match std::fs::read_to_string(path).map_err(anyhow::Error::from).and_then(|text| {
            Policy::from_named_document(&display_name(path), &text).map_err(anyhow::Error::from)
        }) {
            Ok(policy) => {
                let _ = writeln!(out, "ok    {} ({})", display_name(path), policy.name);
                for lint in policy.lints() {
                    linted = true;
                    let _ = writeln!(
                        out,
                        "warn  {} [{}]: {}",
                        display_name(path),
                        lint.code,
                        lint.message
                    );
                }
            }
            Err(error) => {
                ok = false;
                let _ = writeln!(out, "ERROR {}: {error:#}", display_name(path));
            }
        }
    }
    if linted && strict {
        let _ = writeln!(out, "\n--strict: treating the warnings above as failures");
        ok = false;
    }
    ok
}

/// A textual diff of two policy sets, by policy name and then by rule.
pub fn diff(old: &PolicySet, new: &PolicySet, out: &mut String) {
    let names = {
        let mut names: Vec<&str> = old
            .policies()
            .iter()
            .chain(new.policies())
            .map(|p| p.name.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    };

    for name in names {
        match (old.get(name), new.get(name)) {
            (None, Some(_)) => {
                let _ = writeln!(out, "+ policy {name}");
            }
            (Some(_), None) => {
                let _ = writeln!(out, "- policy {name}");
            }
            (Some(before), Some(after)) => {
                let before_rules = rule_lines(before);
                let after_rules = rule_lines(after);
                for rule in &after_rules {
                    if !before_rules.contains(rule) {
                        let _ = writeln!(out, "  {name}: + {rule}");
                    }
                }
                for rule in &before_rules {
                    if !after_rules.contains(rule) {
                        let _ = writeln!(out, "  {name}: - {rule}");
                    }
                }
                if before.limits != after.limits || before.session != after.session {
                    let _ = writeln!(out, "  {name}: ~ limits or session changed");
                }
            }
            (None, None) => unreachable!("name came from one of the two sets"),
        }
    }
}

fn rule_lines(policy: &Policy) -> Vec<String> {
    policy
        .rules
        .iter()
        .flat_map(|rule| {
            let action = rule.action;
            let app = match &rule.application {
                skimasque_policy::AppPattern::Any => "*".to_owned(),
                skimasque_policy::AppPattern::Name(name) => name.clone(),
            };
            let transport = rule
                .transport
                .as_word()
                .map(|w| format!("{w} "))
                .unwrap_or_default();
            rule.destinations
                .iter()
                .map(move |dest| format!("{action} {app} {transport}{dest}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROD: &str = r#"
        name = "production"
        [match]
        repository = "acme/widget"
        branch = "main"

        [[rules]]
        id = "tf"
        application = "terraform"
        action = "allow"
        destinations = ["api.production.example.com:443"]

        [[tests]]
        application = "terraform"
        destination = "api.production.example.com:443"
        expect = "allow"

        [[tests]]
        application = "terraform"
        destination = "evil.example.com:443"
        expect = "deny"
    "#;

    fn loaded() -> Loaded {
        Loaded {
            set: PolicySet::from_documents([("production.toml", PROD)]).unwrap(),
            sources: vec![PathBuf::from("production.toml")],
        }
    }

    #[test]
    fn evaluate_uses_the_named_policy_directly() {
        let decision = evaluate(
            &loaded(),
            Some("production"),
            "terraform",
            Transport::Tcp,
            "api.production.example.com:443",
            WorkloadIdentity::default(),
        )
        .unwrap();
        assert!(decision.is_allow());
    }

    #[test]
    fn evaluate_by_identity_denies_a_non_matching_workload() {
        // No policy matches an empty identity, since production requires a repo.
        let decision = evaluate(
            &loaded(),
            None,
            "terraform",
            Transport::Tcp,
            "api.production.example.com:443",
            WorkloadIdentity::default(),
        )
        .unwrap();
        assert!(decision.is_deny());
    }

    #[test]
    fn run_tests_reports_the_embedded_assertions() {
        let mut out = String::new();
        assert!(run_tests(&loaded(), &mut out));
        assert!(out.contains("2 assertions, all passed"), "{out}");
    }

    #[test]
    fn diff_shows_added_and_removed_rules() {
        let old = PolicySet::from_documents([("p.toml", PROD)]).unwrap();
        let new = PolicySet::from_documents([(
            "p.toml",
            r#"
            name = "production"
            [[rules]]
            application = "terraform"
            action = "allow"
            destinations = ["api.production.example.com:443", "db.production.example.com:5432"]
            "#,
        )])
        .unwrap();

        let mut out = String::new();
        diff(&old, &new, &mut out);
        assert!(out.contains("+ allow terraform db.production.example.com:5432"), "{out}");
    }

    #[test]
    fn the_branch_flag_becomes_a_head_ref() {
        let id = IdentityArgs {
            branch: Some("main".into()),
            ..Default::default()
        }
        .into_identity();
        assert_eq!(id.git_ref.as_deref(), Some("refs/heads/main"));
    }

    #[test]
    fn a_source_spec_needs_a_dir_or_a_file() {
        assert!(SourceSpec::from_args(None, &[]).is_none());
        assert!(matches!(
            SourceSpec::from_args(Some(Path::new("x")), &[]),
            Some(SourceSpec::Dir(_))
        ));
        assert!(matches!(
            SourceSpec::from_args(None, &[PathBuf::from("p.toml")]),
            Some(SourceSpec::Files(_))
        ));
    }

    #[test]
    fn a_dir_fingerprint_moves_when_a_policy_file_changes_or_appears() {
        let dir = std::env::temp_dir().join(format!("skimasque-fp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.toml"), "name = \"a\"\n").unwrap();

        let spec = SourceSpec::Dir(dir.clone());
        let first = spec.fingerprint();
        assert_eq!(first, spec.fingerprint(), "a quiet source fingerprints the same");

        // A non-policy file does not count.
        std::fs::write(dir.join("notes.txt"), "ignore me").unwrap();
        assert_eq!(first, spec.fingerprint());

        // A new policy file does.
        std::fs::write(dir.join("b.toml"), "name = \"b\"\n").unwrap();
        let second = spec.fingerprint();
        assert_ne!(first, second);

        // So does a change in length to an existing one.
        std::fs::write(dir.join("a.toml"), "name = \"a\"\n# grew\n").unwrap();
        assert_ne!(second, spec.fingerprint());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fingerprint_of_a_file_list_tracks_content_and_order_insensitively() {
        let dir = std::env::temp_dir().join(format!("skimasque-fp-of-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::write(&a, "one").unwrap();
        std::fs::write(&b, "two").unwrap();

        let forward = Fingerprint::of([a.clone(), b.clone()]);
        let reversed = Fingerprint::of([b.clone(), a.clone()]);
        assert_eq!(forward, reversed, "order does not matter");

        std::fs::write(&a, "one!").unwrap();
        assert_ne!(forward, Fingerprint::of([a.clone(), b.clone()]));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_spec_reloads_the_same_list() {
        let dir = std::env::temp_dir().join(format!("skimasque-fp-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prod.toml");
        std::fs::write(&path, "name = \"prod\"\n").unwrap();

        let spec = SourceSpec::Files(vec![path.clone()]);
        assert_eq!(spec.load().unwrap().set.policies().len(), 1);
        assert_eq!(spec.describe(), path.display().to_string());

        std::fs::remove_dir_all(&dir).ok();
    }
}
