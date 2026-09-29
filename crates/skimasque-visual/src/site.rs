//! The pages `sitegen` writes under the website's `site/` directory. For now
//! one page: the component gallery, every primitive in every state, shown in
//! a dark and a light section so both themes can be checked without JS.

use askama::Template;

use crate::{
    Boundary, Component, ConnKind, Connection, DecisionBadge, Flow, Html, Icons, Node, NodeKind,
    Status, StatusBadge,
};

pub struct Page {
    pub path: &'static str,
    pub contents: String,
}

struct Item {
    caption: String,
    html: Html,
}
struct Group {
    title: &'static str,
    /// Items that need the full row (flows measure their own width).
    wide: bool,
    items: Vec<Item>,
}

#[derive(Template)]
#[template(path = "gallery.html")]
struct Gallery {
    sprite: Html,
    groups: Vec<Group>,
    themes: [&'static str; 2],
}

fn item(caption: impl Into<String>, c: &impl Component) -> Item {
    Item {
        caption: caption.into(),
        html: c.html(),
    }
}

fn groups() -> Vec<Group> {
    let nodes = NodeKind::ALL
        .iter()
        .map(|k| item(k.slug(), &Node::new(*k)))
        .collect();
    let conns = [
        ConnKind::Normal,
        ConnKind::Control,
        ConnKind::Active,
        ConnKind::Potential,
        ConnKind::Denied,
    ]
    .into_iter()
    .map(|k| item(k.slug(), &Connection::new(k)))
    .collect();
    let statuses = {
        let mut v: Vec<Item> = [
            Status::Allow,
            Status::Deny,
            Status::Active,
            Status::Expired,
            Status::Pending,
            Status::Blocked,
        ]
        .into_iter()
        .map(|s| item(s.word(), &StatusBadge { status: s }))
        .collect();
        v.push(item("granted", &DecisionBadge { allow: true }));
        v.push(item("denied", &DecisionBadge { allow: false }));
        v
    };
    let flow = Flow::new(
        "A GitHub Actions job proves its identity with OIDC, SkiMasque evaluates the policy, \
         and an active session carries its traffic through the gateway to the private database.",
    )
    .then(&Node::new(NodeKind::GitHub).sub("acme/widget"))
    .via(
        Connection::new(ConnKind::Control).label("OIDC"),
        &Node::new(NodeKind::Identity),
    )
    .via(
        Connection::new(ConnKind::Control).label("policy"),
        &Node::new(NodeKind::Policy).status(Status::Allow),
    )
    .then(&Node::new(NodeKind::Session).sub("20 min"))
    .via(
        Connection::new(ConnKind::Active),
        &Node::new(NodeKind::Gateway).sub("us-west"),
    )
    .via(
        Connection::new(ConnKind::Active),
        &Node::new(NodeKind::Database).sub("db.prod:5432"),
    );
    let denied = Flow::new("A request with no matching allow rule is denied at the policy.")
        .then(&Node::new(NodeKind::CiJob).sub("curl"))
        .then(&Node::new(NodeKind::Policy))
        .via(
            Connection::new(ConnKind::Denied),
            &Node::new(NodeKind::Deny).label("No matching allow rule"),
        );
    let boundary_flow = Flow::new(
        "A developer's request crosses the firewall into the VPC, where the gateway \
         forwards it to the database.",
    )
    .then(&Node::new(NodeKind::Developer))
    .via(
        Connection::new(ConnKind::Active),
        &Boundary::region("YOUR VPC")
            .child(&Node::new(NodeKind::Gateway))
            .child(&Node::new(NodeKind::Database)),
    );
    let region = Boundary::region("YOUR VPC")
        .child(&Node::new(NodeKind::Gateway))
        .child(&Node::new(NodeKind::Database).sub("db.prod:5432"))
        .child(&Node::new(NodeKind::Api).sub("api.internal:443"));
    let firewall = Boundary::firewall("YOUR FIREWALL").child(&Node::new(NodeKind::Network));
    vec![
        Group {
            title: "Nodes",
            wide: false,
            items: nodes,
        },
        Group {
            title: "Connections",
            wide: false,
            items: conns,
        },
        Group {
            title: "Statuses",
            wide: false,
            items: statuses,
        },
        Group {
            title: "Flows",
            wide: true,
            items: vec![
                item("access", &flow),
                item("denied", &denied),
                item("a boundary as a step", &boundary_flow),
            ],
        },
        Group {
            title: "Boundaries",
            wide: false,
            items: vec![item("region", &region), item("firewall", &firewall)],
        },
    ]
}

pub fn pages() -> Vec<Page> {
    let gallery = Gallery {
        sprite: Icons.html(),
        groups: groups(),
        themes: ["light", "dark"],
    };
    vec![
        Page {
            path: "components/index.html",
            contents: gallery.render().expect("gallery renders"),
        },
        Page {
            path: "components/visual.css",
            contents: crate::CSS.to_owned(),
        },
    ]
}

/// Paths under `root` that are stale: a generated file that is missing or
/// differs from a fresh render, or an orphan under `components/` that `pages()`
/// no longer produces. Line endings are normalised so a CRLF checkout is not
/// reported.
pub fn stale(root: &std::path::Path) -> Vec<String> {
    let pages = pages();
    let mut out: Vec<String> = pages
        .iter()
        .filter(|p| match std::fs::read_to_string(root.join(p.path)) {
            Ok(on_disk) => on_disk.replace("\r\n", "\n") != p.contents.replace("\r\n", "\n"),
            Err(_) => true,
        })
        .map(|p| p.path.to_owned())
        .collect();
    let mut orphans = Vec::new();
    collect_files(&root.join("components"), "components", &mut orphans);
    orphans.retain(|rel| !pages.iter().any(|p| p.path == rel));
    orphans.sort();
    out.extend(orphans);
    out
}

/// Every file below `dir`, as `/`-separated paths starting with `prefix`.
fn collect_files(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let rel = format!("{prefix}/{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            collect_files(&entry.path(), &rel, out);
        } else {
            out.push(rel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_is_deterministic() {
        let a: Vec<_> = pages().into_iter().map(|p| (p.path, p.contents)).collect();
        let b: Vec<_> = pages().into_iter().map(|p| (p.path, p.contents)).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn the_gallery_shows_every_primitive_in_both_themes() {
        let page = pages()
            .into_iter()
            .find(|p| p.path == "components/index.html")
            .unwrap()
            .contents;
        assert!(
            page.contains(r#"<link rel="stylesheet" href="visual.css">"#),
            "stylesheet linked relatively"
        );
        assert!(
            page.contains(r#"<svg class="v-sprite""#),
            "sprite emitted once"
        );
        assert_eq!(page.matches(r#"<svg class="v-sprite""#).count(), 1);
        assert_eq!(page.matches("<h1").count(), 1, "one page-level h1");
        assert_eq!(page.matches("<h2").count(), 2, "one h2 per theme");
        assert!(page.contains(r#"data-theme="dark""#) && page.contains(r#"data-theme="light""#));
        for kind in crate::NodeKind::ALL {
            assert!(
                page.contains(&format!(r#"data-kind="{}""#, kind.slug())),
                "{kind:?}"
            );
        }
        for slug in ["normal", "control", "active", "potential", "denied"] {
            assert!(page.contains(&format!("v-conn-{slug}")), "{slug}");
        }
        for word in [
            "ALLOW",
            "DENY",
            "ACTIVE",
            "EXPIRED",
            "PENDING",
            "BLOCKED",
            "ACCESS GRANTED",
            "ACCESS DENIED",
        ] {
            assert!(page.contains(word), "{word}");
        }
        assert!(
            page.contains("v-flow")
                && page.contains("v-boundary-region")
                && page.contains("v-boundary-firewall")
        );
        assert!(page.contains(r#"class="g-cap""#), "captions carry g-cap");
        assert!(!page.contains("exec"), "honesty rule: no `skimasque exec`");
    }

    #[test]
    fn stale_ignores_crlf_and_reports_missing_or_changed_files() {
        let dir = std::env::temp_dir().join(format!("sitegen-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for p in pages() {
            let path = dir.join(p.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, p.contents.replace('\n', "\r\n")).unwrap();
        }
        assert!(stale(&dir).is_empty(), "CRLF checkout is not stale");
        std::fs::write(dir.join("components/visual.css"), "changed").unwrap();
        assert_eq!(stale(&dir), vec!["components/visual.css"]);
        std::fs::remove_file(dir.join("components/index.html")).unwrap();
        assert_eq!(stale(&dir).len(), 2);
        std::fs::write(dir.join("components/visual.css"), crate::CSS).unwrap();
        std::fs::create_dir_all(dir.join("components/old")).unwrap();
        std::fs::write(dir.join("components/old/gone.html"), "orphan").unwrap();
        let stale_now = stale(&dir);
        assert!(
            stale_now.contains(&"components/old/gone.html".to_owned()),
            "orphans are reported: {stale_now:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
