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

const CONNECT: &str = "# Bridge one TCP tunnel to stdin/stdout, signed in with skimasque login\n$ skimasque connect db.internal:5432 --proxy gateway.example.com --org acme\n\n# Or run a local SOCKS5 relay and point ALL_PROXY at it\n$ skimasque-client --proxy gateway.example.com --org acme --app psql socks5 --listen 127.0.0.1:1080\n$ ALL_PROXY=socks5h://127.0.0.1:1080 psql -h db.internal -p 5432";

const QUICK_START: &str =
    "$ skimasque login\n$ skimasque org create \"Acme\"\n$ skimasque init\n$ skimasque policy test";

const LOCAL_POLICY: &str = "$ skimasque policy check developer-db dev-db.internal:5432 --app psql\n$ skimasque policy explain developer-db dev-db.internal:5432 --app psql\n$ skimasque why dev-db.internal:5432 --app psql --repository acme/platform --branch main";

const EXEC_FLOW: &str =
    "$ skimasque exec \\\n    --policy production \\\n    --app terraform \\\n    -- terraform apply";
const EXEC_READOUT: &str = "$ skimasque exec --policy production -- terraform plan\n\nIdentity:\n  alice\n\nApplication:\n  terraform\n\nPolicy:\n  production\n\nDestination:\n  db.prod:5432\n\nSession:\n  20m\n\nAccess:\n  GRANTED";

fn exec_flow() -> Flow {
    let n = |k: NodeKind, l: &str| Node::new(k).label(l);
    let c = || Connection::new(ConnKind::Normal);
    let a = || Connection::new(ConnKind::Active);
    Flow::new(
        "A wrapped command would be checked for identity and policy, given a session, and then run.",
    )
    .then(&n(NodeKind::Cli, "COMMAND"))
    .via(c(), &n(NodeKind::Identity, "IDENTITY"))
    .via(c(), &n(NodeKind::Policy, "POLICY"))
    .via(a(), &n(NodeKind::Session, "SESSION"))
    .via(a(), &n(NodeKind::Application, "COMMAND EXECUTION"))
}

pub fn page() -> Page {
    let hero = Hero::new("Run the command. Get the access. Lose the access when you're done.")
        .lead("SkiMasque should feel like a developer tool, not a VPN client.")
        .cta(Cta::primary("Get Started", GET_STARTED_URL))
        .cta(Cta::secondary("Read the Docs", doc("cli.md")));

    let cli = Section::new("The command line today")
        .push(&Prose::new().p(
            "skimasque connect opens a tunnel to one destination through your gateway, delegating to skimasque-client. Tools that honour ALL_PROXY can use the local SOCKS5 relay instead.",
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
                .p("A command wrapper would request the access a command needs, run the command, and let the access expire.")
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
            CodeExample::new("what the command would print", EXEC_READOUT).html(),
        ],
    };
    let planned_exec = Section::new("Planned: a command wrapper")
        .alt()
        .push(&PlannedBlock::new(
            "skimasque exec — a command wrapper that requests access, runs the command, and lets access expire",
            &exec_body,
        ));

    let cp_note = Section::new("What is not finished yet").push(&PlannedBlock::new(
        "control-plane-backed connect",
        &Prose::new().p(
            "Today skimasque connect needs the gateway address passed explicitly. A control-plane-backed connect, which would resolve the organisation's gateway and credential from your signed-in session, is not finished.",
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
        .push(&planned_exec)
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
            "SkiMasque should feel like a developer tool, not a VPN client.",
            "skimasque connect",
            "skimasque login",
            "skimasque policy test",
            "Local development",
            "No network ceremony",
            "Developer flow",
            "PLANNED",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
        assert!(s.contains("v-planned-block") && s.contains("skimasque exec"));
        assert_eq!(
            s.matches("<div class=\"v-planned-block\"").count(),
            2,
            "planned blocks are siblings, never nested"
        );
    }
}
