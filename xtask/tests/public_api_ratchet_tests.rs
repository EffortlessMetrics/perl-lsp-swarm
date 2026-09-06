//! Integration tests for issue #4497: Facade-Only Public API Ratchet
//! (crate set derived from one enforced list since #14607)
//!
//! These tests verify the public API surface ratchet infrastructure:
//! - `.ci/public-api-baselines/ratchet-crates.txt` is the single crate list
//! - a baseline file exists for every listed crate, and for nothing else
//! - baselines are non-empty
//! - just public-api-check and just public-api-update recipes exist and read the list
//! - CI workflow includes public-api-check job
//! - the nightly semver-check job loops over the list
//! - CONTRIBUTING.md documents the public API workflow
//!
//! Tests assert config state, not runtime behavior. The original five facades
//! (`perl-lsp-rs`, `perl-parser`, `perl-uri`, `perl-dap`, `perllsp`) are still
//! pinned as required members so the list cannot silently shrink below them.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn project_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop();
    dir
}

/// The five facades #4497 introduced; every one must stay listed.
const ORIGINAL_FACADES: [&str; 5] =
    ["perl-lsp-rs", "perl-parser", "perl-uri", "perl-dap", "perllsp"];

/// Read `ratchet-crates.txt` with the rule every reader shares (#14607):
/// everything after `#` is a comment, whitespace is trimmed, blank lines skipped.
fn ratchet_crates() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let list_path = project_root().join(".ci/public-api-baselines/ratchet-crates.txt");
    let content = fs::read_to_string(&list_path)
        .map_err(|e| format!("Failed to read {}: {}", list_path.display(), e))?;
    let crates: Vec<String> = content
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    assert!(!crates.is_empty(), "{} lists no crates", list_path.display());
    Ok(crates)
}

/// Test A: a baseline file exists for every listed crate, and the original
/// five facades are still listed.
#[test]
fn baselines_exist_for_every_listed_crate() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let baselines_dir = root.join(".ci/public-api-baselines");

    let crates = ratchet_crates()?;
    for facade in &ORIGINAL_FACADES {
        assert!(
            crates.iter().any(|c| c == facade),
            "ratchet-crates.txt must still list the original facade crate: {facade}"
        );
    }

    for crate_name in &crates {
        let baseline_path = baselines_dir.join(format!("{}.txt", crate_name));
        assert!(
            baseline_path.exists(),
            "Baseline file missing: {} (expected at {})",
            crate_name,
            baseline_path.display()
        );
    }

    Ok(())
}

/// Test B: Each listed crate's baseline file is non-empty
#[test]
fn baseline_files_are_non_empty() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let baselines_dir = root.join(".ci/public-api-baselines");

    let crates = ratchet_crates()?;

    for crate_name in &crates {
        let baseline_path = baselines_dir.join(format!("{}.txt", crate_name));
        let content = fs::read_to_string(&baseline_path)
            .map_err(|e| format!("Failed to read baseline {}: {}", crate_name, e))?;

        assert!(
            !content.trim().is_empty(),
            "Baseline file is empty: {} (expected at least 1 line)",
            crate_name
        );

        // Verify that lines start with "pub " (public API items)
        let non_empty_lines: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        for (line_num, line) in non_empty_lines.iter().enumerate() {
            assert!(
                line.starts_with("pub "),
                "Baseline {} line {} does not start with 'pub ': {}",
                crate_name,
                line_num + 1,
                line
            );
        }
    }

    Ok(())
}

/// Test C: Justfile has public-api-check and public-api-update recipes
#[test]
fn justfile_has_public_api_recipes() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let justfile = fs::read_to_string(root.join("justfile"))?;

    assert!(
        justfile.contains("public-api-check:"),
        "justfile must contain 'public-api-check:' recipe (did not find it)"
    );

    assert!(
        justfile.contains("public-api-update:"),
        "justfile must contain 'public-api-update:' recipe (did not find it)"
    );

    assert!(
        justfile.contains("_public-api-install:"),
        "justfile must contain '_public-api-install:' helper recipe (did not find it)"
    );

    // Verify recipes appear in just --list output by checking justfile syntax
    // (just --list output via Command requires runtime, so we verify source instead)
    assert!(
        justfile.contains("just _public-api-install"),
        "public-api recipes must call _public-api-install helper"
    );

    Ok(())
}

/// Test D: CI workflow includes public-api-check job
#[test]
fn ci_nightly_workflow_has_public_api_check_job() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow_path = root.join(".github/workflows/ci-nightly.yml");
    let workflow = fs::read_to_string(&workflow_path)
        .map_err(|e| format!("Failed to read CI workflow: {}", e))?;

    // Verify job name exists
    assert!(
        workflow.contains("public-api-check:"),
        "ci-nightly.yml must contain 'public-api-check:' job"
    );

    // Verify the job runs 'just public-api-check'
    assert!(
        workflow.contains("just public-api-check"),
        "ci-nightly.yml public-api-check job must run 'just public-api-check' step"
    );

    // Verify all 5 crate names are referenced in the workflow context
    let facade_crates = ["perl-lsp-rs", "perl-parser", "perl-uri", "perl-dap", "perllsp"];
    for crate_name in &facade_crates {
        assert!(
            workflow.contains(crate_name),
            "ci-nightly.yml workflow must reference facade crate: {}",
            crate_name
        );
    }

    // Verify --simplified flag is present in the justfile recipe (critical for baseline stability).
    // The CI job delegates to `just public-api-check`, so the flag lives in the justfile, not the
    // workflow YAML itself. Check there instead to avoid asserting on a comment.
    let justfile = fs::read_to_string(root.join("justfile"))?;
    assert!(
        justfile.contains("--simplified"),
        "justfile public-api recipes must use '--simplified' flag for cargo public-api"
    );

    // Verify NO continue-on-error on public-api-check (hard-fail only)
    let public_api_section = workflow
        .split("public-api-check:")
        .nth(1)
        .ok_or("Could not find public-api-check job section")?;

    // Extract the job block (ends at next top-level key starting with 2 spaces)
    let job_block = public_api_section
        .split('\n')
        .take_while(|line| line.is_empty() || !line.starts_with("  ") || line.starts_with("    "))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        !job_block.contains("continue-on-error: true")
            && !job_block.contains("continue-on-error: false"),
        "public-api-check job must have hard-fail semantics (no continue-on-error)"
    );

    Ok(())
}

/// Test E: the nightly semver-check job loops over the ratchet list (#14607)
///
/// Before #14607 the job named five crates in five steps and disagreed with
/// `public-api-check`. It must now read the same list and must not restate any
/// crate name, so the two ratchets cannot drift apart again.
#[test]
fn semver_check_loops_over_the_ratchet_list() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let workflow = fs::read_to_string(root.join(".github/workflows/ci-nightly.yml"))?;

    let job = workflow
        .split("\n  semver-check:\n")
        .nth(1)
        .ok_or("Could not find semver-check job in ci-nightly.yml")?
        .split("\n  public-api-check:\n")
        .next()
        .ok_or("Could not delimit semver-check job")?;

    assert!(
        job.contains(".ci/public-api-baselines/ratchet-crates.txt"),
        "semver-check job must read .ci/public-api-baselines/ratchet-crates.txt"
    );
    assert!(
        job.contains("cargo semver-checks check-release -p \"${crate}\""),
        "semver-check job must run cargo semver-checks once per listed crate"
    );
    for crate_name in ratchet_crates()? {
        let restated = format!("check-release -p {crate_name}");
        assert!(
            !job.contains(&restated),
            "semver-check job must not restate a crate name ({restated}); the list is the authority"
        );
    }

    Ok(())
}

/// Test F: CONTRIBUTING.md documents public API workflow
#[test]
fn contributing_md_documents_public_api_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let contributing = fs::read_to_string(root.join("CONTRIBUTING.md"))?;

    // The subsection is asserted by role rather than by exact wording: #4504 introduced it
    // as "Public API Surface Ratchet" and #6868 renamed it to "Public API and SemVer" while
    // keeping the workflow intact. A dedicated `### Public API…` subsection is the contract;
    // its exact title is editorial.
    assert!(
        contributing.lines().any(|line| line.trim_end().starts_with("### Public API")),
        "CONTRIBUTING.md must have a dedicated '### Public API…' subsection documenting the \
         public-API surface workflow"
    );

    assert!(
        contributing.contains("just public-api-update"),
        "CONTRIBUTING.md must mention 'just public-api-update' command"
    );

    assert!(
        contributing.contains(".ci/public-api-baselines"),
        "CONTRIBUTING.md must reference '.ci/public-api-baselines/' directory"
    );

    Ok(())
}

/// Test G (regression guard): public-api-check script body has correct hard-fail behaviour
///
/// Specifically verifies:
/// 1. `set -euo pipefail` is present so the script aborts on errors.
/// 2. The grep pipeline uses `|| true` so that an empty match (e.g., from a compile error
///    silenced by `2>/dev/null`) does NOT abort the script early via set -e, allowing the
///    FAILED counter and the final exit-1 to report the real problem instead.
/// 3. The `diff -u` comparison runs and FAILED is set on non-zero diff exit.
#[test]
fn public_api_check_script_has_correct_fail_semantics() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let justfile = fs::read_to_string(root.join("justfile"))?;

    // Extract the public-api-check recipe body (up to the next recipe header)
    let check_body = justfile
        .split("public-api-check:")
        .nth(1)
        .ok_or("Could not find public-api-check recipe in justfile")?
        .split("\npublic-api-update:")
        .next()
        .ok_or("Could not delimit public-api-check recipe body")?;

    assert!(
        check_body.contains("set -euo pipefail"),
        "public-api-check must use 'set -euo pipefail'"
    );

    // The grep invocation must have '|| true' to avoid aborting the loop when
    // cargo-public-api produces empty output (e.g., due to a compile error silenced
    // by `2>/dev/null`).  Without it, grep exits 1 on zero matches and set -e kills
    // the script before the FAILED counter is evaluated.
    assert!(
        check_body.contains("grep \"^pub \"") || check_body.contains("grep '^pub '"),
        "public-api-check must grep for '^pub ' items"
    );
    assert!(
        check_body.contains("grep \"^pub \" > \"/tmp/${crate}-current.txt\" || true")
            || check_body.contains("grep \"^pub \" > \"/tmp/${crate}-current.txt\"  || true")
            || (check_body.contains("grep \"^pub \"") && check_body.contains("|| true")),
        "public-api-check grep pipeline must end with '|| true' to prevent set -e abort on empty output"
    );

    assert!(
        check_body.contains("diff -u"),
        "public-api-check must use 'diff -u' to compare baseline vs current"
    );

    assert!(check_body.contains("FAILED=1"), "public-api-check must set FAILED=1 on diff mismatch");

    assert!(check_body.contains("exit 1"), "public-api-check must exit 1 when FAILED > 0");

    Ok(())
}

/// Test H (edge case): the ratchet list is the single authority (#14607)
///
/// This test verifies that the crate set is derived, not restated:
/// - the justfile `public-api-check` recipe reads the list through
///   `_api-ratchet-crates` and names no crate itself;
/// - the baseline directory holds exactly one `<crate>.txt` per listed crate.
///
/// If the list and the baselines drift (a listed crate without a baseline, or a
/// baseline for an unlisted crate), `cargo xtask publish-manifest-check` fails on
/// every PR; this test keeps that contract visible at the config level too.
#[test]
fn ratchet_list_is_the_single_authority() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let listed: BTreeSet<String> = ratchet_crates()?.into_iter().collect();

    // The justfile recipe reads the list and restates no crate name.
    let justfile = fs::read_to_string(root.join("justfile"))?;
    let public_api_section = justfile
        .split("\npublic-api-check:")
        .nth(1)
        .ok_or("Could not find public-api-check recipe in justfile")?
        .split("\npublic-api-update:") // End at next recipe
        .next()
        .ok_or("Could not parse public-api-check recipe")?;

    assert!(
        public_api_section.contains("just _api-ratchet-crates"),
        "Justfile public-api-check recipe must read the crate set via `just _api-ratchet-crates`"
    );
    for crate_name in &listed {
        assert!(
            !public_api_section.contains(crate_name.as_str()),
            "Justfile public-api-check recipe must not restate crate name {crate_name}; \
             ratchet-crates.txt is the authority"
        );
    }

    // The baseline directory is exactly the listed set.
    let baselines_dir = root.join(".ci/public-api-baselines");
    let baselined: BTreeSet<String> = fs::read_dir(&baselines_dir)?
        .filter_map(|entry| entry.ok().and_then(|e| e.file_name().into_string().ok()))
        .filter_map(|name| name.strip_suffix(".txt").map(str::to_string))
        .filter(|stem| stem != "ratchet-crates")
        .collect();

    assert_eq!(
        baselined,
        listed,
        "baseline files in {} must be exactly the crates listed in ratchet-crates.txt",
        baselines_dir.display()
    );

    Ok(())
}

/// Test I (edge case): unlisted crates do not have baselines
///
/// The ratchet applies only to listed crates. Internal support crates such as
/// `perl-tdd-support`, `perl-corpus`, or `xtask` must be neither listed nor
/// baselined; a baseline appearing for one of them means the scope drifted
/// without the list (and its `publish-manifest-check` admission rule) changing.
#[test]
fn unlisted_crates_have_no_baselines() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let baselines_dir = root.join(".ci/public-api-baselines");
    let listed = ratchet_crates()?;

    // Internal crates that must stay outside the ratchet.
    let internal_crates = ["perl-tdd-support", "perl-corpus", "perl-lexer-core", "xtask"];

    for crate_name in &internal_crates {
        assert!(
            !listed.iter().any(|c| c == crate_name),
            "Internal crate {crate_name} must not be listed in ratchet-crates.txt"
        );
        let baseline_path = baselines_dir.join(format!("{}.txt", crate_name));
        assert!(
            !baseline_path.exists(),
            "Internal crate {} should NOT have a baseline file (only listed crates should)",
            crate_name
        );
    }

    Ok(())
}

/// Test J (regression): perllsp baseline keeps the thin-facade shape
///
/// `perllsp` is the published Cargo facade that re-exports the `perl-lsp-rs`
/// implementation. Since #7924 it also carries reviewed facade-owned modules
/// (`claude_compat` compatibility contracts), and #12030 regenerated the
/// baseline additively to that accepted surface.
///
/// This test pins:
/// - line 1: the crate module declaration;
/// - line 2: the wholesale `perl_lsp` re-export;
/// - the accepted facade-owned module set (`ACCEPTED_FACADE_MODULES`): every
///   `pub mod perllsp::<name>` declaration must be one of them, and every
///   remaining item's path must start inside one of them.
///
/// A lost re-export, a renamed module declaration, an undeclared new module,
/// or a root-level public item (an accidental lib-target addition) fails here
/// before CI's cargo-public-api diff runs.
#[test]
fn perllsp_baseline_has_expected_reexport_format() -> Result<(), Box<dyn std::error::Error>> {
    /// Facade-owned modules accepted on top of the wholesale re-export. A new
    /// intentional module extends this list in the same reviewed change that
    /// lands it and refreshes the baseline (#12030 recipe).
    const ACCEPTED_FACADE_MODULES: [&str; 1] = ["claude_compat"];

    let root = project_root();
    let perllsp_baseline = root.join(".ci/public-api-baselines/perllsp.txt");

    let content = fs::read_to_string(&perllsp_baseline)
        .map_err(|e| format!("Failed to read perllsp baseline: {}", e))?;

    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    assert!(
        lines.len() >= 2,
        "perllsp baseline should have at least 2 lines (mod + re-export), got {}. Content:\n{}",
        lines.len(),
        content
    );

    // First line is the crate module declaration
    assert!(
        lines[0] == "pub mod perllsp",
        "perllsp baseline first line should be 'pub mod perllsp', got: {}",
        lines[0]
    );

    // Second line is the wholesale implementation re-export
    assert!(
        lines[1] == "pub use perllsp::<<perl_lsp::*>>",
        "perllsp baseline second line should be 'pub use perllsp::<<perl_lsp::*>>', got: {}",
        lines[1]
    );

    // Every declared facade-owned module must be accepted, and every remaining
    // item's path must start inside one of them. The item path begins at the
    // first `perllsp::` occurrence; signature types referenced later in the
    // line must never satisfy the check. Root-level items beyond the re-export
    // would mean the lib target grew its own public API again.
    for line in &lines[1..] {
        let Some(declared) = line.strip_prefix("pub mod perllsp::") else {
            continue;
        };
        assert!(
            ACCEPTED_FACADE_MODULES.contains(&declared),
            "perllsp baseline declares module 'perllsp::{declared}' outside the accepted facade-owned set {ACCEPTED_FACADE_MODULES:?} (extend the set in the same reviewed change): {line}"
        );
    }

    for line in &lines[1..] {
        if *line == "pub use perllsp::<<perl_lsp::*>>" || line.starts_with("pub mod perllsp::") {
            continue;
        }
        assert!(
            !line.starts_with("pub use "),
            "perllsp baseline carries a non-wholesale root-level re-export (only 'pub use perllsp::<<perl_lsp::*>>' is accepted): {line}"
        );
        let path_start = line.find("perllsp::");
        assert!(path_start.is_some(), "perllsp baseline item has no perllsp:: path: {line}");
        // The assert above guarantees `Some`; `unwrap_or` keeps the repo's
        // `clippy::panic`/`expect_used` denials out of this test.
        let after_crate = &line[path_start.unwrap_or(0) + "perllsp::".len()..];
        assert!(
            ACCEPTED_FACADE_MODULES.iter().any(|module| after_crate.starts_with(module)
                && after_crate[module.len()..].starts_with("::")),
            "perllsp baseline item path is not under an accepted facade-owned module {ACCEPTED_FACADE_MODULES:?} (accidental lib-target API?): {line}"
        );
    }

    Ok(())
}

/// Test K (regression guard): Tool version is pinned consistently
///
/// This test verifies that `cargo-public-api` version is specified identically in:
/// - justfile `_public-api-install` recipe
/// - CI workflow `.github/workflows/ci-nightly.yml` install step
///
/// Version drift between local and CI could cause baselines to diverge if the tool
/// changes its output format between versions. This test catches silent mismatches.
#[test]
fn tool_version_pinned_consistently() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();

    // Extract version from justfile (looks like: --version 0.50.1;)
    let justfile = fs::read_to_string(root.join("justfile"))?;
    let justfile_version_line = justfile
        .lines()
        .find(|line| line.contains("cargo-public-api") && line.contains("--version"))
        .ok_or("Could not find cargo-public-api version in justfile")?;

    // Parse version: extract digits.digits.digits pattern
    let justfile_version_num = justfile_version_line
        .split("--version")
        .nth(1)
        .and_then(|s| {
            // Extract version like "0.50.1" from strings like " 0.50.1; \" or " 0.50.1"
            s.trim()
                .split(|c: char| !c.is_numeric() && c != '.')
                .find(|s| !s.is_empty() && s.chars().next().is_some_and(|c| c.is_numeric()))
        })
        .ok_or("Could not parse version from justfile")?;

    // Extract version from CI workflow (looks like: --version 0.50.1)
    let workflow = fs::read_to_string(root.join(".github/workflows/ci-nightly.yml"))?;
    let ci_version_line = workflow
        .lines()
        .find(|line| line.contains("cargo-public-api") && line.contains("--version"))
        .ok_or("Could not find cargo-public-api version in CI workflow")?;

    let ci_version_num = ci_version_line
        .split("--version")
        .nth(1)
        .and_then(|s| {
            s.trim()
                .split(|c: char| !c.is_numeric() && c != '.')
                .find(|s| !s.is_empty() && s.chars().next().is_some_and(|c| c.is_numeric()))
        })
        .ok_or("Could not parse version from CI workflow")?;

    assert_eq!(
        justfile_version_num, ci_version_num,
        "cargo-public-api version mismatch: justfile={}, CI={}. Both must be identical.",
        justfile_version_num, ci_version_num
    );

    Ok(())
}
