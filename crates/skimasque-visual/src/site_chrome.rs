//! Public-site chrome: the navigation, the footer and the page shell.
//! Links are relative (the site is served from a project path), so every
//! component takes the page's `root` prefix ("", "../", "../../").

use stucco_core::Render;

use crate::{Component, Contour, Html, Icons, Mountain, Planned};

pub const SIGN_IN_URL: &str = "https://control.skimasque.com";

pub const NAV_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Product",
        &[
            ("How It Works", "how-it-works"),
            ("Identities", "identities"),
            ("Policies", "policies"),
            ("CI/CD", "ci-cd"),
            ("Developers", "developers"),
            ("Security", "security"),
            ("Deployment", "deployment"),
            ("Gateways", "gateways"),
        ],
    ),
    ("Compare", &[("How SkiMasque Compares", "compare")]),
    (
        "Technology",
        &[
            ("Architecture", "architecture"),
            ("MASQUE", "technology/masque"),
        ],
    ),
    (
        "Resources",
        &[
            ("Documentation", "docs"),
            ("Use Cases", "use-cases"),
            ("Open Source", "open-source"),
            ("FAQ", "faq"),
            ("Trust", "trust"),
        ],
    ),
];

/// A directory-style relative link: `link("../", "policies")` -> `../policies/`.
pub fn link(root: &str, path: &str) -> String {
    if path.is_empty() {
        if root.is_empty() {
            "./".to_owned()
        } else {
            root.to_owned()
        }
    } else {
        format!("{root}{path}/")
    }
}

pub struct NavLink {
    pub label: &'static str,
    pub href: String,
    pub current: bool,
}

pub struct NavGroup {
    pub title: &'static str,
    pub links: Vec<NavLink>,
}

#[derive(Debug, Clone)]
pub struct SiteNav {
    pub root: String,
    pub current: String,
}

impl Render for SiteNav {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            "<header class=\"v-site-header\"><div class=\"v-wrap v-site-bar\"><a class=\"v-brand\"",
        );
        if self.home_current() {
            crate::render::markup(cx, r#" aria-current="page""#);
        }
        crate::render::markup(cx, r#" href=""#);
        crate::render::text(cx, &self.home());
        crate::render::markup(
            cx,
            "\">SkiMasque</a><nav class=\"v-site-nav\" aria-label=\"Primary\">",
        );
        for g in (self.groups()).iter() {
            crate::render::markup(
                cx,
                "<details class=\"v-nav-group\" name=\"site-nav\"><summary>",
            );
            crate::render::text(cx, &g.title);
            crate::render::markup(cx, r#"</summary><ul>"#);
            for l in g.links.iter() {
                crate::render::markup(cx, r#"<li><a href=""#);
                crate::render::text(cx, &l.href);
                crate::render::markup(cx, r#"""#);
                if l.current {
                    crate::render::markup(cx, r#" aria-current="page""#);
                }
                crate::render::markup(cx, r#">"#);
                crate::render::text(cx, &l.label);
                crate::render::markup(cx, r#"</a></li>"#);
            }
            crate::render::markup(cx, r#"</ul></details>"#);
        }
        let p = self.pricing();
        crate::render::markup(cx, r#"<a class="v-nav-top" href=""#);
        crate::render::text(cx, &p.href);
        crate::render::markup(cx, r#"""#);
        if p.current {
            crate::render::markup(cx, r#" aria-current="page""#);
        }
        crate::render::markup(cx, r#">"#);
        crate::render::text(cx, &p.label);
        crate::render::markup(
            cx,
            "</a></nav><div class=\"v-site-actions\"><a class=\"v-btn v-btn-quiet\" href=\"",
        );
        crate::render::text(cx, &self.sign_in());
        crate::render::markup(cx, r#"">Sign In</a><a class="v-btn" href=""#);
        crate::render::text(cx, &self.get_started());
        crate::render::markup(
            cx,
            r#"">Get Started</a></div><nav class="v-menu-nav" aria-label="Primary"><details class="v-menu"><summary>Menu</summary><div class="v-menu-panel">"#,
        );
        for g in (self.groups()).iter() {
            crate::render::markup(cx, r#"<p class="v-menu-title">"#);
            crate::render::text(cx, &g.title);
            crate::render::markup(cx, r#"</p><ul>"#);
            for l in g.links.iter() {
                crate::render::markup(cx, r#"<li><a href=""#);
                crate::render::text(cx, &l.href);
                crate::render::markup(cx, r#"""#);
                if l.current {
                    crate::render::markup(cx, r#" aria-current="page""#);
                }
                crate::render::markup(cx, r#">"#);
                crate::render::text(cx, &l.label);
                crate::render::markup(cx, r#"</a></li>"#);
            }
            crate::render::markup(cx, r#"</ul>"#);
        }
        crate::render::markup(
            cx,
            "<p class=\"v-menu-title\">Pricing</p><ul><li><a href=\"",
        );
        crate::render::text(cx, &p.href);
        crate::render::markup(cx, r#"""#);
        if p.current {
            crate::render::markup(cx, r#" aria-current="page""#);
        }
        crate::render::markup(
            cx,
            ">Pricing</a></li></ul><p class=\"v-cta-row\"><a class=\"v-btn v-btn-quiet\" href=\"",
        );
        crate::render::text(cx, &self.sign_in());
        crate::render::markup(cx, r#"">Sign In</a><a class="v-btn" href=""#);
        crate::render::text(cx, &self.get_started());
        crate::render::markup(
            cx,
            r#"">Get Started</a></p></div></details></nav><label class="v-theme-control" hidden for="site-theme"><span>Theme</span><select id="site-theme"><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></select></label></div></header>"#,
        );
    }
}
impl SiteNav {
    pub fn new(root: &str, current: &str) -> Self {
        Self {
            root: root.to_owned(),
            current: current.to_owned(),
        }
    }
    fn home(&self) -> String {
        link(&self.root, "")
    }
    fn home_current(&self) -> bool {
        self.current.is_empty()
    }
    fn groups(&self) -> Vec<NavGroup> {
        NAV_GROUPS
            .iter()
            .map(|(title, links)| NavGroup {
                title,
                links: links
                    .iter()
                    .map(|(label, path)| NavLink {
                        label,
                        href: link(&self.root, path),
                        current: *path == self.current,
                    })
                    .collect(),
            })
            .collect()
    }
    fn pricing(&self) -> NavLink {
        NavLink {
            label: "Pricing",
            href: link(&self.root, "pricing"),
            current: self.current == "pricing",
        }
    }
    fn sign_in(&self) -> &'static str {
        SIGN_IN_URL
    }
    fn get_started(&self) -> String {
        link(&self.root, "docs/getting-started")
    }
}
impl Component for SiteNav {}

pub const GET_STARTED_URL: &str = "../docs/getting-started/";

#[derive(Debug, Clone)]
pub struct SiteFooter {
    pub root: String,
}

impl Render for SiteFooter {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            "<footer class=\"v-site-footer\"><div class=\"v-footer-art\" aria-hidden=\"true\">",
        );
        self.contour().render(cx);
        self.mountain().render(cx);
        crate::render::markup(
            cx,
            r#"</div><div class="v-wrap v-footer-grid"><div class="v-footer-brand"><p class="v-footer-name">SkiMasque</p><p>Identity-aware network access<br>for developers and workloads.</p></div><nav aria-label="Footer"><div class="v-footer-col"><p class="v-footer-title">Product</p><ul><li><a href=""#,
        );
        crate::render::text(cx, &self.href("how-it-works"));
        crate::render::markup(cx, r#"">How It Works</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("identities"));
        crate::render::markup(cx, r#"">Identities</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("policies"));
        crate::render::markup(cx, r#"">Policies</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("ci-cd"));
        crate::render::markup(cx, r#"">CI/CD</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("developers"));
        crate::render::markup(cx, r#"">Developers</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("security"));
        crate::render::markup(cx, r#"">Security</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("deployment"));
        crate::render::markup(cx, r#"">Deployment</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("gateways"));
        crate::render::markup(
            cx,
            r#"">Gateways</a></li></ul></div><div class="v-footer-col"><p class="v-footer-title">Resources</p><ul><li><a href=""#,
        );
        crate::render::text(cx, &self.href("docs"));
        crate::render::markup(cx, r#"">Documentation</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("architecture"));
        crate::render::markup(cx, r#"">Architecture</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("technology/masque"));
        crate::render::markup(cx, r#"">MASQUE</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("open-source"));
        crate::render::markup(cx, r#"">Open Source</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("faq"));
        crate::render::markup(
            cx,
            r#"">FAQ</a></li></ul></div><div class="v-footer-col"><p class="v-footer-title">Company</p><ul><li><a href=""#,
        );
        crate::render::text(cx, &self.href("about"));
        crate::render::markup(cx, r#"">About</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("contact"));
        crate::render::markup(cx, r#"">Contact</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("trust"));
        crate::render::markup(cx, r#"">Trust</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("status"));
        crate::render::markup(cx, r#"">Status</a></li><li><a href=""#);
        crate::render::text(cx, &self.href("security"));
        crate::render::markup(
            cx,
            r#"">Security</a></li></ul></div><div class="v-footer-col"><p class="v-footer-title">Legal</p><ul><li>Privacy "#,
        );
        self.planned().render(cx);
        crate::render::markup(cx, r#"</li><li>Terms "#);
        self.planned().render(cx);
        crate::render::markup(
            cx,
            r#"</li></ul></div></nav></div><p class="v-wrap v-footer-legal">© SkiMasque</p></footer>"#,
        );
    }
}
impl SiteFooter {
    pub fn new(root: &str) -> Self {
        Self {
            root: root.to_owned(),
        }
    }
    fn href(&self, path: &str) -> String {
        link(&self.root, path)
    }
    fn contour(&self) -> Html {
        Contour.html()
    }
    fn mountain(&self) -> Html {
        Mountain.html()
    }
    fn planned(&self) -> Html {
        Planned::new().html()
    }
}
impl Component for SiteFooter {}

#[derive(Debug, Clone)]
pub struct SitePage {
    pub root: String,
    pub current: String,
    pub title: String,
    pub description: String,
    pub parts: Vec<Html>,
}

impl Render for SitePage {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><script>"#,
        );
        stucco_core::Raw::trusted(self.theme_script()).render(cx);
        crate::render::markup(cx, r#"</script><title>"#);
        crate::render::text(cx, &self.title);
        crate::render::markup(cx, r#"</title><meta name="description" content=""#);
        crate::render::text(cx, &self.description);
        crate::render::markup(cx, r#""><meta property="og:title" content=""#);
        crate::render::text(cx, &self.title);
        crate::render::markup(cx, r#""><meta property="og:description" content=""#);
        crate::render::text(cx, &self.description);
        crate::render::markup(
            cx,
            "\"><meta property=\"og:type\" content=\"website\"><link rel=\"icon\" href=\"",
        );
        crate::render::text(cx, &self.root);
        crate::render::markup(
            cx,
            r#"favicon.svg" type="image/svg+xml"><link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin><link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600&display=swap"><link rel="stylesheet" href=""#,
        );
        crate::render::text(cx, &self.root);
        crate::render::markup(cx, r#"assets/visual.css?v="#);
        crate::render::text(cx, &self.css_version());
        crate::render::markup(cx, r#""></head><body class="v-site">"#);
        self.sprite().render(cx);
        crate::render::markup(
            cx,
            r##"<a class="v-skip" href="#main">Skip to content</a>"##,
        );
        self.nav().render(cx);
        crate::render::markup(cx, r#"<main id="main">"#);
        for p in &self.parts {
            p.render(cx);
        }
        crate::render::markup(cx, r#"</main>"#);
        self.footer().render(cx);
        crate::render::markup(cx, r#"</body></html>"#);
    }
}
impl SitePage {
    fn theme_script(&self) -> &'static str {
        include_str!("../static/theme.js")
    }
    fn css_version(&self) -> String {
        // Stable content fingerprint: changes whenever the embedded stylesheet changes.
        let hash = crate::CSS
            .bytes()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
        format!("{hash:016x}")
    }
    pub fn new(root: &str, current: &str, title: &str, description: &str) -> Self {
        Self {
            root: root.to_owned(),
            current: current.to_owned(),
            title: title.to_owned(),
            description: description.to_owned(),
            parts: Vec::new(),
        }
    }
    pub fn push(mut self, c: &impl Component) -> Self {
        self.parts.push(c.html());
        self
    }
    fn sprite(&self) -> Html {
        Icons.html()
    }
    fn nav(&self) -> Html {
        SiteNav::new(&self.root, &self.current).html()
    }
    fn footer(&self) -> Html {
        SiteFooter::new(&self.root).html()
    }
}
impl Component for SitePage {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Prose};

    #[test]
    fn links_are_relative_and_directory_style() {
        assert_eq!(link("", "how-it-works"), "how-it-works/");
        assert_eq!(link("../", "technology/masque"), "../technology/masque/");
        assert_eq!(link("../../", ""), "../../");
        assert_eq!(link("", ""), "./");
    }

    #[test]
    fn the_nav_lists_every_group_marks_the_current_page_and_has_a_mobile_menu() {
        let s = SiteNav::new("../", "policies").html();
        let s = s.as_str();
        for label in [
            "How It Works",
            "Identities",
            "Policies",
            "CI/CD",
            "Developers",
            "Security",
            "Deployment",
            "Gateways",
            "How SkiMasque Compares",
            "Architecture",
            "MASQUE",
            "Documentation",
            "Use Cases",
            "Open Source",
            "FAQ",
            "Trust",
            "Pricing",
            "Sign In",
            "Get Started",
        ] {
            assert!(s.contains(label), "{label}");
        }
        assert_eq!(
            s.matches(r#"aria-current="page""#).count(),
            2,
            "desktop and mobile menu each mark the current link"
        );
        assert!(
            s.contains(r#"href="../policies/""#) && s.contains(r#"href="../technology/masque/""#)
        );
        assert!(s.contains("<details class=\"v-menu\"") && s.contains("<summary>Menu</summary>"));
        assert!(s.contains("https://control.skimasque.com"));
    }

    #[test]
    fn the_home_page_marks_the_brand_current() {
        let s = SiteNav::new("", "").html();
        assert!(s
            .as_str()
            .contains(r#"class="v-brand" aria-current="page""#));
    }

    #[test]
    fn the_footer_has_the_canonical_columns_and_shows_missing_pages_as_planned() {
        let s = SiteFooter::new("").html();
        let s = s.as_str();
        for label in [
            "Identity-aware network access",
            "Product",
            "Resources",
            "Company",
            "Legal",
            "About",
            "Contact",
            "Status",
            "Privacy",
            "Terms",
        ] {
            assert!(s.contains(label), "{label}");
        }
        assert!(s.contains("v-contour") && s.contains("v-mountain"));
        assert!(
            s.matches("PLANNED").count() >= 2,
            "Privacy and Terms have no pages yet"
        );
        assert!(!s.contains(r#"href="privacy"#));
    }

    #[test]
    fn a_page_is_a_complete_document_with_a_skip_link_and_relative_assets() {
        let s = SitePage::new(
            "../",
            "policies",
            "Policies · SkiMasque",
            "How policies decide access.",
        )
        .push(&Prose::new().p("hello"))
        .html();
        let s = s.as_str();
        assert!(s.starts_with("<!doctype html>") && s.contains(r#"<html lang="en">"#));
        assert!(
            s.contains("<title>Policies · SkiMasque</title>")
                && s.contains(r#"name="description" content="How policies decide access.""#)
        );
        assert!(
            s.contains(r#"href="../assets/visual.css?v="#)
                && s.contains(r#"href="../favicon.svg""#)
        );
        assert!(
            s.contains(r##"<a class="v-skip" href="#main">"##) && s.contains(r#"<main id="main">"#)
        );
        assert!(s.contains(r#"<svg class="v-sprite""#), "icon sprite once");
        assert!(
            s.find("v-site-header").unwrap() < s.find("<main").unwrap()
                && s.find("</main>").unwrap() < s.find("v-site-footer").unwrap()
        );
    }

    #[test]
    fn page_metadata_is_escaped() {
        let s = SitePage::new("", "", "<script>x</script>", "a \"b\" <i>").html();
        assert!(!s.as_str().contains("<script>x") && !s.as_str().contains("<i>"));
    }
}
