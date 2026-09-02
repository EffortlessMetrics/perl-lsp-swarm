//! CLI integration tests for `cargo xtask kwalitee-inventory` (#8752).
//!
//! The discriminating proofs run against hermetic fixture trees via `--root`:
//! an unclassified new reference must fail the checker, a classified line that
//! moved must fail as stale, historical prose must stay distinct from an active
//! command caller in the report, and generated/ignored surfaces must not hide
//! callers. One additional test reconciles the real committed ledger against
//! the real working tree so the ledger cannot rot silently.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use assert_cmd::Command;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Recreate the ledger's line-identity contract: full SHA-256 over the
/// one-based line number and trimmed line bytes. Recomputed independently of
/// the task code so a hashing regression cannot prove itself.
fn hash_line(line_no: usize, line: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(line_no.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(line.trim().as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn hashes(lines: &[&str]) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(idx, line)| format!("{:?}", hash_line(idx + 1, line)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn entry(path: &str, classification: &str, target: &str, lines: &[&str]) -> String {
    format!(
        "[[entry]]\n\
         path = \"{path}\"\n\
         classification = \"{classification}\"\n\
         migration_target = \"{target}\"\n\
         owner_issue = 7192\n\
         removal_condition = \"test fixture row\"\n\
         allowed_to_remain = true\n\
         line_hashes = [{}]\n\n",
        hashes(lines)
    )
}

const EXCLUDED_TARGET: &str = "\
[[excluded_surface]]\n\
pattern = \"target/**\"\n\
owner_issue = 8752\n\
reason = \"test: ephemeral build output\"\n\n";

const HISTORICAL_LINE: &str = "The retired perl-kwalitee evaluator predates the rails.";
const CALLER_LINE: &str = "cargo xtask perl-kwalitee report --profile nightly";
const READER_LINE: &str = "const KIND: &str = \"perl_kwalitee\";";

/// A hermetic tree with one classified occurrence of each discriminating class:
/// historical prose, an active command caller, and legacy receipt readability.
fn classified_tree() -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    write(&dir, "docs/history.md", &format!("{HISTORICAL_LINE}\n"))?;
    write(&dir, "ci/nightly.sh", &format!("{CALLER_LINE}\n"))?;
    write(&dir, "src/legacy_reader.rs", &format!("{READER_LINE}\n"))?;
    let caller_row =
        entry("ci/nightly.sh", "release_readiness", "independent_readiness_rails", &[CALLER_LINE]);
    let reader_row = entry(
        "src/legacy_reader.rs",
        "legacy_compatibility",
        "legacy_receipt_readability",
        &[READER_LINE],
    );
    let history_row = entry("docs/history.md", "historical_prose", "none", &[HISTORICAL_LINE]);
    let ledger = format!(
        "{}{}{}{}{}",
        "schema_version = \"kwalitee_namespace_inventory.v1\"\ncontroller_issue = 8752\n\n",
        EXCLUDED_TARGET,
        history_row,
        caller_row,
        reader_row
    );
    write(&dir, "policy/kwalitee-namespace-inventory.toml", &ledger)?;
    Ok(dir)
}

fn write(dir: &TempDir, rel: &str, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = dir.path().join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    Ok(())
}

fn inventory(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("xtask").expect("xtask binary");
    cmd.args(["kwalitee-inventory", "--root"]).arg(root);
    cmd
}

#[test]
fn classified_tree_passes_and_reports_unresolved_active_by_target() {
    let dir = classified_tree().expect("fixture tree");
    let output = inventory(dir.path()).arg("--check").output().expect("run kwalitee-inventory");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "classified tree must pass: {stdout} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The active command caller is counted under its migration target...
    assert!(
        stdout.contains("independent_readiness_rails: 1 occurrence(s)"),
        "report must expose unresolved active counts by target:\n{stdout}"
    );
    assert!(
        stdout.contains("legacy_receipt_readability: 1 occurrence(s)"),
        "legacy receipt readability must be counted separately:\n{stdout}"
    );
    // ...while historical prose is resolved and never an unresolved row.
    assert!(
        !stdout.contains("unresolved active occurrences by migration target:\n  none:"),
        "historical prose must not appear as unresolved:\n{stdout}"
    );
    assert!(
        stdout.contains("historical_prose: 1 occurrence(s)"),
        "per-classification totals must include historical prose:\n{stdout}"
    );
}

#[test]
fn new_ambiguous_reference_fails_the_checker() {
    let dir = classified_tree().expect("fixture tree");
    write(&dir, "automation/new_caller.sh", "run perl-kwalitee check\n").expect("write caller");
    let output = inventory(dir.path()).arg("--check").output().expect("run kwalitee-inventory");
    assert!(
        !output.status.success(),
        "an unclassified perl-kwalitee reference must fail the checker"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unclassified reference"),
        "failure must name the unclassified reference: {stderr}"
    );
    assert!(
        stderr.contains("automation/new_caller.sh"),
        "failure must name the offending file: {stderr}"
    );
}

#[test]
fn source_movement_invalidates_the_stale_classification() {
    let dir = classified_tree().expect("fixture tree");
    // Move the classified line's text without adding a new row.
    write(&dir, "ci/nightly.sh", &format!("{CALLER_LINE} --strict\n")).expect("rewrite caller");
    let output = inventory(dir.path()).arg("--check").output().expect("run kwalitee-inventory");
    assert!(!output.status.success(), "a moved classified line must fail as stale");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("stale classification") && stderr.contains("ci/nightly.sh"),
        "failure must name the stale row and its file: {stderr}"
    );

    // Deleting a classified file is the same staleness from the other side.
    let dir = classified_tree().expect("fixture tree");
    fs::remove_file(dir.path().join("src/legacy_reader.rs")).expect("delete classified file");
    let output = inventory(dir.path()).arg("--check").output().expect("run kwalitee-inventory");
    assert!(!output.status.success(), "a vanished classified file must fail as stale");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no longer exists"),
        "failure must explain the vanished path: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Relocating unchanged source text also invalidates its reviewed identity.
    let dir = classified_tree().expect("fixture tree");
    write(&dir, "ci/nightly.sh", &format!("# moved down\n{CALLER_LINE}\n"))
        .expect("relocate caller");
    let output = inventory(dir.path()).arg("--check").output().expect("run kwalitee-inventory");
    assert!(!output.status.success(), "a relocated classified line must fail as stale");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("stale classification"),
        "failure must identify the relocated row: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn duplicate_identical_occurrence_is_not_covered_by_one_claim() {
    let dir = classified_tree().expect("fixture tree");
    write(&dir, "ci/nightly.sh", &format!("{CALLER_LINE}\n{CALLER_LINE}\n"))
        .expect("duplicate caller");
    let output = inventory(dir.path()).arg("--check").output().expect("run kwalitee-inventory");
    assert!(!output.status.success(), "an extra identical occurrence must be unclassified");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unclassified reference") && stderr.contains("ci/nightly.sh"),
        "failure must name the extra occurrence and its file: {stderr}"
    );
}

#[test]
fn one_occurrence_cannot_carry_two_classifications() {
    let dir = classified_tree().expect("fixture tree");
    let ledger_path = dir.path().join("policy/kwalitee-namespace-inventory.toml");
    let ledger = fs::read_to_string(&ledger_path).expect("read ledger");
    let duplicate =
        entry("ci/nightly.sh", "legacy_compatibility", "perl_release_readiness", &[CALLER_LINE]);
    fs::write(&ledger_path, format!("{ledger}{duplicate}")).expect("append duplicate row");
    let output = inventory(dir.path()).arg("--check").output().expect("run kwalitee-inventory");
    assert!(!output.status.success(), "one occurrence under two classifications must fail");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("duplicate classification"),
        "failure must name the duplicate: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn closed_vocabularies_reject_unknown_values_and_pairings() {
    for (classification, target) in [
        ("not_a_class", "none"),
        ("release_readiness", "legacy_receipt_readiness"),
        ("historical_prose", "perl_release_readiness"),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        write(&dir, "a.txt", &format!("{CALLER_LINE}\n")).expect("write");
        let ledger = format!(
            "{}{}",
            "schema_version = \"kwalitee_namespace_inventory.v1\"\ncontroller_issue = 8752\n\n",
            entry("a.txt", classification, target, &[CALLER_LINE])
        );
        write(&dir, "policy/kwalitee-namespace-inventory.toml", &ledger).expect("ledger");
        let output = inventory(dir.path()).arg("--check").output().expect("run kwalitee-inventory");
        assert!(
            !output.status.success(),
            "classification {classification:?} targeting {target:?} must be rejected"
        );
    }
}

#[test]
fn generated_and_ignored_surfaces_cannot_hide_callers() {
    // A reference inside a generated (non-excluded) surface is still a caller.
    let dir = classified_tree().expect("fixture tree");
    write(&dir, "generated/status.json", "\"cmd\": \"perl_kwalitee report\"\n").expect("write");
    let output = inventory(dir.path()).arg("--check").output().expect("run kwalitee-inventory");
    assert!(!output.status.success(), "a generated surface must not hide an unclassified caller");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("generated/status.json"),
        "failure must name the generated file: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The declared non-content surface stays excluded, on purpose and by row.
    let dir = classified_tree().expect("fixture tree");
    write(&dir, "target/receipts/out.md", "perl-kwalitee verdict\n").expect("write");
    let output = inventory(dir.path()).arg("--check").output().expect("run kwalitee-inventory");
    assert!(
        output.status.success(),
        "declared non-content surfaces are excluded: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // A tracked fixture directory named `target` below the repository root is
    // content and must not inherit the root build-output exclusion.
    write(
        &dir,
        "crates/example/tests/fixtures/nested/target/hidden.rs",
        "const COMMAND: &str = \"perl-kwalitee report\";\n",
    )
    .expect("write nested tracked target fixture");
    let output = inventory(dir.path()).arg("--check").output().expect("run kwalitee-inventory");
    assert!(!output.status.success(), "a nested target fixture must remain visible");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("nested/target/hidden.rs"),
        "failure must name the nested target fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn scaffold_prints_hashes_but_writes_nothing() {
    let dir = classified_tree().expect("fixture tree");
    write(&dir, "extra/new.md", "mentions perl-kwalitee once\n").expect("write unclassified");
    let output = inventory(dir.path()).arg("--scaffold").output().expect("run kwalitee-inventory");
    assert!(
        output.status.success(),
        "scaffold never fails: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("path = \"extra/new.md\""), "scaffold covers new files:\n{stdout}");
    assert!(
        stdout.contains(&hash_line(1, "mentions perl-kwalitee once")),
        "scaffold prints the exact current line hash:\n{stdout}"
    );
    let ledger = fs::read_to_string(dir.path().join("policy/kwalitee-namespace-inventory.toml"))
        .expect("read ledger");
    assert!(!ledger.contains("extra/new.md"), "scaffold must not mutate the ledger");
}

/// The committed ledger must reconcile the real working tree right now. This
/// is the standing protection: any new ambiguous `perl-kwalitee` or
/// `perl_kwalitee` reference anywhere in the repository fails this test until
/// it carries exactly one reviewed classification.
#[test]
fn committed_ledger_is_current_against_the_real_tree() {
    let output = Command::cargo_bin("xtask")
        .expect("xtask binary")
        .args(["kwalitee-inventory", "--check"])
        .output()
        .expect("run kwalitee-inventory");
    assert!(
        output.status.success(),
        "committed ledger is stale against the real tree:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
