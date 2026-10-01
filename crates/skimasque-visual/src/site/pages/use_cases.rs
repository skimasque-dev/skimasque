//! Use cases (`/use-cases`): canonical spec §23. Developer debugging shows
//! `skimasque connect`, the real command for one tunnel.

use crate::site::Page;
use crate::{
    Component, ConnKind, Connection, Flow, Hero, Node, NodeKind, PolicySummary, Prose, Section,
    SitePage,
};

/// A local flow: each step is joined to the next by an active connection.
fn flow(caption: &str, steps: &[(NodeKind, &str)]) -> Flow {
    let mut it = steps.iter();
    let (k, l) = it.next().expect("at least one step");
    let mut f = Flow::new(caption).then(&Node::new(*k).label(*l));
    for (k, l) in it {
        f = f.via(Connection::new(ConnKind::Active), &Node::new(*k).label(*l));
    }
    f
}

pub fn page() -> Page {
    let hero = Hero::new("Wherever workloads need temporary access to private infrastructure.");

    let terraform = Section::new("Terraform")
        .push(&flow(
            "Terraform reaches the production database.",
            &[
                (NodeKind::Application, "Terraform"),
                (NodeKind::Database, "Production database"),
            ],
        ))
        .push(&PolicySummary::new(
            "acme/widget · main",
            "terraform",
            "db.prod:5432",
            "20m",
        ));

    let apis = Section::new("Private APIs").alt().push(&flow(
        "A CI job reaches a private API.",
        &[
            (NodeKind::CiJob, "CI job"),
            (NodeKind::Api, "private-api.internal:443"),
        ],
    ));

    let k8s = Section::new("Kubernetes").push(&flow(
        "A deployment workflow reaches the Kubernetes API.",
        &[
            (NodeKind::CiJob, "Deployment workflow"),
            (NodeKind::Kubernetes, "Kubernetes API"),
        ],
    ));

    let migrations = Section::new("Database migrations")
        .alt()
        .push(&flow(
            "A migration job reaches the database.",
            &[
                (NodeKind::CiJob, "migration job"),
                (NodeKind::Database, "database:5432"),
            ],
        ))
        .push(
            &Prose::new()
                .p("Access can be limited to:")
                .quote("20 minutes"),
        );

    let debugging = Section::new("Developer debugging").push(&flow(
        "A developer runs skimasque connect to reach a private service.",
        &[
            (NodeKind::Developer, "developer"),
            (NodeKind::Cli, "skimasque connect"),
            (NodeKind::Service, "private service"),
        ],
    ));

    let automation = Section::new("Infrastructure automation").alt().push(&flow(
        "CI runs Terraform or Pulumi against private infrastructure.",
        &[
            (NodeKind::CiJob, "CI"),
            (NodeKind::Application, "Terraform / Pulumi"),
            (NodeKind::Network, "Private infrastructure"),
        ],
    ));

    Page {
        path: "use-cases/index.html",
        contents: SitePage::new(
            "../",
            "use-cases",
            "Use Cases · SkiMasque",
            "Terraform, private APIs, Kubernetes, database migrations, developer debugging and infrastructure automation: temporary access to private infrastructure.",
        )
        .push(&hero)
        .push(&terraform)
        .push(&apis)
        .push(&k8s)
        .push(&migrations)
        .push(&debugging)
        .push(&automation)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_cases_has_six_sections_and_the_real_command() {
        let p = page();
        assert_eq!(p.path, "use-cases/index.html");
        let s = p.contents.replace("&#x27;", "'").replace("&#39;", "'");
        for want in [
            "Terraform",
            "Private APIs",
            "Kubernetes",
            "Database migrations",
            "Developer debugging",
            "Infrastructure automation",
            "db.prod:5432",
            "20m",
            "skimasque connect",
        ] {
            assert!(s.contains(want), "missing {want:?}");
        }
        let main = &s[s.find("<main").unwrap()..s.find("</main>").unwrap()];
        assert_eq!(main.matches("<h2").count(), 6, "one section per use case");
    }
}
