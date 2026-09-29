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
