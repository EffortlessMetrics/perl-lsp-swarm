//! Discriminating `--lib` vs `--all-targets` Clippy occupancy for #9600.
//!
//! Does not invoke Clippy. The executable proof is the exact cargo command in
//! CLAUDE.md plus these occupancy/subject tests. Command ownership remains #9606.

use std::path::{Path, PathBuf};

use crate::source_scan::{
    PanicFamilyLint, SuppressionKind, SuppressionScope, lib_source, panic_family_suppressions,
    skip_balanced,
};

/// Exact package `--lib` command this issue reproduces. Not a second executor.
const LIB_CLIPPY: ClippySubject = ClippySubject {
    selector: "--lib",
    argv_tail: &["--locked", "--no-deps", "--", "-D", "warnings", "-A", "missing_docs"],
    unit_tests: false,
    integration_tests: false,
    benches: false,
};

/// `--tests` is the #9599/#9618 configuration contrast, not the product subject.
/// It still compiles `build.rs` as a prerequisite of the selected targets.
const TESTS_CLIPPY: ClippySubject = ClippySubject {
    selector: "--tests",
    argv_tail: &["--locked", "--no-deps", "--", "-D", "warnings", "-A", "missing_docs"],
    unit_tests: true,
    integration_tests: true,
    benches: false,
};

/// Exact package `--all-targets` command this issue reproduces.
const ALL_TARGETS_CLIPPY: ClippySubject = ClippySubject {
    selector: "--all-targets",
    argv_tail: &["--locked", "--no-deps", "--", "-D", "warnings", "-A", "missing_docs"],
    unit_tests: true,
    integration_tests: true,
    benches: true,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClippySubject {
    selector: &'static str,
    argv_tail: &'static [&'static str],
    unit_tests: bool,
    integration_tests: bool,
    benches: bool,
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_source(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn walk_rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        let file_type =
            entry.file_type().map_err(|error| format!("{}: {error}", path.display()))?;
        if file_type.is_dir() {
            walk_rust_files(&path, out)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn package_rust_files() -> Result<Vec<PathBuf>, String> {
    let root = crate_root();
    let mut files = Vec::new();
    for rel in ["src", "tests", "benches"] {
        let dir = root.join(rel);
        if dir.is_dir() {
            walk_rust_files(&dir, &mut files)?;
        }
    }
    let build = root.join("build.rs");
    if build.is_file() {
        files.push(build);
    }
    files.sort();
    Ok(files)
}

#[test]
fn tests_selector_is_not_all_targets() {
    assert_ne!(
        TESTS_CLIPPY.selector, ALL_TARGETS_CLIPPY.selector,
        "substituting --tests for --all-targets hides benches"
    );
    let tests_omits = occupancy_flags(TESTS_CLIPPY);
    let all_includes = occupancy_flags(ALL_TARGETS_CLIPPY);
    assert!(
        !tests_omits.contains("benches") && all_includes.contains("benches"),
        "--tests omits benches; --all-targets includes them"
    );
    assert_eq!(LIB_CLIPPY.argv_tail, ALL_TARGETS_CLIPPY.argv_tail);
}

fn occupancy_flags(subject: ClippySubject) -> String {
    let mut flags = Vec::new();
    if subject.benches {
        flags.push("benches");
    }
    if subject.unit_tests {
        flags.push("unit_tests");
    }
    if subject.integration_tests {
        flags.push("integration_tests");
    }
    flags.join(",")
}

#[test]
fn lib_selector_hides_unit_and_integration_tests() {
    let lib = occupancy_flags(LIB_CLIPPY);
    let all = occupancy_flags(ALL_TARGETS_CLIPPY);
    assert!(
        !lib.contains("unit_tests") && all.contains("unit_tests"),
        "--lib does not compile cfg(test) unit tests; --all-targets does"
    );
    assert!(
        !lib.contains("integration_tests") && all.contains("integration_tests"),
        "--lib does not compile integration tests; --all-targets does"
    );
}

/// Whether a manifest declares a live `[[bench]]` table header. Comment and
/// string mentions do not count: the bench subject must actually exist for
/// `--all-targets` to include it.
fn manifest_declares_bench_target(manifest: &str) -> bool {
    manifest.lines().any(|line| line.split('#').next().unwrap_or("").trim() == "[[bench]]")
}

#[test]
fn package_has_bench_and_build_subjects_all_targets_must_include() -> Result<(), String> {
    let root = crate_root();
    let manifest = read_source(&root.join("Cargo.toml"))?;
    assert!(
        manifest_declares_bench_target(&manifest),
        "perl-lsp-rs must keep a bench target so --all-targets is not --tests"
    );
    assert!(
        root.join("build.rs").is_file(),
        "perl-lsp-rs must keep build.rs so the crate walk still covers the custom-build script"
    );
    assert!(
        root.join("tests").is_dir(),
        "perl-lsp-rs must keep integration tests so --all-targets includes them"
    );
    assert!(
        !root.join("examples").exists(),
        "do not claim an examples all-target row that is not present"
    );
    Ok(())
}

#[test]
fn test_only_expect_is_absent_from_lib_source() {
    let source = r#"
fn production() {}

#[cfg(test)]
mod tests {
    fn hidden_from_lib() {
        None.expect("test-only expect_used");
    }
}
"#;
    assert!(
        source.contains(".expect("),
        "fixture control: the test-only expect must exist in full source"
    );
    assert!(
        !lib_source(source).contains(".expect("),
        "a lib-only gate cannot see cfg(test) expect_used; --all-targets must"
    );
}

#[test]
fn module_scope_import_used_only_in_tests_stays_in_lib_source() {
    let original_defect = r#"
use crate::features::formatting::{FormatPosition, FormatRange};

fn production() {}

#[cfg(test)]
mod tests {
    fn uses_them() {}
}
"#;
    assert!(
        lib_source(original_defect).contains("FormatPosition"),
        "#9599 unused-import shape must remain a --lib hit"
    );
}

#[test]
fn module_level_panic_family_allow_is_forbidden() {
    let carved = "#![allow(clippy::expect_used)]\nfn production() {}\n";
    let hits = panic_family_suppressions(carved);
    assert_eq!(hits.len(), 1, "inner expect_used allow must be detected");
    assert!(hits[0].is_forbidden(), "module-wide allow is a blanket carve-out");
    assert_eq!(hits[0].scope, SuppressionScope::Inner);
    assert_eq!(hits[0].kind, SuppressionKind::Allow);
}

#[test]
fn module_level_expect_with_debt_reason_is_still_forbidden() {
    let carved = r#"#![expect(
    clippy::unwrap_used,
    reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]
fn production() {}
"#;
    let hits = panic_family_suppressions(carved);
    assert_eq!(hits.len(), 1);
    assert!(
        hits[0].is_forbidden(),
        "file-wide expect is still a blanket carve-out even with a reason"
    );
    assert!(hits[0].has_reason);
    assert_eq!(hits[0].lints, vec![PanicFamilyLint::UnwrapUsed]);
}

#[test]
fn cfg_test_wide_expect_used_is_forbidden() {
    let carved = "#![cfg_attr(test, allow(clippy::expect_used))]\nfn production() {}\n";
    let hits = panic_family_suppressions(carved);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].is_forbidden());
    assert_eq!(hits[0].scope, SuppressionScope::Inner);
}

#[test]
fn item_level_expect_with_reason_is_allowed() {
    let allowed = r#"
#[expect(clippy::panic, reason = "the handler under test must actually panic")]
fn boom() {
    panic!("provider exploded");
}
"#;
    let hits = panic_family_suppressions(allowed);
    assert_eq!(hits.len(), 1);
    assert!(
        !hits[0].is_forbidden(),
        "item-level expect with reason is the accepted deliberate-panic form"
    );
    assert!(!hits[0].decorates_wide_item);
    assert_eq!(hits[0].scope, SuppressionScope::Outer);
    assert_eq!(hits[0].kind, SuppressionKind::Expect);
}

#[test]
fn outer_expect_on_mod_is_forbidden() {
    let carved = r#"
#[expect(clippy::panic, reason = "one panic in this module is enough")]
mod tests {
    fn boom() { panic!("one"); }
    fn also_boom() { panic!("two"); }
}
"#;
    let hits = panic_family_suppressions(carved);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].is_forbidden(), "expect on mod is still a module-wide blanket");
    assert!(hits[0].decorates_wide_item);
    assert_eq!(hits[0].scope, SuppressionScope::Outer);
    assert!(hits[0].has_reason);
}

#[test]
fn stacked_outer_expect_on_mod_is_forbidden() {
    let carved = r#"
#[expect(clippy::unwrap_used, reason = "tracked conversion debt")]
#[cfg(test)]
mod tests {
    fn hidden() { None::<()>.unwrap(); }
}
"#;
    let hits = panic_family_suppressions(carved);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].is_forbidden());
    assert!(hits[0].decorates_wide_item);
}

#[test]
fn outer_expect_on_impl_is_forbidden() {
    let carved = r#"
struct Probe;
#[expect(clippy::panic, reason = "the handler under test must actually panic")]
impl Probe {
    fn boom() { panic!("impl-wide"); }
}
"#;
    let hits = panic_family_suppressions(carved);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].is_forbidden(), "expect on impl is still a blanket");
    assert!(hits[0].decorates_wide_item);
}

#[test]
fn item_level_allow_is_forbidden_even_with_reason() {
    let carved = r#"
#[allow(clippy::panic, reason = "the handler under test must actually panic")]
fn boom() {
    panic!("provider exploded");
}
"#;
    let hits = panic_family_suppressions(carved);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].is_forbidden(), "item-level allow must be expect-with-reason");
}

#[test]
fn print_only_allow_is_not_a_panic_family_hit() {
    let print_only = "#![allow(clippy::print_stdout, clippy::print_stderr)]\n";
    assert!(panic_family_suppressions(print_only).is_empty());
}

#[test]
fn comment_and_string_do_not_create_false_carve_outs() {
    let source = r##"
fn production() {}
// #![allow(clippy::expect_used)]
const MARKER: &str = "#![allow(clippy::panic)]";
"##;
    assert!(
        panic_family_suppressions(source).is_empty(),
        "comment/string markers must not count as live suppressions"
    );
}

#[test]
fn nested_block_comment_is_not_a_live_carve_out() {
    let source = r##"
/* outer comment /* inner comment #![allow(clippy::panic)] */
#![allow(clippy::expect_used)] still inside the outer comment */
fn production() {}
"##;
    assert!(
        panic_family_suppressions(source).is_empty(),
        "text after an inner block-comment close is still inside the outer \
         comment and must not count as a live suppression"
    );
}

#[test]
fn bench_target_declaration_requires_live_table_header() {
    let commented = "# a [[bench]] mention\n[package]\nname = \"x [[bench]] y\"\n";
    assert!(
        !manifest_declares_bench_target(commented),
        "a comment or string mention of [[bench]] must not satisfy bench occupancy"
    );
    let declared = "[package]\nname = \"x\"\n\n[[bench]]\nname = \"occupied\"\nharness = false\n";
    assert!(manifest_declares_bench_target(declared));
}

#[test]
fn mixed_print_and_expect_inner_allow_is_forbidden() {
    let mixed = "#![allow(clippy::print_stderr, clippy::expect_used)]\n";
    let hits = panic_family_suppressions(mixed);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].is_forbidden());
    assert_eq!(hits[0].lints, vec![PanicFamilyLint::ExpectUsed]);
}

#[test]
fn package_guidance_names_lib_and_all_targets() -> Result<(), String> {
    let claude = read_source(&crate_root().join("CLAUDE.md"))?;
    let verify = claude
        .split("## Verify")
        .nth(1)
        .and_then(|rest| rest.split("## ").next())
        .ok_or_else(|| "CLAUDE.md must keep a Verify section".to_string())?;
    assert!(
        verify.contains("cargo clippy -p perl-lsp-rs --lib"),
        "Verify must retain the --lib canary"
    );
    assert!(
        verify.contains("cargo clippy -p perl-lsp-rs --all-targets"),
        "Verify must name --all-targets; --tests is not the product subject"
    );
    assert!(
        verify.contains("--locked --no-deps"),
        "Verify must keep the exact locked no-deps lint arguments"
    );
    assert!(
        !verify.trim().lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("cargo clippy")
                && trimmed.contains("--tests")
                && !trimmed.contains("--all-targets")
        }),
        "Verify must not present --tests as the Clippy product command"
    );
    assert!(
        !verify.contains("omits benches/build"),
        "Verify must not claim --tests omits build.rs; Cargo compiles it as a prerequisite"
    );
    Ok(())
}

#[test]
fn windows_sandbox_fail_closed_uses_must_err_not_unwrap() -> Result<(), String> {
    let source = read_source(&crate_root().join("src/security/sandbox.rs"))?;
    let start = source
        .find("fn test_windows_sandbox_fails_closed")
        .ok_or_else(|| "sandbox.rs must keep the Windows fail-closed test".to_string())?;
    let rest = source.get(start..).ok_or_else(|| "Windows fail-closed test slice".to_string())?;
    let brace =
        rest.find('{').ok_or_else(|| "Windows fail-closed test must have a body".to_string())?;
    let end = skip_balanced(&source, start + brace, '{', '}');
    let body =
        source.get(start..end).ok_or_else(|| "Windows fail-closed body bounds".to_string())?;
    assert!(
        !body.contains("unwrap_err"),
        "Windows --all-targets compiles this test; unwrap_err is clippy::unwrap_used"
    );
    assert!(body.contains("must_err"), "Windows fail-closed must use must_err like the Linux twin");
    Ok(())
}

#[test]
fn package_has_no_forbidden_panic_family_blankets() -> Result<(), String> {
    let mut hits = Vec::new();
    for path in package_rust_files()? {
        let source = read_source(&path)?;
        for suppression in panic_family_suppressions(&source) {
            if suppression.is_forbidden() {
                hits.push(format!(
                    "{} {:?} {:?} {:?}",
                    path.display(),
                    suppression.scope,
                    suppression.kind,
                    suppression.lints
                ));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "forbidden panic-family blankets (inner/file-wide or allow-without-expect):\n{}",
        hits.join("\n")
    );
    Ok(())
}
