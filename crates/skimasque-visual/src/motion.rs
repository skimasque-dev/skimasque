//! Motion that communicates state, never decoration. Each wrapper only sets
//! a class; the animation lives in CSS and is off under reduced motion.

use askama::Template;

use crate::{Component, Html};

/// The steps of a decision appear in order (the "authorization reveal").
#[derive(Template, Debug, Clone)]
#[template(path = "reveal.html")]
pub struct Reveal {
    pub inner: Html,
}
impl Reveal {
    pub fn new(c: &impl Component) -> Self {
        Self { inner: c.html() }
    }
}
impl Component for Reveal {}

/// An expired session settles to a faded state (static under reduced motion).
#[derive(Template, Debug, Clone)]
#[template(path = "expire.html")]
pub struct Expire {
    pub inner: Html,
}
impl Expire {
    pub fn new(c: &impl Component) -> Self {
        Self { inner: c.html() }
    }
}
impl Component for Expire {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Node, NodeKind};

    #[test]
    fn wrappers_wrap_the_component_and_mark_the_motion() {
        let n = Node::new(NodeKind::Policy);
        let r = Reveal::new(&n).html();
        assert!(
            r.as_str().starts_with(r#"<div class="v-reveal">"#) && r.as_str().contains("v-node")
        );
        let e = Expire::new(&n).html();
        assert!(
            e.as_str().starts_with(r#"<div class="v-expire">"#) && e.as_str().contains("v-node")
        );
    }
}
