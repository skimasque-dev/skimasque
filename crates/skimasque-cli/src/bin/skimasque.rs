//! `skimasque` -- the command-line interface.
//!
//! One binary, organised around this command surface:
//!
//! | Command | State |
//! |---|---|
//! | `skimasque policy`  | the policy toolkit: `check` / `explain` / `test` / `validate` / `diff` / `learn` |
//! | `skimasque why`     | explain whether an identity may reach a destination -- local policy set, or `--control-plane` for the org's published policy |
//! | `skimasque gateway` | run the MASQUE gateway -- delegates to `skimasque-server` |
//! | `skimasque connect` | open a tunnel to a destination -- delegates to `skimasque-client` |
//! | `skimasque init`    | scaffold `.masque/policies/` for a new project |
//! | `skimasque status`  | the local policy set, plus the control-plane fleet when signed in |
//! | `skimasque login` / `logout` / `whoami` | sign in to a control plane via GitHub (device flow) |
//! | `skimasque org`     | `create` / `list` / `members` / `add-member` on the control plane |
//! | `skimasque audit`   | recent policy decisions across the fleet, from the control plane |
//! | `skimasque gateway register` | enrol a gateway: mint a token, print the `skimasque-server` command |
//!
//! `login` runs the GitHub OAuth device flow against a control plane —
//! SkiMasque Cloud (`https://control.skimasque.com`) by default, or the
//! `--control-plane` / `$SKIMASQUE_CONTROL_PLANE` you set — and stores a
//! session in a local credential file ([`account`](skimasque_cli::account)).
//! `org`, `audit`, `gateway register`, `status`, and `why --control-plane` use
//! that session. Everything else works with no sign-in.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};

use anyhow::{bail, Context};
use clap::{Args, Parser, Subcommand};
use skimasque_cli::policy::{self, IdentityArgs, Loaded};
use skimasque_core::connect_udp::Target;
use skimasque_policy::{Decision, Transport, WorkloadIdentity};

/// SkiMasque Cloud — the default control plane. `--control-plane` /
/// `$SKIMASQUE_CONTROL_PLANE` select another (a self-hosted one, or a staging
/// instance).
const DEFAULT_CONTROL_PLANE: &str = "https://control.skimasque.com";

#[derive(Debug, Parser)]
#[command(
    name = "skimasque",
    version,
    about = "Identity-aware, least-privilege network access for CI/CD jobs and developers",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Scaffold `.masque/policies/` for a new project.
    Init {
        /// Where the policy directory should live.
        #[arg(long, value_name = "DIR", default_value = ".masque/policies")]
        policy_dir: PathBuf,
    },

    /// Sign in to a control plane via GitHub (device flow).
    Login {
        /// The control plane URL. Defaults to SkiMasque Cloud
        /// (`https://control.skimasque.com`); pass a self-hosted one here or in
        /// `$SKIMASQUE_CONTROL_PLANE`.
        #[arg(long, value_name = "URL", env = "SKIMASQUE_CONTROL_PLANE")]
        control_plane: Option<String>,
    },

    /// Sign out: revoke the session and delete the local credential file.
    Logout,

    /// Show the signed-in developer and which control plane.
    Whoami,

    /// Manage organisations on the control plane (needs `skimasque login`).
    Org {
        #[command(subcommand)]
        command: OrgCommand,
    },

    /// Show recent policy decisions from the fleet (needs `skimasque login`).
    Audit {
        /// The organisation. Inferred if you belong to exactly one.
        #[arg(long, value_name = "ORG_ID")]
        org: Option<String>,
        /// Only this gateway's events.
        #[arg(long, value_name = "GATEWAY_ID")]
        gateway: Option<String>,
        /// Only `allow` or only `deny`.
        #[arg(long, value_name = "allow|deny")]
        decision: Option<String>,
        /// Only events at or after this RFC 3339 time.
        #[arg(long, value_name = "TIMESTAMP")]
        since: Option<String>,
        /// How many to show (newest first). Any size — the client pages the
        /// control plane to gather them.
        #[arg(long, default_value_t = 50, value_name = "N")]
        limit: usize,
    },

    /// Run a MASQUE gateway, or `skimasque gateway register` to enrol one with a
    /// control plane.
    ///
    /// Anything other than `register` is passed straight to `skimasque-server`,
    /// including `--help`.
    #[command(disable_help_flag = true)]
    Gateway {
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "ARGS"
        )]
        args: Vec<OsString>,
    },

    /// Open a tunnel to a destination through a gateway.
    ///
    /// Bridges the tunnel to stdin/stdout, so it works as an SSH `ProxyCommand`
    /// or in a shell pipeline. Arguments after the destination go to
    /// `skimasque-client`, which needs at least `--proxy <gateway address>`
    /// (run `skimasque-client --help` for the full set).
    Connect {
        /// The destination, as `host:port`.
        destination: String,
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "ARGS"
        )]
        args: Vec<OsString>,
    },

    /// Work with network-access policies.
    #[command(subcommand)]
    Policy(PolicyCommand),

    /// Explain whether an identity may reach a destination, and why.
    ///
    /// Selects the policy whose match accepts the identity, evaluates the
    /// request against it, and prints the decision -- the local equivalent of
    /// what a gateway would log for the same tunnel. With `--control-plane` the
    /// evaluation runs on the server against the org's published policy (or
    /// `--revision` / `--draft`), through the same engine the gateway uses.
    Why {
        /// The destination, as `host:port`.
        destination: String,
        #[command(flatten)]
        request: RequestArgs,
        /// Evaluate against your org's published policy on the control plane
        /// instead of local files (needs `skimasque login`).
        #[arg(long)]
        control_plane: bool,
        /// The organisation, if you belong to more than one.
        #[arg(long, value_name = "ID", requires = "control_plane")]
        org: Option<String>,
        /// Simulate against this revision instead of the current one.
        #[arg(long, value_name = "N", requires = "control_plane")]
        revision: Option<u64>,
        /// Simulate against the unpublished draft.
        #[arg(long, requires = "control_plane", conflicts_with = "revision")]
        draft: bool,
        /// Scope to the documents this gateway (id or name) receives.
        #[arg(long, value_name = "GATEWAY", requires = "control_plane")]
        gateway: Option<String>,
        #[command(flatten)]
        source: SourceArgs,
    },

    /// Summarise the local policy set: what loads, and whether its tests pass.
    Status {
        #[command(flatten)]
        source: SourceArgs,
    },
}

#[derive(Debug, Subcommand)]
enum OrgCommand {
    /// Create an organisation. You become its owner.
    Create {
        /// A display name.
        name: String,
    },
    /// List the organisations you belong to.
    List,
    /// List an organisation's members.
    Members {
        /// The organisation. Inferred if you belong to exactly one.
        #[arg(long, value_name = "ORG_ID")]
        org: Option<String>,
    },
    /// Add a member by their GitHub login. They must have signed in once.
    ///
    /// You must be an owner of the organisation.
    AddMember {
        /// The GitHub login to add.
        github_login: String,
        /// The organisation. Inferred if you belong to exactly one.
        #[arg(long, value_name = "ORG_ID")]
        org: Option<String>,
    },
    /// Promote a member to owner, or demote an owner to member.
    ///
    /// You must be an owner. An organisation can never be left with no owner.
    SetRole {
        /// The member's GitHub login.
        github_login: String,
        /// The role to give them.
        #[arg(value_parser = ["owner", "member"])]
        role: String,
        /// The organisation. Inferred if you belong to exactly one.
        #[arg(long, value_name = "ORG_ID")]
        org: Option<String>,
    },
    /// Remove a member from the organisation.
    ///
    /// You must be an owner. The last owner cannot be removed.
    RemoveMember {
        /// The member's GitHub login.
        github_login: String,
        /// The organisation. Inferred if you belong to exactly one.
        #[arg(long, value_name = "ORG_ID")]
        org: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum PolicyCommand {
    /// Decide whether a request would be allowed, and print ALLOW or DENY.
    ///
    /// Exit status is 0 for ALLOW and 1 for DENY, so it composes in a script.
    Check {
        /// The policy to evaluate against.
        policy: String,
        /// The destination, as `host:port`.
        destination: String,
        #[command(flatten)]
        request: RequestArgs,
        #[command(flatten)]
        source: SourceArgs,
    },
    /// Like `check`, but always exits 0 and prints the full reasoning.
    Explain {
        policy: String,
        destination: String,
        #[command(flatten)]
        request: RequestArgs,
        #[command(flatten)]
        source: SourceArgs,
    },
    /// Run every policy's `[[tests]]` assertions.
    Test {
        #[command(flatten)]
        source: SourceArgs,
    },
    /// Check that every policy file parses, and warn about matches that are
    /// probably too broad (an empty `[match]`, no `repository`, no ref).
    Validate {
        /// Exit non-zero if any policy is linted, not only if one fails to
        /// parse. For a CI gate.
        #[arg(long)]
        strict: bool,
        #[command(flatten)]
        source: SourceArgs,
    },
    /// Show what changed between two policy directories or files.
    Diff {
        /// The old revision: a directory of policy files, or a single file.
        old: PathBuf,
        /// The new revision.
        new: PathBuf,
    },
    /// Generate a policy draft from observed traffic (the "generate" step of
    /// learning mode).
    Learn {
        /// Observations file: a JSON array, or one JSON object per line.
        /// Each object is `{"application": "...", "destination": "host:port"}`.
        observations: PathBuf,
        /// Name for the generated policy.
        #[arg(long, default_value = "learned")]
        name: String,
        #[command(flatten)]
        identity: IdentityFlags,
    },
}

/// The identity fields shared by `check`, `explain`, `why` and `learn`.
#[derive(Debug, Args)]
struct IdentityFlags {
    #[arg(long, value_name = "ORG")]
    organization: Option<String>,
    #[arg(long, value_name = "OWNER/NAME")]
    repository: Option<String>,
    #[arg(long, value_name = "FILE")]
    workflow: Option<String>,
    /// The full git ref, e.g. `refs/heads/main`.
    #[arg(long = "ref", value_name = "REF")]
    git_ref: Option<String>,
    /// Shorthand for `--ref refs/heads/<BRANCH>`.
    #[arg(long, value_name = "BRANCH", conflicts_with = "git_ref")]
    branch: Option<String>,
    #[arg(long, value_name = "NAME")]
    environment: Option<String>,
    #[arg(long, value_name = "NAME")]
    actor: Option<String>,
}

impl IdentityFlags {
    fn to_args(&self) -> IdentityArgs {
        IdentityArgs {
            organization: self.organization.clone(),
            repository: self.repository.clone(),
            workflow: self.workflow.clone(),
            git_ref: self.git_ref.clone(),
            branch: self.branch.clone(),
            environment: self.environment.clone(),
            actor: self.actor.clone(),
        }
    }
}

#[derive(Debug, Args)]
struct RequestArgs {
    /// The application being run, e.g. `terraform`. Session context only: the
    /// engine matches on it, but it is not an authenticated fact. Empty matches
    /// only `application = "*"` rules.
    #[arg(long, value_name = "NAME", default_value = "")]
    app: String,

    /// The tunnel transport: `tcp` (the default) or `udp`.
    #[arg(long, value_name = "TCP|UDP", default_value = "tcp", value_parser = parse_transport)]
    transport: Transport,

    #[command(flatten)]
    identity: IdentityFlags,
}

fn parse_transport(value: &str) -> Result<Transport, String> {
    match value.to_ascii_lowercase().as_str() {
        "tcp" => Ok(Transport::Tcp),
        "udp" => Ok(Transport::Udp),
        other => Err(format!("expected tcp or udp, got {other:?}")),
    }
}

impl RequestArgs {
    fn identity(&self) -> IdentityArgs {
        self.identity.to_args()
    }
}

#[derive(Debug, Args)]
struct SourceArgs {
    /// Directory of `*.toml` policy files.
    #[arg(long, value_name = "DIR", default_value = ".masque/policies")]
    policy_dir: PathBuf,
    /// Load these files instead of scanning `--policy-dir`. Repeatable.
    #[arg(long = "policy-file", value_name = "PATH")]
    policy_files: Vec<PathBuf>,
}

impl SourceArgs {
    fn load(&self) -> anyhow::Result<Loaded> {
        if self.policy_files.is_empty() {
            policy::load_dir(&self.policy_dir)
        } else {
            policy::load_files(&self.policy_files)
        }
    }

    fn paths(&self) -> anyhow::Result<Vec<PathBuf>> {
        if !self.policy_files.is_empty() {
            return Ok(self.policy_files.clone());
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.policy_dir)
            .with_context(|| format!("reading {}", self.policy_dir.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| policy::is_policy_file(path))
            .collect();
        files.sort();
        Ok(files)
    }

    fn describe(&self) -> String {
        if self.policy_files.is_empty() {
            self.policy_dir.display().to_string()
        } else {
            self.policy_files
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { policy_dir } => run_init(&policy_dir),
        Command::Login { control_plane } => run_login(control_plane),
        Command::Logout => run_logout(),
        Command::Whoami => run_whoami(),
        Command::Org { command } => run_org(command),
        Command::Audit {
            org,
            gateway,
            decision,
            since,
            limit,
        } => run_audit(org, gateway, decision, since, limit),
        Command::Gateway { args } => {
            if args.first().is_some_and(|a| a == "register") {
                run_gateway_register(&args[1..])
            } else {
                delegate("skimasque-server", &args)
            }
        }
        Command::Connect { destination, args } => run_connect(&destination, &args),
        Command::Why {
            destination,
            request,
            control_plane,
            org,
            revision,
            draft,
            gateway,
            source,
        } => {
            if control_plane {
                run_why_remote(&destination, &request, org, revision, draft, gateway)
            } else {
                run_why(&destination, &request, &source)
            }
        }
        Command::Status { source } => run_status(&source),
        Command::Policy(command) => run_policy(command),
    }
}

fn run_policy(command: PolicyCommand) -> anyhow::Result<ExitCode> {
    match command {
        PolicyCommand::Check {
            policy: name,
            destination,
            request,
            source,
        } => {
            let loaded = source.load()?;
            let decision = policy::evaluate(
                &loaded,
                Some(&name),
                &request.app,
                request.transport,
                &destination,
                request.identity().into_identity(),
            )?;
            println!("{decision}");
            Ok(exit_for(&decision))
        }

        PolicyCommand::Explain {
            policy: name,
            destination,
            request,
            source,
        } => {
            let loaded = source.load()?;
            let decision = policy::evaluate(
                &loaded,
                Some(&name),
                &request.app,
                request.transport,
                &destination,
                request.identity().into_identity(),
            )?;
            println!("Application: {}", request.app);
            println!("Transport: {}", request.transport);
            println!("Destination: {destination}");
            println!("Policy: {name}\n");
            println!("{decision}");
            Ok(ExitCode::SUCCESS)
        }

        PolicyCommand::Test { source } => {
            let loaded = source.load()?;
            let mut out = String::new();
            let passed = policy::run_tests(&loaded, &mut out);
            print!("{out}");
            Ok(if passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }

        PolicyCommand::Validate { strict, source } => {
            let paths = source.paths()?;
            if paths.is_empty() {
                bail!("no policy files found");
            }
            let mut out = String::new();
            let ok = policy::validate(&paths, strict, &mut out);
            print!("{out}");
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }

        PolicyCommand::Learn {
            observations,
            name,
            identity,
        } => {
            let draft = policy::learn(&observations, &name, identity.to_args().into_identity())?;
            print!("{draft}");
            Ok(ExitCode::SUCCESS)
        }

        PolicyCommand::Diff { old, new } => {
            let old = load_revision(&old)?;
            let new = load_revision(&new)?;
            let mut out = String::new();
            policy::diff(&old.set, &new.set, &mut out);
            if out.is_empty() {
                println!("no changes");
            } else {
                print!("{out}");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// The starter policy `skimasque init` writes.
const EXAMPLE_POLICY: &str = r#"# A starter policy. Edit it, then `skimasque policy test` and
# `skimasque gateway --policy-dir .masque/policies`.
#
# `[match]` decides which workloads this policy governs -- every field is a
# constraint, and an empty match governs everyone. `[[rules]]` are evaluated in
# order; the absence of an `allow` is a deny.

name = "example"

[match]
repository = "acme/widget"
branch = "main"

[[rules]]
id = "terraform-api"
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

fn run_init(policy_dir: &Path) -> anyhow::Result<ExitCode> {
    std::fs::create_dir_all(policy_dir)
        .with_context(|| format!("creating {}", policy_dir.display()))?;

    let example = policy_dir.join("example.toml");
    if example.exists() {
        println!("{} already exists; leaving it as it is", example.display());
    } else {
        std::fs::write(&example, EXAMPLE_POLICY)
            .with_context(|| format!("writing {}", example.display()))?;
        println!("wrote {}", example.display());
    }

    let dir = policy_dir.display();
    println!();
    println!("next:");
    println!("  1. edit {} to describe who may reach what", example.display());
    println!("  2. check it:        skimasque policy test --policy-dir {dir}");
    println!("  3. run a gateway:   skimasque gateway --policy-dir {dir} \\");
    println!("                          --authority <host:port> --write-cert gateway.pem");
    println!("  4. reach a target:  skimasque connect <host:port> --proxy <gateway> \\");
    println!("                          --ca gateway.pem --auth-token <token>");
    println!();
    println!("from CI, use the setup action instead of step 4 (see docs/github-actions.md).");
    println!("guided gateway registration and `skimasque login` use a control plane");
    println!("(SkiMasque Cloud by default) -- see docs/getting-started.md.");
    Ok(ExitCode::SUCCESS)
}

fn run_login(control_plane: Option<String>) -> anyhow::Result<ExitCode> {
    use skimasque_cli::account;

    // `--control-plane` / `$SKIMASQUE_CONTROL_PLANE`, else the control plane the
    // last session was for, else the hosted default. A bare host gains https://.
    let base_url = skimasque_cli::normalize_base_url(
        &control_plane
            .or_else(|| account::load().ok().flatten().map(|creds| creds.control_plane))
            .unwrap_or_else(|| DEFAULT_CONTROL_PLANE.to_owned()),
    );

    let api = account::Api::new(&base_url)?;
    let (session_token, user) = account::block_on(api.login())?;
    account::save(&account::Credentials {
        control_plane: base_url.clone(),
        session_token,
        github_login: user.github_login.clone(),
        user_id: user.id,
    })?;
    println!("Signed in to {base_url} as {}.", user.github_login);
    Ok(ExitCode::SUCCESS)
}

fn run_logout() -> anyhow::Result<ExitCode> {
    use skimasque_cli::account;

    match account::load()? {
        Some(creds) => {
            // Best effort: revoke server-side, then always drop the local file.
            if let Ok(api) = account::Api::new(&creds.control_plane) {
                let _ = account::block_on(api.logout(&creds.session_token));
            }
            account::clear()?;
            println!("Signed out of {}.", creds.control_plane);
        }
        None => println!("Not signed in."),
    }
    Ok(ExitCode::SUCCESS)
}

fn run_whoami() -> anyhow::Result<ExitCode> {
    use skimasque_cli::account;

    let Some(creds) = account::load()? else {
        eprintln!("Not signed in -- run `skimasque login`.");
        return Ok(ExitCode::from(1));
    };
    let api = account::Api::new(&creds.control_plane)?;
    match account::block_on(api.me(&creds.session_token)) {
        Ok(user) => {
            println!("{} ({})", user.github_login, creds.control_plane);
            if let Some(email) = user.email {
                println!("  email: {email}");
            }
            println!("  user id: {}", user.id);
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            eprintln!("session for {} is not valid: {error:#}", creds.control_plane);
            eprintln!("run `skimasque login` again.");
            Ok(ExitCode::from(1))
        }
    }
}

fn run_audit(
    org: Option<String>,
    gateway: Option<String>,
    decision: Option<String>,
    since: Option<String>,
    limit: usize,
) -> anyhow::Result<ExitCode> {
    use skimasque_cli::account::{self, AuditFilter};

    let creds = account::require()?;
    let api = account::Api::new(&creds.control_plane)?;
    let org = account::block_on(account::resolve_org(&api, &creds, org))?;
    let filter = AuditFilter {
        gateway,
        decision,
        since,
        limit: Some(limit),
    };
    let events = account::block_on(api.query_audit(&creds.session_token, &org, &filter))?;
    if events.is_empty() {
        println!("no audit events match.");
        return Ok(ExitCode::SUCCESS);
    }
    for e in events {
        let field = |name: &str| {
            e.event
                .as_ref()
                .and_then(|v| v.get(name))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned()
        };
        let ts = field("timestamp");
        let decision = field("decision");
        let dest = field("destination");
        let app = field("application");
        let detail = if decision == "deny" {
            field("reason")
        } else {
            field("rule")
        };
        println!(
            "{:<24} {:<5} {:<24} {:<12} {}  ({})",
            ts, decision, dest, app, detail, e.gateway_id
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn run_org(command: OrgCommand) -> anyhow::Result<ExitCode> {
    use skimasque_cli::account;

    let creds = account::require()?;
    let api = account::Api::new(&creds.control_plane)?;
    let session = &creds.session_token;

    match command {
        OrgCommand::Create { name } => {
            let created = account::block_on(api.create_org(session, &name))?;
            println!("Created {} ({}).", created.org.name, created.org.id);
            println!();
            println!("A one-time gateway registration token (expires in ~1h):");
            println!("  {}", created.registration_token);
            println!();
            println!("Or run `skimasque gateway register --org {}` later.", created.org.id);
        }
        OrgCommand::List => {
            let orgs = account::block_on(api.list_orgs(session))?;
            if orgs.is_empty() {
                println!("You are not a member of any organisation.");
            }
            for org in orgs {
                println!("{}  {}", org.id, org.name);
            }
        }
        OrgCommand::Members { org } => {
            let org = account::block_on(account::resolve_org(&api, &creds, org))?;
            let members = account::block_on(api.list_members(session, &org))?;
            for m in members {
                let who = m.github_login.as_deref().unwrap_or(&m.user_id);
                println!("{:<20} {}", who, m.role);
            }
        }
        OrgCommand::AddMember { github_login, org } => {
            let org = account::block_on(account::resolve_org(&api, &creds, org))?;
            let member = account::block_on(api.add_member(session, &org, &github_login))?;
            println!(
                "Added {} to {org} as {}.",
                member.github_login.as_deref().unwrap_or(&github_login),
                member.role
            );
        }
        OrgCommand::SetRole {
            github_login,
            role,
            org,
        } => {
            let org = account::block_on(account::resolve_org(&api, &creds, org))?;
            let user_id = resolve_member(&api, session, &org, &github_login)?;
            let member = account::block_on(api.set_member_role(session, &org, &user_id, &role))?;
            println!(
                "{} is now {} of {org}.",
                member.github_login.as_deref().unwrap_or(&github_login),
                member.role
            );
        }
        OrgCommand::RemoveMember { github_login, org } => {
            let org = account::block_on(account::resolve_org(&api, &creds, org))?;
            let user_id = resolve_member(&api, session, &org, &github_login)?;
            account::block_on(api.remove_member(session, &org, &user_id))?;
            println!("Removed {github_login} from {org}.");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Resolve a GitHub login to the control plane's `usr_` id by scanning the
/// organisation's member list (the role/remove endpoints are keyed by id).
fn resolve_member(
    api: &skimasque_cli::account::Api,
    session: &str,
    org: &str,
    github_login: &str,
) -> anyhow::Result<String> {
    let members = skimasque_cli::account::block_on(api.list_members(session, org))?;
    members
        .into_iter()
        .find(|m| {
            m.github_login
                .as_deref()
                .is_some_and(|l| l.eq_ignore_ascii_case(github_login))
        })
        .map(|m| m.user_id)
        .ok_or_else(|| anyhow::anyhow!("{github_login} is not a member of {org}"))
}

/// `skimasque gateway register` -- mint a registration token with the stored
/// session and print (or run) the `skimasque-server --control-plane …` command.
#[derive(Debug, Parser)]
#[command(name = "skimasque gateway register", about = "Enrol a gateway with a control plane")]
struct GatewayRegisterArgs {
    /// The organisation the gateway belongs to. If omitted and you belong to
    /// exactly one, that one is used.
    #[arg(long, value_name = "ORG_ID")]
    org: Option<String>,

    /// The name the gateway shows as in the fleet view. Defaults to the hostname.
    #[arg(long, value_name = "NAME")]
    name: Option<String>,

    /// Override the control plane URL. Defaults to the one you logged in to
    /// (`https://control.skimasque.com`).
    #[arg(long, value_name = "URL")]
    control_plane: Option<String>,

    /// The `--control-plane-state` directory the printed command should use.
    #[arg(long, value_name = "DIR", default_value = "skimasque-gateway-state")]
    state: PathBuf,

    /// A label the gateway declares for policy targeting, `KEY=VALUE`,
    /// repeatable.
    #[arg(long = "label", value_name = "KEY=VALUE")]
    labels: Vec<String>,

    /// The public name the gateway is reached at. Threaded into the generated
    /// command as `--hostname`, and used as the ACME certificate name.
    #[arg(long, value_name = "HOST[:PORT]", default_value = "gateway.skimasque.com")]
    hostname: String,

    /// Have the gateway obtain a Let's Encrypt certificate for `--hostname`
    /// via ACME (TLS-ALPN-01) and renew it automatically.
    #[arg(long)]
    acme: bool,

    /// An additional name on the ACME certificate, beyond `--hostname`.
    /// Repeatable.
    #[arg(long = "acme-extra-domain", value_name = "DOMAIN", requires = "acme")]
    acme_extra_domains: Vec<String>,

    /// ACME account contact for `--acme` (a bare address gets a `mailto:`
    /// prefix).
    #[arg(long, value_name = "EMAIL", requires = "acme")]
    acme_email: Option<String>,

    /// Run the gateway now instead of only printing the command. Arguments after
    /// `--` are passed through to `skimasque-server`.
    #[arg(long)]
    run: bool,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true, value_name = "SERVER_ARGS")]
    extra: Vec<OsString>,
}

fn run_gateway_register(args: &[OsString]) -> anyhow::Result<ExitCode> {
    use skimasque_cli::account;

    // `--help` / `--version` / a usage error: let clap render it (stdout + exit
    // 0 for help, stderr + exit 2 for a mistake) rather than anyhow's "error:".
    let parsed = match GatewayRegisterArgs::try_parse_from(
        std::iter::once(OsString::from("skimasque gateway register"))
            .chain(args.iter().cloned()),
    ) {
        Ok(parsed) => parsed,
        Err(err) => err.exit(),
    };
    let creds = account::require()?;
    let base_url = skimasque_cli::normalize_base_url(
        &parsed
            .control_plane
            .unwrap_or_else(|| creds.control_plane.clone()),
    );
    let api = account::Api::new(&base_url)?;

    let org = account::block_on(account::resolve_org(&api, &creds, parsed.org))?;

    let token = account::block_on(api.mint_registration_token(&creds.session_token, &org))?;
    let name = parsed.name.unwrap_or_else(hostname_or_default);

    let mut cmd: Vec<String> = vec![
        "skimasque-server".into(),
        "--control-plane".into(),
        base_url.clone(),
        "--control-plane-state".into(),
        parsed.state.display().to_string(),
        "--control-plane-token".into(),
        token.registration_token.clone(),
        "--control-plane-name".into(),
        name,
        "--hostname".into(),
        parsed.hostname.clone(),
    ];
    for label in &parsed.labels {
        cmd.push("--control-plane-label".into());
        cmd.push(label.clone());
    }
    if parsed.acme {
        cmd.push("--acme".into());
        for extra in &parsed.acme_extra_domains {
            cmd.push("--acme-extra-domain".into());
            cmd.push(extra.clone());
        }
        if let Some(email) = &parsed.acme_email {
            cmd.push("--acme-email".into());
            cmd.push(email.clone());
        }
    }
    for extra in &parsed.extra {
        cmd.push(extra.to_string_lossy().into_owned());
    }

    if parsed.run {
        eprintln!("registered {org}; starting the gateway");
        let server_args: Vec<OsString> = cmd[1..].iter().map(OsString::from).collect();
        return delegate("skimasque-server", &server_args);
    }

    println!("Registration token minted for {org}. It is one-time and expires in about an hour.");
    println!();
    println!("  {}", shell_join(&cmd));
    println!();
    println!("Keep --control-plane-state: the gateway persists its identity there and will not");
    println!("need the token on later starts. Pass SKIMASQUE_CONTROL_TOKEN in the environment");
    println!("instead of the flag if you would rather not have it in shell history.");
    Ok(ExitCode::SUCCESS)
}

/// The machine's hostname, or `"gateway"` if it cannot be determined without a
/// dependency.
fn hostname_or_default() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .filter(|h| !h.trim().is_empty())
        .unwrap_or_else(|| "gateway".to_owned())
}

/// Join a command for display, single-quoting any argument that a shell would
/// otherwise split or expand.
fn shell_join(parts: &[String]) -> String {
    parts
        .iter()
        .map(|p| {
            if p.is_empty()
                || p.chars()
                    .any(|c| c.is_whitespace() || "\"'$`\\*?~#&|<>(){}[];".contains(c))
            {
                format!("'{}'", p.replace('\'', r"'\''"))
            } else {
                p.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn run_connect(destination: &str, passthrough: &[OsString]) -> anyhow::Result<ExitCode> {
    Target::parse(destination)
        .with_context(|| format!("the destination {destination:?} is not a valid host:port"))?;
    delegate("skimasque-client", &client_args(destination, passthrough))
}

/// `skimasque connect <dest> <rest>` maps to
/// `skimasque-client <rest> connect --target <dest>`: the client's connection
/// flags (`--proxy`, `--ca`, `--auth-token`, ...) sit before the subcommand.
fn client_args(destination: &str, passthrough: &[OsString]) -> Vec<OsString> {
    let mut args: Vec<OsString> = passthrough.to_vec();
    args.push("connect".into());
    args.push("--target".into());
    args.push(destination.into());
    args
}

fn run_why(
    destination: &str,
    request: &RequestArgs,
    source: &SourceArgs,
) -> anyhow::Result<ExitCode> {
    let loaded = source.load()?;
    let identity = request.identity().into_identity();
    let decision = policy::evaluate(
        &loaded,
        None,
        &request.app,
        request.transport,
        destination,
        identity.clone(),
    )?;

    println!("{}\n", if decision.is_allow() { "ALLOW" } else { "DENY" });
    for (label, value) in identity_lines(&identity) {
        println!("{label:<13}{value}");
    }
    if !request.app.is_empty() {
        println!("{:<13}{}", "Application:", request.app);
    }
    println!("{:<13}{}", "Transport:", request.transport);
    println!("{:<13}{destination}", "Destination:");

    match &decision {
        Decision::Allow(allowed) => {
            println!("{:<13}{}", "Policy:", allowed.policy);
            println!("{:<13}{}", "Rule:", allowed.rule);
        }
        Decision::Deny(denied) => {
            if let Some(policy) = &denied.policy {
                println!("{:<13}{policy}", "Policy:");
            }
            println!("\nReason:\n  {}", denied.reason.summary());
            if !denied.closest.is_empty() {
                println!("\nClosest rules:");
                for rule in &denied.closest {
                    println!("  {rule}");
                }
            }
            println!("\nSuggested rule:\n  {}", denied.suggested_rule);
        }
    }
    Ok(exit_for(&decision))
}

/// `why --control-plane`: evaluate the request against the org's published
/// policy through `POST /v1/orgs/{org}/policy/simulate` — the same engine the
/// gateway runs — rather than local files.
fn run_why_remote(
    destination: &str,
    request: &RequestArgs,
    org: Option<String>,
    revision: Option<u64>,
    draft: bool,
    gateway: Option<String>,
) -> anyhow::Result<ExitCode> {
    use skimasque_cli::account;

    let creds = account::require()?;
    let api = account::Api::new(&creds.control_plane)?;
    let org = account::block_on(account::resolve_org(&api, &creds, org))?;
    let identity = request.identity().into_identity();

    let body = serde_json::json!({
        "identity": identity,
        "application": request.app,
        "destination": destination,
        "transport": request.transport.as_str(),
    });
    let result = account::block_on(api.simulate(
        &creds.session_token,
        &org,
        revision,
        draft,
        gateway.as_deref(),
        &body,
    ))?;

    println!("{}\n", result.outcome.to_uppercase());
    for (label, value) in identity_lines(&identity) {
        println!("{label:<13}{value}");
    }
    if !request.app.is_empty() {
        println!("{:<13}{}", "Application:", request.app);
    }
    println!("{:<13}{}", "Transport:", request.transport);
    println!("{:<13}{destination}", "Destination:");
    println!("{:<13}{}", "Against:", result.against);
    if let Some(policy) = &result.policy {
        println!("{:<13}{policy}", "Policy:");
    }
    if let Some(rule) = &result.rule {
        println!("{:<13}{rule}", "Rule:");
    }

    if !result.steps.is_empty() {
        println!("\nWhy:");
        for step in &result.steps {
            println!("  {} {}", if step.ok { '+' } else { '-' }, step.text);
        }
    }
    if !result.limits.is_empty() {
        println!("\nLimits:");
        for limit in &result.limits {
            println!("  {:<16}{}", limit.label, limit.value);
        }
    }
    if let Some(rule) = &result.suggested_rule {
        println!("\nSuggested rule:\n  {rule}");
    }
    if !result.closest.is_empty() {
        println!("\nClosest rules:");
        for rule in &result.closest {
            println!("  {rule}");
        }
    }

    Ok(if result.outcome == "allow" {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// The identity block `why` prints, skipping fields that were not given. GitHub
/// is the only provider today, so the subject is rendered `github:<repo>`.
fn identity_lines(identity: &WorkloadIdentity) -> Vec<(&'static str, String)> {
    let subject = match (&identity.repository, &identity.organization) {
        (Some(repo), _) => format!("github:{repo}"),
        (None, Some(org)) => format!("github:{org}"),
        (None, None) => "(unspecified)".to_owned(),
    };
    let mut lines = vec![("Identity:", subject)];
    if let Some(workflow) = &identity.workflow {
        lines.push(("Workflow:", workflow.clone()));
    }
    if let Some(git_ref) = &identity.git_ref {
        lines.push(("Ref:", git_ref.clone()));
    }
    if let Some(environment) = &identity.environment {
        lines.push(("Environment:", environment.clone()));
    }
    if let Some(actor) = &identity.actor {
        lines.push(("Actor:", actor.clone()));
    }
    lines
}

fn run_status(source: &SourceArgs) -> anyhow::Result<ExitCode> {
    println!("policy source: {}", source.describe());

    let mut ok = true;
    match source.load() {
        Ok(loaded) => {
            let policies = loaded.set.policies();
            let rules: usize = policies.iter().map(|policy| policy.rules.len()).sum();
            println!(
                "  {} polic{}, {} rule{}, from {} file{}",
                policies.len(),
                if policies.len() == 1 { "y" } else { "ies" },
                rules,
                if rules == 1 { "" } else { "s" },
                loaded.sources.len(),
                if loaded.sources.len() == 1 { "" } else { "s" },
            );
            for policy in policies {
                let n = policy.rules.len();
                println!("    - {} ({n} rule{})", policy.name, if n == 1 { "" } else { "s" });
            }
            let mut out = String::new();
            let passed = policy::run_tests(&loaded, &mut out);
            println!(
                "  embedded tests: {}",
                if passed {
                    "all pass"
                } else {
                    "FAILURES -- run `skimasque policy test`"
                }
            );
            ok &= passed;
        }
        Err(error) => {
            println!("  does not load: {error:#}");
            ok = false;
        }
    }

    println!();
    match print_fleet_status() {
        Ok(()) => {}
        Err(error) => {
            println!("control plane: {error:#}");
            ok = false;
        }
    }

    Ok(if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// The control-plane section of `skimasque status`: the signed-in developer,
/// their organisations, and each org's gateway fleet. A no-op (one line) when
/// not signed in.
fn print_fleet_status() -> anyhow::Result<()> {
    use skimasque_cli::account;

    let Some(creds) = account::load()? else {
        println!("control plane: not signed in (run `skimasque login`)");
        return Ok(());
    };
    println!("control plane: {} as {}", creds.control_plane, creds.github_login);

    let api = account::Api::new(&creds.control_plane)?;
    let orgs = account::block_on(api.list_orgs(&creds.session_token))
        .context("listing organisations (session may be stale -- try `skimasque login`)")?;
    if orgs.is_empty() {
        println!("  no organisations");
        return Ok(());
    }
    for org in &orgs {
        let gateways = account::block_on(api.list_gateways(&creds.session_token, &org.id))?;
        println!("  {} ({})  {} gateway{}", org.name, org.id, gateways.len(), plural(gateways.len()));
        if let Ok(usage) = account::block_on(api.usage(&creds.session_token, &org.id)) {
            if usage.updated_at_ms > 0 {
                println!(
                    "      usage: {} tunnels, {} relayed, {} active, updated {}",
                    usage.tunnels_total,
                    human_bytes(usage.bytes_total),
                    usage.active_gateways,
                    ago(usage.updated_at_ms),
                );
            }
        }
        for gw in &gateways {
            let seen = match gw.last_seen_ms {
                Some(ms) => format!("last seen {}", ago(ms)),
                None => "never seen".to_owned(),
            };
            let version = gw
                .acked_policy_version
                .map(|v| format!(", policy v{v}"))
                .unwrap_or_default();
            let labels = if gw.labels.is_empty() {
                String::new()
            } else {
                let pairs: Vec<_> = gw.labels.iter().map(|(k, v)| format!("{k}={v}")).collect();
                format!(" [{}]", pairs.join(" "))
            };
            println!("    - {:<20} {:<9} {seen}{version}{labels}", gw.name, gw.status);
        }
    }
    Ok(())
}

/// A coarse human-readable byte count (`1.4 GB`).
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// A coarse "n minutes ago" for a Unix-millis timestamp.
fn ago(then_ms: u64) -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let secs = now_ms.saturating_sub(then_ms) / 1000;
    match secs {
        0..=59 => format!("{secs}s ago"),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

/// Run a sibling binary (`skimasque-server`, `skimasque-client`) with `args`,
/// forwarding its exit status. Looks next to the running `skimasque` first, then
/// falls back to `PATH`.
fn delegate(sibling: &str, args: &[OsString]) -> anyhow::Result<ExitCode> {
    let program = sibling_path(sibling);
    let status = std::process::Command::new(&program)
        .args(args)
        .status()
        .with_context(|| {
            format!(
                "running {} -- is it installed alongside skimasque?",
                program.to_string_lossy()
            )
        })?;
    Ok(exit_code(status))
}

/// `<sibling>` next to the current executable if it is there, otherwise the bare
/// name for a `PATH` lookup. Carries over the current binary's extension so this
/// works on Windows.
fn sibling_path(sibling: &str) -> OsString {
    let mut name = OsString::from(sibling);
    let current = std::env::current_exe().ok();
    if let Some(ext) = current.as_ref().and_then(|path| path.extension()) {
        name.push(".");
        name.push(ext);
    }
    if let Some(dir) = current.as_ref().and_then(|path| path.parent()) {
        let candidate = dir.join(&name);
        if candidate.is_file() {
            return candidate.into_os_string();
        }
    }
    name
}

fn exit_code(status: ExitStatus) -> ExitCode {
    match status.code() {
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        // Terminated by a signal: report a generic failure.
        None => ExitCode::from(1),
    }
}

fn exit_for(decision: &Decision) -> ExitCode {
    if decision.is_allow() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn load_revision(path: &std::path::Path) -> anyhow::Result<Loaded> {
    if path.is_dir() {
        policy::load_dir(path)
    } else {
        policy::load_files(std::slice::from_ref(&path.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn branch_and_ref_cannot_both_be_given() {
        assert!(Cli::try_parse_from([
            "skimasque", "policy", "check", "prod", "api.example.com:443", "--app", "terraform",
            "--ref", "refs/heads/main", "--branch", "main",
        ])
        .is_err());
    }

    #[test]
    fn why_control_plane_flags_require_control_plane() {
        // The remote options are gated on --control-plane.
        assert!(Cli::try_parse_from([
            "skimasque", "why", "db.internal:5432", "--org", "org_1",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "skimasque", "why", "db.internal:5432", "--control-plane", "--org", "org_1",
            "--revision", "3",
        ])
        .is_ok());
        // --revision and --draft are mutually exclusive.
        assert!(Cli::try_parse_from([
            "skimasque", "why", "db.internal:5432", "--control-plane", "--revision", "3", "--draft",
        ])
        .is_err());
    }

    #[test]
    fn gateway_register_args_parse_and_the_command_is_shell_safe() {
        let parsed = GatewayRegisterArgs::try_parse_from([
            "skimasque gateway register",
            "--org",
            "org_1",
            "--label",
            "env=prod",
            "--label",
            "region=eu",
            "--",
            "--connect-tcp",
        ])
        .unwrap();
        assert_eq!(parsed.org.as_deref(), Some("org_1"));
        assert_eq!(parsed.labels, vec!["env=prod", "region=eu"]);
        assert_eq!(parsed.state.to_str(), Some("skimasque-gateway-state"));
        assert_eq!(parsed.extra, vec![OsString::from("--connect-tcp")]);

        // Values that a shell would split or expand are quoted.
        assert_eq!(
            shell_join(&["skimasque-server".into(), "a b".into(), "plain".into(), "$x".into()]),
            "skimasque-server 'a b' plain '$x'"
        );
        assert_eq!(shell_join(&["skmreg_deadbeef".into()]), "skmreg_deadbeef");
    }

    #[test]
    fn connect_puts_the_destination_after_the_client_subcommand() {
        let args = client_args(
            "db.internal:5432",
            &[OsString::from("--proxy"), OsString::from("10.0.0.1:4433")],
        );
        assert_eq!(
            args,
            vec![
                OsString::from("--proxy"),
                OsString::from("10.0.0.1:4433"),
                OsString::from("connect"),
                OsString::from("--target"),
                OsString::from("db.internal:5432"),
            ]
        );
    }

    #[test]
    fn why_selects_a_policy_by_identity_and_reports_the_rule() {
        let dir = std::env::temp_dir().join(format!("skimasque-why-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("p.toml"),
            r#"
            name = "prod"
            [match]
            repository = "acme/widget"
            [[rules]]
            id = "tf"
            application = "terraform"
            action = "allow"
            destinations = ["api.example.com:443"]
        "#,
        )
        .unwrap();

        let source = SourceArgs {
            policy_dir: dir.clone(),
            policy_files: vec![],
        };
        let request = RequestArgs {
            app: "terraform".to_owned(),
            transport: Transport::Tcp,
            identity: IdentityFlags {
                organization: None,
                repository: Some("acme/widget".to_owned()),
                workflow: None,
                git_ref: None,
                branch: None,
                environment: None,
                actor: None,
            },
        };

        let allow = run_why("api.example.com:443", &request, &source).unwrap();
        assert_eq!(allow, ExitCode::SUCCESS);
        let deny = run_why("evil.example.com:443", &request, &source).unwrap();
        assert_eq!(deny, ExitCode::FAILURE);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn validate_warns_on_a_broad_match_and_strict_makes_it_fail() {
        let dir = std::env::temp_dir().join(format!("skimasque-validate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("broad.toml");
        std::fs::write(
            &file,
            r#"
            name = "broad"
            [match]
            organization = "acme"
            [[rules]]
            application = "terraform"
            action = "allow"
            destinations = ["api.example.com:443"]
        "#,
        )
        .unwrap();

        let mut out = String::new();
        assert!(
            policy::validate(std::slice::from_ref(&file), false, &mut out),
            "a lint alone does not fail validate: {out}"
        );
        assert!(out.contains("warn"), "{out}");
        assert!(out.contains("match-lacks-repository"), "{out}");

        let mut strict_out = String::new();
        assert!(
            !policy::validate(std::slice::from_ref(&file), true, &mut strict_out),
            "--strict turns the lint into a failure: {strict_out}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn init_scaffolds_a_policy_directory_and_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("skimasque-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let policy_dir = dir.join(".masque/policies");

        run_init(&policy_dir).unwrap();
        let example = policy_dir.join("example.toml");
        assert!(example.is_file());
        let first = std::fs::read_to_string(&example).unwrap();
        // The starter policy must itself be valid.
        skimasque_policy::Policy::from_toml(&first).expect("the starter policy parses");

        // Running again leaves the (possibly edited) file untouched.
        std::fs::write(&example, "name = \"edited\"\n").unwrap();
        run_init(&policy_dir).unwrap();
        assert_eq!(std::fs::read_to_string(&example).unwrap(), "name = \"edited\"\n");

        std::fs::remove_dir_all(&dir).ok();
    }
}
