//! Public diagrams, part B: security, architecture, MASQUE, audit and the
//! deployment runs. Examples use the marketing data set; nothing here is an
//! address or a promise the product does not keep.

use crate::{
    Boundary, Branch, Compare, ConnKind, Connection, Flow, Layers, Node, NodeKind, Run, RunCard,
    Sequence, Status, Tone,
};

fn active() -> Connection {
    Connection::new(ConnKind::Active)
}
fn control(label: &str) -> Connection {
    Connection::new(ConnKind::Control).label(label)
}

pub fn ci_lifecycle() -> Flow {
    Flow::new("A CI job starts, proves its identity, gets a session, deploys, completes, and its session expires.")
        .then(&Node::new(NodeKind::CiJob).label("JOB START"))
        .via(control("OIDC"), &Node::new(NodeKind::Identity).label("IDENTITY"))
        .via(control("policy"), &Node::new(NodeKind::Session).label("SESSION CREATED").status(Status::Active))
        .via(active(), &Node::new(NodeKind::Service).label("DEPLOYMENT"))
        .via(Connection::new(ConnKind::Normal), &Node::new(NodeKind::CiJob).label("JOB COMPLETE"))
        .via(Connection::new(ConnKind::Potential), &Node::new(NodeKind::Session).label("SESSION EXPIRED").status(Status::Expired))
}

pub fn security_layers() -> Layers {
    Layers::new("Security is layered: identity, policy, session, gateway and network each rest on the layer below.")
        .upward()
        .row_sub("IDENTITY", "who is asking")
        .row_sub("POLICY", "what they may reach")
        .row_sub("SESSION", "for how long")
        .row_sub("GATEWAY", "the path in")
        .row_sub("NETWORK", "your private network")
}

pub fn no_standing_access() -> Compare {
    let standing =
        Flow::new("A stored credential gives standing access to the network at all times.")
            .then(&Node::new(NodeKind::Credential))
            .via(
                Connection::new(ConnKind::Normal).label("standing access"),
                &Node::new(NodeKind::Network),
            );
    let temporary = Flow::new(
        "An identity is checked against policy, receives a session, and the session expires.",
    )
    .then(&Node::new(NodeKind::Identity))
    .via(control("policy"), &Node::new(NodeKind::Policy))
    .via(
        active(),
        &Node::new(NodeKind::Session).status(Status::Active),
    )
    .via(
        Connection::new(ConnKind::Potential).label("expires"),
        &Node::new(NodeKind::Session)
            .label("Session")
            .status(Status::Expired),
    );
    Compare::new(
        "A stored credential can keep working until it is revoked; a SkiMasque session expires.",
    )
    .side("Standing access", Tone::Neutral, &standing)
    .side("No standing access", Tone::Active, &temporary)
}

pub fn compartmentalisation() -> Compare {
    let job = |name: &str, allowed: NodeKind, sub: &str| {
        Branch::new(
            format!("{name} may reach one destination; everything else is denied."),
            &Node::new(NodeKind::CiJob).label(name),
        )
        .arm(active().label("allowed"), &Node::new(allowed).sub(sub))
        .arm(
            Connection::new(ConnKind::Denied).label("everything else"),
            &Node::new(NodeKind::Deny).sub("DENY"),
        )
    };
    Compare::new("Each job is allowed one destination; anything else is denied by policy.")
        .side(
            "Job A",
            Tone::Active,
            &job("Job A", NodeKind::Database, "db.prod:5432"),
        )
        .side(
            "Job B",
            Tone::Active,
            &job("Job B", NodeKind::Api, "api.internal:443"),
        )
}

pub fn control_data_plane() -> Flow {
    Flow::new("The control plane decides who may connect; the data plane carries the traffic through the gateway.")
        .then(
            &Boundary::region("CONTROL PLANE")
                .child(&Node::new(NodeKind::Identity))
                .child(&Node::new(NodeKind::Policy)),
        )
        .via(
            control("sessions"),
            &Boundary::region("DATA PLANE").child(&Node::new(NodeKind::Gateway)),
        )
        .via(active(), &Node::new(NodeKind::Network))
}

pub fn architecture() -> Flow {
    Flow::new(
        "Developers and CI/CD jobs authenticate with the control plane, which authorizes a \
         session; the MASQUE gateway then forwards traffic into the private network.",
    )
    .then(&Node::new(NodeKind::Developer).label("Developer or CI/CD"))
    .via(
        control("authenticate"),
        &Node::new(NodeKind::ControlPlane).sub("identity · policy · sessions"),
    )
    .via(
        control("session"),
        &Node::new(NodeKind::Session).label("Session authorization"),
    )
    .via(
        active(),
        &Node::new(NodeKind::Gateway).label("MASQUE gateway"),
    )
    .via(active(), &Node::new(NodeKind::Network))
}

pub fn masque_stack() -> Layers {
    Layers::new("Traffic rides MASQUE over HTTP/3, which runs on QUIC over UDP/IP.")
        .row("Application")
        .row_sub("MASQUE", "CONNECT-UDP")
        .row("HTTP/3")
        .row("QUIC")
        .row("UDP/IP")
}

pub fn connect_udp_sequence() -> Sequence {
    Sequence::new("The client asks the gateway for a CONNECT-UDP tunnel, the gateway confirms it, and datagrams flow both ways.", "Client", "Gateway")
        .to_right("CONNECT-UDP request")
        .to_left("tunnel established")
        .to_right("UDP datagrams")
        .to_left("UDP datagrams")
}

pub fn multiple_gateways() -> Branch {
    let vpc = |region: &str| {
        Boundary::region(format!("VPC · {region}"))
            .child(&Node::new(NodeKind::Gateway).sub(region.to_owned()))
    };
    Branch::new(
        "One control plane manages several gateways, each inside its own network.",
        &Node::new(NodeKind::ControlPlane),
    )
    .arm(control("us-west"), &vpc("us-west"))
    .arm(control("us-east"), &vpc("us-east"))
    .arm(control("eu-west"), &vpc("eu-west"))
}

pub fn audit_flow() -> Flow {
    Flow::new(
        "Access decisions, granted or denied, are recorded in the audit log, where they can be reviewed.",
    )
    .then(&Node::new(NodeKind::Workload).label("Request"))
    .via(control("evaluated"), &Node::new(NodeKind::Policy))
    .via(
        Connection::new(ConnKind::Normal).label("decision"),
        &Node::new(NodeKind::Audit),
    )
    .via(
        Connection::new(ConnKind::Normal).label("reviewed"),
        &Node::new(NodeKind::Developer).label("Audit page"),
    )
}

pub fn deployment_models() -> Compare {
    let green = RunCard::new(
        Run::Green,
        "SkiMasque Cloud",
        &[
            "Control plane: SkiMasque",
            "Gateway: SkiMasque",
            "Operations: minimal",
            "Free tier",
        ],
    )
    .planned("paid plans");
    let blue = RunCard::new(
        Run::Blue,
        "Your Gateway",
        &[
            "Control plane: SkiMasque",
            "Gateway: Customer",
            "The gateway lives inside your network",
        ],
    );
    let black = RunCard::new(
        Run::Black,
        "Self-hosted",
        &[
            "Control plane: Customer",
            "Gateway: Customer",
            "Operations: Customer",
        ],
    );
    Compare::new("Three ways to run SkiMasque: hosted, with your own gateway, or fully self-hosted. Each is named by run and by what it is.")
        .side("", Tone::Active, &green)
        .side("", Tone::Info, &blue)
        .side("", Tone::Neutral, &black)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    fn html(c: &impl Component) -> String {
        c.html().as_str().to_owned()
    }

    #[test]
    fn the_ci_lifecycle_runs_job_start_to_session_expired() {
        let s = html(&ci_lifecycle());
        let order = [
            "JOB START",
            "IDENTITY",
            "SESSION CREATED",
            "DEPLOYMENT",
            "JOB COMPLETE",
            "SESSION EXPIRED",
        ];
        for w in order.windows(2) {
            assert!(
                s.find(w[0]).unwrap() < s.find(w[1]).unwrap(),
                "{} before {}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn security_layers_rest_on_each_other_identity_to_network() {
        let s = html(&security_layers());
        assert!(s.contains("v-layers-up"));
        for l in ["IDENTITY", "POLICY", "SESSION", "GATEWAY", "NETWORK"] {
            assert!(s.contains(l), "{l}");
        }
    }

    #[test]
    fn no_standing_access_contrasts_a_credential_with_an_expiring_session() {
        let s = html(&no_standing_access());
        assert!(s.contains("Credential") && s.contains("EXPIRED") && s.contains("standing access"));
    }

    #[test]
    fn compartmentalisation_allows_one_destination_per_job_and_denies_the_rest() {
        let s = html(&compartmentalisation());
        assert_eq!(s.matches("v-compare-side").count(), 2);
        assert!(
            s.matches("v-conn-denied").count() >= 2 && (s.contains("DENY") || s.contains("Deny"))
        );
    }

    #[test]
    fn the_masque_stack_and_sequence_use_connect_udp_only() {
        let s = html(&masque_stack());
        let order = ["Application", "MASQUE", "HTTP/3", "QUIC", "UDP/IP"];
        for w in order.windows(2) {
            assert!(s.find(w[0]).unwrap() < s.find(w[1]).unwrap());
        }
        let q = html(&connect_udp_sequence());
        assert!(q.contains("CONNECT-UDP") && !q.contains("CONNECT-IP"));
    }

    #[test]
    fn multiple_gateways_hang_off_one_control_plane() {
        let s = html(&multiple_gateways());
        for r in ["us-west", "us-east", "eu-west"] {
            assert!(s.contains(r), "{r}");
        }
        assert_eq!(s.matches("v-branch-arm\"").count(), 3);
    }

    #[test]
    fn deployment_models_always_show_the_technical_name_and_mark_unbuilt_paid_plans_planned() {
        let s = html(&deployment_models());
        for (run, tech) in [
            ("Green Run", "SkiMasque Cloud"),
            ("Blue Run", "Your Gateway"),
            ("Black Run", "Self-hosted"),
        ] {
            assert!(s.contains(run) && s.contains(tech), "{run}/{tech}");
        }
        for line in [
            "Control plane: SkiMasque",
            "Gateway: SkiMasque",
            "Operations: minimal",
            "Gateway: Customer",
            "The gateway lives inside your network",
            "Control plane: Customer",
            "Operations: Customer",
        ] {
            assert!(s.contains(line), "{line}");
        }
        assert!(
            s.contains("Free tier"),
            "the hosted run is live with a free tier"
        );
        assert_eq!(
            s.matches("PLANNED").count(),
            1,
            "only paid plans are planned (billing is not in place)"
        );
        assert!(
            s.contains("paid plans <span class=\"v-planned\">") && !s.contains("v-sr\"> — paid"),
            "the note is visible beside the marker"
        );
    }

    #[test]
    fn architecture_control_data_plane_and_audit_are_captioned_flows() {
        for s in [
            html(&architecture()),
            html(&control_data_plane()),
            html(&audit_flow()),
        ] {
            assert!(s.contains("v-flow"));
        }
        assert!(
            html(&control_data_plane()).contains("CONTROL PLANE")
                && html(&control_data_plane()).contains("DATA PLANE")
        );
        assert!(html(&audit_flow()).contains("Audit log"));
    }
}
