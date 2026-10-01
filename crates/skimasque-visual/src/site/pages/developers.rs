//! Developers (`/developers`): canonical spec §16.

use askama::Template;

use super::doc;
use crate::diagrams::public;
use crate::site::Page;
use crate::site_chrome::GET_STARTED_URL;
use crate::{
    CodeExample, Component, ConnKind, Connection, Cta, Flow, Hero, Html, Node, NodeKind,
    PlannedBlock, Prose, Section, SitePage,
};

/// Several components rendered one after another (used inside a planned block).
#[derive(Template)]
#[template(
    source = r#"{% for p in parts %}{{ p|safe }}{% endfor %}"#,
    ext = "html"
)]
struct Stack {
    parts: Vec<Html>,
}
impl Component for Stack {}

const CONNECT: &str = "# Bridge one TCP tunnel to stdin/stdout, signed in with skimasque login\n$ skimasque connect db.internal:5432 --proxy gateway.example.com --org acme\n\n# Or run a local SOCKS5 relay and point ALL_PROXY at it\n$ skimasque-client --proxy gateway.example.com --org acme --app curl socks5 --listen 127.0.0.1:1080\n$ ALL_PROXY=socks5h://127.0.0.1:1080 curl https://api.internal";

const QUICK_START: &str =
    "$ skimasque login\n$ skimasque org create \"Acme\"\n$ skimasque init\n$ skimasque policy test";

const LOCAL_POLICY: &str = "$ skimasque policy check developer-db dev-db.internal:5432 --app psql\n$ skimasque policy explain developer-db dev-db.internal:5432 --app psql\n$ skimasque why dev-db.internal:5432 --app psql --repository acme/platform --branch main";

const EXEC_FLOW: &str =
    "$ skimasque exec \\\n    --policy production \\\n    --app terraform \\\n    -- terraform apply";
const EXEC_READOUT: &str = "$ skimasque exec --policy production --forward 15432:db.prod:5432 -- psql -h 127.0.0.1 -p 15432\nSkiMasque\n\nIdentity     alice\nPolicy       production\nApplication  psql\nGateway      gateway.skimasque.com\n\nAccess\n  db.prod:5432     ✓  → 127.0.0.1:15432\n\nSession\n  20m\n\nConnected.";

fn exec_flow() -> Flow {
    let n = |k: NodeKind, l: &str| Node::new(k).label(l);
    let c = || Connection::new(ConnKind::Normal);
    let a = || Connection::new(ConnKind::Active);
    Flow::new("A command is checked for identity and policy, given a session, and then run.")
        .then(&n(NodeKind::Cli, "COMMAND"))
        .via(c(), &n(NodeKind::Identity, "IDENTITY"))
        .via(c(), &n(NodeKind::Policy, "POLICY"))
        .via(a(), &n(NodeKind::Session, "SESSION"))
        .via(a(), &n(NodeKind::Application, "COMMAND EXECUTION"))
}

pub fn page() -> Page {
    let hero = Hero::new("Run the command. Get the access. Lose the access when you're done.")
        .lead(
            "Use the CLI to check a policy, open a tunnel, or run a command with temporary access.",
        )
        .cta(Cta::primary("Get Started", GET_STARTED_URL))
        .cta(Cta::secondary("Read the Docs", doc("cli.md")));

    let cli = Section::new("The command line today")
        .push(&Prose::new().p(
            "skimasque connect opens a tunnel to one destination through your gateway. Tools that honour ALL_PROXY can use the local SOCKS5 relay instead.",
        ))
        .push(&CodeExample::new("connect to a private service", CONNECT))
        .push(&public::developer_cli());

    let start = Section::new("Quick start")
        .alt()
        .push(&Prose::new().p(
            "Sign in with GitHub, create an organisation, scaffold a policy, and test it offline. The tests run without a network and fit in CI.",
        ))
        .push(&CodeExample::new("sign in and write a policy", QUICK_START));

    let local = Section::new("Local development")
        .push(&Prose::new().p(
            "Check what a policy would do before you run anything: check, explain and why evaluate a request locally, with no network.",
        ))
        .push(&CodeExample::new("check policy locally", LOCAL_POLICY))
        .push(&Prose::new().p(
            "The same command can reach different destinations under different policies.",
        ))
        .push(&public::same_command_different_policy());

    let exec_body = Stack {
        parts: vec![
            Prose::new()
                .p("skimasque exec requests the access a command needs, runs the command, and drops the access when it exits. Tools that honour HTTPS_PROXY or ALL_PROXY can use its local proxy; for tools that don't, --forward opens a local port to one destination.")
                .html(),
            CodeExample::new("wrap a command", EXEC_FLOW).html(),
            exec_flow().html(),
            Prose::new()
                .sub("No network ceremony")
                .p("Avoid:")
                .list(&[
                    "connect VPN",
                    "wait",
                    "change network",
                    "run command",
                    "remember to disconnect",
                ])
                .p("Prefer:")
                .html(),
            CodeExample::new(
                "one command",
                "$ skimasque exec --policy production -- terraform apply",
            )
            .html(),
            Prose::new().sub("Developer flow").html(),
            CodeExample::new("what it prints", EXEC_READOUT).html(),
        ],
    };
    let exec = Section::new("Run a command with access")
        .alt()
        .push(&exec_body);

    let cp_note = Section::new("What is not finished yet").push(&PlannedBlock::new(
        "control-plane-backed connect",
        &Prose::new().p(
            "The client can mint a developer credential from your signed-in session. Automatic discovery of a customer gateway is not implemented: use --proxy for connect or --gateway for exec when the Cloud default is not your destination.",
        ),
    ));

    Page {
        path: "developers/index.html",
        contents: SitePage::new(
            "../",
            "developers",
            "Developers · SkiMasque",
            "Run the command, get the access, lose the access when you're done: the skimasque command line for developers, with local policy checks.",
        )
        .push(&hero)
        .push(&cli)
        .push(&start)
        .push(&local)
        .push(&exec)
        .push(&Section::new("Coding agents").push(&Prose::new()
            .p("Use skimasque exec --agent --sandbox srt with allowed domains to run a coding agent through the gateway, or explicitly choose --unsandboxed. SkiMasque integrates with an external sandbox; proxy variables alone do not confine a process.")
            .p("Agent sessions default to 30 minutes, are capped at 4 hours or a lower organisation limit, and support delegation. End a session in the CLI or console to revoke it and its children. Gateways apply revocation updates when received; expiry bounds access during outages.")))
        .push(&cp_note)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn developers_follows_the_canonical_spec() {
        let p = page();
        assert_eq!(p.path, "developers/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        for want in [
            "Run the command. Get the access. Lose the access when you're done.",
            "Use the CLI to check a policy, open a tunnel, or run a command with temporary access.",
            "skimasque connect",
            "skimasque login",
            "skimasque policy test",
            "Local development",
            "No network ceremony",
            "Developer flow",
            "skimasque exec",
            "--forward",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
        assert!(
            !s.contains("Planned: a command wrapper"),
            "exec is built now"
        );
        assert_eq!(
            s.matches("<div class=\"v-planned-block\"").count(),
            1,
            "only the control-plane-backed connect note is still planned"
        );
    }
}
