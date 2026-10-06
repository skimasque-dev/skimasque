//! Small content components: the Planned marker, empty states, code examples.

use stucco_core::Render;

/// Marks anything the product does not do yet. Never shown as a working control.
#[derive(Debug, Clone, Default)]
pub struct Planned {
    pub note: Option<String>,
}

impl Render for Planned {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(
            cx,
            "<span class=\"v-planned\"><span aria-hidden=\"true\">◇</span> PLANNED",
        );
        if let Some(n) = &self.note {
            crate::render::markup(cx, r#"<span class="v-sr"> — "#);
            crate::render::text(cx, &n);
            crate::render::markup(cx, r#"</span>"#);
        }
        crate::render::markup(cx, r#"</span>"#);
    }
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

#[derive(Debug, Clone)]
pub struct EmptyState {
    pub title: String,
    pub lines: Vec<String>,
    /// (label, href). The href must be an app-built path: it is HTML-escaped but
    /// not scheme-checked; never pass user input.
    pub action: Option<(String, String)>,
    pub level: u8,
}

impl Render for EmptyState {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<div class="v-empty"><h"#);
        crate::render::text(cx, &self.level);
        crate::render::markup(cx, r#" class="v-empty-title">"#);
        crate::render::text(cx, &self.title);
        crate::render::markup(cx, r#"</h"#);
        crate::render::text(cx, &self.level);
        crate::render::markup(cx, r#"><p class="v-empty-body">"#);
        for l in &self.lines {
            crate::render::markup(cx, r#"<span>"#);
            crate::render::text(cx, &l);
            crate::render::markup(cx, r#"</span>"#);
        }
        crate::render::markup(cx, r#"</p>"#);
        if let Some((label, href)) = &self.action {
            crate::render::markup(cx, r#"<a class="v-btn" href=""#);
            crate::render::text(cx, &href);
            crate::render::markup(cx, r#"">"#);
            crate::render::text(cx, &label);
            crate::render::markup(cx, r#"</a>"#);
        }
        crate::render::markup(cx, r#"</div>"#);
    }
}
impl EmptyState {
    pub fn new(title: impl Into<String>, lines: &[&str]) -> Self {
        Self {
            title: title.into(),
            lines: lines.iter().map(|l| (*l).to_owned()).collect(),
            action: None,
            level: 3,
        }
    }
    /// The heading level of the card title, `2..=6` (default 3). Pick the level
    /// that follows the page's own headings so levels never skip.
    pub fn level(mut self, n: u8) -> Self {
        self.level = n.clamp(2, 6);
        self
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
/// everything else output. `data-copy` carries the commands for the host's copy button:
/// any script that looks for `[data-copy]` and copies the attribute works.
#[derive(Debug, Clone)]
pub struct CodeExample {
    pub label: String,
    pub code: String,
}

impl Render for CodeExample {
    fn render(&self, cx: &mut stucco_core::Cx) {
        crate::render::markup(cx, r#"<figure class="v-code""#);
        if let Some(c) = &self.copy_text() {
            crate::render::markup(cx, r#" data-copy=""#);
            crate::render::text(cx, &c);
            crate::render::markup(cx, r#"""#);
        }
        crate::render::markup(cx, r#"><figcaption class="v-code-label">"#);
        crate::render::text(cx, &self.label);
        crate::render::markup(cx, r#"</figcaption><pre class="v-code-body">"#);
        for r in (self.rows()).iter() {
            crate::render::markup(cx, r#"<span class=""#);
            crate::render::text(cx, &r.class);
            crate::render::markup(cx, r#"">"#);
            if r.prompt {
                crate::render::markup(
                    cx,
                    "<span class=\"v-code-prompt\" aria-hidden=\"true\">$ </span>",
                );
            }
            crate::render::text(cx, &r.text);
            crate::render::markup(
                cx,
                r#"</span>
"#,
            );
        }
        crate::render::markup(cx, r#"</pre></figure>"#);
    }
}
impl CodeExample {
    pub fn new(label: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            code: code.into(),
        }
    }
    fn rows(&self) -> Vec<CodeRow> {
        self.code
            .lines()
            .map(|l| match (l.strip_prefix("$ "), l.starts_with("# ")) {
                (Some(cmd), _) => CodeRow {
                    class: "v-code-cmd",
                    prompt: true,
                    text: cmd.to_owned(),
                },
                (None, true) => CodeRow {
                    class: "v-code-comment",
                    prompt: false,
                    text: l.to_owned(),
                },
                _ => CodeRow {
                    class: "v-code-out",
                    prompt: false,
                    text: l.to_owned(),
                },
            })
            .collect()
    }
    /// The commands as typed: each `$ ` line, plus the continuation lines that
    /// follow one ending in `\`. `None` when there is no command (YAML, TOML).
    fn copy_text(&self) -> Option<String> {
        let mut out: Vec<&str> = Vec::new();
        let mut continued = false;
        for l in self.code.lines() {
            if continued {
                out.push(l);
                continued = l.trim_end().ends_with('\\');
            } else if let Some(cmd) = l.strip_prefix("$ ") {
                out.push(cmd);
                continued = cmd.trim_end().ends_with('\\');
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out.join("\n"))
        }
    }
}
impl crate::Component for CodeExample {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Component;

    #[test]
    fn planned_says_planned_and_escapes_its_note() {
        let plain = Planned::new().html();
        assert!(
            plain.as_str().contains(r#"class="v-planned""#) && plain.as_str().contains("PLANNED")
        );
        let noted = Planned::new().note("<script>x</script>").html();
        assert!(noted.as_str().contains("&lt;script&gt;") && !noted.as_str().contains("<script>"));
    }

    #[test]
    fn canonical_empty_states_use_the_spec_copy() {
        let p = EmptyState::no_policies("/app/policies/new").html();
        for s in [
            "No policies yet.",
            "Create your first policy to give a workload",
            "temporary access to private infrastructure.",
            "Create Policy",
            r#"href="/app/policies/new""#,
        ] {
            assert!(p.as_str().contains(s), "{s}");
        }
        let s = EmptyState::no_sessions().html();
        assert!(
            s.as_str().contains("No active sessions.") && s.as_str().contains("will appear here.")
        );
        assert!(!s.as_str().contains("<a "), "sessions have no action");
        let g = EmptyState::no_gateways("/app/gateways/new").html();
        assert!(
            g.as_str().contains("No gateways connected.") && g.as_str().contains("Add Gateway")
        );
    }

    #[test]
    fn empty_state_escapes_and_has_no_action_by_default() {
        let e = EmptyState::new("<b>t</b>", &["a & b"]).html();
        assert!(e.as_str().contains("&lt;b&gt;t&lt;/b&gt;") && e.as_str().contains("a &amp; b"));
    }

    #[test]
    fn code_example_marks_prompts_comments_and_copies_only_commands() {
        let c = CodeExample::new(
            "connect",
            "# open a session\n$ skimasque connect db.prod:5432\nlistening on 127.0.0.1:5432",
        )
        .html();
        let s = c.as_str();
        assert!(
            s.contains(r#"<span class="v-code-prompt""#)
                && s.contains("v-code-comment")
                && s.contains("v-code-out")
        );
        assert!(s.contains(r#"data-copy="skimasque connect db.prod:5432""#));
        let h = CodeExample::new("x", "$ echo \"<script>\"").html();
        assert!(!h.as_str().contains("<script>"));
        assert!(h.as_str().contains("&lt;script&gt;"));
    }

    #[test]
    fn code_example_copies_backslash_continued_commands_as_typed() {
        let c = CodeExample::new(
            "x",
            "# run\n$ skimasque exec \\n    --org acme \\n    -- terraform apply\nplanning...\n$ echo done",
        )
        .html();
        assert!(c.as_str().contains(
            "data-copy=\"skimasque exec \\n    --org acme \\n    -- terraform apply\necho done\""
        ));
    }

    #[test]
    fn code_example_without_commands_has_no_data_copy() {
        let c = CodeExample::new("policy", "# yaml\nname: x\nmax: 20m").html();
        assert!(!c.as_str().contains("data-copy"));
    }

    #[test]
    fn an_empty_state_takes_a_heading_level() {
        let e = EmptyState::no_sessions().level(2);
        assert!(e.html().as_str().contains("<h2 class=\"v-empty-title\">"));
        assert!(EmptyState::no_sessions()
            .html()
            .as_str()
            .contains("<h3 class=\"v-empty-title\">"));
    }
}
