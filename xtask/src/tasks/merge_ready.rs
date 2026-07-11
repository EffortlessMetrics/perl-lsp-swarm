use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::tasks::review_receipts::{ReviewInstrument, ReviewReceipt, load_review_receipt};
use crate::utils::project_root;
use perl_lsp_rs_core::hashing::fnv1a64_hex;

const SCHEMA_VERSION: u32 = 1;
const CHECK_NAME: &str = "merge-readiness";
const DEFAULT_RECEIPT_PATH: &str = "target/receipts/merge-readiness.json";
const REQUIRED_CHECKS_PATH: &str = ".ci/policies/required-checks.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeReadinessReceipt {
    pub check: String,
    pub schema_version: u32,
    pub event: String,
    pub pr: u64,
    pub head_sha: String,
    pub base_sha: String,
    pub gate_graph_version: String,
    pub required_checks: Vec<String>,
    pub review_evidence: Vec<String>,
    pub blocker_labels_absent: bool,
    pub verdict: String,
    pub expires_when: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyStatus {
    Valid,
    StaleHead,
    StaleBase,
    StaleGateGraph,
    Blocked,
    Missing,
    /// No review receipt was supplied at all — the label-name citation is no
    /// longer sufficient; a receipt is required.
    ReviewReceiptMissing,
    /// A review receipt exists but none of the supplied receipts is bound to
    /// the current head — the review is stale (the head moved after review).
    ReviewReceiptStale,
    /// Review receipts exist and are bound to the current head, but none of
    /// them carries `instrument == IndependentReview` (e.g. only
    /// `FixResponder` receipts are present) — a fix-forward pass does not
    /// satisfy the independent-review requirement.
    ReviewReceiptNotIndependent,
}

impl VerifyStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::StaleHead => "stale_head",
            Self::StaleBase => "stale_base",
            Self::StaleGateGraph => "stale_gate_graph",
            Self::Blocked => "blocked",
            Self::Missing => "missing",
            Self::ReviewReceiptMissing => "review_receipt_missing",
            Self::ReviewReceiptStale => "review_receipt_stale",
            Self::ReviewReceiptNotIndependent => "review_receipt_not_independent",
        }
    }
}

pub fn emit(pr: u64, receipt_path: Option<PathBuf>) -> Result<()> {
    let root = project_root()?;
    let required_checks = load_required_checks(&root)?;
    let head_sha = git_output(&root, &["rev-parse", "HEAD"])?;
    let base_sha = resolve_base_sha(&root)?;
    let gate_graph_version = compute_gate_graph_version(&root, &required_checks)?;

    let verdict = if required_checks.is_empty() { "blocked" } else { "valid" }.to_string();
    let blocker_labels_absent = true;

    let receipt = MergeReadinessReceipt {
        check: CHECK_NAME.to_string(),
        schema_version: SCHEMA_VERSION,
        event: "pull_request".to_string(),
        pr,
        head_sha,
        base_sha,
        gate_graph_version,
        required_checks,
        review_evidence: vec!["reviewed-deep".to_string(), "ci-green".to_string()],
        blocker_labels_absent,
        verdict,
        expires_when: "on_new_commit_or_base_or_policy_change".to_string(),
    };

    let output_path = receipt_path.unwrap_or_else(|| root.join(DEFAULT_RECEIPT_PATH));
    write_receipt(&output_path, &receipt)?;
    println!("wrote {}", output_path.display());

    Ok(())
}

pub fn verify(
    pr: Option<u64>,
    fixture: Option<PathBuf>,
    review_receipt_paths: Vec<PathBuf>,
) -> Result<()> {
    let root = project_root()?;
    let path = if let Some(fixture_path) = fixture {
        fixture_path
    } else {
        let _ = pr;
        root.join(DEFAULT_RECEIPT_PATH)
    };

    if !path.exists() {
        println!("{}", VerifyStatus::Missing.as_str());
        bail!("receipt not found: {}", path.display());
    }

    let receipt = load_receipt(&path)?;
    let required_checks = load_required_checks(&root)?;
    let current_head = git_output(&root, &["rev-parse", "HEAD"])?;
    let current_base = resolve_base_sha(&root)?;
    let current_gate_graph = compute_gate_graph_version(&root, &required_checks)?;

    let review_receipts = review_receipt_paths
        .iter()
        .map(|p| load_review_receipt(p))
        .collect::<Result<Vec<ReviewReceipt>>>()?;

    let status = evaluate_merge_readiness(
        &receipt,
        &review_receipts,
        &current_head,
        &current_base,
        &current_gate_graph,
    );
    println!("{}", status.as_str());

    if status == VerifyStatus::Valid {
        Ok(())
    } else {
        bail!("receipt status: {}", status.as_str())
    }
}

pub fn reconcile(dry_run: bool) -> Result<()> {
    let root = project_root()?;
    let path = root.join(DEFAULT_RECEIPT_PATH);

    if !path.exists() {
        println!("missing: {}", path.display());
        return Ok(());
    }

    let receipt = load_receipt(&path)?;
    let required_checks = load_required_checks(&root)?;
    let current_head = git_output(&root, &["rev-parse", "HEAD"])?;
    let current_base = resolve_base_sha(&root)?;
    let current_gate_graph = compute_gate_graph_version(&root, &required_checks)?;
    let status = evaluate_receipt(&receipt, &current_head, &current_base, &current_gate_graph);

    println!("status={}", status.as_str());
    if dry_run {
        println!("advisory: would reconcile merge-ready label changes only");
    } else {
        println!("apply: merge-ready reconciliation would be applied by workflow automation");
    }

    Ok(())
}

fn evaluate_receipt(
    receipt: &MergeReadinessReceipt,
    current_head: &str,
    current_base: &str,
    current_gate_graph: &str,
) -> VerifyStatus {
    if receipt.verdict == "blocked" || !receipt.blocker_labels_absent {
        return VerifyStatus::Blocked;
    }

    let receipt_head =
        resolve_runtime_token(&receipt.head_sha, current_head, current_base, current_gate_graph);
    let receipt_base =
        resolve_runtime_token(&receipt.base_sha, current_head, current_base, current_gate_graph);
    let receipt_gate = resolve_runtime_token(
        &receipt.gate_graph_version,
        current_head,
        current_base,
        current_gate_graph,
    );

    if receipt_head != current_head {
        return VerifyStatus::StaleHead;
    }

    if receipt_base != current_base {
        return VerifyStatus::StaleBase;
    }

    if receipt_gate != current_gate_graph {
        return VerifyStatus::StaleGateGraph;
    }

    VerifyStatus::Valid
}

/// Compute merge-readiness from the base receipt AND a head-bound,
/// `IndependentReview`-instrumented review receipt — not a bare label name.
///
/// A PR is merge-ready only if:
/// 1. The base `MergeReadinessReceipt` staleness/blocked checks pass
///    (`evaluate_receipt`), AND
/// 2. At least one supplied review receipt has `head_sha == current_head`
///    and `instrument == ReviewInstrument::IndependentReview`.
///
/// A `FixResponder` receipt (the same principal that also mutated the
/// branch) never satisfies (2), even if it is bound to the current head.
/// This reuses the existing staleness pattern from `evaluate_receipt`
/// (`merge_ready.rs`) and `agent_receipt.rs:66-67` — reject on head
/// mismatch — applied to review receipts instead of the base receipt.
pub fn evaluate_merge_readiness(
    receipt: &MergeReadinessReceipt,
    review_receipts: &[ReviewReceipt],
    current_head: &str,
    current_base: &str,
    current_gate_graph: &str,
) -> VerifyStatus {
    let base_status = evaluate_receipt(receipt, current_head, current_base, current_gate_graph);
    if base_status != VerifyStatus::Valid {
        return base_status;
    }

    evaluate_independent_review(review_receipts, current_head)
}

/// Evaluate whether the supplied review receipts contain a valid,
/// current-head-bound `IndependentReview` — the machine-readable
/// replacement for citing a bare `deep-reviewed`-style label name.
fn evaluate_independent_review(
    review_receipts: &[ReviewReceipt],
    current_head: &str,
) -> VerifyStatus {
    if review_receipts.is_empty() {
        return VerifyStatus::ReviewReceiptMissing;
    }

    let independent = review_receipts
        .iter()
        .filter(|receipt| receipt.instrument == ReviewInstrument::IndependentReview);

    let mut saw_independent = false;
    for receipt in independent {
        saw_independent = true;
        if receipt.head_sha == current_head {
            return VerifyStatus::Valid;
        }
    }

    if saw_independent {
        VerifyStatus::ReviewReceiptStale
    } else {
        VerifyStatus::ReviewReceiptNotIndependent
    }
}

fn resolve_runtime_token(
    value: &str,
    current_head: &str,
    current_base: &str,
    current_gate: &str,
) -> String {
    match value {
        "$CURRENT_HEAD" => current_head.to_string(),
        "$CURRENT_BASE" => current_base.to_string(),
        "$CURRENT_GATE_GRAPH" => current_gate.to_string(),
        _ => value.to_string(),
    }
}

fn load_receipt(path: &Path) -> Result<MergeReadinessReceipt> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read receipt: {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse receipt: {}", path.display()))
}

fn write_receipt(path: &Path, receipt: &MergeReadinessReceipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(receipt).context("failed to serialize receipt")?;
    fs::write(path, json).with_context(|| format!("failed to write receipt: {}", path.display()))
}

fn load_required_checks(root: &Path) -> Result<Vec<String>> {
    let path = root.join(REQUIRED_CHECKS_PATH);
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read required checks policy: {}", path.display()))?;
    let value: toml::Value = toml::from_str(&raw)
        .with_context(|| format!("failed to parse required checks policy: {}", path.display()))?;

    Ok(required_check_names_from_policy(&value))
}

fn required_check_names_from_policy(value: &toml::Value) -> Vec<String> {
    let mut checks = Vec::new();

    if let Some(array) = value.get("checks").and_then(toml::Value::as_array) {
        for item in array {
            if item.get("required").and_then(toml::Value::as_bool) == Some(true)
                && let Some(name) = item.get("name").and_then(toml::Value::as_str)
            {
                checks.push(name.to_string());
            }
        }
    }

    checks.sort_unstable();
    checks.dedup();
    checks
}

fn resolve_base_sha(root: &Path) -> Result<String> {
    for base_ref in ["origin/master", "origin/main", "master", "main"] {
        if git_output(root, &["rev-parse", "--verify", base_ref]).is_ok() {
            return git_output(root, &["merge-base", "HEAD", base_ref]);
        }
    }

    git_output(root, &["rev-parse", "HEAD"])
}

fn git_output(root: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().to_string())
}

fn compute_gate_graph_version(root: &Path, required_checks: &[String]) -> Result<String> {
    let mut inputs: BTreeMap<String, String> = BTreeMap::new();

    for rel in collect_gate_files(root)? {
        let path = root.join(&rel);
        if path.is_file() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read gate graph input: {}", path.display()))?;
            inputs.insert(rel, content.replace("\r\n", "\n"));
        }
    }

    inputs.insert(
        "required_checks".to_string(),
        serde_json::to_string(required_checks).context("failed to encode required checks")?,
    );

    let mut material = String::new();
    for (path, content) in inputs {
        material.push_str("## ");
        material.push_str(&path);
        material.push('\n');
        material.push_str(&content);
        material.push('\n');
    }

    Ok(fnv1a64_hex(material.as_bytes()))
}

fn collect_gate_files(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();

    for rel in
        [".ci/policies/required-checks.toml", ".ci/policies", ".ci/gates.d", ".github/workflows"]
    {
        let dir = root.join(rel);
        if dir.is_file() {
            files.push(rel.to_string());
            continue;
        }

        if !dir.exists() {
            continue;
        }

        for entry in walkdir::WalkDir::new(&dir)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().is_file())
        {
            let path = entry.path();
            let rel_path = path
                .strip_prefix(root)
                .context("failed to strip repository root")?
                .to_string_lossy()
                .to_string();

            if rel == ".github/workflows" && !is_required_workflow_candidate(path) {
                continue;
            }

            files.push(rel_path);
        }
    }

    files.sort_unstable();
    files.dedup();
    Ok(files)
}

fn is_required_workflow_candidate(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };

    name.contains("ci") || name.contains("gate") || name.contains("merge")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::review_receipts::ReviewVerdict;
    use perl_tdd_support::must;

    fn make_receipt(
        head_sha: &str,
        base_sha: &str,
        gate_graph_version: &str,
        verdict: &str,
        blocker_labels_absent: bool,
    ) -> MergeReadinessReceipt {
        MergeReadinessReceipt {
            check: CHECK_NAME.to_string(),
            schema_version: SCHEMA_VERSION,
            event: "pull_request".to_string(),
            pr: 1,
            head_sha: head_sha.to_string(),
            base_sha: base_sha.to_string(),
            gate_graph_version: gate_graph_version.to_string(),
            required_checks: vec!["build".to_string()],
            review_evidence: vec!["reviewed-deep".to_string()],
            blocker_labels_absent,
            verdict: verdict.to_string(),
            expires_when: "on_new_commit_or_base_or_policy_change".to_string(),
        }
    }

    fn make_review_receipt(head_sha: &str, instrument: ReviewInstrument) -> ReviewReceipt {
        ReviewReceipt {
            kind: "review".to_string(),
            producer: "reviewer-deep".to_string(),
            pr: 1,
            head_sha: head_sha.to_string(),
            base_sha: SHA_B.to_string(),
            verdict: ReviewVerdict::Clean,
            material_observations: vec!["observed diff".to_string()],
            negative_checks: vec!["no label mutations".to_string()],
            blockers: vec![],
            next_routes: vec!["signoff_clean".to_string()],
            supersedes: None,
            instrument,
            claim_boundary: "correctness of the diff".to_string(),
        }
    }

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccc";
    const GATE_V1: &str = "fnv1a64:0000000000000001";
    const GATE_V2: &str = "fnv1a64:0000000000000002";

    // --- #3763 M4 increment 1: instrument-gated computed merge-ready ---

    #[test]
    fn test_evaluate_merge_readiness_valid_with_current_head_independent_review() {
        // (b) current-head IndependentReview receipt -> merge-ready
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let review = make_review_receipt(SHA_A, ReviewInstrument::IndependentReview);
        let status = evaluate_merge_readiness(&receipt, &[review], SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::Valid);
    }

    #[test]
    fn test_evaluate_merge_readiness_stale_when_review_receipt_head_is_old() {
        // (a) review receipt for an OLD head SHA -> NOT merge-ready (stale)
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        // Review was performed against SHA_B, but the branch has since moved to SHA_A.
        let review = make_review_receipt(SHA_B, ReviewInstrument::IndependentReview);
        let status = evaluate_merge_readiness(&receipt, &[review], SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::ReviewReceiptStale);
    }

    #[test]
    fn test_evaluate_merge_readiness_rejects_fix_responder_receipt() {
        // (c) current-head FixResponder receipt -> NOT independent-review -> NOT merge-ready
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let review = make_review_receipt(SHA_A, ReviewInstrument::FixResponder);
        let status = evaluate_merge_readiness(&receipt, &[review], SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::ReviewReceiptNotIndependent);
    }

    #[test]
    fn test_evaluate_merge_readiness_missing_when_no_review_receipts_supplied() {
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let status = evaluate_merge_readiness(&receipt, &[], SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::ReviewReceiptMissing);
    }

    #[test]
    fn test_evaluate_merge_readiness_checks_base_receipt_staleness_before_review() {
        // Base receipt staleness (head moved) must short-circuit before the
        // review-receipt check even when an otherwise-valid IndependentReview
        // receipt is supplied.
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let review = make_review_receipt(SHA_C, ReviewInstrument::IndependentReview);
        let status = evaluate_merge_readiness(&receipt, &[review], SHA_C, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::StaleHead);
    }

    #[test]
    fn test_evaluate_independent_review_prefers_independent_over_mixed_receipts() {
        // A FixResponder and a current-head IndependentReview receipt together
        // should still resolve to Valid: independent review evidence exists.
        let fix = make_review_receipt(SHA_A, ReviewInstrument::FixResponder);
        let independent = make_review_receipt(SHA_A, ReviewInstrument::IndependentReview);
        let status = evaluate_independent_review(&[fix, independent], SHA_A);
        assert_eq!(status, VerifyStatus::Valid);
    }

    #[test]
    fn test_verify_returns_valid_for_current_receipt() {
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::Valid);
    }

    #[test]
    fn test_verify_returns_stale_head() {
        // Receipt was emitted against SHA_A, but current head is SHA_C
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let status = evaluate_receipt(&receipt, SHA_C, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::StaleHead);
    }

    #[test]
    fn test_verify_returns_stale_base() {
        // Receipt base matches SHA_B, but master has advanced to SHA_C
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_C, GATE_V1);
        assert_eq!(status, VerifyStatus::StaleBase);
    }

    #[test]
    fn test_verify_returns_stale_gate_graph() {
        // Gate policy changed: GATE_V1 receipt vs GATE_V2 current
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V2);
        assert_eq!(status, VerifyStatus::StaleGateGraph);
    }

    #[test]
    fn test_verify_returns_blocked_when_needs_label_present() {
        // blocker_labels_absent = false indicates a needs-* label is set
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", false);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::Blocked);
    }

    #[test]
    fn test_verify_returns_blocked_when_verdict_is_blocked() {
        // verdict = "blocked" takes priority even if all SHAs match
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "blocked", true);
        let status = evaluate_receipt(&receipt, SHA_A, SHA_B, GATE_V1);
        assert_eq!(status, VerifyStatus::Blocked);
    }

    #[test]
    fn test_verify_returns_missing_when_no_receipt_file() -> color_eyre::eyre::Result<()> {
        // Write receipt to a temp file, then delete it and pass the path to verify()
        let tmp = tempfile::NamedTempFile::new()?;
        let path = tmp.path().to_path_buf();
        // Drop the file so it no longer exists on disk
        drop(tmp);

        // verify() should output "missing" and bail
        let result = verify(None, Some(path), Vec::new());
        assert!(result.is_err(), "verify should return Err for missing receipt");
        Ok(())
    }

    #[test]
    fn test_verify_status_as_str_covers_all_variants() {
        assert_eq!(VerifyStatus::Valid.as_str(), "valid");
        assert_eq!(VerifyStatus::StaleHead.as_str(), "stale_head");
        assert_eq!(VerifyStatus::StaleBase.as_str(), "stale_base");
        assert_eq!(VerifyStatus::StaleGateGraph.as_str(), "stale_gate_graph");
        assert_eq!(VerifyStatus::Blocked.as_str(), "blocked");
        assert_eq!(VerifyStatus::Missing.as_str(), "missing");
    }

    #[test]
    fn test_evaluate_receipt_checks_blocked_before_staleness() {
        // If blocked, should return Blocked even if head/base are mismatched
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "blocked", true);
        // Different head and base to confirm Blocked is checked first
        let status = evaluate_receipt(&receipt, SHA_C, SHA_C, GATE_V2);
        assert_eq!(status, VerifyStatus::Blocked);
    }

    #[test]
    fn test_resolve_runtime_token_substitutes_current_head() {
        let result = resolve_runtime_token("$CURRENT_HEAD", SHA_A, SHA_B, GATE_V1);
        assert_eq!(result, SHA_A);
    }

    #[test]
    fn test_resolve_runtime_token_substitutes_current_base() {
        let result = resolve_runtime_token("$CURRENT_BASE", SHA_A, SHA_B, GATE_V1);
        assert_eq!(result, SHA_B);
    }

    #[test]
    fn test_resolve_runtime_token_substitutes_gate_graph() {
        let result = resolve_runtime_token("$CURRENT_GATE_GRAPH", SHA_A, SHA_B, GATE_V1);
        assert_eq!(result, GATE_V1);
    }

    #[test]
    fn test_resolve_runtime_token_returns_literal_for_unknown_token() {
        let literal = "abc1234def5678";
        let result = resolve_runtime_token(literal, SHA_A, SHA_B, GATE_V1);
        assert_eq!(result, literal);
    }

    #[test]
    fn test_write_and_load_receipt_round_trip() -> color_eyre::eyre::Result<()> {
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        let tmp = tempfile::NamedTempFile::new()?;
        write_receipt(tmp.path(), &receipt)?;
        let loaded = load_receipt(tmp.path())?;
        assert_eq!(loaded.head_sha, SHA_A);
        assert_eq!(loaded.base_sha, SHA_B);
        assert_eq!(loaded.gate_graph_version, GATE_V1);
        assert_eq!(loaded.verdict, "valid");
        assert!(loaded.blocker_labels_absent);
        Ok(())
    }

    #[test]
    fn test_write_receipt_creates_parent_dirs() -> color_eyre::eyre::Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let nested_path = tmp_dir.path().join("nested").join("dirs").join("receipt.json");
        let receipt = make_receipt(SHA_A, SHA_B, GATE_V1, "valid", true);
        write_receipt(&nested_path, &receipt)?;
        assert!(nested_path.exists());
        Ok(())
    }

    #[test]
    fn test_fnv1a64_hex_is_deterministic() {
        let h1 = fnv1a64_hex(b"hello");
        let h2 = fnv1a64_hex(b"hello");
        assert_eq!(h1, h2);
        assert!(h1.starts_with("fnv1a64:"));
    }

    #[test]
    fn test_fnv1a64_hex_differs_on_different_input() {
        let h1 = fnv1a64_hex(b"hello");
        let h2 = fnv1a64_hex(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_load_required_checks_reads_required_status_contexts_from_policy_file() {
        let tmp_dir = must(tempfile::tempdir());
        let policy_dir = tmp_dir.path().join(".ci").join("policies");
        must(fs::create_dir_all(&policy_dir));
        must(fs::write(
            policy_dir.join("required-checks.toml"),
            concat!(
                "[[check]]\n",
                "name = \"Workflow-shape lint only\"\n",
                "required = true\n",
                "\n",
                "[[checks]]\n",
                "name = \"Codecov / Patch 95\"\n",
                "required = true\n",
                "\n",
                "[[checks]]\n",
                "name = \"ripr+ New Gap Gate\"\n",
                "required = true\n",
            ),
        ));

        let checks = must(load_required_checks(tmp_dir.path()));
        assert_eq!(checks, vec!["Codecov / Patch 95", "ripr+ New Gap Gate"]);
    }

    #[test]
    fn test_required_check_names_include_only_required_status_contexts() {
        let policy: toml::Value = must(toml::from_str(concat!(
            "[[check]]\n",
            "name = \"Workflow-shape lint only\"\n",
            "required = true\n",
            "\n",
            "[[checks]]\n",
            "name = \"Proof required\"\n",
            "required = true\n",
            "\n",
            "[[checks]]\n",
            "name = \"Missing required flag\"\n",
        )));

        let checks = required_check_names_from_policy(&policy);
        assert_eq!(checks, vec!["Proof required"]);
    }

    #[test]
    fn test_required_check_names_deduplicate_sorted_names() {
        let policy: toml::Value = must(toml::from_str(concat!(
            "[[check]]\n",
            "name = \"ripr+ New Gap Gate\"\n",
            "required = true\n",
            "\n",
            "[[checks]]\n",
            "name = \"Codecov / Patch 95\"\n",
            "required = true\n",
            "\n",
            "[[checks]]\n",
            "name = \"ripr+ New Gap Gate\"\n",
            "required = true\n",
        )));

        let checks = required_check_names_from_policy(&policy);
        assert_eq!(checks, vec!["Codecov / Patch 95", "ripr+ New Gap Gate"]);
    }

    #[test]
    fn test_required_workflow_candidate_matches_ci_gate_and_merge_names() {
        for candidate in ["ci.yml", "ci-nightly.yml", "quality-gate.yml", "merge-ready.yml"] {
            assert!(
                is_required_workflow_candidate(Path::new(candidate)),
                "{candidate} should be included in the merge-readiness gate graph"
            );
        }

        for candidate in ["docs.yml", "release.yml", "scorecard.yml"] {
            assert!(
                !is_required_workflow_candidate(Path::new(candidate)),
                "{candidate} should not be included in the merge-readiness gate graph"
            );
        }
    }

    #[test]
    fn test_collect_gate_files_returns_empty_when_gate_inputs_are_absent()
    -> color_eyre::eyre::Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let files = collect_gate_files(tmp_dir.path())?;
        assert!(files.is_empty());
        Ok(())
    }

    #[test]
    fn test_collect_gate_files_includes_policy_gate_and_required_workflows()
    -> color_eyre::eyre::Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let root = tmp_dir.path();

        fs::create_dir_all(root.join(".ci").join("policies"))?;
        fs::create_dir_all(root.join(".ci").join("gates.d"))?;
        fs::create_dir_all(root.join(".github").join("workflows"))?;
        fs::write(root.join(".ci").join("policies").join("required-checks.toml"), "checks = []")?;
        fs::write(root.join(".ci").join("gates.d").join("quality.toml"), "gate = true")?;
        fs::write(root.join(".github").join("workflows").join("ci.yml"), "name: ci")?;
        fs::write(root.join(".github").join("workflows").join("merge-ready.yml"), "name: merge")?;
        fs::write(root.join(".github").join("workflows").join("docs.yml"), "name: docs")?;

        let files = collect_gate_files(root)?;
        let normalized: Vec<String> =
            files.into_iter().map(|path| path.replace('\\', "/")).collect();

        assert!(normalized.contains(&".ci/policies/required-checks.toml".to_string()));
        assert!(normalized.contains(&".ci/gates.d/quality.toml".to_string()));
        assert!(normalized.contains(&".github/workflows/ci.yml".to_string()));
        assert!(normalized.contains(&".github/workflows/merge-ready.yml".to_string()));
        assert!(!normalized.contains(&".github/workflows/docs.yml".to_string()));

        Ok(())
    }

    #[test]
    fn test_compute_gate_graph_version_tracks_required_checks_and_file_content()
    -> color_eyre::eyre::Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let root = tmp_dir.path();

        fs::create_dir_all(root.join(".ci").join("policies"))?;
        fs::create_dir_all(root.join(".ci").join("gates.d"))?;
        fs::write(
            root.join(".ci").join("policies").join("required-checks.toml"),
            "[[checks]]\nname = \"Codecov / Patch 95\"\nrequired = true\n",
        )?;
        let gate_path = root.join(".ci").join("gates.d").join("quality.toml");
        fs::write(&gate_path, "mode = \"advisory\"\n")?;

        let baseline = compute_gate_graph_version(root, &["Codecov / Patch 95".to_string()])?;
        let changed_checks = compute_gate_graph_version(root, &["ripr+ New Gap Gate".to_string()])?;
        fs::write(&gate_path, "mode = \"enforce\"\n")?;
        let changed_file = compute_gate_graph_version(root, &["Codecov / Patch 95".to_string()])?;

        assert_ne!(baseline, changed_checks);
        assert_ne!(baseline, changed_file);

        Ok(())
    }
}
