# SkiMasque product surface — spec amendment

Status: draft, pending approval
Amends: `2026-09-29-visual-library-design.md` (visual library) and, in
`skimasque-dev/control`, `2026-09-28-alpine-dashboard-design.md` (dashboard).
Source: "SkiMasque — Complete Public Website & Control Plane Specification"
(the product owner's canonical product/content spec, §1–§102), supplied
2026-09-29. Where this amendment and the earlier specs disagree, this
amendment wins.

## Decisions (product owner, 2026-09-29)

1. **Palette:** adopt the canonical palette. It replaces the Alpine
   palette in the library tokens and, through the library, in the dashboard,
   the public site and the CLI.
2. **Default theme:** the public website defaults to **light** (Snow /
   Pine). The control plane defaults to **dark** (a Pine-derived operations
   console). Each follows `prefers-color-scheme` / `data-theme` to the other.
3. **Unbuilt features are shown, marked Planned.** Pages and components
   follow the canonical spec, but anything the product does not do today
   carries a visible **Planned** marker. It is never described in the
   present tense and never offered as a working control. Working examples
   use real commands (`skimasque connect`, the `skimasque-dev/connect`
   Action); `skimasque exec` appears only in Planned contexts.
   - Planned today: `skimasque exec`, pricing tiers, per-gateway egress IP,
     disabling a policy, revoking a session, team roles beyond the current
     owner/member, SSO, CONNECT-IP transport, live policy tests in the
     dashboard, interactive topology, onboarding "Create Example Policy" if
     it needs an API the control plane lacks.
4. **Control-plane URLs move to `/app/...`** for the current organisation
   (organisation chosen by switcher and cookie). Every old
   `/dashboard/orgs/{org}/...` URL redirects (308, query kept). Navigation
   uses the canonical names: Overview, Policies, Identities, Sessions,
   Gateways, Audit, Settings.

## Palette (tokens)

Canonical colours (hex only in the `visual.css` token block):

| token | hex | role |
|---|---|---|
| `--snow` | `#F4F3ED` | light background and surfaces |
| `--ice` | `#E5F2EE` | light secondary surface |
| `--mint` | `#72C7A5` | active / granted / primary action / selected |
| `--pine` | `#183C35` | dark infrastructure, headers, footer, strong text |
| `--forest` | `#28584C` | secondary infrastructure |
| `--earth` | `#795C43` | gateway / network edge |
| `--slate` | `#66736F` | neutral |
| `--deny` | restrained red, chosen for contrast | denial / warning only |

The library derives the extra steps it needs, such as dark surfaces from
Pine, lighter and darker mint, and ink variants for small text. Every
derived value is defined in the token block. A **contrast test** parses
the tokens and asserts ≥4.5:1 for every text/background role pair in both
themes. Canonical colours that fail as small text (Mint on Snow, Slate on
Snow at 4.48:1) are used as fills, borders and lines, never as small text.

## Status wording (canonical §9)

Customer-facing views say **Access granted / Access denied**. Policy
editors, the audit's technical detail and the CLI's policy tooling may say
**ALLOW / DENY**. `DecisionBadge` supports both wordings; `Status` gains
`Granted`/`Denied`.

## Library additions (supersede the old Phase 2/3 lists)

**Product components (HTML/CSS):**
- `PolicyCard`, `PolicySummary`, and `PolicyExplorer`. The explorer is a
  WHO/WHAT/WHERE/LIMITS stack ending in a decision, with example data
  (marketing) or live data (dashboard).
- `DecisionExplainer`: per dimension ✓/✕ (identity, application,
  destination, limits, policy active), then Access granted/denied and the
  reason, per canonical §43/§71. It is built from the policy engine's
  existing explanation steps.
- `DecisionCard`, `AuditEventCard`, `SessionCard` (with a remaining-time
  bar), `SessionTimeline` (canonical §38/§50), `GatewayCard`, `HealthCard`,
  `IdentityCard`, `PolicyDiff` (+ added / − removed / ~ changed, canonical
  §92), `EmptyState` (canonical §51 copy), `CodeExample` (Pine background,
  mint highlights, `data-copy`), and a `Planned` marker.

**Diagrams (Flow/Boundary compositions and inline SVG):**
- The public set (canonical §61): identity→policy→access; WHO→LIMITS;
  traditional vs SkiMasque; access lifecycle; policy decision; GitHub
  Actions; developer CLI; gateway; customer VPC; deployment models (run
  markers with the technical name always shown); security layers; control
  plane / data plane; MASQUE stack; multiple gateways; audit flow.
- The control-plane set (canonical §62): identity flow, session flow,
  gateway topology, organisation topology. The "live" variants render real
  data; interactive inspection is Planned.
- Alpine motifs: contour background, mountain silhouette, route lines,
  trail/elevation markers.
- Motion: an active-session pulse along a path, the authorization reveal,
  and expiry fade, all disabled under `prefers-reduced-motion`.

**Site chrome:** navigation (desktop groups; a mobile menu built with
`<details>`, no JS), footer (Pine background with contours and a mountain
silhouette), hero, feature grid, CTA band, comparison table, FAQ
(`<details>`), and deployment run cards.

## Public website

`sitegen` renders the canonical information architecture (§74) to static
HTML under `site/`, which replaces the hand-written `site/index.html`. The
pages are `/`, how-it-works, identities, policies, ci-cd, developers,
compare, deployment, gateways, security, architecture, technology/masque,
use-cases, open-source, pricing, docs, faq, trust, about, contact and
status.

The content is the canonical spec's, with these conditions:
- Planned markers per decision 3.
- `docs` is an index linking to the repository's existing `docs/*.md`.
- `status` shows static text and makes no live claims.
- `contact` is a `mailto:` link or form markup with no working backend; a
  working form is Planned.

GitHub Pages publishes `site/` as today. The `connect` Action page adopts
the palette and chrome.

## Control plane

It moves to `/app` with the canonical navigation (§29, §75), rebuilt on
library components:
- **Overview:** summary line, active sessions, recent access, gateway
  health, and the identity→policy→session→gateway→network diagram.
- **Policies:** a table, the detail page with the Explorer and recent
  decisions, and the wizard with a live access-request summary on every
  step (server-rendered per step).
- **Identities:** a list with source and last seen (from audit), and the
  detail page.
- **Sessions:** the derived Active access, relabelled. The session detail
  page has a timeline built from the audit event and the policy's limits.
- **Gateways:** cards and detail. Egress IP is Planned; health comes from
  heartbeats.
- **Audit:** events with the DecisionExplainer, and denied requests with
  "applicable policies" (§42).
- **Settings:** organisation, team, identity, security, API, billing.
- **Developer view and onboarding** (§47, §79–§82), with anything lacking
  an API marked Planned.

Dangerous actions show their impact (§52). Actions that exist today, such
as deleting a gateway or discarding a draft, get impact dialogs. Disabling
a policy is Planned. Data rules from the dashboard spec still hold: derived
views are labelled, and nothing is fabricated.

## CLI

The palette constants in `skimasque-cli`'s `style` module move to the new
Mint, Slate, deny and warning values. Its output wording already follows
§89/§90. `exec` is Planned and is not built by this project.

## Delivery (supersedes the old plans 2–4)

| # | Plan | Repo |
|---|---|---|
| 2 | Palette v2 + product components (+ the parked `.v-flow-wrap` width fix and a 980px threshold review) | skimasque `feat/visual-library` |
| 3 | Diagrams and Alpine motifs (public + control-plane sets, motion) | skimasque |
| 4 | Site chrome + public website pages via sitegen | skimasque (+ connect page) |
| 5 | Control plane on the library at `/app` (includes the parked Task 8 identities work and the paused dashboard tasks) | control `style/alpine-ui` |
| 6 | CLI palette + retire superseded styling (the `style/alpine` site restyle is superseded by plan 4; its CLI commits stay) | skimasque, connect |

Each plan is written, reviewed and executed in order. A plan is written
only after the previous one lands, so it builds on real APIs.
