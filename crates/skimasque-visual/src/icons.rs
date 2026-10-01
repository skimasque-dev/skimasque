//! The icon sprite: emit once per page, then reference icons with
//! `<svg><use href="#i-…"/></svg>`. Stroke icons drawn in `currentColor`.

use askama::Template;

#[derive(Template, Debug, Clone)]
#[template(path = "icons.html")]
pub struct Icons;

impl crate::Component for Icons {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, NodeKind};

    #[test]
    fn every_node_kind_has_a_symbol_in_the_sprite() {
        let sprite = Icons.html().as_str().to_owned();
        assert!(sprite.contains(r#"class="v-sprite""#));
        for kind in NodeKind::ALL {
            assert!(
                sprite.contains(&format!(r#"<symbol id="{}""#, kind.icon())),
                "{:?} → {}",
                kind,
                kind.icon()
            );
        }
        for extra in ["i-arrow", "i-check", "i-cross", "i-clock"] {
            assert!(
                sprite.contains(&format!(r#"<symbol id="{extra}""#)),
                "{extra}"
            );
        }
    }

    #[test]
    fn kinds_map_to_the_visual_grammar() {
        use crate::Tone::*;
        assert_eq!(NodeKind::Database.tone(), Structure);
        assert_eq!(NodeKind::Gateway.tone(), Edge);
        assert_eq!(NodeKind::Session.tone(), Active);
        assert_eq!(NodeKind::Deny.tone(), Deny);
        assert_eq!(NodeKind::Developer.tone(), Neutral);
        assert_eq!(NodeKind::CiJob.slug(), "ci-job");
        assert_eq!(NodeKind::Session.icon(), "i-clock");
    }

    #[test]
    fn icon_ids_are_unique() {
        let sprite = Icons.html().as_str().to_owned();
        let mut ids: Vec<&str> = sprite
            .split(r#"<symbol id=""#)
            .skip(1)
            .filter_map(|s| s.split('"').next())
            .collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate <symbol id>");
    }
}
