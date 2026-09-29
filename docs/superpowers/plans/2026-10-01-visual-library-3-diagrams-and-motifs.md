# Visual Library 3 — Diagrams & Alpine Motifs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the diagram layer to `skimasque-visual`: four layout containers (layers, compare, branch, sequence), Alpine motifs (contour, mountain, route, trail/run markers), state-only motion, the canonical public diagram set (§61), and the data-driven control-plane diagram set (§62), all shown in the generated gallery.

**Architecture:** Same shape as plans 1–2. Layout containers and motifs are askama-template structs implementing `Component`. Each diagram is a plain function that composes existing components (`Flow`, `Boundary`, `Node`, `Connection`, the new containers) and returns a concrete component; a registry (`diagrams::public_set()`, `diagrams::control_set()`) lists them with a slug and title so the gallery and the honesty tests iterate one list. Control-plane diagrams take real data as arguments (the dashboard will call them; the gallery calls them with clearly-labelled example data).

**Tech Stack:** Rust 1.88, askama 0.12, plain CSS (container queries), inline SVG, no JS.

**Spec:** `docs/superpowers/specs/2026-09-30-product-surface-amendment.md` ("Diagrams", "Alpine motifs", "Motion") over `2026-09-29-visual-library-design.md` (Phase 3 diagram content, Motion, Accessibility). Plans 1–2 are merged (this branch is `feat/visual-library-3`, cut from `main` at `81f53e0`).

## Global Constraints

- Work in `crates/skimasque-visual` (repo root = the worktree). Crate stays `publish = false`; feature `site` gates `pub mod site` and the `sitegen` bin.
- askama **0.12**. Nested markup only ever as `Html` from another component; every caller string is escaped by askama.
- **Hex colours only inside `/* tokens */ … /* end tokens */`** of `static/visual.css`; templates (including inline SVG) use `currentColor`, classes and role tokens only. Use literal glyphs (`→ ← ↑ ↓ ✓ ×`), never numeric entities.
- Themes follow the OS with no `data-theme`; `data-theme="light"|"dark"` forces a theme. Every colour a diagram uses is a role token already covered by the contrast test.
- Every composed diagram carries a visually-hidden text-equivalent caption (`class="v-sr"`). Status always shows its word; connections convey allowed/denied in text.
- Decorative SVG (contour, mountain, route) is `aria-hidden="true" focusable="false"` (it carries no information, so it has no `<title>`; this refines the older "every SVG has title/desc" rule and is a recorded ruling).
- **Honesty:** no diagram shows `skimasque exec`; no fabricated IP address (a gateway's egress is described in words: "gateway egress IP"); CONNECT-IP and other unbuilt features are not drawn; anything not built carries the `Planned` marker. The generated gallery never contains the substring `exec`.
- Motion only where it communicates state (active-session flow, authorization reveal, expiry fade); every animation is disabled under `@media (prefers-reduced-motion: reduce)`; a test enforces it.
- Formatting: never run workspace `cargo fmt`; only `rustfmt --edition 2021 <file>` on files you create or edit. **Never edit Rust, CSS, or templates with Python on Windows** (it wrote CRLF and mangled `·` twice): use the Edit/Write tools or bash heredocs, keep files LF (`grep -c $'\r' file` = 0).
- Commits: conventional style, **no AI attribution trailers or footers**.
- Verify commands (repo root): `cargo test -p skimasque-visual --features site`, `cargo clippy -p skimasque-visual --all-targets --features site -- -D warnings`, `cargo run -q -p skimasque-visual --features site --bin sitegen -- --check` (regenerate with the same command minus `-- --check` and commit `site/components/`).

## Review Focus

- Long or hostile strings in any diagram argument (node labels, sequence messages, layer titles): escaped, no page overflow at 320px (Tasks 2, 8).
- Empty inputs: `gateway_topology(&[])`, a `Layers` with no rows, a `Sequence` with no messages, `Compare` with one side: render a sensible result, no broken markup (Tasks 2, 7).
- Diagrams in a narrow column: vertical, no horizontal scroll, connectors still readable; wide: horizontal without overflow (Tasks 2, 8 browser check).
- Reduced motion: every animation has a static equivalent that still shows the state (Task 4 test).
- Owner decision (2026-10-01): Green Run (SkiMasque Cloud) is live with a free tier; billing is not in place, so the Green card shows the free tier and marks **paid plans** Planned (Task 6). The Blue/Black run descriptions are still owner-review before plan 4 publishes the deployment page.

## File Structure

| file | responsibility |
|---|---|
| `src/node.rs`, `templates/icons.html` | four new node kinds + sprite icons (Task 1) |
| `src/layout.rs` + `templates/{layers,compare,branch,sequence}.html` | `Layers`, `Compare`, `Branch`, `Sequence` (Task 2) |
| `src/motif.rs` + `templates/{contour,mountain,route,trail_marker,run_card}.html` | `Contour`, `Mountain`, `Route`, `TrailMarker`, `Run`, `RunCard` (Task 3) |
| `src/motion.rs` + `templates/{reveal,expire}.html` | `Reveal`, `Expire` wrappers (Task 4) |
| `src/diagrams/mod.rs` | `Entry`, `public_set()`, `control_set()`, registry tests |
| `src/diagrams/public.rs` | canonical public diagrams, part A (Task 5) |
| `src/diagrams/platform.rs` | canonical public diagrams, part B (Task 6) |
| `src/diagrams/control.rs` | data-driven control-plane diagrams (Task 7) |
| `static/visual.css` | one CSS layer per task |
| `src/site.rs` | gallery groups (Task 8; each task adds its own group as it lands) |

---

### Task 1: Node kinds for the diagram vocabulary

**Files:** Modify `src/node.rs`, `templates/icons.html`.

**Interfaces:**
- Produces: `NodeKind::{ControlPlane, Cli, Credential, Audit}` with slugs `control-plane`, `cli`, `credential`, `audit`; default labels `Control plane`, `CLI`, `Credential`, `Audit log`; icons `i-control-plane`, `i-cli`, `i-credential`, `i-audit`; tones Structure, Neutral, Neutral, Neutral. `NodeKind::ALL` becomes `[NodeKind; 23]`.

- [ ] **Step 1: Failing test** — in `src/node.rs` tests add:

```rust
    #[test]
    fn the_diagram_vocabulary_kinds_exist_with_slugs_labels_and_tones() {
        for (k, slug, label, icon) in [
            (NodeKind::ControlPlane, "control-plane", "Control plane", "i-control-plane"),
            (NodeKind::Cli, "cli", "CLI", "i-cli"),
            (NodeKind::Credential, "credential", "Credential", "i-credential"),
            (NodeKind::Audit, "audit", "Audit log", "i-audit"),
        ] {
            assert_eq!((k.slug(), k.default_label(), k.icon()), (slug, label, icon));
            assert!(NodeKind::ALL.contains(&k), "{slug} is in ALL");
        }
        assert_eq!(NodeKind::ControlPlane.tone(), Tone::Structure);
        assert_eq!(NodeKind::Cli.tone(), Tone::Neutral);
        assert_eq!(NodeKind::ALL.len(), 23);
    }
```
  Run `cargo test -p skimasque-visual --features site node` → FAIL (compile).

- [ ] **Step 2: Implement** — add the four variants to `NodeKind`; extend `ALL` (type `[NodeKind; 23]`, the four appended), `slug()`, `icon()`, `default_label()` with the values above; `tone()`: `NodeKind::ControlPlane` joins the `Tone::Structure` arm (the others fall into the existing `_ => Tone::Neutral`). In `templates/icons.html`, before the closing `</svg>`, add (LF, same `{{ a|safe }}` attribute style as the other symbols):

```html
<symbol id="i-control-plane" viewBox="0 0 24 24" {{ a|safe }}><rect x="3" y="4" width="18" height="6" rx="1.5"/><rect x="3" y="14" width="18" height="6" rx="1.5"/><path d="M7 7h.01M7 17h.01"/></symbol>
<symbol id="i-cli" viewBox="0 0 24 24" {{ a|safe }}><path d="M4 6l7 6-7 6M13 18h7"/></symbol>
<symbol id="i-credential" viewBox="0 0 24 24" {{ a|safe }}><rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 018 0v3"/></symbol>
<symbol id="i-audit" viewBox="0 0 24 24" {{ a|safe }}><path d="M6 3h12v18H6z"/><path d="M9 8h6M9 12h6M9 16h4"/></symbol>
```
  The existing icons test (every kind has a symbol) and `icon_ids_are_unique` now cover the new ones.

- [ ] **Step 3: Gallery** — the Nodes group already iterates `NodeKind::ALL`; no site.rs change. Run the full commands; `rustfmt --edition 2021 src/node.rs`; regenerate; commit `feat(visual): control plane, CLI, credential and audit node kinds`.

---

### Task 2: Layout containers — Layers, Compare, Branch, Sequence

**Files:** Create `src/layout.rs`, `templates/{layers,compare,branch,sequence}.html`. Modify `src/lib.rs` (`pub mod layout;` + `pub use layout::{Arm, Branch, Compare, LayerRow, Layers, Message, Sequence, Side};`), `static/visual.css`, `src/site.rs` (temporary group "Layout" with one sample of each; Task 8 replaces samples with registry diagrams).

**Interfaces:**
- Consumes: `Component`, `Connection`, `Html`, `Tone`.
- Produces:
  - `Layers::new(caption) -> Layers`, `.upward() -> Layers`, `.row(title) -> Layers`, `.row_sub(title, sub) -> Layers`.
  - `Compare::new(caption) -> Compare`, `.side(title: impl Into<String>, tone: Tone, body: &impl Component) -> Compare` (an empty title renders no heading).
  - `Branch::new(caption, root: &impl Component) -> Branch`, `.arm(conn: Connection, body: &impl Component) -> Branch`.
  - `Sequence::new(caption, left, right) -> Sequence`, `.to_right(label) -> Sequence`, `.to_left(label) -> Sequence`.

- [ ] **Step 1: Failing tests** (`src/layout.rs`, module and tests only first so it compiles to a behavioural failure — create the structs with empty templates, or stub `todo!()`-free minimal versions; record RED):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, ConnKind, Node, NodeKind, Tone};

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
        assert!(Layers::new("x").html().as_str().contains("<ol"), "empty layers still render");
        assert_eq!(Layers::new("x").row("A").row("B").html().as_str().matches("↓").count(), 1);
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
        assert_eq!(s.matches("<h4").count(), 1, "an empty title renders no heading");
        assert!(Compare::new("x").side("only", Tone::Neutral, &a).html().as_str().contains("only"));
    }

    #[test]
    fn branch_puts_the_root_before_its_arms_each_with_a_connection() {
        let root = Node::new(NodeKind::Policy);
        let s = Branch::new("A policy either matches or not.", &root)
            .arm(Connection::new(ConnKind::Active).label("match"), &Node::new(NodeKind::Allow))
            .arm(Connection::new(ConnKind::Denied).label("no match"), &Node::new(NodeKind::Deny))
            .html();
        let s = s.as_str();
        assert!(s.find("v-branch-root").unwrap() < s.find("v-branch-arms").unwrap());
        assert_eq!(s.matches("v-branch-arm\"").count(), 2);
        assert!(s.contains("v-conn-active") && s.contains("v-conn-denied"));
        assert!(Branch::new("none", &root).html().as_str().contains("v-branch-root"), "no arms still renders the root");
    }

    #[test]
    fn sequence_orders_messages_and_speaks_their_direction() {
        let s = Sequence::new("A tunnel is requested and confirmed.", "Client", "Gateway")
            .to_right("CONNECT-UDP request")
            .to_left("tunnel established")
            .html();
        let s = s.as_str();
        assert!(s.find("CONNECT-UDP request").unwrap() < s.find("tunnel established").unwrap());
        assert!(s.contains("→") && s.contains("←"));
        assert!(s.contains("Client to Gateway") && s.contains("Gateway to Client"));
        assert!(Sequence::new("x", "A", "B").html().as_str().contains("v-seq-actor"));
    }

    #[test]
    fn layout_strings_are_escaped() {
        let h = Layers::new("<script>").row("<b>x</b>").html();
        assert!(!h.as_str().contains("<script>") && !h.as_str().contains("<b>x"));
        let q = Sequence::new("c", "<i>", "r").to_right("\"<img>\"").html();
        assert!(!q.as_str().contains("<i>") && !q.as_str().contains("<img>"));
    }
}
```

- [ ] **Step 2: Implement `src/layout.rs`** (above the tests):

```rust
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
        Self { caption: caption.into(), rows: Vec::new(), upward: false }
    }
    pub fn upward(mut self) -> Self {
        self.upward = true;
        self
    }
    pub fn row(mut self, title: impl Into<String>) -> Self {
        self.rows.push(LayerRow { title: title.into(), sub: None });
        self
    }
    pub fn row_sub(mut self, title: impl Into<String>, sub: impl Into<String>) -> Self {
        self.rows.push(LayerRow { title: title.into(), sub: Some(sub.into()) });
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
        Self { caption: caption.into(), sides: Vec::new() }
    }
    pub fn side(mut self, title: impl Into<String>, tone: Tone, body: &impl Component) -> Self {
        self.sides.push(Side { title: title.into(), tone, body: body.html() });
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
    pub fn new(caption: impl Into<String>, root: &impl Component) -> Self {
        Self { caption: caption.into(), root: root.html(), arms: Vec::new() }
    }
    pub fn arm(mut self, conn: Connection, body: &impl Component) -> Self {
        self.arms.push(Arm { conn: conn.html(), body: body.html() });
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
    pub fn new(caption: impl Into<String>, left: impl Into<String>, right: impl Into<String>) -> Self {
        Self { caption: caption.into(), left: left.into(), right: right.into(), messages: Vec::new() }
    }
    pub fn to_right(mut self, label: impl Into<String>) -> Self {
        let spoken = format!("{} to {}", self.left, self.right);
        self.messages.push(Message { label: label.into(), rightward: true, spoken });
        self
    }
    pub fn to_left(mut self, label: impl Into<String>) -> Self {
        let spoken = format!("{} to {}", self.right, self.left);
        self.messages.push(Message { label: label.into(), rightward: false, spoken });
        self
    }
}
impl Component for Sequence {}
```

  Templates (LF, no colours):
  - `layers.html`: `<figure class="v-layers-wrap"><ol class="v-layers{% if upward %} v-layers-up{% endif %}">{% for r in rows %}<li class="v-layer-row"><span class="v-layer-name">{{ r.title }}</span>{% if let Some(s) = r.sub %}<span class="v-layer-sub">{{ s }}</span>{% endif %}</li>{% if !loop.last %}<li class="v-layer-gap" aria-hidden="true">{% if upward %}↑{% else %}↓{% endif %}</li>{% endif %}{% endfor %}</ol><figcaption class="v-sr">{{ caption }}</figcaption></figure>`
  - `compare.html`: `<figure class="v-compare-wrap"><div class="v-compare">{% for s in sides %}<section class="v-compare-side v-tone-{{ s.tone.class() }}">{% if !s.title.is_empty() %}<h4 class="v-compare-title">{{ s.title }}</h4>{% endif %}{{ s.body|safe }}</section>{% endfor %}</div><figcaption class="v-sr">{{ caption }}</figcaption></figure>`
  - `branch.html`: `<figure class="v-branch-wrap"><div class="v-branch"><div class="v-branch-root">{{ root|safe }}</div><ul class="v-branch-arms">{% for a in arms %}<li class="v-branch-arm">{{ a.conn|safe }}{{ a.body|safe }}</li>{% endfor %}</ul></div><figcaption class="v-sr">{{ caption }}</figcaption></figure>`
  - `sequence.html`: `<figure class="v-seq-wrap"><div class="v-seq"><div class="v-seq-actor">{{ left }}</div><div class="v-seq-actor">{{ right }}</div>{% for m in messages %}<div class="v-seq-msg {% if m.rightward %}v-seq-right{% else %}v-seq-left{% endif %}"><span class="v-seq-label">{{ m.label }}</span><span class="v-seq-arrow" aria-hidden="true">{% if m.rightward %}→{% else %}←{% endif %}</span><span class="v-sr">{{ m.spoken }}</span></div>{% endfor %}</div><figcaption class="v-sr">{{ caption }}</figcaption></figure>`

  If askama 0.12 rejects a template construct, make the smallest change that keeps the tested behaviour and record it.

- [ ] **Step 3: CSS layer** (append; roles only; `*-wrap` are inline-size containers like `.v-flow-wrap`):

```css
/* layout containers */
.v-layers-wrap, .v-compare-wrap, .v-branch-wrap, .v-seq-wrap { margin: 0; width: 100%; min-width: 0; box-sizing: border-box; container-type: inline-size; font: 13px/1.5 var(--font); color: var(--text); }
.v-layers { list-style: none; margin: 0; padding: 0; display: grid; justify-items: start; gap: 2px; }
.v-layer-row { box-sizing: border-box; width: min(100%, 360px); display: grid; gap: 2px; padding: 10px 14px; border: 1px solid var(--line-default);
  border-left: 3px solid var(--v-structure); border-radius: var(--radius-md); background: var(--surface); }
.v-layer-name { font-weight: 600; letter-spacing: .08em; overflow-wrap: anywhere; }
.v-layer-sub { color: var(--text-muted); font-size: 12px; overflow-wrap: anywhere; }
.v-layer-gap { margin-left: 20px; line-height: 1; color: var(--v-neutral-text); }
.v-compare { display: grid; gap: 20px; grid-template-columns: minmax(0, 1fr); }
@container (min-width: 720px) { .v-compare { grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); } }
.v-compare-side { min-width: 0; padding: 16px; border: 1px solid var(--line-default); border-top: 3px solid var(--v-tone); border-radius: var(--radius-lg); background: var(--surface); display: grid; gap: 12px; align-content: start; }
.v-compare-title { margin: 0; font-size: 11px; font-weight: 600; letter-spacing: .1em; text-transform: uppercase; color: var(--v-tone-text); }
.v-branch { display: grid; gap: 12px; }
.v-branch-root { min-width: 0; }
.v-branch-arms { list-style: none; margin: 0; padding: 0 0 0 20px; display: grid; gap: 12px; min-width: 0; }
.v-branch-arm { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; min-width: 0; }
@container (min-width: 560px) {
  .v-branch { grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: 16px; }
  .v-branch-arms { padding-left: 0; }
}
.v-seq { display: grid; grid-template-columns: 1fr 1fr; gap: 8px 0; padding: 0 0 4px;
  background: linear-gradient(var(--line-strong), var(--line-strong)) 25% 0 / 2px 100% no-repeat,
              linear-gradient(var(--line-strong), var(--line-strong)) 75% 0 / 2px 100% no-repeat; }
.v-seq-actor { justify-self: center; padding: 6px 12px; border: 1px solid var(--line-default); border-radius: var(--radius-md); background: var(--surface); font-weight: 600; overflow-wrap: anywhere; }
.v-seq-msg { grid-column: 1 / -1; display: flex; flex-wrap: wrap; align-items: baseline; gap: 2px 8px; margin: 0 25%; padding: 2px 0; }
.v-seq-left { flex-direction: row-reverse; }
.v-seq-arrow { color: var(--v-active); font-weight: 700; }
.v-seq-label { overflow-wrap: anywhere; }
```

- [ ] **Step 4: Gallery + verify** — `site.rs` group `Layout` (`wide: true`): a `Layers` (upward, five security layers), a `Compare` of two small flows, a `Branch` (policy → allow/deny), a `Sequence` (Client/Gateway). Extend the gallery test: `for s in ["v-layers", "v-compare", "v-branch", "v-seq"] { assert!(page.contains(s)) }`. Run everything, rustfmt, regenerate, commit `feat(visual): layers, compare, branch and sequence containers`.

---

### Task 3: Alpine motifs and deployment run cards

**Files:** Create `src/motif.rs`, `templates/{contour,mountain,route,trail_marker,run_card}.html`. Modify `src/lib.rs` (`pub mod motif;` + `pub use motif::{Contour, Mountain, Route, Run, RunCard, Shape, TrailMarker};`), `static/visual.css`, `src/site.rs` (group "Motifs").

**Interfaces:**
- Consumes: `Component`, `Html`, `Planned`, `Tone`.
- Produces:
  - `Contour`, `Mountain` — unit structs (decorative SVG). `Route { flowing: bool }` with `Route::new()`, `.flowing()`.
  - `Shape::{Circle, Square, Diamond}`; `TrailMarker { shape: Shape, tone: Tone, label: String }` with `TrailMarker::new(shape, tone, label)`.
  - `Run::{Green, Blue, Black}` with `name() -> &'static str` ("Green Run", "Blue Run", "Black Run"), `shape()` (Circle, Square, Diamond), `tone()` (Active, Info, Neutral); `RunCard { run: Run, technical: String, lines: Vec<String>, planned: Option<String> }` with `RunCard::new(run, technical, &[lines])`, `.planned(note)`.

- [ ] **Step 1: Failing tests** (`src/motif.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Tone};

    #[test]
    fn decorative_svg_is_hidden_from_assistive_tech() {
        for h in [Contour.html(), Mountain.html(), Route::new().html()] {
            let s = h.as_str();
            assert!(s.starts_with("<svg") && s.contains(r#"aria-hidden="true""#) && s.contains(r#"focusable="false""#), "{s}");
        }
        assert!(Mountain.html().as_str().contains("v-mountain-far") && Mountain.html().as_str().contains("v-mountain-near"));
    }

    #[test]
    fn a_route_flows_only_when_asked() {
        assert!(!Route::new().html().as_str().contains("v-route-flowing"));
        assert!(Route::new().flowing().html().as_str().contains("v-route-flowing"));
    }

    #[test]
    fn markers_differ_by_shape_not_only_colour() {
        let shape = |s| TrailMarker::new(s, Tone::Active, "x").html().as_str().to_owned();
        assert!(shape(Shape::Circle).contains("<circle"));
        assert!(shape(Shape::Square).contains("<rect"));
        assert!(shape(Shape::Diamond).contains("<polygon"));
        assert!(shape(Shape::Circle).contains("v-tone-active") && shape(Shape::Circle).contains(">x<"));
    }

    #[test]
    fn a_run_card_always_shows_the_run_and_its_technical_name() {
        for (run, word, shape) in [(Run::Green, "Green Run", "<circle"), (Run::Blue, "Blue Run", "<rect"), (Run::Black, "Black Run", "<polygon")] {
            let s = RunCard::new(run, "Your Gateway", &["a line"]).html().as_str().to_owned();
            assert!(s.contains(word) && s.contains("Your Gateway") && s.contains("a line") && s.contains(shape), "{word}: {s}");
            assert!(!s.contains("PLANNED"));
        }
        let p = RunCard::new(Run::Green, "SkiMasque Cloud", &[]).planned("hosted gateway").html().as_str().to_owned();
        assert!(p.contains("PLANNED") && p.contains("hosted gateway"));
    }

    #[test]
    fn motif_strings_are_escaped() {
        let s = TrailMarker::new(Shape::Circle, Tone::Neutral, "<script>").html();
        assert!(!s.as_str().contains("<script>"));
        let c = RunCard::new(Run::Blue, "<b>x</b>", &["<i>y</i>"]).html();
        assert!(!c.as_str().contains("<b>x") && !c.as_str().contains("<i>y"));
    }
}
```

- [ ] **Step 2: Implement `src/motif.rs`** (above the tests):

```rust
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
        Self { shape, tone, label: label.into() }
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
        Self { run, technical: technical.into(), lines: lines.iter().map(|l| (*l).to_owned()).collect(), planned: None }
    }
    pub fn planned(mut self, note: impl Into<String>) -> Self {
        self.planned = Some(note.into());
        self
    }
    fn marker_html(&self) -> Html {
        TrailMarker::new(self.run.shape(), self.run.tone(), self.run.name()).html()
    }
    fn planned_html(&self) -> Option<Html> {
        self.planned.as_ref().map(|n| Planned::new().note(n.clone()).html())
    }
}
impl Component for RunCard {}
```

  Templates (LF):
  - `contour.html`: `<svg class="v-contour" viewBox="0 0 800 240" preserveAspectRatio="xMidYMid slice" aria-hidden="true" focusable="false"><g fill="none" stroke="currentColor" stroke-width="1"><path d="M0 200 C120 160 200 210 320 170 S560 120 800 160"/><path d="M0 170 C120 130 200 180 320 140 S560 90 800 130"/><path d="M0 140 C120 100 200 150 320 110 S560 60 800 100"/><path d="M0 110 C120 70 200 120 320 80 S560 30 800 70"/><path d="M0 80 C120 40 200 90 320 50 S560 0 800 40"/></g></svg>`
  - `mountain.html`: `<svg class="v-mountain" viewBox="0 0 800 160" preserveAspectRatio="none" aria-hidden="true" focusable="false"><path class="v-mountain-far" d="M0 160 L0 90 L120 40 L200 80 L300 20 L420 90 L520 50 L640 100 L720 60 L800 90 L800 160 Z"/><path class="v-mountain-near" d="M0 160 L0 120 L90 80 L180 115 L280 70 L380 120 L480 95 L590 125 L690 85 L800 120 L800 160 Z"/></svg>`
  - `route.html`: `<svg class="v-route{% if flowing %} v-route-flowing{% endif %}" viewBox="0 0 400 80" aria-hidden="true" focusable="false"><path class="v-route-line" fill="none" d="M8 64 C90 64 100 16 200 16 S310 64 392 24"/><circle class="v-route-start" cx="8" cy="64" r="5"/><circle class="v-route-end" cx="392" cy="24" r="5"/></svg>`
  - `trail_marker.html`: `<span class="v-trail v-tone-{{ tone.class() }}"><svg class="v-trail-shape" viewBox="0 0 12 12" aria-hidden="true" focusable="false">{% if self.is_circle() %}<circle cx="6" cy="6" r="4.5"/>{% else if self.is_square() %}<rect x="1.5" y="1.5" width="9" height="9"/>{% else %}<polygon points="6,0.8 11.2,6 6,11.2 0.8,6"/>{% endif %}</svg><span class="v-trail-label">{{ label }}</span></span>`
  - `run_card.html`: `<article class="v-card v-run-card"><header class="v-card-head"><h3 class="v-card-title">{{ self.marker_html()|safe }}</h3></header><p class="v-run-tech">{{ technical }}</p>{% if !lines.is_empty() %}<ul class="v-run-lines">{% for l in lines %}<li>{{ l }}</li>{% endfor %}</ul>{% endif %}{% if let Some(p) = self.planned_html() %}{{ p|safe }}{% endif %}</article>`

- [ ] **Step 3: CSS layer** (append):

```css
/* motifs */
.v-contour { display: block; width: 100%; height: 120px; color: var(--v-structure); opacity: .5; }
.v-mountain { display: block; width: 100%; height: 100px; color: var(--v-structure); }
.v-mountain-far { fill: currentColor; opacity: .35; }
.v-mountain-near { fill: currentColor; opacity: .65; }
.v-route { display: block; width: 100%; max-width: 400px; height: auto; color: var(--v-active); }
.v-route-line { stroke: currentColor; stroke-width: 3; stroke-linecap: round; }
.v-route-start, .v-route-end { fill: currentColor; }
.v-route-end { fill: var(--v-edge); }
.v-trail { display: inline-flex; align-items: center; gap: 6px; font: 600 11px/1.4 var(--font); letter-spacing: .06em; color: var(--v-tone-text, var(--text)); white-space: nowrap; }
.v-trail-shape { width: 12px; height: 12px; flex: 0 0 auto; fill: var(--v-tone); }
.v-run-card .v-card-title { display: flex; }
.v-run-tech { margin: 0; font-weight: 600; overflow-wrap: anywhere; }
.v-run-lines { margin: 0; padding-left: 18px; color: var(--text-soft); }
```

- [ ] **Step 4: Gallery + verify** — group `Motifs` (`wide: false`): `contour` (`Contour`), `mountain`, `route`, `route · flowing`, `trail markers` are three items (`Shape::Circle/Square/Diamond`, tones Active/Info/Neutral, labels "trailhead", "checkpoint", "summit" ), `run · Blue` (`RunCard::new(Run::Blue, "Your Gateway", &["Cloud control plane", "Gateway in your network"])`). Test: `for s in ["v-contour", "v-mountain", "v-route", "v-trail", "v-run-card"]`. Run everything, rustfmt, regenerate, commit `feat(visual): contour, mountain, route, trail markers and run cards`.

---

### Task 4: State motion — flow, reveal, expire — and the reduced-motion guard

**Files:** Create `src/motion.rs`, `templates/{reveal,expire}.html`. Modify `src/lib.rs` (`pub mod motion;` + `pub use motion::{Expire, Reveal};`), `static/visual.css`, `tests/css.rs`, `src/site.rs`.

**Interfaces:**
- Produces: `Reveal::new(c: &impl Component) -> Reveal` (steps of a contained `Flow`/`Branch` fade in in order), `Expire::new(c: &impl Component) -> Expire` (an expired thing settles to a faded state). Both `Component`.

- [ ] **Step 1: Failing tests.** In `src/motion.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Node, NodeKind};

    #[test]
    fn wrappers_wrap_the_component_and_mark_the_motion() {
        let n = Node::new(NodeKind::Policy);
        let r = Reveal::new(&n).html();
        assert!(r.as_str().starts_with(r#"<div class="v-reveal">"#) && r.as_str().contains("v-node"));
        let e = Expire::new(&n).html();
        assert!(e.as_str().starts_with(r#"<div class="v-expire">"#) && e.as_str().contains("v-node"));
    }
}
```
  and in `tests/css.rs` (uses the existing `rules()` helper is for custom properties only; add a small text scan):

```rust
#[test]
fn every_animation_is_switched_off_under_reduced_motion() {
    let css = skimasque_visual::CSS;
    let (main, rm): (Vec<&str>, Vec<&str>) = {
        // split the stylesheet into the reduced-motion blocks and the rest
        let mut main = Vec::new();
        let mut rm = Vec::new();
        let mut depth = 0i32;
        let mut in_rm = false;
        for line in css.lines() {
            if !in_rm && line.contains("prefers-reduced-motion") {
                in_rm = true;
                depth = 0;
            }
            if in_rm {
                depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
                rm.push(line);
                if depth <= 0 {
                    in_rm = false;
                }
            } else {
                main.push(line);
            }
        }
        (main, rm)
    };
    let rm_text = rm.join("\n");
    let mut seen = 0;
    for line in main {
        if let Some(open) = line.find('{') {
            let (sel, body) = line.split_at(open);
            if body.contains("animation:") && !body.contains("animation: none") && !sel.trim_start().starts_with("@keyframes") {
                seen += 1;
                let sel = sel.trim();
                assert!(rm_text.contains(sel), "no reduced-motion override for `{sel}`");
            }
        }
    }
    assert!(rm_text.contains("animation: none"));
    assert!(seen >= 1, "the guard found the existing .v-conn-active animation");
}
```
  This scan assumes each animated rule sits on one line (`selector { … animation: … }`), as the existing `.v-conn-active .v-conn-line` rule does; every rule Task 4 adds follows that shape. RED: with only the Reveal/Expire test failing to compile and the guard test passing (existing rule is guarded), record both.

- [ ] **Step 2: Implement** `src/motion.rs`:

```rust
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
```
  Templates: `reveal.html` = `<div class="v-reveal">{{ inner|safe }}</div>`; `expire.html` = `<div class="v-expire">{{ inner|safe }}</div>`.

- [ ] **Step 3: CSS** (append; every animated rule on a single line, each with a matching override in the reduced-motion block):

```css
/* motion: state only */
@keyframes v-route-flow { to { stroke-dashoffset: -28; } }
.v-route-flowing .v-route-line { stroke-dasharray: 4 10; animation: v-route-flow 1.6s linear infinite; }
@keyframes v-reveal-in { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }
.v-reveal .v-flow-step, .v-reveal .v-branch-root, .v-reveal .v-branch-arm { animation: v-reveal-in .5s ease-out both; }
.v-reveal .v-flow-step:nth-child(2), .v-reveal .v-branch-arm:nth-child(1) { animation-delay: .25s; }
.v-reveal .v-flow-step:nth-child(3), .v-reveal .v-branch-arm:nth-child(2) { animation-delay: .5s; }
.v-reveal .v-flow-step:nth-child(4), .v-reveal .v-branch-arm:nth-child(3) { animation-delay: .75s; }
@keyframes v-expire-out { from { opacity: 1; } to { opacity: .55; } }
.v-expire { opacity: .55; animation: v-expire-out 1.2s ease-out 1s both; }
@media (prefers-reduced-motion: reduce) {
  .v-route-flowing .v-route-line { animation: none; }
  .v-reveal .v-flow-step, .v-reveal .v-branch-root, .v-reveal .v-branch-arm { animation: none; }
  .v-reveal .v-flow-step:nth-child(2), .v-reveal .v-branch-arm:nth-child(1) { animation: none; }
  .v-reveal .v-flow-step:nth-child(3), .v-reveal .v-branch-arm:nth-child(2) { animation: none; }
  .v-reveal .v-flow-step:nth-child(4), .v-reveal .v-branch-arm:nth-child(3) { animation: none; }
  .v-expire { animation: none; }
}
```
  The existing `@media (prefers-reduced-motion: reduce) { .v-conn-active .v-conn-line { animation: none; } }` stays (it is a second reduced-motion block; the scan handles several). The delay rules contain `animation-delay:`, not `animation:`, so the guard skips them; keep them anyway for clarity of which selectors are delayed. If the scan flags them, drop the redundant `animation: none` lines for the delay selectors (the first rule's override already disables the animation).

- [ ] **Step 4: Gallery** — group `Motion` (`wide: false`): `expired session` → `Expire::new(&Node::new(NodeKind::Session).status(Status::Expired))`, `reveal` → `Reveal::new(&Flow::new("Request, policy, decision.").then(&Node::new(NodeKind::Workload)).then(&Node::new(NodeKind::Policy)).then(&Node::new(NodeKind::Allow)))`. Run everything, rustfmt, regenerate, commit `feat(visual): state motion with a reduced-motion guard`.

---

### Task 5: Public diagrams, part A — access model, comparison, lifecycle, integrations

**Files:** Create `src/diagrams/mod.rs`, `src/diagrams/public.rs`. Modify `src/lib.rs` (`pub mod diagrams;`), `src/site.rs` (group "Public diagrams", generated from the registry), `static/visual.css` only if a defect appears.

**Interfaces:**
- Consumes: everything from Tasks 1–4.
- Produces in `diagrams/mod.rs`: `pub struct Entry { pub slug: &'static str, pub title: &'static str, pub html: Html }`, `pub fn public_set() -> Vec<Entry>` (later tasks extend it), `pub fn control_set() -> Vec<Entry>` (empty until Task 7). Produces in `diagrams/public.rs` (each returns a concrete component): `identity_policy_access() -> Flow`, `policy_model() -> Flow`, `traditional_vs_skimasque() -> Compare`, `access_lifecycle() -> Flow`, `policy_decision() -> Reveal`, `github_actions() -> Flow`, `developer_cli() -> Flow`, `same_command_different_policy() -> Branch`, `gateway() -> Flow`, `customer_vpc() -> Flow`.

- [ ] **Step 1: Failing tests** in `src/diagrams/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Any dotted quad (a fabricated IP address) in the markup.
    fn has_dotted_quad(s: &str) -> bool {
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_digit() && b[i - 1] != b'.') {
                let mut j = i;
                let mut groups = 0;
                loop {
                    let s0 = j;
                    while j < b.len() && b[j].is_ascii_digit() { j += 1; }
                    if j == s0 { break; }
                    groups += 1;
                    if j < b.len() && b[j] == b'.' && j + 1 < b.len() && b[j + 1].is_ascii_digit() { j += 1 } else { break }
                }
                if groups >= 4 { return true; }
                i = j.max(i + 1);
            } else {
                i += 1;
            }
        }
        false
    }

    #[test]
    fn dotted_quad_detector_works() {
        assert!(has_dotted_quad("egress 203.0.113.7 ok"));
        assert!(!has_dotted_quad("db.prod:5432 and v1.2.3 and 100 Mbps"));
    }

    #[test]
    fn every_diagram_is_named_captioned_honest_and_unique() {
        let all: Vec<Entry> = public_set().into_iter().chain(control_set()).collect();
        assert!(!all.is_empty());
        let mut slugs: Vec<_> = all.iter().map(|e| e.slug).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), all.len(), "slugs are unique");
        for e in &all {
            let s = e.html.as_str();
            assert!(s.contains(r#"class="v-sr""#), "{}: no text equivalent", e.slug);
            assert!(!s.to_lowercase().contains("exec"), "{}: mentions exec", e.slug);
            assert!(!has_dotted_quad(s), "{}: fabricated IP address", e.slug);
            assert!(!e.title.is_empty());
        }
    }

    #[test]
    fn the_public_set_covers_the_canonical_list() {
        let slugs: Vec<_> = public_set().iter().map(|e| e.slug).collect();
        for want in [
            "identity-policy-access", "policy-model", "traditional-vs-skimasque", "access-lifecycle",
            "policy-decision", "github-actions", "developer-cli", "same-command-different-policy",
            "gateway", "customer-vpc",
        ] {
            assert!(slugs.contains(&want), "{want}");
        }
    }
}
```
  Also in `public.rs` tests, one behavioural test per interesting diagram:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    #[test]
    fn the_policy_decision_ends_in_allow_and_a_denial_with_its_rule() {
        let s = policy_decision().html().as_str().to_owned();
        assert!(s.contains("v-reveal") && s.contains("Allow") && s.contains("Deny"));
        assert!(s.contains("No matching allow rule means DENY."));
        assert!(s.contains("v-conn-active") && s.contains("v-conn-denied"));
    }

    #[test]
    fn traditional_access_and_skimasque_are_two_sides() {
        let s = traditional_vs_skimasque().html().as_str().to_owned();
        assert_eq!(s.matches("v-compare-side").count(), 2);
        assert!(s.contains("VPN") && s.contains("Identity") && s.contains("Policy"));
    }

    #[test]
    fn the_lifecycle_runs_request_to_expire_in_order() {
        let s = access_lifecycle().html().as_str().to_owned();
        let pos = |w: &str| s.find(w).unwrap_or_else(|| panic!("{w}"));
        let order = ["REQUEST", "AUTHENTICATE", "AUTHORIZE", "CONNECT", "ACTIVE", "EXPIRE"];
        for w in order.windows(2) {
            assert!(pos(w[0]) < pos(w[1]), "{} before {}", w[0], w[1]);
        }
        assert!(s.contains("EXPIRED"), "the last step carries the expired status word");
    }

    #[test]
    fn the_developer_cli_uses_connect_never_exec() {
        let s = developer_cli().html().as_str().to_owned();
        assert!(s.contains("skimasque connect") && !s.to_lowercase().contains("exec"));
        let b = same_command_different_policy().html().as_str().to_owned();
        assert!(b.contains("staging") && b.contains("production") && b.contains("skimasque connect"));
    }

    #[test]
    fn the_gateway_diagram_describes_egress_in_words_only() {
        let s = gateway().html().as_str().to_owned();
        assert!(s.contains("v-conn-control") && s.contains("v-boundary-region") && s.contains("Database"));
    }
}
```

- [ ] **Step 2: Implement `src/diagrams/mod.rs`** (above tests):

```rust
//! The diagram sets. Each diagram is a function composing library
//! components; the registries list them for the gallery and the site
//! generator. Control-plane diagrams take real data as arguments.

use crate::{Component, Html};

pub mod control;
pub mod platform;
pub mod public;

/// A named diagram, rendered.
pub struct Entry {
    pub slug: &'static str,
    pub title: &'static str,
    pub html: Html,
}

fn entry(slug: &'static str, title: &'static str, c: &impl Component) -> Entry {
    Entry { slug, title, html: c.html() }
}

/// The canonical public diagrams (marketing examples: `acme/widget`,
/// `db.prod:5432`, `20 min`).
pub fn public_set() -> Vec<Entry> {
    vec![
        entry("identity-policy-access", "Identity → policy → access", &public::identity_policy_access()),
        entry("policy-model", "Who, what, where, limits", &public::policy_model()),
        entry("traditional-vs-skimasque", "Traditional vs SkiMasque", &public::traditional_vs_skimasque()),
        entry("access-lifecycle", "Access lifecycle", &public::access_lifecycle()),
        entry("policy-decision", "Policy decision", &public::policy_decision()),
        entry("github-actions", "GitHub Actions", &public::github_actions()),
        entry("developer-cli", "Developer CLI", &public::developer_cli()),
        entry("same-command-different-policy", "Same command, different policy", &public::same_command_different_policy()),
        entry("gateway", "Gateway", &public::gateway()),
        entry("customer-vpc", "Customer VPC", &public::customer_vpc()),
    ]
}

/// Control-plane diagrams rendered with example data (the dashboard calls
/// the `control` functions with real data).
pub fn control_set() -> Vec<Entry> {
    Vec::new()
}
```
  Create `src/diagrams/platform.rs` and `src/diagrams/control.rs` as empty modules containing only a `//!` doc line (Tasks 6 and 7 fill them) so the crate compiles.

- [ ] **Step 3: Implement `src/diagrams/public.rs`:**

```rust
//! Public diagrams, part A: the access model, comparisons, the lifecycle
//! and the integrations. Examples use the marketing data set.

use crate::{
    Boundary, Branch, Compare, ConnKind, Connection, Flow, Node, NodeKind, Reveal, Status, Tone,
};

fn active() -> Connection {
    Connection::new(ConnKind::Active)
}
fn control(label: &str) -> Connection {
    Connection::new(ConnKind::Control).label(label)
}

pub fn identity_policy_access() -> Flow {
    Flow::new(
        "A workload proves its identity, SkiMasque checks the policy, and an active session \
         carries its traffic to the private service.",
    )
    .then(&Node::new(NodeKind::Workload).sub("acme/widget"))
    .via(control("proves identity"), &Node::new(NodeKind::Identity))
    .via(control("checked"), &Node::new(NodeKind::Policy).status(Status::Granted))
    .via(active(), &Node::new(NodeKind::Session).sub("20 min").status(Status::Active))
    .via(active(), &Node::new(NodeKind::Service).sub("db.prod:5432"))
}

pub fn policy_model() -> Flow {
    Flow::new(
        "A policy answers four questions: who is asking, what they are running, where they want \
         to go, and the limits on the access. The answer is network access.",
    )
    .then(&Node::new(NodeKind::Identity).label("WHO").sub("acme/widget · deploy-production"))
    .then(&Node::new(NodeKind::Application).label("WHAT").sub("terraform"))
    .then(&Node::new(NodeKind::Database).label("WHERE").sub("db.prod:5432"))
    .then(&Node::new(NodeKind::Policy).label("LIMITS").sub("20m · 100 Mbps · us-west"))
    .via(active(), &Node::new(NodeKind::Session).label("NETWORK ACCESS").status(Status::Granted))
}

pub fn traditional_vs_skimasque() -> Compare {
    let traditional = Flow::new("Traditional access: a CI job connects through a VPN to the whole network.")
        .then(&Node::new(NodeKind::CiJob))
        .via(Connection::new(ConnKind::Potential), &Node::new(NodeKind::Network).label("VPN"))
        .via(Connection::new(ConnKind::Potential), &Node::new(NodeKind::Network).label("Whole network"))
        .via(Connection::new(ConnKind::Potential), &Node::new(NodeKind::Service).label("Everything on it"));
    let skimasque = Flow::new(
        "SkiMasque access: a CI job proves an identity, a policy allows one destination, and a \
         temporary session carries the traffic.",
    )
    .then(&Node::new(NodeKind::CiJob))
    .via(control("proves identity"), &Node::new(NodeKind::Identity))
    .via(control("checked"), &Node::new(NodeKind::Policy))
    .via(active(), &Node::new(NodeKind::Service).label("One destination").sub("db.prod:5432"))
    .via(active(), &Node::new(NodeKind::Session).sub("expires").status(Status::Active));
    Compare::new("Traditional access reaches a whole network; SkiMasque reaches one destination for a limited time.")
        .side("Traditional access", Tone::Neutral, &traditional)
        .side("SkiMasque", Tone::Active, &skimasque)
}

pub fn access_lifecycle() -> Flow {
    Flow::new("An access request is authenticated, authorized, connected, active for a limited time, and then expires.")
        .then(&Node::new(NodeKind::Workload).label("REQUEST"))
        .via(control("identity"), &Node::new(NodeKind::Identity).label("AUTHENTICATE"))
        .via(control("policy"), &Node::new(NodeKind::Policy).label("AUTHORIZE"))
        .via(active(), &Node::new(NodeKind::Gateway).label("CONNECT"))
        .via(active(), &Node::new(NodeKind::Session).label("ACTIVE").status(Status::Active))
        .via(Connection::new(ConnKind::Potential), &Node::new(NodeKind::Session).label("EXPIRE").status(Status::Expired))
}

pub fn policy_decision() -> Reveal {
    let request = Flow::new("A request is evaluated by the policy.")
        .then(&Node::new(NodeKind::Workload).label("Request"))
        .via(control("evaluated"), &Node::new(NodeKind::Policy));
    Reveal::new(
        &Branch::new("If a policy matches the request, access is allowed. With no matching allow rule the request is denied.", &request)
            .arm(active().label("match"), &Node::new(NodeKind::Allow))
            .arm(
                Connection::new(ConnKind::Denied).label("no match"),
                &Node::new(NodeKind::Deny).sub("No matching allow rule means DENY."),
            ),
    )
}

pub fn github_actions() -> Flow {
    Flow::new(
        "A GitHub Actions job proves its identity with OIDC, SkiMasque evaluates the policy, and \
         an active session carries its traffic through the gateway to the private database.",
    )
    .then(&Node::new(NodeKind::GitHub).sub("acme/widget"))
    .via(control("OIDC"), &Node::new(NodeKind::Identity))
    .via(control("policy"), &Node::new(NodeKind::Policy).status(Status::Granted))
    .then(&Node::new(NodeKind::Session).sub("20 min"))
    .via(active(), &Node::new(NodeKind::Gateway))
    .via(active(), &Node::new(NodeKind::Database).sub("db.prod:5432"))
}

pub fn developer_cli() -> Flow {
    Flow::new(
        "A developer runs skimasque connect, the CLI proves who they are, the policy is checked, \
         and a temporary session reaches the private service.",
    )
    .then(&Node::new(NodeKind::Developer))
    .via(Connection::new(ConnKind::Normal), &Node::new(NodeKind::Cli).sub("skimasque connect db.prod:5432"))
    .via(control("proves identity"), &Node::new(NodeKind::Identity))
    .via(control("checked"), &Node::new(NodeKind::Policy))
    .via(active(), &Node::new(NodeKind::Session).sub("temporary").status(Status::Active))
    .via(active(), &Node::new(NodeKind::Service).sub("db.prod:5432"))
}

pub fn same_command_different_policy() -> Branch {
    let staging = Flow::new("The staging policy reaches the staging database.")
        .then(&Node::new(NodeKind::Policy).label("staging policy"))
        .via(active(), &Node::new(NodeKind::Database).sub("db.staging:5432"));
    let production = Flow::new("The production policy reaches the production database with tighter limits.")
        .then(&Node::new(NodeKind::Policy).label("production policy").sub("10 min"))
        .via(active(), &Node::new(NodeKind::Database).sub("db.prod:5432"));
    Branch::new(
        "The same command reaches different destinations depending on which policy applies.",
        &Node::new(NodeKind::Cli).sub("skimasque connect"),
    )
    .arm(control("staging identity"), &staging)
    .arm(control("production identity"), &production)
}

pub fn gateway() -> Flow {
    Flow::new(
        "The control plane tells the gateway which sessions are allowed; the gateway forwards \
         traffic only to the private services in your network.",
    )
    .then(&Node::new(NodeKind::ControlPlane))
    .via(control("sessions and policy"), &Node::new(NodeKind::Gateway))
    .via(
        active(),
        &Boundary::region("YOUR NETWORK")
            .child(&Node::new(NodeKind::Database))
            .child(&Node::new(NodeKind::Api))
            .child(&Node::new(NodeKind::Kubernetes)),
    )
}

pub fn customer_vpc() -> Flow {
    Flow::new("A session enters your VPC through the gateway, which forwards it to the database inside.")
        .then(&Node::new(NodeKind::Developer))
        .via(
            active().label("session"),
            &Boundary::region("YOUR VPC")
                .child(&Node::new(NodeKind::Gateway))
                .child(&Node::new(NodeKind::Database).sub("db.prod:5432")),
        )
}
```
  Note: `Flow::via` on a `Flow` step whose next step is added with `.then` (as in `github_actions`) sets a `Normal` connection; that matches the gallery's existing "access" flow. If clippy objects to the long lines, rustfmt handles it.

- [ ] **Step 4: Gallery** — in `site.rs`, add a group "Public diagrams" with `wide: true` whose items come from `crate::diagrams::public_set()` (caption = `e.title`, html = `e.html.clone()`; write a tiny helper `fn entry_items(v: Vec<diagrams::Entry>) -> Vec<Item>`). Extend the gallery test with `for s in ["v-reveal", "v-compare-side", "skimasque connect", "YOUR VPC"]`. Run everything, rustfmt, regenerate, commit `feat(visual): public diagrams for access, comparison, lifecycle and integrations`.

---

### Task 6: Public diagrams, part B — security, architecture, MASQUE, deployment, audit

**Files:** Modify `src/diagrams/platform.rs` (fill), `src/diagrams/mod.rs` (register), `static/visual.css` only if needed.

**Interfaces:**
- Consumes: Task 5's helpers pattern; `Layers`, `Compare`, `Branch`, `Sequence`, `RunCard`, `Run`, `Boundary`.
- Produces (concrete component each): `ci_lifecycle() -> Flow`, `security_layers() -> Layers`, `no_standing_access() -> Compare`, `compartmentalisation() -> Compare`, `control_data_plane() -> Flow`, `architecture() -> Flow`, `masque_stack() -> Layers`, `connect_udp_sequence() -> Sequence`, `multiple_gateways() -> Branch`, `audit_flow() -> Flow`, `deployment_models() -> Compare`. Registry slugs: `ci-lifecycle`, `security-layers`, `no-standing-access`, `compartmentalisation`, `control-data-plane`, `architecture`, `masque-stack`, `connect-udp-sequence`, `multiple-gateways`, `audit-flow`, `deployment-models`.

- [ ] **Step 1: Failing tests** (in `platform.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    fn html(c: &impl Component) -> String {
        c.html().as_str().to_owned()
    }

    #[test]
    fn the_ci_lifecycle_runs_job_start_to_session_expired() {
        let s = html(&ci_lifecycle());
        let order = ["JOB START", "IDENTITY", "SESSION CREATED", "DEPLOYMENT", "JOB COMPLETE", "SESSION EXPIRED"];
        for w in order.windows(2) {
            assert!(s.find(w[0]).unwrap() < s.find(w[1]).unwrap(), "{} before {}", w[0], w[1]);
        }
    }

    #[test]
    fn security_layers_rest_on_each_other_identity_to_network() {
        let s = html(&security_layers());
        assert!(s.contains("v-layers-up"));
        for l in ["IDENTITY", "POLICY", "SESSION", "GATEWAY", "NETWORK"] {
            assert!(s.contains(l), "{l}");
        }
    }

    #[test]
    fn no_standing_access_contrasts_a_credential_with_an_expiring_session() {
        let s = html(&no_standing_access());
        assert!(s.contains("Credential") && s.contains("EXPIRED") && s.contains("standing access"));
    }

    #[test]
    fn compartmentalisation_allows_one_destination_per_job_and_denies_the_rest() {
        let s = html(&compartmentalisation());
        assert_eq!(s.matches("v-compare-side").count(), 2);
        assert!(s.matches("v-conn-denied").count() >= 2 && s.contains("DENY") || s.contains("Deny"));
    }

    #[test]
    fn the_masque_stack_and_sequence_use_connect_udp_only() {
        let s = html(&masque_stack());
        let order = ["Application", "MASQUE", "HTTP/3", "QUIC", "UDP/IP"];
        for w in order.windows(2) {
            assert!(s.find(w[0]).unwrap() < s.find(w[1]).unwrap());
        }
        let q = html(&connect_udp_sequence());
        assert!(q.contains("CONNECT-UDP") && !q.contains("CONNECT-IP"));
    }

    #[test]
    fn multiple_gateways_hang_off_one_control_plane() {
        let s = html(&multiple_gateways());
        for r in ["us-west", "us-east", "eu-west"] {
            assert!(s.contains(r), "{r}");
        }
        assert_eq!(s.matches("v-branch-arm\"").count(), 3);
    }

    #[test]
    fn deployment_models_always_show_the_technical_name_and_mark_unbuilt_paid_plans_planned() {
        let s = html(&deployment_models());
        for (run, tech) in [("Green Run", "SkiMasque Cloud"), ("Blue Run", "Your Gateway"), ("Black Run", "Self-hosted")] {
            assert!(s.contains(run) && s.contains(tech), "{run}/{tech}");
        }
        assert!(s.contains("Free tier"), "the hosted run is live with a free tier");
        assert_eq!(s.matches("PLANNED").count(), 1, "only paid plans are planned (billing is not in place)");
        assert!(s.contains("paid plans"));
    }

    #[test]
    fn architecture_control_data_plane_and_audit_are_captioned_flows() {
        for s in [html(&architecture()), html(&control_data_plane()), html(&audit_flow())] {
            assert!(s.contains("v-flow"));
        }
        assert!(html(&control_data_plane()).contains("CONTROL PLANE") && html(&control_data_plane()).contains("DATA PLANE"));
        assert!(html(&audit_flow()).contains("Audit log"));
    }
}
```
  (Fix the operator precedence typo in `compartmentalisation`'s test to `assert!(s.matches("v-conn-denied").count() >= 2 && (s.contains("DENY") || s.contains("Deny")));` when writing it.)

- [ ] **Step 2: Implement `src/diagrams/platform.rs`:**

```rust
//! Public diagrams, part B: security, architecture, MASQUE, audit and the
//! deployment runs. Examples use the marketing data set; nothing here is an
//! address or a promise the product does not keep.

use crate::{
    Boundary, Branch, Compare, ConnKind, Connection, Flow, Layers, Node, NodeKind, Run, RunCard,
    Sequence, Status, Tone,
};

fn active() -> Connection {
    Connection::new(ConnKind::Active)
}
fn control(label: &str) -> Connection {
    Connection::new(ConnKind::Control).label(label)
}

pub fn ci_lifecycle() -> Flow {
    Flow::new("A CI job starts, proves its identity, gets a session, deploys, completes, and its session expires.")
        .then(&Node::new(NodeKind::CiJob).label("JOB START"))
        .via(control("OIDC"), &Node::new(NodeKind::Identity).label("IDENTITY"))
        .via(control("policy"), &Node::new(NodeKind::Session).label("SESSION CREATED").status(Status::Active))
        .via(active(), &Node::new(NodeKind::Service).label("DEPLOYMENT"))
        .via(Connection::new(ConnKind::Normal), &Node::new(NodeKind::CiJob).label("JOB COMPLETE"))
        .via(Connection::new(ConnKind::Potential), &Node::new(NodeKind::Session).label("SESSION EXPIRED").status(Status::Expired))
}

pub fn security_layers() -> Layers {
    Layers::new("Security is layered: identity, policy, session, gateway and network each rest on the layer below.")
        .upward()
        .row_sub("IDENTITY", "who is asking")
        .row_sub("POLICY", "what they may reach")
        .row_sub("SESSION", "for how long")
        .row_sub("GATEWAY", "the only path in")
        .row_sub("NETWORK", "private, not exposed")
}

pub fn no_standing_access() -> Compare {
    let standing = Flow::new("A stored credential gives standing access to the network at all times.")
        .then(&Node::new(NodeKind::Credential))
        .via(Connection::new(ConnKind::Normal).label("standing access"), &Node::new(NodeKind::Network));
    let temporary = Flow::new("An identity is checked against policy, receives a session, and the session expires.")
        .then(&Node::new(NodeKind::Identity))
        .via(control("policy"), &Node::new(NodeKind::Policy))
        .via(active(), &Node::new(NodeKind::Session).status(Status::Active))
        .via(Connection::new(ConnKind::Potential).label("expires"), &Node::new(NodeKind::Session).label("Session").status(Status::Expired));
    Compare::new("A stored credential is always able to reach the network; a SkiMasque session exists only while it is needed.")
        .side("Standing access", Tone::Neutral, &standing)
        .side("No standing access", Tone::Active, &temporary)
}

pub fn compartmentalisation() -> Compare {
    let job = |name: &str, allowed: NodeKind, sub: &str| {
        Branch::new(
            format!("{name} may reach one destination; everything else is denied."),
            &Node::new(NodeKind::CiJob).label(name),
        )
        .arm(active().label("allowed"), &Node::new(allowed).sub(sub))
        .arm(
            Connection::new(ConnKind::Denied).label("everything else"),
            &Node::new(NodeKind::Deny).sub("DENY"),
        )
    };
    Compare::new("Each job is allowed exactly one destination, so a compromised job cannot reach the others.")
        .side("Job A", Tone::Active, &job("Job A", NodeKind::Database, "db.prod:5432"))
        .side("Job B", Tone::Active, &job("Job B", NodeKind::Api, "api.internal:443"))
}

pub fn control_data_plane() -> Flow {
    Flow::new("The control plane decides who may connect; the data plane carries the traffic through the gateway.")
        .then(
            &Boundary::region("CONTROL PLANE")
                .child(&Node::new(NodeKind::Identity))
                .child(&Node::new(NodeKind::Policy)),
        )
        .via(
            control("sessions"),
            &Boundary::region("DATA PLANE").child(&Node::new(NodeKind::Gateway)),
        )
        .via(active(), &Node::new(NodeKind::Network))
}

pub fn architecture() -> Flow {
    Flow::new(
        "Developers and CI/CD jobs authenticate with the control plane, which authorizes a \
         session; the MASQUE gateway then forwards traffic into the private network.",
    )
    .then(&Node::new(NodeKind::Developer).label("Developer or CI/CD"))
    .via(control("authenticate"), &Node::new(NodeKind::ControlPlane).sub("identity · policy · sessions"))
    .via(control("session"), &Node::new(NodeKind::Session).label("Session authorization"))
    .via(active(), &Node::new(NodeKind::Gateway).label("MASQUE gateway"))
    .via(active(), &Node::new(NodeKind::Network))
}

pub fn masque_stack() -> Layers {
    Layers::new("Traffic rides MASQUE over HTTP/3, which runs on QUIC over UDP/IP.")
        .row("Application")
        .row_sub("MASQUE", "CONNECT-UDP")
        .row("HTTP/3")
        .row("QUIC")
        .row("UDP/IP")
}

pub fn connect_udp_sequence() -> Sequence {
    Sequence::new("The client asks the gateway for a CONNECT-UDP tunnel, the gateway confirms it, and datagrams flow both ways.", "Client", "Gateway")
        .to_right("CONNECT-UDP request")
        .to_left("tunnel established")
        .to_right("UDP datagrams")
        .to_left("UDP datagrams")
}

pub fn multiple_gateways() -> Branch {
    let vpc = |region: &str| {
        Boundary::region(format!("VPC · {region}")).child(&Node::new(NodeKind::Gateway).sub(region.to_owned()))
    };
    Branch::new(
        "One control plane manages several gateways, each inside its own network.",
        &Node::new(NodeKind::ControlPlane),
    )
    .arm(control("us-west"), &vpc("us-west"))
    .arm(control("us-east"), &vpc("us-east"))
    .arm(control("eu-west"), &vpc("eu-west"))
}

pub fn audit_flow() -> Flow {
    Flow::new("Every decision, granted or denied, is written to the audit log, where it can be reviewed.")
        .then(&Node::new(NodeKind::Workload).label("Request"))
        .via(control("evaluated"), &Node::new(NodeKind::Policy))
        .via(Connection::new(ConnKind::Normal).label("decision"), &Node::new(NodeKind::Audit))
        .via(Connection::new(ConnKind::Normal).label("reviewed"), &Node::new(NodeKind::Developer).label("Audit page"))
}

pub fn deployment_models() -> Compare {
    let green = RunCard::new(Run::Green, "SkiMasque Cloud", &["Hosted control plane and gateway", "Free tier"]).planned("paid plans");
    let blue = RunCard::new(Run::Blue, "Your Gateway", &["SkiMasque control plane", "Gateway runs in your network"]);
    let black = RunCard::new(Run::Black, "Self-hosted", &["Control plane and gateway run in your infrastructure"]);
    Compare::new("Three ways to run SkiMasque: hosted, with your own gateway, or fully self-hosted. Each is named by run and by what it is.")
        .side("", Tone::Active, &green)
        .side("", Tone::Info, &blue)
        .side("", Tone::Neutral, &black)
}
```
  Register all eleven in `public_set()` (`entry(slug, title, &platform::…())`), titles: "CI/CD lifecycle", "Security layers", "No standing access", "Compartmentalisation", "Control plane and data plane", "Architecture", "MASQUE stack", "CONNECT-UDP sequence", "Multiple gateways", "Audit flow", "Deployment models". Extend `the_public_set_covers_the_canonical_list` with the eleven slugs.

- [ ] **Step 3: Run** the full commands (the `compartmentalisation` closure formats `Branch::new` caption with `format!`, which takes `impl Into<String>`; the sr caption strings are user-visible-to-AT only). The `Compare` with empty side titles renders no `<h4>` (Task 2); confirm the three run cards sit in a row at ≥720px in the browser check (Task 8). rustfmt `src/diagrams/*.rs`, regenerate, commit `feat(visual): security, architecture, MASQUE, audit and deployment diagrams`.

---

### Task 7: Control-plane diagrams (data-driven)

**Files:** Modify `src/diagrams/control.rs`, `src/diagrams/mod.rs` (`control_set()`), `src/site.rs`.

**Interfaces:**
- Produces: `GatewayRef<'a> { name: &'a str, region: &'a str, status: Status }`; `identity_flow(source: &str, identity: &str, policy: Option<&str>) -> Flow`; `session_flow(identity: &str, target: &str, gateway: &str, status: Status) -> Flow`; `gateway_topology(gateways: &[GatewayRef]) -> Branch`; `org_topology(org: &str, identities: u32, policies: u32, gateways: u32) -> Flow`. Registry slugs `identity-flow`, `session-flow`, `gateway-topology`, `org-topology`.

- [ ] **Step 1: Failing tests** (`control.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Status};

    fn html(c: &impl Component) -> String {
        c.html().as_str().to_owned()
    }

    #[test]
    fn an_identity_with_a_matching_policy_is_granted_and_without_one_is_denied_in_words() {
        let ok = html(&identity_flow("GitHub Actions", "acme/widget", Some("production-deploy")));
        assert!(ok.contains("acme/widget") && ok.contains("production-deploy") && ok.contains("ACCESS GRANTED"));
        let none = html(&identity_flow("GitHub Actions", "acme/rogue", None));
        assert!(none.contains("acme/rogue") && none.contains("v-conn-denied") && none.contains("No matching policy"));
        assert!(!none.contains("ACCESS GRANTED"));
    }

    #[test]
    fn a_session_flow_shows_the_sessions_real_status_word() {
        for (st, word) in [(Status::Active, "ACTIVE"), (Status::Expired, "EXPIRED")] {
            let s = html(&session_flow("acme/widget", "db.prod:5432", "gw-us-west", st));
            assert!(s.contains(word) && s.contains("gw-us-west") && s.contains("db.prod:5432"));
        }
    }

    #[test]
    fn the_gateway_topology_lists_each_gateway_with_its_status_and_handles_none() {
        let g = [
            GatewayRef { name: "gw-us-west", region: "us-west-2", status: Status::Healthy },
            GatewayRef { name: "gw-eu", region: "eu-west-1", status: Status::Offline },
        ];
        let s = html(&gateway_topology(&g));
        assert_eq!(s.matches("v-branch-arm\"").count(), 2);
        assert!(s.contains("HEALTHY") && s.contains("OFFLINE") && s.contains("gw-eu"));
        let none = html(&gateway_topology(&[]));
        assert!(none.contains("No gateways connected") && none.contains("v-branch-root"));
    }

    #[test]
    fn the_org_topology_states_only_the_counts_it_is_given() {
        let s = html(&org_topology("Acme", 4, 3, 1));
        assert!(s.contains("Acme") && s.contains("4 identities") && s.contains("3 policies") && s.contains("1 gateway"));
        let one = html(&org_topology("Solo", 1, 1, 2));
        assert!(one.contains("1 identity") && one.contains("1 policy") && one.contains("2 gateways"));
    }

    #[test]
    fn control_diagram_strings_are_escaped() {
        let s = html(&identity_flow("<b>", "<script>", Some("\"><img>")));
        assert!(!s.contains("<script>") && !s.contains("<img>") && !s.contains("<b>"));
    }
}
```

- [ ] **Step 2: Implement `control.rs`:**

```rust
//! Control-plane diagrams: the same visual language as the public set, filled
//! with the organisation's real data. Nothing is invented — a diagram states
//! only the identities, policies, sessions and gateways it is handed.
//! Interactive inspection of these diagrams is Planned.

use crate::{Branch, ConnKind, Connection, Flow, Node, NodeKind, Status};

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub struct GatewayRef<'a> {
    pub name: &'a str,
    pub region: &'a str,
    pub status: Status,
}

/// An identity and the policy (if any) that lets it in.
pub fn identity_flow(source: &str, identity: &str, policy: Option<&str>) -> Flow {
    let who = Node::new(NodeKind::Identity).label(identity.to_owned()).sub(source.to_owned());
    match policy {
        Some(p) => Flow::new(format!("{identity} from {source} matches the policy {p}, so access is granted."))
            .then(&who)
            .via(
                Connection::new(ConnKind::Control).label("matches"),
                &Node::new(NodeKind::Policy).label(p.to_owned()).status(Status::Granted),
            ),
        None => Flow::new(format!("{identity} from {source} matches no policy, so access is denied."))
            .then(&who)
            .via(
                Connection::new(ConnKind::Denied),
                &Node::new(NodeKind::Deny).label("No matching policy").status(Status::Denied),
            ),
    }
}

/// One session: who, through which gateway, to what, and its real status.
pub fn session_flow(identity: &str, target: &str, gateway: &str, status: Status) -> Flow {
    let conn = if status == Status::Active { ConnKind::Active } else { ConnKind::Potential };
    Flow::new(format!("{identity} has a session {} through {gateway} to {target}.", status.word().to_lowercase()))
        .then(&Node::new(NodeKind::Identity).label(identity.to_owned()))
        .via(Connection::new(conn), &Node::new(NodeKind::Session).status(status))
        .via(Connection::new(conn), &Node::new(NodeKind::Gateway).label(gateway.to_owned()))
        .via(Connection::new(conn), &Node::new(NodeKind::Service).label(target.to_owned()))
}

/// The control plane and the gateways connected to it, each with its health.
pub fn gateway_topology(gateways: &[GatewayRef]) -> Branch {
    let caption = if gateways.is_empty() {
        "No gateways connected.".to_owned()
    } else {
        format!("The control plane manages {}.", plural(gateways.len() as u32, "gateway", "gateways"))
    };
    let root = Node::new(NodeKind::ControlPlane).sub(if gateways.is_empty() { "No gateways connected" } else { "gateways connected" });
    gateways.iter().fold(Branch::new(caption, &root), |b, g| {
        b.arm(
            Connection::new(ConnKind::Control),
            &Node::new(NodeKind::Gateway).label(g.name.to_owned()).sub(g.region.to_owned()).status(g.status),
        )
    })
}

/// An organisation at a glance, from counts the caller already holds.
pub fn org_topology(org: &str, identities: u32, policies: u32, gateways: u32) -> Flow {
    Flow::new(format!(
        "{org} has {}, {} and {}.",
        plural(identities, "identity", "identities"),
        plural(policies, "policy", "policies"),
        plural(gateways, "gateway", "gateways"),
    ))
    .then(&Node::new(NodeKind::Identity).label(plural(identities, "identity", "identities")).sub(org.to_owned()))
    .via(
        Connection::new(ConnKind::Control),
        &Node::new(NodeKind::Policy).label(plural(policies, "policy", "policies")),
    )
    .via(
        Connection::new(ConnKind::Control),
        &Node::new(NodeKind::Gateway).label(plural(gateways, "gateway", "gateways")),
    )
}
```
  In `diagrams/mod.rs` change `control_set()` to build from example data (comment "example data"): `identity-flow` ← `control::identity_flow("GitHub Actions", "acme/widget", Some("production-deploy"))`, `session-flow` ← `session_flow("acme/widget", "db.prod:5432", "gw-us-west", Status::Active)`, `gateway-topology` ← two example gateways (Healthy `gw-us-west`/`us-west-2`, Degraded `gw-eu-west`/`eu-west-1`), `org-topology` ← `org_topology("Acme", 4, 3, 2)`. Add `use crate::Status;`.

- [ ] **Step 3: Gallery** — group "Control-plane diagrams (example data)" (`wide: true`) from `control_set()` plus a final item `item("interactive inspection", &Planned::new().note("interactive inspection of these diagrams"))`. Extend the gallery test with `"gw-us-west"`, `"4 identities"`, and `"interactive inspection"`. Run everything, rustfmt, regenerate, commit `feat(visual): data-driven control-plane diagrams`.

---

### Task 8: Gallery completion, honesty pass, browser check

**Files:** Modify `src/site.rs` (drop the temporary "Layout" group if the registry diagrams now cover each container; keep one labelled sample of each container if a container has no diagram), `docs/superpowers/specs/2026-09-30-product-surface-amendment.md` (status line: plan 3 implemented).

- [ ] **Step 1: Whole suite** — the three verify commands and `cargo test --workspace` are clean; the gallery test asserts the presence of every group title (`Public diagrams`, `Control-plane diagrams`, `Motifs`, `Motion`) and `!page.contains("exec")`, `!page.to_lowercase().contains("exec")` is already covered per-diagram by the registry test.
- [ ] **Step 2: Browser check** — serve `site/components/` (`python -m http.server` from that directory in the background; stop it after) and, with the Browser tools at widths 320, 720, 1100 and 1400, in both theme sections confirm: `document.documentElement.scrollWidth <= innerWidth`; no `.v-flow-wrap`, `.v-compare-wrap`, `.v-branch-wrap`, `.v-seq-wrap` or `.v-layers-wrap` has `scrollWidth > clientWidth`; the deployment run cards sit in a row at ≥720px; the Compare flows stack vertically; `document.body.innerText.toLowerCase().includes('exec')` is false. Emulate `prefers-reduced-motion: reduce` (resize_window has no such option; instead run `getComputedStyle(document.querySelector('.v-route-flowing .v-route-line')).animationName` with the emulation you have, or inject `@media` — if the emulation is unavailable, record that the CSS test covers it). Reset the viewport with preset "desktop". Fix any overflow with a CSS-only change and re-check.
- [ ] **Step 3: Spec status** — set `Status: approved 2026-09-29; plans 2 (palette, product components) and 3 (diagrams, motifs) implemented`. Commit `docs(visual): amendment status` and stop; the controller runs the final review and the PR.

---

## Self-Review

- **Spec coverage:** public set §61 → identity→policy→access, WHO→LIMITS (`policy_model`), traditional vs SkiMasque, access lifecycle, policy decision, GitHub Actions, developer CLI (+ same command/different policy), gateway, customer VPC (Tasks 5); deployment models (run markers, technical name always shown), security layers, control/data plane, MASQUE stack (+ CONNECT-UDP sequence), multiple gateways, audit flow, plus no-standing-access, compartmentalisation, CI lifecycle, architecture (Task 6). Control-plane set §62: identity flow, session flow, gateway topology, organisation topology, data-driven, interactive inspection Planned (Task 7). Alpine motifs: contour, mountain, route lines, trail/elevation markers (Task 3). Motion: active-session flow, authorization reveal, expiry fade, all reduced-motion guarded (Task 4). Site chrome and the ~20 public pages are plan 4.
- **Placeholders:** none; the two "if askama rejects…" notes give a concrete fallback rule, not a gap.
- **Type consistency:** `Layers/Compare/Branch/Sequence` (Task 2) are used verbatim in Tasks 5–7; `Reveal` (Task 4) in `policy_decision` (Task 5); `RunCard/Run` (Task 3) in `deployment_models` (Task 6); `NodeKind::{ControlPlane, Cli, Credential, Audit}` (Task 1) in Tasks 5–7; `Status::{Granted, Denied, Healthy, Offline, Degraded}` come from plan 2; `Planned` from plan 2 in Tasks 3 and 7.
- **Owner-review items:** Green Run is live with a free tier, paid plans Planned (owner decision); confirm the Blue and Black run descriptions before plan 4 publishes them. `Compare` with an empty side title is used for the run cards.
