//! `skill` — port of `printSkill` in `src/adapters/cli/architext-cli.mjs` (~line 1033).
//!
//! JS: reads `skills/architext/SKILL.md` relative to package root and
//! prints `content.trimEnd()`.

use std::process;

fn skill_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("ARCHITEXT_SKILL_PATH") {
        return std::path::PathBuf::from(p);
    }
    // During `cargo run` from repo root, or installed relative to package root.
    std::path::PathBuf::from("skills")
        .join("architext")
        .join("SKILL.md")
}

pub fn run() {
    let path = skill_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            // JS: console.log(content.trimEnd())
            println!("{}", content.trim_end());
        }
        Err(err) => {
            eprintln!("Cannot read SKILL.md at {}: {err}", path.display());
            process::exit(1);
        }
    }
}
