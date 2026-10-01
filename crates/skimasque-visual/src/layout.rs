//! Layout containers for diagrams that are not a straight line: stacked
//! layers, side-by-side comparisons, branches and message sequences.
//! Every container carries a visually-hidden caption as its text equivalent.

use askama::Template;

use crate::{Component, Connection, Html, Tone};

#[derive(Debug, Clone)]
pub struct LayerRow {
    pub title: String,
    pub sub: Option<String>,
}

#[derive(Template, Debug, Clone)]
#[template(path = "layers.html")]
pub struct Layers {
    pub caption: String,
    pub rows: Vec<LayerRow>,
    /// Arrows point up: each layer rests on the one below it.
    pub upward: bool,
}
impl Layers {
    pub fn new(caption: impl Into<String>) -> Self {
        Self {
            caption: caption.into(),
            rows: Vec::new(),
            upward: false,
        }
    }
    pub fn upward(mut self) -> Self {
        self.upward = true;
        self
    }
    pub fn row(mut self, title: impl Into<String>) -> Self {
        self.rows.push(LayerRow {
            title: title.into(),
            sub: None,
        });
        self
    }
    pub fn row_sub(mut self, title: impl Into<String>, sub: impl Into<String>) -> Self {
        self.rows.push(LayerRow {
            title: title.into(),
            sub: Some(sub.into()),
        });
        self
    }
}
impl Component for Layers {}

#[derive(Debug, Clone)]
pub struct Side {
    pub title: String,
    pub tone: Tone,
    pub body: Html,
}

#[derive(Template, Debug, Clone)]
#[template(path = "compare.html")]
pub struct Compare {
    pub caption: String,
    pub sides: Vec<Side>,
}
impl Compare {
    pub fn new(caption: impl Into<String>) -> Self {
        Self {
            caption: caption.into(),
            sides: Vec::new(),
        }
    }
    /// Nest containers only inside `Compare` sides and `Branch` arms, which have a definite width: a `Flow`, `Branch` or `Compare` placed in a content-sized slot (a `Flow` step body, a `Branch` root) may collapse.
    pub fn side(mut self, title: impl Into<String>, tone: Tone, body: &impl Component) -> Self {
        self.sides.push(Side {
            title: title.into(),
            tone,
            body: body.html(),
        });
        self
    }
}
impl Component for Compare {}

#[derive(Debug, Clone)]
pub struct Arm {
    pub conn: Html,
    pub body: Html,
}

#[derive(Template, Debug, Clone)]
#[template(path = "branch.html")]
pub struct Branch {
    pub caption: String,
    pub root: Html,
    pub arms: Vec<Arm>,
}
impl Branch {
    /// Nest containers only inside `Compare` sides and `Branch` arms, which have a definite width: a `Flow`, `Branch` or `Compare` placed in a content-sized slot (a `Flow` step body, a `Branch` root) may collapse.
    pub fn new(caption: impl Into<String>, root: &impl Component) -> Self {
        Self {
            caption: caption.into(),
            root: root.html(),
            arms: Vec::new(),
        }
    }
    /// Nest containers only inside `Compare` sides and `Branch` arms, which have a definite width: a `Flow`, `Branch` or `Compare` placed in a content-sized slot (a `Flow` step body, a `Branch` root) may collapse.
    pub fn arm(mut self, conn: Connection, body: &impl Component) -> Self {
        self.arms.push(Arm {
            conn: conn.html(),
            body: body.html(),
        });
        self
    }
}
impl Component for Branch {}

#[derive(Debug, Clone)]
pub struct Message {
    pub label: String,
    pub rightward: bool,
    pub spoken: String,
}

#[derive(Template, Debug, Clone)]
#[template(path = "sequence.html")]
pub struct Sequence {
    pub caption: String,
    pub left: String,
    pub right: String,
    pub messages: Vec<Message>,
}
impl Sequence {
    fn arrow(&self) -> Html {
        Connection::new(crate::ConnKind::Normal).html()
    }
    pub fn new(
        caption: impl Into<String>,
        left: impl Into<String>,
        right: impl Into<String>,
    ) -> Self {
        Self {
            caption: caption.into(),
            left: left.into(),
            right: right.into(),
            messages: Vec::new(),
        }
    }
    pub fn to_right(mut self, label: impl Into<String>) -> Self {
        let spoken = format!("{} to {}", self.left, self.right);
        self.messages.push(Message {
            label: label.into(),
            rightward: true,
            spoken,
        });
        self
    }
    pub fn to_left(mut self, label: impl Into<String>) -> Self {
        let spoken = format!("{} to {}", self.right, self.left);
        self.messages.push(Message {
            label: label.into(),
            rightward: false,
            spoken,
        });
        self
    }
}
impl Component for Sequence {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConnKind, Node, NodeKind, Tone};

    #[test]
    fn layers_are_an_ordered_list_with_a_directed_gap_between_rows() {
        let s = Layers::new("Each layer rests on the one below.")
            .upward()
            .row("IDENTITY")
            .row_sub("POLICY", "who may reach what")
            .row("NETWORK")
            .html();
        let s = s.as_str();
        assert!(s.contains("<ol") && s.contains("v-layers-up"));
        assert_eq!(s.matches("v-layer-row").count(), 3);
        assert_eq!(s.matches("↑").count(), 2, "a gap between rows only");
        assert!(s.contains("who may reach what") && s.contains(r#"class="v-sr">Each layer rests"#));
        assert!(
            Layers::new("x").html().as_str().contains("<ol"),
            "empty layers still render"
        );
        assert_eq!(
            Layers::new("x")
                .row("A")
                .row("B")
                .html()
                .as_str()
                .matches("↓")
                .count(),
            1
        );
    }

    #[test]
    fn compare_shows_sides_with_titles_and_tones() {
        let a = Node::new(NodeKind::CiJob);
        let s = Compare::new("Two ways.")
            .side("Traditional", Tone::Neutral, &a)
            .side("", Tone::Active, &a)
            .html();
        let s = s.as_str();
        assert_eq!(s.matches("v-compare-side").count(), 2);
        assert!(s.contains("v-tone-neutral") && s.contains("v-tone-active"));
        assert!(
            s.contains("Traditional") && !s.contains("<h4") && !s.contains("<h3"),
            "titles are text, not headings"
        );
        assert_eq!(
            s.matches("v-compare-title").count(),
            1,
            "an empty title renders no title"
        );
        assert!(Compare::new("x")
            .side("only", Tone::Neutral, &a)
            .html()
            .as_str()
            .contains("only"));
    }

    #[test]
    fn branch_puts_the_root_before_its_arms_each_with_a_connection() {
        let root = Node::new(NodeKind::Policy);
        let s = Branch::new("A policy either matches or not.", &root)
            .arm(
                Connection::new(ConnKind::Active).label("match"),
                &Node::new(NodeKind::Allow),
            )
            .arm(
                Connection::new(ConnKind::Denied).label("no match"),
                &Node::new(NodeKind::Deny),
            )
            .html();
        let s = s.as_str();
        assert!(s.find("v-branch-root").unwrap() < s.find("v-branch-arms").unwrap());
        assert_eq!(s.matches("v-branch-arm\"").count(), 2);
        assert!(s.contains("v-conn-active") && s.contains("v-conn-denied"));
        assert!(
            Branch::new("none", &root)
                .html()
                .as_str()
                .contains("v-branch-root"),
            "no arms still renders the root"
        );
    }

    #[test]
    fn sequence_orders_messages_and_speaks_their_direction() {
        let s = Sequence::new("A tunnel is requested and confirmed.", "Client", "Gateway")
            .to_right("CONNECT-UDP request")
            .to_left("tunnel established")
            .html();
        let s = s.as_str();
        assert!(s.find("CONNECT-UDP request").unwrap() < s.find("tunnel established").unwrap());
        assert!(s.contains("v-seq-right") && s.contains("v-seq-left"));
        assert_eq!(s.matches("v-conn-horizontal").count(), 2);
        assert!(s.contains("Client to Gateway") && s.contains("Gateway to Client"));
        assert!(Sequence::new("x", "A", "B")
            .html()
            .as_str()
            .contains("v-seq-actor"));
    }

    #[test]
    fn layout_strings_are_escaped() {
        let h = Layers::new("<script>").row("<b>x</b>").html();
        assert!(!h.as_str().contains("<script>") && !h.as_str().contains("<b>x"));
        let q = Sequence::new("c", "<i>", "r").to_right("\"<img>\"").html();
        assert!(!q.as_str().contains("<i>") && !q.as_str().contains("<img>"));
    }
}
