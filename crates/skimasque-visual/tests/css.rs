//! The stylesheet keeps every hex colour inside its token block, and no
//! component template hard-codes a colour.

fn hex_colours(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in line.match_indices('#') {
        let hex: String = line[i + 1..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        let next = line[i + 1 + hex.len()..].chars().next().unwrap_or(' ');
        if matches!(hex.len(), 3 | 6 | 8) && !next.is_ascii_alphanumeric() && next != '-' {
            out.push(hex);
        }
    }
    out
}

#[test]
fn hex_colours_live_only_in_the_token_block() {
    let css = skimasque_visual::CSS;
    let mut in_tokens = false;
    let mut seen_tokens = false;
    for (n, line) in css.lines().enumerate() {
        if line.contains("/* tokens */") {
            in_tokens = true;
            seen_tokens = true;
            continue;
        }
        if line.contains("/* end tokens */") {
            in_tokens = false;
            continue;
        }
        if !in_tokens {
            assert!(
                hex_colours(line).is_empty(),
                "visual.css:{}: hex outside tokens: {line}",
                n + 1
            );
        }
    }
    assert!(seen_tokens, "token block markers present");
}

#[test]
fn the_palette_matches_the_canonical_spec() {
    for decl in [
        "--snow: #F4F3ED",
        "--ice: #E5F2EE",
        "--mint: #72C7A5",
        "--pine: #183C35",
        "--forest: #28584C",
        "--earth: #795C43",
        "--slate: #66736F",
    ] {
        assert!(skimasque_visual::CSS.contains(decl), "missing {decl}");
    }
}

struct Rule {
    selector: String,
    decls: Vec<(String, String)>,
}

/// Every rule that declares custom properties (`--x: y`), comments removed.
fn rules(css: &str) -> Vec<Rule> {
    let mut clean = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        clean.push_str(&rest[..i]);
        let end = rest[i..].find("*/").expect("closed comment");
        rest = &rest[i + end + 2..];
    }
    clean.push_str(rest);
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut buf = String::new();
    for ch in clean.chars() {
        match ch {
            '{' => {
                stack.push(buf.trim().to_owned());
                buf.clear();
            }
            '}' => {
                let selector = stack.pop().expect("balanced braces");
                let decls: Vec<(String, String)> = buf
                    .split(';')
                    .filter_map(|d| {
                        let (k, v) = d.split_once(':')?;
                        let k = k.trim();
                        k.starts_with("--")
                            .then(|| (k.to_owned(), v.trim().to_owned()))
                    })
                    .collect();
                if !decls.is_empty() {
                    out.push(Rule { selector, decls });
                }
                buf.clear();
            }
            c => buf.push(c),
        }
    }
    out
}

fn rule<'a>(rules: &'a [Rule], selector: &str) -> &'a Rule {
    rules
        .iter()
        .find(|r| r.selector == selector)
        .unwrap_or_else(|| panic!("no rule `{selector}`"))
}

const LIGHT: &str = r#":root, [data-theme="light"]"#;
const DARK: &str = r#"[data-theme="dark"]"#;
const AUTO: &str = r#":root:not([data-theme="light"])"#;

#[test]
fn the_three_theme_blocks_declare_the_same_roles() {
    let rules = rules(skimasque_visual::CSS);
    let names = |sel: &str| {
        let mut v: Vec<&str> = rule(&rules, sel)
            .decls
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        v.sort_unstable();
        v
    };
    assert_eq!(names(LIGHT), names(DARK), "light vs dark");
    assert_eq!(names(DARK), names(AUTO), "dark vs OS-dark");
    let auto = &rule(&rules, AUTO).decls;
    let dark = &rule(&rules, DARK).decls;
    assert_eq!(
        auto, dark,
        "the OS-dark block must repeat the dark values exactly"
    );
}

fn resolve(name: &str, theme: &Rule, tokens: &Rule) -> Option<String> {
    let value = theme
        .decls
        .iter()
        .chain(tokens.decls.iter())
        .find(|(k, _)| k == name)?
        .1
        .clone();
    match value.strip_prefix("var(").and_then(|v| v.strip_suffix(')')) {
        Some(inner) => resolve(inner.trim(), theme, tokens),
        None => Some(value),
    }
}

fn luminance(hex: &str) -> f64 {
    let hex = hex.trim_start_matches('#');
    let ch = |i: usize| {
        let c = u8::from_str_radix(&hex[i..i + 2], 16).unwrap() as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(0) + 0.7152 * ch(2) + 0.0722 * ch(4)
}

fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[test]
fn every_text_role_meets_4_5_to_1_on_every_surface_in_both_themes() {
    let rules = rules(skimasque_visual::CSS);
    let tokens = rule(&rules, ":root");
    let texts = [
        "--text",
        "--text-soft",
        "--text-muted",
        "--accent-text",
        "--success",
        "--warning",
        "--danger",
        "--info",
        "--v-active",
        "--v-deny",
        "--v-info",
        "--v-warning",
        "--v-structure-text",
        "--v-edge-text",
        "--v-neutral-text",
    ];
    let grounds = [
        "--bg",
        "--surface",
        "--surface-raised",
        "--input",
        "--v-structure-fill",
        "--v-edge-fill",
    ];
    for (theme_name, sel) in [("light", LIGHT), ("dark", DARK)] {
        let theme = rule(&rules, sel);
        for t in texts {
            for g in grounds {
                let fg = resolve(t, theme, tokens).unwrap_or_else(|| panic!("{t} unresolved"));
                let bg = resolve(g, theme, tokens).unwrap_or_else(|| panic!("{g} unresolved"));
                let ratio = contrast(&fg, &bg);
                assert!(
                    ratio >= 4.5,
                    "{theme_name}: {t} {fg} on {g} {bg} = {ratio:.2}"
                );
            }
        }
        let (fg, bg) = (
            resolve("--on-accent", theme, tokens).unwrap(),
            resolve("--accent", theme, tokens).unwrap(),
        );
        assert!(
            contrast(&fg, &bg) >= 4.5,
            "{theme_name}: on-accent on accent"
        );
    }
}

fn files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn templates_hard_code_no_colours() {
    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/templates"));
    for path in files(dir) {
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            assert!(
                hex_colours(line).is_empty(),
                "{}:{}: {line}",
                path.display(),
                n + 1
            );
        }
    }
}

#[test]
fn code_block_palette_pairs_meet_4_5_to_1() {
    let rules = rules(skimasque_visual::CSS);
    let t = rule(&rules, ":root");
    let get = |n: &str| t.decls.iter().find(|(k, _)| k == n).unwrap().1.clone();
    for fg in ["--snow", "--ice", "--slate-300", "--mint"] {
        assert!(
            contrast(&get(fg), &get("--pine-950")) >= 4.5,
            "{fg} on code bg"
        );
    }
}

fn rgb(hex: &str) -> [f64; 3] {
    let h = hex.trim_start_matches('#');
    [0, 2, 4].map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f64)
}

fn to_hex(c: [f64; 3]) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        c[0].round() as u8,
        c[1].round() as u8,
        c[2].round() as u8
    )
}

/// Alpha-blend `top` (r, g, b in 0..=255) at `alpha` over the `ground` hex.
fn blend(top: [f64; 3], alpha: f64, ground: &str) -> String {
    let g = rgb(ground);
    to_hex([0, 1, 2].map(|i| top[i] * alpha + g[i] * (1.0 - alpha)))
}

fn parse_rgba(v: &str) -> ([f64; 3], f64) {
    let inner = v
        .strip_prefix("rgba(")
        .and_then(|v| v.strip_suffix(')'))
        .unwrap_or_else(|| panic!("not rgba(): {v}"));
    let p: Vec<f64> = inner
        .split(',')
        .map(|s| s.trim().parse().unwrap())
        .collect();
    ([p[0], p[1], p[2]], p[3])
}

#[test]
fn translucent_tints_keep_their_text_at_4_5_to_1_on_every_ground() {
    let rules = rules(skimasque_visual::CSS);
    let tokens = rule(&rules, ":root");
    let grounds = ["--bg", "--surface", "--surface-raised"];
    for (theme_name, sel) in [("light", LIGHT), ("dark", DARK)] {
        let theme = rule(&rules, sel);
        let get = |n: &str| resolve(n, theme, tokens).unwrap_or_else(|| panic!("{n} unresolved"));
        let (soft, soft_a) = parse_rgba(&get("--accent-soft"));
        let deny = rgb(&get("--v-deny"));
        for g in grounds {
            let ground = get(g);
            // .v-diff-added: --accent-soft tint; field text is --text-soft there.
            let added = blend(soft, soft_a, &ground);
            for t in ["--text", "--text-soft", "--v-active"] {
                let r = contrast(&get(t), &added);
                println!("{theme_name} added {t} on {g}: {r:.2}");
                assert!(
                    r >= 4.5,
                    "{theme_name}: {t} on added tint over {g} = {r:.2}"
                );
            }
            // .v-check-fail: --v-deny at 8%.
            let failed = blend(deny, 0.08, &ground);
            for t in ["--text", "--text-muted", "--v-deny"] {
                let r = contrast(&get(t), &failed);
                println!("{theme_name} failed {t} on {g}: {r:.2}");
                assert!(
                    r >= 4.5,
                    "{theme_name}: {t} on failed tint over {g} = {r:.2}"
                );
            }
        }
        let r = contrast(&get("--on-accent"), &get("--accent-hover"));
        println!("{theme_name} on-accent on accent-hover: {r:.2}");
        assert!(r >= 4.5, "{theme_name}: on-accent on accent-hover = {r:.2}");
    }
}

struct StyleRule {
    selectors: Vec<String>,
    body: String,
    reduced: bool,
}

fn strip_comments(css: &str) -> String {
    let mut out = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = match rest[i..].find("*/") {
            Some(j) => &rest[i + j + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// Style rules by brace depth. `@media` (and other conditional groups) are
/// transparent; a `prefers-reduced-motion: reduce` condition marks its rules.
/// `@keyframes` and `@font-face` bodies are skipped.
fn parse_rules(css: &str) -> Vec<StyleRule> {
    fn block_end(b: &[u8], mut i: usize) -> usize {
        // `i` is just past an opening brace; returns the index after its match.
        let mut depth = 1;
        while i < b.len() && depth > 0 {
            match b[i] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        i
    }
    fn walk(css: &str, reduced: bool, out: &mut Vec<StyleRule>) {
        let b = css.as_bytes();
        let mut i = 0;
        let mut start = 0;
        while i < b.len() {
            match b[i] {
                b'{' => {
                    let prelude = css[start..i].trim();
                    let end = block_end(b, i + 1);
                    let inner = &css[i + 1..end - 1];
                    if let Some(at) = prelude.strip_prefix('@') {
                        let norm: String = at.split_whitespace().collect::<Vec<_>>().join(" ");
                        if norm.starts_with("keyframes") || norm.starts_with("font-face") {
                            // out of scope
                        } else {
                            let is_reduce = norm
                                .replace(" :", ":")
                                .contains("prefers-reduced-motion: reduce");
                            walk(inner, reduced || is_reduce, out);
                        }
                    } else {
                        out.push(StyleRule {
                            selectors: prelude.split(',').map(|s| s.trim().to_string()).collect(),
                            body: inner.to_string(),
                            reduced,
                        });
                    }
                    i = end;
                    start = end;
                }
                _ => i += 1,
            }
        }
    }
    let mut out = Vec::new();
    walk(&strip_comments(css), false, &mut out);
    out
}

/// Animation declarations in a body as (property, value).
fn animation_decls(body: &str) -> Vec<(String, String)> {
    body.split(';')
        .filter_map(|d| d.split_once(':'))
        .map(|(p, v)| (p.trim().to_ascii_lowercase(), v.trim().to_string()))
        .filter(|(p, _)| {
            matches!(
                p.as_str(),
                "animation" | "animation-name" | "-webkit-animation" | "-webkit-animation-name"
            )
        })
        .collect()
}

#[test]
fn every_animation_is_switched_off_under_reduced_motion() {
    let rules = parse_rules(skimasque_visual::CSS);
    let mut seen = 0;
    for rule in rules.iter().filter(|r| !r.reduced) {
        let animated = animation_decls(&rule.body).iter().any(|(_, v)| v != "none");
        if !animated {
            continue;
        }
        seen += 1;
        for sel in &rule.selectors {
            let switched_off = rules.iter().any(|o| {
                o.reduced
                    && o.selectors.contains(sel)
                    && animation_decls(&o.body).iter().any(|(_, v)| v == "none")
            });
            assert!(switched_off, "no reduced-motion override for `{sel}`");
        }
    }
    assert!(seen >= 4, "the guard found the animated rules (saw {seen})");
}

#[test]
fn expiry_never_fades_text() {
    let rules = parse_rules(skimasque_visual::CSS);
    let mut seen = 0;
    for rule in &rules {
        let expire = rule.selectors.iter().any(|s| s.contains(".v-expire"));
        if !expire {
            continue;
        }
        seen += 1;
        if body_sets_opacity(&rule.body) {
            assert!(
                rule.selectors
                    .iter()
                    .all(|s| s.trim_end().ends_with(".v-icon")),
                "`{}` sets opacity; only .v-icon may fade under .v-expire",
                rule.selectors.join(", ")
            );
        }
    }
    assert!(seen >= 2, "found the .v-expire rules");
}

fn body_sets_opacity(body: &str) -> bool {
    body.split(';')
        .filter_map(|d| d.split_once(':'))
        .any(|(p, _)| p.trim().eq_ignore_ascii_case("opacity"))
}

#[test]
fn a_branch_has_definite_tracks_so_a_long_flow_root_cannot_starve_its_arms() {
    let css = strip_comments(include_str!("../static/visual.css"));
    let norm: String = css.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        norm.contains(".v-branch { grid-template-columns: minmax(0, 2fr) minmax(0, 3fr);"),
        "wide branches use definite tracks"
    );
    assert!(
        !norm.contains("width: max-content"),
        "no max-content override on a branch root"
    );
    assert!(norm.contains(".v-branch-arm .v-flow-wrap { width: auto; flex: 1 1 0; min-width: 0; }"));
}
