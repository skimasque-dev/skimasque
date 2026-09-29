//! The pages `sitegen` writes under the website's `site/` directory. For now
//! one page: the component gallery, every primitive in every state, shown in
//! a dark and a light section so both themes can be checked without JS.

use askama::Template;

use crate::{
    AuditEventCard, Boundary, Change, Check, CodeExample, Component, ConnKind, Connection, Contour,
    DecisionBadge, DecisionCard, DecisionExplainer, EmptyState, Expire, Flow, GatewayCard,
    HealthCard, Html, Icons, IdentityCard, Mountain, Node, NodeKind, Planned, PolicyCard,
    PolicyDiff, PolicyExplorer, PolicySummary, Reveal, Route, Run, RunCard, SessionCard,
    SessionTimeline, Shape, Status, StatusBadge, TimelineEvent, Tone, TrailMarker, DIMENSIONS,
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

fn entry_items(v: Vec<crate::diagrams::Entry>) -> Vec<Item> {
    v.into_iter()
        .map(|e| Item {
            caption: e.title.to_owned(),
            html: e.html,
        })
        .collect()
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
            Status::Granted,
            Status::Denied,
            Status::Healthy,
            Status::Degraded,
            Status::Offline,
        ]
        .into_iter()
        .map(|s| item(s.word(), &StatusBadge { status: s }))
        .collect();
        v.push(item("granted", &DecisionBadge::new(true)));
        v.push(item("denied", &DecisionBadge::new(false)));
        v.push(item(
            "technical allow",
            &DecisionBadge::new(true).technical(),
        ));
        v.push(item(
            "technical deny",
            &DecisionBadge::new(false).technical(),
        ));
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
    let summary = || PolicySummary::new("acme/widget", "terraform", "db.prod:5432", "20 min");
    let policies = vec![
        item("summary", &summary()),
        item(
            "card · active",
            &PolicyCard::new("production-deploy", Status::Active, summary())
                .meta("updated 2h ago")
                .href("#"),
        ),
        item(
            "card · pending",
            &PolicyCard::new("staging-access", Status::Pending, summary()),
        ),
        item(
            "explorer · granted",
            &PolicyExplorer::new(
                &["acme/widget"],
                &["terraform"],
                &["db.prod:5432"],
                &["20 min"],
                true,
                "policy production-deploy matches",
            ),
        ),
        item(
            "explorer · denied",
            &PolicyExplorer::new(&[], &[], &[], &[], false, "no matching allow rule"),
        ),
        item(
            "diff",
            &PolicyDiff::new(vec![
                Change::added("limit", "TCP only"),
                Change::removed("target", "db.old:5432"),
                Change::changed("duration", "20 min", "10 min"),
            ]),
        ),
        item("diff · empty", &PolicyDiff::new(vec![])),
    ];
    let dims_ok = || {
        DIMENSIONS
            .iter()
            .map(|d| Check::pass(*d))
            .collect::<Vec<_>>()
    };
    let decisions = vec![
        item(
            "explainer · granted",
            &DecisionExplainer::new(
                dims_ok(),
                true,
                "identity, application, destination and limits all match production-deploy",
            ),
        ),
        item(
            "explainer · denied",
            &DecisionExplainer::new(
                vec![
                    Check::pass("Identity"),
                    Check::pass("Application"),
                    Check::fail("Destination").detail("db.staging:5432 is not in this policy"),
                    Check::pass("Limits"),
                    Check::pass("Policy active"),
                ],
                false,
                "destination not allowed",
            ),
        ),
        item(
            "decision card · granted",
            &DecisionCard::new(true, "acme/widget", "db.prod:5432", "2026-09-29 14:02 UTC")
                .policy("production-deploy"),
        ),
        item(
            "decision card · denied",
            &DecisionCard::new(
                false,
                "acme/widget",
                "db.staging:5432",
                "2026-09-29 14:05 UTC",
            ),
        ),
        item(
            "audit event · with explanation",
            &AuditEventCard::new(
                DecisionCard::new(
                    false,
                    "acme/widget",
                    "db.staging:5432",
                    "2026-09-29 14:05 UTC",
                ),
                "destination not allowed",
            )
            .explainer(DecisionExplainer::new(
                vec![
                    Check::pass("Identity"),
                    Check::fail("Destination").detail("db.staging:5432 is not in this policy"),
                ],
                false,
                "destination not allowed",
            ))
            .technical(),
        ),
    ];
    let long = "x".repeat(200);
    let access = vec![
        item(
            "session · active 40%",
            &SessionCard::new(
                "acme/widget",
                "db.prod:5432",
                "us-west",
                Status::Active,
                40,
                "12 min left",
            ),
        ),
        item(
            "session · expired",
            &SessionCard::new(
                "acme/widget",
                "db.prod:5432",
                "us-west",
                Status::Expired,
                0,
                "expired",
            ),
        ),
        item(
            "timeline",
            &SessionTimeline::new(vec![
                TimelineEvent::new("14:02:01", "Requested", Tone::Neutral),
                TimelineEvent::new("14:02:01", "Authorized", Tone::Active),
                TimelineEvent::new("14:02:02", "Session started", Tone::Active),
                TimelineEvent::new("14:22:01", "Expires", Tone::Neutral),
            ]),
        ),
        item(
            "gateway · healthy (egress planned)",
            &GatewayCard::new("gw-us-west", "us-west-2", Status::Healthy, "12 s ago", 3),
        ),
        item(
            "gateway · degraded (egress known)",
            &GatewayCard::new(
                "gw-eu-central",
                "eu-central-1",
                Status::Degraded,
                "9 min ago",
                0,
            )
            .egress_ip("203.0.113.7"),
        ),
        item(
            "health · healthy",
            &HealthCard::new("Control plane", Status::Healthy, "all checks passing"),
        ),
        item(
            "health · offline",
            &HealthCard::new("Control plane", Status::Offline, "no heartbeat for 10 min"),
        ),
        item(
            "identity",
            &IdentityCard::new("acme/widget", "GitHub Actions", 2).last_seen("2 h ago"),
        ),
        item(
            "identity · never seen",
            &IdentityCard::new("acme/new-repo", "GitHub Actions", 0),
        ),
    ];
    let hostile = vec![
        item(
            "policy card",
            &PolicyCard::new(
                "<script>alert(1)</script>",
                Status::Active,
                PolicySummary::new(long.as_str(), "terraform", "db.prod:5432", "20 min"),
            ),
        ),
        item(
            "identity",
            &IdentityCard::new("<script>alert(1)</script>", long.as_str(), 1)
                .last_seen(long.as_str()),
        ),
    ];
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
        Group {
            title: "Policies",
            wide: false,
            items: policies,
        },
        Group {
            title: "Decisions",
            wide: false,
            items: decisions,
        },
        Group {
            title: "Access",
            wide: false,
            items: access,
        },
        Group {
            title: "Hostile input",
            wide: false,
            items: hostile,
        },
        Group {
            title: "Content",
            wide: false,
            items: vec![
                item("planned", &Planned::new()),
                item(
                    "planned with note",
                    &Planned::new().note("shown, not available"),
                ),
                item("empty · policies", &EmptyState::no_policies("#")),
                item("empty · sessions", &EmptyState::no_sessions()),
                item("empty · gateways", &EmptyState::no_gateways("#")),
                item(
                    "code",
                    &CodeExample::new(
                        "connect to a private database",
                        "# open a session
$ skimasque connect db.prod:5432
listening on 127.0.0.1:5432",
                    ),
                ),
            ],
        },
        Group {
            title: "Motifs",
            wide: false,
            items: vec![
                item("contour", &Contour),
                item("mountain", &Mountain),
                item("route", &Route::new()),
                item("route · flowing", &Route::new().flowing()),
                item(
                    "trail marker · circle",
                    &TrailMarker::new(Shape::Circle, Tone::Active, "trailhead"),
                ),
                item(
                    "trail marker · square",
                    &TrailMarker::new(Shape::Square, Tone::Info, "checkpoint"),
                ),
                item(
                    "trail marker · diamond",
                    &TrailMarker::new(Shape::Diamond, Tone::Neutral, "summit"),
                ),
                item(
                    "run · Blue",
                    &RunCard::new(
                        Run::Blue,
                        "Your Gateway",
                        &["SkiMasque control plane", "Gateway runs in your network"],
                    ),
                ),
            ],
        },
        Group {
            title: "Motion",
            wide: true,
            items: vec![
                item(
                    "expired session",
                    &Expire::new(&Node::new(NodeKind::Session).status(Status::Expired)),
                ),
                item(
                    "reveal",
                    &Reveal::new(
                        &Flow::new("Request, policy, decision.")
                            .then(&Node::new(NodeKind::Workload))
                            .then(&Node::new(NodeKind::Policy))
                            .then(&Node::new(NodeKind::Allow)),
                    ),
                ),
            ],
        },
        Group {
            title: "Public diagrams",
            wide: true,
            items: entry_items(crate::diagrams::public_set()),
        },
        Group {
            title: "Control-plane diagrams (example data)",
            wide: true,
            items: {
                let mut items = entry_items(crate::diagrams::control_set());
                items.push(item(
                    "interactive inspection",
                    &Planned::new().note("interactive inspection of these diagrams"),
                ));
                items
            },
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
        for s in [
            "PLANNED",
            "No policies yet.",
            "v-code-prompt",
            "data-layer=\"who\"",
            "v-diff-changed",
            "No changes",
            "v-check-fail",
            "<details class=\"v-audit-why\">",
            "role=\"progressbar\"",
            "v-timeline",
            "Egress IP",
            "never seen",
            "v-layers",
            "v-compare",
            "v-branch",
            "v-seq",
            "v-contour",
            "v-reveal",
            "v-compare-side",
            "skimasque connect",
            "YOUR VPC",
            "v-mountain",
            "v-route",
            "v-trail",
            "v-run-card",
            "gw-eu-west",
            "4 identities",
            "interactive inspection",
            "Public diagrams",
            "Control-plane diagrams",
            "Motifs",
            "Motion",
        ] {
            assert!(page.contains(s), "{s}");
        }
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
            "HEALTHY",
            "DEGRADED",
            "OFFLINE",
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
        assert!(
            !page.contains("<script>alert"),
            "hostile strings are escaped"
        );
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
