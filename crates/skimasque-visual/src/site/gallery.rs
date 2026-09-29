//! The pages `sitegen` writes under the website's `site/` directory. For now
//! one page: the component gallery, every primitive in every state, shown in
//! a dark and a light section so both themes can be checked without JS.

use askama::Template;

use super::Page;
use crate::{
    AuditEventCard, Boundary, Change, Check, CodeExample, ComparisonTable, Component, ConnKind,
    Connection, Contour, Cta, CtaBand, DecisionBadge, DecisionCard, DecisionExplainer, EmptyState,
    Expire, Faq, FeatureGrid, Flow, GatewayCard, HealthCard, Hero, Html, Icons, IdentityCard,
    Mountain, Node, NodeKind, Planned, PlannedBlock, PolicyCard, PolicyDiff, PolicyExplorer,
    PolicySummary, Prose, Reveal, Route, Run, RunCard, Section, SessionCard, SessionTimeline,
    Shape, Status, StatusBadge, TierCard, TimelineEvent, Tone, TrailMarker, DIMENSIONS,
};

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
                        &[
                            "Control plane: SkiMasque",
                            "Gateway: Customer",
                            "The gateway lives inside your network",
                        ],
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
            title: "Site content",
            wide: true,
            items: vec![
                item(
                    "hero",
                    &Hero::new("Give every workload exactly the network access it needs.")
                        .eyebrow("Identity-aware network access")
                        .lead("Short-lived network access, granted by policy.")
                        .cta(Cta::primary("Get Started", "#"))
                        .cta(Cta::secondary("See How It Works", "#"))
                        .aside(&Node::new(NodeKind::Gateway)),
                ),
                item(
                    "section",
                    &Section::new("Product model")
                        .eyebrow("Concepts")
                        .alt()
                        .push(
                            &Prose::new()
                                .lead("A workload asks; policy decides.")
                                .p("Access is granted per session and expires.")
                                .sub("Destinations")
                                .list(&["A destination is a name", "A gateway reaches it"])
                                .quote("Network access should be temporary.")
                                .kv("WHO", "acme/widget"),
                        ),
                ),
                item(
                    "feature grid",
                    &FeatureGrid::new()
                        .feature("Identity-aware", "Know who is asking for access.")
                        .planned(
                            "Command wrapper",
                            "Run a command with access.",
                            "command wrapper (planned)",
                        ),
                ),
                item(
                    "comparison table",
                    &ComparisonTable::new(&["Approach", "Model"])
                        .row(&["Network tunnel", "Network"])
                        .row(&["SkiMasque", "Capability"])
                        .highlight_last(),
                ),
                item(
                    "faq",
                    &Faq::new().item(
                        "What does a policy decide?",
                        &["Who may reach which destination.", "And for how long."],
                    ),
                ),
                item(
                    "tier cards",
                    &Section::new("Plans")
                        .push(
                            &TierCard::new("Free", "$0", "For evaluation.")
                                .include("Core policies")
                                .live(),
                        )
                        .push(
                            &TierCard::new("Team", "$49 / month", "Small teams.")
                                .include("More gateways")
                                .planned("paid plans"),
                        ),
                ),
                item(
                    "cta band",
                    &CtaBand::new("Network access should be temporary.")
                        .line("Give it to them, then take it back.")
                        .cta(Cta::primary("Create Your First Policy", "#")),
                ),
                item(
                    "planned block",
                    &PlannedBlock::new(
                        "command wrapper (planned)",
                        &Prose::new().p("Run a command with short-lived access."),
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

pub(super) fn pages() -> Vec<Page> {
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
        // only public pages must have exactly one h1; the gallery shows a demo hero per theme
        assert_eq!(
            page.matches("<h1").count(),
            3,
            "page h1 plus one demo hero per theme"
        );
        assert_eq!(
            page.matches("<h2").count(),
            8,
            "theme h2 plus demo section, plans section and cta band per theme"
        );
        for s in [
            "v-hero",
            "v-features",
            "v-table",
            "v-faq",
            "v-tier",
            "v-cta-band",
            "v-planned-block",
        ] {
            assert!(page.contains(s), "{s}");
        }
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
}
