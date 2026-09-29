//! The generated website: the component gallery and the public pages.
//! `sitegen` writes [`pages()`] under `site/`; `sitegen --check` compares.

mod gallery;
pub mod pages;

pub const REPO_URL: &str = "https://github.com/skimasque-dev/skimasque";
pub const DOCS_BASE: &str = "https://github.com/skimasque-dev/skimasque/blob/main/docs/";
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

    /// Routes the navigation already links to that later page tasks still
    /// owe. Each page task removes its routes from this list. Task 9 (the
    /// last page task) deletes this constant, the two tests below that use
    /// it, and its use in `every_relative_link_resolves`.
    const PENDING_ROUTES: &[&str] = &[
        "about/index.html",
        "contact/index.html",
        "status/index.html",
    ];

    #[test]
    fn pending_routes_are_not_yet_generated() {
        let owned = pages();
        for r in PENDING_ROUTES {
            assert!(
                !owned.iter().any(|p| p.path == *r),
                "{r} exists now: remove it from PENDING_ROUTES"
            );
        }
    }

    #[test]
    fn pending_routes_are_all_linked_from_the_chrome() {
        let home = pages()
            .into_iter()
            .find(|p| p.path == "index.html")
            .expect("homepage")
            .contents;
        for r in PENDING_ROUTES {
            let dir = r.trim_end_matches("index.html");
            assert!(
                home.contains(&format!("href=\"{dir}\"")),
                "{r} is not linked from the nav/footer: drop it from PENDING_ROUTES"
            );
        }
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
    /// generated page or asset, or to a hand-written file in KEEP (or is a
    /// route still pending).
    #[test]
    fn every_relative_link_resolves() {
        let owned = pages();
        let all: Vec<&str> = owned.iter().map(|p| p.path).collect();
        for p in html_pages() {
            for url in relative_urls(&p.contents) {
                let target = resolve(p.path, &url)
                    .unwrap_or_else(|| panic!("{}: link {url:?} leaves the site", p.path));
                assert!(
                    resolves(&all, &target) || PENDING_ROUTES.contains(&target.as_str()),
                    "{}: broken link {url:?} -> {target:?}",
                    p.path
                );
            }
        }
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
            let mut s = p.contents.clone();
            // Planned blocks may describe what is not built yet, including `skimasque exec`.
            while let Some(start) = s.find("<aside class=\"v-planned-block\"") {
                let end = s[start..]
                    .find("</aside>")
                    .map(|e| start + e + 8)
                    .expect("closed planned block");
                s.replace_range(start..end, "");
            }
            let lower = s.to_lowercase();
            assert!(
                !lower.contains("skimasque exec"),
                "{}: `skimasque exec` outside a planned block",
                p.path
            );
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
