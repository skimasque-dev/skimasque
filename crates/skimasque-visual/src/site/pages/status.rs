//! Status (`/status`): canonical spec §55, amended: static text, no live
//! claims. Services are listed without status words; live status is Planned.

use crate::site::Page;
use crate::{Component, Hero, PlannedBlock, Prose, Section, SitePage};

pub fn page() -> Page {
    let hero = Hero::new("SkiMasque Status");

    let services = Section::new("Services").push(&Prose::new().list(&[
        "Control plane",
        "Gateway service",
        "Authentication",
        "API",
    ]));

    let live = Section::new("Live status").alt().push(&PlannedBlock::new(
        "live service status and incident history",
        &Prose::new().p("Status reporting and historical incidents will appear here."),
    ));

    Page {
        path: "status/index.html",
        contents: SitePage::new(
            "../",
            "status",
            "Status · SkiMasque",
            "The SkiMasque services. Live status reporting and incident history are planned and not available yet.",
        )
        .push(&hero)
        .push(&services)
        .push(&live)
        .html()
        .as_str()
        .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lists_services_with_no_status_claims_and_a_planned_block() {
        let p = page();
        assert_eq!(p.path, "status/index.html");
        let s = &p.contents;
        for want in [
            "SkiMasque Status",
            "Control plane",
            "Gateway service",
            "Authentication",
            "API",
            "PLANNED",
        ] {
            assert!(s.contains(want), "missing {want}");
        }
        for banned in ["Operational", "Degraded", "Outage"] {
            assert!(!s.contains(banned), "{banned}");
        }
        assert!(!s.contains(">Up<"));
    }
}
