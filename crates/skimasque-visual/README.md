# SkiMasque visual components

The dashboard and public website share SkiMasque's visual language. Rendering
uses `stucco-core` 0.2.1, the rendering core of
[stucco](https://github.com/mcaveniathor/stucco). The custom diagrams, CSS tokens,
icon sprite and progressive enhancement scripts belong to this crate.

Every component implements stucco's `Render` trait. Compose it directly with
stucco elements, layout components or pages:

```rust
use skimasque_visual::{Component, Node, NodeKind};
use stucco_core::{el, to_html};

let node = Node::new(NodeKind::Gateway).label("Production gateway");
let section = el::section().child(node);
let html = to_html(&section);

// Existing callers can still obtain an owned, nestable fragment.
let fragment = Node::new(NodeKind::Service).html();
assert!(html.contains("Production gateway"));
assert!(fragment.as_str().contains("Service"));
```

Serve `skimasque_visual::CSS` and emit `Icons` once per document for diagrams.
When composing into a stucco `Page`, add SkiMasque's stylesheet alongside the
stucco bundle's assets. `SitePage` owns the public site's document shell and
links its relative stylesheet; `sitegen` writes all site assets automatically.

`Component` now extends `Render`, replacing the former Askama `Template` bound.
Downstream components that nest in a `Flow`, `Boundary` or `SitePage` must
implement `Render` and `Component`. Components escape text and attributes through
stucco; only source markup, embedded scripts, repository Markdown and rendered
`Html` fragments are trusted. `html()` materializes HTML only; use direct stucco
composition when a component requires stucco assets collected in the render
context.

Edit rendering in `src/`. Fixed SVG/HTML and scripts live in `static/`; there
are no runtime or compile-time HTML templates. The site content is authored in
`src/site/pages/`.

```console
cargo test -p skimasque-visual --features site
cargo clippy -p skimasque-visual --all-targets --features site -- -D warnings
node --test crates/skimasque-visual/tests/*.test.cjs
cargo run -p skimasque-visual --features site --bin sitegen
cargo run -p skimasque-visual --features site --bin sitegen -- --check
```
