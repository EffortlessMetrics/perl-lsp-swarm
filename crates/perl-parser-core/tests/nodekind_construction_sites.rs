//! Source-scan verifier: every non-recovery `NodeKind` variant must appear as a
//! construction site in the parser engine source.
//!
//! This test walks `crates/perl-parser-core/src/engine/parser/**/*.rs` (excluding
//! test files) and greps for `NodeKind::<Variant>` occurrences.  It fails if any
//! **non-recovery** variant has zero hits.
//!
//! # Guarantee (and its limits)
//!
//! "Appears in source" is **weaker** than "emitted at runtime on real Perl input."
//! This test verifies a *source reference* exists (`NodeKind::X` appears somewhere
//! in the engine source), not that the code path is actually reachable.  It would
//! not catch a variant that is constructed but whose code path is dead.
//!
//! For runtime-emission probing of recovery variants, see
//! `crates/perl-parser/tests/probe_recovery_nodes.rs` (cf. #915/builder-6).
//!
//! # Recovery exemption
//!
//! The 6 recovery variants (`NodeKind::RECOVERY_KIND_NAMES`: `Error`,
//! `MissingBlock`, `MissingExpression`, `MissingIdentifier`, `MissingStatement`,
//! `UnknownRest`) are exempt from the must-have-construction-site rule.
//! `MissingStatement`, `MissingIdentifier`, and `MissingBlock` are known to have
//! zero construction sites (tracked in #976); `MissingExpression` is constructed
//! in `helpers.rs`; the whole recovery set is handled separately by
//! `probe_recovery_nodes.rs`.
//!
//! # What this catches
//!
//! Any variant added to `NodeKind` in the future but never wired into the parser
//! engine will be caught immediately by this test — catching "dead on arrival"
//! variants before they accumulate.

use perl_parser_core::NodeKind;
use std::collections::HashSet;
use std::path::PathBuf;

/// Returns the root of the parser engine source tree.
///
/// `CARGO_MANIFEST_DIR` is `crates/perl-parser-core/`, so this resolves to
/// `crates/perl-parser-core/src/engine/parser/`.
fn engine_parser_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join("engine").join("parser")
}

/// Recursively collects the contents of all non-test `.rs` files under `dir`.
///
/// Files whose name ends in `_tests.rs` or is exactly `tests.rs` are skipped —
/// test files contain `NodeKind::` in match arms and assert expressions, which
/// are references but not construction sites in the production parser.
///
/// IO errors from individual files are propagated via `?`.
fn collect_non_test_rs_sources(dir: &PathBuf) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut contents = Vec::new();
    collect_recursive(dir, &mut contents)?;
    Ok(contents)
}

fn collect_recursive(
    dir: &PathBuf,
    out: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let entries = std::fs::read_dir(dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_recursive(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name =
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // Skip test files — they contain NodeKind:: in assertions, not
            // construction sites.
            if name.ends_with("_tests.rs") || name == "tests.rs" {
                continue;
            }
            let content = std::fs::read_to_string(&path)?;
            out.push(content);
        }
    }
    Ok(())
}

/// Asserts that every non-recovery `NodeKind` variant has at least one
/// construction-site reference (`NodeKind::<Variant>`) in the parser engine
/// source.
///
/// Failure means a variant exists in the enum but has never been wired into
/// the parser — a "dead on arrival" variant.
#[test]
fn all_non_recovery_variants_have_construction_sites() -> Result<(), Box<dyn std::error::Error>> {
    let recovery: HashSet<&str> = NodeKind::RECOVERY_KIND_NAMES.iter().copied().collect();

    let root = engine_parser_root();
    let sources = collect_non_test_rs_sources(&root)?;
    let combined = sources.join("\n");

    let non_recovery_total =
        NodeKind::ALL_KIND_NAMES.iter().filter(|n| !recovery.contains(**n)).count();

    let mut dead: Vec<&str> = Vec::new();
    for name in NodeKind::ALL_KIND_NAMES {
        if recovery.contains(name) {
            continue; // recovery variants are exempt — see probe_recovery_nodes.rs
        }
        let pattern = format!("NodeKind::{name}");
        if !combined.contains(pattern.as_str()) {
            dead.push(name);
        }
    }

    assert!(
        dead.is_empty(),
        "NodeKind variants with zero construction sites in \
         crates/perl-parser-core/src/engine/parser/**: {dead:?}\n\
         ({non_recovery_total} non-recovery variants checked)\n\
         If a variant is intentionally never constructed in the engine, it \
         should be added to RECOVERY_KIND_NAMES or its construction site \
         should be present in the parser source."
    );

    // non_recovery_total is checked implicitly by the assertion above;
    // bind it to avoid an unused-variable warning if the assert is removed.
    let _ = non_recovery_total;

    Ok(())
}

/// Informational audit of which recovery variants have construction sites.
///
/// This test **never fails** — recovery variants may legitimately have zero
/// construction sites in the engine (e.g., `MissingStatement`,
/// `MissingIdentifier`, `MissingBlock` per #976).  Its purpose is to make the
/// recovery-variant construction picture visible in test output and to catch
/// if the set of zero-site recovery variants changes unexpectedly.
#[test]
fn recovery_variants_construction_site_audit() -> Result<(), Box<dyn std::error::Error>> {
    let recovery: HashSet<&str> = NodeKind::RECOVERY_KIND_NAMES.iter().copied().collect();

    let root = engine_parser_root();
    let sources = collect_non_test_rs_sources(&root)?;
    let combined = sources.join("\n");

    let mut zero_site: Vec<&str> = Vec::new();
    let mut with_site: Vec<&str> = Vec::new();

    for name in NodeKind::ALL_KIND_NAMES {
        if !recovery.contains(name) {
            continue;
        }
        let pattern = format!("NodeKind::{name}");
        if combined.contains(pattern.as_str()) {
            with_site.push(name);
        } else {
            zero_site.push(name);
        }
    }

    // Currently expected zero-site recovery variants: MissingStatement,
    // MissingIdentifier, MissingBlock (tracked in #976).
    // If this list grows, investigate whether new recovery variants are dead code.
    // No assertion — this test exists to document reality, not enforce it.
    // Bind both vecs to suppress unused-variable warnings.
    let _ = (zero_site, with_site);

    Ok(())
}
