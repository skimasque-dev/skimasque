//! Static checks for policies that parse but are likely mistakes.
//!
//! A policy's `[match]` block decides *which workloads* the policy governs, and
//! its allow rules decide *what they may reach*. The dangerous combination is a
//! broad match with permissive rules: an attacker who can produce a workload
//! identity that satisfies the match -- by pushing any workflow to a repository
//! in the named organisation, or opening a pull request that runs on the named
//! repository -- inherits every allow rule.
//!
//! These lints do not fail a policy on their own; `skimasque policy validate`
//! prints them, `--strict` turns them into an error, and the gateway logs them
//! when it loads a policy set. They are heuristics: a single-tenant deployment
//! that deliberately runs one catch-all policy is expected to see
//! [`LintCode::MatchGovernsEveryWorkload`] and ignore it.

use crate::model::{Action, Policy, PolicySet};

/// One flagged concern about one policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lint {
    /// The name of the policy the lint is about.
    pub policy: String,
    pub code: LintCode,
    /// A one-line, human-facing explanation of the concern and the fix.
    pub message: String,
}

/// The kinds of concern [`Policy::lints`] reports. Non-exhaustive: more checks
/// may be added without it being a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LintCode {
    /// The `[match]` block constrains nothing, so every workload with a valid
    /// credential is governed by this policy's allow rules.
    MatchGovernsEveryWorkload,
    /// The `[match]` block constrains some identity fields but not
    /// `repository`, so a workload from *any* repository that satisfies the
    /// other constraints is granted the rules.
    MatchLacksRepository,
    /// The `[match]` block pins a `repository` but not a `branch` or `git_ref`,
    /// so any ref of that repository matches -- including a pull-request branch
    /// pushed by an outside contributor.
    MatchLacksRef,
}

impl LintCode {
    /// A stable kebab-case identifier, for `--strict` output and log fields.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MatchGovernsEveryWorkload => "match-governs-every-workload",
            Self::MatchLacksRepository => "match-lacks-repository",
            Self::MatchLacksRef => "match-lacks-ref",
        }
    }
}

impl std::fmt::Display for LintCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Policy {
    /// Heuristic checks for a policy that parses but is probably too broad. See
    /// the [module docs](self).
    pub fn lints(&self) -> Vec<Lint> {
        let mut lints = Vec::new();

        // A policy with no allow rule cannot over-grant: a broad match on a
        // deny-only policy is a normal, safe pattern (a fleet-wide floor).
        if !self.rules.iter().any(|rule| rule.action == Action::Allow) {
            return lints;
        }

        let m = &self.match_spec;
        let make = |code: LintCode, message: String| Lint {
            policy: self.name.clone(),
            code,
            message,
        };

        if m.specificity() == 0 {
            lints.push(make(
                LintCode::MatchGovernsEveryWorkload,
                "the [match] block is empty, so every workload with a valid credential is \
                 granted this policy's allow rules; add repository (and a branch) unless this \
                 is a deliberate single-tenant catch-all"
                    .to_owned(),
            ));
            return lints;
        }

        if m.repository.is_none() {
            let named = constrained_field_names(self);
            lints.push(make(
                LintCode::MatchLacksRepository,
                format!(
                    "[match] constrains {named} but not repository, so a workload from any \
                     repository that satisfies {named} is granted this policy's allow rules; \
                     pin repository as well"
                ),
            ));
        } else if m.branch.is_none() && m.git_ref.is_none() {
            lints.push(make(
                LintCode::MatchLacksRef,
                "[match] pins a repository but no branch or git_ref, so any ref matches -- \
                 including a pull-request branch from an outside contributor; add branch or \
                 git_ref to keep untrusted refs out"
                    .to_owned(),
            ));
        }

        lints
    }
}

impl PolicySet {
    /// [`Policy::lints`] for every policy in the set, in set order.
    pub fn lints(&self) -> Vec<Lint> {
        self.policies().iter().flat_map(Policy::lints).collect()
    }
}

/// A comma-joined list of the identity fields a policy's match constrains, for
/// a lint message. Never empty when called (the empty case is its own lint).
fn constrained_field_names(policy: &Policy) -> String {
    let m = &policy.match_spec;
    let mut names = Vec::new();
    if m.organization.is_some() {
        names.push("organization");
    }
    if m.workflow.is_some() {
        names.push("workflow");
    }
    if m.git_ref.is_some() {
        names.push("git_ref");
    }
    if m.branch.is_some() {
        names.push("branch");
    }
    if m.environment.is_some() {
        names.push("environment");
    }
    if m.actor.is_some() {
        names.push("actor");
    }
    names.join(", ")
}

#[cfg(test)]
mod tests {
    use crate::model::Policy;

    fn lint_codes(toml: &str) -> Vec<String> {
        Policy::from_toml(toml)
            .unwrap()
            .lints()
            .into_iter()
            .map(|lint| lint.code.to_string())
            .collect()
    }

    const ALLOW_RULE: &str = r#"
        [[rules]]
        application = "terraform"
        action = "allow"
        destinations = ["api.example.com:443"]
    "#;

    #[test]
    fn a_well_scoped_policy_is_clean() {
        let toml = format!(
            r#"
            name = "prod"
            [match]
            repository = "acme/widget"
            branch = "main"
            {ALLOW_RULE}
        "#
        );
        assert!(lint_codes(&toml).is_empty());
    }

    #[test]
    fn an_empty_match_with_an_allow_rule_is_flagged_once() {
        let toml = format!("name = \"any\"\n{ALLOW_RULE}");
        assert_eq!(lint_codes(&toml), ["match-governs-every-workload"]);
    }

    #[test]
    fn an_empty_match_with_only_deny_rules_is_clean() {
        let toml = r#"
            name = "floor"
            [[rules]]
            application = "terraform"
            action = "deny"
            destinations = ["169.254.169.254:80"]
        "#;
        assert!(lint_codes(toml).is_empty());
    }

    #[test]
    fn an_organization_only_match_is_flagged_for_lacking_a_repository() {
        let toml = format!(
            r#"
            name = "org-wide"
            [match]
            organization = "acme"
            {ALLOW_RULE}
        "#
        );
        assert_eq!(lint_codes(&toml), ["match-lacks-repository"]);
    }

    #[test]
    fn a_repository_without_a_ref_is_flagged_for_pull_request_exposure() {
        let toml = format!(
            r#"
            name = "repo-any-ref"
            [match]
            repository = "acme/widget"
            {ALLOW_RULE}
        "#
        );
        assert_eq!(lint_codes(&toml), ["match-lacks-ref"]);
    }

    #[test]
    fn a_repository_pinned_to_a_git_ref_is_clean() {
        let toml = format!(
            r#"
            name = "repo-main"
            [match]
            repository = "acme/widget"
            ref = "refs/heads/main"
            {ALLOW_RULE}
        "#
        );
        assert!(lint_codes(&toml).is_empty());
    }
}
