//! Small content components: the Planned marker, empty states, code examples.

use askama::Template;

/// Marks anything the product does not do yet. Never shown as a working control.
#[derive(Template, Debug, Clone, Default)]
#[template(path = "planned.html")]
pub struct Planned {
    pub note: Option<String>,
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

#[derive(Template, Debug, Clone)]
#[template(path = "empty_state.html")]
pub struct EmptyState {
    pub title: String,
    pub lines: Vec<String>,
    pub action: Option<(String, String)>,
}
impl EmptyState {
    pub fn new(title: impl Into<String>, lines: &[&str]) -> Self {
        Self {
            title: title.into(),
            lines: lines.iter().map(|l| (*l).to_owned()).collect(),
            action: None,
        }
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
/// everything else output. `data-copy` carries the commands for the host's copy button.
#[derive(Template, Debug, Clone)]
#[template(path = "code_example.html")]
pub struct CodeExample {
    pub label: String,
    pub code: String,
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
    fn copy_text(&self) -> String {
        self.code
            .lines()
            .filter_map(|l| l.strip_prefix("$ "))
            .collect::<Vec<_>>()
            .join("\n")
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
}
