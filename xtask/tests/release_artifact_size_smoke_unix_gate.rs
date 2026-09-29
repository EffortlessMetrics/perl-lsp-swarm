//! Discriminating proof that `release_artifact_size_smoke_script` is crate-gated
//! Unix-only (#14355).
//!
//! The integration target imports `std::os::unix` and drives a bash script, so
//! the whole crate must be `#![cfg(unix)]`. A doc-comment mention, a trailing
//! line comment, a block comment, or an outer `#[cfg(unix)]` on a later item
//! still leaves the Unix import in the Windows compile surface.
//!
//! This target itself is *not* Unix-gated: it must keep compiling (and running)
//! on Windows so the gate cannot bit-rot behind `#![cfg(unix)]` docs.

use std::{fs, path::PathBuf};

/// Drop `//` line comments (including `//!` / `///`) and `/* */` block comments.
/// Nested block comments are not required for this seam.
fn uncommented(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_block = false;
    while let Some(c) = chars.next() {
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            in_block = true;
            continue;
        }
        out.push(c);
    }
    out
}

/// A whole-line crate-root inner `cfg(unix)` appears before any `os::unix` item.
fn crate_root_unix_gate_precedes_unix_import(source: &str) -> bool {
    let code = uncommented(source);
    let gate = code.lines().position(|line| line.trim() == "#![cfg(unix)]");
    let unix_import = code.lines().position(|line| line.contains("os::unix"));
    match (gate, unix_import) {
        (Some(gate), Some(unix_import)) => gate < unix_import,
        _ => false,
    }
}

fn smoke_script_source() -> Result<String, Box<dyn std::error::Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("release_artifact_size_smoke_script.rs");
    Ok(fs::read_to_string(path)?)
}

#[test]
fn live_smoke_script_is_crate_gated_before_the_unix_import()
-> Result<(), Box<dyn std::error::Error>> {
    let source = smoke_script_source()?;
    assert!(
        crate_root_unix_gate_precedes_unix_import(&source),
        "xtask/tests/release_artifact_size_smoke_script.rs must carry #![cfg(unix)] in code, before any os::unix import"
    );
    Ok(())
}

#[test]
fn doc_comment_mention_of_the_gate_is_not_enough() {
    let original = r#"//! `#![cfg(unix)]` keeps the target compiling (as nothing) on other hosts.
use std::os::unix::fs::PermissionsExt;
"#;
    assert!(
        !crate_root_unix_gate_precedes_unix_import(original),
        "a docs-only mention of the gate still leaves the Unix import in the compile surface"
    );
}

#[test]
fn trailing_line_comment_is_not_a_crate_gate() {
    let source = "use std::os::unix::fs::PermissionsExt; // #![cfg(unix)]\n";
    assert!(!crate_root_unix_gate_precedes_unix_import(source));
}

#[test]
fn block_comment_is_not_a_crate_gate() {
    let source = "/* #![cfg(unix)] */\nuse std::os::unix::fs::PermissionsExt;\n";
    assert!(!crate_root_unix_gate_precedes_unix_import(source));
}

#[test]
fn multiline_block_comment_interior_is_not_a_crate_gate() {
    let source = "/*\n#![cfg(unix)]\n*/\nuse std::os::unix::fs::PermissionsExt;\n";
    assert!(!crate_root_unix_gate_precedes_unix_import(source));
}

#[test]
fn outer_cfg_on_the_use_does_not_empty_the_target() {
    let source = r#"
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
"#;
    assert!(
        !crate_root_unix_gate_precedes_unix_import(source),
        "an outer #[cfg(unix)] gates one item, not the integration crate"
    );
}

#[test]
fn crate_root_gate_after_the_unix_import_is_too_late() {
    let source = r#"
use std::os::unix::fs::PermissionsExt;
#![cfg(unix)]
"#;
    assert!(!crate_root_unix_gate_precedes_unix_import(source));
}

#[test]
fn crate_root_gate_before_the_unix_import_is_the_accepted_form() {
    let source = r#"
#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt};
"#;
    assert!(crate_root_unix_gate_precedes_unix_import(source));
}
