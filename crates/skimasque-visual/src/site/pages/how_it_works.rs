//! How it works (`/how-it-works`): canonical spec §12.

use crate::diagrams::public;
use crate::site::Page;
use crate::{
    CodeExample, Component, ConnKind, Connection, Flow, Hero, Node, NodeKind, Prose, Section,
    SitePage, Status, WorkflowDemo,
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

    let walkthrough = Section::new("Follow a deployment from request to expiry")
        .push(&Prose::new().p("In this example, a GitHub Actions job needs temporary access to one private database. Play the walkthrough to follow the decision and the resulting traffic path, or read each step at your own pace."))
        .push(&WorkflowDemo::new());

    let identify = Section::new("Step 1 — Identify")
        .push(&Prose::new().p("A workload requests access to a destination. SkiMasque identifies who is asking and which application needs the connection."))
        .push(&CodeExample::new(
            "Example",
            "repository: acme/widget\nworkflow: deploy-production\nref: main\napplication: terraform",
        ));
    let authenticate = Section::new("Step 2 — Authenticate").alt().push(
        &Prose::new()
            .p("SkiMasque verifies the presented identity using the configured identity source.")
            .p("For GitHub Actions, workload identity/OIDC establishes which repository and workflow is making the request. Authentication establishes the identity; policy determines its access."),
    );
    let authorize = Section::new("Step 3 — Authorize")
        .push(&Prose::new().p("SkiMasque evaluates the identity, application, and requested destination against policy."))
        .push(&CodeExample::new(
            "Policy",
            "WHO\nacme/widget\n\nWHAT\nterraform\n\nWHERE\ndb.prod:5432",
        ))
        .push(&Prose::new().p("A matching allow rule grants the requested access within its limits. With no matching allow rule, the request is denied before a session is opened."));
    let session = Section::new("Step 4 — Establish a session")
        .alt()
        .push(&Prose::new().p("If authorized, SkiMasque creates a short-lived session for the permitted destination. The 20-minute duration and bandwidth below are illustrative policy limits."))
        .push(&CodeExample::new(
            "Example",
            "SESSION\nStatus: Active\nDestination: db.prod:5432\nDuration: 20m\nBandwidth: 100 Mbps\nGateway: us-west",
        ));
    let connect = Section::new("Step 5 — Connect")
        .push(
            &Prose::new()
                .p("The control plane authorizes the session. Traffic flows through the SkiMasque gateway to the permitted service in the customer's network."),
        )
        .push(&public::gateway());
    let expire = Section::new("Step 6 — Expire")
        .alt()
        .push(&Prose::new().p("When the session expires or ends, that session's access closes. A later connection requires a new request and another policy decision."))
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
        .push(&walkthrough)
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
