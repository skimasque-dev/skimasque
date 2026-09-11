//! URI Templates for locating a MASQUE proxy (RFC 6570, profiled by RFC 9298
//! Section 2 and RFC 9484 Section 3).
//!
//! A client is configured with a template such as
//! `https://proxy.example/.well-known/masque/udp/{target_host}/{target_port}/`
//! and expands it to build the `:path` of its request. The proxy has to run the
//! same template *backwards*, recovering the target from the path it received,
//! so this module implements both directions from one parsed representation.
//!
//! Only the subset the MASQUE specifications permit is supported: level 3 or
//! lower, and only the simple (`{var}`), form-style query (`{?a,b}`) and query
//! continuation (`{&a,b}`) operators. The reserved, fragment, dot-prefix,
//! slash-prefix and semicolon-prefix operators are rejected at parse time
//! because they would let a template smuggle structure into the path.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// The variables a CONNECT-UDP template must define (RFC 9298, Section 2).
pub const CONNECT_UDP_VARIABLES: [&str; 2] = ["target_host", "target_port"];

/// The variables a CONNECT-TCP template must define
/// (`draft-ietf-httpbis-connect-tcp`, Section 3). The same pair as CONNECT-UDP.
pub const CONNECT_TCP_VARIABLES: [&str; 2] = ["target_host", "target_port"];

/// The variables a CONNECT-IP template may define (RFC 9484, Section 3). Both
/// are optional; a template with neither requests a full tunnel.
pub const CONNECT_IP_VARIABLES: [&str; 2] = ["target", "ipproto"];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("template must be absolute with a scheme and authority")]
    NotAbsolute,
    #[error("template has an empty {0} component")]
    EmptyComponent(&'static str),
    #[error("template path must start with a slash")]
    PathMustStartWithSlash,
    #[error("template contains {0:?}, outside the permitted ASCII range 0x21-0x7E")]
    IllegalCharacter(char),
    #[error("template uses the unsupported operator {0:?}; MASQUE forbids it")]
    UnsupportedOperator(char),
    #[error("template uses a level 4 modifier ({0:?}) but must be level 3 or lower")]
    UnsupportedModifier(char),
    #[error("template has an unterminated {{ expression")]
    UnterminatedExpression,
    #[error("template has an empty expression")]
    EmptyExpression,
    #[error("template puts variables outside the path and query components")]
    VariableOutsidePathOrQuery,
    #[error("template repeats the variable {0:?}")]
    DuplicateVariable(String),
    #[error("a query expression must be the last part of the template")]
    QueryExpressionMustBeLast,
    #[error("template is missing the required variable {0:?}")]
    MissingVariable(&'static str),
    #[error("no value supplied for the variable {0:?}")]
    UndefinedVariable(String),
    #[error("value for {variable:?} is empty, which MASQUE forbids")]
    EmptyValue { variable: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    /// `{var}` -- RFC 6570 simple string expansion.
    Simple(String),
    /// `{?a,b}` -- form-style query expansion, emitting `?a=v&b=v`.
    Query(Vec<String>),
    /// `{&a,b}` -- form-style query continuation, emitting `&a=v&b=v`.
    QueryContinuation(Vec<String>),
}

/// A parsed, validated MASQUE proxy URI Template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UriTemplate {
    scheme: String,
    authority: String,
    /// The path and query, as literals interleaved with expressions. Always
    /// begins with a literal starting `/`.
    parts: Vec<Part>,
    variables: Vec<String>,
    raw: String,
}

impl UriTemplate {
    /// Parse and structurally validate a template.
    ///
    /// This enforces every requirement in RFC 9298 Section 2 that does not
    /// depend on which protocol the template is for; call
    /// [`require_variables`](Self::require_variables) for those.
    pub fn parse(template: &str) -> Result<Self, Error> {
        for ch in template.chars() {
            if !matches!(ch, '\u{21}'..='\u{7e}') {
                return Err(Error::IllegalCharacter(ch));
            }
        }

        let (scheme, rest) = template
            .split_once("://")
            .ok_or(Error::NotAbsolute)?;
        if scheme.is_empty() {
            return Err(Error::EmptyComponent("scheme"));
        }
        // The path begins at the first slash after the authority. A template
        // with no slash has an empty path, which is not allowed.
        let slash = rest.find('/').ok_or(Error::EmptyComponent("path"))?;
        let (authority, path_and_query) = rest.split_at(slash);
        if authority.is_empty() {
            return Err(Error::EmptyComponent("authority"));
        }
        if scheme.contains('{') || authority.contains('{') {
            return Err(Error::VariableOutsidePathOrQuery);
        }
        if !path_and_query.starts_with('/') {
            return Err(Error::PathMustStartWithSlash);
        }

        let parts = parse_parts(path_and_query)?;

        let mut variables = Vec::new();
        for part in &parts {
            let names: &[String] = match part {
                Part::Literal(_) => &[],
                Part::Simple(name) => std::slice::from_ref(name),
                Part::Query(names) | Part::QueryContinuation(names) => names,
            };
            for name in names {
                if variables.contains(name) {
                    return Err(Error::DuplicateVariable(name.clone()));
                }
                variables.push(name.clone());
            }
        }

        Ok(Self {
            scheme: scheme.to_owned(),
            authority: authority.to_owned(),
            parts,
            variables,
            raw: template.to_owned(),
        })
    }

    /// The default CONNECT-UDP template for a proxy that advertises no other
    /// (RFC 9298, Section 2).
    pub fn default_connect_udp(proxy_authority: &str) -> Result<Self, Error> {
        Self::parse(&format!(
            "https://{proxy_authority}/.well-known/masque/udp/{{target_host}}/{{target_port}}/"
        ))
    }

    /// The default CONNECT-TCP template for a proxy that advertises no other
    /// (`draft-ietf-httpbis-connect-tcp`, Section 3). Only used for the
    /// template-driven variant; classic `CONNECT` needs no template.
    pub fn default_connect_tcp(proxy_authority: &str) -> Result<Self, Error> {
        Self::parse(&format!(
            "https://{proxy_authority}/.well-known/masque/tcp/{{target_host}}/{{target_port}}/"
        ))
    }

    /// The default CONNECT-IP template (RFC 9484, Section 3).
    pub fn default_connect_ip(proxy_authority: &str) -> Result<Self, Error> {
        Self::parse(&format!(
            "https://{proxy_authority}/.well-known/masque/ip/{{target}}/{{ipproto}}/"
        ))
    }

    /// Check that every name in `required` appears in the template.
    pub fn require_variables(&self, required: &[&'static str]) -> Result<(), Error> {
        for name in required {
            if !self.variables.iter().any(|v| v == name) {
                return Err(Error::MissingVariable(name));
            }
        }
        Ok(())
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn authority(&self) -> &str {
        &self.authority
    }

    pub fn variables(&self) -> &[String] {
        &self.variables
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Expand to the `:path` of a request: the path and query, with variables
    /// substituted and percent-encoded.
    pub fn expand_path(&self, values: &BTreeMap<&str, String>) -> Result<String, Error> {
        let mut out = String::with_capacity(self.raw.len());
        for part in &self.parts {
            match part {
                Part::Literal(literal) => out.push_str(literal),
                Part::Simple(name) => out.push_str(&pct_encode(self.lookup(values, name)?)),
                Part::Query(names) => self.expand_query(values, names, '?', &mut out)?,
                Part::QueryContinuation(names) => {
                    self.expand_query(values, names, '&', &mut out)?
                }
            }
        }
        Ok(out)
    }

    /// Expand to the full absolute URI, for logging and diagnostics.
    pub fn expand_absolute(&self, values: &BTreeMap<&str, String>) -> Result<String, Error> {
        Ok(format!(
            "{}://{}{}",
            self.scheme,
            self.authority,
            self.expand_path(values)?
        ))
    }

    fn lookup<'a>(
        &self,
        values: &'a BTreeMap<&str, String>,
        name: &str,
    ) -> Result<&'a str, Error> {
        let value = values
            .get(name)
            .ok_or_else(|| Error::UndefinedVariable(name.to_owned()))?;
        if value.is_empty() {
            return Err(Error::EmptyValue {
                variable: name.to_owned(),
            });
        }
        Ok(value)
    }

    fn expand_query(
        &self,
        values: &BTreeMap<&str, String>,
        names: &[String],
        first_separator: char,
        out: &mut String,
    ) -> Result<(), Error> {
        for (i, name) in names.iter().enumerate() {
            let separator = if i == 0 { first_separator } else { '&' };
            let value = pct_encode(self.lookup(values, name)?);
            let _ = write!(out, "{separator}{name}={value}");
        }
        Ok(())
    }

    /// Recover variable values from a request path (and query), the inverse of
    /// [`expand_path`](Self::expand_path).
    ///
    /// Returns `None` if `path_and_query` was not produced by this template.
    /// Values are percent-decoded, so an IPv6 target arrives as `2001:db8::42`
    /// rather than `2001%3Adb8%3A%3A42`.
    pub fn match_path(&self, path_and_query: &str) -> Option<BTreeMap<String, String>> {
        let mut values = BTreeMap::new();
        match_parts(&self.parts, path_and_query, &mut values).then_some(values)
    }
}

fn parse_parts(input: &str) -> Result<Vec<Part>, Error> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut rest = input;

    while let Some(open) = rest.find('{') {
        literal.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find('}').ok_or(Error::UnterminatedExpression)?;
        let expression = &after[..close];
        rest = &after[close + 1..];

        if !literal.is_empty() {
            parts.push(Part::Literal(std::mem::take(&mut literal)));
        }
        parts.push(parse_expression(expression)?);
    }
    literal.push_str(rest);
    if !literal.is_empty() {
        parts.push(Part::Literal(literal));
    }

    // Query expansion consumes the remainder of the URI, so nothing may follow
    // it; allowing it would make matching ambiguous.
    if let Some(index) = parts
        .iter()
        .position(|p| matches!(p, Part::Query(_) | Part::QueryContinuation(_)))
    {
        if index + 1 != parts.len() {
            return Err(Error::QueryExpressionMustBeLast);
        }
    }
    Ok(parts)
}

fn parse_expression(expression: &str) -> Result<Part, Error> {
    let mut chars = expression.chars();
    let first = chars.next().ok_or(Error::EmptyExpression)?;
    let (operator, names) = match first {
        '?' | '&' => (Some(first), chars.as_str()),
        '+' | '#' | '.' | '/' | ';' | '=' | ',' | '!' | '@' | '|' => {
            return Err(Error::UnsupportedOperator(first))
        }
        _ => (None, expression),
    };

    let names = names
        .split(',')
        .map(|name| {
            if name.is_empty() {
                return Err(Error::EmptyExpression);
            }
            // `*` (explode) and `:` (prefix) are level 4; RFC 9298 caps at level 3.
            if let Some(modifier) = name.chars().find(|c| matches!(c, '*' | ':')) {
                return Err(Error::UnsupportedModifier(modifier));
            }
            Ok(name.to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(match operator {
        Some('?') => Part::Query(names),
        Some('&') => Part::QueryContinuation(names),
        _ => {
            if names.len() != 1 {
                // `{a,b}` is legal RFC 6570 but expands to a comma-joined list,
                // which no MASQUE template has any use for.
                return Err(Error::UnsupportedOperator(','));
            }
            Part::Simple(names.into_iter().next().expect("checked length"))
        }
    })
}

fn match_parts(parts: &[Part], input: &str, values: &mut BTreeMap<String, String>) -> bool {
    let Some((part, tail)) = parts.split_first() else {
        return input.is_empty();
    };

    match part {
        Part::Literal(literal) => match input.strip_prefix(literal.as_str()) {
            Some(rest) => match_parts(tail, rest, values),
            None => false,
        },
        Part::Simple(name) => {
            // Simple expansion percent-encodes everything but the unreserved
            // set, so a match can only span those characters plus escapes.
            // Take the longest such run and shrink until the rest lines up.
            let span = input
                .find(|c: char| !is_expansion_safe(c))
                .unwrap_or(input.len());
            for len in (1..=span).rev() {
                if !input.is_char_boundary(len) {
                    continue;
                }
                let Some(decoded) = pct_decode(&input[..len]) else {
                    continue;
                };
                values.insert(name.clone(), decoded);
                if match_parts(tail, &input[len..], values) {
                    return true;
                }
                values.remove(name);
            }
            false
        }
        Part::Query(names) | Part::QueryContinuation(names) => {
            let separator = if matches!(part, Part::Query(_)) { '?' } else { '&' };
            let Some(query) = input.strip_prefix(separator) else {
                return false;
            };
            let mut found = BTreeMap::new();
            for pair in query.split('&') {
                let Some((key, value)) = pair.split_once('=') else {
                    continue;
                };
                // Be liberal: an intermediary may have added parameters we do
                // not know about, and order is not guaranteed to survive.
                let (Some(key), Some(value)) = (pct_decode(key), pct_decode(value)) else {
                    return false;
                };
                found.insert(key, value);
            }
            for name in names {
                match found.get(name) {
                    Some(value) if !value.is_empty() => {
                        values.insert(name.clone(), value.clone());
                    }
                    _ => return false,
                }
            }
            // Query expansion is validated to be the final part, so it consumes
            // the whole remainder and nothing is left to match.
            debug_assert!(tail.is_empty());
            tail.is_empty()
        }
    }
}

/// Characters that can appear in an expanded simple expression: the RFC 3986
/// unreserved set, plus the `%XX` escapes standing in for everything else.
fn is_expansion_safe(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '%')
}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// Percent-encode per RFC 6570 string expansion: keep the unreserved set, escape
/// everything else. This is what turns `2001:db8::42` into `2001%3Adb8%3A%3A42`.
pub fn pct_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if is_unreserved(*byte) {
            out.push(*byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Reverse [`pct_encode`]. Returns `None` for malformed escapes or non-UTF-8 output.
pub fn pct_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&'static str, &str)]) -> BTreeMap<&'static str, String> {
        pairs
            .iter()
            .map(|(k, v)| (*k, (*v).to_owned()))
            .collect()
    }

    /// The three templates given as examples in RFC 9298, Section 2.
    #[test]
    fn expands_the_rfc9298_example_templates() {
        let cases = [
            (
                "https://example.org/.well-known/masque/udp/{target_host}/{target_port}/",
                "/.well-known/masque/udp/192.0.2.6/443/",
            ),
            (
                "https://proxy.example.org:4443/masque?h={target_host}&p={target_port}",
                "/masque?h=192.0.2.6&p=443",
            ),
            (
                "https://proxy.example.org:4443/masque{?target_host,target_port}",
                "/masque?target_host=192.0.2.6&target_port=443",
            ),
        ];
        let vars = values(&[("target_host", "192.0.2.6"), ("target_port", "443")]);

        for (template, expected) in cases {
            let template = UriTemplate::parse(template).unwrap();
            template.require_variables(&CONNECT_UDP_VARIABLES).unwrap();
            assert_eq!(template.expand_path(&vars).unwrap(), expected);
        }
    }

    /// Every example template must also round-trip back to the target, because
    /// the proxy recovers the target by matching the path it received.
    #[test]
    fn every_example_template_round_trips() {
        let templates = [
            "https://example.org/.well-known/masque/udp/{target_host}/{target_port}/",
            "https://proxy.example.org:4443/masque?h={target_host}&p={target_port}",
            "https://proxy.example.org:4443/masque{?target_host,target_port}",
        ];
        let vars = values(&[("target_host", "2001:db8::42"), ("target_port", "53")]);

        for template in templates {
            let template = UriTemplate::parse(template).unwrap();
            let path = template.expand_path(&vars).unwrap();
            let matched = template
                .match_path(&path)
                .unwrap_or_else(|| panic!("{path} did not match {}", template.as_str()));
            assert_eq!(matched["target_host"], "2001:db8::42");
            assert_eq!(matched["target_port"], "53");
        }
    }

    /// RFC 9298 requires the colons of an IPv6 literal to be percent-encoded.
    #[test]
    fn percent_encodes_ipv6_colons() {
        let template =
            UriTemplate::parse("https://e.org/.well-known/masque/udp/{target_host}/{target_port}/")
                .unwrap();
        let path = template
            .expand_path(&values(&[("target_host", "2001:db8::42"), ("target_port", "443")]))
            .unwrap();
        assert_eq!(path, "/.well-known/masque/udp/2001%3Adb8%3A%3A42/443/");
    }

    #[test]
    fn default_template_matches_the_specified_form() {
        let template = UriTemplate::default_connect_udp("proxy.example:4443").unwrap();
        assert_eq!(
            template.as_str(),
            "https://proxy.example:4443/.well-known/masque/udp/{target_host}/{target_port}/"
        );
        assert_eq!(template.authority(), "proxy.example:4443");
        template.require_variables(&CONNECT_UDP_VARIABLES).unwrap();
    }

    #[test]
    fn rejects_templates_masque_forbids() {
        let cases: &[(&str, Error)] = &[
            ("/.well-known/masque/udp/{target_host}/", Error::NotAbsolute),
            (
                "https://example.org",
                Error::EmptyComponent("path"),
            ),
            (
                "https://{target_host}.example.org/masque/",
                Error::VariableOutsidePathOrQuery,
            ),
            (
                "https://example.org/masque/{+target_host}/",
                Error::UnsupportedOperator('+'),
            ),
            (
                "https://example.org/masque/{#target_host}/",
                Error::UnsupportedOperator('#'),
            ),
            (
                "https://example.org/masque{/target_host}/",
                Error::UnsupportedOperator('/'),
            ),
            (
                "https://example.org/masque{.target_host}/",
                Error::UnsupportedOperator('.'),
            ),
            (
                "https://example.org/masque{;target_host}/",
                Error::UnsupportedOperator(';'),
            ),
            (
                "https://example.org/masque/{target_host:4}/",
                Error::UnsupportedModifier(':'),
            ),
            (
                "https://example.org/masque/{target_host*}/",
                Error::UnsupportedModifier('*'),
            ),
            (
                "https://example.org/masque/{target_host/",
                Error::UnterminatedExpression,
            ),
            (
                "https://example.org/masque/{}/",
                Error::EmptyExpression,
            ),
            (
                "https://example.org/masque{?target_host}/tail",
                Error::QueryExpressionMustBeLast,
            ),
            (
                "https://example.org/masque/{target_host}/{target_host}/",
                Error::DuplicateVariable("target_host".to_owned()),
            ),
            (
                "https://example.org/masque/\u{e9}/{target_host}/",
                Error::IllegalCharacter('\u{e9}'),
            ),
        ];
        for (template, expected) in cases {
            assert_eq!(
                UriTemplate::parse(template).unwrap_err(),
                *expected,
                "parsing {template}"
            );
        }
    }

    #[test]
    fn missing_required_variable_is_rejected() {
        let template = UriTemplate::parse("https://example.org/masque/{target_host}/").unwrap();
        assert_eq!(
            template.require_variables(&CONNECT_UDP_VARIABLES),
            Err(Error::MissingVariable("target_port"))
        );
    }

    #[test]
    fn empty_values_are_rejected() {
        let template = UriTemplate::default_connect_udp("example.org").unwrap();
        assert_eq!(
            template.expand_path(&values(&[("target_host", ""), ("target_port", "443")])),
            Err(Error::EmptyValue {
                variable: "target_host".to_owned()
            })
        );
    }

    #[test]
    fn non_matching_paths_are_rejected() {
        let template = UriTemplate::default_connect_udp("example.org").unwrap();
        for path in [
            "/.well-known/masque/udp/1.2.3.4/",       // missing a segment
            "/.well-known/masque/udp/1.2.3.4/53",     // missing the trailing slash
            "/.well-known/masque/tcp/1.2.3.4/53/",    // wrong literal
            "/.well-known/masque/udp//53/",           // empty target_host
            "/.well-known/masque/udp/1.2.3.4/53/xtra",
        ] {
            assert!(template.match_path(path).is_none(), "{path} should not match");
        }
    }

    /// CONNECT-IP templates use `*` as a value meaning "anything allowable", and
    /// `*` is not in the unreserved set, so it must survive the encode/decode
    /// round trip rather than being mistaken for an explode modifier.
    #[test]
    fn connect_ip_wildcard_values_round_trip() {
        let template = UriTemplate::default_connect_ip("example.org").unwrap();
        let path = template
            .expand_path(&values(&[("target", "*"), ("ipproto", "*")]))
            .unwrap();
        assert_eq!(path, "/.well-known/masque/ip/%2A/%2A/");
        let matched = template.match_path(&path).unwrap();
        assert_eq!(matched["target"], "*");
        assert_eq!(matched["ipproto"], "*");
    }

    /// A proxy will also see literal `*` from clients that do not encode it,
    /// since RFC 9484 writes the path as `/.well-known/masque/ip/*/*/`.
    #[test]
    fn unencoded_wildcard_path_is_not_silently_mismatched() {
        let template = UriTemplate::default_connect_ip("example.org").unwrap();
        assert!(template.match_path("/.well-known/masque/ip/*/*/").is_none());
    }

    #[test]
    fn percent_coding_round_trips_and_rejects_garbage() {
        for value in ["plain", "2001:db8::42", "a b", "/../", "%", "\u{e9}"] {
            assert_eq!(pct_decode(&pct_encode(value)).as_deref(), Some(value));
        }
        assert_eq!(pct_decode("%zz"), None);
        assert_eq!(pct_decode("%4"), None);
    }
}
