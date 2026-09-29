# Visual Library — Plan 1: Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the `skimasque-visual` crate with the Alpine tokens, an icon sprite, the primitive components (Status, DecisionBadge, Node, Connection, Flow, Boundary), a component gallery page, and the `sitegen` static generator with a CI staleness check.

**Architecture:** Components are Rust structs that derive `askama::Template` (askama 0.12). Their templates live inside the crate. They render to an `Html` newtype that only library components can produce, and callers embed it with `{{ x|safe }}`. CSS is one file, `static/visual.css`, exposed as `CSS`. `sitegen` renders the gallery (`site/components/index.html` plus `site/components/visual.css`) from crate code. `--check` compares against committed files.

**Tech Stack:** Rust 1.88 (workspace MSRV), askama 0.12.1, plain CSS, inline SVG.

**Spec:** `docs/superpowers/specs/2026-09-29-visual-library-design.md`

**Working directory:** `C:\Users\Thor\Documents\src\skimasque-dev\wt-visual` (repo `skimasque`, branch `feat/visual-library`). All paths are relative to it.

## Global Constraints

- Palette values exactly: alpine-950 `#0b1117`, -900 `#111a22`, -850 `#16212a`, -800 `#1d2a34`, -700 `#30414d`, -600 `#435763`, -500 `#61737e`, -400 `#81919a`; snow-50 `#f7faf9`, -100 `#edf3f1`, -200 `#dce7e3`; mint-500 `#63d7b1`, -400 `#7de4c2`, -300 `#a1efd5`, -200 `#c9f7e7`; forest-950 `#0b1512`, -900 `#102019`, -800 `#173026`, -700 `#234638`, -600 `#35614d`; earth-950 `#17120e`, -900 `#211914`, -800 `#30221a`, -700 `#4a3527`, -600 `#66503d`; success `#63d7b1`, warning `#e7c66a`, danger `#e87979`, info `#79bde8`; light-mode accent text `#0b7a5b`.
- Hex colours appear only between `/* tokens */` and `/* end tokens */` in `static/visual.css`. No template contains a hex colour.
- Grammar tokens: `--v-active` mint, `--v-structure` forest, `--v-edge` earth, `--v-neutral` slate, `--v-deny` danger.
- Typeface: IBM Plex Mono, fallback `ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`.
- Every status shows its word; a dot is never alone. Denied and active connections carry a visually hidden word.
- Every SVG has `aria-hidden="true" focusable="false"` (decorative icons) or `<title>`+`<desc>` (diagrams).
- Motion: only the Active connection's glow; `@media (prefers-reduced-motion: reduce)` disables it.
- Flows are horizontal at ≥720px and vertical below, via CSS. Never a scaled drawing.
- Honesty rule: gallery examples use real commands and features only (`skimasque connect`, the connect Action, CONNECT-UDP). No `skimasque exec`, no pricing, no fabricated IP addresses.
- askama must be `0.12` (same as `skimasque-dev/control`).
- The crate is `publish = false` for now.
- **Formatting:** do not run `cargo fmt` over the workspace (the local rustfmt nightly rewrites unrelated files). Run `rustfmt --edition 2021 <new files>` only.
- Commits: conventional prefix, **no AI attribution trailers or footers**. Run `cargo clippy -p skimasque-visual --all-targets -- -D warnings` before each commit.

## Review Focus

1. **Hostile labels** (`<script>`, quotes, `&`) in any component must come out escaped, never as markup. Every component task's test covers this.
2. **Windows checkouts:** git may convert the committed `site/` files to CRLF, and then `sitegen --check` would falsely report staleness. `--check` compares with `\r\n` normalized to `\n`. Task 8 tests this.
3. **A flow with one step, or a last step with a connection:** no dangling connector after the final node. Task 6 tests this.
4. **Light theme inside a dark page** (the gallery's paired sections): components must read their colours from role and grammar tokens scoped by `data-theme`, never from `:root` only. Task 1's CSS and Task 9's visual pass cover this.
5. **Narrow screens:** a 6-step flow at 375px must stack vertically with no horizontal scroll. Task 9 checks this.

---

## File structure

```
Cargo.toml                                         MOD  (nothing: members = crates/* picks it up; add askama to [workspace.dependencies])
.github/workflows/ci.yml                           MOD  sitegen --check step
.gitattributes                                     MOD  site/** text eol=lf
crates/skimasque-visual/Cargo.toml                 NEW
crates/skimasque-visual/askama.toml                NEW  dirs = ["templates"]
crates/skimasque-visual/src/lib.rs                 NEW  Html, Component, CSS, re-exports
crates/skimasque-visual/src/icons.rs               NEW  sprite()
crates/skimasque-visual/src/status.rs              NEW  Status, StatusBadge, DecisionBadge
crates/skimasque-visual/src/node.rs                NEW  NodeKind, Tone, Node
crates/skimasque-visual/src/connection.rs          NEW  ConnKind, Connection
crates/skimasque-visual/src/flow.rs                NEW  Step, Flow
crates/skimasque-visual/src/boundary.rs            NEW  BoundaryKind, Boundary
crates/skimasque-visual/src/site.rs                NEW  pages(), gallery
crates/skimasque-visual/src/bin/sitegen.rs         NEW
crates/skimasque-visual/static/visual.css          NEW
crates/skimasque-visual/templates/*.html           NEW  one per component + gallery.html
crates/skimasque-visual/tests/css.rs               NEW  hex-placement tests
site/components/index.html, site/components/visual.css   GENERATED, committed
```

---

### Task 1: Crate scaffold, `Html`/`Component`, tokens CSS

**Files:**
- Create: `crates/skimasque-visual/Cargo.toml`, `askama.toml`, `src/lib.rs`, `static/visual.css`, `tests/css.rs`
- Modify: root `Cargo.toml` (`[workspace.dependencies]`: `askama = "0.12"`)

**Interfaces:**
- Produces:

```rust
pub const CSS: &str;                        // include_str!("../static/visual.css")
pub struct Html(String);                    // impl Display, as_str(); constructed only inside the crate
pub trait Component: askama::Template {     // blanket helper
    fn html(&self) -> Html;
}
```

- CSS custom properties: palette (`--alpine-950`…), grammar (`--v-active`, `--v-structure`, `--v-edge`, `--v-neutral`, `--v-deny`, `--v-info`), roles (`--bg --surface --surface-raised --text --text-muted --line --line-strong --accent --accent-text --on-accent --focus --font`), scoped for `:root`, `[data-theme="dark"]`, `[data-theme="light"]` and `prefers-color-scheme: light`.
- Utility classes: `.v-sr` (visually hidden), `.v-tone-active|structure|edge|neutral|deny|info`.

- [ ] **Step 1: Write the failing CSS tests**

`crates/skimasque-visual/tests/css.rs`:

```rust
//! The stylesheet keeps every hex colour inside its token block, and no
//! component template hard-codes a colour.

fn hex_colours(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in line.match_indices('#') {
        let hex: String = line[i + 1..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        let next = line[i + 1 + hex.len()..].chars().next().unwrap_or(' ');
        if matches!(hex.len(), 3 | 6 | 8) && !next.is_ascii_alphanumeric() && next != '-' {
            out.push(hex);
        }
    }
    out
}

#[test]
fn hex_colours_live_only_in_the_token_block() {
    let css = skimasque_visual::CSS;
    let mut in_tokens = false;
    let mut seen_tokens = false;
    for (n, line) in css.lines().enumerate() {
        if line.contains("/* tokens */") { in_tokens = true; seen_tokens = true; continue; }
        if line.contains("/* end tokens */") { in_tokens = false; continue; }
        if !in_tokens {
            assert!(hex_colours(line).is_empty(), "visual.css:{}: hex outside tokens: {line}", n + 1);
        }
    }
    assert!(seen_tokens, "token block markers present");
}

#[test]
fn the_palette_matches_the_design_guide() {
    for decl in [
        "--alpine-950: #0b1117", "--mint-500: #63d7b1", "--forest-800: #173026",
        "--earth-700: #4a3527", "--snow-50: #f7faf9", "--danger: #e87979",
    ] {
        assert!(skimasque_visual::CSS.contains(decl), "missing {decl}");
    }
}

#[test]
fn templates_hard_code_no_colours() {
    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/templates"));
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            assert!(hex_colours(line).is_empty(), "{}:{}: {line}", path.display(), n + 1);
        }
    }
}
```

`tests/css.rs` needs a `templates/` directory to exist. Create `templates/.keep` (empty) in this step.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p skimasque-visual --test css`
Expected: FAIL, because the package `skimasque-visual` isn't found (the crate doesn't exist yet).

- [ ] **Step 3: Create the crate**

Root `Cargo.toml`, `[workspace.dependencies]`, after `thiserror`:

```toml
# Server-rendered components (crate `skimasque-visual`). 0.12 to match the
# control plane's dashboard, which renders these components.
askama = "0.12"
```

`crates/skimasque-visual/Cargo.toml`:

```toml
[package]
name = "skimasque-visual"
description = "SkiMasque's visual language: nodes, connections, statuses, flows and diagrams, rendered server-side for the dashboard and the website."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish = false

[dependencies]
askama.workspace = true
```

`crates/skimasque-visual/askama.toml`:

```toml
[general]
dirs = ["templates"]
```

`crates/skimasque-visual/src/lib.rs`:

```rust
//! SkiMasque's visual language, rendered on the server.
//!
//! Every component is a plain struct that renders itself with askama. Callers
//! embed the result with `{{ component|safe }}`: the dashboard fills
//! components with live data, the website's generator with examples, and both
//! share [`CSS`]. Components escape every string they are given; the only
//! markup a component accepts is [`Html`] produced by another component.

#![forbid(unsafe_code)]

use std::fmt;

/// The library stylesheet: design tokens, then the component layers.
pub const CSS: &str = include_str!("../static/visual.css");

/// Rendered markup from a library component. Only this crate can create one,
/// so a caller cannot smuggle unescaped strings into a component that nests
/// others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Html(String);

impl Html {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Html {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A library component: anything that renders to [`Html`].
pub trait Component: askama::Template {
    fn html(&self) -> Html {
        // Component templates only format owned strings; rendering cannot
        // fail except on a formatter error, which would be a bug here.
        Html(self.render().expect("component templates render infallibly"))
    }
}
```

- [ ] **Step 4: Write `static/visual.css` (tokens + base)**

```css
/* SkiMasque visual library. Canonical source of the Alpine tokens: the
   dashboard (skimasque-dev/control) and the website both load this file.
   1 tokens · 2 roles · 3 base utilities · 4+ component layers (appended by later tasks). */

/* tokens */
:root {
  --alpine-950: #0b1117; --alpine-900: #111a22; --alpine-850: #16212a; --alpine-800: #1d2a34;
  --alpine-700: #30414d; --alpine-600: #435763; --alpine-500: #61737e; --alpine-400: #81919a;
  --snow-50: #f7faf9; --snow-100: #edf3f1; --snow-200: #dce7e3; --white: #ffffff;
  --mint-500: #63d7b1; --mint-400: #7de4c2; --mint-300: #a1efd5; --mint-200: #c9f7e7;
  --forest-950: #0b1512; --forest-900: #102019; --forest-800: #173026; --forest-700: #234638; --forest-600: #35614d;
  --earth-950: #17120e; --earth-900: #211914; --earth-800: #30221a; --earth-700: #4a3527; --earth-600: #66503d;
  --success: #63d7b1; --warning: #e7c66a; --danger: #e87979; --info: #79bde8;
  --mint-ink: #0b7a5b; --danger-ink: #b93b3b; --warning-ink: #8a6300; --info-ink: #1f6fa8;
  --font: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  --radius-sm: 4px; --radius-md: 8px; --radius-lg: 12px;
}
/* end tokens */

/* roles: dark is the default */
:root, [data-theme="dark"] {
  color-scheme: dark;
  --bg: var(--alpine-950); --surface: var(--alpine-900); --surface-raised: var(--alpine-850);
  --text: var(--snow-50); --text-muted: var(--alpine-400);
  --line: rgba(255,255,255,.10); --line-strong: rgba(255,255,255,.20);
  --accent: var(--mint-500); --accent-text: var(--mint-400); --on-accent: var(--alpine-950); --focus: var(--mint-400);
  --v-active: var(--mint-500); --v-structure: var(--forest-600); --v-structure-fill: var(--forest-900);
  --v-edge: var(--earth-600); --v-edge-fill: var(--earth-900);
  --v-neutral: var(--alpine-500); --v-deny: var(--danger); --v-info: var(--info);
}
@media (prefers-color-scheme: light) {
  :root:not([data-theme="dark"]) {
    color-scheme: light;
    --bg: var(--snow-50); --surface: var(--white); --surface-raised: var(--snow-100);
    --text: var(--alpine-950); --text-muted: var(--alpine-600);
    --line: rgba(11,17,23,.12); --line-strong: rgba(11,17,23,.24);
    --accent-text: var(--mint-ink); --focus: var(--mint-ink);
    --v-active: var(--mint-ink); --v-structure: var(--forest-600); --v-structure-fill: var(--snow-100);
    --v-edge: var(--earth-600); --v-edge-fill: var(--snow-100);
    --v-neutral: var(--alpine-500); --v-deny: var(--danger-ink); --v-info: var(--info-ink);
  }
}
[data-theme="light"] {
  color-scheme: light;
  --bg: var(--snow-50); --surface: var(--white); --surface-raised: var(--snow-100);
  --text: var(--alpine-950); --text-muted: var(--alpine-600);
  --line: rgba(11,17,23,.12); --line-strong: rgba(11,17,23,.24);
  --accent-text: var(--mint-ink); --focus: var(--mint-ink);
  --v-active: var(--mint-ink); --v-structure: var(--forest-600); --v-structure-fill: var(--snow-100);
  --v-edge: var(--earth-600); --v-edge-fill: var(--snow-100);
  --v-neutral: var(--alpine-500); --v-deny: var(--danger-ink); --v-info: var(--info-ink);
}

/* base utilities */
.v-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
.v-tone-active    { --v-tone: var(--v-active); }
.v-tone-structure { --v-tone: var(--v-structure); }
.v-tone-edge      { --v-tone: var(--v-edge); }
.v-tone-neutral   { --v-tone: var(--v-neutral); }
.v-tone-deny      { --v-tone: var(--v-deny); }
.v-tone-info      { --v-tone: var(--v-info); }
```

The palette test asserts `--danger: #e87979`, which the token block above contains.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p skimasque-visual --test css`
Expected: 3 passed.

- [ ] **Step 6: Commit**

```bash
rustfmt --edition 2021 crates/skimasque-visual/src/lib.rs crates/skimasque-visual/tests/css.rs
cargo clippy -p skimasque-visual --all-targets -- -D warnings
git add Cargo.toml Cargo.lock crates/skimasque-visual
git commit -m "feat(visual): skimasque-visual crate with Alpine tokens"
```

---

### Task 2: Icon sprite and `NodeKind`

**Files:**
- Create: `src/icons.rs`, `src/node.rs` (the `NodeKind` and `Tone` part only; `Node` comes in Task 4), `templates/icons.html`
- Modify: `src/lib.rs` (`mod icons; mod node; pub use icons::Icons; pub use node::{NodeKind, Tone};`)

**Interfaces:**
- Produces:

```rust
pub enum Tone { Active, Structure, Edge, Neutral, Deny, Info }   // fn class(self) -> &'static str  ("active"…)
pub enum NodeKind { Workload, Developer, GitHub, CiJob, Application, Identity, Policy, Session,
                    Gateway, Network, Service, Database, Api, Kubernetes, Cloud, Firewall, Internet, Allow, Deny }
impl NodeKind {
    pub const ALL: [NodeKind; 19];
    pub fn slug(self) -> &'static str;   // "workload", "ci-job", "github", …
    pub fn icon(self) -> &'static str;   // "i-workload" …; Session → "i-clock", Allow → "i-check", Deny → "i-cross"
    pub fn tone(self) -> Tone;           // per spec grammar
    pub fn default_label(self) -> &'static str; // "Workload", "GitHub Actions", "CI job", …
}
pub struct Icons;  // impl Component; renders the <svg> sprite with one <symbol id="i-…"> per icon
```

- [ ] **Step 1: Write the failing tests** (in `src/icons.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, NodeKind};

    #[test]
    fn every_node_kind_has_a_symbol_in_the_sprite() {
        let sprite = Icons.html().as_str().to_owned();
        assert!(sprite.contains(r#"aria-hidden="true""#) && sprite.contains("display:none") || sprite.contains(r#"class="v-sprite""#));
        for kind in NodeKind::ALL {
            assert!(sprite.contains(&format!(r#"<symbol id="{}""#, kind.icon())), "{:?} → {}", kind, kind.icon());
        }
        for extra in ["i-arrow", "i-check", "i-cross", "i-clock"] {
            assert!(sprite.contains(&format!(r#"<symbol id="{extra}""#)), "{extra}");
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
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p skimasque-visual --lib icons::`
Expected: compile error, because `Icons` isn't found.

- [ ] **Step 3: Implement `NodeKind`/`Tone` in `src/node.rs`**

```rust
//! Node kinds and the visual grammar that colours them (spec: Components → Node).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone { Active, Structure, Edge, Neutral, Deny, Info }

impl Tone {
    pub fn class(self) -> &'static str {
        match self {
            Tone::Active => "active", Tone::Structure => "structure", Tone::Edge => "edge",
            Tone::Neutral => "neutral", Tone::Deny => "deny", Tone::Info => "info",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Workload, Developer, GitHub, CiJob, Application, Identity, Policy, Session, Gateway,
    Network, Service, Database, Api, Kubernetes, Cloud, Firewall, Internet, Allow, Deny,
}

impl NodeKind {
    pub const ALL: [NodeKind; 19] = [
        NodeKind::Workload, NodeKind::Developer, NodeKind::GitHub, NodeKind::CiJob,
        NodeKind::Application, NodeKind::Identity, NodeKind::Policy, NodeKind::Session,
        NodeKind::Gateway, NodeKind::Network, NodeKind::Service, NodeKind::Database,
        NodeKind::Api, NodeKind::Kubernetes, NodeKind::Cloud, NodeKind::Firewall,
        NodeKind::Internet, NodeKind::Allow, NodeKind::Deny,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            NodeKind::Workload => "workload", NodeKind::Developer => "developer",
            NodeKind::GitHub => "github", NodeKind::CiJob => "ci-job",
            NodeKind::Application => "application", NodeKind::Identity => "identity",
            NodeKind::Policy => "policy", NodeKind::Session => "session",
            NodeKind::Gateway => "gateway", NodeKind::Network => "network",
            NodeKind::Service => "service", NodeKind::Database => "database",
            NodeKind::Api => "api", NodeKind::Kubernetes => "kubernetes",
            NodeKind::Cloud => "cloud", NodeKind::Firewall => "firewall",
            NodeKind::Internet => "internet", NodeKind::Allow => "allow", NodeKind::Deny => "deny",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            NodeKind::Session => "i-clock",
            NodeKind::Allow => "i-check",
            NodeKind::Deny => "i-cross",
            NodeKind::Workload => "i-workload", NodeKind::Developer => "i-developer",
            NodeKind::GitHub => "i-github", NodeKind::CiJob => "i-ci-job",
            NodeKind::Application => "i-application", NodeKind::Identity => "i-identity",
            NodeKind::Policy => "i-policy", NodeKind::Gateway => "i-gateway",
            NodeKind::Network => "i-network", NodeKind::Service => "i-service",
            NodeKind::Database => "i-database", NodeKind::Api => "i-api",
            NodeKind::Kubernetes => "i-kubernetes", NodeKind::Cloud => "i-cloud",
            NodeKind::Firewall => "i-firewall", NodeKind::Internet => "i-internet",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            NodeKind::Network | NodeKind::Service | NodeKind::Database | NodeKind::Api
            | NodeKind::Kubernetes | NodeKind::Cloud => Tone::Structure,
            NodeKind::Gateway | NodeKind::Firewall | NodeKind::Internet => Tone::Edge,
            NodeKind::Session | NodeKind::Allow => Tone::Active,
            NodeKind::Deny => Tone::Deny,
            _ => Tone::Neutral,
        }
    }

    pub fn default_label(self) -> &'static str {
        match self {
            NodeKind::Workload => "Workload", NodeKind::Developer => "Developer",
            NodeKind::GitHub => "GitHub Actions", NodeKind::CiJob => "CI job",
            NodeKind::Application => "Application", NodeKind::Identity => "Identity",
            NodeKind::Policy => "Policy", NodeKind::Session => "Session",
            NodeKind::Gateway => "Gateway", NodeKind::Network => "Private network",
            NodeKind::Service => "Service", NodeKind::Database => "Database",
            NodeKind::Api => "Internal API", NodeKind::Kubernetes => "Kubernetes API",
            NodeKind::Cloud => "SkiMasque Cloud", NodeKind::Firewall => "Firewall",
            NodeKind::Internet => "Internet", NodeKind::Allow => "Allow", NodeKind::Deny => "Deny",
        }
    }
}
```

- [ ] **Step 4: Implement `Icons` in `src/icons.rs` and `templates/icons.html`**

```rust
//! The icon sprite: emit once per page, then reference icons with
//! `<svg><use href="#i-…"/></svg>`. Stroke icons drawn in `currentColor`.

use askama::Template;

#[derive(Template)]
#[template(path = "icons.html")]
pub struct Icons;

impl crate::Component for Icons {}
```

`templates/icons.html` (24×24, `fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"` on each symbol):

```html
<svg class="v-sprite" aria-hidden="true" focusable="false" style="display:none" xmlns="http://www.w3.org/2000/svg">
{%- let a = "fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.6\" stroke-linecap=\"round\" stroke-linejoin=\"round\"" %}
<symbol id="i-workload" viewBox="0 0 24 24" {{ a|safe }}><rect x="4" y="5" width="16" height="14" rx="2"/><path d="M8 10h8M8 14h5"/></symbol>
<symbol id="i-developer" viewBox="0 0 24 24" {{ a|safe }}><circle cx="12" cy="8" r="3.5"/><path d="M5 20c1-4 4-6 7-6s6 2 7 6"/></symbol>
<symbol id="i-github" viewBox="0 0 24 24" {{ a|safe }}><circle cx="7" cy="6" r="2"/><circle cx="7" cy="18" r="2"/><circle cx="17" cy="8" r="2"/><path d="M7 8v8M17 10c0 4-4 4-8 6"/></symbol>
<symbol id="i-ci-job" viewBox="0 0 24 24" {{ a|safe }}><path d="M4 7h10M4 12h16M4 17h10"/><path d="M17 4.5l3 2.5-3 2.5"/></symbol>
<symbol id="i-application" viewBox="0 0 24 24" {{ a|safe }}><rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 10l3 2-3 2M12 15h5"/></symbol>
<symbol id="i-identity" viewBox="0 0 24 24" {{ a|safe }}><circle cx="8" cy="12" r="3.5"/><path d="M11.5 12H21M17 12v3M20 12v2"/></symbol>
<symbol id="i-policy" viewBox="0 0 24 24" {{ a|safe }}><path d="M6 3h9l3 3v15H6z"/><path d="M9 13l2 2 4-4"/></symbol>
<symbol id="i-clock" viewBox="0 0 24 24" {{ a|safe }}><circle cx="12" cy="12" r="8"/><path d="M12 8v4l3 2"/></symbol>
<symbol id="i-gateway" viewBox="0 0 24 24" {{ a|safe }}><path d="M4 20V9l8-5 8 5v11"/><path d="M9 20v-6h6v6"/></symbol>
<symbol id="i-network" viewBox="0 0 24 24" {{ a|safe }}><circle cx="12" cy="5" r="2"/><circle cx="5" cy="18" r="2"/><circle cx="19" cy="18" r="2"/><path d="M11 6.8L6 16.2M13 6.8l5 9.4M7 18h10"/></symbol>
<symbol id="i-service" viewBox="0 0 24 24" {{ a|safe }}><rect x="4" y="4" width="16" height="7" rx="1.5"/><rect x="4" y="13" width="16" height="7" rx="1.5"/><path d="M8 7.5h.01M8 16.5h.01"/></symbol>
<symbol id="i-database" viewBox="0 0 24 24" {{ a|safe }}><ellipse cx="12" cy="6" rx="7" ry="2.5"/><path d="M5 6v12c0 1.4 3.1 2.5 7 2.5s7-1.1 7-2.5V6M5 12c0 1.4 3.1 2.5 7 2.5s7-1.1 7-2.5"/></symbol>
<symbol id="i-api" viewBox="0 0 24 24" {{ a|safe }}><path d="M9 4C7 4 6 5 6 7v2c0 1-1 3-2 3 1 0 2 2 2 3v2c0 2 1 3 3 3M15 4c2 0 3 1 3 3v2c0 1 1 3 2 3-1 0-2 2-2 3v2c0 2-1 3-3 3"/></symbol>
<symbol id="i-kubernetes" viewBox="0 0 24 24" {{ a|safe }}><circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="2.5"/><path d="M12 4v5.5M12 14.5V20M4 12h5.5M14.5 12H20"/></symbol>
<symbol id="i-cloud" viewBox="0 0 24 24" {{ a|safe }}><path d="M7 18h10a4 4 0 0 0 .5-8A6 6 0 0 0 6 9a4.5 4.5 0 0 0 1 9z"/></symbol>
<symbol id="i-firewall" viewBox="0 0 24 24" {{ a|safe }}><rect x="3" y="5" width="18" height="14" rx="1"/><path d="M3 10h18M3 15h18M9 5v5M15 10v5M9 15v4"/></symbol>
<symbol id="i-internet" viewBox="0 0 24 24" {{ a|safe }}><circle cx="12" cy="12" r="8"/><path d="M4 12h16M12 4c2.5 2.5 2.5 13.5 0 16M12 4c-2.5 2.5-2.5 13.5 0 16"/></symbol>
<symbol id="i-check" viewBox="0 0 24 24" {{ a|safe }}><path d="M5 12.5l4.5 4.5L19 7"/></symbol>
<symbol id="i-cross" viewBox="0 0 24 24" {{ a|safe }}><path d="M6 6l12 12M18 6L6 18"/></symbol>
<symbol id="i-arrow" viewBox="0 0 24 24" {{ a|safe }}><path d="M5 12h14M13 6l6 6-6 6"/></symbol>
</svg>
```

If askama 0.12 rejects `{% let %}` with escaped quotes, write the attributes out on each `<symbol>` instead. The output must be identical in substance. The first test's sprite assertion accepts either the `display:none` style or the `v-sprite` class; keep both.

Also add to `visual.css`:

```css
/* icons */
.v-icon { width: 18px; height: 18px; flex: 0 0 auto; color: var(--v-tone, var(--text-muted)); }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p skimasque-visual`
Expected: all pass, including `templates_hard_code_no_colours`.

- [ ] **Step 6: Commit**

```bash
rustfmt --edition 2021 crates/skimasque-visual/src/*.rs
cargo clippy -p skimasque-visual --all-targets -- -D warnings
git add crates/skimasque-visual
git commit -m "feat(visual): icon sprite and node kinds with the colour grammar"
```

---

### Task 3: `Status`, `StatusBadge`, `DecisionBadge`

**Files:**
- Create: `src/status.rs`, `templates/status.html`, `templates/decision_badge.html`
- Modify: `src/lib.rs` (`mod status; pub use status::{Status, StatusBadge, DecisionBadge};`), `static/visual.css`

**Interfaces:**
- Produces:

```rust
pub enum Status { Allow, Deny, Active, Expired, Pending, Blocked }
impl Status { pub fn word(self) -> &'static str; pub fn tone(self) -> Tone; }
pub struct StatusBadge { pub status: Status }           // impl Component
pub struct DecisionBadge { pub allow: bool }            // impl Component
```

- [ ] **Step 1: Write the failing tests** (in `src/status.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    #[test]
    fn every_status_carries_its_word_beside_the_dot() {
        for (s, word, tone) in [
            (Status::Allow, "ALLOW", "active"), (Status::Deny, "DENY", "deny"),
            (Status::Active, "ACTIVE", "active"), (Status::Expired, "EXPIRED", "neutral"),
            (Status::Pending, "PENDING", "info"), (Status::Blocked, "BLOCKED", "deny"),
        ] {
            let html = StatusBadge { status: s }.html().as_str().to_owned();
            assert_eq!(
                html,
                format!(r#"<span class="v-status v-tone-{tone}"><span aria-hidden="true">●</span> {word}</span>"#)
            );
        }
    }

    #[test]
    fn decision_badges_say_granted_or_denied_in_words() {
        assert_eq!(
            DecisionBadge { allow: true }.html().as_str(),
            r#"<span class="v-decision v-tone-active"><span aria-hidden="true">✓</span> ACCESS GRANTED</span>"#
        );
        assert_eq!(
            DecisionBadge { allow: false }.html().as_str(),
            r#"<span class="v-decision v-tone-deny"><span aria-hidden="true">×</span> ACCESS DENIED</span>"#
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p skimasque-visual --lib status::`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Status words and decision badges. A dot is never shown without its word.

use askama::Template;

use crate::Tone;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status { Allow, Deny, Active, Expired, Pending, Blocked }

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Allow => "ALLOW", Status::Deny => "DENY", Status::Active => "ACTIVE",
            Status::Expired => "EXPIRED", Status::Pending => "PENDING", Status::Blocked => "BLOCKED",
        }
    }
    pub fn tone(self) -> Tone {
        match self {
            Status::Allow | Status::Active => Tone::Active,
            Status::Deny | Status::Blocked => Tone::Deny,
            Status::Expired => Tone::Neutral,
            Status::Pending => Tone::Info,
        }
    }
}

#[derive(Template)]
#[template(path = "status.html")]
pub struct StatusBadge { pub status: Status }
impl crate::Component for StatusBadge {}

#[derive(Template)]
#[template(path = "decision_badge.html")]
pub struct DecisionBadge { pub allow: bool }
impl crate::Component for DecisionBadge {}
```

`templates/status.html` (a single line with no trailing newline, so the exact-string test holds; if your editor adds one, use `{%- -%}` trimming):

```html
<span class="v-status v-tone-{{ status.tone().class() }}"><span aria-hidden="true">●</span> {{ status.word() }}</span>
```

`templates/decision_badge.html`:

```html
{% if allow %}<span class="v-decision v-tone-active"><span aria-hidden="true">✓</span> ACCESS GRANTED</span>{% else %}<span class="v-decision v-tone-deny"><span aria-hidden="true">×</span> ACCESS DENIED</span>{% endif %}
```

Askama keeps a template file's final newline by default in 0.12. If the exact-string assertions fail on a trailing `\n`, set `[general] whitespace = "suppress"` in `askama.toml` only if that doesn't break other templates, or compare against `.trim_end()` in the test. Prefer the latter, which changes the test minimally, and note it in the report.

`visual.css`:

```css
/* status */
.v-status, .v-decision { display: inline-flex; align-items: center; gap: 6px; font: 600 11px/1.4 var(--font);
  letter-spacing: .08em; color: var(--v-tone); white-space: nowrap; }
.v-decision { font-size: 12px; padding: 4px 10px; border: 1px solid var(--v-tone); border-radius: var(--radius-sm); }
```

- [ ] **Step 4: Run the tests and commit**

```bash
cargo test -p skimasque-visual
rustfmt --edition 2021 crates/skimasque-visual/src/status.rs
cargo clippy -p skimasque-visual --all-targets -- -D warnings
git add crates/skimasque-visual
git commit -m "feat(visual): status words and decision badges"
```

---

### Task 4: `Node`

**Files:**
- Create: `templates/node.html`
- Modify: `src/node.rs` (add `Node`), `src/lib.rs` (`pub use node::Node;`), `static/visual.css`

**Interfaces:**
- Produces:

```rust
pub struct Node { pub kind: NodeKind, pub label: String, pub sub: Option<String>, pub status: Option<Status> }
impl Node {
    pub fn new(kind: NodeKind) -> Self;                // label = kind.default_label()
    pub fn label(self, label: impl Into<String>) -> Self;
    pub fn sub(self, sub: impl Into<String>) -> Self;
    pub fn status(self, status: Status) -> Self;
}
// impl Component
```

- [ ] **Step 1: Write the failing tests** (append to `src/node.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Status};

    #[test]
    fn a_node_shows_icon_label_sub_and_status_in_its_tone() {
        let html = Node::new(NodeKind::GitHub).sub("acme/widget").status(Status::Active).html();
        let h = html.as_str();
        assert!(h.contains(r#"class="v-node v-tone-neutral""#), "{h}");
        assert!(h.contains(r#"data-kind="github""#), "{h}");
        assert!(h.contains(r##"<use href="#i-github"></use>"##), "{h}");
        assert!(h.contains(r#"<span class="v-node-label">GitHub Actions</span>"#), "{h}");
        assert!(h.contains(r#"<span class="v-node-sub">acme/widget</span>"#), "{h}");
        assert!(h.contains("ACTIVE"), "status word: {h}");
    }

    #[test]
    fn node_text_is_escaped() {
        let h = Node::new(NodeKind::Service).label("<script>x</script>").sub("a \"b\" & c").html();
        assert!(!h.as_str().contains("<script>"), "{h}");
        assert!(h.as_str().contains("&lt;script&gt;"), "{h}");
        assert!(h.as_str().contains("&quot;b&quot; &amp; c") || h.as_str().contains("&#34;b&#34; &amp; c"), "{h}");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p skimasque-visual --lib node::`
Expected: compile error, because `Node` isn't found.

- [ ] **Step 3: Implement**

In `src/node.rs`:

```rust
use askama::Template;

use crate::{Component, Html, Status, StatusBadge};

/// One thing in a diagram: a workload, a policy, a gateway, a database…
#[derive(Template, Debug, Clone)]
#[template(path = "node.html")]
pub struct Node {
    pub kind: NodeKind,
    pub label: String,
    pub sub: Option<String>,
    pub status: Option<Status>,
}

impl Node {
    pub fn new(kind: NodeKind) -> Self {
        Node { kind, label: kind.default_label().to_owned(), sub: None, status: None }
    }
    pub fn label(mut self, label: impl Into<String>) -> Self { self.label = label.into(); self }
    pub fn sub(mut self, sub: impl Into<String>) -> Self { self.sub = Some(sub.into()); self }
    pub fn status(mut self, status: Status) -> Self { self.status = Some(status); self }

    fn status_html(&self) -> Option<Html> {
        self.status.map(|status| StatusBadge { status }.html())
    }
}

impl Component for Node {}
```

`templates/node.html`:

```html
<div class="v-node v-tone-{{ kind.tone().class() }}" data-kind="{{ kind.slug() }}"><svg class="v-icon" aria-hidden="true" focusable="false"><use href="#{{ kind.icon() }}"></use></svg><span class="v-node-text"><span class="v-node-label">{{ label }}</span>{% if let Some(s) = sub %}<span class="v-node-sub">{{ s }}</span>{% endif %}</span>{% if let Some(h) = self.status_html() %}{{ h|safe }}{% endif %}</div>
```

`visual.css`:

```css
/* node */
.v-node { display: inline-flex; align-items: center; gap: 10px; min-width: 0; max-width: 100%;
  padding: 10px 12px; border: 1px solid color-mix(in srgb, var(--v-tone) 55%, transparent);
  border-radius: var(--radius-md); background: var(--surface); color: var(--text); font: 13px/1.35 var(--font); }
.v-node.v-tone-structure { background: var(--v-structure-fill); }
.v-node.v-tone-edge { background: var(--v-edge-fill); }
.v-node-text { display: grid; min-width: 0; }
.v-node-label { font-weight: 600; overflow-wrap: anywhere; }
.v-node-sub { color: var(--text-muted); font-size: 12px; overflow-wrap: anywhere; }
.v-node .v-status { margin-left: auto; }
```

- [ ] **Step 4: Run the tests and commit**

```bash
cargo test -p skimasque-visual
rustfmt --edition 2021 crates/skimasque-visual/src/node.rs
cargo clippy -p skimasque-visual --all-targets -- -D warnings
git add crates/skimasque-visual
git commit -m "feat(visual): Node component"
```

---

### Task 5: `Connection`

**Files:**
- Create: `src/connection.rs`, `templates/connection.html`
- Modify: `src/lib.rs` (`mod connection; pub use connection::{ConnKind, Connection};`), `static/visual.css`

**Interfaces:**
- Produces:

```rust
pub enum ConnKind { Normal, Control, Active, Potential, Denied }
impl ConnKind { pub fn slug(self) -> &'static str; pub fn spoken(self) -> &'static str; }
// spoken: Normal "", Control "control plane", Active "active", Potential "potential", Denied "denied"
pub struct Connection { pub kind: ConnKind, pub label: Option<String> }
impl Connection { pub fn new(kind: ConnKind) -> Self; pub fn label(self, l: impl Into<String>) -> Self; }
// impl Component
```

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    #[test]
    fn each_kind_has_its_class_and_spoken_word() {
        for (k, slug, word) in [
            (ConnKind::Normal, "normal", ""), (ConnKind::Control, "control", "control plane"),
            (ConnKind::Active, "active", "active"), (ConnKind::Potential, "potential", "potential"),
            (ConnKind::Denied, "denied", "denied"),
        ] {
            let h = Connection::new(k).html().as_str().to_owned();
            assert!(h.contains(&format!(r#"class="v-conn v-conn-{slug}""#)), "{h}");
            if word.is_empty() {
                assert!(!h.contains("v-sr"), "{h}");
            } else {
                assert!(h.contains(&format!(r#"<span class="v-sr">{word}</span>"#)), "{h}");
            }
        }
        assert!(Connection::new(ConnKind::Denied).html().as_str().contains(r#"<span class="v-conn-x" aria-hidden="true">×</span>"#));
    }

    #[test]
    fn labels_are_shown_and_escaped() {
        let h = Connection::new(ConnKind::Control).label("OIDC <tok>").html();
        assert!(h.as_str().contains(r#"<span class="v-conn-label">OIDC &lt;tok&gt;</span>"#), "{h}");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p skimasque-visual --lib connection::`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Connections between nodes. Solid = data, dashed = control plane, thick
//! mint = an active session, dotted = a potential route, broken with × =
//! denied. The meaning is also spoken for screen readers.

use askama::Template;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnKind { Normal, Control, Active, Potential, Denied }

impl ConnKind {
    pub fn slug(self) -> &'static str {
        match self {
            ConnKind::Normal => "normal", ConnKind::Control => "control", ConnKind::Active => "active",
            ConnKind::Potential => "potential", ConnKind::Denied => "denied",
        }
    }
    pub fn spoken(self) -> &'static str {
        match self {
            ConnKind::Normal => "", ConnKind::Control => "control plane", ConnKind::Active => "active",
            ConnKind::Potential => "potential", ConnKind::Denied => "denied",
        }
    }
    fn is_denied(self) -> bool { self == ConnKind::Denied }
}

#[derive(Template, Debug, Clone)]
#[template(path = "connection.html")]
pub struct Connection { pub kind: ConnKind, pub label: Option<String> }

impl Connection {
    pub fn new(kind: ConnKind) -> Self { Connection { kind, label: None } }
    pub fn label(mut self, label: impl Into<String>) -> Self { self.label = Some(label.into()); self }
}

impl crate::Component for Connection {}
```

`templates/connection.html`:

```html
<span class="v-conn v-conn-{{ kind.slug() }}"><span class="v-conn-line" aria-hidden="true"></span>{% if kind.is_denied() %}<span class="v-conn-x" aria-hidden="true">×</span>{% endif %}{% if let Some(l) = label %}<span class="v-conn-label">{{ l }}</span>{% endif %}{% if !kind.spoken().is_empty() %}<span class="v-sr">{{ kind.spoken() }}</span>{% endif %}</span>
```

`is_denied` is private but used by the template. Askama generates the impl in the same module, so that compiles.

`visual.css`: the orientation is set by the flow container (Task 6). Standalone, a connection is horizontal.

```css
/* connection */
.v-conn { position: relative; display: inline-flex; align-items: center; justify-content: center;
  gap: 6px; min-width: 48px; min-height: 28px; color: var(--text-muted); font: 11px/1.3 var(--font); }
.v-conn-line { flex: 1 1 auto; align-self: center; min-width: 32px; height: 0;
  border-top: 2px solid var(--v-neutral); position: relative; }
.v-conn-line::after { content: "▸"; position: absolute; right: -6px; top: -9px; font-size: 12px; color: inherit; }
.v-conn-label { position: absolute; top: -2px; left: 50%; transform: translate(-50%, -100%); white-space: nowrap;
  padding: 0 4px; background: var(--bg); }
.v-conn-control .v-conn-line { border-top-style: dashed; }
.v-conn-potential { opacity: .55; }
.v-conn-potential .v-conn-line { border-top-style: dotted; }
.v-conn-active { color: var(--v-active); }
.v-conn-active .v-conn-line { border-top: 3px solid var(--v-active); animation: v-glow 2.4s ease-in-out infinite; }
.v-conn-denied { color: var(--v-deny); }
.v-conn-denied .v-conn-line { border-top: 2px dashed var(--v-deny); }
.v-conn-x { position: absolute; left: 50%; top: 50%; transform: translate(-50%, -52%);
  font-size: 16px; font-weight: 700; color: var(--v-deny); background: var(--bg); padding: 0 2px; }
@keyframes v-glow { 0%, 100% { opacity: .55; } 50% { opacity: 1; } }
@media (prefers-reduced-motion: reduce) { .v-conn-active .v-conn-line { animation: none; } }
```

- [ ] **Step 4: Run the tests and commit**

```bash
cargo test -p skimasque-visual
rustfmt --edition 2021 crates/skimasque-visual/src/connection.rs
cargo clippy -p skimasque-visual --all-targets -- -D warnings
git add crates/skimasque-visual
git commit -m "feat(visual): Connection component with the line grammar"
```

---

### Task 6: `Flow`

**Files:**
- Create: `src/flow.rs`, `templates/flow.html`
- Modify: `src/lib.rs` (`mod flow; pub use flow::{Flow, Step};`), `static/visual.css`

**Interfaces:**
- Produces:

```rust
pub struct Step { pub node: Node, pub next: Option<Connection> }
pub struct Flow { pub steps: Vec<Step>, pub caption: String }  // caption: the text equivalent
impl Flow {
    pub fn new(caption: impl Into<String>) -> Self;
    pub fn then(self, node: Node) -> Self;                          // appends a step; the previous step gets a Normal connection if it has none
    pub fn via(self, conn: Connection, node: Node) -> Self;         // appends with an explicit connection from the previous step
}
// impl Component; the last step never renders a connection
```

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, ConnKind, NodeKind};

    #[test]
    fn a_flow_is_an_ordered_list_with_connections_between_steps_only() {
        let f = Flow::new("GitHub Actions reaches the database through a session.")
            .then(Node::new(NodeKind::GitHub))
            .via(Connection::new(ConnKind::Control).label("OIDC"), Node::new(NodeKind::Identity))
            .via(Connection::new(ConnKind::Active), Node::new(NodeKind::Database));
        let h = f.html().as_str().to_owned();
        assert!(h.starts_with(r#"<figure class="v-flow-wrap">"#), "{h}");
        assert_eq!(h.matches(r#"<li class="v-flow-step">"#).count(), 3, "{h}");
        assert_eq!(h.matches(r#"class="v-conn "#).count(), 2, "two connections for three steps: {h}");
        assert!(h.contains("v-conn-control") && h.contains("v-conn-active"), "{h}");
        assert!(h.contains(r#"<figcaption class="v-sr">GitHub Actions reaches the database through a session.</figcaption>"#), "{h}");
    }

    #[test]
    fn a_single_step_flow_has_no_dangling_connector() {
        let h = Flow::new("Just a gateway.").then(Node::new(NodeKind::Gateway)).html();
        assert!(!h.as_str().contains("v-conn"), "{h}");
    }

    #[test]
    fn then_defaults_the_link_to_a_normal_connection() {
        let h = Flow::new("x").then(Node::new(NodeKind::Workload)).then(Node::new(NodeKind::Policy)).html();
        assert!(h.as_str().contains("v-conn-normal"), "{h}");
    }
}
```

The count uses `class="v-conn ` (with a trailing space) to match only the outer connection span, since class values are `v-conn v-conn-<kind>`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p skimasque-visual --lib flow::`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! A linear flow of nodes. Horizontal on wide screens, vertical on narrow
//! ones (CSS), and an ordered list either way so the sequence survives
//! without styles. The caption is the flow's text equivalent.

use askama::Template;

use crate::{Component, ConnKind, Connection, Html, Node};

#[derive(Debug, Clone)]
pub struct Step { pub node: Node, pub next: Option<Connection> }

#[derive(Template, Debug, Clone)]
#[template(path = "flow.html")]
pub struct Flow { pub steps: Vec<Step>, pub caption: String }

impl Flow {
    pub fn new(caption: impl Into<String>) -> Self { Flow { steps: Vec::new(), caption: caption.into() } }

    pub fn then(self, node: Node) -> Self { self.push(None, node) }

    pub fn via(self, conn: Connection, node: Node) -> Self { self.push(Some(conn), node) }

    fn push(mut self, conn: Option<Connection>, node: Node) -> Self {
        if let Some(prev) = self.steps.last_mut() {
            prev.next = Some(conn.unwrap_or_else(|| Connection::new(ConnKind::Normal)));
        }
        self.steps.push(Step { node, next: None });
        self
    }

    fn node_html(step: &Step) -> Html { step.node.html() }
    fn conn_html(step: &Step) -> Option<Html> { step.next.as_ref().map(|c| c.html()) }
}

impl Component for Flow {}
```

`templates/flow.html`:

```html
<figure class="v-flow-wrap"><ol class="v-flow">{% for s in steps %}<li class="v-flow-step">{{ Self::node_html(s)|safe }}{% if !loop.last %}{% if let Some(c) = Self::conn_html(s) %}{{ c|safe }}{% endif %}{% endif %}</li>{% endfor %}</ol><figcaption class="v-sr">{{ caption }}</figcaption></figure>
```

If askama 0.12 rejects `Self::` paths in expressions, make `node_html`/`conn_html` methods on `Step` (`s.node_html()`), and keep the output identical.

`visual.css`:

```css
/* flow: vertical by default, horizontal from 720px */
.v-flow-wrap { margin: 0; }
.v-flow { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; align-items: flex-start; }
.v-flow-step { display: flex; flex-direction: column; align-items: flex-start; }
.v-flow .v-conn { min-height: 36px; min-width: 0; margin-left: 20px; flex-direction: column; }
.v-flow .v-conn-line { min-width: 0; min-height: 28px; width: 0; height: auto; border-top: 0; border-left: 2px solid var(--v-neutral); }
.v-flow .v-conn-line::after { content: "▾"; right: auto; left: -6px; top: auto; bottom: -12px; }
.v-flow .v-conn-control .v-conn-line { border-left-style: dashed; }
.v-flow .v-conn-potential .v-conn-line { border-left-style: dotted; }
.v-flow .v-conn-active .v-conn-line { border-left: 3px solid var(--v-active); }
.v-flow .v-conn-denied .v-conn-line { border-left: 2px dashed var(--v-deny); }
.v-flow .v-conn-label { position: static; transform: none; background: none; padding: 0; margin-left: 10px; align-self: center; }
@media (min-width: 720px) {
  .v-flow { flex-direction: row; align-items: center; flex-wrap: nowrap; }
  .v-flow-step { flex-direction: row; align-items: center; }
  .v-flow .v-conn { flex-direction: row; min-height: 28px; min-width: 64px; margin: 0 4px; }
  .v-flow .v-conn-line { min-height: 0; min-width: 48px; height: 0; width: auto; border-left: 0; border-top: 2px solid var(--v-neutral); }
  .v-flow .v-conn-line::after { content: "▸"; left: auto; right: -6px; bottom: auto; top: -9px; }
  .v-flow .v-conn-control .v-conn-line { border-top-style: dashed; }
  .v-flow .v-conn-potential .v-conn-line { border-top-style: dotted; }
  .v-flow .v-conn-active .v-conn-line { border-top: 3px solid var(--v-active); }
  .v-flow .v-conn-denied .v-conn-line { border-top: 2px dashed var(--v-deny); }
  .v-flow .v-conn-label { position: absolute; top: -2px; left: 50%; transform: translate(-50%, -100%); margin: 0; background: var(--bg); padding: 0 4px; }
}
```

- [ ] **Step 4: Run the tests and commit**

```bash
cargo test -p skimasque-visual
rustfmt --edition 2021 crates/skimasque-visual/src/flow.rs
cargo clippy -p skimasque-visual --all-targets -- -D warnings
git add crates/skimasque-visual
git commit -m "feat(visual): responsive Flow of nodes and connections"
```

---

### Task 7: `Boundary`

**Files:**
- Create: `src/boundary.rs`, `templates/boundary.html`
- Modify: `src/lib.rs` (`mod boundary; pub use boundary::{Boundary, BoundaryKind};`), `static/visual.css`

**Interfaces:**
- Produces:

```rust
pub enum BoundaryKind { Region, Firewall }
pub struct Boundary { pub label: String, pub kind: BoundaryKind, pub children: Vec<Html> }
impl Boundary {
    pub fn region(label: impl Into<String>) -> Self;
    pub fn firewall(label: impl Into<String>) -> Self;
    pub fn child(self, c: &impl Component) -> Self;   // pushes c.html()
}
// impl Component
```

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Node, NodeKind};

    #[test]
    fn a_region_frames_its_children_under_a_label() {
        let h = Boundary::region("YOUR VPC").child(&Node::new(NodeKind::Gateway)).child(&Node::new(NodeKind::Database)).html();
        let s = h.as_str();
        assert!(s.starts_with(r#"<section class="v-boundary v-boundary-region" aria-label="YOUR VPC">"#), "{s}");
        assert!(s.contains(r#"<span class="v-boundary-label">YOUR VPC</span>"#), "{s}");
        assert_eq!(s.matches(r#"class="v-node "#).count(), 2, "{s}");
    }

    #[test]
    fn a_firewall_draws_a_labelled_rule_above_the_protected_side() {
        let s = Boundary::firewall("YOUR FIREWALL").child(&Node::new(NodeKind::Network)).html().as_str().to_owned();
        assert!(s.contains(r#"class="v-boundary v-boundary-firewall""#), "{s}");
        let rule = s.find("v-firewall-rule").unwrap();
        let node = s.find("v-node").unwrap();
        assert!(rule < node, "rule before the protected side: {s}");
    }

    #[test]
    fn the_label_is_escaped() {
        let s = Boundary::region("<b>vpc</b>").html().as_str().to_owned();
        assert!(!s.contains("<b>") && s.contains("&lt;b&gt;"), "{s}");
    }
}
```

The `class="v-node ` count relies on `Node` rendering `class="v-node v-tone-…"` (Task 4).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p skimasque-visual --lib boundary::`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Network boundaries: a labelled region ("YOUR VPC") or a firewall rule
//! with the protected side below it. Children are other components.

use askama::Template;

use crate::{Component, Html};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryKind { Region, Firewall }

#[derive(Template, Debug, Clone)]
#[template(path = "boundary.html")]
pub struct Boundary { pub label: String, pub kind: BoundaryKind, pub children: Vec<Html> }

impl Boundary {
    pub fn region(label: impl Into<String>) -> Self {
        Boundary { label: label.into(), kind: BoundaryKind::Region, children: Vec::new() }
    }
    pub fn firewall(label: impl Into<String>) -> Self {
        Boundary { label: label.into(), kind: BoundaryKind::Firewall, children: Vec::new() }
    }
    pub fn child(mut self, c: &impl Component) -> Self { self.children.push(c.html()); self }
    fn is_firewall(&self) -> bool { self.kind == BoundaryKind::Firewall }
}

impl Component for Boundary {}
```

`templates/boundary.html`:

```html
{% if self.is_firewall() %}<section class="v-boundary v-boundary-firewall" aria-label="{{ label }}"><div class="v-firewall-rule"><span class="v-boundary-label">{{ label }}</span></div><div class="v-boundary-body">{% for c in children %}{{ c|safe }}{% endfor %}</div></section>{% else %}<section class="v-boundary v-boundary-region" aria-label="{{ label }}"><span class="v-boundary-label">{{ label }}</span><div class="v-boundary-body">{% for c in children %}{{ c|safe }}{% endfor %}</div></section>{% endif %}
```

`visual.css`:

```css
/* boundary */
.v-boundary { position: relative; font: 12px/1.4 var(--font); }
.v-boundary-label { font-weight: 600; letter-spacing: .1em; color: var(--v-structure); }
.v-boundary-body { display: flex; flex-wrap: wrap; gap: 12px; align-items: center; }
.v-boundary-region { border: 1px dashed var(--v-structure); border-radius: var(--radius-lg);
  background: color-mix(in srgb, var(--v-structure-fill) 60%, transparent); padding: 22px 16px 16px; }
.v-boundary-region > .v-boundary-label { position: absolute; top: -9px; left: 14px; padding: 0 6px; background: var(--bg); }
.v-boundary-firewall { padding-top: 4px; }
.v-firewall-rule { border-top: 3px double var(--v-edge); border-bottom: 3px double var(--v-edge);
  height: 10px; margin: 0 0 14px; position: relative; }
.v-firewall-rule .v-boundary-label { position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%);
  padding: 0 8px; background: var(--bg); color: var(--v-edge); }
```

- [ ] **Step 4: Run the tests and commit**

```bash
cargo test -p skimasque-visual
rustfmt --edition 2021 crates/skimasque-visual/src/boundary.rs
cargo clippy -p skimasque-visual --all-targets -- -D warnings
git add crates/skimasque-visual
git commit -m "feat(visual): Boundary regions and firewall rules"
```

---

### Task 8: Gallery page and `sitegen` with `--check`

**Files:**
- Create: `src/site.rs`, `src/bin/sitegen.rs`, `templates/gallery.html`, `site/components/index.html` (generated), `site/components/visual.css` (generated)
- Modify: `src/lib.rs` (`pub mod site;`), `.gitattributes`, `.github/workflows/ci.yml`

**Interfaces:**
- Produces:

```rust
pub mod site {
    pub struct Page { pub path: &'static str, pub contents: String }
    pub fn pages() -> Vec<Page>;                    // [components/index.html, components/visual.css]
    pub fn stale(root: &std::path::Path) -> Vec<&'static str>;  // paths whose file differs (after \r\n→\n) or is missing
}
```

- CLI: `sitegen [--out <dir>] [--check]`. `--out` defaults to `site`. `--check` exits 1 and lists stale paths.

- [ ] **Step 1: Write the failing tests** (in `src/site.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_is_deterministic() {
        let a: Vec<_> = pages().into_iter().map(|p| (p.path, p.contents)).collect();
        let b: Vec<_> = pages().into_iter().map(|p| (p.path, p.contents)).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn the_gallery_shows_every_primitive_in_both_themes() {
        let page = pages().into_iter().find(|p| p.path == "components/index.html").unwrap().contents;
        assert!(page.contains(r#"<link rel="stylesheet" href="visual.css">"#), "stylesheet linked relatively");
        assert!(page.contains(r#"<svg class="v-sprite""#), "sprite emitted once");
        assert_eq!(page.matches(r#"<svg class="v-sprite""#).count(), 1);
        assert!(page.contains(r#"data-theme="dark""#) && page.contains(r#"data-theme="light""#));
        for kind in crate::NodeKind::ALL {
            assert!(page.contains(&format!(r#"data-kind="{}""#, kind.slug())), "{kind:?}");
        }
        for slug in ["normal", "control", "active", "potential", "denied"] {
            assert!(page.contains(&format!("v-conn-{slug}")), "{slug}");
        }
        for word in ["ALLOW", "DENY", "ACTIVE", "EXPIRED", "PENDING", "BLOCKED", "ACCESS GRANTED", "ACCESS DENIED"] {
            assert!(page.contains(word), "{word}");
        }
        assert!(page.contains("v-flow") && page.contains("v-boundary-region") && page.contains("v-boundary-firewall"));
        assert!(!page.contains("exec"), "honesty rule: no `skimasque exec`");
    }

    #[test]
    fn stale_ignores_crlf_and_reports_missing_or_changed_files() {
        let dir = std::env::temp_dir().join(format!("sitegen-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for p in pages() {
            let path = dir.join(p.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, p.contents.replace('\n', "\r\n")).unwrap();
        }
        assert!(stale(&dir).is_empty(), "CRLF checkout is not stale");
        std::fs::write(dir.join("components/visual.css"), "changed").unwrap();
        assert_eq!(stale(&dir), vec!["components/visual.css"]);
        std::fs::remove_file(dir.join("components/index.html")).unwrap();
        assert_eq!(stale(&dir).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p skimasque-visual --lib site::`
Expected: compile error.

- [ ] **Step 3: Implement `src/site.rs`**

```rust
//! The pages `sitegen` writes under the website's `site/` directory. For now
//! one page: the component gallery, every primitive in every state, shown in
//! a dark and a light section so both themes can be checked without JS.

use askama::Template;

use crate::{
    Boundary, Component, ConnKind, Connection, DecisionBadge, Flow, Html, Icons, Node, NodeKind,
    Status, StatusBadge,
};

pub struct Page { pub path: &'static str, pub contents: String }

struct Item { caption: String, html: Html }
struct Group { title: &'static str, items: Vec<Item> }

#[derive(Template)]
#[template(path = "gallery.html")]
struct Gallery { sprite: Html, groups: Vec<Group>, themes: [&'static str; 2] }

fn item(caption: impl Into<String>, c: &impl Component) -> Item { Item { caption: caption.into(), html: c.html() } }

fn groups() -> Vec<Group> {
    let nodes = NodeKind::ALL.iter().map(|k| item(k.slug(), &Node::new(*k))).collect();
    let conns = [ConnKind::Normal, ConnKind::Control, ConnKind::Active, ConnKind::Potential, ConnKind::Denied]
        .into_iter()
        .map(|k| item(k.slug(), &Connection::new(k)))
        .collect();
    let statuses = {
        let mut v: Vec<Item> = [Status::Allow, Status::Deny, Status::Active, Status::Expired, Status::Pending, Status::Blocked]
            .into_iter()
            .map(|s| item(s.word(), &StatusBadge { status: s }))
            .collect();
        v.push(item("granted", &DecisionBadge { allow: true }));
        v.push(item("denied", &DecisionBadge { allow: false }));
        v
    };
    let flow = Flow::new(
        "A GitHub Actions job proves its identity with OIDC, SkiMasque evaluates the policy, \
         and an active session carries its traffic through the gateway to the private database.",
    )
    .then(Node::new(NodeKind::GitHub).sub("acme/widget"))
    .via(Connection::new(ConnKind::Control).label("OIDC"), Node::new(NodeKind::Identity))
    .via(Connection::new(ConnKind::Control).label("policy"), Node::new(NodeKind::Policy).status(Status::Allow))
    .then(Node::new(NodeKind::Session).sub("20 min"))
    .via(Connection::new(ConnKind::Active), Node::new(NodeKind::Gateway).sub("us-west"))
    .via(Connection::new(ConnKind::Active), Node::new(NodeKind::Database).sub("db.prod:5432"));
    let denied = Flow::new("A request with no matching allow rule is denied at the policy.")
        .then(Node::new(NodeKind::CiJob).sub("curl"))
        .then(Node::new(NodeKind::Policy))
        .via(Connection::new(ConnKind::Denied), Node::new(NodeKind::Deny).label("No matching allow rule"));
    let region = Boundary::region("YOUR VPC")
        .child(&Node::new(NodeKind::Gateway))
        .child(&Node::new(NodeKind::Database).sub("db.prod:5432"))
        .child(&Node::new(NodeKind::Api).sub("api.internal:443"));
    let firewall = Boundary::firewall("YOUR FIREWALL").child(&Node::new(NodeKind::Network));
    vec![
        Group { title: "Nodes", items: nodes },
        Group { title: "Connections", items: conns },
        Group { title: "Statuses", items: statuses },
        Group { title: "Flows", items: vec![item("access", &flow), item("denied", &denied)] },
        Group { title: "Boundaries", items: vec![item("region", &region), item("firewall", &firewall)] },
    ]
}

pub fn pages() -> Vec<Page> {
    let gallery = Gallery { sprite: Icons.html(), groups: groups(), themes: ["dark", "light"] };
    vec![
        Page { path: "components/index.html", contents: gallery.render().expect("gallery renders") },
        Page { path: "components/visual.css", contents: crate::CSS.to_owned() },
    ]
}

/// Paths under `root` whose file is missing or differs from a fresh render.
/// Line endings are normalised so a CRLF checkout is not reported.
pub fn stale(root: &std::path::Path) -> Vec<&'static str> {
    pages()
        .into_iter()
        .filter(|p| match std::fs::read_to_string(root.join(p.path)) {
            Ok(on_disk) => on_disk.replace("\r\n", "\n") != p.contents.replace("\r\n", "\n"),
            Err(_) => true,
        })
        .map(|p| p.path)
        .collect()
}
```

The gallery groups are rendered twice, once per theme. Each group's items are the same `Html` values rendered into both sections. `Html` is `Clone` and the template iterates by reference, so no clone is needed.

`templates/gallery.html`:

```html
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Component gallery · SkiMasque</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600&display=swap">
<link rel="stylesheet" href="visual.css">
<style>
  body { margin: 0; background: var(--bg); color: var(--text); font: 14px/1.6 var(--font); }
  .g-theme { background: var(--bg); color: var(--text); padding: 32px 24px 48px; }
  .g-theme > h1 { font-size: 20px; margin: 0 0 4px; }
  .g-theme > p { color: var(--text-muted); margin: 0 0 24px; }
  .g-group > h2 { font-size: 11px; letter-spacing: .1em; text-transform: uppercase; color: var(--text-muted);
    border-bottom: 1px solid var(--line); padding-bottom: 6px; margin: 32px 0 16px; }
  .g-items { display: flex; flex-wrap: wrap; gap: 20px 28px; align-items: flex-start; }
  .g-item { display: grid; gap: 6px; min-width: 0; max-width: 100%; }
  .g-item > span { font-size: 11px; color: var(--text-muted); }
  @media (max-width: 719px) { .g-theme { padding: 24px 16px 40px; } }
</style>
</head>
<body>
{{ sprite|safe }}
<main>
{% for theme in themes %}
<section class="g-theme" data-theme="{{ theme }}" aria-label="{{ theme }} theme">
  <h1>SkiMasque components</h1>
  <p>{{ theme }} theme · every primitive in every state</p>
  {% for g in groups %}
  <section class="g-group">
    <h2>{{ g.title }}</h2>
    <div class="g-items">
      {% for i in g.items %}<div class="g-item"><span>{{ i.caption }}</span>{{ i.html|safe }}</div>{% endfor %}
    </div>
  </section>
  {% endfor %}
</section>
{% endfor %}
</main>
</body>
</html>
```

The gallery's own `<style>` uses only `var(--…)`, so `templates_hard_code_no_colours` still passes.

- [ ] **Step 4: Implement `src/bin/sitegen.rs`**

```rust
//! Render the website's generated pages into `site/` (or `--out <dir>`).
//! `--check` writes nothing and exits 1 if any committed page is stale.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut out = PathBuf::from("site");
    let mut check = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" => check = true,
            "--out" => match args.next() {
                Some(dir) => out = PathBuf::from(dir),
                None => {
                    eprintln!("sitegen: --out needs a directory");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("sitegen: unknown argument {other:?}\nusage: sitegen [--out <dir>] [--check]");
                return ExitCode::from(2);
            }
        }
    }

    if check {
        let stale = skimasque_visual::site::stale(&out);
        if stale.is_empty() {
            return ExitCode::SUCCESS;
        }
        for path in stale {
            eprintln!("stale: {}", out.join(path).display());
        }
        eprintln!("run `cargo run -p skimasque-visual --bin sitegen` and commit the result");
        return ExitCode::from(1);
    }

    for page in skimasque_visual::site::pages() {
        let path = out.join(page.path);
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("sitegen: creating {}: {e}", parent.display());
                return ExitCode::from(1);
            }
        }
        if let Err(e) = std::fs::write(&path, page.contents) {
            eprintln!("sitegen: writing {}: {e}", path.display());
            return ExitCode::from(1);
        }
        println!("wrote {}", path.display());
    }
    ExitCode::SUCCESS
}
```

- [ ] **Step 5: Generate, wire into CI and line endings**

```bash
cargo test -p skimasque-visual
cargo run -q -p skimasque-visual --bin sitegen
cargo run -q -p skimasque-visual --bin sitegen -- --check && echo fresh
```

Expected: tests pass, two `wrote` lines, then `fresh`.

`.gitattributes`: append

```
# Generated by `sitegen`; LF so `sitegen --check` compares cleanly everywhere.
site/components/** text eol=lf
```

`.github/workflows/ci.yml`: in the `test` job, after `cargo test --workspace`, add:

```yaml
      - name: Generated site pages are up to date
        run: cargo run -q -p skimasque-visual --bin sitegen -- --check
```

- [ ] **Step 6: Commit**

```bash
rustfmt --edition 2021 crates/skimasque-visual/src/site.rs crates/skimasque-visual/src/bin/sitegen.rs
cargo clippy -p skimasque-visual --all-targets -- -D warnings
git add crates/skimasque-visual site/components .gitattributes .github/workflows/ci.yml
git commit -m "feat(visual): component gallery and sitegen with a staleness check"
```

---

### Task 9: Visual verification of the gallery

**Files:** none unless defects are found (`static/visual.css`, the component templates, then regenerate `site/components/`).

- [ ] **Step 1: Serve the gallery**

Add a configuration to `C:\Users\Thor\Documents\src\skimasque-dev\.claude\launch.json`, creating the file if needed:

```json
{ "name": "visual-gallery", "runtimeExecutable": "python", "runtimeArgs": ["-m", "http.server", "8811", "--directory", "wt-visual/site"], "port": 8811 }
```

Start it with the browser pane's `preview_start` and open `http://localhost:8811/components/`.

- [ ] **Step 2: Check at 1440×900 and mobile 375×812**

- **Both theme sections render:** the light section has a snow background and dark text, and its nodes, lines and statuses read clearly (Review Focus 4).
- **Flows:** horizontal at 1440 and stacked vertically at 375, with arrows pointing the right way and no dangling connector (Review Focus 3).
- **No horizontal overflow:** `document.documentElement.scrollWidth <= innerWidth` at 375 (Review Focus 5).
- **Active glow:** the active connection glows. With `resize_window` / DevTools emulating `prefers-reduced-motion: reduce` (or checked via `matchMedia`), it doesn't animate: `getComputedStyle(el).animationName` is `none` under reduced motion. If the pane can't emulate reduced motion, confirm the rule exists in `visual.css` and say so.
- **Icons:** each node's icon renders from the sprite (none blank).
- **Contrast:** status words and light-theme mint text are legible.

- [ ] **Step 3: Fix, regenerate, commit**

For each defect, fix the CSS or template, run `cargo test -p skimasque-visual`, regenerate with `cargo run -q -p skimasque-visual --bin sitegen`, confirm `--check` passes, and commit `fix(visual): …`. Then stop the server and reset the viewport to desktop.

- [ ] **Step 4: Report**

Report a one-line result per check, plus screenshots of the gallery's dark and light sections at desktop and the flow at mobile width.
