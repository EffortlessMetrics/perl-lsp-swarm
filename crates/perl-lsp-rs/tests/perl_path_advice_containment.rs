//! Crate-wide containment for the "remediation names a setting nobody can set"
//! defect class.
//!
//! It has been fixed four times in four files — #969 (extension onboarding),
//! #5034 and #5373 (interpreter detection), #5376 (execute-command and
//! `perl.debugFile`) — each time by a guard scoped to the file being fixed, so
//! the next instance in the next file went uncaught. #5373's guards matched
//! `perl-lsp.perl.path` and could not see the two messages that said
//! `perl.path`.
//!
//! This test does not read message text. It asserts *where* the token may
//! appear at all: in the shared remediation constant's documentation, and in
//! the guards that assert its absence. A new message that names the setting
//! lands in some other file and fails here, whatever wording it uses.
//!
//! Widening the allowlist is the deliberate review moment this exists to force.

use std::path::{Path, PathBuf};

/// Files permitted to contain the literal token, and why.
const ALLOWED: &[(&str, &str)] = &[
    ("src/perl_remediation.rs", "documents why the setting is never named, and guards it"),
    (
        "src/runtime/lifecycle/workspace.rs",
        "asserts the interpreter-detection messages never name it (#5034/#5373)",
    ),
    (
        "src/execute_command/provider/perl_remediation_tests.rs",
        "asserts the execute-command message never names it (#5376)",
    ),
    (
        "src/runtime/language/misc/debug_launch.rs",
        "asserts the perl.debugFile error never names it (#5376)",
    ),
];

fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_sources(&path, found)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
    Ok(())
}

#[test]
fn only_the_remediation_owner_and_its_guards_name_the_unsettable_setting()
-> Result<(), Box<dyn std::error::Error>> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = crate_root.join("src");

    let mut sources = Vec::new();
    rust_sources(&src, &mut sources)?;
    assert!(!sources.is_empty(), "found no Rust sources under {}", src.display());

    let allowed: Vec<PathBuf> = ALLOWED.iter().map(|(path, _)| crate_root.join(path)).collect();

    let mut offenders = Vec::new();
    for path in sources {
        if allowed.contains(&path) {
            continue;
        }
        if std::fs::read_to_string(&path)?.contains("perl.path") {
            offenders.push(path.strip_prefix(crate_root).unwrap_or(&path).display().to_string());
        }
    }

    assert!(
        offenders.is_empty(),
        "`perl.path` names a setting no user-facing channel can write, so no message may \
         mention it. Found it in: {}. If a new site genuinely needs the token — a guard, or \
         documentation of why it is never advised — add it to ALLOWED in this test with a \
         reason. If an interpreter-path channel was actually wired, update \
         `PERL_REMEDIATION` and the DAP-side guidance together (#5376).",
        offenders.join(", ")
    );

    Ok(())
}

/// The allowlist itself must not rot: an entry naming a file that no longer
/// exists, or one that no longer contains the token, is stale permission.
#[test]
fn every_allowlisted_file_exists_and_still_needs_its_entry()
-> Result<(), Box<dyn std::error::Error>> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    for (relative, reason) in ALLOWED {
        let path = crate_root.join(relative);
        assert!(path.is_file(), "allowlisted file {relative} does not exist ({reason})");
        assert!(
            std::fs::read_to_string(&path)?.contains("perl.path"),
            "allowlisted file {relative} no longer contains the token; drop the entry ({reason})"
        );
    }

    Ok(())
}
