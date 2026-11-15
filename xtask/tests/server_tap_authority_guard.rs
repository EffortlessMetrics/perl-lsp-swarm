//! Reject a second server-side TAP line grammar outside `perl-test-facts` (#6943 / #7678).
//!
//! The LSP compatibility projection and runtime adapters may map canonical facts
//! into product shapes, but they must not reintroduce raw `ok` / `not ok` line
//! classification in production Rust sources.

use std::{
    fs,
    path::{Path, PathBuf},
};

const SCAN_ROOTS: &[&str] =
    &["crates/perl-lsp-rs/src", "crates/perl-lsp-rs-core/src", "crates/perl-tdd-support/src"];

const FORBIDDEN_SNIPPETS: &[&str] = &[
    "starts_with(\"ok \")",
    "starts_with(\"not ok \")",
    "strip_prefix(\"ok \")",
    "strip_prefix(\"not ok \")",
];

#[test]
fn server_sources_do_not_reintroduce_raw_tap_line_classifiers() {
    let workspace = workspace_root();
    let mut violations = Vec::new();

    for root in SCAN_ROOTS {
        let dir = workspace.join(root);
        if !dir.is_dir() {
            continue;
        }
        walk_rust_sources(&dir, &mut |path, line_number, line| {
            if line.trim_start().starts_with("//") {
                return;
            }
            for snippet in FORBIDDEN_SNIPPETS {
                if line.contains(snippet) {
                    violations.push(format!(
                        "{}:{}: found `{snippet}` — route TAP through `perl-test-facts`",
                        path.strip_prefix(&workspace).unwrap_or(path).display(),
                        line_number
                    ));
                }
            }
        })
        .expect("scan server sources for forbidden TAP classifiers");
    }

    assert!(violations.is_empty(), "server TAP authority violations:\n{}", violations.join("\n"));
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn walk_rust_sources(dir: &Path, visit: &mut dyn FnMut(&Path, usize, &str)) -> std::io::Result<()> {
    let entries = fs::read_dir(dir).map_err(|error| {
        std::io::Error::new(error.kind(), format!("read_dir failed for {}: {error}", dir.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("read_dir entry failed in {}: {error}", dir.display()),
            )
        })?;
        let path = entry.path();
        if path.is_dir() {
            walk_rust_sources(&path, visit)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let source = fs::read_to_string(&path).map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("read failed for {}: {error}", path.display()),
                )
            })?;
            for (index, line) in source.lines().enumerate() {
                visit(&path, index + 1, line);
            }
        }
    }
    Ok(())
}
