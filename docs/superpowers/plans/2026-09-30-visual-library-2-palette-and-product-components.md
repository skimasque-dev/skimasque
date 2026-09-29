# Visual Library 2 — Palette v2 & Product Components Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebase the `skimasque-visual` tokens on the canonical palette (light-default, dark-capable, contrast-tested), then add the product components (policy, decision, audit, session, gateway, health, identity, empty state, code example, Planned marker) and show them all in the generated gallery.

**Architecture:** Same shape as plan 1: every component is a plain struct deriving `askama::Template` (templates under `templates/`), rendering to opaque `Html` via the open `Component` trait; components that contain other components hold them as data and embed them through private `*_html()` helpers. `static/visual.css` gains a new token block and role blocks, then one CSS layer per component group. `sitegen` regenerates `site/components/`.

**Tech Stack:** Rust 1.88, askama 0.12, plain CSS (container queries, `color-mix`), no JS.

**Spec:** `docs/superpowers/specs/2026-09-30-product-surface-amendment.md` (amends `2026-09-29-visual-library-design.md`). Plan 1 (`2026-09-29-visual-library-1-foundation.md`) is merged; this builds on its APIs.

## Global Constraints

- Work in `crates/skimasque-visual` (repo root = the worktree). Crate stays `publish = false`; feature `site` gates `pub mod site` and the `sitegen` bin.
- askama **0.12**. Components are structs; nested markup is only ever `Html` from another component (never a caller string). Every caller string is escaped by askama.
- **Hex colours only inside `/* tokens */ … /* end tokens */`** of `static/visual.css`. Templates and component CSS use role tokens only. Use literal glyphs (`✓ × ● + − ~ ↓ ◇`) in templates, never numeric entities (the hex scanner flags `&#x…;`-like text).
- Canonical colours: Snow `#F4F3ED`, Ice `#E5F2EE`, Mint `#72C7A5`, Pine `#183C35`, Forest `#28584C`, Earth `#795C43`, Slate `#66736F`; deny is a restrained red. Mint and Slate fail 4.5:1 as small text on Snow: they are fills/borders/lines only; text uses the derived `-ink` steps.
- **≥4.5:1** contrast for every text/background role pair in both themes, enforced by a test.
- Themes: `:root` (and `[data-theme="light"]`) = light (website default); `[data-theme="dark"]` = dark (control-plane default, set by the app on `<html>`); `[data-theme="auto"]` follows `prefers-color-scheme`.
- Every status shows its **word** beside the dot/glyph. Customer wording is "Access granted / Access denied"; ALLOW/DENY stay available for policy-editor contexts.
- Anything the product does not do today is rendered with the `Planned` marker, never in the present tense. The generated gallery must never contain the substring `exec`.
- Motion is disabled under `prefers-reduced-motion`.
- Formatting: never run workspace `cargo fmt`. Only `rustfmt --edition 2021 <file>` on files you create or edit.
- Commits: conventional style, **no AI attribution trailers or footers**.
- Verify commands (repo root): `cargo test -p skimasque-visual --features site`, `cargo clippy -p skimasque-visual --all-targets --features site -- -D warnings`, `cargo run -q -p skimasque-visual --features site --bin sitegen -- --check` (regenerate with the same command minus `-- --check` and commit `site/components/`).

## Review Focus

- A long or hostile string (`<script>`, quotes, 200 chars without spaces) in any field: escaped, no horizontal page overflow at 320px (Tasks 4–7 tests + gallery check).
- Empty collections (a policy with no limits, an explainer with no checks, a timeline with no events, a diff with no changes): render a sensible empty state, never a broken list (Tasks 5–7).
- `SessionCard` with `remaining_pct` > 100 or 0: bar clamps to 0–100, label still shown (Task 7).
- Dark and light both legible: every new colour use goes through a role token that the contrast test covers (Task 1 + each CSS layer).
- Unbuilt features (`egress_ip: None`, disabling a policy) show Planned, not blank or fake data (Tasks 4, 7).

## File Structure

| file | responsibility |
|---|---|
| `static/visual.css` | new token block, 3 role blocks, existing layers adjusted, one new layer per component group |
| `tests/css.rs` | hex rule, canonical palette, role-block parity, contrast |
| `src/status.rs`, `src/node.rs` | `Status::{Granted,Denied,Healthy,Degraded,Offline}`, `Tone::Warning`, `DecisionBadge` wording |
| `src/content.rs` + `templates/{planned,empty_state,code_example}.html` | `Planned`, `EmptyState`, `CodeExample` |
| `src/policy.rs` + `templates/{policy_summary,policy_card,policy_explorer,policy_diff}.html` | `PolicySummary`, `PolicyCard`, `PolicyExplorer`, `PolicyDiff` |
| `src/decision.rs` + `templates/{decision_explainer,decision_card,audit_event_card}.html` | `DecisionExplainer`, `DecisionCard`, `AuditEventCard` |
| `src/access.rs` + `templates/{session_card,session_timeline,gateway_card,health_card,identity_card}.html` | `SessionCard`, `SessionTimeline`, `GatewayCard`, `HealthCard`, `IdentityCard` |
| `src/site.rs`, `templates/gallery.html` | gallery groups for everything, light section first |
| `site/components/` | regenerated output (committed) |

---

### Task 1: Palette v2 tokens, theme roles, contrast test

**Files:**
- Modify: `static/visual.css` (token block, role blocks, `.v-flow-wrap`)
- Modify: `tests/css.rs`
- Modify: `src/site.rs` (theme order; nothing else yet), `src/status.rs` untouched

**Interfaces:**
- Produces: tokens `--snow --ice --white --mint --mint-300 --mint-ink --pine --pine-900 --pine-950 --forest --forest-300 --forest-fill --earth --earth-ink --earth-300 --earth-fill --sand --slate --slate-ink --slate-300 --deny --deny-300 --warning-ink --warning-300 --info-ink --info-300`; the role set listed in Step 3 (unchanged names from plan 1 plus `--v-warning`), declared identically in light, dark and auto blocks. Later tasks use roles only.

- [ ] **Step 1: Write the failing tests** — replace `tests/css.rs` `the_palette_matches_the_design_guide` and append parity + contrast tests:

```rust
#[test]
fn the_palette_matches_the_canonical_spec() {
    for decl in [
        "--snow: #F4F3ED",
        "--ice: #E5F2EE",
        "--mint: #72C7A5",
        "--pine: #183C35",
        "--forest: #28584C",
        "--earth: #795C43",
        "--slate: #66736F",
    ] {
        assert!(skimasque_visual::CSS.contains(decl), "missing {decl}");
    }
}

struct Rule {
    selector: String,
    decls: Vec<(String, String)>,
}

/// Every rule that declares custom properties (`--x: y`), comments removed.
fn rules(css: &str) -> Vec<Rule> {
    let mut clean = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        clean.push_str(&rest[..i]);
        let end = rest[i..].find("*/").expect("closed comment");
        rest = &rest[i + end + 2..];
    }
    clean.push_str(rest);
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut buf = String::new();
    for ch in clean.chars() {
        match ch {
            '{' => {
                stack.push(buf.trim().to_owned());
                buf.clear();
            }
            '}' => {
                let selector = stack.pop().expect("balanced braces");
                let decls: Vec<(String, String)> = buf
                    .split(';')
                    .filter_map(|d| {
                        let (k, v) = d.split_once(':')?;
                        let k = k.trim();
                        k.starts_with("--").then(|| (k.to_owned(), v.trim().to_owned()))
                    })
                    .collect();
                if !decls.is_empty() {
                    out.push(Rule { selector, decls });
                }
                buf.clear();
            }
            c => buf.push(c),
        }
    }
    out
}

fn rule<'a>(rules: &'a [Rule], selector: &str) -> &'a Rule {
    rules
        .iter()
        .find(|r| r.selector == selector)
        .unwrap_or_else(|| panic!("no rule `{selector}`"))
}

const LIGHT: &str = r#":root, [data-theme="light"]"#;
const DARK: &str = r#"[data-theme="dark"]"#;
const AUTO: &str = r#":root[data-theme="auto"]"#;

#[test]
fn the_three_theme_blocks_declare_the_same_roles() {
    let rules = rules(skimasque_visual::CSS);
    let names = |sel: &str| {
        let mut v: Vec<&str> = rule(&rules, sel).decls.iter().map(|(k, _)| k.as_str()).collect();
        v.sort_unstable();
        v
    };
    assert_eq!(names(LIGHT), names(DARK), "light vs dark");
    assert_eq!(names(DARK), names(AUTO), "dark vs auto");
    let auto = &rule(&rules, AUTO).decls;
    let dark = &rule(&rules, DARK).decls;
    assert_eq!(auto, dark, "auto must repeat the dark values exactly");
}

fn resolve(name: &str, theme: &Rule, tokens: &Rule) -> Option<String> {
    let value = theme
        .decls
        .iter()
        .chain(tokens.decls.iter())
        .find(|(k, _)| k == name)?
        .1
        .clone();
    match value.strip_prefix("var(").and_then(|v| v.strip_suffix(')')) {
        Some(inner) => resolve(inner.trim(), theme, tokens),
        None => Some(value),
    }
}

fn luminance(hex: &str) -> f64 {
    let hex = hex.trim_start_matches('#');
    let ch = |i: usize| {
        let c = u8::from_str_radix(&hex[i..i + 2], 16).unwrap() as f64 / 255.0;
        if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(0) + 0.7152 * ch(2) + 0.0722 * ch(4)
}

fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[test]
fn every_text_role_meets_4_5_to_1_on_every_surface_in_both_themes() {
    let rules = rules(skimasque_visual::CSS);
    let tokens = rule(&rules, ":root");
    let texts = [
        "--text", "--text-soft", "--text-muted", "--accent-text", "--success", "--warning",
        "--danger", "--info", "--v-active", "--v-deny", "--v-info", "--v-warning",
        "--v-structure-text", "--v-edge-text", "--v-neutral-text",
    ];
    let grounds = [
        "--bg", "--surface", "--surface-raised", "--input", "--v-structure-fill", "--v-edge-fill",
    ];
    for (theme_name, sel) in [("light", LIGHT), ("dark", DARK)] {
        let theme = rule(&rules, sel);
        for t in texts {
            for g in grounds {
                let fg = resolve(t, theme, tokens).unwrap_or_else(|| panic!("{t} unresolved"));
                let bg = resolve(g, theme, tokens).unwrap_or_else(|| panic!("{g} unresolved"));
                let ratio = contrast(&fg, &bg);
                assert!(ratio >= 4.5, "{theme_name}: {t} {fg} on {g} {bg} = {ratio:.2}");
            }
        }
        let (fg, bg) = (
            resolve("--on-accent", theme, tokens).unwrap(),
            resolve("--accent", theme, tokens).unwrap(),
        );
        assert!(contrast(&fg, &bg) >= 4.5, "{theme_name}: on-accent on accent");
    }
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p skimasque-visual --features site --test css` → FAIL (palette declarations missing / `no rule ...`).

- [ ] **Step 3: Replace the token block and role blocks in `static/visual.css`** — replace everything from `/* tokens */` through the end of the `[data-theme="light"] { … }` block (lines 5–75) with:

```css
/* tokens */
:root {
  --snow: #F4F3ED; --ice: #E5F2EE; --white: #FFFFFF; --sand: #ECE5DA;
  --mint: #72C7A5; --mint-300: #9FDCC2; --mint-ink: #1D7156;
  --pine: #183C35; --pine-900: #11241F; --pine-950: #0C1A17;
  --forest: #28584C; --forest-300: #8CC2AE; --forest-fill: #14302A;
  --earth: #795C43; --earth-ink: #6D5139; --earth-300: #D2B391; --earth-fill: #2A2118;
  --slate: #66736F; --slate-ink: #56625E; --slate-300: #A9B5B1;
  --deny: #B03A2E; --deny-300: #F0958A;
  --warning-ink: #7D5A00; --warning-300: #E8C874;
  --info-ink: #1D6394; --info-300: #8CC4EA;
  --font: "IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  --space-1: 4px; --space-2: 8px; --space-3: 12px; --space-4: 16px; --space-5: 20px; --space-6: 24px;
  --space-8: 32px; --space-10: 40px; --space-12: 48px; --space-16: 64px;
  --radius-sm: 4px; --radius-md: 8px; --radius-lg: 12px; --radius-xl: 16px;
}
/* end tokens */

/* roles. Light is the default (the website). The control plane sets data-theme="dark" on <html>;
   data-theme="auto" follows the OS. The same role set is declared three times (light, dark, auto):
   tests/css.rs fails if the sets drift apart or a text role drops under 4.5:1. */
:root, [data-theme="light"] {
  color-scheme: light;
  --bg: var(--snow); --surface: var(--white); --surface-raised: var(--ice); --input: var(--white);
  --text: var(--pine); --text-soft: var(--forest); --text-muted: var(--slate-ink);
  --line: rgba(24,60,53,.10); --line-default: rgba(24,60,53,.16); --line-strong: rgba(24,60,53,.28);
  --hover: rgba(114,199,165,.14);
  --accent: var(--mint); --accent-hover: var(--mint-300); --accent-text: var(--mint-ink);
  --accent-soft: rgba(114,199,165,.20); --on-accent: var(--pine); --focus: var(--mint-ink);
  --success: var(--mint-ink); --warning: var(--warning-ink); --danger: var(--deny); --info: var(--info-ink);
  --shadow: 0 12px 32px rgba(24,60,53,.14);
  --v-active: var(--mint-ink); --v-structure: var(--forest); --v-structure-fill: var(--ice);
  --v-edge: var(--earth); --v-edge-fill: var(--sand);
  --v-neutral: var(--slate); --v-deny: var(--deny); --v-info: var(--info-ink); --v-warning: var(--warning-ink);
  --v-structure-text: var(--forest); --v-edge-text: var(--earth-ink); --v-neutral-text: var(--slate-ink);
}
[data-theme="dark"] {
  color-scheme: dark;
  --bg: var(--pine-950); --surface: var(--pine-900); --surface-raised: var(--pine); --input: var(--forest-fill);
  --text: var(--snow); --text-soft: var(--ice); --text-muted: var(--slate-300);
  --line: rgba(244,243,237,.08); --line-default: rgba(244,243,237,.14); --line-strong: rgba(244,243,237,.22);
  --hover: rgba(114,199,165,.08);
  --accent: var(--mint); --accent-hover: var(--mint-300); --accent-text: var(--mint);
  --accent-soft: rgba(114,199,165,.14); --on-accent: var(--pine); --focus: var(--mint);
  --success: var(--mint); --warning: var(--warning-300); --danger: var(--deny-300); --info: var(--info-300);
  --shadow: 0 12px 32px rgba(0,0,0,.45);
  --v-active: var(--mint); --v-structure: var(--forest); --v-structure-fill: var(--forest-fill);
  --v-edge: var(--earth); --v-edge-fill: var(--earth-fill);
  --v-neutral: var(--slate); --v-deny: var(--deny-300); --v-info: var(--info-300); --v-warning: var(--warning-300);
  --v-structure-text: var(--forest-300); --v-edge-text: var(--earth-300); --v-neutral-text: var(--slate-300);
}
@media (prefers-color-scheme: dark) {
  :root[data-theme="auto"] {
    color-scheme: dark;
    --bg: var(--pine-950); --surface: var(--pine-900); --surface-raised: var(--pine); --input: var(--forest-fill);
    --text: var(--snow); --text-soft: var(--ice); --text-muted: var(--slate-300);
    --line: rgba(244,243,237,.08); --line-default: rgba(244,243,237,.14); --line-strong: rgba(244,243,237,.22);
    --hover: rgba(114,199,165,.08);
    --accent: var(--mint); --accent-hover: var(--mint-300); --accent-text: var(--mint);
    --accent-soft: rgba(114,199,165,.14); --on-accent: var(--pine); --focus: var(--mint);
    --success: var(--mint); --warning: var(--warning-300); --danger: var(--deny-300); --info: var(--info-300);
    --shadow: 0 12px 32px rgba(0,0,0,.45);
    --v-active: var(--mint); --v-structure: var(--forest); --v-structure-fill: var(--forest-fill);
    --v-edge: var(--earth); --v-edge-fill: var(--earth-fill);
    --v-neutral: var(--slate); --v-deny: var(--deny-300); --v-info: var(--info-300); --v-warning: var(--warning-300);
    --v-structure-text: var(--forest-300); --v-edge-text: var(--earth-300); --v-neutral-text: var(--slate-300);
  }
}
```

  Note: `color-scheme` is a normal property, not a `--` custom property, so the parity test ignores it. In the same file: add `.v-tone-warning { --v-tone: var(--v-warning); }` after `.v-tone-info`, change the shared text line to `.v-tone-active, .v-tone-deny, .v-tone-info, .v-tone-warning { --v-tone-text: var(--v-tone); }`, and change `.v-flow-wrap { margin: 0; container-type: inline-size; }` to `.v-flow-wrap { margin: 0; width: 100%; min-width: 0; container-type: inline-size; }`.

- [ ] **Step 4: Add `Tone::Warning`** in `src/node.rs`: enum variant `Warning`, `class()` arm `Tone::Warning => "warning"`. Add to the `kinds_map_to_the_visual_grammar` test nothing. (`Status` mapping comes in Task 2.)

- [ ] **Step 5: Gallery shows light first** — in `src/site.rs` change `themes: ["dark", "light"]` to `themes: ["light", "dark"]`. `gallery.html` needs no change (`data-theme` sections already exist; light is now also the default).

- [ ] **Step 6: Parked minors** — (a) in `src/icons.rs` tests add:

```rust
    #[test]
    fn icon_ids_are_unique() {
        let sprite = Icons.html().as_str().to_owned();
        let mut ids: Vec<&str> = sprite.split(r#"<symbol id=""#).skip(1).filter_map(|s| s.split('"').next()).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate <symbol id>");
    }
```
  (b) in `tests/css.rs` make `templates_hard_code_no_colours` recurse into sub-directories (replace `read_dir` loop with a helper `fn files(dir) -> Vec<PathBuf>` that recurses). (c) in `visual.css` add one comment line above `.v-conn-label`: `/* label text takes the connection's colour via currentColor on .v-conn-*; --v-active/--v-deny are text-safe */`.

- [ ] **Step 7: Run everything** — `cargo test -p skimasque-visual --features site` → PASS. If a contrast assertion names a pair below 4.5, adjust **only** that derived token in the block (the values above were pre-checked; the light `--v-edge-text` on `--sand` is the tightest at 5.82). Then `rustfmt --edition 2021 tests/css.rs src/node.rs src/icons.rs src/site.rs`.

- [ ] **Step 8: Regenerate and commit**

```bash
cargo clippy -p skimasque-visual --all-targets --features site -- -D warnings
cargo run -q -p skimasque-visual --features site --bin sitegen
git add crates/skimasque-visual site/components
git commit -m "feat(visual): canonical palette, light/dark/auto roles, contrast test"
```

---

### Task 2: Status wording and DecisionBadge variants

**Files:**
- Modify: `src/status.rs`, `templates/status.html` (unchanged), `templates/decision_badge.html`, `src/site.rs` (literals), `static/visual.css` (nothing)

**Interfaces:**
- Produces: `Status::{Granted, Denied, Healthy, Degraded, Offline}` with words `ACCESS GRANTED`, `ACCESS DENIED`, `HEALTHY`, `DEGRADED`, `OFFLINE`; tones Active, Deny, Active, Warning, Neutral. `DecisionBadge { allow: bool, technical: bool }`, `DecisionBadge::new(allow)` (customer wording) and `.technical()` (ALLOW/DENY wording). `Status::Allow`/`Deny` remain.

- [ ] **Step 1: Failing tests** — in `src/status.rs` `tests`, extend the table in `every_status_carries_its_word_beside_the_dot` with
  `(Status::Granted, "ACCESS GRANTED", "active")`, `(Status::Denied, "ACCESS DENIED", "deny")`, `(Status::Healthy, "HEALTHY", "active")`, `(Status::Degraded, "DEGRADED", "warning")`, `(Status::Offline, "OFFLINE", "neutral")`, and replace the decision test with:

```rust
    #[test]
    fn decision_badges_use_customer_wording_unless_technical() {
        let g = DecisionBadge::new(true).html();
        assert!(g.as_str().contains("✓") && g.as_str().contains("ACCESS GRANTED"));
        let d = DecisionBadge::new(false).html();
        assert!(d.as_str().contains("×") && d.as_str().contains("ACCESS DENIED"));
        assert!(d.as_str().contains("v-tone-deny"));
        let a = DecisionBadge::new(true).technical().html();
        assert!(a.as_str().contains("ALLOW") && !a.as_str().contains("ACCESS"));
        let n = DecisionBadge::new(false).technical().html();
        assert!(n.as_str().contains("DENY") && !n.as_str().contains("ACCESS"));
    }
```

- [ ] **Step 2:** `cargo test -p skimasque-visual --features site status` → FAIL (compile).

- [ ] **Step 3: Implement.** In `status.rs` add the five variants to `Status`, the words above to `word()`, and to `tone()`: `Status::Allow | Status::Active | Status::Granted | Status::Healthy => Tone::Active`, `Status::Deny | Status::Blocked | Status::Denied => Tone::Deny`, `Status::Expired | Status::Offline => Tone::Neutral`, `Status::Pending => Tone::Info`, `Status::Degraded => Tone::Warning`. Replace `DecisionBadge`:

```rust
#[derive(Template, Debug, Clone)]
#[template(path = "decision_badge.html")]
pub struct DecisionBadge {
    pub allow: bool,
    /// Policy-editor wording (ALLOW / DENY) instead of customer wording.
    pub technical: bool,
}

impl DecisionBadge {
    pub fn new(allow: bool) -> Self {
        Self { allow, technical: false }
    }
    pub fn technical(mut self) -> Self {
        self.technical = true;
        self
    }
    fn word(&self) -> &'static str {
        match (self.allow, self.technical) {
            (true, false) => "ACCESS GRANTED",
            (false, false) => "ACCESS DENIED",
            (true, true) => "ALLOW",
            (false, true) => "DENY",
        }
    }
}
impl crate::Component for DecisionBadge {}
```

  `templates/decision_badge.html` (one line, no trailing newline): `{% if allow %}<span class="v-decision v-tone-active"><span aria-hidden="true">✓</span> {{ self.word() }}</span>{% else %}<span class="v-decision v-tone-deny"><span aria-hidden="true">×</span> {{ self.word() }}</span>{% endif %}`.

  In `src/site.rs` replace the two `DecisionBadge { allow: … }` literals with `DecisionBadge::new(true)` / `DecisionBadge::new(false)`, and add two more items `item("technical allow", &DecisionBadge::new(true).technical())`, `item("technical deny", &DecisionBadge::new(false).technical())`; add the five new statuses to the `statuses` array. Extend the gallery test's word list with `"ACCESS GRANTED"` (already), `"HEALTHY"`, `"DEGRADED"`, `"OFFLINE"`.

- [ ] **Step 4:** `cargo test -p skimasque-visual --features site` → PASS; rustfmt the edited files; regenerate site; commit `feat(visual): access granted/denied statuses, technical decision wording, health statuses`.

---

### Task 3: Flow threshold review

**Files:** Modify `static/visual.css` (the `@container (min-width: …)` value, comment). Regenerate `site/components`.

- [ ] **Step 1: Build the gallery** — `cargo run -q -p skimasque-visual --features site --bin sitegen` then serve `site/components/` (launch entry `visual-gallery` in `.claude/launch.json`, or `python -m http.server` from `site/components`).
- [ ] **Step 2: Measure** — for viewport widths 720, 800, 900, 980, 1100, 1300 (`resize_window`), run in the page: 

```js
[...document.querySelectorAll('.v-flow-wrap')].map(w => ({ w: w.clientWidth, over: w.scrollWidth > w.clientWidth, page: document.documentElement.scrollWidth > innerWidth }))
```
  The widest flow (7 nodes, "access") must not overflow (`over: false`, `page: false`) at any width, in both themes. If it overflows once horizontal, raise the `@container (min-width: 980px)` threshold to the smallest of {980, 1040, 1100, 1160, 1240} that fixes it; if 980 has headroom of more than 120px (`scrollWidth` well under `clientWidth`) lower it in steps of 60 while still overflow-free. Update the CSS comment above the container query to state the measured width and the widest flow used, e.g. `/* 7-node "access" flow needs ≈NNNpx; threshold measured in plan 2 */`.
- [ ] **Step 3: Commit** — `fix(visual): flow container threshold measured against the widest gallery flow`, with regenerated `site/components`. (If 980 is already right, commit only the comment.)

---

### Task 4: Content components — Planned, EmptyState, CodeExample

**Files:**
- Create: `src/content.rs`, `templates/planned.html`, `templates/empty_state.html`, `templates/code_example.html`
- Modify: `src/lib.rs` (`pub mod content;`, re-export), `static/visual.css` (append layer), `src/site.rs` (group "Content")

**Interfaces:**
- Produces: `Planned::new() -> Planned`, `.note(impl Into<String>) -> Planned`; `EmptyState::new(title, lines: &[&str])`, `.action(label, href)`, `EmptyState::no_policies(create_href)`, `::no_sessions()`, `::no_gateways(add_href)`; `CodeExample::new(label, code)`. All `Component`. Re-exported at crate root: `Planned, EmptyState, CodeExample`.

- [ ] **Step 1: Failing tests** (bottom of new `src/content.rs`; create the file with the module doc and tests only, plus `pub mod content;` in lib.rs so it compiles to a failure):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    #[test]
    fn planned_says_planned_and_escapes_its_note() {
        let plain = Planned::new().html();
        assert!(plain.as_str().contains(r#"class="v-planned""#) && plain.as_str().contains("PLANNED"));
        let noted = Planned::new().note("<script>x</script>").html();
        assert!(noted.as_str().contains("&lt;script&gt;") && !noted.as_str().contains("<script>"));
    }

    #[test]
    fn canonical_empty_states_use_the_spec_copy() {
        let p = EmptyState::no_policies("/app/policies/new").html();
        for s in ["No policies yet.", "Create your first policy to give a workload", "temporary access to private infrastructure.", "Create Policy", r#"href="/app/policies/new""#] {
            assert!(p.as_str().contains(s), "{s}");
        }
        let s = EmptyState::no_sessions().html();
        assert!(s.as_str().contains("No active sessions.") && s.as_str().contains("will appear here."));
        assert!(!s.as_str().contains("<a "), "sessions have no action");
        let g = EmptyState::no_gateways("/app/gateways/new").html();
        assert!(g.as_str().contains("No gateways connected.") && g.as_str().contains("Add Gateway"));
    }

    #[test]
    fn empty_state_escapes_and_has_no_action_by_default() {
        let e = EmptyState::new("<b>t</b>", &["a & b"]).html();
        assert!(e.as_str().contains("&lt;b&gt;t&lt;/b&gt;") && e.as_str().contains("a &amp; b"));
    }

    #[test]
    fn code_example_marks_prompts_comments_and_copies_only_commands() {
        let c = CodeExample::new("connect", "# open a session\n$ skimasque connect db.prod:5432\nlistening on 127.0.0.1:5432").html();
        let s = c.as_str();
        assert!(s.contains(r#"<span class="v-code-prompt""#) && s.contains("v-code-comment") && s.contains("v-code-out"));
        assert!(s.contains(r#"data-copy="skimasque connect db.prod:5432""#));
        let h = CodeExample::new("x", "$ echo \"<script>\"").html();
        assert!(!h.as_str().contains("<script>"));
        assert!(h.as_str().contains("&lt;script&gt;"));
    }
}
```

- [ ] **Step 2:** `cargo test -p skimasque-visual --features site content` → FAIL.

- [ ] **Step 3: Implement `src/content.rs`** (above the tests):

```rust
//! Small content components: the Planned marker, empty states, code examples.

use askama::Template;

/// Marks anything the product does not do yet. Never shown as a working control.
#[derive(Template, Debug, Clone, Default)]
#[template(path = "planned.html")]
pub struct Planned {
    pub note: Option<String>,
}
impl Planned {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}
impl crate::Component for Planned {}

#[derive(Template, Debug, Clone)]
#[template(path = "empty_state.html")]
pub struct EmptyState {
    pub title: String,
    pub lines: Vec<String>,
    pub action: Option<(String, String)>,
}
impl EmptyState {
    pub fn new(title: impl Into<String>, lines: &[&str]) -> Self {
        Self {
            title: title.into(),
            lines: lines.iter().map(|l| (*l).to_owned()).collect(),
            action: None,
        }
    }
    pub fn action(mut self, label: impl Into<String>, href: impl Into<String>) -> Self {
        self.action = Some((label.into(), href.into()));
        self
    }
    pub fn no_policies(create_href: impl Into<String>) -> Self {
        Self::new(
            "No policies yet.",
            &[
                "Create your first policy to give a workload",
                "temporary access to private infrastructure.",
            ],
        )
        .action("Create Policy", create_href)
    }
    pub fn no_sessions() -> Self {
        Self::new(
            "No active sessions.",
            &[
                "When a workload or developer receives access,",
                "its active session will appear here.",
            ],
        )
    }
    pub fn no_gateways(add_href: impl Into<String>) -> Self {
        Self::new(
            "No gateways connected.",
            &[
                "Add a gateway to provide a network path",
                "to your infrastructure.",
            ],
        )
        .action("Add Gateway", add_href)
    }
}
impl crate::Component for EmptyState {}

struct CodeRow {
    class: &'static str,
    prompt: bool,
    text: String,
}

/// A code block: `$ ` lines are commands (mint prompt), `# ` lines comments,
/// everything else output. `data-copy` carries the commands for the host's copy button.
#[derive(Template, Debug, Clone)]
#[template(path = "code_example.html")]
pub struct CodeExample {
    pub label: String,
    pub code: String,
}
impl CodeExample {
    pub fn new(label: impl Into<String>, code: impl Into<String>) -> Self {
        Self { label: label.into(), code: code.into() }
    }
    fn rows(&self) -> Vec<CodeRow> {
        self.code
            .lines()
            .map(|l| match (l.strip_prefix("$ "), l.starts_with("# ")) {
                (Some(cmd), _) => CodeRow { class: "v-code-cmd", prompt: true, text: cmd.to_owned() },
                (None, true) => CodeRow { class: "v-code-comment", prompt: false, text: l.to_owned() },
                _ => CodeRow { class: "v-code-out", prompt: false, text: l.to_owned() },
            })
            .collect()
    }
    fn copy_text(&self) -> String {
        self.code
            .lines()
            .filter_map(|l| l.strip_prefix("$ "))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
impl crate::Component for CodeExample {}
```

  Templates (no colours):
  - `planned.html`: `<span class="v-planned"><span aria-hidden="true">◇</span> PLANNED{% if let Some(n) = note %}<span class="v-sr"> — {{ n }}</span>{% endif %}</span>`
  - `empty_state.html`: `<div class="v-empty"><h3 class="v-empty-title">{{ title }}</h3><p class="v-empty-body">{% for l in lines %}<span>{{ l }}</span>{% endfor %}</p>{% if let Some((label, href)) = action %}<a class="v-btn" href="{{ href }}">{{ label }}</a>{% endif %}</div>`
  - `code_example.html`: `<figure class="v-code" data-copy="{{ self.copy_text() }}"><figcaption class="v-code-label">{{ label }}</figcaption><pre class="v-code-body">{% for r in self.rows() %}<span class="{{ r.class }}">{% if r.prompt %}<span class="v-code-prompt" aria-hidden="true">$ </span>{% endif %}{{ r.text }}</span>
{% endfor %}</pre></figure>` — the literal newline after `</span>` inside the loop is intentional (the `<pre>` keeps it). Update the test's prompt assertion to `<span class="v-code-prompt"` (already written that way).

  In `lib.rs`: `pub mod content;` and `pub use content::{CodeExample, EmptyState, Planned};`.

- [ ] **Step 4: CSS layer** — append to `visual.css` (roles only; `.v-btn` is the library's one button):

```css
/* content */
.v-planned { display: inline-flex; align-items: center; gap: 5px; padding: 1px 8px; border: 1px dashed var(--v-neutral);
  border-radius: var(--radius-sm); font: 600 10px/1.6 var(--font); letter-spacing: .1em; color: var(--v-neutral-text); white-space: nowrap; }
.v-btn { display: inline-block; padding: 8px 16px; border-radius: var(--radius-md); background: var(--accent); color: var(--on-accent);
  font: 600 13px/1.4 var(--font); text-decoration: none; }
.v-btn:hover { background: var(--accent-hover); }
.v-btn:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.v-empty { display: grid; justify-items: start; gap: 8px; padding: 24px; border: 1px dashed var(--line-strong);
  border-radius: var(--radius-lg); background: var(--surface); color: var(--text); font: 13px/1.6 var(--font); max-width: 100%; box-sizing: border-box; }
.v-empty-title { margin: 0; font-size: 14px; font-weight: 600; overflow-wrap: anywhere; }
.v-empty-body { margin: 0; color: var(--text-muted); overflow-wrap: anywhere; }
.v-empty-body span { display: block; }
.v-code { margin: 0; border-radius: var(--radius-lg); background: var(--pine-950); color: var(--snow); font: 13px/1.6 var(--font); overflow: hidden; max-width: 100%; }
.v-code-label { padding: 8px 16px; color: var(--slate-300); font-size: 11px; letter-spacing: .08em; border-bottom: 1px solid var(--line-default); }
.v-code-body { margin: 0; padding: 12px 16px; overflow-x: auto; white-space: pre; }
.v-code-prompt { color: var(--mint); }
.v-code-cmd { color: var(--snow); }
.v-code-comment { color: var(--slate-300); }
.v-code-out { color: var(--ice); }
```

  Careful: this layer references `--pine-950`, `--snow`, `--slate-300`, `--mint`, `--ice` directly. That is allowed for `.v-code` only because CodeExample is a fixed Pine surface in both themes (spec: "Pine background, mint highlights"); the other layers use roles. Add a comment line `/* code blocks are Pine in both themes, so they use palette tokens, not roles */` above `.v-code`. Palette contrast on those pairs (all ≥ 5.7) is asserted by adding to `tests/css.rs`:

```rust
#[test]
fn code_block_palette_pairs_meet_4_5_to_1() {
    let rules = rules(skimasque_visual::CSS);
    let t = rule(&rules, ":root");
    let get = |n: &str| t.decls.iter().find(|(k, _)| k == n).unwrap().1.clone();
    for fg in ["--snow", "--ice", "--slate-300", "--mint"] {
        assert!(contrast(&get(fg), &get("--pine-950")) >= 4.5, "{fg} on code bg");
    }
}
```

- [ ] **Step 5: Gallery** — in `site.rs` add a group `Content` (`wide: false`) with items: `planned` → `Planned::new()`, `planned with note` → `Planned::new().note("shown, not available")`, `empty · policies` → `EmptyState::no_policies("#")`, `empty · sessions`, `empty · gateways`, `code` → `CodeExample::new("connect to a private database", "# open a session\n$ skimasque connect db.prod:5432\nlistening on 127.0.0.1:5432")`. Add to the gallery test: `for s in ["PLANNED", "No policies yet.", "v-code-prompt"] { assert!(page.contains(s)) }`. The word `exec` must not appear (check the strings above: none do).

- [ ] **Step 6:** run the full test + clippy commands, rustfmt the new/edited `.rs`, regenerate, commit `feat(visual): Planned marker, empty states, code example`.

---

### Task 5: Policy components — Summary, Card, Explorer, Diff

**Files:**
- Create: `src/policy.rs`, `templates/{policy_summary,policy_card,policy_explorer,policy_diff}.html`
- Modify: `src/lib.rs`, `static/visual.css`, `src/site.rs`

**Interfaces:**
- Consumes: `Status`, `StatusBadge`, `DecisionBadge`, `Component`, `Html`.
- Produces:
  - `PolicySummary { who, what, target, limits: String }` (`PolicySummary::new(who, what, target, limits)`), 
  - `PolicyCard { name: String, status: Status, summary: PolicySummary, meta: Option<String>, href: Option<String> }` (`PolicyCard::new(name, status, summary)`, `.meta(s)`, `.href(s)`),
  - `PolicyExplorer { who, what, target, limits: Vec<String>, allow: bool, reason: String }` (`PolicyExplorer::new(who: &[&str], what: &[&str], target: &[&str], limits: &[&str], allow, reason)`),
  - `ChangeKind::{Added, Removed, Changed}`, `Change { kind, field: String, before: Option<String>, after: Option<String> }` with `Change::added(field, after)`, `::removed(field, before)`, `::changed(field, before, after)`, and `PolicyDiff { changes: Vec<Change> }` (`PolicyDiff::new(changes)`).

- [ ] **Step 1: Failing tests** (in `src/policy.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Status};

    fn summary() -> PolicySummary {
        PolicySummary::new("acme/widget", "terraform", "db.prod:5432", "20 min")
    }

    #[test]
    fn summary_lists_who_what_where_limits_in_order() {
        let h = summary().html();
        let s = h.as_str();
        let pos = |w: &str| s.find(w).unwrap_or_else(|| panic!("{w}"));
        assert!(pos("WHO") < pos("WHAT") && pos("WHAT") < pos("WHERE") && pos("WHERE") < pos("LIMITS"));
        for v in ["acme/widget", "terraform", "db.prod:5432", "20 min"] {
            assert!(s.contains(v), "{v}");
        }
    }

    #[test]
    fn card_shows_name_status_word_summary_and_optional_link() {
        let h = PolicyCard::new("production-deploy", Status::Active, summary()).meta("updated 2h ago").href("/app/policies/1").html();
        let s = h.as_str();
        assert!(s.contains("production-deploy") && s.contains("ACTIVE") && s.contains("WHO"));
        assert!(s.contains(r#"<a href="/app/policies/1""#) && s.contains("updated 2h ago"));
        assert!(!PolicyCard::new("x", Status::Pending, summary()).html().as_str().contains("<a "));
    }

    #[test]
    fn explorer_stacks_four_layers_then_the_decision() {
        let h = PolicyExplorer::new(&["acme/widget"], &["terraform"], &["db.prod:5432"], &["20 min", "TCP only"], true, "policy production-deploy matches").html();
        let s = h.as_str();
        for layer in ["WHO", "WHAT", "WHERE", "LIMITS"] {
            assert!(s.contains(&format!(r#"data-layer="{}""#, layer.to_lowercase())), "{layer}");
        }
        assert_eq!(s.matches("v-layer-link").count(), 4, "a link before each layer after the first and before the result");
        assert!(s.contains("ACCESS GRANTED") && s.contains("policy production-deploy matches"));
        assert!(s.contains("TCP only"));
        let denied = PolicyExplorer::new(&[], &[], &[], &[], false, "no matching allow rule").html();
        assert!(denied.as_str().contains("ACCESS DENIED"));
        assert!(denied.as_str().contains("any"), "an empty layer says so");
    }

    #[test]
    fn diff_marks_added_removed_changed_with_glyph_and_word() {
        let h = PolicyDiff::new(vec![
            Change::added("limit", "TCP only"),
            Change::removed("target", "db.old:5432"),
            Change::changed("duration", "20 min", "10 min"),
        ]).html();
        let s = h.as_str();
        assert!(s.contains("v-diff-added") && s.contains("v-diff-removed") && s.contains("v-diff-changed"));
        assert!(s.contains("+") && s.contains("−") && s.contains("~"));
        for w in ["added", "removed", "changed"] {
            assert!(s.contains(w), "{w}");
        }
        assert!(s.contains("20 min") && s.contains("10 min") && s.contains("→"));
        assert!(PolicyDiff::new(vec![]).html().as_str().contains("No changes"));
    }

    #[test]
    fn policy_components_escape_their_strings() {
        let s = PolicySummary::new("<script>", "a", "b", "c").html();
        assert!(!s.as_str().contains("<script>") && s.as_str().contains("&lt;script&gt;"));
        let d = PolicyDiff::new(vec![Change::added("<i>", "\"q\"")]).html();
        assert!(!d.as_str().contains("<i>"));
    }
}
```

- [ ] **Step 2:** `cargo test -p skimasque-visual --features site policy` → FAIL.

- [ ] **Step 3: Implement `src/policy.rs`** (above tests):

```rust
//! Policy components: the WHO → WHAT → WHERE → LIMITS pattern in four shapes.

use askama::Template;

use crate::{Component, DecisionBadge, Html, Status, StatusBadge};

#[derive(Template, Debug, Clone)]
#[template(path = "policy_summary.html")]
pub struct PolicySummary {
    pub who: String,
    pub what: String,
    pub target: String,
    pub limits: String,
}
impl PolicySummary {
    pub fn new(who: impl Into<String>, what: impl Into<String>, target: impl Into<String>, limits: impl Into<String>) -> Self {
        Self { who: who.into(), what: what.into(), target: target.into(), limits: limits.into() }
    }
}
impl Component for PolicySummary {}

#[derive(Template, Debug, Clone)]
#[template(path = "policy_card.html")]
pub struct PolicyCard {
    pub name: String,
    pub status: Status,
    pub summary: PolicySummary,
    pub meta: Option<String>,
    pub href: Option<String>,
}
impl PolicyCard {
    pub fn new(name: impl Into<String>, status: Status, summary: PolicySummary) -> Self {
        Self { name: name.into(), status, summary, meta: None, href: None }
    }
    pub fn meta(mut self, meta: impl Into<String>) -> Self {
        self.meta = Some(meta.into());
        self
    }
    pub fn href(mut self, href: impl Into<String>) -> Self {
        self.href = Some(href.into());
        self
    }
    fn status_html(&self) -> Html {
        StatusBadge { status: self.status }.html()
    }
    fn summary_html(&self) -> Html {
        self.summary.html()
    }
}
impl Component for PolicyCard {}

struct Layer {
    key: &'static str,
    title: &'static str,
    lines: Vec<String>,
}

#[derive(Template, Debug, Clone)]
#[template(path = "policy_explorer.html")]
pub struct PolicyExplorer {
    pub who: Vec<String>,
    pub what: Vec<String>,
    pub target: Vec<String>,
    pub limits: Vec<String>,
    pub allow: bool,
    pub reason: String,
}
impl PolicyExplorer {
    pub fn new(who: &[&str], what: &[&str], target: &[&str], limits: &[&str], allow: bool, reason: impl Into<String>) -> Self {
        let own = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect();
        Self { who: own(who), what: own(what), target: own(target), limits: own(limits), allow, reason: reason.into() }
    }
    fn layers(&self) -> Vec<Layer> {
        vec![
            Layer { key: "who", title: "WHO", lines: self.who.clone() },
            Layer { key: "what", title: "WHAT", lines: self.what.clone() },
            Layer { key: "where", title: "WHERE", lines: self.target.clone() },
            Layer { key: "limits", title: "LIMITS", lines: self.limits.clone() },
        ]
    }
    fn decision_html(&self) -> Html {
        DecisionBadge::new(self.allow).html()
    }
}
impl Component for PolicyExplorer {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone)]
pub struct Change {
    pub kind: ChangeKind,
    pub field: String,
    pub before: Option<String>,
    pub after: Option<String>,
}
impl Change {
    pub fn added(field: impl Into<String>, after: impl Into<String>) -> Self {
        Self { kind: ChangeKind::Added, field: field.into(), before: None, after: Some(after.into()) }
    }
    pub fn removed(field: impl Into<String>, before: impl Into<String>) -> Self {
        Self { kind: ChangeKind::Removed, field: field.into(), before: Some(before.into()), after: None }
    }
    pub fn changed(field: impl Into<String>, before: impl Into<String>, after: impl Into<String>) -> Self {
        Self { kind: ChangeKind::Changed, field: field.into(), before: Some(before.into()), after: Some(after.into()) }
    }
    fn class(&self) -> &'static str {
        match self.kind {
            ChangeKind::Added => "added",
            ChangeKind::Removed => "removed",
            ChangeKind::Changed => "changed",
        }
    }
    fn glyph(&self) -> &'static str {
        match self.kind {
            ChangeKind::Added => "+",
            ChangeKind::Removed => "−",
            ChangeKind::Changed => "~",
        }
    }
}

#[derive(Template, Debug, Clone)]
#[template(path = "policy_diff.html")]
pub struct PolicyDiff {
    pub changes: Vec<Change>,
}
impl PolicyDiff {
    pub fn new(changes: Vec<Change>) -> Self {
        Self { changes }
    }
}
impl Component for PolicyDiff {}
```

  Templates:
  - `policy_summary.html`: `<dl class="v-summary"><div><dt>WHO</dt><dd>{{ who }}</dd></div><div><dt>WHAT</dt><dd>{{ what }}</dd></div><div><dt>WHERE</dt><dd>{{ target }}</dd></div><div><dt>LIMITS</dt><dd>{{ limits }}</dd></div></dl>`
  - `policy_card.html`: `<article class="v-card v-policy-card"><header class="v-card-head"><h3 class="v-card-title">{% if let Some(h) = href %}<a href="{{ h }}">{{ name }}</a>{% else %}{{ name }}{% endif %}</h3>{{ self.status_html()|safe }}</header>{{ self.summary_html()|safe }}{% if let Some(m) = meta %}<footer class="v-card-meta">{{ m }}</footer>{% endif %}</article>`
  - `policy_explorer.html`:
```html
<div class="v-explorer" role="group" aria-label="Policy explorer">{% for l in self.layers() %}<section class="v-layer" data-layer="{{ l.key }}"><h4 class="v-layer-title">{{ l.title }}</h4>{% if l.lines.is_empty() %}<p class="v-layer-line v-layer-any">any</p>{% else %}<ul class="v-layer-lines">{% for line in l.lines %}<li>{{ line }}</li>{% endfor %}</ul>{% endif %}</section><span class="v-layer-link" aria-hidden="true">↓</span>{% endfor %}<section class="v-layer v-explorer-result">{{ self.decision_html()|safe }}<p class="v-layer-line">{{ reason }}</p></section></div>
```
    (four layers each followed by a link = 4 `v-layer-link` spans, matching the test.)
  - `policy_diff.html`: `{% if changes.is_empty() %}<p class="v-diff-empty">No changes</p>{% else %}<ul class="v-diff">{% for c in changes %}<li class="v-diff-{{ c.class() }}"><span class="v-diff-glyph" aria-hidden="true">{{ c.glyph() }}</span><span class="v-sr">{{ c.class() }} </span><span class="v-diff-field">{{ c.field }}</span>{% if let Some(b) = c.before %}<span class="v-diff-before">{{ b }}</span>{% endif %}{% if c.before.is_some() && c.after.is_some() %}<span aria-hidden="true">→</span>{% endif %}{% if let Some(a) = c.after %}<span class="v-diff-after">{{ a }}</span>{% endif %}</li>{% endfor %}</ul>{% endif %}`
    If askama rejects calling `c.class()` on a loop item inside `{{ }}`, it works for methods on items (0.12 supports it); the `{{ c.glyph() }}` likewise.

  `lib.rs`: `pub mod policy;` and `pub use policy::{Change, ChangeKind, PolicyCard, PolicyDiff, PolicyExplorer, PolicySummary};`.

- [ ] **Step 4: CSS layer** — append (`.v-card` is shared by later tasks; roles only):

```css
/* cards */
.v-card, .v-summary, .v-explorer, .v-diff { box-sizing: border-box; max-width: 100%; font: 13px/1.5 var(--font); color: var(--text); }
.v-card { min-width: 0; padding: 16px; border: 1px solid var(--line-default); border-radius: var(--radius-lg); background: var(--surface); display: grid; gap: 12px; }
.v-card-head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; }
.v-card-title { margin: 0; font-size: 14px; font-weight: 600; overflow-wrap: anywhere; }
.v-card-title a { color: var(--text); text-decoration-color: var(--v-active); text-underline-offset: 3px; }
.v-card-title a:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.v-card-meta { color: var(--text-muted); font-size: 12px; overflow-wrap: anywhere; }

/* policy summary */
.v-summary { margin: 0; display: grid; gap: 6px; }
.v-summary > div { display: grid; grid-template-columns: 72px minmax(0, 1fr); gap: 12px; }
.v-summary dt { color: var(--text-muted); font-size: 11px; font-weight: 600; letter-spacing: .1em; padding-top: 2px; }
.v-summary dd { margin: 0; overflow-wrap: anywhere; }

/* policy explorer */
.v-explorer { display: grid; justify-items: start; gap: 2px; min-width: 0; }
.v-layer { width: 100%; box-sizing: border-box; padding: 10px 14px; border: 1px solid var(--line-default); border-left: 3px solid var(--v-structure);
  border-radius: var(--radius-md); background: var(--surface); }
.v-layer-title { margin: 0 0 4px; color: var(--v-structure-text); font-size: 11px; font-weight: 600; letter-spacing: .1em; }
.v-layer-lines { margin: 0; padding: 0; list-style: none; display: grid; gap: 2px; }
.v-layer-lines li, .v-layer-line { overflow-wrap: anywhere; }
.v-layer-line { margin: 4px 0 0; }
.v-layer-any { color: var(--text-muted); font-style: italic; margin: 0; }
.v-layer-link { margin-left: 20px; color: var(--v-neutral-text); line-height: 1; }
.v-explorer-result { border-left-color: var(--accent); background: var(--surface-raised); }

/* policy diff */
.v-diff { margin: 0; padding: 0; list-style: none; display: grid; gap: 4px; }
.v-diff li { display: flex; flex-wrap: wrap; gap: 4px 10px; align-items: baseline; padding: 4px 10px; border-left: 3px solid var(--v-tone); border-radius: var(--radius-sm); overflow-wrap: anywhere; }
.v-diff-added { --v-tone: var(--v-active); background: var(--accent-soft); }
.v-diff-removed { --v-tone: var(--v-deny); }
.v-diff-changed { --v-tone: var(--v-warning); }
.v-diff-glyph { font-weight: 700; color: var(--v-tone); }
.v-diff-field { color: var(--text-muted); }
.v-diff-before { text-decoration: line-through; color: var(--text-muted); }
.v-diff-empty { margin: 0; color: var(--text-muted); }
```

  (`--v-tone` on a `border-left` is a line; the glyph's colour is a contrast-covered role text colour only where `v-active/v-deny/v-warning` — all three are in the contrast test's text list.)

- [ ] **Step 5: Gallery** — group `Policies` (`wide: false`): `summary`, `card · active` (with href and meta), `card · pending`, `explorer · granted`, `explorer · denied` (empty layers, reason "no matching allow rule"), `diff`, `diff · empty`. Example data: who `acme/widget`, what `terraform`, target `db.prod:5432`, limits `20 min`. Test additions: `for s in ["data-layer=\"who\"", "v-diff-changed", "No changes"] { assert!(page.contains(s)) }`.

- [ ] **Step 6:** test + clippy + rustfmt + regenerate; commit `feat(visual): policy summary, card, explorer and diff`.

---

### Task 6: Decision components — Explainer, DecisionCard, AuditEventCard

**Files:**
- Create: `src/decision.rs`, `templates/{decision_explainer,decision_card,audit_event_card}.html`
- Modify: `src/lib.rs`, `static/visual.css`, `src/site.rs`

**Interfaces:**
- Produces:
  - `Check { label: String, pass: bool, detail: Option<String> }` with `Check::pass(label)`, `Check::fail(label)`, `.detail(s)`;
  - `DecisionExplainer { checks: Vec<Check>, allow: bool, reason: String }` (`DecisionExplainer::new(checks, allow, reason)`) and the five canonical labels as `pub const DIMENSIONS: [&str; 5] = ["Identity", "Application", "Destination", "Limits", "Policy active"];`
  - `DecisionCard { allow: bool, who: String, target: String, policy: Option<String>, when: String }` (`DecisionCard::new(allow, who, target, when)`, `.policy(s)`)
  - `AuditEventCard { card: DecisionCard, reason: String, explainer: Option<DecisionExplainer>, technical: bool }` (`AuditEventCard::new(card, reason)`, `.explainer(e)`, `.technical()`). `technical` switches the badge to ALLOW/DENY (audit's technical detail).

- [ ] **Step 1: Failing tests** (in `src/decision.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    fn granted() -> DecisionExplainer {
        DecisionExplainer::new(
            DIMENSIONS.iter().map(|d| Check::pass(*d)).collect(),
            true,
            "identity, application, destination and limits all match production-deploy",
        )
    }

    #[test]
    fn explainer_shows_a_glyph_and_a_word_per_check() {
        let s = granted().html().as_str().to_owned();
        assert_eq!(s.matches("v-check-pass").count(), 5);
        assert_eq!(s.matches("✓").count() >= 5, true);
        for d in DIMENSIONS {
            assert!(s.contains(d), "{d}");
        }
        assert!(s.contains("ACCESS GRANTED") && s.contains("all match production-deploy"));
    }

    #[test]
    fn a_failed_check_reads_as_failed_and_carries_its_detail() {
        let e = DecisionExplainer::new(
            vec![Check::pass("Identity"), Check::fail("Destination").detail("db.staging:5432 is not in this policy")],
            false,
            "destination not allowed",
        );
        let s = e.html().as_str().to_owned();
        assert!(s.contains("v-check-fail") && s.contains("×") && s.contains("did not match"));
        assert!(s.contains("db.staging:5432 is not in this policy") && s.contains("ACCESS DENIED"));
    }

    #[test]
    fn an_explainer_with_no_checks_still_states_the_decision() {
        let s = DecisionExplainer::new(vec![], false, "no policy applies").html().as_str().to_owned();
        assert!(s.contains("ACCESS DENIED") && s.contains("no policy applies") && !s.contains("<li"));
    }

    #[test]
    fn decision_card_lists_who_target_policy_and_time() {
        let s = DecisionCard::new(true, "acme/widget", "db.prod:5432", "2026-09-29 14:02 UTC").policy("production-deploy").html().as_str().to_owned();
        for v in ["acme/widget", "db.prod:5432", "production-deploy", "2026-09-29 14:02 UTC", "ACCESS GRANTED"] {
            assert!(s.contains(v), "{v}");
        }
        assert!(!DecisionCard::new(false, "a", "b", "c").html().as_str().contains("Policy"));
    }

    #[test]
    fn audit_card_expands_to_the_explainer_and_can_use_technical_wording() {
        let card = DecisionCard::new(true, "acme/widget", "db.prod:5432", "14:02");
        let plain = AuditEventCard::new(card.clone(), "matched production-deploy").html().as_str().to_owned();
        assert!(!plain.contains("<details"));
        let full = AuditEventCard::new(card.clone(), "matched production-deploy").explainer(granted()).technical().html().as_str().to_owned();
        assert!(full.contains("<details") && full.contains("v-check-pass"));
        assert!(full.contains("ALLOW"));
    }

    #[test]
    fn decision_components_escape_their_strings() {
        let s = DecisionCard::new(true, "<script>", "\"><img>", "t").html().as_str().to_owned();
        assert!(!s.contains("<script>") && !s.contains("<img>"));
    }
}
```

- [ ] **Step 2:** `cargo test -p skimasque-visual --features site decision` → FAIL.

- [ ] **Step 3: Implement `src/decision.rs`:**

```rust
//! Decision components: why access was granted or denied.

use askama::Template;

use crate::{Component, DecisionBadge, Html};

/// The five dimensions a decision is explained along (canonical §43/§71).
pub const DIMENSIONS: [&str; 5] = ["Identity", "Application", "Destination", "Limits", "Policy active"];

#[derive(Debug, Clone)]
pub struct Check {
    pub label: String,
    pub pass: bool,
    pub detail: Option<String>,
}
impl Check {
    pub fn pass(label: impl Into<String>) -> Self {
        Self { label: label.into(), pass: true, detail: None }
    }
    pub fn fail(label: impl Into<String>) -> Self {
        Self { label: label.into(), pass: false, detail: None }
    }
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Template, Debug, Clone)]
#[template(path = "decision_explainer.html")]
pub struct DecisionExplainer {
    pub checks: Vec<Check>,
    pub allow: bool,
    pub reason: String,
    pub technical: bool,
}
impl DecisionExplainer {
    pub fn new(checks: Vec<Check>, allow: bool, reason: impl Into<String>) -> Self {
        Self { checks, allow, reason: reason.into(), technical: false }
    }
    pub fn technical(mut self) -> Self {
        self.technical = true;
        self
    }
    fn decision_html(&self) -> Html {
        let b = DecisionBadge::new(self.allow);
        if self.technical { b.technical() } else { b }.html()
    }
}
impl Component for DecisionExplainer {}

#[derive(Template, Debug, Clone)]
#[template(path = "decision_card.html")]
pub struct DecisionCard {
    pub allow: bool,
    pub who: String,
    pub target: String,
    pub policy: Option<String>,
    pub when: String,
    pub technical: bool,
}
impl DecisionCard {
    pub fn new(allow: bool, who: impl Into<String>, target: impl Into<String>, when: impl Into<String>) -> Self {
        Self { allow, who: who.into(), target: target.into(), policy: None, when: when.into(), technical: false }
    }
    pub fn policy(mut self, policy: impl Into<String>) -> Self {
        self.policy = Some(policy.into());
        self
    }
    pub fn technical(mut self) -> Self {
        self.technical = true;
        self
    }
    fn decision_html(&self) -> Html {
        let b = DecisionBadge::new(self.allow);
        if self.technical { b.technical() } else { b }.html()
    }
}
impl Component for DecisionCard {}

#[derive(Template, Debug, Clone)]
#[template(path = "audit_event_card.html")]
pub struct AuditEventCard {
    pub card: DecisionCard,
    pub reason: String,
    pub explainer: Option<DecisionExplainer>,
}
impl AuditEventCard {
    pub fn new(card: DecisionCard, reason: impl Into<String>) -> Self {
        Self { card, reason: reason.into(), explainer: None }
    }
    pub fn explainer(mut self, e: DecisionExplainer) -> Self {
        self.explainer = Some(e);
        self
    }
    /// Audit's technical detail: ALLOW / DENY wording on the card and the explainer.
    pub fn technical(mut self) -> Self {
        self.card = self.card.technical();
        self.explainer = self.explainer.map(DecisionExplainer::technical);
        self
    }
    fn card_html(&self) -> Html {
        self.card.html()
    }
    fn explainer_html(&self) -> Option<Html> {
        self.explainer.as_ref().map(|e| e.html())
    }
}
impl Component for AuditEventCard {}
```

  Templates:
  - `decision_explainer.html`: `<div class="v-explain">{% if !checks.is_empty() %}<ul class="v-checks">{% for c in checks %}<li class="v-check {% if c.pass %}v-check-pass{% else %}v-check-fail{% endif %}"><span class="v-check-glyph" aria-hidden="true">{% if c.pass %}✓{% else %}×{% endif %}</span><span class="v-check-label">{{ c.label }}</span><span class="v-check-word">{% if c.pass %}matched{% else %}did not match{% endif %}</span>{% if let Some(d) = c.detail %}<span class="v-check-detail">{{ d }}</span>{% endif %}</li>{% endfor %}</ul>{% endif %}<div class="v-explain-result">{{ self.decision_html()|safe }}<p class="v-explain-reason">{{ reason }}</p></div></div>`
  - `decision_card.html`: `<article class="v-card v-decision-card"><header class="v-card-head">{{ self.decision_html()|safe }}<time class="v-card-meta">{{ when }}</time></header><p class="v-decision-line"><span class="v-decision-who">{{ who }}</span> <span aria-hidden="true">→</span> <span class="v-sr">to</span> <span class="v-decision-target">{{ target }}</span></p>{% if let Some(p) = policy %}<p class="v-card-meta">Policy {{ p }}</p>{% endif %}</article>`
  - `audit_event_card.html`: `<div class="v-audit-event">{{ self.card_html()|safe }}<p class="v-audit-reason">{{ reason }}</p>{% if let Some(e) = self.explainer_html() %}<details class="v-audit-why"><summary>Why?</summary>{{ e|safe }}</details>{% endif %}</div>`

  `lib.rs`: `pub mod decision;` and `pub use decision::{AuditEventCard, Check, DecisionCard, DecisionExplainer, DIMENSIONS};`. (Fix the test line `assert_eq!(s.matches("✓").count() >= 5, true)` to `assert!(s.matches("✓").count() >= 5)` when clippy complains.)

- [ ] **Step 4: CSS layer:**

```css
/* decisions */
.v-explain, .v-audit-event { box-sizing: border-box; max-width: 100%; min-width: 0; font: 13px/1.5 var(--font); color: var(--text); }
.v-checks { margin: 0 0 12px; padding: 0; list-style: none; display: grid; gap: 4px; }
.v-check { display: flex; flex-wrap: wrap; align-items: baseline; gap: 2px 10px; padding: 4px 10px; border-left: 3px solid var(--v-tone); border-radius: var(--radius-sm); overflow-wrap: anywhere; }
.v-check-pass { --v-tone: var(--v-active); }
.v-check-fail { --v-tone: var(--v-deny); background: color-mix(in srgb, var(--v-deny) 8%, transparent); }
.v-check-glyph { font-weight: 700; color: var(--v-tone); }
.v-check-label { font-weight: 600; }
.v-check-word { color: var(--text-muted); }
.v-check-detail { flex: 1 1 100%; color: var(--text-muted); font-size: 12px; }
.v-explain-result { display: grid; gap: 6px; justify-items: start; }
.v-explain-reason { margin: 0; overflow-wrap: anywhere; }
.v-decision-line { margin: 0; overflow-wrap: anywhere; }
.v-decision-who { font-weight: 600; }
.v-audit-event { display: grid; gap: 8px; }
.v-audit-reason { margin: 0; color: var(--text-soft); overflow-wrap: anywhere; }
.v-audit-why summary { cursor: pointer; color: var(--accent-text); }
.v-audit-why summary:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.v-audit-why[open] summary { margin-bottom: 8px; }
```

- [ ] **Step 5: Gallery** — group `Decisions`: `explainer · granted` (five checks from `DIMENSIONS`), `explainer · denied` (Identity, Application pass; Destination fail with detail `db.staging:5432 is not in this policy`; Limits, Policy active pass... ordering as you like; reason `destination not allowed`), `decision card · granted` (with policy), `decision card · denied`, `audit event · with explanation` (technical, explainer denied). Add test asserts: `v-check-fail`, `<details class="v-audit-why">`.

- [ ] **Step 6:** test + clippy + rustfmt + regenerate; commit `feat(visual): decision explainer, decision card, audit event card`.

---

### Task 7: Access components — Session, Timeline, Gateway, Health, Identity

**Files:**
- Create: `src/access.rs`, `templates/{session_card,session_timeline,gateway_card,health_card,identity_card}.html`
- Modify: `src/lib.rs`, `static/visual.css`, `src/site.rs`

**Interfaces:**
- Consumes: `Status`, `StatusBadge`, `Tone`, `Planned`, `Component`, `Html`.
- Produces:
  - `SessionCard { identity, target, gateway: String, status: Status, remaining_pct: u32, remaining_label: String }` (`SessionCard::new(identity, target, gateway, status, remaining_pct, remaining_label)`); bar width clamped to 0–100 in `fn pct(&self) -> u32`.
  - `TimelineEvent { time: String, label: String, tone: Tone }` (`TimelineEvent::new(time, label, tone)`), `SessionTimeline { events: Vec<TimelineEvent> }` (`SessionTimeline::new(events)`).
  - `GatewayCard { name, region: String, status: Status, last_heartbeat: String, sessions: u32, egress_ip: Option<String> }` (`GatewayCard::new(name, region, status, last_heartbeat, sessions)`, `.egress_ip(s)`). `egress_ip: None` renders `Planned`.
  - `HealthCard { title: String, status: Status, detail: String }` (`HealthCard::new`).
  - `IdentityCard { name, source: String, last_seen: Option<String>, policies: u32 }` (`IdentityCard::new(name, source, policies)`, `.last_seen(s)`).

- [ ] **Step 1: Failing tests** (in `src/access.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Status, Tone};

    #[test]
    fn session_card_shows_status_word_target_and_remaining_time() {
        let s = SessionCard::new("acme/widget", "db.prod:5432", "us-west", Status::Active, 40, "12 min left").html().as_str().to_owned();
        for v in ["acme/widget", "db.prod:5432", "us-west", "ACTIVE", "12 min left"] {
            assert!(s.contains(v), "{v}");
        }
        assert!(s.contains(r#"role="progressbar""#) && s.contains(r#"aria-valuenow="40""#) && s.contains("width: 40%"));
    }

    #[test]
    fn the_remaining_time_bar_clamps() {
        let over = SessionCard::new("a", "b", "c", Status::Active, 400, "x").html().as_str().to_owned();
        assert!(over.contains("width: 100%") && over.contains(r#"aria-valuenow="100""#));
        let none = SessionCard::new("a", "b", "c", Status::Expired, 0, "expired").html().as_str().to_owned();
        assert!(none.contains("width: 0%") && none.contains("EXPIRED") && none.contains("expired"));
    }

    #[test]
    fn timeline_lists_events_in_order_with_their_words() {
        let t = SessionTimeline::new(vec![
            TimelineEvent::new("14:02:01", "Requested", Tone::Neutral),
            TimelineEvent::new("14:02:01", "Authorized", Tone::Active),
            TimelineEvent::new("14:22:01", "Expires", Tone::Neutral),
        ]).html().as_str().to_owned();
        assert!(t.find("Requested").unwrap() < t.find("Authorized").unwrap());
        assert!(t.find("Authorized").unwrap() < t.find("Expires").unwrap());
        assert!(t.contains("<ol") && t.contains("v-tone-active") && t.contains("14:22:01"));
        assert!(SessionTimeline::new(vec![]).html().as_str().contains("No events"));
    }

    #[test]
    fn gateway_egress_ip_is_planned_unless_known() {
        let g = GatewayCard::new("gw-us-west", "us-west-2", Status::Healthy, "12 s ago", 3).html().as_str().to_owned();
        assert!(g.contains("HEALTHY") && g.contains("12 s ago") && g.contains("gw-us-west"));
        assert!(g.contains("Egress IP") && g.contains("PLANNED"));
        let k = GatewayCard::new("g", "r", Status::Degraded, "9 min ago", 0).egress_ip("203.0.113.7").html().as_str().to_owned();
        assert!(k.contains("203.0.113.7") && !k.contains("PLANNED") && k.contains("DEGRADED"));
    }

    #[test]
    fn health_and_identity_cards_carry_words_and_escape() {
        let h = HealthCard::new("Control plane", Status::Offline, "no heartbeat for 10 min").html().as_str().to_owned();
        assert!(h.contains("OFFLINE") && h.contains("no heartbeat"));
        let i = IdentityCard::new("acme/widget", "GitHub Actions", 2).last_seen("2 h ago").html().as_str().to_owned();
        assert!(i.contains("GitHub Actions") && i.contains("2 h ago") && i.contains("2 policies"));
        assert!(IdentityCard::new("x", "y", 1).html().as_str().contains("1 policy"));
        assert!(IdentityCard::new("x", "y", 0).html().as_str().contains("never seen"));
        let e = IdentityCard::new("<script>", "<b>", 0).html().as_str().to_owned();
        assert!(!e.contains("<script>") && !e.contains("<b>"));
    }
}
```

- [ ] **Step 2:** `cargo test -p skimasque-visual --features site access` → FAIL.

- [ ] **Step 3: Implement `src/access.rs`:**

```rust
//! Access components: sessions, gateways, health, identities.

use askama::Template;

use crate::{Component, Html, Planned, Status, StatusBadge, Tone};

#[derive(Template, Debug, Clone)]
#[template(path = "session_card.html")]
pub struct SessionCard {
    pub identity: String,
    pub target: String,
    pub gateway: String,
    pub status: Status,
    pub remaining_pct: u32,
    pub remaining_label: String,
}
impl SessionCard {
    pub fn new(identity: impl Into<String>, target: impl Into<String>, gateway: impl Into<String>, status: Status, remaining_pct: u32, remaining_label: impl Into<String>) -> Self {
        Self { identity: identity.into(), target: target.into(), gateway: gateway.into(), status, remaining_pct, remaining_label: remaining_label.into() }
    }
    fn pct(&self) -> u32 {
        self.remaining_pct.min(100)
    }
    fn status_html(&self) -> Html {
        StatusBadge { status: self.status }.html()
    }
}
impl Component for SessionCard {}

#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub time: String,
    pub label: String,
    pub tone: Tone,
}
impl TimelineEvent {
    pub fn new(time: impl Into<String>, label: impl Into<String>, tone: Tone) -> Self {
        Self { time: time.into(), label: label.into(), tone }
    }
}

#[derive(Template, Debug, Clone)]
#[template(path = "session_timeline.html")]
pub struct SessionTimeline {
    pub events: Vec<TimelineEvent>,
}
impl SessionTimeline {
    pub fn new(events: Vec<TimelineEvent>) -> Self {
        Self { events }
    }
}
impl Component for SessionTimeline {}

#[derive(Template, Debug, Clone)]
#[template(path = "gateway_card.html")]
pub struct GatewayCard {
    pub name: String,
    pub region: String,
    pub status: Status,
    pub last_heartbeat: String,
    pub sessions: u32,
    pub egress_ip: Option<String>,
}
impl GatewayCard {
    pub fn new(name: impl Into<String>, region: impl Into<String>, status: Status, last_heartbeat: impl Into<String>, sessions: u32) -> Self {
        Self { name: name.into(), region: region.into(), status, last_heartbeat: last_heartbeat.into(), sessions, egress_ip: None }
    }
    pub fn egress_ip(mut self, ip: impl Into<String>) -> Self {
        self.egress_ip = Some(ip.into());
        self
    }
    fn status_html(&self) -> Html {
        StatusBadge { status: self.status }.html()
    }
    fn planned_html(&self) -> Html {
        Planned::new().note("per-gateway egress IP is not available yet").html()
    }
}
impl Component for GatewayCard {}

#[derive(Template, Debug, Clone)]
#[template(path = "health_card.html")]
pub struct HealthCard {
    pub title: String,
    pub status: Status,
    pub detail: String,
}
impl HealthCard {
    pub fn new(title: impl Into<String>, status: Status, detail: impl Into<String>) -> Self {
        Self { title: title.into(), status, detail: detail.into() }
    }
    fn status_html(&self) -> Html {
        StatusBadge { status: self.status }.html()
    }
}
impl Component for HealthCard {}

#[derive(Template, Debug, Clone)]
#[template(path = "identity_card.html")]
pub struct IdentityCard {
    pub name: String,
    pub source: String,
    pub last_seen: Option<String>,
    pub policies: u32,
}
impl IdentityCard {
    pub fn new(name: impl Into<String>, source: impl Into<String>, policies: u32) -> Self {
        Self { name: name.into(), source: source.into(), last_seen: None, policies }
    }
    pub fn last_seen(mut self, when: impl Into<String>) -> Self {
        self.last_seen = Some(when.into());
        self
    }
    fn policy_count(&self) -> String {
        match self.policies {
            1 => "1 policy".to_owned(),
            n => format!("{n} policies"),
        }
    }
}
impl Component for IdentityCard {}
```

  Templates:
  - `session_card.html`: `<article class="v-card v-session-card"><header class="v-card-head"><h3 class="v-card-title">{{ identity }}</h3>{{ self.status_html()|safe }}</header><p class="v-session-route"><span>{{ target }}</span> <span class="v-card-meta">via {{ gateway }}</span></p><div class="v-session-time"><div class="v-bar" role="progressbar" aria-label="Session time remaining" aria-valuemin="0" aria-valuemax="100" aria-valuenow="{{ self.pct() }}"><span class="v-bar-fill" style="width: {{ self.pct() }}%"></span></div><span class="v-card-meta">{{ remaining_label }}</span></div></article>`
  - `session_timeline.html`: `{% if events.is_empty() %}<p class="v-timeline-empty">No events</p>{% else %}<ol class="v-timeline">{% for e in events %}<li class="v-timeline-event v-tone-{{ e.tone.class() }}"><span class="v-timeline-dot" aria-hidden="true"></span><time class="v-timeline-time">{{ e.time }}</time><span class="v-timeline-label">{{ e.label }}</span></li>{% endfor %}</ol>{% endif %}`
  - `gateway_card.html`: `<article class="v-card v-gateway-card v-tone-edge"><header class="v-card-head"><h3 class="v-card-title">{{ name }}</h3>{{ self.status_html()|safe }}</header><dl class="v-facts"><div><dt>Region</dt><dd>{{ region }}</dd></div><div><dt>Last heartbeat</dt><dd>{{ last_heartbeat }}</dd></div><div><dt>Active sessions</dt><dd>{{ sessions }}</dd></div><div><dt>Egress IP</dt><dd>{% if let Some(ip) = egress_ip %}{{ ip }}{% else %}{{ self.planned_html()|safe }}{% endif %}</dd></div></dl></article>`
  - `health_card.html`: `<article class="v-card v-health-card"><header class="v-card-head"><h3 class="v-card-title">{{ title }}</h3>{{ self.status_html()|safe }}</header><p class="v-card-meta">{{ detail }}</p></article>`
  - `identity_card.html`: `<article class="v-card v-identity-card"><header class="v-card-head"><h3 class="v-card-title">{{ name }}</h3><span class="v-card-meta">{{ source }}</span></header><p class="v-card-meta">{% if let Some(w) = last_seen %}last seen {{ w }}{% else %}never seen{% endif %} · {{ self.policy_count() }}</p></article>`

  `lib.rs`: `pub mod access;` and `pub use access::{GatewayCard, HealthCard, IdentityCard, SessionCard, SessionTimeline, TimelineEvent};`. Inline `style` here is the one place the library emits a style attribute; the value is a `u32` clamped to 0–100 and cannot carry markup.

- [ ] **Step 4: CSS layer:**

```css
/* access */
.v-session-route { margin: 0; overflow-wrap: anywhere; }
.v-session-time { display: grid; gap: 4px; }
.v-bar { height: 6px; border-radius: 3px; background: var(--line-default); overflow: hidden; }
.v-bar-fill { display: block; height: 100%; background: var(--accent); }
.v-gateway-card { border-color: color-mix(in srgb, var(--v-edge) 55%, transparent); }
.v-facts { margin: 0; display: grid; gap: 4px; }
.v-facts > div { display: grid; grid-template-columns: 120px minmax(0, 1fr); gap: 12px; }
.v-facts dt { color: var(--text-muted); font-size: 12px; }
.v-facts dd { margin: 0; overflow-wrap: anywhere; }
.v-timeline { margin: 0; padding: 0; list-style: none; display: grid; }
.v-timeline-event { position: relative; display: grid; grid-template-columns: 16px 88px minmax(0, 1fr); gap: 4px 10px; align-items: baseline;
  padding: 0 0 14px; font: 13px/1.5 var(--font); color: var(--text); }
.v-timeline-event::before { content: ""; position: absolute; left: 5px; top: 14px; bottom: 0; border-left: 2px solid var(--line-strong); }
.v-timeline-event:last-child::before { display: none; }
.v-timeline-dot { width: 12px; height: 12px; border-radius: 50%; background: var(--v-tone); align-self: center; box-sizing: border-box; }
.v-timeline-time { color: var(--text-muted); font-size: 12px; }
.v-timeline-label { overflow-wrap: anywhere; font-weight: 600; }
.v-timeline-empty { margin: 0; color: var(--text-muted); font: 13px/1.5 var(--font); }
@media (max-width: 480px) { .v-facts > div, .v-summary > div { grid-template-columns: minmax(0, 1fr); gap: 0; } }
```

- [ ] **Step 5: Gallery** — group `Access`: `session · active 40%` (`Status::Active`, 40, "12 min left"), `session · expired` (0), `timeline` (Requested/Authorized/Session started/Expires: neutral/active/active/neutral), `gateway · healthy (egress planned)`, `gateway · degraded (egress known)`, `health · healthy`, `health · offline`, `identity`, `identity · never seen`. Add a `Group { title: "Hostile input", wide: false, items: … }` with one `PolicyCard` whose name is `<script>alert(1)</script>` plus a 200-character unbroken string in `who`, and an `IdentityCard` with the same: it proves escaping and no overflow at 320px. Test additions: `for s in ["role=\"progressbar\"", "v-timeline", "Egress IP", "never seen"]`, and `assert!(!page.contains("<script>alert"), "hostile strings are escaped")`.

- [ ] **Step 6:** test + clippy + rustfmt + regenerate. Then a manual check: serve `site/components`, `resize_window` to 320 wide, confirm `document.documentElement.scrollWidth <= innerWidth` in both theme sections, and screenshot both themes at 1280 wide to skim contrast. Commit `feat(visual): session, timeline, gateway, health and identity cards`.

---

### Task 8: Wrap-up — gallery honesty, docs, CI

**Files:** Modify `docs/superpowers/specs/2026-09-30-product-surface-amendment.md` (status line), `site/components` (already regenerated).

- [ ] **Step 1: Whole-suite run** — `cargo test -p skimasque-visual --features site`, `cargo clippy -p skimasque-visual --all-targets --features site -- -D warnings`, `cargo run -q -p skimasque-visual --features site --bin sitegen -- --check` all clean; `cargo test --workspace` not regressed (CLI crate untouched).
- [ ] **Step 2: Gallery check in both themes** at 320, 720 and 1280 px: no horizontal page scroll; every status shows a word; Planned markers visible on gateway egress; no `exec` (`document.body.innerText.includes('exec') === false`).
- [ ] **Step 3: Spec status** — change `Status: draft, pending approval` in the amendment to `Status: approved 2026-09-29; plan 2 (palette, product components) implemented`.
- [ ] **Step 4: Commit** `docs(visual): amendment status`. Do not push; the controller does the final review and PR.

---

## Self-Review

- **Spec coverage:** palette + derived tokens + contrast test + light/dark/auto → Task 1; `Status::Granted/Denied`, technical wording → Task 2; parked `.v-flow-wrap` fix (Task 1) and 980px review (Task 3); `Planned`, `EmptyState` (§51 copy), `CodeExample` (Pine, mint, `data-copy`) → Task 4; `PolicyCard`, `PolicySummary`, `PolicyExplorer`, `PolicyDiff` → Task 5; `DecisionExplainer`, `DecisionCard`, `AuditEventCard` → Task 6; `SessionCard` (+bar), `SessionTimeline`, `GatewayCard`, `HealthCard`, `IdentityCard` → Task 7; parked minors (icon-id uniqueness, recursive template scan, hostile-caption test via hostile group, `--v-warning`, connection-label comment) → Tasks 1 and 7. Diagrams/motifs (plan 3), site chrome/pages (plan 4) are out of scope by the delivery table.
- **Placeholders:** none; every step carries code or a command.
- **Type consistency:** `DecisionBadge::new/technical` (Task 2) used in Tasks 5–6; `Planned::new().note` (Task 4) used in Task 7; `Tone::Warning` (Task 1) used by `Status::Degraded` (Task 2); `Status::Healthy/Degraded/Offline` (Task 2) used in Task 7; `.v-card` classes defined in Task 5 are reused in Tasks 6–7; `rules()/rule()/contrast()` helpers defined in Task 1's test file are reused by Task 4's palette-pair test.
- **Known judgement calls:** default theme is light without media query (the app must set `data-theme="dark"`; `auto` is opt-in); `--sand` is added as the edge fill for light; the library's only button style is `.v-btn` (Mint fill, Pine text, 6.0:1).
