use color_eyre::eyre::Result;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

use crate::{GREEN, NC, RED, YELLOW, display_path, path_has_component, read_lines, walk_entries};

static ABORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"std::process::abort\s*\(")
        .unwrap_or_else(|error| unreachable!("ABORT_RE is a known-good static pattern: {error}"))
});

static EXIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"std::process::exit\s*\(")
        .unwrap_or_else(|error| unreachable!("EXIT_RE is a known-good static pattern: {error}"))
});

pub(crate) fn forbid_fatal_constructs(repo_root: &Path, verbose: bool) -> Result<i32> {
    let mut aborts = Vec::new();
    let mut exits = Vec::new();

    let crates_root = repo_root.join("crates");
    for entry in walk_entries(&crates_root) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let rel = display_path(repo_root, path);
        if is_fatal_excluded(path, repo_root)? {
            continue;
        }
        let lines = read_lines(path)?;
        for (line_no, line) in lines.iter().enumerate() {
            let number = line_no + 1;
            if ABORT_RE.is_match(line) {
                aborts.push(format!("{rel}:{number}:{line}"));
            }
            if EXIT_RE.is_match(line) {
                exits.push(format!("{rel}:{number}:{line}"));
            }
        }
    }

    if !aborts.is_empty() {
        println!("{RED}ERROR: std::process::abort() found in production code{NC}");
        println!();
        println!("abort() is never allowed - it terminates without unwinding.");
        println!("==================================================");
        for hit in &aborts {
            println!("{hit}");
        }
        println!("==================================================");
        println!();
        println!("To fix: return an error and let the caller handle it.");
        println!();
    }

    let exit_violations: Vec<String> =
        exits.into_iter().filter(|hit| !is_allowlisted_exit_hit(hit)).collect();

    if !exit_violations.is_empty() {
        println!("{RED}ERROR: std::process::exit() found outside allowlist{NC}");
        println!();
        println!("exit() is only allowed in:");
        println!("  - bin/ directories (CLI entry points)");
        println!("  - lifecycle.rs (LSP exit handler)");
        println!("==================================================");
        for hit in &exit_violations {
            println!("{hit}");
        }
        println!("==================================================");
        println!();
        println!("To fix: return an error, use Result<(), E>, or move to an allowlisted path.");
        println!();
    }

    if (!aborts.is_empty()) || !exit_violations.is_empty() {
        return Ok(1);
    }

    if verbose {
        println!("{GREEN}OK: No forbidden fatal constructs in production code{NC}");
        println!();
        println!("{YELLOW}Policy summary:{NC}");
        println!("  - abort(): NEVER allowed (banned everywhere)");
        println!("  - exit():  allowed in bin/ and lifecycle.rs only");
        println!();
        println!("{YELLOW}Note: panic!/unwrap!/expect! are enforced by Clippy deny lints:{NC}");
        println!("  - clippy::panic, clippy::unwrap_used, clippy::expect_used");
        println!("  - See [workspace.lints.clippy] in Cargo.toml");
    }
    Ok(0)
}

fn is_fatal_excluded(path: &Path, repo_root: &Path) -> Result<bool> {
    let rel = path.strip_prefix(repo_root).unwrap_or(path).to_path_buf();
    let rel_string = format!("/{}", normalize_path_for_match(&rel.display().to_string()));

    if rel_string.contains("/tests/") {
        return Ok(true);
    }
    if rel_string.contains("/benches/") {
        return Ok(true);
    }
    if path.file_name().is_some_and(|name| name == "build.rs") {
        return Ok(true);
    }
    if path.file_name().is_some_and(|name| {
        name.to_string_lossy().ends_with("_test.rs")
            || name.to_string_lossy().ends_with("_tests.rs")
    }) {
        return Ok(true);
    }
    for excluded in ["tree-sitter-perl-c", "perl-tdd-support", "perl-ci-hygiene"] {
        if rel_string.contains(&format!("/{excluded}/")) {
            return Ok(true);
        }
    }

    Ok(path_has_component(path, "tests")
        || path_has_component(path, "benches")
        || path_has_component(path, "build.rs")
        || path_has_component(path, "examples"))
}

pub(crate) fn normalize_path_for_match(value: &str) -> String {
    value.replace('\\', "/")
}

pub(crate) fn is_allowlisted_exit_hit(hit: &str) -> bool {
    let normalized = normalize_path_for_match(hit);
    normalized.contains("/bin/") || normalized.contains("/lifecycle.rs:")
}
