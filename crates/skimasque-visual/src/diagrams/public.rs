//! Public diagrams, part A: the access model, comparisons, the lifecycle
//! and the integrations. Examples use the marketing data set.

use crate::{
    Boundary, Branch, Compare, ConnKind, Connection, Flow, Node, NodeKind, Reveal, Status, Tone,
};

fn active() -> Connection {
    Connection::new(ConnKind::Active)
}
fn control(label: &str) -> Connection {
    Connection::new(ConnKind::Control).label(label)
}

pub fn identity_policy_access() -> Flow {
    Flow::new(
        "A workload proves its identity, SkiMasque checks the policy, and an active session \
         carries its traffic to the private service.",
    )
    .then(&Node::new(NodeKind::Workload).sub("acme/widget"))
    .via(control("proves identity"), &Node::new(NodeKind::Identity))
    .via(
        control("checked"),
        &Node::new(NodeKind::Policy).status(Status::Granted),
    )
    .via(
        active(),
        &Node::new(NodeKind::Session)
            .sub("20 min")
            .status(Status::Active),
    )
    .via(active(), &Node::new(NodeKind::Service).sub("db.prod:5432"))
}

pub fn policy_model() -> Flow {
    Flow::new(
        "A policy answers four questions: who is asking, what they are running, where they want \
         to go, and the limits on the access. The answer is network access.",
    )
    .then(
        &Node::new(NodeKind::Identity)
            .label("WHO")
            .sub("acme/widget · deploy-production"),
    )
    .then(
        &Node::new(NodeKind::Application)
            .label("WHAT")
            .sub("terraform"),
    )
    .then(
        &Node::new(NodeKind::Database)
            .label("WHERE")
            .sub("db.prod:5432"),
    )
    .then(
        &Node::new(NodeKind::Policy)
            .label("LIMITS")
            .sub("20m · 100 Mbps · us-west"),
    )
    .via(
        active(),
        &Node::new(NodeKind::Session)
            .label("NETWORK ACCESS")
            .status(Status::Granted),
    )
}

pub fn traditional_vs_skimasque() -> Compare {
    let traditional =
        Flow::new("Traditional access: a CI job connects through a VPN to the whole network.")
            .then(&Node::new(NodeKind::CiJob))
            .via(
                Connection::new(ConnKind::Potential),
                &Node::new(NodeKind::Network).label("VPN"),
            )
            .via(
                Connection::new(ConnKind::Potential),
                &Node::new(NodeKind::Network).label("Whole network"),
            )
            .via(
                Connection::new(ConnKind::Potential),
                &Node::new(NodeKind::Service).label("Everything on it"),
            );
    let skimasque = Flow::new(
        "SkiMasque access: a CI job proves an identity, a policy allows one destination, and a \
         temporary session carries the traffic.",
    )
    .then(&Node::new(NodeKind::CiJob))
    .via(control("proves identity"), &Node::new(NodeKind::Identity))
    .via(control("checked"), &Node::new(NodeKind::Policy))
    .via(
        active(),
        &Node::new(NodeKind::Service)
            .label("One destination")
            .sub("db.prod:5432"),
    )
    .via(
        active(),
        &Node::new(NodeKind::Session)
            .sub("expires")
            .status(Status::Active),
    );
    Compare::new("Traditional access reaches a whole network; SkiMasque reaches one destination for a limited time.")
        .side("Traditional access", Tone::Neutral, &traditional)
        .side("SkiMasque", Tone::Active, &skimasque)
}

pub fn access_lifecycle() -> Flow {
    Flow::new("An access request is authenticated, authorized, connected, active for a limited time, and then expires.")
        .then(&Node::new(NodeKind::Workload).label("REQUEST"))
        .via(control("identity"), &Node::new(NodeKind::Identity).label("AUTHENTICATE"))
        .via(control("policy"), &Node::new(NodeKind::Policy).label("AUTHORIZE"))
        .via(active(), &Node::new(NodeKind::Gateway).label("CONNECT"))
        .via(active(), &Node::new(NodeKind::Session).label("ACTIVE").status(Status::Active))
        .via(Connection::new(ConnKind::Potential), &Node::new(NodeKind::Session).label("EXPIRE").status(Status::Expired))
}

pub fn policy_decision() -> Reveal {
    let request = Flow::new("A request is evaluated by the policy.")
        .then(&Node::new(NodeKind::Workload).label("Request"))
        .via(control("evaluated"), &Node::new(NodeKind::Policy));
    Reveal::new(
        &Branch::new("If a policy matches the request, access is allowed. With no matching allow rule the request is denied.", &request)
            .arm(active().label("match"), &Node::new(NodeKind::Allow))
            .arm(
                Connection::new(ConnKind::Denied).label("no match"),
                &Node::new(NodeKind::Deny).sub("No matching allow rule means DENY."),
            ),
    )
}

pub fn github_actions() -> Flow {
    Flow::new(
        "A GitHub Actions job proves its identity with OIDC, SkiMasque evaluates the policy, and \
         an active session carries its traffic through the gateway to the private database.",
    )
    .then(&Node::new(NodeKind::GitHub).sub("acme/widget"))
    .via(control("OIDC"), &Node::new(NodeKind::Identity))
    .via(
        control("policy"),
        &Node::new(NodeKind::Policy).status(Status::Granted),
    )
    .then(&Node::new(NodeKind::Session).sub("20 min"))
    .via(active(), &Node::new(NodeKind::Gateway))
    .via(active(), &Node::new(NodeKind::Database).sub("db.prod:5432"))
}

pub fn developer_cli() -> Flow {
    Flow::new(
        "A developer runs skimasque connect, the CLI proves who they are, the policy is checked, \
         and a temporary session reaches the private service.",
    )
    .then(&Node::new(NodeKind::Developer))
    .via(
        Connection::new(ConnKind::Normal),
        &Node::new(NodeKind::Cli).sub("skimasque connect db.prod:5432"),
    )
    .via(control("proves identity"), &Node::new(NodeKind::Identity))
    .via(control("checked"), &Node::new(NodeKind::Policy))
    .via(
        active(),
        &Node::new(NodeKind::Session)
            .sub("temporary")
            .status(Status::Active),
    )
    .via(active(), &Node::new(NodeKind::Service).sub("db.prod:5432"))
}

pub fn same_command_different_policy() -> Branch {
    let staging = Flow::new("The staging policy reaches the staging database.")
        .then(&Node::new(NodeKind::Policy).label("staging policy"))
        .via(
            active(),
            &Node::new(NodeKind::Database).sub("db.staging:5432"),
        );
    let production =
        Flow::new("The production policy reaches the production database with tighter limits.")
            .then(
                &Node::new(NodeKind::Policy)
                    .label("production policy")
                    .sub("10 min"),
            )
            .via(active(), &Node::new(NodeKind::Database).sub("db.prod:5432"));
    Branch::new(
        "The same command reaches different destinations depending on which policy applies.",
        &Node::new(NodeKind::Cli).sub("skimasque connect"),
    )
    .arm(control("staging identity"), &staging)
    .arm(control("production identity"), &production)
}

pub fn gateway() -> Flow {
    Flow::new(
        "The control plane tells the gateway which sessions are allowed; the gateway forwards \
         traffic only to the private services in your network.",
    )
    .then(&Node::new(NodeKind::ControlPlane))
    .via(
        control("sessions and policy"),
        &Node::new(NodeKind::Gateway),
    )
    .via(
        active(),
        &Boundary::region("YOUR NETWORK")
            .child(&Node::new(NodeKind::Database))
            .child(&Node::new(NodeKind::Api))
            .child(&Node::new(NodeKind::Kubernetes)),
    )
}

pub fn customer_vpc() -> Flow {
    Flow::new(
        "A session enters your VPC through the gateway, which forwards it to the database inside.",
    )
    .then(&Node::new(NodeKind::Developer))
    .via(
        active().label("session"),
        &Boundary::region("YOUR VPC")
            .child(&Node::new(NodeKind::Gateway))
            .child(&Node::new(NodeKind::Database).sub("db.prod:5432")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    #[test]
    fn the_policy_decision_ends_in_allow_and_a_denial_with_its_rule() {
        let s = policy_decision().html().as_str().to_owned();
        assert!(s.contains("v-reveal") && s.contains("Allow") && s.contains("Deny"));
        assert!(s.contains("No matching allow rule means DENY."));
        assert!(s.contains("v-conn-active") && s.contains("v-conn-denied"));
    }

    #[test]
    fn traditional_access_and_skimasque_are_two_sides() {
        let s = traditional_vs_skimasque().html().as_str().to_owned();
        assert_eq!(s.matches("v-compare-side").count(), 2);
        assert!(s.contains("VPN") && s.contains("Identity") && s.contains("Policy"));
    }

    #[test]
    fn the_lifecycle_runs_request_to_expire_in_order() {
        let s = access_lifecycle().html().as_str().to_owned();
        let pos = |w: &str| s.find(w).unwrap_or_else(|| panic!("{w}"));
        let order = [
            "REQUEST",
            "AUTHENTICATE",
            "AUTHORIZE",
            "CONNECT",
            "ACTIVE",
            "EXPIRE",
        ];
        for w in order.windows(2) {
            assert!(pos(w[0]) < pos(w[1]), "{} before {}", w[0], w[1]);
        }
        assert!(
            s.contains("EXPIRED"),
            "the last step carries the expired status word"
        );
    }

    #[test]
    fn the_developer_cli_uses_connect_never_exec() {
        let s = developer_cli().html().as_str().to_owned();
        assert!(s.contains("skimasque connect") && !s.to_lowercase().contains("exec"));
        let b = same_command_different_policy().html().as_str().to_owned();
        assert!(
            b.contains("staging") && b.contains("production") && b.contains("skimasque connect")
        );
    }

    #[test]
    fn the_gateway_diagram_describes_egress_in_words_only() {
        let s = gateway().html().as_str().to_owned();
        assert!(
            s.contains("v-conn-control")
                && s.contains("v-boundary-region")
                && s.contains("Database")
        );
    }
}
