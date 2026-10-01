# SkiMasque visual library

Status: approved design, pending implementation plans
Branch: `feat/visual-library` (repo `skimasque-dev/skimasque`)
Source: the "SkiMasque Website & Visual Component System" guide (the product
owner's forward-looking spec, §20–§46), scoped to its build phases 1–3.
Related: `skimasque-dev/control` `docs/superpowers/specs/2026-09-28-alpine-dashboard-design.md`
(dashboard redesign, paused before its Task 9 to adopt this library).

## Goal

One set of visual components — nodes, connections, statuses, flows,
boundaries, cards and composed diagrams — that the web dashboard renders with
live data and the marketing site renders with example data, so SkiMasque's
visual language is learned once and means the same thing everywhere.

The central metaphor: network access as a controlled route through
infrastructure — WHO → WHAT → WHERE → LIMITS → temporary access.

## Decisions

- **A Rust crate, `skimasque-visual`,** in the open-source `skimasque`
  workspace (`crates/skimasque-visual`). The dashboard (`control`) depends on
  it by git, as it does on the other workspace crates.
- **Components are Rust types that render themselves** (askama 0.12, the
  dashboard's version). Askama templates cannot be imported across crates, so
  each component owns its template inside the crate and implements
  `askama::Template` (and `Display`). Callers embed the rendered HTML with
  `{{ component|safe }}`. Inputs are escaped by askama; components never take
  pre-rendered HTML except from other library components.
- **CSS ships in the crate** as `static/visual.css` (`pub const CSS: &str`),
  and becomes the single source of the Alpine palette tokens. The dashboard
  serves it; the generator writes it next to the pages.
- **Static site generator** `sitegen` (a binary in the crate) renders page
  templates to static HTML under the repo's `site/`. Generated files are
  committed so GitHub Pages still publishes `site/` with no build step;
  `sitegen --check` fails when committed output is stale and runs in CI.
- **Scope: guide phases 1–3.** Primitives and tokens; core product visuals;
  marketing diagram families. Live/interactive visualisation (phases 4–5) and
  the marketing pages themselves are later projects.
- **Honesty rule.** Only what exists today is shown as working. Examples use
  real commands (`skimasque connect`, the `skimasque-dev/connect` Action);
  `skimasque exec`, pricing, static egress IPs and CONNECT-IP transport are not
  depicted as available. CONNECT-UDP (RFC 9298) and HTTP Datagrams (RFC 9297)
  are implemented; CONNECT-IP (RFC 9484) is wire formats only and may appear
  only labelled as such.

## Tokens (`visual.css` section 1–2)

Palette (unchanged from the Alpine dashboard/site work):
alpine-950 `#0b1117`, -900 `#111a22`, -850 `#16212a`, -800 `#1d2a34`,
-700 `#30414d`, -600 `#435763`, -500 `#61737e`, -400 `#81919a`;
snow-50 `#f7faf9`, -100 `#edf3f1`, -200 `#dce7e3`;
mint-500 `#63d7b1`, -400 `#7de4c2`, -300 `#a1efd5`, -200 `#c9f7e7`;
forest-950 `#0b1512`, -900 `#102019`, -800 `#173026`, -700 `#234638`, -600 `#35614d`;
earth-950 `#17120e`, -900 `#211914`, -800 `#30221a`, -700 `#4a3527`, -600 `#66503d`;
success `#63d7b1`, warning `#e7c66a`, danger `#e87979`, info `#79bde8`;
light-mode accent/success text `#0b7a5b` (≥4.5:1 on snow-50).

Role tokens (`--bg`, `--surface`, `--surface-raised`, `--text`,
`--text-muted`, `--line`, `--accent`, `--accent-text`, `--on-accent`,
`--focus`, …) are defined for dark (default) and light
(`prefers-color-scheme: light`), matching the dashboard's `app.css`, which
will import them from here instead of defining its own.

Visual grammar tokens (§36):
- `--v-active` mint — active traffic, permitted paths, sessions, positive state
- `--v-structure` forest — infrastructure nodes, boundaries
- `--v-edge` earth — gateways, firewall, network edge
- `--v-neutral` slate — neutral / inactive / secondary
- `--v-deny` danger — denied, blocked, expired (used sparingly)

Hex values appear only in the token block; every other rule and every
template uses `var(--…)`.

## Components

### Phase 1 — primitives

**`Icon`** — one inline SVG `<symbol>` sprite (`Icons::sprite()`, emitted
once per page) with stroke icons drawn in `currentColor`, referenced by
`<use href="#i-…">`. One icon per `NodeKind`, plus `check`, `cross`,
`arrow`, `clock`.

**`Node { kind: NodeKind, label, sub: Option<String>, status: Option<Status> }`**
`NodeKind`: `Workload, Developer, GitHub, CiJob, Application, Identity, Policy,
Session, Gateway, Network, Service, Database, Api, Kubernetes, Cloud, Firewall,
Internet, Allow, Deny`. Renders a compact card: icon, label, optional
secondary line, optional status. Kind decides the grammar colour:
structure (Network, Service, Database, Api, Kubernetes, Cloud), edge
(Gateway, Firewall, Internet), active (Session, Allow), deny (Deny), neutral
(everything else).

**`Connection { kind: ConnKind, label: Option<String> }`**
`ConnKind`: `Normal` (solid), `Control` (dashed), `Active` (thick mint, with
the pulse), `Potential` (faded dotted), `Denied` (broken line with ×). Label
examples: `OIDC`, `policy`, `session auth`. Denied connections carry the
visually-hidden word "denied"; active ones "active".

**`Status`** — `Allow, Deny, Active, Expired, Pending, Blocked`. Always a dot
plus the uppercase word (dot `aria-hidden`). **`DecisionBadge { allow }`** —
"✓ ACCESS GRANTED" / "× ACCESS DENIED".

**`Flow { steps: Vec<(Node, Option<Connection>)>, caption }`** — the linear
diagram workhorse. CSS grid: horizontal at ≥720px, vertical below, with the
connector rotating; never a scaled-down desktop drawing. Emits an ordered list
(`<ol>`) so the sequence reads correctly without CSS.

**`Boundary { label, kind: BoundaryKind, children }`** — `Region` (a
forest-tinted dashed frame, e.g. "YOUR VPC", "YOUR NETWORK") or `Firewall` (a
double rule with its label, e.g. "YOUR FIREWALL"). Children are library
components.

### Phase 2 — core product visuals

Each is built from the primitives and has a text equivalent.

- **`AccessFlow`** — Workload → Identity → Policy → Session → Gateway →
  Private infrastructure; the session→gateway→infrastructure path Active; the
  infrastructure inside a Boundary.
- **`PolicyModel`** — WHO → WHAT → WHERE → LIMITS → NETWORK ACCESS, with
  values (marketing example: `acme/widget · deploy-production`, `terraform`,
  `db.prod:5432`, `20m · 100 Mbps · us-west`).
- **`PolicyDecision`** — Request → Policy → MATCH → ALLOW / NO MATCH → DENY,
  with "No matching allow rule means DENY."
- **`AccessLifecycle`** — REQUEST → AUTHENTICATE → AUTHORIZE → CONNECT →
  ACTIVE → EXPIRE.
- **`GatewayDiagram { vpc: bool, egress: Option<String> }`** — control plane
  (Control connection) → gateway → your network → Database / API /
  Kubernetes; `vpc` wraps the gateway in a Region; `egress` adds a
  "Customer firewall" step labelled with the given egress description (the
  marketing example says "gateway egress IP", never a fabricated address).
- **`NetworkBoundary`** — Internet → Gateway → Firewall boundary → Private
  network.
- **`PolicyCard { name, who, what, where_, limits, sentence }`** — §26 layout
  (WHO / WHAT / WHERE / LIMITS labelled fields, optional sentence).
- **`SessionCard { application, destination, identity, remaining_label,
  fraction_remaining: Option<f32>, region }`** — §34: route, identity, a
  remaining-time bar (`<meter>` with text fallback) and "12m remaining";
  Active status.
- **`GatewayCard { name, status, region, egress: Option<String>,
  active_sessions: Option<u32> }`** — §34; omits fields that are `None`
  rather than inventing them.
- **`IdentityCard { source, lines }`** — e.g. GitHub / acme/widget /
  deploy-production / main.
- **`PolicyExplorer { who, what, where_, limits, decision: Option<bool> }`**
  — §35 vertical WHO→WHAT→WHERE→LIMITS→decision; one struct used with
  example data (marketing) or real data (dashboard).

### Phase 3 — marketing diagram families

- **Comparison**: Traditional vs SkiMasque (CI → VPN → Network → Everything
  vs CI → Identity → Policy → Destination → Session); VPN comparison.
- **CI/CD**: GitHub Actions → OIDC → SkiMasque → policy → session → Gateway →
  Private infrastructure; CI lifecycle (JOB START → IDENTITY → SESSION CREATED
  → DEPLOYMENT → JOB COMPLETE → SESSION EXPIRED).
- **Developer**: Developer → CLI (`skimasque connect`) → Identity → Policy →
  Temporary session → Private service; "same command, different policy"
  (one CLI node branching to staging/production policies).
- **Deployment runs**: Green Run — SkiMasque Cloud; Blue Run — Your Gateway;
  Black Run — Self-hosted. Identical geometry; the run colour is a small
  trail marker (circle/square/diamond shape plus colour, so not colour
  alone) and never implies quality; the technical name is always shown.
- **Security**: layers (IDENTITY ↑ POLICY ↑ SESSION ↑ GATEWAY ↑ NETWORK);
  no standing access (Credential ─standing access→ Network vs Identity →
  Policy → Session → EXPIRE); compartmentalisation (two jobs, each allowed
  one destination, everything else DENY).
- **Architecture**: control plane / data plane split; full architecture
  (Developer and CI/CD → control plane → session auth → MASQUE gateway →
  private network).
- **MASQUE**: protocol stack (Application → MASQUE → HTTP/3 → QUIC → UDP/IP);
  CONNECT-UDP sequence (client ⇄ gateway); multiple gateways (control plane
  → us-west / us-east / eu-west → VPCs).

## Motion

Only where it communicates state (§42): the Active connection pulse and the
PolicyDecision reveal. Both are CSS-only and disabled under
`@media (prefers-reduced-motion: reduce)`. No decorative, background or
parallax motion.

## Accessibility

Every SVG has `<title>` and `<desc>`; every composed diagram carries a
visually-hidden text equivalent sentence (e.g. "GitHub Actions authenticates
using workload identity, SkiMasque evaluates the request against policy, and
an authorized session is established through the gateway to the private
database."). Status always has a word; connections convey allowed/denied in
text; focusable elements get the mint focus ring; contrast ≥4.5:1 for text
in both themes.

## Generator and gallery

`sitegen` renders page templates from `crates/skimasque-visual/pages/` to
`site/`. This project produces one page: `site/components/index.html`, the
component gallery — every primitive in every kind and state, every Phase 2
and 3 diagram, in both themes (a theme toggle via a `data-theme` attribute on
sections, no JS required: each gallery section is shown twice, dark and
light). The current `site/index.html` is not regenerated in this project.

`sitegen --check` exits non-zero if rendering differs from the committed
files; a CI step runs it.

## Delivery

Four implementation plans, each independently shippable:

1. Crate, tokens, icons, primitives, Flow/Boundary, gallery skeleton, sitegen
   with `--check`.
2. Phase 2 core product visuals, added to the gallery.
3. Phase 3 marketing families, added to the gallery.
4. Dashboard adoption (in `skimasque-dev/control`, branch `style/alpine-ui`):
   depend on the crate, serve `visual.css`, move palette tokens out of
   `app.css`, swap Policies/Active access/Identities to library components,
   then resume dashboard tasks 9–13 on the library.

## Testing

- Per component: render tests for structure, text equivalents, SVG
  `<title>`/`<desc>`, status word beside every dot, connection-kind classes,
  escaping of hostile input (`<script>`, quotes).
- `visual.css` has hex only in the token block; no template contains a raw
  colour.
- Generator: deterministic output test; `sitegen --check` in CI.
- Browser-pane pass over the gallery at 1440px and 375px, dark and light,
  and with reduced motion (pulse stops).

## Out of scope

Marketing pages (Home, How it works, CI/CD, Developers, Compare, Deployment,
Security, Architecture, MASQUE, Use cases, Open source, Pricing, Docs, FAQ,
Trust); live/interactive visualisation (guide phases 4–5); `skimasque exec`;
pricing; CONNECT-IP transport.
