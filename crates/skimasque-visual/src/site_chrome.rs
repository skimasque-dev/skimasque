//! Public-site chrome: the navigation, the footer and the page shell.
//! Links are relative (the site is served from a project path), so every
//! component takes the page's `root` prefix ("", "../", "../../").

use askama::Template;

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
        ],
    ),
];

/// A directory-style relative link: `link("../", "policies")` -> `../policies/`.
pub fn link(root: &str, path: &str) -> String {
    if path.is_empty() {
        root.to_owned()
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

#[derive(Template, Debug, Clone)]
#[template(path = "site_nav.html")]
pub struct SiteNav {
    pub root: String,
    pub current: String,
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
    fn get_started(&self) -> &'static str {
        GET_STARTED_URL
    }
}
impl Component for SiteNav {}

pub const GET_STARTED_URL: &str =
    "https://github.com/skimasque-dev/skimasque/blob/main/docs/getting-started.md";

#[derive(Template, Debug, Clone)]
#[template(path = "site_footer.html")]
pub struct SiteFooter {
    pub root: String,
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

#[derive(Template, Debug, Clone)]
#[template(path = "site_page.html")]
pub struct SitePage {
    pub root: String,
    pub current: String,
    pub title: String,
    pub description: String,
    pub parts: Vec<Html>,
}
impl SitePage {
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
            "How SkiMasque Compares",
            "Architecture",
            "MASQUE",
            "Documentation",
            "Use Cases",
            "Open Source",
            "FAQ",
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
            s.contains(r#"href="../assets/visual.css""#) && s.contains(r#"href="../favicon.svg""#)
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
