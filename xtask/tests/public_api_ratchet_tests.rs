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
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn nightly_reports_require_successful_nonempty_baseline() -> Result<(), Box<dyn std::error::Error>>
{
    let workflow = fs::read_to_string(project_root().join(".github/workflows/ci-nightly.yml"))?
        .replace("\r\n", "\n");
    for step in ["Generate breaking changes report", "Upload breaking changes report"] {
        let marker = format!("      - name: {step}\n");
        let guard = workflow
            .split(&marker)
            .nth(1)
            .and_then(|rest| rest.lines().next())
            .and_then(|line| line.trim().strip_prefix("if: ${{ "))
            .and_then(|line| line.strip_suffix(" }}"))
            .ok_or("missing nightly report guard")?;
        // Evaluate the restricted conjunction used by these steps; unknown syntax
        // fails the proof instead of silently acquiring guessed semantics.
        for cancelled in [false, true] {
            for outcome in ["success", "failure", "skipped", "cancelled"] {
                for baseline in ["", "none", "v0.17.0"] {
                    let mut actual = true;
                    for term in guard.split(" && ") {
                        actual &= match term {
                            "!cancelled()" => !cancelled,
                            "steps.baseline.outcome == 'success'" => outcome == "success",
                            "steps.baseline.outputs.baseline != ''" => !baseline.is_empty(),
                            "steps.baseline.outputs.baseline != 'none'" => baseline != "none",
                            _ => {
                                return Err(format!("unsupported report guard term: {term}").into());
                            }
                        };
                    }
                    let expected = !cancelled
                        && outcome == "success"
                        && !baseline.is_empty()
                        && baseline != "none";
                    if actual != expected {
                        return Err(format!("{step}: cancelled={cancelled}, outcome={outcome}, baseline={baseline:?}: selected={actual}").into());
                    }
                }
            }
        }
    }
    Ok(())
}

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

/// True when the line is a guarded public item (#15634): a plain `pub ` item,
/// or an attribute-fronted item — any run of leading bracketed attributes
/// (`#[repr(u8)]`, `#[non_exhaustive] #[repr(i32)]`, ...) followed by `pub `.
///
/// The attribute run is stripped iteratively from the front rather than by
/// splitting on `"] "`: an item whose own signature contains `] ` (for
/// example a slice in a rendered return type) would otherwise split at the
/// wrong bracket and produce a false negative.
fn is_guarded_surface_line(line: &str) -> bool {
    let mut tail = line;
    while let Some(rest) = tail.strip_prefix("#[") {
        match rest.find(']') {
            Some(end) => tail = rest[end + 1..].trim_start(),
            None => return false,
        }
    }
    tail.starts_with("pub ")
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

        // Every line must be a guarded public item (#15634): a plain `pub `
        // item, or an attribute-fronted item (`#[repr(u8)] pub ...`) —
        // attributes on public items are part of the public contract and the
        // ratchet must record them, not silently drop them.
        let non_empty_lines: Vec<_> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        for (line_num, line) in non_empty_lines.iter().enumerate() {
            assert!(
                is_guarded_surface_line(line),
                "Baseline {} line {} is not a guarded public item (expected 'pub ' or \
                 attribute-fronted 'pub'): {}",
                crate_name,
                line_num + 1,
                line
            );
        }
    }

    Ok(())
}

/// Test B2 (unit): the guarded-surface shape checker admits exactly the lines
/// the widened `_public-api-filter` regex keeps (#15634).
#[test]
fn guarded_surface_line_shape() {
    // Plain public items.
    assert!(is_guarded_surface_line("pub struct alpha"));
    assert!(is_guarded_surface_line("pub fn perl_lsp_rs_core::f"));
    // Single and repeated attribute fronts (the cases the old `^pub ` filter
    // dropped).
    assert!(is_guarded_surface_line("#[repr(u8)] pub enum beta"));
    assert!(is_guarded_surface_line(
        "#[repr(i32)] #[non_exhaustive] pub enum perl_lsp_rs_core::protocol::ErrorCode"
    ));
    // An item signature containing `] ` must not break the attribute scan.
    assert!(is_guarded_surface_line("#[some_attr] pub fn foo() -> &[u8] where T: Debug"));
    // Non-item lines stay out.
    assert!(!is_guarded_surface_line("// comment line"));
    assert!(!is_guarded_surface_line(""));
    // Attributes without a following `pub ` item stay out: the filter must
    // not admit arbitrary attribute lines.
    assert!(!is_guarded_surface_line("#[repr(u8)] struct secret"));
    assert!(!is_guarded_surface_line("#[unterminated"));
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
/// 2. The guarded surface is derived through the shared `_public-api-filter`
///    helper (#15634), which keeps the `|| true` tolerance so that an empty
///    match (e.g., from a compile error silenced by `2>/dev/null`) does NOT
///    abort the script early via set -e, allowing the FAILED counter and the
///    final exit-1 to report the real problem instead. The helper must admit
///    attribute-fronted public items (`#[repr(u8)] pub ...`) — the old
///    `^pub `-only filter silently dropped them from the ratchet.
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

    // One filter, one owner (#15634): the check recipe must not restate the
    // surface filter; it delegates to the shared _public-api-filter helper.
    assert!(
        check_body.contains(
            "just _public-api-filter \"/tmp/${crate}-raw.txt\" \"/tmp/${crate}-current.txt\""
        ),
        "public-api-check must derive the guarded surface via the shared _public-api-filter helper (#15634)"
    );
    assert!(
        !check_body.contains("grep \"^pub \"") && !check_body.contains("grep '^pub '"),
        "public-api-check must not restate the surface filter; _public-api-filter owns it (#15634)"
    );

    // Extract the shared filter recipe body: the signature line opens it and
    // the next recipe's comment header closes it.
    let filter_body = justfile
        .split("_public-api-filter raw out:\n")
        .nth(1)
        .ok_or("Could not find _public-api-filter recipe in justfile")?
        .split("\n# Check public API surface")
        .next()
        .ok_or("Could not delimit _public-api-filter recipe body")?;

    // The filter invocation must have '|| true' to avoid aborting the loop when
    // cargo-public-api produces empty output (e.g., due to a compile error silenced
    // by `2>/dev/null`).  Without it, grep exits 1 on zero matches and set -e kills
    // the script before the FAILED counter is evaluated.
    assert!(
        filter_body.contains("|| true"),
        "_public-api-filter must tolerate grep's no-match exit so an empty surface \
         reaches the caller's INSTRUMENT-FAIL classification"
    );

    // The filter must admit attribute-fronted public items (#15634): a run of
    // bracketed attributes followed by `pub `, alongside plain `pub ` items.
    assert!(
        filter_body.contains("^(pub |(#\\[[^]]*\\][[:space:]]*)+pub )"),
        "_public-api-filter must keep both plain 'pub ' items and attribute-fronted \
         public items ('#[...] pub ...'), not drop the latter (#15634)"
    );

    assert!(
        check_body.contains("diff -u"),
        "public-api-check must use 'diff -u' to compare baseline vs current"
    );

    assert!(check_body.contains("FAILED=1"), "public-api-check must set FAILED=1 on diff mismatch");

    assert!(check_body.contains("exit 1"), "public-api-check must exit 1 when FAILED > 0");

    Ok(())
}

#[test]
fn public_api_filter_normalizes_only_io_reexport_paths() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let justfile = fs::read_to_string(root.join("justfile"))?;
    let filter = justfile
        .split("_public-api-filter raw out:")
        .nth(1)
        .ok_or("Could not find _public-api-filter recipe")?
        .split("\n# Check public API surface")
        .next()
        .ok_or("Could not delimit _public-api-filter recipe")?;

    assert!(filter.contains("core::io::(write::|error::)?"));
    // Both `alloc::io` submodules, not just `buf_read` (#16117). `Read` lands in
    // `alloc::io::read::`, and folding only its sibling left `&mut dyn
    // alloc::io::read::Read` unnormalized. That survived on both sides of the
    // diff and so caused no failure -- but the symmetry was luck, and it breaks
    // the moment one side is regenerated and the other is not.
    assert!(
        filter.contains("alloc::io::(buf_read::|read::)?"),
        "_public-api-filter must fold both `alloc::io` submodules to `std::io::` (#16117)"
    );
    assert!(filter.contains("std::io::"));
    assert!(filter.contains("grep -E"));
    Ok(())
}

/// Extract the `-e '<expr>'` substitutions from the shared filter recipe, in
/// order, so a behavioural test runs the recipe's own expressions rather than a
/// copy of them that can drift.
fn filter_sed_expressions() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let justfile = fs::read_to_string(project_root().join("justfile"))?;
    let recipe = justfile
        .split("_public-api-filter raw out:")
        .nth(1)
        .ok_or("Could not find _public-api-filter recipe")?
        .split("\n# Check public API surface")
        .next()
        .ok_or("Could not delimit _public-api-filter recipe")?;

    let mut exprs = Vec::new();
    for line in recipe.lines() {
        let Some(rest) = line.trim().strip_prefix("-e '") else {
            continue;
        };
        let Some(expr) = rest.rsplit_once('\'') else {
            continue;
        };
        exprs.push(expr.0.to_string());
    }
    // A parse that silently found fewer expressions than the recipe carries
    // would leave the lookalike cases below vacuously green: sed with no
    // substitution echoes its input, which is exactly what they assert. Pin
    // the count so a reformatted recipe fails loudly here instead.
    if exprs.len() != 2 {
        return Err(format!(
            "expected 2 -e substitutions in _public-api-filter, parsed {}: {exprs:?}",
            exprs.len()
        )
        .into());
    }
    Ok(exprs)
}

/// The io fold must only rewrite an external-crate path, never a user-owned one
/// that merely ends in `core` or `alloc`.
///
/// `#16117` put this filter on **both** sides of the diff, which is what makes
/// an over-broad match dangerous rather than merely untidy: before, a genuine
/// rename of `perl_lsp_rs_core::foo_alloc::io::Thing` to
/// `perl_lsp_rs_core::foo_std::io::Thing` still diffed, because only the
/// generated side was rewritten. With both sides folded, an unanchored
/// substitution normalizes the two spellings to identical text and the rename
/// walks past the ratchet unseen — the one thing this gate exists to stop.
///
/// The check runs the recipe's own `sed` expressions, so it measures behaviour
/// rather than spelling and cannot pass against a differently-written regex
/// with the same hole.
#[test]
fn the_io_fold_leaves_user_owned_lookalike_paths_alone() -> Result<(), Box<dyn std::error::Error>> {
    let exprs = filter_sed_expressions()?;

    // Each case is a public-API line the filter would see, paired with what it
    // must read after folding.
    let cases = [
        // Genuine external-crate re-exports: these are the ones to fold.
        ("pub fn b(x: &mut dyn alloc::io::read::Read)", "pub fn b(x: &mut dyn std::io::Read)"),
        ("pub fn d(x: core::io::error::Error)", "pub fn d(x: std::io::Error)"),
        // A user-owned module whose name ends in `alloc` or `core`.
        (
            "pub fn a(x: &perl_lsp_rs_core::foo_alloc::io::Thing)",
            "pub fn a(x: &perl_lsp_rs_core::foo_alloc::io::Thing)",
        ),
        ("pub fn c(x: bar_core::io::Sink)", "pub fn c(x: bar_core::io::Sink)"),
        // A user-owned module actually named `alloc`, reached through a path.
        ("pub fn e(x: crate::alloc::io::Thing)", "pub fn e(x: crate::alloc::io::Thing)"),
        // A Rust identifier may contain any XID_Continue character, so an
        // exclusion set written over ASCII (`[^A-Za-z0-9_:]`) presented a
        // Unicode letter as a separator and folded a user-owned path. The
        // anchor is a closed ASCII delimiter whitelist for this reason, which
        // also keeps the answer independent of the shell's locale (#16119).
        ("pub fn f(x: \u{3b1}alloc::io::Thing)", "pub fn f(x: \u{3b1}alloc::io::Thing)"),
        ("pub fn g(x: \u{3a9}core::io::error::Sink)", "pub fn g(x: \u{3a9}core::io::error::Sink)"),
        // Positions that must still fold, so the whitelist is not so narrow
        // that it stops doing its job: generic argument, tuple element after a
        // comma, behind a reference, and after `-> `.
        ("pub fn h(x: Vec<core::io::error::Error>)", "pub fn h(x: Vec<std::io::Error>)"),
        (
            "pub fn i(x: (core::io::error::Error, alloc::io::read::Read))",
            "pub fn i(x: (std::io::Error, std::io::Read))",
        ),
        ("pub fn j(x: &core::io::error::Error)", "pub fn j(x: &std::io::Error)"),
        ("pub fn k() -> core::io::error::Result<()>", "pub fn k() -> std::io::Result<()>"),
    ];

    for (input, expected) in cases {
        let mut command = Command::new("sed");
        command.arg("-E");
        for expr in &exprs {
            command.arg("-e").arg(expr);
        }
        let mut child =
            command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        child
            .stdin
            .as_mut()
            .ok_or("sed stdin unavailable")?
            .write_all(format!("{input}\n").as_bytes())?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(format!(
                "sed failed on {input:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let got = String::from_utf8(output.stdout)?;
        assert_eq!(
            got.trim_end(),
            expected,
            "the io fold rewrote a path it must not touch (#16117)"
        );
    }
    Ok(())
}

/// Run the recipe's own `sed` expressions over one line, as the filter would.
fn apply_io_fold(line: &str) -> Result<String, Box<dyn std::error::Error>> {
    let exprs = filter_sed_expressions()?;
    let mut command = Command::new("sed");
    command.arg("-E");
    for expr in &exprs {
        command.arg("-e").arg(expr);
    }
    let mut child =
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    child
        .stdin
        .as_mut()
        .ok_or("sed stdin unavailable")?
        .write_all(format!("{line}\n").as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(
            format!("sed failed on {line:?}: {}", String::from_utf8_lossy(&output.stderr)).into()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim_end().to_string())
}

/// Run the recipe's own `sed` expressions over a whole buffer in one process.
///
/// `apply_io_fold` spawns per line, which is fine for a handful of fixtures and
/// is not for thirty thousand baseline lines.
fn apply_io_fold_to_all(text: &str) -> Result<String, Box<dyn std::error::Error>> {
    let exprs = filter_sed_expressions()?;
    let mut command = Command::new("sed");
    command.arg("-E");
    for expr in &exprs {
        command.arg("-e").arg(expr);
    }
    let mut child =
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut stdin = child.stdin.take().ok_or("sed stdin unavailable")?;
    let buffer = text.to_string();
    let writer = std::thread::spawn(move || stdin.write_all(buffer.as_bytes()));
    let output = child.wait_with_output()?;
    writer.join().map_err(|_| "sed writer thread panicked")??;
    if !output.status.success() {
        return Err(format!("sed failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Folding both sides must not cost the gate its ability to see a real change.
///
/// The lookalike test above proves the filter leaves the wrong paths alone.
/// This proves the converse, and it is the one that carries the claim: two
/// surfaces that differ for a reason a reviewer would care about must still
/// differ after both sides are folded. A fold that normalized such a pair to
/// equality would be a silently weakened ratchet, which is precisely the risk
/// `#16117` takes on by folding the stored side as well as the generated one.
#[test]
fn the_io_fold_still_discriminates_a_real_rename() -> Result<(), Box<dyn std::error::Error>> {
    // (what the baseline holds, what the current surface renders, what changed)
    let pairs = [
        // The type identifier changed; only the module path is foldable.
        (
            "pub fn f() -> core::io::error::Result<()>",
            "pub fn f() -> core::io::error::Error",
            "Result -> Error",
        ),
        // A user-owned module renamed across the fold's own vocabulary. This is
        // the case the lookalike anchor exists to keep visible.
        (
            "pub fn g(x: &perl_lsp_rs_core::foo_alloc::io::Thing)",
            "pub fn g(x: &perl_lsp_rs_core::foo_std::io::Thing)",
            "foo_alloc -> foo_std",
        ),
        // The same rename one module up, behind a Unicode identifier prefix.
        // This is the pair the previous `[^A-Za-z0-9_:]` anchor collapsed:
        // it read `\u{3b1}` as a separator, folded the baseline side to
        // `\u{3b1}std::io::Thing`, and met a current side already spelled
        // that way -- equal text, and a real rename of a user-owned module
        // gone from the diff. Under an ASCII delimiter whitelist neither side
        // folds and the two spellings stay distinguishable.
        (
            "pub fn h(x: \u{3b1}alloc::io::Thing)",
            "pub fn h(x: \u{3b1}std::io::Thing)",
            "\u{3b1}alloc -> \u{3b1}std",
        ),
        // Arity change around an otherwise foldable path.
        (
            "pub fn i(x: core::io::error::Error)",
            "pub fn i(x: core::io::error::Error, y: u8)",
            "parameter added",
        ),
    ];

    for (baseline, current, what_changed) in pairs {
        let folded_baseline = apply_io_fold(baseline)?;
        let folded_current = apply_io_fold(current)?;
        assert_ne!(
            folded_baseline, folded_current,
            "folding both sides hid a real API change ({what_changed}): {baseline:?} and \
             {current:?} both normalized to {folded_baseline:?} (#16119)"
        );
    }
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

/// #16117: the io-path normalization (#16043/#16058) must run on BOTH sides of
/// the comparison.
///
/// The filter was applied to the freshly generated surface only, while the
/// committed baseline was diffed raw. A baseline captured under a nightly that
/// rendered `core::io::*` / `alloc::io::*` therefore disagreed with every
/// folded line -- 102 of them across three baselines -- on pull requests that
/// changed no Rust at all. Asserting the recipe diffs a normalized baseline is
/// what stops that from silently coming back: a one-sided filter still passes
/// every other test here, because both recipes and both file sets look fine in
/// isolation.
#[test]
fn public_api_check_normalizes_both_sides_of_the_diff() -> Result<(), Box<dyn std::error::Error>> {
    let justfile = fs::read_to_string(project_root().join("justfile"))?.replace("\r\n", "\n");
    let recipe = justfile
        .split("\npublic-api-check:")
        .nth(1)
        .and_then(|rest| rest.split("\npublic-api-update:").next())
        .ok_or("Could not find the public-api-check recipe body")?;

    assert!(
        recipe.contains(r#"just _public-api-filter "$BASELINE""#),
        "public-api-check must pass the committed baseline through _public-api-filter; \
         diffing a raw baseline against a normalized surface reports a toolchain \
         rendering change as an API change (#16117)"
    );

    let diffs_normalized_baseline =
        recipe.lines().any(|line| line.contains("diff -u") && line.contains("-baseline.txt"));
    assert!(
        diffs_normalized_baseline,
        "public-api-check must diff the normalized baseline, not $BASELINE directly (#16117)"
    );

    // Normalizing the stored side routes it through the filter's `grep` stage
    // too, so any committed line that is not a guarded public item -- a
    // conflict marker, a stray comment, a truncated body -- is dropped from
    // the comparison rather than showing up as a `-`. Compared raw, that
    // corruption reddened the gate. This guard is what keeps #16117 from
    // trading a phantom-red class for a silent blind spot (#16119).
    let guards_canonical_form =
        recipe.lines().any(|line| line.contains("cmp -s") && line.contains(r#""$BASELINE""#));
    assert!(
        guards_canonical_form,
        "public-api-check must reject a committed baseline the filter would alter; \
         without it a corrupted baseline is silently filtered out of the diff and the \
         gate passes where a raw comparison would have failed (#16119)"
    );

    Ok(())
}

/// The canonical-form guard must be free on the baselines actually committed.
///
/// A guard that fires on the current tree would be a broken gate rather than a
/// safeguard, so this pins the state the guard depends on: every committed
/// baseline is already exactly what `_public-api-filter` produces from it,
/// which is what `public-api-update` writes. It is the state assertion behind
/// the recipe assertion above, and it fails loudly if a regeneration or a
/// merge resolution ever writes a baseline that is not in canonical form
/// (#16119).
#[test]
fn committed_baselines_are_already_in_canonical_filtered_form()
-> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();

    for crate_name in ratchet_crates()? {
        let baseline_path = root.join(".ci/public-api-baselines").join(format!("{crate_name}.txt"));
        let committed = fs::read_to_string(&baseline_path)?;

        // The filter is `grep <guarded items> | sed <io fold>`. Apply the grep
        // half here and hand the whole remainder to one sed, rather than one
        // sed per line: 32k lines across twelve baselines is a minute of
        // process spawning otherwise, and this suite runs on every shard.
        let kept: String = committed
            .lines()
            .filter(|line| is_guarded_surface_line(line))
            .map(|line| format!("{line}\n"))
            .collect();
        let filtered = apply_io_fold_to_all(&kept)?;

        assert_eq!(
            filtered, committed,
            "{crate_name}: committed baseline is not in canonical filtered form, so the \
             canonical-form guard in public-api-check would fire on a clean tree. Run \
             'just public-api-update' (#16119)"
        );
    }
    Ok(())
}

/// #16117: the committed baselines must already be in the canonical form the
/// filter produces.
///
/// This is the state assertion behind the recipe assertion above. It holds
/// whether the baselines were regenerated or the stored side is normalized on
/// read, and it fails if a future regeneration on a drifted nightly writes
/// `core::io::*` / `alloc::io::*` back into the stored surface.
#[test]
fn committed_baselines_carry_no_unfolded_io_paths() -> Result<(), Box<dyn std::error::Error>> {
    let root = project_root();
    let mut offenders = Vec::new();
    for crate_name in ratchet_crates()? {
        let path = root.join(".ci/public-api-baselines").join(format!("{crate_name}.txt"));
        let Ok(content) = fs::read_to_string(&path) else {
            // baselines_exist_for_every_listed_crate owns that failure.
            continue;
        };
        for (index, line) in content.lines().enumerate() {
            if line.contains("core::io::") || line.contains("alloc::io::") {
                offenders.push(format!("{crate_name}.txt:{}: {line}", index + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "committed baselines carry {} unfolded io path(s); the ratchet folds them to \
         `std::io::*` on the generated side, so a stored `core::io::*` / `alloc::io::*` \
         reds every api_scope pull request on a diff nobody wrote (#16117). First few:\n{}",
        offenders.len(),
        offenders.iter().take(5).cloned().collect::<Vec<_>>().join("\n")
    );

    Ok(())
}
