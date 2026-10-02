//! The generated website: the component gallery and the public pages.
//! `sitegen` writes [`pages()`] under `site/`; `sitegen --check` compares.

mod gallery;
pub mod pages;

pub const REPO_URL: &str = "https://github.com/skimasque-dev/skimasque";
pub const DOCS_BASE: &str = "../docs/";
pub const ISSUES_URL: &str = "https://github.com/skimasque-dev/skimasque/issues";

/// Files under `site/` that are written by hand and never reported as orphans.
pub(crate) const KEEP: &[&str] = &["favicon.svg"];

pub struct Page {
    pub path: &'static str,
    pub contents: String,
}

pub fn pages() -> Vec<Page> {
    let mut v = gallery::pages();
    v.push(Page {
        path: "assets/visual.css",
        contents: crate::CSS.to_owned(),
    });
    v.extend(pages::all());
    // No sitemap.xml and no `Sitemap:` line: both need an absolute base URL,
    // which a project site does not have.
    v.push(Page {
        path: "robots.txt",
        contents: "User-agent: *\nAllow: /\n".to_owned(),
    });
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
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.path().is_dir() {
            collect_files(&entry.path(), &rel, out);
        } else {
            out.push(rel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generated public pages: every HTML page except the component gallery.
    fn html_pages() -> Vec<Page> {
        pages()
            .into_iter()
            .filter(|p| p.path.ends_with(".html") && !p.path.starts_with("components/"))
            .collect()
    }

    #[test]
    fn the_public_pages_are_registered() {
        let paths: Vec<&str> = html_pages().iter().map(|p| p.path).collect();
        assert!(!paths.is_empty());
        for want in [
            "index.html",
            "how-it-works/index.html",
            "identities/index.html",
        ] {
            assert!(paths.contains(&want), "missing page {want}");
        }
    }

    #[test]
    fn rendering_is_deterministic() {
        let a: Vec<_> = pages().into_iter().map(|p| (p.path, p.contents)).collect();
        let b: Vec<_> = pages().into_iter().map(|p| (p.path, p.contents)).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn the_shared_stylesheet_is_published_for_the_pages() {
        let css = pages()
            .into_iter()
            .find(|p| p.path == "assets/visual.css")
            .expect("assets/visual.css")
            .contents;
        assert_eq!(css, crate::CSS);
    }

    #[test]
    fn every_page_has_one_h1_a_title_a_description_and_nav() {
        for p in html_pages() {
            let s = &p.contents;
            assert_eq!(s.matches("<h1").count(), 1, "{}: exactly one h1", p.path);
            assert!(
                s.contains("<title>") && s.contains(r#"name="description""#),
                "{}",
                p.path
            );
            assert!(
                s.contains("v-site-header")
                    && s.contains("v-site-footer")
                    && s.contains(r#"<main id="main">"#),
                "{}",
                p.path
            );
        }
    }

    /// Relative `href`/`src` values found in `html`.
    fn relative_urls(html: &str) -> Vec<String> {
        let mut out = Vec::new();
        for attr in ["href=\"", "src=\""] {
            for chunk in html.split(attr).skip(1) {
                let url = chunk.split('"').next().unwrap_or("");
                let external = url.contains("://")
                    || url.starts_with("mailto:")
                    || url.starts_with("data:")
                    || url.starts_with('#');
                if !url.is_empty() && !external {
                    out.push(url.to_owned());
                }
            }
        }
        out
    }

    /// Resolve `url` against the directory of `page_path`; `None` if it climbs
    /// out of the site root. A trailing `/` (or an empty path) means the
    /// directory's `index.html`.
    fn resolve(page_path: &str, url: &str) -> Option<String> {
        let url = url.split(['#', '?']).next().unwrap_or("");
        let mut parts: Vec<&str> = page_path.split('/').collect();
        parts.pop(); // the page's own file name
        for seg in url.split('/') {
            match seg {
                ".." => {
                    parts.pop()?;
                }
                "." | "" => {}
                s => parts.push(s),
            }
        }
        let mut target = parts.join("/");
        if url.is_empty() || url.ends_with('/') {
            if !target.is_empty() {
                target.push('/');
            }
            target.push_str("index.html");
        }
        Some(target)
    }

    fn resolves(all: &[&str], target: &str) -> bool {
        all.contains(&target) || KEEP.contains(&target)
    }

    #[test]
    fn link_resolution_handles_directories_and_parents() {
        assert_eq!(
            resolve("index.html", "how-it-works/").unwrap(),
            "how-it-works/index.html"
        );
        assert_eq!(
            resolve("a/index.html", "../assets/visual.css").unwrap(),
            "assets/visual.css"
        );
        assert_eq!(resolve("a/index.html", "../").unwrap(), "index.html");
        assert_eq!(resolve("a/index.html", "./").unwrap(), "a/index.html");
        assert_eq!(
            resolve("a/index.html", "../favicon.svg#x").unwrap(),
            "favicon.svg"
        );
        assert!(resolve("index.html", "../x").is_none());
    }

    /// A heading is at most one level deeper than the one before it.
    #[test]
    fn heading_levels_never_skip_downward() {
        for p in html_pages() {
            let mut prev = 0usize;
            for chunk in p.contents.split("<h").skip(1) {
                let mut cs = chunk.chars();
                let (Some(d), Some(next)) = (cs.next(), cs.next()) else {
                    continue;
                };
                if !('1'..='6').contains(&d) || !(next == ' ' || next == '>') {
                    continue;
                }
                let level = d.to_digit(10).unwrap() as usize;
                assert!(
                    level <= prev + 1,
                    "{}: h{level} follows h{prev} ({})",
                    p.path,
                    chunk.chars().take(60).collect::<String>()
                );
                prev = level;
            }
        }
    }

    /// Every relative href/src on a generated public page resolves to a
    /// generated page or asset, or to a hand-written file in KEEP.
    #[test]
    fn every_relative_link_resolves() {
        let owned = pages();
        let all: Vec<&str> = owned.iter().map(|p| p.path).collect();
        for p in html_pages() {
            for url in relative_urls(&p.contents) {
                let target = resolve(p.path, &url)
                    .unwrap_or_else(|| panic!("{}: link {url:?} leaves the site", p.path));
                assert!(
                    resolves(&all, &target),
                    "{}: broken link {url:?} -> {target:?}",
                    p.path
                );
            }
        }
    }

    #[test]
    fn all_twenty_one_canonical_routes_are_generated() {
        let owned = pages();
        let paths: Vec<&str> = owned.iter().map(|p| p.path).collect();
        for r in [
            "",
            "how-it-works",
            "identities",
            "policies",
            "ci-cd",
            "developers",
            "compare",
            "deployment",
            "gateways",
            "security",
            "architecture",
            "technology/masque",
            "use-cases",
            "open-source",
            "pricing",
            "docs",
            "faq",
            "trust",
            "about",
            "contact",
            "status",
        ] {
            let want = if r.is_empty() {
                "index.html".to_owned()
            } else {
                format!("{r}/index.html")
            };
            assert!(paths.contains(&want.as_str()), "missing route /{r}");
        }
    }

    #[test]
    fn robots_txt_allows_everything_and_names_no_sitemap() {
        let robots = pages()
            .into_iter()
            .find(|p| p.path == "robots.txt")
            .expect("robots.txt")
            .contents;
        assert_eq!(robots, "User-agent: *\nAllow: /\n");
        assert!(!robots.contains("Sitemap:"));
        assert!(!pages().iter().any(|p| p.path == "sitemap.xml"));
    }

    #[test]
    fn every_public_page_is_reachable_from_the_homepage_by_relative_links() {
        let all = html_pages();
        let mut seen = vec!["index.html".to_owned()];
        let mut queue = vec!["index.html".to_owned()];
        while let Some(cur) = queue.pop() {
            let page = all
                .iter()
                .find(|p| p.path == cur)
                .unwrap_or_else(|| panic!("no page {cur}"));
            for url in relative_urls(&page.contents) {
                let Some(t) = resolve(&cur, &url) else {
                    continue;
                };
                if t.ends_with(".html") && !t.starts_with("components/") && !seen.contains(&t) {
                    seen.push(t.clone());
                    queue.push(t);
                }
            }
        }
        assert_eq!(all.len(), 37);
        for p in &all {
            assert!(seen.contains(&p.path.to_owned()), "unreachable: {}", p.path);
        }
    }

    /// `html` with every planned block removed. Blocks are `<div>`s, so the
    /// end is found by matching nesting depth; a block inside another block
    /// panics (planned blocks are siblings, never nested).
    fn strip_planned_blocks(html: &str) -> String {
        const START: &str = "<div class=\"v-planned-block\"";
        let mut s = html.to_owned();
        while let Some(start) = s.find(START) {
            let mut depth = 0usize;
            let mut i = start;
            let end = loop {
                let open = s[i..].find("<div").map(|p| i + p);
                let close = s[i..].find("</div>").map(|p| i + p);
                match (open, close) {
                    (Some(o), c) if c.is_none_or(|c| o < c) => {
                        if o > start {
                            assert!(!s[o..].starts_with(START), "planned blocks must not nest");
                        }
                        depth += 1;
                        i = o + 4;
                    }
                    (_, Some(c)) => {
                        depth -= 1;
                        i = c + 6;
                        if depth == 0 {
                            break i;
                        }
                    }
                    _ => panic!("unclosed planned block"),
                }
            };
            s.replace_range(start..end, "");
        }
        s
    }

    #[test]
    fn the_planned_block_stripper_matches_nesting_and_rejects_nested_blocks() {
        let html = r#"a<div class="v-planned-block" role="note"><div>x</div><p>exec</p></div>b<div class="v-planned-block"><div></div></div>c"#;
        assert_eq!(strip_planned_blocks(html), "abc");
    }

    #[test]
    #[should_panic(expected = "must not nest")]
    fn a_nested_planned_block_is_rejected() {
        strip_planned_blocks(
            r#"<div class="v-planned-block"><div class="v-planned-block"></div></div>"#,
        );
    }

    #[test]
    fn no_page_has_an_email_link_a_form_or_an_input() {
        for p in html_pages() {
            for banned in ["mailto:", "<form", "<input", "<textarea", "type=\"submit\""] {
                assert!(!p.contents.contains(banned), "{}: {banned}", p.path);
            }
            assert_eq!(
                p.contents.matches("<select").count(),
                1,
                "only the theme preference is a selector: {}",
                p.path
            );
            assert!(p.contents.contains("<select id=\"site-theme\">"));
        }
    }

    #[test]
    fn a_root_absolute_href_is_never_used() {
        for p in html_pages() {
            for url in relative_urls(&p.contents) {
                assert!(!url.starts_with('/'), "{}: root-absolute {url:?}", p.path);
            }
        }
    }

    /// Every absolute link (and any `//` host-relative one) targets one of
    /// the hosts the site is allowed to link to.
    fn disallowed_absolute_links(html: &str) -> Vec<String> {
        const ALLOWED: [&str; 5] = [
            "https://github.com/skimasque-dev/skimasque",
            "https://github.com/skimasque-dev/connect",
            "https://control.skimasque.com",
            "https://gateway.skimasque.com",
            "https://crates.io/crates/",
        ];
        let mut bad = Vec::new();
        for attr in ["href=\"", "src=\""] {
            for chunk in html.split(attr).skip(1) {
                let url = chunk.split('"').next().unwrap_or("");
                let absolute = url.contains("://") || url.starts_with("//");
                if absolute && !ALLOWED.iter().any(|a| url.starts_with(a)) {
                    bad.push(url.to_owned());
                }
            }
        }
        bad
    }

    #[test]
    fn absolute_links_only_go_to_allowed_hosts() {
        for p in html_pages() {
            // The head loads the web font; only the body links are checked.
            let body = p.contents.split("<body").nth(1).unwrap_or("");
            let bad = disallowed_absolute_links(body);
            assert!(bad.is_empty(), "{}: {bad:?}", p.path);
        }
        assert_eq!(
            disallowed_absolute_links(
                r#"<a href="https://evil.example/">x</a><a href="//evil.example/">y</a><a href="https://control.skimasque.com">z</a>"#
            ),
            vec!["https://evil.example/", "//evil.example/"]
        );
    }

    fn page_at<'a>(all: &'a [Page], path: &str) -> &'a str {
        &all.iter().find(|p| p.path == path).expect(path).contents
    }

    #[test]
    fn final_review_content_and_chrome_fixes_hold() {
        let all = html_pages();
        let arch = page_at(&all, "architecture/index.html");
        assert!(arch.contains("keeps enforcing its cached policy"));
        assert!(!arch.contains("while the control plane is unreachable"));
        assert_eq!(
            arch.matches("SkiMasque operates the control plane").count(),
            1
        );
        for path in ["policies/index.html", "how-it-works/index.html"] {
            assert!(
                !page_at(&all, path).contains("100 Mbps · us-west")
                    && !page_at(&all, path).contains("100Mbps · us-west"),
                "{path}: a region is not a policy limit"
            );
        }
        assert!(!page_at(&all, "policies/index.html").contains("Egress region"));
        let pricing = page_at(&all, "pricing/index.html");
        assert!(pricing.contains("Try SkiMasque") && pricing.contains(r#"href="../contact/""#));
        assert!(pricing.contains("Talk to Us") && pricing.contains("v-btn v-btn-quiet"));
        assert!(page_at(&all, "index.html").contains("skimasque exec --policy production"));
        // titles use the nav labels' capitalisation
        for (path, title) in [
            (
                "how-it-works/index.html",
                "<title>How It Works · SkiMasque</title>",
            ),
            ("compare/index.html", "<title>Compare · SkiMasque</title>"),
            (
                "open-source/index.html",
                "<title>Open Source · SkiMasque</title>",
            ),
        ] {
            assert!(page_at(&all, path).contains(title), "{path}");
        }
        // one dropdown open at a time; the phone menu is a labelled landmark
        let home = page_at(&all, "index.html");
        assert_eq!(
            home.matches(r#"<details class="v-nav-group" name="site-nav">"#)
                .count(),
            4
        );
        assert!(home
            .contains(r#"<nav class="v-menu-nav" aria-label="Primary"><details class="v-menu">"#));
        assert!(
            home.contains(r#"class="v-brand" href="./""#)
                || home.contains(r#"href="./">SkiMasque"#)
        );
        assert!(
            !home.contains("<aside"),
            "no unlabelled complementary landmarks"
        );
        let dev = page_at(&all, "developers/index.html");
        assert!(dev.contains(r#"<div class="v-planned-block" role="note" aria-label="Planned: "#));
        assert!(crate::CSS.contains("scroll-padding-top"));
    }
    #[test]
    fn a_broken_link_is_detected() {
        let all = ["index.html", "assets/visual.css", "a/index.html"];
        let html = r#"<a href="a/">ok</a><a href="missing/">bad</a><link href="assets/visual.css"><a href="https://x.y/">e</a>"#;
        let bad: Vec<String> = relative_urls(html)
            .into_iter()
            .filter(|u| !resolves(&all, &resolve("index.html", u).unwrap()))
            .collect();
        assert_eq!(bad, vec!["missing/"]);
    }

    #[test]
    fn honesty_and_voice_rules_hold_on_every_page() {
        for p in html_pages() {
            // Reference guides discuss guarantees and their limits explicitly.
            if p.path.starts_with("docs/") && p.path != "docs/index.html" {
                continue;
            }
            let s = strip_planned_blocks(&p.contents);
            let lower = s.to_lowercase();
            for banned in [
                "zero trust",
                "zero-trust",
                "buy now",
                "guarantee",
                "temporary vpn",
                "vpn endpoint",
                "next-generation",
                "military-grade",
            ] {
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
        std::fs::write(dir.join("components/visual.css"), "changed").unwrap();
        assert_eq!(stale(&dir), vec!["components/visual.css"]);
        std::fs::remove_file(dir.join("components/index.html")).unwrap();
        assert_eq!(stale(&dir).len(), 2);
        std::fs::write(dir.join("components/visual.css"), crate::CSS).unwrap();
        std::fs::write(
            dir.join("components/index.html"),
            pages()[0].contents.clone(),
        )
        .unwrap();
        assert!(stale(&dir).is_empty());
        std::fs::create_dir_all(dir.join("old")).unwrap();
        std::fs::write(dir.join("old/gone.html"), "orphan").unwrap();
        assert_eq!(stale(&dir), vec!["old/gone.html"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
