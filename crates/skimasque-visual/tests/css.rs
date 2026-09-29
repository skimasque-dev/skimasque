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
fn the_palette_matches_the_design_guide() {
    for decl in [
        "--alpine-950: #0b1117",
        "--mint-500: #63d7b1",
        "--forest-800: #173026",
        "--earth-700: #4a3527",
        "--snow-50: #f7faf9",
        "--danger-500: #e87979",
        "--forest-ink: #2a6a4e",
        "--mint-ink: #0b7a5b",
    ] {
        assert!(skimasque_visual::CSS.contains(decl), "missing {decl}");
    }
}

#[test]
fn templates_hard_code_no_colours() {
    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/templates"));
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
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
