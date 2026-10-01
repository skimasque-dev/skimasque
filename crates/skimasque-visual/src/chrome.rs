//! Content components for the public site: prose, hero, sections, feature
//! grid, CTA band, comparison table, FAQ and tier cards.

use askama::Template;

use crate::{Component, Html, Planned};

#[derive(Debug, Clone)]
pub struct Cta {
    pub label: String,
    pub href: String,
    pub primary: bool,
}
impl Cta {
    pub fn primary(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: href.into(),
            primary: true,
        }
    }
    pub fn secondary(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: href.into(),
            primary: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Block {
    P(String),
    Lead(String),
    Sub(String),
    List(Vec<String>),
    Quote(String),
    Kv(String, String),
}

#[derive(Template, Debug, Clone, Default)]
#[template(path = "prose.html")]
pub struct Prose {
    pub blocks: Vec<Block>,
}
impl Prose {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn p(mut self, t: impl Into<String>) -> Self {
        self.blocks.push(Block::P(t.into()));
        self
    }
    pub fn lead(mut self, t: impl Into<String>) -> Self {
        self.blocks.push(Block::Lead(t.into()));
        self
    }
    #[allow(clippy::should_implement_trait)]
    pub fn sub(mut self, t: impl Into<String>) -> Self {
        self.blocks.push(Block::Sub(t.into()));
        self
    }
    pub fn list(mut self, items: &[&str]) -> Self {
        self.blocks
            .push(Block::List(items.iter().map(|s| (*s).to_owned()).collect()));
        self
    }
    pub fn quote(mut self, t: impl Into<String>) -> Self {
        self.blocks.push(Block::Quote(t.into()));
        self
    }
    pub fn kv(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.blocks.push(Block::Kv(k.into(), v.into()));
        self
    }
}
impl Component for Prose {}

#[derive(Template, Debug, Clone)]
#[template(path = "hero.html")]
pub struct Hero {
    pub title: String,
    pub eyebrow: Option<String>,
    pub leads: Vec<String>,
    pub ctas: Vec<Cta>,
    pub aside: Option<Html>,
}
impl Hero {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            eyebrow: None,
            leads: Vec::new(),
            ctas: Vec::new(),
            aside: None,
        }
    }
    pub fn eyebrow(mut self, t: impl Into<String>) -> Self {
        self.eyebrow = Some(t.into());
        self
    }
    pub fn lead(mut self, t: impl Into<String>) -> Self {
        self.leads.push(t.into());
        self
    }
    pub fn cta(mut self, c: Cta) -> Self {
        self.ctas.push(c);
        self
    }
    pub fn aside(mut self, c: &impl Component) -> Self {
        self.aside = Some(c.html());
        self
    }
}
impl Component for Hero {}

#[derive(Template, Debug, Clone)]
#[template(path = "section.html")]
pub struct Section {
    pub title: String,
    pub id: Option<String>,
    pub eyebrow: Option<String>,
    pub alt: bool,
    pub bodies: Vec<Html>,
}
impl Section {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            id: None,
            eyebrow: None,
            alt: false,
            bodies: Vec::new(),
        }
    }
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
    pub fn eyebrow(mut self, t: impl Into<String>) -> Self {
        self.eyebrow = Some(t.into());
        self
    }
    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }
    pub fn push(mut self, c: &impl Component) -> Self {
        self.bodies.push(c.html());
        self
    }
}
impl Component for Section {}

#[derive(Debug, Clone)]
pub struct Feature {
    pub title: String,
    pub body: String,
    pub planned: Option<String>,
}
impl Feature {
    /// The marker without a note: the visible note is rendered beside it, so
    /// the marker's own sr-only copy would only repeat it.
    fn marker(&self) -> Html {
        Planned::new().html()
    }
}

#[derive(Template, Debug, Clone, Default)]
#[template(path = "feature_grid.html")]
pub struct FeatureGrid {
    pub features: Vec<Feature>,
}
impl FeatureGrid {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn feature(mut self, title: impl Into<String>, body: impl Into<String>) -> Self {
        self.features.push(Feature {
            title: title.into(),
            body: body.into(),
            planned: None,
        });
        self
    }
    pub fn planned(
        mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        note: impl Into<String>,
    ) -> Self {
        self.features.push(Feature {
            title: title.into(),
            body: body.into(),
            planned: Some(note.into()),
        });
        self
    }
}
impl Component for FeatureGrid {}

#[derive(Template, Debug, Clone)]
#[template(path = "cta_band.html")]
pub struct CtaBand {
    pub title: String,
    pub lines: Vec<String>,
    pub ctas: Vec<Cta>,
}
impl CtaBand {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            lines: Vec::new(),
            ctas: Vec::new(),
        }
    }
    pub fn line(mut self, t: impl Into<String>) -> Self {
        self.lines.push(t.into());
        self
    }
    pub fn cta(mut self, c: Cta) -> Self {
        self.ctas.push(c);
        self
    }
}
impl Component for CtaBand {}

#[derive(Template, Debug, Clone)]
#[template(path = "comparison_table.html")]
pub struct ComparisonTable {
    pub head: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub highlight_last: bool,
}
impl ComparisonTable {
    pub fn new(head: &[&str]) -> Self {
        Self {
            head: head.iter().map(|s| (*s).to_owned()).collect(),
            rows: Vec::new(),
            highlight_last: false,
        }
    }
    pub fn row(mut self, cells: &[&str]) -> Self {
        self.rows
            .push(cells.iter().map(|s| (*s).to_owned()).collect());
        self
    }
    pub fn highlight_last(mut self) -> Self {
        self.highlight_last = true;
        self
    }
    fn is_last(&self, i: &usize) -> bool {
        self.highlight_last && *i + 1 == self.rows.len()
    }
}
impl Component for ComparisonTable {}

#[derive(Debug, Clone)]
pub struct FaqItem {
    pub question: String,
    pub answer: Vec<String>,
}

#[derive(Template, Debug, Clone, Default)]
#[template(path = "faq.html")]
pub struct Faq {
    pub items: Vec<FaqItem>,
}
impl Faq {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn item(mut self, question: impl Into<String>, answer: &[&str]) -> Self {
        self.items.push(FaqItem {
            question: question.into(),
            answer: answer.iter().map(|s| (*s).to_owned()).collect(),
        });
        self
    }
}
impl Component for Faq {}

#[derive(Template, Debug, Clone)]
#[template(path = "tier_card.html")]
pub struct TierCard {
    pub name: String,
    pub price: String,
    pub tagline: String,
    pub includes: Vec<String>,
    pub live: bool,
    pub planned: Option<String>,
}
impl TierCard {
    pub fn new(
        name: impl Into<String>,
        price: impl Into<String>,
        tagline: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            price: price.into(),
            tagline: tagline.into(),
            includes: Vec::new(),
            live: false,
            planned: None,
        }
    }
    pub fn include(mut self, t: impl Into<String>) -> Self {
        self.includes.push(t.into());
        self
    }
    pub fn live(mut self) -> Self {
        self.live = true;
        self
    }
    pub fn planned(mut self, note: impl Into<String>) -> Self {
        self.planned = Some(note.into());
        self
    }
    /// The marker without a note (the visible note sits beside it).
    fn planned_html(&self) -> Html {
        Planned::new().html()
    }
}
impl Component for TierCard {}

/// A responsive grid of [`TierCard`]s.
#[derive(Template, Debug, Clone, Default)]
#[template(path = "tier_grid.html")]
pub struct TierGrid {
    pub cards: Vec<Html>,
}
impl TierGrid {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn card(mut self, c: &TierCard) -> Self {
        self.cards.push(c.html());
        self
    }
}
impl Component for TierGrid {}

/// A block of content the product does not deliver yet. Do not nest one inside another.
#[derive(Template, Debug, Clone)]
#[template(path = "planned_block.html")]
pub struct PlannedBlock {
    pub note: String,
    pub inner: Html,
}
impl PlannedBlock {
    pub fn new(note: impl Into<String>, c: &impl Component) -> Self {
        Self {
            note: note.into(),
            inner: c.html(),
        }
    }
    fn planned_html(&self) -> Html {
        Planned::new().html()
    }
}
impl Component for PlannedBlock {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Node, NodeKind};

    #[test]
    fn prose_renders_blocks_in_order_and_escapes() {
        let s = Prose::new()
            .lead("L")
            .p("a <b>")
            .sub("S")
            .list(&["x", "y"])
            .quote("q")
            .kv("WHO", "acme/widget")
            .html();
        let s = s.as_str();
        assert!(
            s.find("L").unwrap() < s.find("S").unwrap()
                && s.contains("<ul")
                && s.contains("<blockquote")
                && s.contains("WHO")
        );
        assert!(s.contains("a &lt;b&gt;") && !s.contains("<b>"));
    }

    #[test]
    fn a_hero_has_one_h1_ctas_and_an_optional_aside() {
        let h = Hero::new("Give every workload exactly the network access it needs.")
            .eyebrow("Identity-aware network access")
            .lead("Lead one.")
            .cta(Cta::primary("Get Started", "https://example.test/x"))
            .cta(Cta::secondary("See How It Works", "how-it-works/"))
            .aside(&Node::new(NodeKind::Gateway))
            .html();
        let s = h.as_str();
        assert_eq!(s.matches("<h1").count(), 1);
        assert!(
            s.contains("v-btn")
                && s.contains("v-btn-quiet")
                && s.contains("Get Started")
                && s.contains("v-node")
        );
        assert!(!Hero::new("x").html().as_str().contains("v-hero-aside"));
    }

    #[test]
    fn a_section_has_an_h2_an_id_and_its_bodies_in_order() {
        let s = Section::new("Product model")
            .id("model")
            .alt()
            .push(&Prose::new().p("one"))
            .push(&Prose::new().p("two"))
            .html();
        let s = s.as_str();
        assert!(s.contains(r#"id="model""#) && s.contains("<h2") && s.contains("v-section-alt"));
        assert!(s.find("one").unwrap() < s.find("two").unwrap());
    }

    #[test]
    fn feature_cards_mark_unbuilt_ones_planned() {
        let s = FeatureGrid::new()
            .feature("Identity-aware", "Know who is asking.")
            .planned(
                "Egress IP",
                "A fixed source address per gateway.",
                "per-gateway egress IP",
            )
            .html();
        let s = s.as_str();
        assert_eq!(s.matches("v-feature\"").count(), 2);
        assert!(s.contains("PLANNED") && s.contains("<h3"));
        let note = s.find("per-gateway egress IP").unwrap();
        assert!(
            s.find("v-sr").is_none_or(|sr| note < sr),
            "the note is visible, not only in sr-only text"
        );
    }

    #[test]
    fn the_comparison_table_is_a_real_table_and_can_highlight_the_last_row() {
        let s = ComparisonTable::new(&["Approach", "Model"])
            .row(&["VPN", "Network"])
            .row(&["SkiMasque", "Capability"])
            .highlight_last()
            .html();
        let s = s.as_str();
        assert!(s.contains("<table") && s.contains("<th") && s.contains(r#"scope="col""#));
        assert_eq!(s.matches("v-row-highlight").count(), 1);
    }

    #[test]
    fn faq_items_are_details_with_paragraphs() {
        let s = Faq::new()
            .item(
                "Is SkiMasque a VPN?",
                &["Not conceptually.", "It grants access."],
            )
            .html();
        let s = s.as_str();
        assert!(
            s.contains("<details")
                && s.contains("<summary>Is SkiMasque a VPN?")
                && s.matches("<p>").count() >= 2
        );
    }

    #[test]
    fn tier_cards_show_price_and_only_paid_tiers_carry_planned() {
        let free = TierCard::new("Free", "$0", "For evaluation.")
            .include("core policies")
            .live()
            .html();
        assert!(
            free.as_str().contains("$0")
                && free.as_str().contains("Available now")
                && !free.as_str().contains("PLANNED")
        );
        let team = TierCard::new("Team", "$49 / month", "Small teams.")
            .planned("paid plans")
            .html();
        let team = team.as_str();
        assert!(team.contains("PLANNED"));
        let note = team.find("paid plans").unwrap();
        assert!(
            team.find("v-sr").is_none_or(|sr| note < sr),
            "the note is visible, not only in sr-only text"
        );
    }

    #[test]
    fn a_tier_grid_renders_its_cards_in_order() {
        let g = TierGrid::new()
            .card(&TierCard::new("Free", "$0", "a"))
            .card(&TierCard::new("Team", "$49 / month", "b"))
            .html();
        let g = g.as_str();
        assert!(g.starts_with("<div class=\"v-tiers\">"));
        assert!(g.find("Free").unwrap() < g.find("Team").unwrap());
        assert_eq!(g.matches("<article").count(), 2);
    }

    #[test]
    fn a_cta_band_and_planned_block_render() {
        let b = CtaBand::new("Network access should be temporary.")
            .line("Give it to them.")
            .cta(Cta::primary("Create Your First Policy", "x"))
            .html();
        assert!(
            b.as_str().contains("v-cta-band") && b.as_str().contains("Create Your First Policy")
        );
        let p = PlannedBlock::new(
            "SSO",
            &Prose::new().p("Sign in with your identity provider."),
        )
        .html();
        assert!(
            p.as_str()
                .starts_with("<div class=\"v-planned-block\" role=\"note\"")
                && p.as_str().contains("PLANNED")
                && p.as_str().contains("Sign in with your identity provider.")
        );
    }

    #[test]
    fn chrome_strings_are_escaped() {
        let h = Hero::new("<script>x</script>").lead("<i>y</i>").html();
        assert!(!h.as_str().contains("<script>") && !h.as_str().contains("<i>y"));
        let c = ComparisonTable::new(&["<h>"]).row(&["<td>"]).html();
        assert!(!c.as_str().contains("<h>") && !c.as_str().contains("<td>x"));
    }
}
