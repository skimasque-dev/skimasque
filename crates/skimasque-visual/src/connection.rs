//! Connections between nodes. Solid = data, dashed = control plane, thick
//! mint = an active session, dotted = a potential route, broken with × =
//! denied. The meaning is also spoken for screen readers.

use askama::Template;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnKind {
    Normal,
    Control,
    Active,
    Potential,
    Denied,
}

impl ConnKind {
    pub fn slug(self) -> &'static str {
        match self {
            ConnKind::Normal => "normal",
            ConnKind::Control => "control",
            ConnKind::Active => "active",
            ConnKind::Potential => "potential",
            ConnKind::Denied => "denied",
        }
    }
    pub fn spoken(self) -> &'static str {
        match self {
            ConnKind::Normal => "",
            ConnKind::Control => "control plane",
            ConnKind::Active => "active",
            ConnKind::Potential => "potential",
            ConnKind::Denied => "denied",
        }
    }
    fn is_denied(self) -> bool {
        self == ConnKind::Denied
    }
}

#[derive(Template, Debug, Clone)]
#[template(path = "connection.html")]
pub struct Connection {
    pub kind: ConnKind,
    pub label: Option<String>,
    arrow: ArrowGeometry,
}

/// Dimensions in SVG pixels. Clamped to keep the shaft, gap, and head inside
/// the viewport even for very short or thick connectors.
#[derive(Debug, Clone, Copy)]
pub struct ArrowGeometry {
    length: u16,
    head_size: u16,
    gap: u16,
}
impl ArrowGeometry {
    pub fn new(length: u16, head_size: u16, gap: u16) -> Self {
        Self {
            length: length.clamp(32, 256),
            head_size: head_size.clamp(4, 12),
            gap: gap.clamp(1, 8),
        }
    }
    fn cross(self) -> f32 {
        self.head_size as f32 + 4.0
    }
    fn center(self) -> f32 {
        self.cross() / 2.0
    }
    fn head_inset(self) -> f32 {
        self.head_size as f32 + 2.0
    }
    fn shaft_inset(self, stroke: f32) -> f32 {
        self.head_inset() + self.gap as f32 + stroke / 2.0
    }
    fn shaft_start(self, stroke: f32) -> f32 {
        self.shaft_inset(stroke) + stroke / 2.0
    }
    fn head_far(self) -> f32 {
        self.cross() - 2.0
    }
    fn packet_travel(self) -> u16 {
        self.length - self.head_size - self.gap - 8
    }
}
impl Default for ArrowGeometry {
    fn default() -> Self {
        Self::new(32, 8, 2)
    }
}

impl Connection {
    pub fn new(kind: ConnKind) -> Self {
        Connection {
            kind,
            label: None,
            arrow: ArrowGeometry::default(),
        }
    }
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn geometry(mut self, geometry: ArrowGeometry) -> Self {
        self.arrow = geometry;
        self
    }
    fn arrow_geometry(&self) -> ArrowGeometry {
        self.arrow
    }
    fn stroke_width(&self) -> f32 {
        if self.kind == ConnKind::Active {
            3.0
        } else {
            2.0
        }
    }
    fn shaft_inset(&self) -> f32 {
        self.arrow.shaft_inset(self.stroke_width())
    }
    fn shaft_start(&self) -> f32 {
        self.arrow.shaft_start(self.stroke_width())
    }
    fn dash_pattern(&self) -> &'static str {
        match self.kind {
            ConnKind::Control | ConnKind::Denied => "5 4",
            ConnKind::Potential => "0 6",
            _ => "none",
        }
    }
}

impl crate::Component for Connection {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    #[test]
    fn svg_shafts_stop_before_the_head_in_both_orientations() {
        for kind in [
            ConnKind::Normal,
            ConnKind::Control,
            ConnKind::Active,
            ConnKind::Potential,
            ConnKind::Denied,
        ] {
            let arrow = Connection::new(kind).geometry(ArrowGeometry::new(64, 10, 3));
            let g = arrow.arrow_geometry();
            let stroke = arrow.stroke_width();
            assert_eq!(g.length, 64);
            assert_eq!(g.head_size, 10);
            assert_eq!(g.gap, 3);
            assert_eq!(g.shaft_inset(stroke) - stroke / 2.0 - g.head_inset(), 3.0);
            let h = arrow.html();
            assert!(
                h.as_str().contains("v-conn-horizontal") && h.as_str().contains("v-conn-vertical")
            );
            assert_eq!(h.as_str().matches("<polygon").count(), 2);
            assert_eq!(h.as_str().matches("<line ").count(), 2);
            let inset = if kind == ConnKind::Active {
                "16.5"
            } else {
                "16"
            };
            assert!(h
                .as_str()
                .contains(&format!("transform=\"translate(-{inset} 0)\"")));
            assert!(h
                .as_str()
                .contains(&format!("transform=\"translate(0 -{inset})\"")));
            assert!(h.as_str().contains("points=\"-12,2 -2,7 -12,12\""));
            assert!(h.as_str().contains("points=\"2,-12 7,-2 12,-12\""));
        }
    }

    #[test]
    fn extreme_geometry_cannot_reverse_or_clip_the_shaft() {
        for geometry in [
            ArrowGeometry::new(0, u16::MAX, u16::MAX),
            ArrowGeometry::new(u16::MAX, 0, 0),
        ] {
            let arrow = Connection::new(ConnKind::Active).geometry(geometry);
            let g = arrow.arrow_geometry();
            assert!(g.length as f32 - g.shaft_inset(3.0) > 3.0 / 2.0);
            assert!(g.gap >= 1);
            assert!(g.cross() > g.head_size as f32);
        }
    }

    #[test]
    fn each_kind_has_its_class_and_spoken_word() {
        for (k, slug, word) in [
            (ConnKind::Normal, "normal", ""),
            (ConnKind::Control, "control", "control plane"),
            (ConnKind::Active, "active", "active"),
            (ConnKind::Potential, "potential", "potential"),
            (ConnKind::Denied, "denied", "denied"),
        ] {
            let h = Connection::new(k).html().as_str().to_owned();
            assert!(
                h.contains(&format!(r#"class="v-conn v-conn-{slug}""#)),
                "{h}"
            );
            if word.is_empty() {
                assert!(!h.contains("v-sr"), "{h}");
            } else {
                assert!(
                    h.contains(&format!(r#"<span class="v-sr">{word}</span>"#)),
                    "{h}"
                );
            }
        }
        assert!(Connection::new(ConnKind::Denied)
            .html()
            .as_str()
            .contains(r#"<span class="v-conn-x" aria-hidden="true">×</span>"#));
    }

    #[test]
    fn labels_are_shown_and_escaped() {
        let h = Connection::new(ConnKind::Control)
            .label("OIDC <tok>")
            .html();
        assert!(
            h.as_str()
                .contains(r#"<span class="v-conn-label">OIDC &lt;tok&gt;</span>"#),
            "{h}"
        );
    }
}
