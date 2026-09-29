//! Alpine motifs: decorative SVG (contour lines, a mountain silhouette, a
//! route) and the trail markers that label deployment runs. Colour comes
//! from `currentColor` and role classes, so the motifs follow the theme.

use askama::Template;

use crate::{Component, Html, Planned, Tone};

#[derive(Template, Debug, Clone, Copy, Default)]
#[template(path = "contour.html")]
pub struct Contour;
impl Component for Contour {}

#[derive(Template, Debug, Clone, Copy, Default)]
#[template(path = "mountain.html")]
pub struct Mountain;
impl Component for Mountain {}

#[derive(Template, Debug, Clone, Default)]
#[template(path = "route.html")]
pub struct Route {
    pub flowing: bool,
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

#[derive(Template, Debug, Clone)]
#[template(path = "trail_marker.html")]
pub struct TrailMarker {
    pub shape: Shape,
    pub tone: Tone,
    pub label: String,
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

#[derive(Template, Debug, Clone)]
#[template(path = "run_card.html")]
pub struct RunCard {
    pub run: Run,
    pub technical: String,
    pub lines: Vec<String>,
    pub planned: Option<String>,
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
