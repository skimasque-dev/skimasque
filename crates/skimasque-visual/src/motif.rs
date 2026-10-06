//! Alpine motifs: decorative SVG (contour lines, a mountain silhouette, a
//! route) and the trail markers that label deployment runs. Colour comes
//! from `currentColor` and role classes, so the motifs follow the theme.

use stucco_core::Render;

use crate::{Component, Html, Planned, Tone};

#[derive(Debug, Clone, Copy, Default)]
pub struct Contour;

impl Render for Contour {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            r#"<svg class="v-contour" viewBox="0 0 800 240" preserveAspectRatio="xMidYMid slice" aria-hidden="true" focusable="false"><g fill="none" stroke="currentColor" stroke-width="1"><path d="M0 200 C120 160 200 210 320 170 S560 120 800 160"/><path d="M0 170 C120 130 200 180 320 140 S560 90 800 130"/><path d="M0 140 C120 100 200 150 320 110 S560 60 800 100"/><path d="M0 110 C120 70 200 120 320 80 S560 30 800 70"/><path d="M0 80 C120 40 200 90 320 50 S560 0 800 40"/></g></svg>"#,
        );
    }
}
impl Component for Contour {}

#[derive(Debug, Clone, Copy, Default)]
pub struct Mountain;

impl Render for Mountain {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            r#"<svg class="v-mountain" viewBox="0 0 800 160" preserveAspectRatio="none" aria-hidden="true" focusable="false"><path class="v-mountain-far" d="M0 160 L0 90 L120 40 L200 80 L300 20 L420 90 L520 50 L640 100 L720 60 L800 90 L800 160 Z"/><path class="v-mountain-near" d="M0 160 L0 120 L90 80 L180 115 L280 70 L380 120 L480 95 L590 125 L690 85 L800 120 L800 160 Z"/></svg>"#,
        );
    }
}
impl Component for Mountain {}

#[derive(Debug, Clone, Default)]
pub struct Route {
    pub flowing: bool,
}

impl Render for Route {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<svg class="v-route"#);
        if self.flowing {
            crate::render::markup(cx, r#" v-route-flowing"#);
        }
        crate::render::markup(
            cx,
            r#"" viewBox="0 0 400 80" aria-hidden="true" focusable="false"><path class="v-route-line" fill="none" d="M8 64 C90 64 100 16 200 16 S310 64 392 24"/><circle class="v-route-start" cx="8" cy="64" r="5"/><circle class="v-route-end" cx="392" cy="24" r="5"/></svg>"#,
        );
    }
}
impl Route {
    pub fn new() -> Self {
        Self::default()
    }
    /// The route line travels: state motion, disabled under reduced motion.
    pub fn flowing(mut self) -> Self {
        self.flowing = true;
        self
    }
}
impl Component for Route {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Circle,
    Square,
    Diamond,
}

#[derive(Debug, Clone)]
pub struct TrailMarker {
    pub shape: Shape,
    pub tone: Tone,
    pub label: String,
}

impl Render for TrailMarker {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<span class="v-trail v-tone-"#);
        crate::render::text(cx, &self.tone.class());
        crate::render::markup(
            cx,
            r#""><svg class="v-trail-shape" viewBox="0 0 12 12" aria-hidden="true" focusable="false">"#,
        );
        if self.is_circle() {
            crate::render::markup(cx, r#"<circle cx="6" cy="6" r="4.5"/>"#);
        } else if self.is_square() {
            crate::render::markup(cx, r#"<rect x="1.5" y="1.5" width="9" height="9"/>"#);
        } else {
            crate::render::markup(cx, r#"<polygon points="6,0.8 11.2,6 6,11.2 0.8,6"/>"#);
        }
        crate::render::markup(cx, r#"</svg><span class="v-trail-label">"#);
        crate::render::text(cx, &self.label);
        crate::render::markup(cx, r#"</span></span>"#);
    }
}
impl TrailMarker {
    pub fn new(shape: Shape, tone: Tone, label: impl Into<String>) -> Self {
        Self {
            shape,
            tone,
            label: label.into(),
        }
    }
    fn is_circle(&self) -> bool {
        self.shape == Shape::Circle
    }
    fn is_square(&self) -> bool {
        self.shape == Shape::Square
    }
}
impl Component for TrailMarker {}

/// The deployment "runs". The colour never implies quality: the marker's
/// shape differs too, and the technical name is always shown beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Run {
    Green,
    Blue,
    Black,
}
impl Run {
    pub fn name(self) -> &'static str {
        match self {
            Run::Green => "Green Run",
            Run::Blue => "Blue Run",
            Run::Black => "Black Run",
        }
    }
    pub fn shape(self) -> Shape {
        match self {
            Run::Green => Shape::Circle,
            Run::Blue => Shape::Square,
            Run::Black => Shape::Diamond,
        }
    }
    pub fn tone(self) -> Tone {
        match self {
            Run::Green => Tone::Active,
            Run::Blue => Tone::Info,
            Run::Black => Tone::Neutral,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunCard {
    pub run: Run,
    pub technical: String,
    pub lines: Vec<String>,
    pub planned: Option<String>,
}

impl Render for RunCard {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            r#"<article class="v-card v-run-card"><header class="v-card-head"><h3 class="v-card-title">"#,
        );
        self.marker_html().render(cx);
        crate::render::markup(cx, r#"</h3></header><p class="v-run-tech">"#);
        crate::render::text(cx, &self.technical);
        crate::render::markup(cx, r#"</p>"#);
        if !self.lines.is_empty() {
            crate::render::markup(cx, r#"<ul class="v-run-lines">"#);
            for l in &self.lines {
                crate::render::markup(cx, r#"<li>"#);
                crate::render::text(cx, &l);
                crate::render::markup(cx, r#"</li>"#);
            }
            crate::render::markup(cx, r#"</ul>"#);
        }
        if let Some(n) = &self.planned {
            crate::render::markup(cx, r#"<p class="v-run-planned">"#);
            crate::render::text(cx, &n);
            crate::render::markup(cx, r#" "#);
            self.planned_marker().render(cx);
            crate::render::markup(cx, r#"</p>"#);
        }
        crate::render::markup(cx, r#"</article>"#);
    }
}
impl RunCard {
    pub fn new(run: Run, technical: impl Into<String>, lines: &[&str]) -> Self {
        Self {
            run,
            technical: technical.into(),
            lines: lines.iter().map(|l| (*l).to_owned()).collect(),
            planned: None,
        }
    }
    pub fn planned(mut self, note: impl Into<String>) -> Self {
        self.planned = Some(note.into());
        self
    }
    fn marker_html(&self) -> Html {
        TrailMarker::new(self.run.shape(), self.run.tone(), self.run.name()).html()
    }
    fn planned_marker(&self) -> Html {
        Planned::new().html()
    }
}
impl Component for RunCard {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Tone};

    #[test]
    fn decorative_svg_is_hidden_from_assistive_tech() {
        for h in [Contour.html(), Mountain.html(), Route::new().html()] {
            let s = h.as_str();
            assert!(
                s.starts_with("<svg")
                    && s.contains(r#"aria-hidden="true""#)
                    && s.contains(r#"focusable="false""#),
                "{s}"
            );
        }
        assert!(
            Mountain.html().as_str().contains("v-mountain-far")
                && Mountain.html().as_str().contains("v-mountain-near")
        );
    }

    #[test]
    fn a_route_flows_only_when_asked() {
        assert!(!Route::new().html().as_str().contains("v-route-flowing"));
        assert!(Route::new()
            .flowing()
            .html()
            .as_str()
            .contains("v-route-flowing"));
    }

    #[test]
    fn markers_differ_by_shape_not_only_colour() {
        let shape = |s| {
            TrailMarker::new(s, Tone::Active, "x")
                .html()
                .as_str()
                .to_owned()
        };
        assert!(shape(Shape::Circle).contains("<circle"));
        assert!(shape(Shape::Square).contains("<rect"));
        assert!(shape(Shape::Diamond).contains("<polygon"));
        assert!(
            shape(Shape::Circle).contains("v-tone-active") && shape(Shape::Circle).contains(">x<")
        );
    }

    #[test]
    fn a_run_card_always_shows_the_run_and_its_technical_name() {
        for (run, word, shape) in [
            (Run::Green, "Green Run", "<circle"),
            (Run::Blue, "Blue Run", "<rect"),
            (Run::Black, "Black Run", "<polygon"),
        ] {
            let s = RunCard::new(run, "Your Gateway", &["a line"])
                .html()
                .as_str()
                .to_owned();
            assert!(
                s.contains(word)
                    && s.contains("Your Gateway")
                    && s.contains("a line")
                    && s.contains(shape),
                "{word}: {s}"
            );
            assert!(!s.contains("PLANNED"));
        }
        let p = RunCard::new(Run::Green, "SkiMasque Cloud", &[])
            .planned("hosted gateway")
            .html()
            .as_str()
            .to_owned();
        assert_eq!(p.matches("PLANNED").count(), 1);
        assert!(p.contains("hosted gateway"));
        assert!(
            !p.contains(r#"class="v-sr"> — hosted"#),
            "the note is visible, not screen-reader-only"
        );
    }

    #[test]
    fn motif_strings_are_escaped() {
        let s = TrailMarker::new(Shape::Circle, Tone::Neutral, "<script>").html();
        assert!(!s.as_str().contains("<script>"));
        let c = RunCard::new(Run::Blue, "<b>x</b>", &["<i>y</i>"]).html();
        assert!(!c.as_str().contains("<b>x") && !c.as_str().contains("<i>y"));
    }
}
