//! Render the website's generated pages into `site/` (or `--out <dir>`).
//! `--check` writes nothing and exits 1 if any committed page is stale.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut out = PathBuf::from("site");
    let mut check = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" => check = true,
            "--out" => match args.next() {
                Some(dir) => out = PathBuf::from(dir),
                None => {
                    eprintln!("sitegen: --out needs a directory");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!(
                    "sitegen: unknown argument {other:?}\nusage: sitegen [--out <dir>] [--check]"
                );
                return ExitCode::from(2);
            }
        }
    }

    if check {
        let stale = skimasque_visual::site::stale(&out);
        if stale.is_empty() {
            return ExitCode::SUCCESS;
        }
        for path in stale {
            eprintln!("stale: {}", out.join(&path).display());
        }
        eprintln!("run `cargo run -p skimasque-visual --features site --bin sitegen` and commit the result");
        return ExitCode::from(1);
    }

    for page in skimasque_visual::site::pages() {
        let path = out.join(page.path);
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("sitegen: creating {}: {e}", parent.display());
                return ExitCode::from(1);
            }
        }
        if let Err(e) = std::fs::write(&path, page.contents) {
            eprintln!("sitegen: writing {}: {e}", path.display());
            return ExitCode::from(1);
        }
        println!("wrote {}", path.display());
    }
    ExitCode::SUCCESS
}
