//! Public guides rendered into the website from the canonical source.

use super::{doc, DocLinks};
use crate::site::Page;
use crate::{Component, Hero, Section, SitePage};
use pulldown_cmark::{html, Event, Options, Parser, Tag};
use stucco_core::Render;

/// `(section, [(link label, file in docs/)])`, mirroring the canonical nav groups.
const INDEX: &[(&str, &[(&str, &str)])] = &[
    (
        "Getting Started",
        &[("Getting started", "getting-started.md")],
    ),
    (
        "Concepts",
        &[
            ("Policies", "policies.md"),
            ("Control plane", "control-plane.md"),
            ("Gateways", "gateways.md"),
            ("Control protocol", "protocol.md"),
        ],
    ),
    ("CI/CD", &[("GitHub Actions", "github-actions.md")]),
    (
        "Developers",
        &[("CLI", "cli.md"), ("Coding agents", "agents.md")],
    ),
    (
        "Deployment",
        &[
            ("Deployment modes", "deployment-modes.md"),
            ("Gateways", "gateways.md"),
            ("Self-hosting", "self-hosting.md"),
        ],
    ),
    (
        "Security",
        &[
            ("Security model", "security.md"),
            ("Threat model", "threat-model.md"),
        ],
    ),
    (
        "Reference",
        &[
            ("CLI", "cli.md"),
            ("Configuration", "configuration.md"),
            ("Policies", "policies.md"),
        ],
    ),
    (
        "Architecture",
        &[
            ("Architecture", "architecture.md"),
            ("Control protocol", "protocol.md"),
        ],
    ),
    (
        "Contributing",
        &[
            ("Development", "development.md"),
            ("Troubleshooting", "troubleshooting.md"),
        ],
    ),
];

pub fn page() -> Page {
    let hero = Hero::new("Documentation").lead(
        "Guides and reference for connecting workloads, writing policies and operating gateways.",
    );

    let mut page = SitePage::new(
        "../",
        "docs",
        "Documentation · SkiMasque",
        "Documentation index for SkiMasque: getting started, concepts, CI/CD, deployment, security, reference and architecture.",
    )
    .push(&hero);

    for (i, (title, links)) in INDEX.iter().enumerate() {
        let links: Vec<(&str, String)> = links.iter().map(|(l, f)| (*l, doc(f))).collect();
        let mut s = Section::new(*title).push(&DocLinks::new(&links));
        if i % 2 == 1 {
            s = s.alt();
        }
        page = page.push(&s);
    }

    page = page.push(
        &Section::new("Manage your organisation").push(&DocLinks::new(&[
            (
                "Create and publish policies",
                "https://control.skimasque.com/app/policies".into(),
            ),
            (
                "Register a gateway",
                "https://control.skimasque.com/app/gateways".into(),
            ),
            (
                "Review sessions",
                "https://control.skimasque.com/app/sessions".into(),
            ),
            (
                "Inspect audit decisions",
                "https://control.skimasque.com/app/audit".into(),
            ),
            (
                "Verify GitHub owners",
                "https://control.skimasque.com/app/settings/identity".into(),
            ),
        ])),
    );

    Page {
        path: "docs/index.html",
        contents: page.html().as_str().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_linked_guide_exists_in_the_docs_directory() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let mut n = 0;
        for (_, links) in INDEX {
            for (_, file) in *links {
                assert!(dir.join(file).is_file(), "docs/{file} does not exist");
                n += 1;
            }
        }
        assert!(n >= 10);
        let p = page();
        assert_eq!(p.path, "docs/index.html");
        for (_, links) in INDEX {
            for (_, file) in *links {
                assert!(p.contents.contains(&doc(file)), "{file} not linked");
            }
        }
    }

    #[test]
    fn all_guides_are_hosted_and_links_stay_on_site() {
        let guides = guides();
        for (_, links) in INDEX {
            for (_, file) in *links {
                let path = format!("docs/{}/index.html", file.trim_end_matches(".md"));
                assert!(guides.iter().any(|p| p.path == path), "missing {path}");
            }
        }
        for guide in guides {
            assert!(!guide.contents.contains("blob/main/docs/"));
            assert!(!guide
                .contents
                .contains("href=\"https://github.com/skimasque-dev/skimasque/blob/main/docs/"));
            assert_eq!(guide.contents.matches("<h1").count(), 1);
        }
    }
}

struct Guide<'a> {
    path: &'static str,
    source: &'a str,
}
const GUIDES: &[Guide] = &[
    Guide {
        path: "docs/agents/index.html",
        source: include_str!("../../../../../docs/agents.md"),
    },
    Guide {
        path: "docs/architecture/index.html",
        source: include_str!("../../../../../docs/architecture.md"),
    },
    Guide {
        path: "docs/cli/index.html",
        source: include_str!("../../../../../docs/cli.md"),
    },
    Guide {
        path: "docs/configuration/index.html",
        source: include_str!("../../../../../docs/configuration.md"),
    },
    Guide {
        path: "docs/control-plane/index.html",
        source: include_str!("../../../../../docs/control-plane.md"),
    },
    Guide {
        path: "docs/deployment-modes/index.html",
        source: include_str!("../../../../../docs/deployment-modes.md"),
    },
    Guide {
        path: "docs/development/index.html",
        source: include_str!("../../../../../docs/development.md"),
    },
    Guide {
        path: "docs/gateways/index.html",
        source: include_str!("../../../../../docs/gateways.md"),
    },
    Guide {
        path: "docs/getting-started/index.html",
        source: include_str!("../../../../../docs/getting-started.md"),
    },
    Guide {
        path: "docs/github-actions/index.html",
        source: include_str!("../../../../../docs/github-actions.md"),
    },
    Guide {
        path: "docs/policies/index.html",
        source: include_str!("../../../../../docs/policies.md"),
    },
    Guide {
        path: "docs/protocol/index.html",
        source: include_str!("../../../../../docs/protocol.md"),
    },
    Guide {
        path: "docs/security/index.html",
        source: include_str!("../../../../../docs/security.md"),
    },
    Guide {
        path: "docs/self-hosting/index.html",
        source: include_str!("../../../../../docs/self-hosting.md"),
    },
    Guide {
        path: "docs/threat-model/index.html",
        source: include_str!("../../../../../docs/threat-model.md"),
    },
    Guide {
        path: "docs/troubleshooting/index.html",
        source: include_str!("../../../../../docs/troubleshooting.md"),
    },
];

struct GuideBody {
    body: String,
}

impl Render for GuideBody {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<article class="v-guide">"#);
        stucco_core::Raw::trusted(&self.body).render(cx);
        crate::render::markup(cx, r#"</article>"#);
    }
}
impl Component for GuideBody {}

fn guide_link(url: &str) -> String {
    let (path, fragment) = url.split_once('#').unwrap_or((url, ""));
    let suffix = if fragment.is_empty() {
        String::new()
    } else {
        format!("#{fragment}")
    };
    let name = path
        .strip_prefix("https://github.com/skimasque-dev/skimasque/blob/main/docs/")
        .unwrap_or(path);
    if name == "README.md" {
        return format!("../{suffix}");
    }
    if !name.contains('/') && name.ends_with(".md") {
        return format!("../{}/{suffix}", name.trim_end_matches(".md"));
    }
    if let Some(file) = name.strip_prefix("../") {
        return format!("https://github.com/skimasque-dev/skimasque/blob/main/{file}{suffix}");
    }
    url.to_owned()
}

pub fn guides() -> Vec<Page> {
    GUIDES.iter().map(render_guide).collect()
}

fn render_guide(guide: &Guide) -> Page {
    // Markdown is embedded from the checkout, whose line endings vary by OS.
    // Normalize before parsing so multiline text emits deterministic HTML.
    let source = guide.source.replace("\r\n", "\n");
    let (title, body) = source.split_once('\n').expect("guide has a title");
    let title = title.trim().trim_start_matches("# ");
    let mut events: Vec<_> =
        Parser::new_ext(body, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH).collect();
    let mut ids = std::collections::HashMap::new();
    for i in 0..events.len() {
        if matches!(events[i], Event::Start(Tag::Heading { .. })) {
            let mut text = String::new();
            for event in &events[i + 1..] {
                match event {
                    Event::End(pulldown_cmark::TagEnd::Heading(_)) => break,
                    Event::Text(t) | Event::Code(t) => text.push_str(t),
                    _ => {}
                }
            }
            let slug: String = text
                .to_lowercase()
                .chars()
                .filter_map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        Some(c)
                    } else if c.is_whitespace() {
                        Some('-')
                    } else {
                        None
                    }
                })
                .collect();
            let count = ids.entry(slug.clone()).or_insert(0);
            let id = if *count == 0 {
                slug
            } else {
                format!("{slug}-{count}")
            };
            *count += 1;
            if let Event::Start(Tag::Heading { id: heading_id, .. }) = &mut events[i] {
                *heading_id = Some(id.into());
            }
        }
    }
    for event in &mut events {
        match event {
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                *dest_url = guide_link(dest_url).into();
            }
            // Guide source is text content; do not admit raw HTML into the site.
            Event::Html(text) | Event::InlineHtml(text) => *event = Event::Text(text.clone()),
            _ => {}
        }
    }
    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());
    let content = GuideBody { body };
    let console = match guide.path {
        "docs/policies/index.html" => Some(("Manage policies", "policies")),
        "docs/gateways/index.html" => Some(("Register a gateway", "gateways")),
        "docs/agents/index.html" => Some(("Review agent sessions", "sessions")),
        "docs/cli/index.html" => Some(("Open your developer account", "developer")),
        "docs/github-actions/index.html" => Some(("Verify GitHub owners", "settings/identity")),
        "docs/troubleshooting/index.html" => Some(("Inspect audit decisions", "audit")),
        "docs/control-plane/index.html" => Some(("Open the console", "")),
        _ => None,
    };
    let mut hero = Hero::new(title).cta(crate::Cta::secondary("All guides", "../"));
    if let Some((label, path)) = console {
        hero = hero.cta(crate::Cta::primary(
            label,
            format!("https://control.skimasque.com/app/{path}"),
        ));
    }
    Page {
        path: guide.path,
        contents: SitePage::new("../../", "docs", &format!("{title} · SkiMasque"), title)
            .push(&hero)
            .push(&Section::new("Guide").push(&content))
            .html()
            .as_str()
            .to_owned(),
    }
}

#[cfg(test)]
mod line_ending_tests {
    use super::*;

    #[test]
    fn guide_html_is_identical_for_lf_and_crlf() {
        for guide in GUIDES {
            let lf = guide.source.replace("\r\n", "\n");
            let crlf = lf.replace('\n', "\r\n");
            let lf_guide = Guide {
                path: guide.path,
                source: &lf,
            };
            let crlf_guide = Guide {
                path: guide.path,
                source: &crlf,
            };
            assert_eq!(
                render_guide(&lf_guide).contents,
                render_guide(&crlf_guide).contents,
                "{}",
                guide.path
            );
        }
    }
}
