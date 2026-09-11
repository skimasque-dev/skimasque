//! Rendering a [`Policy`] back to the TOML a person would write.
//!
//! This is the inverse of [`Policy::from_toml`] for everything a policy can
//! hold, and it is what `skimasque policy learn` prints: a suggestion the
//! developer reviews, edits, and commits. The output round-trips -- parsing it
//! yields an equal `Policy` -- so a learned policy can be checked straight into
//! `.masque/policies/`.

use std::fmt::Write as _;

use crate::model::{Action, AppPattern, Policy};
use crate::units::{humanize_decimal, humanize_duration};

impl Policy {
    /// Render this policy as a TOML document.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "name = {}", quote(&self.name));

        let m = &self.match_spec;
        if m.specificity() > 0 {
            let _ = writeln!(out, "\n[match]");
            for (key, value) in [
                ("organization", &m.organization),
                ("repository", &m.repository),
                ("workflow", &m.workflow),
                ("ref", &m.git_ref),
                ("branch", &m.branch),
                ("environment", &m.environment),
                ("actor", &m.actor),
            ] {
                if let Some(value) = value {
                    let _ = writeln!(out, "{key} = {}", quote(value));
                }
            }
        }

        if let Some(duration) = self.session.max_duration {
            let _ = writeln!(out, "\n[session]");
            let _ = writeln!(out, "max_duration = {}", quote(&humanize_duration(duration)));
        }

        if let Some(egress) = &self.egress {
            let _ = writeln!(out, "\n[egress]");
            if let Some(region) = &egress.region {
                let _ = writeln!(out, "region = {}", quote(region));
            }
            if let Some(pool) = &egress.ip_pool {
                let _ = writeln!(out, "ip_pool = {}", quote(pool));
            }
        }

        let limits = &self.limits;
        if limits != &Default::default() {
            let _ = writeln!(out, "\n[limits]");
            if let Some(bits) = limits.bandwidth_bits_per_sec {
                let _ = writeln!(out, "bandwidth = {}", quote(&humanize_decimal(bits, "bps")));
            }
            if let Some(bytes) = limits.total_bytes {
                let _ = writeln!(out, "bytes = {}", quote(&humanize_decimal(bytes, "B")));
            }
            if let Some(n) = limits.concurrent_connections {
                let _ = writeln!(out, "connections = {n}");
            }
            if let Some(n) = limits.concurrent_destinations {
                let _ = writeln!(out, "concurrent_destinations = {n}");
            }
            if let Some(rate) = limits.connection_rate {
                let _ = writeln!(
                    out,
                    "connection_rate = {}",
                    quote(&format!("{}/{}", rate.count, per_suffix(rate.per)))
                );
            }
            if let Some(pps) = limits.packets_per_sec {
                let _ = writeln!(out, "packets_per_second = {pps}");
            }
        }

        for rule in &self.rules {
            let _ = writeln!(out, "\n[[rules]]");
            if let Some(id) = &rule.id {
                let _ = writeln!(out, "id = {}", quote(id));
            }
            let app = match &rule.application {
                AppPattern::Any => "*".to_owned(),
                AppPattern::Name(name) => name.clone(),
            };
            let _ = writeln!(out, "application = {}", quote(&app));
            if let Some(word) = rule.transport.as_word() {
                let _ = writeln!(out, "transport = {}", quote(word));
            }
            let _ = writeln!(
                out,
                "action = {}",
                quote(match rule.action {
                    Action::Allow => "allow",
                    Action::Deny => "deny",
                })
            );
            let _ = writeln!(out, "destinations = [");
            for dest in &rule.destinations {
                let _ = writeln!(out, "    {},", quote(dest.as_str()));
            }
            let _ = writeln!(out, "]");
        }

        for test in &self.tests {
            let _ = writeln!(out, "\n[[tests]]");
            let _ = writeln!(out, "application = {}", quote(&test.application));
            if let Some(transport) = test.transport {
                let _ = writeln!(out, "transport = {}", quote(transport.as_str()));
            }
            let _ = writeln!(out, "destination = {}", quote(&test.destination));
            let _ = writeln!(
                out,
                "expect = {}",
                quote(if test.expect == Action::Allow { "allow" } else { "deny" })
            );
        }

        out
    }
}

fn per_suffix(window: std::time::Duration) -> &'static str {
    match window.as_secs() {
        3_600 => "h",
        60 => "m",
        _ => "s",
    }
}

/// A TOML basic string. The values here are hostnames, refs and names, so this
/// only needs to handle the quote and backslash.
fn quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_policy_round_trips_through_toml() {
        let original = Policy::from_toml(
            r#"
            name = "production"

            [match]
            repository = "acme/widget"
            branch = "main"

            [session]
            max_duration = "20m"

            [egress]
            region = "us-west"

            [limits]
            bandwidth = "100Mbps"
            connections = 50
            connection_rate = "10/s"

            [[rules]]
            id = "tf"
            application = "terraform"
            action = "allow"
            destinations = ["api.production.example.com:443", "registry.terraform.io:443"]

            [[tests]]
            application = "terraform"
            destination = "api.production.example.com:443"
            expect = "allow"
        "#,
        )
        .unwrap();

        let rendered = original.to_toml();
        let reparsed = Policy::from_toml(&rendered)
            .unwrap_or_else(|error| panic!("rendered TOML did not parse: {error}\n---\n{rendered}"));
        assert_eq!(original, reparsed, "\n--- rendered ---\n{rendered}");
    }

    #[test]
    fn a_bare_policy_renders_just_its_name() {
        let policy = Policy::from_toml(r#"name = "dev""#).unwrap();
        assert_eq!(policy.to_toml().trim(), r#"name = "dev""#);
    }
}
