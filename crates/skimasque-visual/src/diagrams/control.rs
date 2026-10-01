//! Control-plane diagrams: the same visual language as the public set, filled
//! with the organisation's real data. Nothing is invented — a diagram states
//! only the identities, policies, sessions and gateways it is handed.
//! Interactive inspection of these diagrams is Planned.

use crate::{Branch, ConnKind, Connection, Flow, Node, NodeKind, Status};

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

#[derive(Debug, Clone, Copy)]
pub struct GatewayRef<'a> {
    pub name: &'a str,
    pub region: &'a str,
    pub status: Status,
}

/// An identity and the policy (if any) that lets it in.
pub fn identity_flow(source: &str, identity: &str, policy: Option<&str>) -> Flow {
    let who = Node::new(NodeKind::Identity)
        .label(identity.to_owned())
        .sub(source.to_owned());
    match policy {
        Some(p) => Flow::new(format!(
            "{identity} from {source} matches the policy {p}, so access is granted."
        ))
        .then(&who)
        .via(
            Connection::new(ConnKind::Control).label("matches"),
            &Node::new(NodeKind::Policy)
                .label(p.to_owned())
                .status(Status::Granted),
        ),
        None => Flow::new(format!(
            "{identity} from {source} matches no policy, so access is denied."
        ))
        .then(&who)
        .via(
            Connection::new(ConnKind::Denied),
            &Node::new(NodeKind::Deny)
                .label("No matching policy")
                .status(Status::Denied),
        ),
    }
}

/// One session: who, through which gateway, to what, and its real status.
pub fn session_flow(identity: &str, target: &str, gateway: &str, status: Status) -> Flow {
    let conn = if status == Status::Active {
        ConnKind::Active
    } else {
        ConnKind::Potential
    };
    Flow::new(format!(
        "{identity}'s session ({}) goes through {gateway} to {target}.",
        status.word().to_lowercase()
    ))
    .then(&Node::new(NodeKind::Identity).label(identity.to_owned()))
    .via(
        Connection::new(conn),
        &Node::new(NodeKind::Session).status(status),
    )
    .via(
        Connection::new(conn),
        &Node::new(NodeKind::Gateway).label(gateway.to_owned()),
    )
    .via(
        Connection::new(conn),
        &Node::new(NodeKind::Service).label(target.to_owned()),
    )
}

/// The control plane and the gateways connected to it, each with its health.
pub fn gateway_topology(gateways: &[GatewayRef]) -> Branch {
    let caption = if gateways.is_empty() {
        "No gateways connected.".to_owned()
    } else {
        format!(
            "The control plane manages {}.",
            plural(gateways.len() as u32, "gateway", "gateways")
        )
    };
    let root = Node::new(NodeKind::ControlPlane).sub(if gateways.is_empty() {
        "No gateways connected".to_owned()
    } else {
        plural(gateways.len() as u32, "gateway", "gateways")
    });
    gateways.iter().fold(Branch::new(caption, &root), |b, g| {
        b.arm(
            Connection::new(ConnKind::Control),
            &Node::new(NodeKind::Gateway)
                .label(g.name.to_owned())
                .sub(g.region.to_owned())
                .status(g.status),
        )
    })
}

/// An organisation at a glance, from counts the caller already holds.
pub fn org_topology(org: &str, identities: u32, policies: u32, gateways: u32) -> Flow {
    Flow::new(format!(
        "{org} has {}, {} and {}.",
        plural(identities, "identity", "identities"),
        plural(policies, "policy", "policies"),
        plural(gateways, "gateway", "gateways"),
    ))
    .then(
        &Node::new(NodeKind::Identity)
            .label(plural(identities, "identity", "identities"))
            .sub(org.to_owned()),
    )
    .via(
        Connection::new(ConnKind::Control),
        &Node::new(NodeKind::Policy).label(plural(policies, "policy", "policies")),
    )
    .via(
        Connection::new(ConnKind::Control),
        &Node::new(NodeKind::Gateway).label(plural(gateways, "gateway", "gateways")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Status};

    fn html(c: &impl Component) -> String {
        c.html().as_str().to_owned()
    }

    #[test]
    fn an_identity_with_a_matching_policy_is_granted_and_without_one_is_denied_in_words() {
        let ok = html(&identity_flow(
            "GitHub Actions",
            "acme/widget",
            Some("production-deploy"),
        ));
        assert!(
            ok.contains("acme/widget")
                && ok.contains("production-deploy")
                && ok.contains("ACCESS GRANTED")
        );
        let none = html(&identity_flow("GitHub Actions", "acme/rogue", None));
        assert!(
            none.contains("acme/rogue")
                && none.contains("v-conn-denied")
                && none.contains("No matching policy")
        );
        assert!(!none.contains("ACCESS GRANTED"));
    }

    #[test]
    fn a_session_flow_shows_the_sessions_real_status_word() {
        for (st, word) in [(Status::Active, "ACTIVE"), (Status::Expired, "EXPIRED")] {
            let s = html(&session_flow(
                "acme/widget",
                "db.prod:5432",
                "gw-us-west",
                st,
            ));
            assert!(s.contains(word) && s.contains("gw-us-west") && s.contains("db.prod:5432"));
        }
    }

    #[test]
    fn the_gateway_topology_lists_each_gateway_with_its_status_and_handles_none() {
        let g = [
            GatewayRef {
                name: "gw-us-west",
                region: "us-west-2",
                status: Status::Healthy,
            },
            GatewayRef {
                name: "gw-eu",
                region: "eu-west-1",
                status: Status::Offline,
            },
        ];
        let s = html(&gateway_topology(&g));
        assert_eq!(s.matches("v-branch-arm\"").count(), 2);
        assert!(s.contains("HEALTHY") && s.contains("OFFLINE") && s.contains("gw-eu"));
        let none = html(&gateway_topology(&[]));
        assert!(none.contains("No gateways connected") && none.contains("v-branch-root"));
    }

    #[test]
    fn the_org_topology_states_only_the_counts_it_is_given() {
        let s = html(&org_topology("Acme", 4, 3, 1));
        assert!(
            s.contains("Acme")
                && s.contains("4 identities")
                && s.contains("3 policies")
                && s.contains("1 gateway")
        );
        let one = html(&org_topology("Solo", 1, 1, 2));
        assert!(
            one.contains("1 identity") && one.contains("1 policy") && one.contains("2 gateways")
        );
    }

    #[test]
    fn control_diagram_strings_are_escaped() {
        let s = html(&identity_flow("<b>", "<script>", Some("\"><img>")));
        assert!(!s.contains("<script>") && !s.contains("<img>") && !s.contains("<b>"));
    }
}
