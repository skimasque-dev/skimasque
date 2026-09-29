# Visual Library 4 — Site Chrome & Public Website Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the public-site chrome to `skimasque-visual` (navigation, footer, hero, sections, feature grid, CTA band, comparison table, FAQ, tier cards, prose) and render the canonical ~20-page public website (`site/`) with `sitegen`, replacing the hand-written `site/index.html`.

**Architecture:** Chrome components are askama structs like everything else (`Component` → `Html`). A `SitePage` shell renders a full document (head, skip link, nav, `<main>`, footer) with **relative** links computed from the page's depth (the site is served from a GitHub Pages project path, so root-relative links would break). `sitegen`'s page registry (`site::pages()`) gains one module per page under `src/site/pages/`, each a function returning a `Page`; pages are composed from chrome components, the plan-3 diagrams (`diagrams::public_set()` functions) and `CodeExample`. A crate-level test walks every generated page: one `<h1>`, a description, resolvable relative links, honesty rules (no `skimasque exec` outside a Planned block, no "zero trust", no "Buy Now").

**Tech Stack:** Rust 1.88, askama 0.12, plain CSS in `static/visual.css`, inline SVG, `<details>` for disclosure (no JS).

**Spec:** `docs/superpowers/specs/2026-09-29-canonical-website-and-control-plane.md` (the canonical spec, kept verbatim; **§N and "spec lines A–B" below refer to that file**), amended by `docs/superpowers/specs/2026-09-30-product-surface-amendment.md` (Planned markers, status wording, `docs`/`status`/`contact` conditions). Plans 1–3 are merged; this branch is `feat/visual-library-4` (cut from `main` at `735002d`).

## Global Constraints

- Work in `crates/skimasque-visual` (repo root = the worktree). Feature `site` gates `pub mod site` and the `sitegen` bin. Generated pages are committed under `site/`; CI runs `sitegen --check`.
- askama **0.12**. Nested markup only ever as `Html` from another component; every caller string is escaped (page copy is data → escaped).
- **Hex colours only inside the `/* tokens */` block** of `static/visual.css`. Chrome CSS uses role tokens; the footer and code blocks are always Pine, so they use palette tokens (a documented exception, with a contrast test). Use literal glyphs (`→ ↓ ✓ × ◇ ●`), never numeric entities.
- Themes follow the OS with no `data-theme` (owner decision, plan 2); pages do not pin a theme.
- **Links are relative.** A page at `<dir>/index.html` links with a `root` prefix (`""`, `"../"`, `"../../"`), directory-style (`how-it-works/`). External links are absolute and only to: `https://github.com/skimasque-dev/skimasque…`, `https://control.skimasque.com`, `https://gateway.skimasque.com`, `https://crates.io/crates/…`.
- **Fixed URLs:** Sign In / Try SkiMasque → `https://control.skimasque.com`; Get Started → `https://github.com/skimasque-dev/skimasque/blob/main/docs/getting-started.md`; docs → `https://github.com/skimasque-dev/skimasque/blob/main/docs/<file>.md`; Contact → GitHub Issues (`https://github.com/skimasque-dev/skimasque/issues`, owner decision: no email address yet).
- **Voice (canonical §2):** technical, calm, concise; say workload, identity, policy, access, session, destination, gateway, "short-lived network access", "identity-aware network access"; never "zero trust", never call SkiMasque a VPN (only in comparison text, as the thing it differs from), never "Buy Now". Primary CTA "Get Started", secondary "See How It Works", technical "Read the Docs", customers "Try SkiMasque", enterprise "Talk to Us".
- **Honesty (amendment decision 3):** anything not built carries the `Planned` marker and is never in the present tense. Today: `skimasque exec` (the real commands are `skimasque login`, `org`, `init`, `policy check|test|explain`, `why`, `gateway`, `connect <dest>` and the `skimasque-dev/connect` GitHub Action; a control-plane-backed `connect` is not finished), paid plans/pricing tiers (billing not in place; the **Free** tier and SkiMasque Cloud are live), SkiMasque-provided dedicated egress IPs (a customer-operated gateway egresses from the customer's own address — that is real), CONNECT-IP forwarding (wire formats done, TUN forwarding not wired), SSO/advanced RBAC, disabling a policy, a working contact form, live status reporting, Privacy/Terms pages, security-reporting mailbox. `skimasque exec` may appear **only inside a `PlannedBlock`**.
- **Deployment runs (canonical §18, owner-confirmed):** Green Run = SkiMasque Cloud (control plane: SkiMasque, gateway: SkiMasque, operations: minimal), live with a free tier, paid plans Planned; Blue Run = Your Gateway (control plane: SkiMasque, gateway: customer); Black Run = Self-hosted (customer runs everything). Run colours mean operational responsibility, not quality.
- Accessibility: one `<h1>` per page, sections in order (`h2` → `h3`), `aria-current="page"` on the current nav link, skip link, every diagram already carries its sr-only caption, focus rings visible, ≥4.5:1 text contrast in both themes.
- Formatting: never run workspace `cargo fmt`; only `rustfmt --edition 2021 <file>` on files you create or edit. **Never edit Rust, CSS, templates or generated files with Python on Windows** (CRLF / mangled `·` twice): use Edit/Write or bash heredocs; keep files LF (`tr -cd '\r' < file | wc -c` = 0). Stop any `http.server` you start by PID only (never `taskkill /IM python.exe`).
- Commits: conventional style, **no AI attribution trailers or footers**.
- Verify commands (repo root): `cargo test -p skimasque-visual --features site`, `cargo clippy -p skimasque-visual --all-targets --features site -- -D warnings`, `cargo run -q -p skimasque-visual --features site --bin sitegen -- --check` (regenerate with the same command minus `-- --check`, commit `site/`).

## Review Focus

- Every internal link on every page resolves to a generated page (a broken nav link on one page is invisible in review of another): Task 3 link-check test, Task 10 browser sweep.
- Long words/unbroken strings in headings and table cells at 320px: no horizontal scroll (Tasks 2, 10).
- The mobile menu and desktop dropdowns work with no JS and are keyboard reachable (`<details>`/`<summary>`) (Task 2, Task 10).
- A page claiming something the product does not do (exec, dedicated egress IP, paid plans, live status, CONNECT-IP forwarding) — the honesty tests plus a per-page reviewer read (Tasks 4–9).
- Dark and light: the always-Pine footer and code blocks stay legible in both; hero/section text on both backgrounds (Tasks 2, 10).

## File Structure

| file | responsibility |
|---|---|
| `src/chrome.rs` + `templates/{prose,hero,section,feature_grid,cta_band,comparison_table,faq,tier_card,planned_block}.html` | content components (Task 1) |
| `src/site_chrome.rs` + `templates/{site_nav,site_footer,site_page}.html` | `SiteNav`, `SiteFooter`, `SitePage` shell, nav data (Task 2) |
| `src/site/mod.rs` (was `src/site.rs`) | `Page`, `pages()`, `stale()`, URL constants, page tests (Task 3) |
| `src/site/gallery.rs` | the existing component gallery (moved, Task 3) |
| `src/site/pages/*.rs` | one module per public page (Tasks 4–9) |
| `static/visual.css` | one CSS layer per task |
| `site/` | generated output (committed); `site/favicon.svg` stays hand-written |

---

### Task 1: Content components

**Files:** Create `src/chrome.rs`, `templates/{prose,hero,section,feature_grid,cta_band,comparison_table,faq,tier_card,planned_block}.html`. Modify `src/lib.rs` (`pub mod chrome;` + `pub use chrome::{Block, ComparisonTable, Cta, CtaBand, Faq, Feature, FeatureGrid, Hero, PlannedBlock, Prose, Section, TierCard};`), `static/visual.css`, `src/site.rs` gallery (group "Site content").

**Interfaces:**
- Produces (all `Component`):
  - `Cta::primary(label, href) -> Cta`, `Cta::secondary(label, href) -> Cta` (`Cta { label, href, primary }`).
  - `Prose::new()`, `.p(text)`, `.lead(text)`, `.sub(heading)` (an `h3`), `.list(&[&str])`, `.quote(text)`, `.kv(key, value)`; enum `Block`.
  - `Hero::new(title) -> Hero` (`<h1>`), `.eyebrow(text)`, `.lead(text)` (repeatable), `.cta(Cta)`, `.aside(&impl Component)`.
  - `Section::new(title) -> Section` (`<h2>`), `.id(slug)`, `.eyebrow(text)`, `.alt()` (Ice/surface-raised band), `.push(&impl Component)` (repeatable body).
  - `FeatureGrid::new()`, `.feature(title, body)`, `.planned(title, body, note)`.
  - `CtaBand::new(title)`, `.line(text)`, `.cta(Cta)`.
  - `ComparisonTable::new(&[headers])`, `.row(&[cells])`, `.highlight_last()`.
  - `Faq::new()`, `.item(question, &[paragraphs])`.
  - `TierCard::new(name, price, tagline)`, `.include(text)`, `.live()`, `.planned(note)`.
  - `PlannedBlock::new(note, &impl Component) -> PlannedBlock` (an `<aside class="v-planned-block">` holding a `Planned` marker + the content; **never nest a `PlannedBlock` inside another**).

- [ ] **Step 1: Failing tests** (`src/chrome.rs`, module + tests first, minimal stubs so tests fail on behaviour):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Node, NodeKind};

    #[test]
    fn prose_renders_blocks_in_order_and_escapes() {
        let s = Prose::new().lead("L").p("a <b>").sub("S").list(&["x", "y"]).quote("q").kv("WHO", "acme/widget").html();
        let s = s.as_str();
        assert!(s.find("L").unwrap() < s.find("S").unwrap() && s.contains("<ul") && s.contains("<blockquote") && s.contains("WHO"));
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
        assert!(s.contains("v-btn") && s.contains("v-btn-quiet") && s.contains("Get Started") && s.contains("v-node"));
        assert!(!Hero::new("x").html().as_str().contains("v-hero-aside"));
    }

    #[test]
    fn a_section_has_an_h2_an_id_and_its_bodies_in_order() {
        let s = Section::new("Product model").id("model").alt().push(&Prose::new().p("one")).push(&Prose::new().p("two")).html();
        let s = s.as_str();
        assert!(s.contains(r#"id="model""#) && s.contains("<h2") && s.contains("v-section-alt"));
        assert!(s.find("one").unwrap() < s.find("two").unwrap());
    }

    #[test]
    fn feature_cards_mark_unbuilt_ones_planned() {
        let s = FeatureGrid::new().feature("Identity-aware", "Know who is asking.").planned("Command wrapper", "Run a command with access.", "skimasque exec").html();
        let s = s.as_str();
        assert_eq!(s.matches("v-feature\"").count(), 2);
        assert!(s.contains("PLANNED") && s.contains("<h3"));
    }

    #[test]
    fn the_comparison_table_is_a_real_table_and_can_highlight_the_last_row() {
        let s = ComparisonTable::new(&["Approach", "Model"]).row(&["VPN", "Network"]).row(&["SkiMasque", "Capability"]).highlight_last().html();
        let s = s.as_str();
        assert!(s.contains("<table") && s.contains("<th") && s.contains(r#"scope="col""#));
        assert_eq!(s.matches("v-row-highlight").count(), 1);
    }

    #[test]
    fn faq_items_are_details_with_paragraphs() {
        let s = Faq::new().item("Is SkiMasque a VPN?", &["Not conceptually.", "It grants access."]).html();
        let s = s.as_str();
        assert!(s.contains("<details") && s.contains("<summary>Is SkiMasque a VPN?") && s.matches("<p>").count() >= 2);
    }

    #[test]
    fn tier_cards_show_price_and_only_paid_tiers_carry_planned() {
        let free = TierCard::new("Free", "$0", "For evaluation.").include("core policies").live().html();
        assert!(free.as_str().contains("$0") && free.as_str().contains("Available now") && !free.as_str().contains("PLANNED"));
        let team = TierCard::new("Team", "$49 / month", "Small teams.").planned("paid plans").html();
        let team = team.as_str();
        assert!(team.contains("PLANNED"));
        let note = team.find("paid plans").unwrap();
        assert!(team.find("v-sr").map_or(true, |sr| note < sr), "the note is visible, not only in sr-only text");
    }

    #[test]
    fn a_cta_band_and_planned_block_render() {
        let b = CtaBand::new("Network access should be temporary.").line("Give it to them.").cta(Cta::primary("Create Your First Policy", "x")).html();
        assert!(b.as_str().contains("v-cta-band") && b.as_str().contains("Create Your First Policy"));
        let p = PlannedBlock::new("command wrapper", &Prose::new().p("skimasque exec")).html();
        assert!(p.as_str().starts_with("<aside class=\"v-planned-block\"") && p.as_str().contains("PLANNED") && p.as_str().contains("skimasque exec"));
    }

    #[test]
    fn chrome_strings_are_escaped() {
        let h = Hero::new("<script>x</script>").lead("<i>y</i>").html();
        assert!(!h.as_str().contains("<script>") && !h.as_str().contains("<i>y"));
        let c = ComparisonTable::new(&["<h>"]).row(&["<td>"]).html();
        assert!(!c.as_str().contains("<h>") && !c.as_str().contains("<td>x"));
    }
}
```
  (A live tier shows the small text "Available now" (`v-tier-live`), never a status badge.)

- [ ] **Step 2: Implement `src/chrome.rs`** (above the tests):

```rust
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
        Self { label: label.into(), href: href.into(), primary: true }
    }
    pub fn secondary(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self { label: label.into(), href: href.into(), primary: false }
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
    pub fn sub(mut self, t: impl Into<String>) -> Self {
        self.blocks.push(Block::Sub(t.into()));
        self
    }
    pub fn list(mut self, items: &[&str]) -> Self {
        self.blocks.push(Block::List(items.iter().map(|s| (*s).to_owned()).collect()));
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
        Self { title: title.into(), eyebrow: None, leads: Vec::new(), ctas: Vec::new(), aside: None }
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
        Self { title: title.into(), id: None, eyebrow: None, alt: false, bodies: Vec::new() }
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
        self.features.push(Feature { title: title.into(), body: body.into(), planned: None });
        self
    }
    pub fn planned(mut self, title: impl Into<String>, body: impl Into<String>, note: impl Into<String>) -> Self {
        self.features.push(Feature { title: title.into(), body: body.into(), planned: Some(note.into()) });
        self
    }
    fn planned_html(note: &str) -> Html {
        Planned::new().note(note.to_owned()).html()
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
        Self { title: title.into(), lines: Vec::new(), ctas: Vec::new() }
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
        Self { head: head.iter().map(|s| (*s).to_owned()).collect(), rows: Vec::new(), highlight_last: false }
    }
    pub fn row(mut self, cells: &[&str]) -> Self {
        self.rows.push(cells.iter().map(|s| (*s).to_owned()).collect());
        self
    }
    pub fn highlight_last(mut self) -> Self {
        self.highlight_last = true;
        self
    }
    fn is_last(&self, i: usize) -> bool {
        self.highlight_last && i + 1 == self.rows.len()
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
        self.items.push(FaqItem { question: question.into(), answer: answer.iter().map(|s| (*s).to_owned()).collect() });
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
    pub fn new(name: impl Into<String>, price: impl Into<String>, tagline: impl Into<String>) -> Self {
        Self { name: name.into(), price: price.into(), tagline: tagline.into(), includes: Vec::new(), live: false, planned: None }
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
    fn planned_html(&self) -> Option<Html> {
        self.planned.as_ref().map(|n| Planned::new().note(n.clone()).html())
    }
}
impl Component for TierCard {}

/// A block of content the product does not deliver yet. Do not nest one inside another.
#[derive(Template, Debug, Clone)]
#[template(path = "planned_block.html")]
pub struct PlannedBlock {
    pub note: String,
    pub inner: Html,
}
impl PlannedBlock {
    pub fn new(note: impl Into<String>, c: &impl Component) -> Self {
        Self { note: note.into(), inner: c.html() }
    }
    fn planned_html(&self) -> Html {
        Planned::new().html()
    }
}
impl Component for PlannedBlock {}
```

  Templates (LF, no colours; if `{% match %}` is rejected by askama 0.12 use an `if let`/`else if` chain on `Block` helper methods and note it):
  - `prose.html`: `<div class="v-prose">{% for b in blocks %}{% match b %}{% when Block::P with (t) %}<p>{{ t }}</p>{% when Block::Lead with (t) %}<p class="v-lead">{{ t }}</p>{% when Block::Sub with (t) %}<h3>{{ t }}</h3>{% when Block::List with (items) %}<ul>{% for i in items %}<li>{{ i }}</li>{% endfor %}</ul>{% when Block::Quote with (t) %}<blockquote>{{ t }}</blockquote>{% when Block::Kv with (k, v) %}<dl class="v-kv"><dt>{{ k }}</dt><dd>{{ v }}</dd></dl>{% endmatch %}{% endfor %}</div>`
  - `hero.html`: `<section class="v-hero"><div class="v-wrap v-hero-grid"><div class="v-hero-copy">{% if let Some(e) = eyebrow %}<p class="v-eyebrow">{{ e }}</p>{% endif %}<h1>{{ title }}</h1>{% for l in leads %}<p class="v-lead">{{ l }}</p>{% endfor %}{% if !ctas.is_empty() %}<p class="v-cta-row">{% for c in ctas %}<a class="v-btn{% if !c.primary %} v-btn-quiet{% endif %}" href="{{ c.href }}">{{ c.label }}</a>{% endfor %}</p>{% endif %}</div>{% if let Some(a) = aside %}<div class="v-hero-aside">{{ a|safe }}</div>{% endif %}</div></section>`
  - `section.html`: `<section class="v-section{% if alt %} v-section-alt{% endif %}"{% if let Some(i) = id %} id="{{ i }}"{% endif %}><div class="v-wrap">{% if let Some(e) = eyebrow %}<p class="v-eyebrow">{{ e }}</p>{% endif %}<h2>{{ title }}</h2>{% for b in bodies %}{{ b|safe }}{% endfor %}</div></section>`
  - `feature_grid.html`: `<div class="v-features">{% for f in features %}<article class="v-feature"><h3>{{ f.title }}</h3><p>{{ f.body }}</p>{% if let Some(n) = f.planned %}<p class="v-run-planned"><span>{{ n }}</span> {{ Self::planned_html(n.as_str())|safe }}</p>{% endif %}</article>{% endfor %}</div>`
  - `cta_band.html`: `<section class="v-cta-band"><div class="v-wrap"><h2>{{ title }}</h2>{% for l in lines %}<p>{{ l }}</p>{% endfor %}<p class="v-cta-row">{% for c in ctas %}<a class="v-btn" href="{{ c.href }}">{{ c.label }}</a>{% endfor %}</p></div></section>`
  - `comparison_table.html`: `<div class="v-table-wrap"><table class="v-table"><thead><tr>{% for h in head %}<th scope="col">{{ h }}</th>{% endfor %}</tr></thead><tbody>{% for r in rows %}<tr{% if self.is_last(loop.index0) %} class="v-row-highlight"{% endif %}>{% for c in r %}{% if loop.first %}<th scope="row">{{ c }}</th>{% else %}<td>{{ c }}</td>{% endif %}{% endfor %}</tr>{% endfor %}</tbody></table></div>`
  - `faq.html`: `<div class="v-faq">{% for i in items %}<details class="v-faq-item"><summary>{{ i.question }}</summary>{% for p in i.answer %}<p>{{ p }}</p>{% endfor %}</details>{% endfor %}</div>`
  - `tier_card.html`: `<article class="v-card v-tier"><header class="v-card-head"><h3 class="v-card-title">{{ name }}</h3>{% if live %}<span class="v-tier-live">Available now</span>{% endif %}</header><p class="v-tier-price">{{ price }}</p><p class="v-card-meta">{{ tagline }}</p>{% if !includes.is_empty() %}<ul class="v-tier-includes">{% for i in includes %}<li>{{ i }}</li>{% endfor %}</ul>{% endif %}{% if let Some(p) = self.planned_html() %}{{ p|safe }}{% endif %}</article>`
  - `planned_block.html`: `<aside class="v-planned-block"><p class="v-planned-note">{{ self.planned_html()|safe }} <span>{{ note }}</span></p>{{ inner|safe }}</aside>` — the visible note sits beside the marker (plan-2 lesson: never leave the reason in sr-only text).

  The Planned note must be **visible** (plan-2 lesson): `feature_grid.html` (above) reuses the plan-3 `.v-run-planned` class, and `tier_card.html` must render its planned marker the same way — replace `{% if let Some(p) = self.planned_html() %}{{ p|safe }}{% endif %}` with `{% if let Some(n) = planned %}<p class="v-run-planned"><span>{{ n }}</span> {{ self.planned_html().unwrap()|safe }}</p>{% endif %}` (or make the helper return `Html` unconditionally). Add the same "note appears before any `v-sr` span" assertion for the feature card as for the tier card above.

- [ ] **Step 3: CSS layer** (append to `static/visual.css`; roles only; LF):

```css
/* site content */
.v-wrap { box-sizing: border-box; width: 100%; max-width: 1120px; margin: 0 auto; padding: 0 20px; }
.v-prose { display: grid; gap: 12px; max-width: 68ch; font: 15px/1.7 var(--font); color: var(--text); }
.v-prose > * { margin: 0; overflow-wrap: anywhere; }
.v-prose h3 { font-size: 15px; font-weight: 600; margin-top: 8px; }
.v-prose ul { padding-left: 20px; display: grid; gap: 4px; }
.v-prose blockquote { padding: 8px 16px; border-left: 3px solid var(--v-active); background: var(--accent-soft); font-weight: 600; }
.v-lead { font-size: 17px; line-height: 1.6; color: var(--text-soft); margin: 0; }
.v-kv { margin: 0; display: grid; gap: 2px; }
.v-kv dt { color: var(--text-muted); font-size: 11px; font-weight: 600; letter-spacing: .1em; }
.v-kv dd { margin: 0; font-weight: 600; overflow-wrap: anywhere; }
.v-eyebrow { margin: 0 0 8px; font: 600 11px/1.4 var(--font); letter-spacing: .12em; text-transform: uppercase; color: var(--accent-text); }
.v-hero { padding: 56px 0 40px; background: var(--bg); color: var(--text); font-family: var(--font); }
.v-hero-grid { display: grid; gap: 32px; align-items: center; }
.v-hero h1 { margin: 0 0 16px; font-size: clamp(26px, 5vw, 40px); line-height: 1.2; font-weight: 600; overflow-wrap: anywhere; }
.v-hero-copy { display: grid; gap: 12px; align-content: start; min-width: 0; }
.v-hero-aside { min-width: 0; }
@media (min-width: 900px) { .v-hero-grid { grid-template-columns: minmax(0, 3fr) minmax(0, 2fr); } }
.v-cta-row { margin: 8px 0 0; display: flex; flex-wrap: wrap; gap: 12px; }
.v-btn-quiet { background: transparent; color: var(--text); border: 1px solid var(--line-strong); }
.v-btn-quiet:hover { background: var(--hover); }
.v-section { padding: 48px 0; background: var(--bg); color: var(--text); font-family: var(--font); }
.v-section-alt { background: var(--surface-raised); }
.v-section > .v-wrap { display: grid; gap: 20px; }
.v-section h2 { margin: 0; font-size: clamp(20px, 3.5vw, 28px); line-height: 1.3; font-weight: 600; overflow-wrap: anywhere; }
.v-features { display: grid; gap: 16px; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); }
.v-feature { min-width: 0; padding: 16px; border: 1px solid var(--line-default); border-radius: var(--radius-lg); background: var(--surface); display: grid; gap: 8px; align-content: start; }
.v-feature h3 { margin: 0; font-size: 15px; font-weight: 600; }
.v-feature p { margin: 0; color: var(--text-soft); overflow-wrap: anywhere; }
.v-cta-band { padding: 48px 0; background: var(--surface-raised); color: var(--text); font-family: var(--font); }
.v-cta-band .v-wrap { display: grid; gap: 8px; justify-items: start; }
.v-cta-band h2 { margin: 0; font-size: clamp(20px, 3.5vw, 28px); font-weight: 600; }
.v-cta-band p { margin: 0; color: var(--text-soft); }
.v-table-wrap { overflow-x: auto; max-width: 100%; }
.v-table { border-collapse: collapse; width: 100%; min-width: 480px; font: 14px/1.5 var(--font); color: var(--text); }
.v-table th, .v-table td { text-align: left; vertical-align: top; padding: 10px 12px; border-bottom: 1px solid var(--line-default); overflow-wrap: anywhere; }
.v-table thead th { font-size: 11px; letter-spacing: .1em; text-transform: uppercase; color: var(--text-muted); }
.v-row-highlight th, .v-row-highlight td { background: var(--accent-soft); font-weight: 600; }
.v-faq { display: grid; gap: 8px; max-width: 72ch; font: 15px/1.7 var(--font); color: var(--text); }
.v-faq-item { border: 1px solid var(--line-default); border-radius: var(--radius-md); background: var(--surface); padding: 0 16px; }
.v-faq-item summary { cursor: pointer; padding: 12px 0; font-weight: 600; }
.v-faq-item summary:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.v-faq-item p { margin: 0 0 12px; color: var(--text-soft); }
.v-tier-price { margin: 0; font-size: 22px; font-weight: 600; }
.v-tier-live { color: var(--accent-text); font-size: 12px; font-weight: 600; }
.v-tier-includes { margin: 0; padding-left: 18px; color: var(--text-soft); display: grid; gap: 2px; }
.v-planned-block { padding: 16px; border: 1px dashed var(--v-neutral); border-radius: var(--radius-lg); display: grid; gap: 8px; max-width: 100%; box-sizing: border-box; }
.v-planned-note { margin: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; color: var(--text-soft); }
```

- [ ] **Step 4: Gallery** — group "Site content" (`wide: true`) with one item each: hero (with a `Node` aside), section, feature grid (one planned), comparison table, faq, tier cards (live free + planned team), cta band, planned block. Gallery test additions: `for s in ["v-hero", "v-features", "v-table", "v-faq", "v-tier", "v-cta-band", "v-planned-block"]`, and `!page.contains("skimasque exec")` outside… the gallery may show `PlannedBlock` demo text "skimasque exec": **do not** — use "command wrapper (planned)" in the demo to keep the gallery free of the string `exec` (its existing rule).

- [ ] **Step 5:** run everything, rustfmt edited `.rs`, regenerate, commit `feat(visual): site content components`.

---

### Task 2: Site chrome — nav, footer, page shell

**Files:** Create `src/site_chrome.rs`, `templates/{site_nav,site_footer,site_page}.html`. Modify `src/lib.rs` (`pub mod site_chrome;` + `pub use site_chrome::{SiteFooter, SiteNav, SitePage, NAV_GROUPS};`), `static/visual.css`, `tests/css.rs`.

**Interfaces:**
- Produces:
  - `pub const NAV_GROUPS: &[(&str, &[(&str, &str)])]` — (group, [(label, path)]) exactly: Product → How It Works `how-it-works`, Identities `identities`, Policies `policies`, CI/CD `ci-cd`, Developers `developers`, Security `security`, Deployment `deployment`; Compare → How SkiMasque Compares `compare`; Technology → Architecture `architecture`, MASQUE `technology/masque`; Resources → Documentation `docs`, Use Cases `use-cases`, Open Source `open-source`, FAQ `faq`. Plus the top-level `Pricing` → `pricing`.
  - `SiteNav::new(root: &str, current: &str) -> SiteNav` (`current` is the page path such as `"how-it-works"`, `""` for home); `SiteFooter::new(root: &str)`.
  - `SitePage::new(root, current, title, description) -> SitePage`, `.push(&impl Component)` (repeatable body parts), `.html()` (full document as `Html`). Title rendering: `"{title} · SkiMasque"` (home: `"SkiMasque — identity-aware network access"` when `current` is empty and title is empty — pass the full home title explicitly instead: rule is `title` is used verbatim; pages pass e.g. `"How it works · SkiMasque"`).
  - Link helper `pub fn link(root: &str, path: &str) -> String` → `"{root}{path}/"` (`"{root}"` for the empty path), used by nav, footer, and pages.

- [ ] **Step 1: Failing tests** (`src/site_chrome.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Prose};

    #[test]
    fn links_are_relative_and_directory_style() {
        assert_eq!(link("", "how-it-works"), "how-it-works/");
        assert_eq!(link("../", "technology/masque"), "../technology/masque/");
        assert_eq!(link("../../", ""), "../../");
    }

    #[test]
    fn the_nav_lists_every_group_marks_the_current_page_and_has_a_mobile_menu() {
        let s = SiteNav::new("../", "policies").html();
        let s = s.as_str();
        for label in ["How It Works", "Identities", "Policies", "CI/CD", "Developers", "Security", "Deployment", "How SkiMasque Compares", "Architecture", "MASQUE", "Documentation", "Use Cases", "Open Source", "FAQ", "Pricing", "Sign In", "Get Started"] {
            assert!(s.contains(label), "{label}");
        }
        assert_eq!(s.matches(r#"aria-current="page""#).count(), 2, "desktop and mobile menu each mark the current link");
        assert!(s.contains(r#"href="../policies/""#) && s.contains(r#"href="../technology/masque/""#));
        assert!(s.contains("<details class=\"v-menu\"") && s.contains("<summary>Menu</summary>"));
        assert!(s.contains("https://control.skimasque.com"));
    }

    #[test]
    fn the_home_page_marks_the_brand_current() {
        let s = SiteNav::new("", "").html();
        assert!(s.as_str().contains(r#"class="v-brand" aria-current="page""#));
    }

    #[test]
    fn the_footer_has_the_canonical_columns_and_shows_missing_pages_as_planned() {
        let s = SiteFooter::new("").html();
        let s = s.as_str();
        for label in ["Identity-aware network access", "Product", "Resources", "Company", "Legal", "About", "Contact", "Status", "Privacy", "Terms"] {
            assert!(s.contains(label), "{label}");
        }
        assert!(s.contains("v-contour") && s.contains("v-mountain"));
        assert!(s.matches("PLANNED").count() >= 2, "Privacy and Terms have no pages yet");
        assert!(!s.contains(r#"href="privacy"#));
    }

    #[test]
    fn a_page_is_a_complete_document_with_a_skip_link_and_relative_assets() {
        let s = SitePage::new("../", "policies", "Policies · SkiMasque", "How policies decide access.")
            .push(&Prose::new().p("hello"))
            .html();
        let s = s.as_str();
        assert!(s.starts_with("<!doctype html>") && s.contains(r#"<html lang="en">"#));
        assert!(s.contains("<title>Policies · SkiMasque</title>") && s.contains(r#"name="description" content="How policies decide access.""#));
        assert!(s.contains(r#"href="../assets/visual.css""#) && s.contains(r#"href="../favicon.svg""#));
        assert!(s.contains(r##"<a class="v-skip" href="#main">"##) && s.contains(r#"<main id="main">"#));
        assert!(s.contains(r#"<svg class="v-sprite""#), "icon sprite once");
        assert!(s.find("v-site-header").unwrap() < s.find("<main").unwrap() && s.find("</main>").unwrap() < s.find("v-site-footer").unwrap());
    }

    #[test]
    fn page_metadata_is_escaped() {
        let s = SitePage::new("", "", "<script>x</script>", "a \"b\" <i>").html();
        assert!(!s.as_str().contains("<script>x") && !s.as_str().contains("<i>"));
    }
}
```

- [ ] **Step 2: Implement `src/site_chrome.rs`:**

```rust
//! Public-site chrome: the navigation, the footer and the page shell.
//! Links are relative (the site is served from a project path), so every
//! component takes the page's `root` prefix ("", "../", "../../").

use askama::Template;

use crate::{Component, Contour, Html, Icons, Mountain, Planned};

pub const SIGN_IN_URL: &str = "https://control.skimasque.com";

pub const NAV_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Product",
        &[
            ("How It Works", "how-it-works"),
            ("Identities", "identities"),
            ("Policies", "policies"),
            ("CI/CD", "ci-cd"),
            ("Developers", "developers"),
            ("Security", "security"),
            ("Deployment", "deployment"),
        ],
    ),
    ("Compare", &[("How SkiMasque Compares", "compare")]),
    ("Technology", &[("Architecture", "architecture"), ("MASQUE", "technology/masque")]),
    (
        "Resources",
        &[("Documentation", "docs"), ("Use Cases", "use-cases"), ("Open Source", "open-source"), ("FAQ", "faq")],
    ),
];

/// A directory-style relative link: `link("../", "policies")` → `../policies/`.
pub fn link(root: &str, path: &str) -> String {
    if path.is_empty() {
        root.to_owned()
    } else {
        format!("{root}{path}/")
    }
}

pub struct NavLink {
    pub label: &'static str,
    pub href: String,
    pub current: bool,
}

pub struct NavGroup {
    pub title: &'static str,
    pub links: Vec<NavLink>,
}

#[derive(Template, Debug, Clone)]
#[template(path = "site_nav.html")]
pub struct SiteNav {
    pub root: String,
    pub current: String,
}
impl SiteNav {
    pub fn new(root: &str, current: &str) -> Self {
        Self { root: root.to_owned(), current: current.to_owned() }
    }
    fn home(&self) -> String {
        link(&self.root, "")
    }
    fn home_current(&self) -> bool {
        self.current.is_empty()
    }
    fn groups(&self) -> Vec<NavGroup> {
        NAV_GROUPS
            .iter()
            .map(|(title, links)| NavGroup {
                title,
                links: links
                    .iter()
                    .map(|(label, path)| NavLink { label, href: link(&self.root, path), current: *path == self.current })
                    .collect(),
            })
            .collect()
    }
    fn pricing(&self) -> NavLink {
        NavLink { label: "Pricing", href: link(&self.root, "pricing"), current: self.current == "pricing" }
    }
    fn sign_in(&self) -> &'static str {
        SIGN_IN_URL
    }
    fn get_started(&self) -> &'static str {
        crate::site_chrome::GET_STARTED_URL
    }
}
impl Component for SiteNav {}

pub const GET_STARTED_URL: &str =
    "https://github.com/skimasque-dev/skimasque/blob/main/docs/getting-started.md";

#[derive(Template, Debug, Clone)]
#[template(path = "site_footer.html")]
pub struct SiteFooter {
    pub root: String,
}
impl SiteFooter {
    pub fn new(root: &str) -> Self {
        Self { root: root.to_owned() }
    }
    fn href(&self, path: &str) -> String {
        link(&self.root, path)
    }
    fn contour(&self) -> Html {
        Contour.html()
    }
    fn mountain(&self) -> Html {
        Mountain.html()
    }
    fn planned(&self) -> Html {
        Planned::new().html()
    }
}
impl Component for SiteFooter {}

#[derive(Template, Debug, Clone)]
#[template(path = "site_page.html")]
pub struct SitePage {
    pub root: String,
    pub current: String,
    pub title: String,
    pub description: String,
    pub parts: Vec<Html>,
}
impl SitePage {
    pub fn new(root: &str, current: &str, title: &str, description: &str) -> Self {
        Self { root: root.to_owned(), current: current.to_owned(), title: title.to_owned(), description: description.to_owned(), parts: Vec::new() }
    }
    pub fn push(mut self, c: &impl Component) -> Self {
        self.parts.push(c.html());
        self
    }
    fn sprite(&self) -> Html {
        Icons.html()
    }
    fn nav(&self) -> Html {
        SiteNav::new(&self.root, &self.current).html()
    }
    fn footer(&self) -> Html {
        SiteFooter::new(&self.root).html()
    }
}
impl Component for SitePage {}
```

  Templates:
  - `site_nav.html`: `<header class="v-site-header"><div class="v-wrap v-site-bar"><a class="v-brand" href="{{ self.home() }}"{% if self.home_current() %} aria-current="page"{% endif %}>SkiMasque</a><nav class="v-site-nav" aria-label="Primary">{% for g in self.groups() %}<details class="v-nav-group"><summary>{{ g.title }}</summary><ul>{% for l in g.links %}<li><a href="{{ l.href }}"{% if l.current %} aria-current="page"{% endif %}>{{ l.label }}</a></li>{% endfor %}</ul></details>{% endfor %}{% let p = self.pricing() %}<a class="v-nav-top" href="{{ p.href }}"{% if p.current %} aria-current="page"{% endif %}>{{ p.label }}</a></nav><div class="v-site-actions"><a class="v-btn v-btn-quiet" href="{{ self.sign_in() }}">Sign In</a><a class="v-btn" href="{{ self.get_started() }}">Get Started</a></div><details class="v-menu"><summary>Menu</summary><div class="v-menu-panel">{% for g in self.groups() %}<p class="v-menu-title">{{ g.title }}</p><ul>{% for l in g.links %}<li><a href="{{ l.href }}"{% if l.current %} aria-current="page"{% endif %}>{{ l.label }}</a></li>{% endfor %}</ul>{% endfor %}<p class="v-menu-title">Pricing</p><ul><li><a href="{{ p.href }}"{% if p.current %} aria-current="page"{% endif %}>Pricing</a></li></ul><p class="v-cta-row"><a class="v-btn v-btn-quiet" href="{{ self.sign_in() }}">Sign In</a><a class="v-btn" href="{{ self.get_started() }}">Get Started</a></p></div></details></div></header>` — askama scoping: `{% let p = self.pricing() %}` inside the `<nav>` is visible later in the same template scope; if 0.12 scopes it away, call `self.pricing()` again (a second `{% let %}` in the menu). The test expects two `aria-current="page"` for a current group link (one in the desktop nav, one in the mobile menu).
  - `site_footer.html`: `<footer class="v-site-footer"><div class="v-footer-art" aria-hidden="true">{{ self.contour()|safe }}{{ self.mountain()|safe }}</div><div class="v-wrap v-footer-grid"><div class="v-footer-brand"><p class="v-footer-name">SkiMasque</p><p>Identity-aware network access<br>for developers and workloads.</p></div><nav aria-label="Footer"><div class="v-footer-col"><p class="v-footer-title">Product</p><ul><li><a href="{{ self.href("how-it-works") }}">How It Works</a></li><li><a href="{{ self.href("identities") }}">Identities</a></li><li><a href="{{ self.href("policies") }}">Policies</a></li><li><a href="{{ self.href("ci-cd") }}">CI/CD</a></li><li><a href="{{ self.href("developers") }}">Developers</a></li><li><a href="{{ self.href("security") }}">Security</a></li><li><a href="{{ self.href("deployment") }}">Deployment</a></li></ul></div><div class="v-footer-col"><p class="v-footer-title">Resources</p><ul><li><a href="{{ self.href("docs") }}">Documentation</a></li><li><a href="{{ self.href("architecture") }}">Architecture</a></li><li><a href="{{ self.href("technology/masque") }}">MASQUE</a></li><li><a href="{{ self.href("open-source") }}">Open Source</a></li><li><a href="{{ self.href("faq") }}">FAQ</a></li></ul></div><div class="v-footer-col"><p class="v-footer-title">Company</p><ul><li><a href="{{ self.href("about") }}">About</a></li><li><a href="{{ self.href("contact") }}">Contact</a></li><li><a href="{{ self.href("status") }}">Status</a></li><li><a href="{{ self.href("security") }}">Security</a></li></ul></div><div class="v-footer-col"><p class="v-footer-title">Legal</p><ul><li>Privacy {{ self.planned()|safe }}</li><li>Terms {{ self.planned()|safe }}</li></ul></div></nav></div><p class="v-wrap v-footer-legal">© SkiMasque</p></footer>`
  - `site_page.html`: `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{{ title }}</title><meta name="description" content="{{ description }}"><meta property="og:title" content="{{ title }}"><meta property="og:description" content="{{ description }}"><meta property="og:type" content="website"><link rel="icon" href="{{ root }}favicon.svg" type="image/svg+xml"><link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin><link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600&display=swap"><link rel="stylesheet" href="{{ root }}assets/visual.css"></head><body class="v-site">{{ self.sprite()|safe }}<a class="v-skip" href="#main">Skip to content</a>{{ self.nav()|safe }}<main id="main">{% for p in parts %}{{ p|safe }}{% endfor %}</main>{{ self.footer()|safe }}</body></html>` (the test asserts `<title>Policies · SkiMasque</title>` — write the `·` literally and check LF/encoding).

- [ ] **Step 3: CSS layer** (append; the footer and code stay Pine in both themes — palette tokens, documented):

```css
/* site chrome */
.v-site { margin: 0; background: var(--bg); color: var(--text); font-family: var(--font); }
.v-skip { position: absolute; left: -9999px; top: 8px; padding: 8px 12px; background: var(--accent); color: var(--on-accent); border-radius: var(--radius-sm); z-index: 10; }
.v-skip:focus { left: 8px; }
.v-site-header { position: sticky; top: 0; z-index: 5; background: var(--bg); border-bottom: 1px solid var(--line-default); font: 14px/1.4 var(--font); }
.v-site-bar { display: flex; align-items: center; gap: 20px; min-height: 56px; }
.v-brand { font-weight: 600; letter-spacing: .04em; color: var(--text); text-decoration: none; }
.v-brand[aria-current="page"] { color: var(--accent-text); }
.v-site-nav { display: none; align-items: center; gap: 4px; margin-left: 8px; }
.v-nav-group { position: relative; }
.v-nav-group summary, .v-nav-top { display: block; padding: 8px 10px; border-radius: var(--radius-sm); color: var(--text); cursor: pointer; list-style: none; text-decoration: none; }
.v-nav-group summary::-webkit-details-marker { display: none; }
.v-nav-group summary:hover, .v-nav-top:hover { background: var(--hover); }
.v-nav-group summary:focus-visible, .v-nav-top:focus-visible, .v-menu summary:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.v-nav-group ul { position: absolute; top: 100%; left: 0; z-index: 6; margin: 0; padding: 6px; list-style: none; min-width: 200px; background: var(--surface); border: 1px solid var(--line-default); border-radius: var(--radius-md); box-shadow: var(--shadow); }
.v-nav-group a, .v-menu-panel a:not(.v-btn) { display: block; padding: 6px 10px; color: var(--text); text-decoration: none; border-radius: var(--radius-sm); }
.v-nav-group a:hover, .v-menu-panel a:not(.v-btn):hover { background: var(--hover); }
.v-site-nav a[aria-current="page"], .v-menu-panel a[aria-current="page"] { color: var(--accent-text); font-weight: 600; }
.v-site-actions { display: none; margin-left: auto; gap: 8px; }
.v-menu { margin-left: auto; position: relative; }
.v-menu summary { list-style: none; cursor: pointer; padding: 8px 12px; border: 1px solid var(--line-strong); border-radius: var(--radius-md); }
.v-menu summary::-webkit-details-marker { display: none; }
.v-menu-panel { position: absolute; right: 0; top: calc(100% + 8px); width: min(320px, 90vw); max-height: 80vh; overflow: auto; box-sizing: border-box; padding: 12px; background: var(--surface); border: 1px solid var(--line-default); border-radius: var(--radius-md); box-shadow: var(--shadow); display: grid; gap: 4px; }
.v-menu-panel ul { margin: 0 0 4px; padding: 0; list-style: none; }
.v-menu-title { margin: 8px 0 0; font-size: 11px; font-weight: 600; letter-spacing: .1em; text-transform: uppercase; color: var(--text-muted); }
@media (min-width: 980px) { .v-site-nav, .v-site-actions { display: flex; } .v-menu { display: none; } }
/* the footer is Pine in both themes, so it uses palette tokens, not roles */
.v-site-footer { position: relative; overflow: hidden; margin-top: 0; padding: 48px 0 24px; background: var(--pine); color: var(--slate-300); font: 14px/1.6 var(--font); }
.v-footer-art { position: absolute; inset: 0; pointer-events: none; color: var(--forest); }
.v-footer-art .v-contour { position: absolute; inset: 0; height: 100%; opacity: .5; color: var(--forest); }
.v-footer-art .v-mountain { position: absolute; left: 0; right: 0; bottom: 0; height: 90px; color: var(--pine-950); }
.v-footer-grid { position: relative; display: grid; gap: 32px; }
.v-footer-grid nav { display: grid; gap: 24px; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); }
.v-footer-name { margin: 0 0 8px; font-size: 16px; font-weight: 600; color: var(--snow); }
.v-footer-brand p { margin: 0; }
.v-footer-title { margin: 0 0 8px; font-size: 11px; font-weight: 600; letter-spacing: .1em; text-transform: uppercase; color: var(--ice); }
.v-footer-col ul { margin: 0; padding: 0; list-style: none; display: grid; gap: 4px; }
.v-site-footer a { color: var(--mint); text-decoration: none; }
.v-site-footer a:hover { text-decoration: underline; }
.v-site-footer a:focus-visible { outline: 2px solid var(--mint); outline-offset: 2px; }
.v-site-footer .v-planned { color: var(--slate-300); border-color: var(--slate-300); }
.v-footer-legal { position: relative; margin: 32px auto 0; font-size: 12px; }
@media (min-width: 900px) { .v-footer-grid { grid-template-columns: minmax(200px, 1fr) minmax(0, 3fr); } }
```

  Extend `tests/css.rs`: a `footer_palette_pairs_meet_4_5_to_1` test asserting `--snow`, `--ice`, `--slate-300`, `--mint` on `--pine` (the footer's ground; compute with the existing `contrast()`), and note the mountain silhouette (`--pine-950`) is decorative.

- [ ] **Step 4:** run everything, rustfmt edited `.rs`, regenerate, commit `feat(visual): site navigation, footer and page shell`. (No gallery change; Task 3 shows the shell on real pages.)

---

### Task 3: Site module restructure, page registry, sitegen assets, site-wide tests

**Files:** Move `src/site.rs` → `src/site/gallery.rs` (contents unchanged except `pub(super)` visibility) and create `src/site/mod.rs` (owns `Page`, `pages()`, `stale()`, tests) and `src/site/pages/mod.rs` (empty registry `pub fn all() -> Vec<Page>` returning an empty Vec for now). Modify `src/bin/sitegen.rs` only if needed (it already loops over `pages()`), `.github/workflows/ci.yml` (no change expected), `README.md` (one paragraph on `sitegen`).

**Interfaces:**
- Produces in `site/mod.rs`: `pub struct Page { pub path: &'static str, pub contents: String }` (unchanged), `pub fn pages() -> Vec<Page>` = gallery pages + `assets/visual.css` (the CSS) + `pages::all()`; `pub fn stale(root) -> Vec<String>` now reports orphans anywhere under `root` except `KEEP = ["favicon.svg", "index.html"]` (Task 4 removes `"index.html"` from `KEEP` when the homepage is generated); constants `pub const REPO_URL`, `pub const DOCS_BASE`, `pub const ISSUES_URL`; helper `pub(crate) fn page(root: &str, current: &str, title: &str, description: &str, parts: &[&dyn ...])` — do **not** build a dyn helper: each page module builds `SitePage` itself.
- In `site/pages/mod.rs`: `pub fn all() -> Vec<Page>` calling each page module's `page()` (added by later tasks); a `pub(super) fn doc(file: &str) -> String` (`DOCS_BASE` + file) helper.

- [ ] **Step 1: Failing tests** in `src/site/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn html_pages() -> Vec<Page> {
        pages().into_iter().filter(|p| p.path.ends_with(".html") && !p.path.starts_with("components/")).collect()
    }

    #[test]
    fn rendering_is_deterministic() {
        let a: Vec<_> = pages().into_iter().map(|p| (p.path, p.contents)).collect();
        let b: Vec<_> = pages().into_iter().map(|p| (p.path, p.contents)).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn the_shared_stylesheet_is_published_for_the_pages() {
        let css = pages().into_iter().find(|p| p.path == "assets/visual.css").expect("assets/visual.css").contents;
        assert_eq!(css, crate::CSS);
    }

    #[test]
    fn every_page_has_one_h1_a_title_a_description_and_nav() {
        for p in html_pages() {
            let s = &p.contents;
            assert_eq!(s.matches("<h1").count(), 1, "{}: exactly one h1", p.path);
            assert!(s.contains("<title>") && s.contains(r#"name="description""#), "{}", p.path);
            assert!(s.contains("v-site-header") && s.contains("v-site-footer") && s.contains(r#"<main id="main">"#), "{}", p.path);
        }
    }

    /// Every relative href/src on a generated page resolves to a generated file.
    #[test]
    fn every_relative_link_resolves() {
        let all: Vec<String> = pages().iter().map(|p| p.path.to_owned()).collect();
        let known = |target: &str| all.iter().any(|p| p == target || p == &format!("{target}index.html") || p == &format!("{}/index.html", target.trim_end_matches('/'))) || KEEP.contains(&target);
        for p in html_pages() {
            let dir = p.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            for attr in ["href=\"", "src=\""] {
                for chunk in p.contents.split(attr).skip(1) {
                    let url = chunk.split('"').next().unwrap();
                    if url.is_empty() || url.starts_with('#') || url.contains("://") || url.starts_with("mailto:") || url.starts_with("data:") {
                        continue;
                    }
                    let url = url.split('#').next().unwrap();
                    let mut parts: Vec<&str> = if dir.is_empty() { vec![] } else { dir.split('/').collect() };
                    for seg in url.split('/') {
                        match seg {
                            ".." => { parts.pop(); }
                            "." | "" => {}
                            s => parts.push(s),
                        }
                    }
                    let target = parts.join("/");
                    assert!(known(&target) || known(&format!("{target}/")), "{}: broken link {url:?} → {target:?}", p.path);
                }
            }
        }
    }

    #[test]
    fn honesty_and_voice_rules_hold_on_every_page() {
        for p in html_pages() {
            let mut s = p.contents.clone();
            // Planned blocks may describe what is not built yet, including `skimasque exec`.
            while let Some(start) = s.find("<aside class=\"v-planned-block\"") {
                let end = s[start..].find("</aside>").map(|e| start + e + 8).expect("closed planned block");
                s.replace_range(start..end, "");
            }
            let lower = s.to_lowercase();
            assert!(!lower.contains("skimasque exec"), "{}: `skimasque exec` outside a planned block", p.path);
            for banned in ["zero trust", "zero-trust", "buy now"] {
                assert!(!lower.contains(banned), "{}: {banned}", p.path);
            }
        }
    }

    #[test]
    fn stale_ignores_crlf_reports_missing_changed_and_orphans_and_keeps_hand_written_files() {
        let dir = std::env::temp_dir().join(format!("sitegen-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for p in pages() {
            let path = dir.join(p.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, p.contents.replace('\n', "\r\n")).unwrap();
        }
        std::fs::write(dir.join("favicon.svg"), "<svg/>").unwrap();
        assert!(stale(&dir).is_empty(), "{:?}", stale(&dir));
        std::fs::write(dir.join("assets/visual.css"), "changed").unwrap();
        assert_eq!(stale(&dir), vec!["assets/visual.css"]);
        std::fs::write(dir.join("assets/visual.css"), crate::CSS).unwrap();
        std::fs::create_dir_all(dir.join("old")).unwrap();
        std::fs::write(dir.join("old/gone.html"), "orphan").unwrap();
        assert!(stale(&dir).contains(&"old/gone.html".to_owned()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```
  The gallery tests (`the_gallery_shows_every_primitive_in_both_themes` and the rest) move to `site/gallery.rs`'s `tests` module unchanged. `KEEP` must be `pub(crate) const KEEP: &[&str]` visible to the test.

- [ ] **Step 2: Implement.** `git mv src/site.rs src/site/gallery.rs`; in `gallery.rs` change `pub fn pages()` to `pub(super) fn pages() -> Vec<Page>` (returning `components/index.html` and `components/visual.css`) and `use super::Page;`; delete `Page` and `stale`/`collect_files` from it (they move). `src/site/mod.rs`:

```rust
//! The generated website: the component gallery and the public pages.
//! `sitegen` writes [`pages()`] under `site/`; `sitegen --check` compares.

mod gallery;
pub mod pages;

pub const REPO_URL: &str = "https://github.com/skimasque-dev/skimasque";
pub const DOCS_BASE: &str = "https://github.com/skimasque-dev/skimasque/blob/main/docs/";
pub const ISSUES_URL: &str = "https://github.com/skimasque-dev/skimasque/issues";

/// Files under `site/` that are written by hand and never reported as orphans.
pub(crate) const KEEP: &[&str] = &["favicon.svg", "index.html"];

pub struct Page {
    pub path: &'static str,
    pub contents: String,
}

pub fn pages() -> Vec<Page> {
    let mut v = gallery::pages();
    v.push(Page { path: "assets/visual.css", contents: crate::CSS.to_owned() });
    v.extend(pages::all());
    v
}

/// Paths under `root` that are stale: a generated file that is missing or
/// differs from a fresh render, or a file no page produces (hand-written
/// files in [`KEEP`] excepted). Line endings are normalised so a CRLF
/// checkout is not reported.
pub fn stale(root: &std::path::Path) -> Vec<String> {
    let pages = pages();
    let mut out: Vec<String> = pages
        .iter()
        .filter(|p| match std::fs::read_to_string(root.join(p.path)) {
            Ok(on_disk) => on_disk.replace("\r\n", "\n") != p.contents.replace("\r\n", "\n"),
            Err(_) => true,
        })
        .map(|p| p.path.to_owned())
        .collect();
    let mut found = Vec::new();
    collect_files(root, "", &mut found);
    found.retain(|rel| !pages.iter().any(|p| p.path == rel) && !KEEP.contains(&rel.as_str()));
    found.sort();
    out.extend(found);
    out
}

/// Every file below `dir`, as `/`-separated paths relative to the site root.
fn collect_files(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if entry.path().is_dir() {
            collect_files(&entry.path(), &rel, out);
        } else {
            out.push(rel);
        }
    }
}
```
  `src/site/pages/mod.rs`:

```rust
//! One module per public page. Each exposes `page() -> Page`.

use super::{Page, DOCS_BASE};

pub fn all() -> Vec<Page> {
    Vec::new()
}

/// A link to a file under the repository's `docs/`.
pub(super) fn doc(file: &str) -> String {
    format!("{DOCS_BASE}{file}")
}
```
  Update `lib.rs`'s `pub mod site;` (unchanged path). In `README.md` add a short "Website" note: the site in `site/` is generated by `cargo run -p skimasque-visual --features site --bin sitegen`; CI checks it; `site/favicon.svg` is hand-written.

- [ ] **Step 3:** run everything (`the_shared_stylesheet…` and the orphan test should pass now; link/h1/honesty tests pass vacuously until pages exist — they iterate an empty page list, which is fine but **assert `html_pages()` is non-empty in Task 4**, when the first page lands), rustfmt, regenerate (`site/assets/visual.css` appears), commit `refactor(visual): site module with a page registry, shared stylesheet and site-wide tests`.

---

### Task 4: Homepage, How it works, Identities

**Files:** Create `src/site/pages/{home,how_it_works,identities}.rs`; modify `src/site/pages/mod.rs` (register), `src/site/mod.rs` (`KEEP` loses `"index.html"`), delete nothing by hand (the generated `site/index.html` overwrites the hand-written one; old content stays in git history at commit `735002d`).

**Page contract (all page tasks):** each module has `pub fn page() -> Page` returning `Page { path, contents: SitePage::new(root, current, title, description).push(&…).html().as_str().to_owned() }`. Homepage: `path: "index.html"`, root `""`, current `""`, title `"SkiMasque — identity-aware network access"`. `How it works`: `path: "how-it-works/index.html"`, root `"../"`, current `"how-it-works"`, title `"How it works · SkiMasque"`. Identities similarly (`identities/index.html`). Descriptions: one sentence taken from the page's hero lead. The first pushed part is always a `Hero`; then `Section`s; homepage ends with a `CtaBand`. Diagrams come from `crate::diagrams::public::*` / `platform::*` (call the function and pass it to `Section::push`).

**Copy source:** transcribe the spec text into components (strings verbatim; the spec's ASCII diagrams become library diagrams; its code blocks become `CodeExample`). Deviations are listed per page below and are the **only** allowed ones.

- [ ] **Step 1: Failing tests** per page in its module (`#[cfg(test)]`), e.g. for the homepage:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_homepage_follows_the_canonical_spec() {
        let s = page().contents;
        assert_eq!(page().path, "index.html");
        for want in [
            "Give every workload exactly the network access it needs.",
            "No broad VPN membership.", "No permanent network credentials.", "No standing access.",
            "Your deployment shouldn&#39;t need the whole network.",
            "Network access as a capability.", "WHO", "WHAT", "WHERE", "LIMITS", "ACCESS GRANTED",
            "Identity-aware", "Least privilege", "Short-lived", "Policy-driven", "Developer-friendly", "Real network access",
            "Network access should be temporary.", "Create Your First Policy",
            "Get Started", "See How It Works",
        ] {
            assert!(s.contains(want) || s.contains(&want.replace('\'', "&#39;")), "missing {want:?}");
        }
        assert!(s.contains("v-route"), "the hero route motif");
        assert!(s.contains("PLANNED"), "the command-wrapper feature is planned");
        assert!(s.contains("skimasque connect") || !s.contains("skimasque exec"));
    }
}
```
  Write equivalent tests for the other two pages (required strings below). Apostrophes are escaped by askama (`&#39;`): compare after normalising, as above.

- **Homepage (`/`, spec lines 429–583):**
  - Hero: title, lead, the three "No …" lines (as three `.lead` lines), CTAs `Get Started` (→ `GET_STARTED_URL`) and `See How It Works` (→ `link("", "how-it-works")`); aside: a vertical `Flow` IDENTITY → POLICY → TEMPORARY SESSION → GATEWAY → PRIVATE SERVICE (build locally with `Node`s of kinds Identity, Policy, Session, Gateway, Service and active connections) plus `Route::new().flowing()` beneath it.
  - Problem section ("Your deployment shouldn't need the whole network." + the bullet list + the two paragraphs) with `diagrams::public::traditional_vs_skimasque()`.
  - Product model section ("Network access as a capability." "Every request answers four questions.") with `diagrams::public::policy_model()` **and** a `PolicyExplorer::new(&["acme/widget"], &["terraform"], &["db.prod:5432"], &["20m", "100 Mbps"], true, "policy production-deploy matches")`.
  - Feature grid (six cards, spec 544–568). **Deviation:** "Developer-friendly" reads "Use normal commands with `skimasque connect`, or the GitHub Action in CI." (real); add a seventh card `planned("Command wrapper", "Wrap any command with the access it needs.", "skimasque exec")` and keep "Real network access — Built around MASQUE, HTTP/3, and QUIC." as written.
  - CtaBand (spec 572–582): title, three lines, CTA `Create Your First Policy` → `doc("policies.md")`.
- **How it works (`/how-it-works`, lines 586–706):** hero ("From identity to network access." + lead); a Flow diagram WORKLOAD → IDENTITY → AUTHENTICATE → AUTHORIZE → SESSION → GATEWAY → PRIVATE NETWORK (local flow); six step sections (`Step 1 — Identify` … `Step 6 — Expire`) each a `Section` with `Prose` and, where the spec has a code/example block, a `CodeExample` (Identify: `repository: acme/widget` … `application: terraform`; Session: `SESSION\nStatus: Active\nDestination: db.prod:5432\nDuration: 20m\nBandwidth: 100 Mbps\nGateway: us-west`); Connect uses `diagrams::public::gateway()`; Expire uses `diagrams::public::access_lifecycle()` and the two lines "No manual VPN disconnect." / "No standing session." **Deviation:** none.
- **Identities (`/identities`, lines 709–856):** hero ("Give network access an identity." + the two quoted questions as `Prose::quote`); identity types as a `FeatureGrid` (Developer `developer: alice`, Repository, Workflow, Ref, Application, Workload — body strings as in the spec); "Identity is not authorization" section with `Prose` + `diagrams::public::policy_decision()`; GitHub Actions section with `diagrams::public::github_actions()`; Developer identities section with `diagrams::public::developer_cli()` (**deviation:** the command node says `skimasque connect`, never `exec`); "Identity + application" section with two `PolicySummary` cards side by side inside a `Compare` (alice/terraform vs alice/curl to `db.prod:5432`, limits `—`) — use `Compare::side(title, Tone, &PolicySummary::new(...))`; closing lines as `Prose::p`.

- [ ] **Step 2: Implement** the three modules; register in `pages/mod.rs::all()`; remove `"index.html"` from `KEEP`; add to `site/mod.rs` tests `assert!(!html_pages().is_empty())`.
- [ ] **Step 3:** run everything; regenerate (`site/index.html` is overwritten, `site/how-it-works/index.html`, `site/identities/index.html` appear); rustfmt; commit `feat(site): homepage, how it works and identities pages`.

---

### Task 5: Policies, CI/CD, Developers

**Files:** Create `src/site/pages/{policies,ci_cd,developers}.rs`; register.

**Reuse of accurate existing material** (the retired hand-written homepage, still in git at `git show 735002d:site/index.html`): the Action workflow YAML (CI/CD), the policy TOML and `skimasque policy check` denial output (Policies), the quick-start commands (`skimasque login`, `skimasque org create "Acme"`, `skimasque init`, `skimasque policy test production`) (Developers). Use it verbatim in `CodeExample`s; it is the product's real interface.

- **Policies (`/policies`, lines 860–1003):** hero ("Turn workload identity into network access." + lead); policy model section with `diagrams::public::policy_model()`; "Policy decision" with `diagrams::public::policy_decision()`; **Default deny** section — a prominent `Prose::quote("No matching allow rule means DENY.")` and a small Flow REQUEST → NO MATCH → ACCESS DENIED (Deny node, `ConnKind::Denied`); "Policy examples" — two `CodeExample`s. **Deviation:** the canonical YAML-like examples are not the product's real syntax (real policies are TOML/YAML `[match]`/`[[rules]]`/`[session]`/`[limits]`); use the real TOML from the old homepage for "Production deployment" (`.masque/policies/production.toml`) and add a second real example for developer database access (`name = "developer-db"`, `[match] group = "platform"`, `application = "psql"`, destination `dev-db.internal:5432`, `max_duration = "60m"`), verifying each field against `docs/policies.md` (read it; do not invent fields); "Policy UX" section — show the six-question wizard as a static, non-interactive `Prose::kv` list (Who needs access? acme/widget …) and a `PlannedBlock::new("interactive policy creation wizard in the dashboard", …)` only if the dashboard wizard is not live — it is being built in plan 5, so **use the PlannedBlock**; then the denial explanation (`skimasque policy check production google.com:443 --app terraform` → the real output block from the old homepage).
- **CI/CD (`/ci-cd`, lines 1006–1141):** hero; problem section with `Compare` (GitHub Actions → VPN/VPC → Private network vs the SkiMasque flow) — reuse `traditional_vs_skimasque()`; GitHub Actions flow `diagrams::public::github_actions()`; "Example" section (`PolicySummary` for `acme/infrastructure` / `deploy-production` / `main` / `terraform` / `db.prod:5432` / `20 minutes · 100 Mbps`, then `DecisionBadge` granted, then a `StatusBadge` Expired "After 20 minutes: ACCESS EXPIRED"); "Pull requests vs production" — three small `PolicySummary`s in a row (feature/* → staging-api:443; main → staging-api:443 + db.prod:5432; deploy-production → db.prod:5432 + api.prod:443) with the closing sentence; `diagrams::platform::ci_lifecycle()`; the Action workflow `CodeExample` (from the old homepage, real `skimasque-dev/connect@v1`); "CI/CD integrations" as `Prose::list` — **deviation:** GitHub Actions, generic OIDC, GitLab CI and Buildkite are supported today per the old homepage ("GitLab, Buildkite, and generic OIDC are supported too"); Terraform is a use case; CircleCI and Jenkins are "extensible to" (leave that sentence as in the spec).
- **Developers (`/developers`, lines 1145–1243):** hero ("Run the command. Get the access. Lose the access when you're done." + "SkiMasque should feel like a command execution tool—not a VPN client." → **deviation:** reword to "SkiMasque should feel like a developer tool, not a VPN client."). CLI example: **deviation** — the real commands. Show `CodeExample("connect to a private service", "$ skimasque connect db.internal:5432\n…")` only as far as `docs/cli.md` documents (`skimasque connect <DEST> [client args…]`, which delegates to `skimasque-client`; `skimasque-client socks5` + `ALL_PROXY=socks5h://127.0.0.1:1080`), the quick-start commands, and `skimasque policy check|test|explain` / `why` for local policy work. The canonical `skimasque exec --policy production --app terraform -- terraform apply` flow, "No network ceremony", and the identity/policy/session readout go inside **one** `PlannedBlock::new("skimasque exec — a command wrapper that requests access, runs the command, and lets access expire", …)`, wording written in the future/conditional. "Local development" section with `diagrams::public::developer_cli()` and `diagrams::public::same_command_different_policy()`. Add a `PlannedBlock` note that the control-plane-backed `connect` (resolving the gateway and credential from the session) is not finished (source: README "Not yet done").

- [ ] **Tests** per page: required headings and strings from above; Policies must contain `production.toml`, `No matching allow rule means DENY.` and `PLANNED`; CI/CD must contain `skimasque-dev/connect@v1` and `ACCESS EXPIRED`; Developers must contain `skimasque connect`, `PLANNED`, and — inside a planned block only (covered by the site-wide test) — `skimasque exec`. Add to Developers' test: `assert!(s.contains("v-planned-block") && s.contains("skimasque exec"))`.
- [ ] Register, run everything, regenerate, rustfmt, commit `feat(site): policies, CI/CD and developers pages`.

---

### Task 6: Compare, Deployment, Gateways (+ correct the run-card copy)

**Files:** Create `src/site/pages/{compare,deployment,gateways}.rs`; register. Modify `src/diagrams/platform.rs` (`deployment_models()` copy) and its test; `src/site/gallery.rs` (Blue card lines).

- **Deployment copy correction (do first, with a failing test):** replace the card lines with the canonical spec text (lines 1368–1467): Green `["Control plane: SkiMasque", "Gateway: SkiMasque", "Operations: minimal", "Free tier"]` (still `.planned("paid plans")`); Blue `["Control plane: SkiMasque", "Gateway: Customer", "The gateway lives inside your network"]`; Black `["Control plane: Customer", "Gateway: Customer", "Operations: Customer"]`. Update `deployment_models_always_show_…` to assert these strings, still exactly one `PLANNED`. Update the gallery's "run · Blue" item to the same Blue lines.
- **Compare (`/compare`, lines 1247–1352):** hero ("Network access doesn't have to mean network membership." + two leads); comparison table exactly as the spec's six rows (Traditional VPN / Mesh VPN / ZTNA / Bastion / PAM / SkiMasque) with `.highlight_last()`, and the sentence "This table should remain descriptive…" is an authoring note, **not** page text; sections VPN, Mesh VPN, ZTNA, Bastions, Self-hosted runners (each `Section` + `Prose`; the VPN section includes `traditional_vs_skimasque()`); closing "What should the unit of network access be?" / "The workload and its request." as `Prose::quote`. Wording stays descriptive, never claiming one approach is universally better; **deviation:** the word "ZTNA" may appear (it is a category name); avoid "zero trust" in prose.
- **Deployment (`/deployment`, lines 1354–1483):** hero ("You choose where the network edge lives." + "SkiMasque supports three deployment models."); `diagrams::platform::deployment_models()`; then a `Section` per run (Green Run — SkiMasque Cloud / Blue Run — Your Gateway / Black Run — Self-hosted) with its `TrailMarker`, the one-line summary, the control-plane/gateway/operations `Prose::kv` rows, "Best for" / "useful when" lists as in the spec (Blue's "customer-owned egress IP" stays: a customer-operated gateway egresses from the customer's own address), and the run's flow (a local four-node Flow per the spec's diagram); "Important positioning" section: "The run colors represent operational responsibility, not product quality." with the three lines GREEN/BLUE/BLACK. **Deviation:** Green's section says it is live with a free tier and shows `Planned` next to "paid plans" (visible note).
- **Gateways (`/gateways`, lines 1486–1568):** hero; basic flow (local Flow WORKLOAD → CONTROL PLANE → GATEWAY → CUSTOMER NETWORK → DESTINATION); `diagrams::public::customer_vpc()` under "Customer-operated gateway" with the firewall sentence; "Egress IP" section — **deviation:** state that a customer-operated gateway egresses from the customer's own network address, and add a `PlannedBlock::new("SkiMasque-provided dedicated egress IPs", …)` for the managed case (README: "Dedicated egress IPs … are being built"); `diagrams::public::multiple_gateways()` with "Policy can include an egress region." (`region` is a real policy limit).
- [ ] **Tests:** Compare — six table rows, `v-row-highlight`, the closing quote; Deployment — `Green Run`, `Blue Run`, `Black Run`, `SkiMasque Cloud`, `Your Gateway`, `Self-hosted`, "operational responsibility, not product quality", exactly one `PLANNED` for paid plans **plus** none else (`assert_eq!(s.matches("PLANNED").count(), 1 + footer_planned)` — the footer contributes two `PLANNED` (Privacy, Terms); count only inside `<main>`: slice the string between `<main` and `</main>` in the test); Gateways — `PLANNED` (dedicated egress IP) inside main, `YOUR VPC`.
- [ ] Register, run everything, regenerate, rustfmt, commit `feat(site): compare, deployment and gateways pages`.

---

### Task 7: Security, Architecture, MASQUE, Use cases

**Files:** Create `src/site/pages/{security,architecture,masque,use_cases}.rs`; register. `masque.rs` path: `technology/masque/index.html`, root `"../../"`, current `"technology/masque"`.

- **Security (`/security`, lines 1571–1679):** hero; `diagrams::platform::security_layers()`; `diagrams::platform::no_standing_access()`; Default deny (quote + local flow); "Least privilege" list (identity, application, destination, protocol, port, duration, bandwidth, egress region — verify against `docs/policies.md`; drop any not supported and say so); "Customer firewall" section with the SkiMasque Gateway → Customer Firewall → Private Service flow and the sentence; `diagrams::platform::compartmentalisation()`; "Auditability" section — WHO/WHAT/WHERE/WHEN/WHICH POLICY/WHICH GATEWAY/RESULT as a `FeatureGrid` or `Prose::list`; add the real design principles from the retired homepage ("fail closed", "the gateway checks identity, application, destination, policy, and expiry", "SSRF floor", "keeps working during an outage: cached policy") as a `FeatureGrid` under "How the gateway enforces it", each verified against `docs/security.md` and `docs/threat-model.md` (read them; do not invent claims); link the two docs. **Voice:** no guarantees ("prevents", "cannot"); say "is designed to".
- **Architecture (`/architecture`, lines 1682–1754):** hero; `diagrams::platform::control_data_plane()` and `diagrams::platform::architecture()`; "Control plane" and "Data plane" as two `Section`s with `Prose::list` (spec lists); "Why separation matters" with the two quoted questions.
- **MASQUE (`/technology/masque`, lines 1757–1822):** hero ("Real MASQUE underneath."); `diagrams::platform::masque_stack()` and `connect_udp_sequence()`; HTTP/3 + QUIC, Extended CONNECT, UDP sections as in the spec; **IP support — deviation with real status:** replace "IP support is part of the evolving SkiMasque transport implementation." with the honest statement plus a `ComparisonTable` of the RFCs from the retired homepage (RFC 9297 complete, RFC 9298 complete, connect-tcp draft on by default, **RFC 9484 partial: wire formats complete; TUN forwarding not yet wired** with a visible `Planned` note, RFC 1928 SOCKS5 complete) — copy statuses exactly from `git show 735002d:site/index.html`; "Important positioning" text is an authoring note for the *site's* emphasis — put the two sentences as a quiet closing `Prose::quote` pair.
- **Use cases (`/use-cases`, lines 1827–1898):** hero; one `Section` per case (Terraform, Private APIs, Kubernetes, Database migrations, Developer debugging, Infrastructure automation), each a small local Flow from the spec (Terraform → Production database with a `PolicySummary` for `terraform → db.prod:5432 → 20m`; …). **Deviation:** "Developer debugging" shows `skimasque connect`, not `exec`.
- [ ] **Tests:** Security — `Default deny`-related quote, all five layer words, "Customer firewall", no absolute guarantee words (`assert!(!s.to_lowercase().contains("guarantee"))`); Architecture — `CONTROL PLANE`, `DATA PLANE`, "Should this session exist?"; MASQUE — the five stack layers, `RFC 9484`, `PLANNED` inside main (TUN forwarding), the page path/root correct (`../../assets/visual.css` present); Use cases — six section titles.
- [ ] Register, run everything, regenerate, rustfmt, commit `feat(site): security, architecture, MASQUE and use cases pages`.

---

### Task 8: Open source, Pricing, Docs, FAQ, Trust

**Files:** Create `src/site/pages/{open_source,pricing,docs,faq,trust}.rs`; register.

- **Open source (`/open-source`, lines 1901–1965):** hero; "Open architecture" `Prose::list` (control-plane components where applicable, gateway, protocol implementation, configuration, deployment, extension points) with links to the repo (`REPO_URL`), the crates (`https://crates.io/crates/skimasque-protocol`, `…/skimasque-policy`, `…/skimasque-core`) and `doc("architecture.md")`; the OPEN SOURCE → CORE NETWORKING → SELF-HOSTED Flow; "The hosted service adds" list — **deviation:** items not built (billing, advanced audit/search, enterprise integrations, multi-region orchestration, support) go under a `PlannedBlock::new("hosted-service additions still being built", Prose::list…)` while managed infrastructure, hosted dashboard, organization management and managed gateways stay in the live list (verify: the control plane and dashboard are live per the README); closing quote "Run SkiMasque yourself, or let us operate it for you." License: MIT / Apache-2.0 (from the old homepage).
- **Pricing (`/pricing`, lines 1968–2054):** hero ("Pay for the platform. Choose where traffic runs." + "Pricing should initially remain simple." → drop that authoring sentence). Four `TierCard`s in a new `TierGrid` component added to `chrome.rs` in this task (`pub struct TierGrid { pub cards: Vec<Html> }`, `TierGrid::new().card(&TierCard) -> TierGrid`, template `templates/tier_grid.html` = `<div class="v-tiers">{% for c in cards %}{{ c|safe }}{% endfor %}</div>`, CSS `.v-tiers { display: grid; gap: 16px; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); }`, re-export in `lib.rs`, one unit test that it renders its cards in order). Free `$0` — `.live()` (includes: evaluation, personal projects, small workloads); Team `$49 / month`, Business `$199 / month` — each `.planned("paid plans — billing is not available yet")` with the spec's include lists; Enterprise `Custom` — `.planned("enterprise plans")` with the "Potential capabilities" list where SSO and advanced RBAC each read "(Planned)". "Pricing philosophy" section as the spec. **Owner note:** paid prices are shown as planned, not final.
- **Docs (`/docs`, lines 2057–2112):** a documentation *index* (amendment): hero ("Documentation" + one lead); sections mirroring the canonical nav groups but linking only to existing repo docs — Getting Started → `getting-started.md`; Concepts → `policies.md`, `control-plane.md`, `gateways.md`, `protocol.md`; CI/CD → `github-actions.md`; Developers → `cli.md`; Deployment → `deployment-modes.md`, `gateways.md`, `self-hosting.md`; Security → `security.md`, `threat-model.md`; Reference → `cli.md`, `configuration.md`, `policies.md`; Architecture → `architecture.md`, `protocol.md`; plus `development.md`, `troubleshooting.md`. Every link is `doc("<file>.md")`; **test that each linked file exists in `docs/`** (read the directory from `CARGO_MANIFEST_DIR/../../docs`). Entries in the canonical nav with no doc (Terraform, Generic CI, Local Development, API reference, OIDC, HTTP/3, QUIC…) are omitted, with a single `Prose::p` "More guides are planned." + `Planned` marker.
- **FAQ (`/faq`, lines 2115–2202):** the ten questions and answers via `Faq`, verbatim except: "Can developers use it?" → answer "Yes. The CLI supports local policy work and opening a tunnel with `skimasque connect`; a command wrapper (`skimasque exec`) is planned." — this sentence contains `skimasque exec`, so render that FAQ item's **whole answer** as text that avoids the literal string: write "a command wrapper is planned" and put the literal command only in a `PlannedBlock` below the FAQ ("Planned: `skimasque exec --policy production -- terraform plan`"). Include the default-deny diagram after "What happens if no policy matches?" as a small `Flow` (NO MATCH → DENY).
- **Trust (`/trust`, lines 2205–2243):** hero; one short `Section` per covered topic from the spec's list (service architecture, availability, gateway health, session handling, auditability, operational boundaries, security reporting, incident response, data handling, customer responsibility) — **each states only what is true and documented** (`docs/security.md`, `docs/threat-model.md`, `docs/control-plane.md`; read them) and anything else is a `Planned` line (availability targets, incident response process, security reporting mailbox, formal data-handling documents); the Responsibility model as a `Compare` (SKIMASQUE: control plane, platform, managed gateways / CUSTOMER: policies, destinations, firewall, customer gateways, identity configuration).
- [ ] **Tests:** Open source — `MIT`, three crate links, one `PLANNED` block; Pricing — four tiers, `$0`, `$49 / month`, `$199 / month`, `Custom`, exactly 3 tier `PLANNED` markers inside main (Team, Business, Enterprise) and the Free tier has none; Docs — every `doc(...)` target file exists; FAQ — ten `<details`, `Is SkiMasque a VPN?`, no `skimasque exec` outside planned blocks (site-wide test); Trust — ten topic sections, responsibility `Compare` with two sides, `PLANNED` present.
- [ ] Register, run everything, regenerate, rustfmt, commit `feat(site): open source, pricing, docs, FAQ and trust pages`.

---

### Task 9: About, Contact, Status

**Files:** Create `src/site/pages/{about,contact,status}.rs`; register.

- **About (`/about`, lines 3293–3322):** hero ("Network access should work the way modern infrastructure works." + the three-sentence lead); "Philosophy" section with `Layers::new("Identity, intent, policy, capability and expiration.").row("Identity").row("Intent").row("Policy").row("Capability").row("Expiration")` (downward).
- **Contact (`/contact`, lines 3325–3350; amendment: no working backend):** hero ("Talk to us about your network access problem."); a `Section` "Start a conversation" with `Prose::p` of the discovery question ("How does your infrastructure currently provide network access to CI/CD or developers?") and a primary `Cta::primary("Open an issue", ISSUES_URL)` plus the repo link; a `PlannedBlock::new("contact form (Name, Email, Company, Role)", …)` describing the intended form fields as text (no `<form>` element, no inputs, no `mailto:`). Add the line "Do not force visitors through a sales qualification funnel" **not** as page text (authoring note).
- **Status (`/status`, lines 3353–3377; amendment: static text, no live claims):** hero ("SkiMasque Status"); a `Section` "Services" with `Prose::list(["Control plane", "Gateway service", "Authentication", "API"])` — **no "Operational" status words**; a `PlannedBlock::new("live service status and incident history", Prose::p("Status reporting and historical incidents will appear here."))`. Test: the page contains none of "Operational", "Degraded", "Outage" and contains `PLANNED` inside main.
- [ ] **Tests:** About — the five philosophy words in order; Contact — `github.com/skimasque-dev/skimasque/issues`, no `<form`, no `mailto:`, `PLANNED`; Status — as above.
- [ ] Register, run everything, regenerate, rustfmt, commit `feat(site): about, contact and status pages`.

---

### Task 10: Site-wide sweep, sitemap, docs, spec status

**Files:** Modify `src/site/mod.rs` (add `sitemap.xml` + `robots.txt` pages), `.github/workflows/pages.yml` (path filter already covers `site/**`; no change unless `sitemap.xml` needs none), the amendment spec status.

- [ ] **Step 1: Sitemap and robots** — `pages()` also emits `robots.txt` (`User-agent: *\nAllow: /\nSitemap: <base>/sitemap.xml` — the base URL is unknown for a project site, so emit `robots.txt` with just `User-agent: *` / `Allow: /` and **no** sitemap line) and `sitemap.xml` is **not** generated (needs an absolute base URL; recorded as a ruling). Test: `robots.txt` present, no `Sitemap:`.
- [ ] **Step 2: Whole-suite checks** — the three verify commands and `cargo test --workspace` clean. Add one more site-wide test: every generated page path is reachable from the homepage nav or footer (walk `href`s starting at `index.html`, following only relative links; assert all 20 canonical routes are visited).
- [ ] **Step 3: Browser sweep** (Browser tools, `python -m http.server <port>` from `site/` in the background, stop it by PID only): for every generated page (20) at widths 320, 720 and 1400, in both `prefers-color-scheme` modes if the emulation is available (`resize_window colorScheme`), check `document.documentElement.scrollWidth <= innerWidth`, no `.v-flow-wrap/.v-compare-wrap/.v-branch-wrap/.v-seq-wrap/.v-layers-wrap/.v-table-wrap` clipped beyond its own intended scroll (`.v-table-wrap` may scroll horizontally), no zero-width diagram roots, the mobile menu opens (`details.v-menu[open]`) and lists every page below 980px, the desktop group `<details>` opens above 980px, the skip link is reachable by keyboard, `document.body.innerText.toLowerCase()` contains none of `skimasque exec` outside planned blocks (use the DOM: exclude `.v-planned-block`), and every `.v-planned` is visible with its note. Compute contrast for any new text/background pair not in the role test (e.g. the footer's `.v-planned` in slate-300 on Pine, `.v-tier-live`, `.v-eyebrow`). Report per-page numbers; fix defects with CSS-only or copy changes and re-check. Reset the viewport with preset "desktop".
- [ ] **Step 4: Docs** — README "Website" paragraph mentions the 20 routes; amendment status becomes `approved 2026-09-29; plans 2, 3 and 4 (site chrome and public pages) implemented`; record in the ledger that the `connect` Action page palette is deferred to plan 6.
- [ ] **Step 5:** commit `docs(site): sweep results and status`; stop (the controller runs the final review and the PR).

---

## Self-Review

- **Spec coverage:** chrome §85 (navigation, footer, hero, feature grid, CTA band, comparison table, FAQ, run cards, code example — Tasks 1–2, plan-3 run cards); nav §3 and footer §4 (Task 2); IA §74 (20 routes: `/` T4, how-it-works/identities T4, policies/ci-cd/developers T5, compare/deployment/gateways T6, security/architecture/technology/masque/use-cases T7, open-source/pricing/docs/faq/trust T8, about/contact/status T9); CTA strategy §99 (Global Constraints + per-page CTAs); code example style §88 (`CodeExample` reuse); amendment conditions: Planned markers (`PlannedBlock`, `TierCard.planned`, `FeatureGrid.planned`), `docs` index, static `status`, `contact` without a backend; deployment run copy per canonical §18 (Task 6). Not in this plan (by the delivery table): the `connect` Action page palette (plan 6), the dashboard (plan 5).
- **Placeholders:** none; page tasks transcribe named spec line ranges under explicit, enumerated deviations, and every page has required-string tests plus the site-wide link, h1, honesty and reachability tests.
- **Type consistency:** `Cta`, `Prose`, `Hero`, `Section`, `FeatureGrid`, `CtaBand`, `ComparisonTable`, `Faq`, `TierCard`, `PlannedBlock` (Task 1) are used verbatim by Tasks 4–9; `TierGrid` is added in Task 8 with its own unit test; `SitePage`/`SiteNav`/`SiteFooter`/`link`/`GET_STARTED_URL`/`SIGN_IN_URL` (Task 2) by every page; `site::{Page, pages, stale, KEEP, DOCS_BASE, ISSUES_URL}` and `pages::doc` (Task 3) by Tasks 4–10.
- **Owner-review items:** paid prices are shown ($49/$199) as Planned, not final; "Privacy"/"Terms" appear in the footer as Planned (no pages); no `sitemap.xml` (needs a base URL); the site follows the OS colour scheme (no pinned light default), per the plan-2 decision; contact goes to GitHub Issues.
