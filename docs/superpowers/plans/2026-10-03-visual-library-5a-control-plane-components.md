# Visual Library 5a: Control-Plane Component Gaps Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the four library gaps the control plane (plan 5) cannot work around: a heading level on card components, links on the access cards, narrated decision checks, and accessible names on the Policy Explorer's groups.

**Architecture:** Additive builder methods on existing components. Defaults keep today's markup byte-for-byte, so every generated site page stays identical and `sitegen --check` stays clean except for the Explorer (Task 2), whose regenerated pages are committed in the same task.

**Tech Stack:** Rust 1.88, askama 0.12, plain CSS. Crate `crates/skimasque-visual`.

**Spec:** `docs/superpowers/specs/2026-09-30-product-surface-amendment.md` (Library additions, Control plane). Carried-forward findings from the plan-4 final review: card `h3` is fixed, and the Explorer's `<section>`s have no headings.

**Working directory:** `C:\Users\Thor\Documents\src\skimasque-dev\wt-visual` (branch `feat/visual-library-5a`, cut from `main` at `5fa37f2`). All paths below are relative to it.

## Global Constraints

- Every component escapes every string it is given; nested markup is only ever `skimasque_visual::Html`. `|safe` only on `Html`.
- Hex colours live only in the `/* tokens */` block of `crates/skimasque-visual/static/visual.css`. This plan changes no colour.
- Defaults do not change output: a component built with today's constructors renders byte-identical markup (heading `h3`, no link).
- `href` values are app-built paths. They are HTML-escaped but not scheme-checked; the doc comment says so, as `PolicyCard::href` does today.
- Commits: conventional prefix, **no AI attribution trailers or footers**.
- Before each commit: `cargo fmt`, `cargo clippy -p skimasque-visual --all-targets --features site -- -D warnings`, `cargo test -p skimasque-visual --features site`, `cargo run -p skimasque-visual --features site --bin sitegen -- --check`.

## Review Focus

1. **Out-of-range heading level** (`level(0)`, `level(1)`, `level(9)`): clamps to 2..=6, never emits `<h0>`/`<h9>` and never a second `<h1>` on a page. Test in Task 1.
2. **A link whose text contains markup** (`<b>`, quotes): the link text and the `href` attribute are escaped. Test in Task 1.
3. **Narrated check with an empty or very long sentence**: renders one list item, no "matched" word, and a screen-reader word for the state. Test in Task 2.
4. **Two Explorers on one page** (the gallery renders several): group names repeat but no `id` is duplicated. Test in Task 2.
5. **Regenerated site**: every changed `site/` file is a page that contains an Explorer; no other page changes. Checked in Task 2, Step 6.

---

## File structure

```
crates/skimasque-visual/src/content.rs      MOD  EmptyState: level
crates/skimasque-visual/src/access.rs       MOD  SessionCard/GatewayCard/IdentityCard: level + href; HealthCard: level
crates/skimasque-visual/src/policy.rs       MOD  PolicyCard: level; PolicyExplorer: named groups
crates/skimasque-visual/src/decision.rs     MOD  Check::step (narrated)
crates/skimasque-visual/templates/{empty_state,session_card,gateway_card,health_card,identity_card,policy_card,policy_explorer,decision_explainer}.html  MOD
crates/skimasque-visual/static/visual.css   MOD  narrated check; group selector if any rule targets `section.v-layer`
site/**                                     REGEN pages that contain an Explorer
```

---

### Task 1: Heading level and links on the cards

**Files:**
- Modify: `crates/skimasque-visual/src/{content,access,policy}.rs`
- Modify: `crates/skimasque-visual/templates/{empty_state,session_card,gateway_card,health_card,identity_card,policy_card}.html`
- Test: the `#[cfg(test)]` modules in the same source files

**Interfaces:**
- Produces, on each of `PolicyCard`, `SessionCard`, `GatewayCard`, `HealthCard`, `IdentityCard`, `EmptyState`:
  `pub level: u8` (default `3`) and `pub fn level(self, n: u8) -> Self` (clamps to `2..=6`).
- Produces, on `SessionCard`, `GatewayCard`, `IdentityCard`: `pub href: Option<String>` (default `None`) and `pub fn href(self, href: impl Into<String>) -> Self`. `PolicyCard` already has `href`.
- Consumes nothing new.

- [ ] **Step 1: Write the failing tests**

Add to the test module of `crates/skimasque-visual/src/access.rs` (create the module if the file has none; it needs `use super::*; use crate::Component;`):

```rust
#[test]
fn cards_default_to_h3_and_take_a_level() {
    let s = SessionCard::new("acme/widget", "db:5432", "gw-1", Status::Active, 60, "10m left");
    assert!(s.html().as_str().contains("<h3 class=\"v-card-title\">"));
    let s = s.level(2);
    let h = s.html();
    assert!(h.as_str().contains("<h2 class=\"v-card-title\">"), "{}", h.as_str());
    assert!(h.as_str().contains("</h2>"));
    assert!(!h.as_str().contains("<h3"));
}

#[test]
fn the_level_is_clamped_to_two_through_six() {
    for (asked, want) in [(0u8, 2u8), (1, 2), (2, 2), (6, 6), (9, 6)] {
        let g = GatewayCard::new("gw", "us-west", Status::Healthy, "now", 0).level(asked);
        assert_eq!(g.level, want);
        let h = g.html();
        assert!(h.as_str().contains(&format!("<h{want} ")), "{}", h.as_str());
        assert!(!h.as_str().contains("<h1"));
    }
}

#[test]
fn a_card_title_can_link_and_escapes_text_and_href() {
    let g = GatewayCard::new("<b>gw</b>", "us", Status::Healthy, "now", 0)
        .href("/app/gateways/a\"b");
    let h = g.html();
    let s = h.as_str();
    assert!(s.contains("&lt;b&gt;gw&lt;/b&gt;"), "{s}");
    assert!(!s.contains("<b>gw</b>"), "{s}");
    assert!(s.contains("href=\"/app/gateways/a&quot;b\"") || s.contains("href=\"/app/gateways/a&#34;b\""), "{s}");
    let i = IdentityCard::new("acme/widget", "GitHub Actions", 2).href("/app/identities/x");
    assert!(i.html().as_str().contains("<a href=\"/app/identities/x\">acme/widget</a>"));
    let n = SessionCard::new("a", "b", "c", Status::Active, 1, "x").href("/app/sessions/g.1");
    assert!(n.html().as_str().contains("<a href=\"/app/sessions/g.1\">a</a>"));
}
```

Add to `crates/skimasque-visual/src/content.rs` tests:

```rust
#[test]
fn an_empty_state_takes_a_heading_level() {
    let e = EmptyState::no_sessions().level(2);
    assert!(e.html().as_str().contains("<h2 class=\"v-empty-title\">"));
    assert!(EmptyState::no_sessions().html().as_str().contains("<h3 class=\"v-empty-title\">"));
}
```

Add to `crates/skimasque-visual/src/policy.rs` tests:

```rust
#[test]
fn a_policy_card_takes_a_heading_level() {
    let c = PolicyCard::new("prod", Status::Active, PolicySummary::new("a", "b", "c", "d"))
        .href("/app/policies/prod")
        .level(2);
    let h = c.html();
    assert!(h.as_str().contains("<h2 class=\"v-card-title\"><a href=\"/app/policies/prod\">prod</a></h2>"), "{}", h.as_str());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p skimasque-visual --features site level href`
Expected: compile errors, `no method named level` / `href` on the cards.

- [ ] **Step 3: Implement**

In each struct add the fields (`pub level: u8`, and `pub href: Option<String>` where listed) and set `level: 3, href: None` in every `new`. Add the builders:

```rust
/// The heading level of the card title, `2..=6` (default 3). Pick the level
/// that follows the page's own headings so levels never skip.
pub fn level(mut self, n: u8) -> Self {
    self.level = n.clamp(2, 6);
    self
}
/// Makes the title a link. Must be an app-built path: it is HTML-escaped but
/// not scheme-checked; never pass user input.
pub fn href(mut self, href: impl Into<String>) -> Self {
    self.href = Some(href.into());
    self
}
```

`PolicyCard` and `EmptyState` get `level` only (`PolicyCard` has `href`; `EmptyState` has its own `action`). `HealthCard` gets `level` only.

Templates: replace the fixed heading with the level. `session_card.html`, `gateway_card.html`, `identity_card.html`:

```html
<h{{ level }} class="v-card-title">{% if let Some(h) = href %}<a href="{{ h }}">{{ NAME }}</a>{% else %}{{ NAME }}{% endif %}</h{{ level }}>
```

where `NAME` is the field the template already prints (`identity` for the session card, `name` for the others). `health_card.html` and `policy_card.html` (link logic already present there) and `empty_state.html` change only the tag: `<h{{ level }} class="v-empty-title">…</h{{ level }}>`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p skimasque-visual --features site`
Expected: PASS. Then `cargo run -p skimasque-visual --features site --bin sitegen -- --check` must report nothing changed (defaults are byte-identical).

- [ ] **Step 5: Commit**

```bash
git add crates/skimasque-visual
git commit -m "feat(visual): heading level on cards and links on the access cards"
```

---

### Task 2: Narrated checks and named Explorer groups

**Files:**
- Modify: `crates/skimasque-visual/src/decision.rs`, `crates/skimasque-visual/templates/decision_explainer.html`
- Modify: `crates/skimasque-visual/src/policy.rs`, `crates/skimasque-visual/templates/policy_explorer.html`
- Modify: `crates/skimasque-visual/static/visual.css` (only if a rule selects `section.v-layer`; otherwise unchanged)
- Regenerate: `site/**`

**Interfaces:**
- Produces: `Check::step(text: impl Into<String>, pass: bool) -> Check` and `pub narrated: bool` on `Check`. A narrated check prints its text as a sentence and no "matched / did not match" word; the state is carried by the glyph plus a visually hidden "passed" / "failed".
- Produces: `PolicyExplorer` groups are `<div class="v-layer" role="group" aria-label="WHO">` (and WHAT, WHERE, LIMITS, and `Decision` for the result). No `<section>`, no ids.
- Consumes: `Check` as it is today (`pass`, `fail`, `detail`).

- [ ] **Step 1: Write the failing tests**

In `crates/skimasque-visual/src/decision.rs` tests:

```rust
#[test]
fn a_narrated_check_reads_as_a_sentence_with_a_hidden_state_word() {
    let e = DecisionExplainer::new(
        vec![
            Check::step("Identity is governed by policy `production-db`.", true),
            Check::step("No rule allows curl to db.prod:5432.", false),
        ],
        false,
        "No matching allow rule.",
    );
    let h = e.html();
    let s = h.as_str();
    assert!(s.contains("Identity is governed by policy `production-db`."), "{s}");
    assert!(!s.contains("matched"), "{s}");
    assert!(!s.contains("did not match"), "{s}");
    assert!(s.contains("<span class=\"v-sr\">passed</span>"), "{s}");
    assert!(s.contains("<span class=\"v-sr\">failed</span>"), "{s}");
    assert_eq!(s.matches("<li class=\"v-check ").count(), 2);
}

#[test]
fn a_dimension_check_still_shows_its_word() {
    let e = DecisionExplainer::new(vec![Check::pass("Identity")], true, "ok");
    assert!(e.html().as_str().contains("matched"));
}

#[test]
fn a_narrated_check_with_an_empty_sentence_still_renders_one_item() {
    let e = DecisionExplainer::new(vec![Check::step("", true)], true, "ok");
    assert_eq!(e.html().as_str().matches("<li class=\"v-check ").count(), 1);
}
```

In `crates/skimasque-visual/src/policy.rs` tests:

```rust
#[test]
fn explorer_groups_are_named_and_carry_no_ids() {
    let e = PolicyExplorer::new(&["acme/widget"], &["terraform"], &["db:5432"], &["20 min"], true, "ok");
    let h = e.html();
    let s = h.as_str();
    assert!(!s.contains("<section"), "{s}");
    for name in ["WHO", "WHAT", "WHERE", "LIMITS", "Decision"] {
        assert!(s.contains(&format!("role=\"group\" aria-label=\"{name}\"")), "{name}: {s}");
    }
    assert!(!s.contains(" id="), "{s}");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p skimasque-visual --features site narrated explorer_groups`
Expected: FAIL (`Check::step` missing; `<section` present).

- [ ] **Step 3: Implement**

`Check` gains `pub narrated: bool` (false in `pass`/`fail`), and:

```rust
/// One narrated step ("Identity is governed by policy X."): the text is the
/// whole message, so no "matched" word follows it. A screen reader still hears
/// whether it passed.
pub fn step(text: impl Into<String>, pass: bool) -> Self {
    Self { label: text.into(), pass, detail: None, narrated: true }
}
```

`decision_explainer.html`, replace the word span:

```html
{% if c.narrated %}<span class="v-sr">{% if c.pass %}passed{% else %}failed{% endif %}</span>{% else %}<span class="v-check-word">{% if c.pass %}matched{% else %}did not match{% endif %}</span>{% endif %}
```

`policy_explorer.html`: change each layer to

```html
<div class="v-layer" role="group" aria-label="{{ l.title }}" data-layer="{{ l.key }}"><p class="v-layer-title" aria-hidden="true">{{ l.title }}</p>…</div>
```

and the result to `<div class="v-layer v-explorer-result" role="group" aria-label="Decision">…</div>`. Keep the arrows between groups. If `visual.css` has a selector `section.v-layer`, change it to `.v-layer` (the class rules already match).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p skimasque-visual --features site`
Expected: PASS, including `tests/css.rs`.

- [ ] **Step 5: Regenerate the site**

```bash
cargo run -p skimasque-visual --features site --bin sitegen
git status --short site
```

- [ ] **Step 6: Check that only Explorer pages changed**

```bash
git diff --name-only site | xargs grep -L 'v-explorer'
```

Expected: no output (every changed page contains an Explorer). If a page without one changed, stop and find out why.

- [ ] **Step 7: Full verification and commit**

```bash
cargo fmt
cargo clippy -p skimasque-visual --all-targets --features site -- -D warnings
cargo test --workspace
cargo run -p skimasque-visual --features site --bin sitegen -- --check
git add crates/skimasque-visual site
git commit -m "feat(visual): narrated decision checks and named Policy Explorer groups"
```

---

### Task 3: Rulings, spec status and hand-off

**Files:**
- Modify: `docs/superpowers/specs/2026-09-30-product-surface-amendment.md` (status line only)

- [ ] **Step 1: Record the rulings**

`CodeExample` copy hooks: no library change. The component already emits `data-copy="<commands>"` on its `<figure>` (only `$ ` lines), and the control plane's `app.js` wires any `[data-copy]` element. Add one sentence to the doc comment of `CodeExample` (in `crates/skimasque-visual/src/content.rs`) saying so: "Hosts add the copy button: any script that looks for `[data-copy]` and copies the attribute works."

Change the amendment's status line to: `Status: approved 2026-09-29; plans 2, 3, 4 (site chrome and public pages) and 5a (control-plane component gaps) implemented`.

- [ ] **Step 2: Verify, commit, push, open the PR**

```bash
cargo fmt
cargo test --workspace
git add -A docs crates
git commit -m "docs(visual): CodeExample copy contract; amendment status"
git push -u origin feat/visual-library-5a
gh pr create --repo skimasque-dev/skimasque --base main --title "feat(visual): control-plane component gaps (heading level, card links, narrated checks)" --body "Additive library changes the control plane needs: a heading level and links on the access cards, narrated decision checks, and named Policy Explorer groups. Defaults render byte-identical markup; only pages containing an Explorer are regenerated."
```

Stop here. Plan 5 (control repo) starts after this PR merges, because it depends on `skimasque-visual` from `main`.
