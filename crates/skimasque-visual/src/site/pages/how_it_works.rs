//! How it works (`/how-it-works`): canonical spec §12.

use crate::diagrams::public;
use crate::site::Page;
use crate::{
    CodeExample, Component, ConnKind, Connection, Flow, Hero, Node, NodeKind, Prose, Section,
    SitePage, Status,
};

fn overview() -> Flow {
    let conn = |k| Connection::new(k);
    Flow::new(
        "A workload presents an identity, is authenticated and authorized, receives a session, \
         and reaches the private network through the gateway.",
    )
    .then(&Node::new(NodeKind::Workload).label("WORKLOAD"))
    .via(
        conn(ConnKind::Control),
        &Node::new(NodeKind::Identity).label("IDENTITY"),
    )
    .via(
        conn(ConnKind::Control),
        &Node::new(NodeKind::Credential).label("AUTHENTICATE"),
    )
    .via(
        conn(ConnKind::Control),
        &Node::new(NodeKind::Policy).label("AUTHORIZE"),
    )
    .via(
        conn(ConnKind::Active),
        &Node::new(NodeKind::Session)
            .label("SESSION")
            .status(Status::Active),
    )
    .via(
        conn(ConnKind::Active),
        &Node::new(NodeKind::Gateway).label("GATEWAY"),
    )
    .via(
        conn(ConnKind::Active),
        &Node::new(NodeKind::Network).label("PRIVATE NETWORK"),
    )
}

pub fn page() -> Page {
    let hero = Hero::new("From identity to network access.")
        .lead(
            "SkiMasque turns an authenticated identity into a short-lived, policy-controlled \
             network session.",
        )
        .aside(&overview());

    let identify = Section::new("Step 1 — Identify")
        .push(&Prose::new().p("SkiMasque establishes who or what is making the request."))
        .push(&CodeExample::new(
            "Example",
            "repository: acme/widget\nworkflow: deploy-production\nref: main\napplication: terraform",
        ));
    let authenticate = Section::new("Step 2 — Authenticate").alt().push(
        &Prose::new()
            .p("The identity is verified using the configured identity source.")
            .p("For GitHub Actions, this can use workload identity/OIDC."),
    );
    let authorize = Section::new("Step 3 — Authorize")
        .push(&Prose::new().p("The identity is evaluated against policy."))
        .push(&CodeExample::new(
            "Policy",
            "WHO\nacme/widget\n\nWHAT\nterraform\n\nWHERE\ndb.prod:5432",
        ))
        .push(&Prose::new().p("The policy determines whether the requested access is allowed."));
    let session = Section::new("Step 4 — Establish a session")
        .alt()
        .push(&Prose::new().p("If authorized, SkiMasque creates a short-lived network session."))
        .push(&CodeExample::new(
            "Example",
            "SESSION\nStatus: Active\nDestination: db.prod:5432\nDuration: 20m\nBandwidth: 100 Mbps\nGateway: us-west",
        ));
    let connect = Section::new("Step 5 — Connect")
        .push(
            &Prose::new()
                .p("Traffic flows through the SkiMasque gateway into the customer's network."),
        )
        .push(&public::gateway());
    let expire = Section::new("Step 6 — Expire")
        .alt()
        .push(&Prose::new().p("When the session ends, access disappears."))
        .push(&public::access_lifecycle())
        .push(
            &Prose::new()
                .p("No manual VPN disconnect.")
                .p("No standing session."),
        );

    Page {
        path: "how-it-works/index.html",
        contents: SitePage::new(
            "../",
            "how-it-works",
            "How It Works · SkiMasque",
            "SkiMasque turns an authenticated identity into a short-lived, policy-controlled network session.",
        )
        .push(&hero)
        .push(&identify)
        .push(&authenticate)
        .push(&authorize)
        .push(&session)
        .push(&connect)
        .push(&expire)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn how_it_works_follows_the_canonical_spec() {
        let p = page();
        assert_eq!(p.path, "how-it-works/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        assert!(s.contains("From identity to network access."));
        let mut at = 0;
        for step in [
            "Step 1 — Identify",
            "Step 2 — Authenticate",
            "Step 3 — Authorize",
            "Step 4 — Establish a session",
            "Step 5 — Connect",
            "Step 6 — Expire",
        ] {
            let i = s[at..]
                .find(step)
                .unwrap_or_else(|| panic!("missing or out of order: {step}"));
            at += i + step.len();
        }
        for want in [
            "WORKLOAD",
            "PRIVATE NETWORK",
            "repository: acme/widget",
            "application: terraform",
            "Status: Active",
            "Destination: db.prod:5432",
            "Gateway: us-west",
            "No manual VPN disconnect.",
            "No standing session.",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
        assert!(s.contains(r#"aria-current="page""#));
    }
}
