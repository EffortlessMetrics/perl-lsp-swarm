//! E1 gap precision/recall bench (#17154).
//!
//! The new-RIPR-gap gate (`cargo xtask quality-gate --mode enforce-new-ripr`)
//! is the merge-blocking oracle for fresh RIPR seams. This bench scores its
//! fidelity over the labeled `fixtures/ripr-gate/` matrix the way E2 scores
//! boundary recall: every case declares its ground truth (`seam`, `no-seam`,
//! or `unprovable`) plus the exact expected decision, exit, and next actions,
//! and the bench asserts each one through the real CLI.
//!
//! Harness 1 (gate matrix, all platforms): stages each case into one hermetic
//! temp git repository (pinned identity/timestamps, no network), substitutes
//! the `E1_HEAD_SHA` token with the runtime head, runs the real
//! `quality-gate --mode enforce-new-ripr` with explicit receipt paths, and
//! asserts exit code, receipt decision/actions, markdown summary, and a `--check`
//! replay. FP = block on a `no-seam` case; FN = pass on a `seam`/`unprovable`
//! case. Gate: FP = 0 and FN = 0 — stricter than the issue's FP <= 5%, because
//! every labeled case must match exactly. Any decision/exit MISMATCH against a
//! pinned expectation also fails, so fixture rot cannot hide in a green run.
//!
//! Harness 2 (classifier mapping, unix): pipes each classifier case through
//! the real `scripts/ci/classify-ripr-lane-termination` (log-file, log-stdin,
//! and api-evidence modes, exactly as `ripr.yml` invokes it) and asserts the
//! mapped class plus evidence counters. Base inputs prove each mutant flips.
//! The classifier is bash-only, so non-unix runs report SKIP (CI enforces).
//!
//! Mutants: 12 attempted. 9 kill (8 decision flips + 1 NOT_PROVEN reporting
//! pin), 3 dropped-cannot-fail with written reasons in their `case.toml`
//! (`suppressed-only`, `dropped-over-credit`, `dropped-nonprod-hiding`) — kept
//! as deliberate-pass contract pins, counted as true negatives.
//!
//! NOT_PROVEN boundaries: live git/network drift (hermetic repo by design),
//! real OOM log shapes (synthetic lane logs), upstream producer correctness
//! (the gate trusts receipt contents), unfixed interleavings outside the matrix.

// The bench prints its score table; the workspace-wide print denial is a
// production-code rule.
#![allow(clippy::print_stdout)]

#[cfg(unix)]
use std::{collections::BTreeMap, process::Command as StdCommand};
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use assert_cmd::Command;

mod git_test_support;

use git_test_support::HermeticGit;

/// Token every gate fixture binds its receipt heads with; substituted with the
/// runtime hermetic head at stage time so fixtures stay checkout-independent.
const HEAD_TOKEN: &str = "E1_HEAD_SHA";

/// Every case the matrix must contain. A stray or missing case directory fails
/// the bench: silent matrix drift is a recall hole.
const EXPECTED_CASES: &[&str] = &[
    "clean-zero",
    "dropped-nonprod-hiding",
    "dropped-over-credit",
    "genuine-seam",
    "invalid",
    "missing",
    "mutant-annotation-loss",
    "mutant-gap-receipt-outranks",
    "mutant-genuine-with-teardown",
    "mutant-incomplete-static",
    "mutant-missing-comments",
    "mutant-mixed-scope",
    "mutant-stale-base",
    "mutant-truncated-log-pass",
    "mutant-unnamed-seams",
    "nonprod-only",
    "one-seam",
    "stale",
    "static-creditable",
    "suppressed-only",
    "unknown-count",
];

const MINIMAL_EXCEPTION_POLICY: &str = "schema_version = 1\npolicy = \"quality-gate-exceptions\"\nowner = \"test\"\nstatus = \"active\"\nupdated = \"2026-01-01\"\ndue_review = \"fail\"\n[requirements]\nrequired_active = []\n";

fn repo_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask manifest must be nested under repo root")
}

fn matrix_dir() -> Result<PathBuf> {
    Ok(repo_root()?.join("fixtures").join("ripr-gate"))
}

fn read_case_manifest(case_dir: &Path) -> Result<toml::Value> {
    let raw = fs::read_to_string(case_dir.join("case.toml"))
        .with_context(|| format!("reading {}", case_dir.join("case.toml").display()))?;
    toml::from_str(&raw)
        .with_context(|| format!("parsing {}", case_dir.join("case.toml").display()))
}

fn manifest_str(manifest: &toml::Value, field: &str, case: &str) -> Result<String> {
    manifest
        .get(field)
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .with_context(|| format!("case `{case}` manifest must declare `{field}`"))
}

fn manifest_string_list(manifest: &toml::Value, field: &str, case: &str) -> Result<Vec<String>> {
    let items = manifest
        .get(field)
        .and_then(toml::Value::as_array)
        .with_context(|| format!("case `{case}` manifest must declare `{field}`"))?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(
            item.as_str()
                .with_context(|| format!("case `{case}` manifest `{field}` must hold strings"))?
                .to_string(),
        );
    }
    Ok(out)
}

/// The matrix is a fixed oracle: exactly these cases, no more, no fewer.
fn assert_matrix_identity() -> Result<Vec<(String, PathBuf)>> {
    let matrix = matrix_dir()?;
    let mut cases = Vec::new();
    for entry in
        fs::read_dir(&matrix).with_context(|| format!("reading matrix {}", matrix.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            cases.push((entry.file_name().to_string_lossy().into_owned(), entry.path()));
        }
    }
    cases.sort();
    let names: Vec<&str> = cases.iter().map(|(name, _)| name.as_str()).collect();
    if names.as_slice() != EXPECTED_CASES {
        bail!(
            "E1 matrix drift: found [{}], want [{}]",
            names.join(", "),
            EXPECTED_CASES.join(", ")
        );
    }
    Ok(cases)
}

fn cases_of_kind(kind: &str) -> Result<Vec<(String, PathBuf)>> {
    let mut selected = Vec::new();
    for (name, dir) in assert_matrix_identity()? {
        let manifest = read_case_manifest(&dir)?;
        if manifest_str(&manifest, "kind", &name)? == kind {
            selected.push((name, dir));
        }
    }
    Ok(selected)
}

// ---------------------------------------------------------------------------
// Harness 1: gate matrix (all platforms).
// ---------------------------------------------------------------------------

/// One hermetic git repository for the whole matrix: pinned identity and
/// timestamps, no network. The gate resolves its head from the cwd via
/// `git rev-parse HEAD`, so every case stages under this repo and binds its
/// receipts to the runtime head through the `E1_HEAD_SHA` token.
struct HermeticStage {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    hermetic: HermeticGit,
    head: String,
}

impl HermeticStage {
    fn create() -> Result<Self> {
        let temp = tempfile::tempdir().context("creating E1 stage tempdir")?;
        let hermetic = HermeticGit::at(&temp.path().join("pins"))?;
        let repo = temp.path().join("e1-repo");
        hermetic.init_repo(&repo)?;
        fs::write(repo.join("e1-subject.txt"), "E1 hermetic head subject\n")?;
        hermetic.git(&repo, &["add", "e1-subject.txt"])?;
        hermetic.git(&repo, &["commit", "-m", "E1 bench hermetic head"])?;
        let head = hermetic.git(&repo, &["rev-parse", "HEAD"])?;
        if head.len() != 40 {
            bail!("hermetic head must be a 40-character SHA-1 identity, got `{head}`");
        }
        Ok(Self { _temp: temp, repo, hermetic, head })
    }

    /// Stage one gate case: declared inputs only, token substituted, plus the
    /// fixed (non-seam) exception policy and output paths.
    fn stage_gate_case(
        &self,
        name: &str,
        case_dir: &Path,
        manifest: &toml::Value,
    ) -> Result<GatePaths> {
        let declared = manifest_string_list(manifest, "inputs", name)?;
        for input in &declared {
            if !matches!(input.as_str(), "ripr-plus.json" | "repo-exposure.json" | "comments.json")
            {
                bail!("case `{name}` declares unknown gate input `{input}`");
            }
        }
        assert_case_file_set(
            name,
            case_dir,
            &declared,
            None,
            &[
                "case.toml",
                "head_sha",
                "expected_decision",
                "expected_exit",
                "expected_next_actions.json",
            ],
        )?;
        let stage = self.repo.join("cases").join(name);
        fs::create_dir_all(&stage)?;
        for input in &declared {
            let raw = fs::read_to_string(case_dir.join(input)).with_context(|| {
                format!("case `{name}` declares `{input}` but the file is missing")
            })?;
            let staged = raw.replace(HEAD_TOKEN, &self.head);
            if staged.contains(HEAD_TOKEN) {
                bail!("case `{name}` input `{input}` kept an unsubstituted head token");
            }
            fs::write(stage.join(input), staged)?;
        }
        // The stale sentinel is length-incompatible with a real head by
        // construction (44 chars vs the 40-char hex asserted at stage
        // creation), so a stale fixture can never match the runtime head.
        let policy = stage.join("quality-gate-exceptions.toml");
        fs::write(&policy, MINIMAL_EXCEPTION_POLICY)?;
        Ok(GatePaths {
            ripr: stage.join("ripr-plus.json"),
            ripr_pr: stage.join("repo-exposure.json"),
            review: stage.join("comments.json"),
            receipt: stage.join("quality-gate.json"),
            summary: stage.join("quality-gate.md"),
            policy,
        })
    }

    fn gate_command(&self, paths: &GatePaths, check: bool) -> Result<Command> {
        let mut command = Command::cargo_bin("xtask")?;
        command.current_dir(&self.repo).args(["quality-gate", "--mode", "enforce-new-ripr"]);
        command.arg("--exception-policy").arg(&paths.policy);
        command.arg("--ripr-receipt").arg(&paths.ripr);
        command.arg("--ripr-pr-receipt").arg(&paths.ripr_pr);
        command.arg("--review-receipt").arg(&paths.review);
        command.arg("--receipt").arg(&paths.receipt);
        command.arg("--summary").arg(&paths.summary);
        if check {
            command.arg("--check");
        }
        self.hermetic.apply_env_to_assert(&mut command);
        Ok(command)
    }
}

struct GatePaths {
    ripr: PathBuf,
    ripr_pr: PathBuf,
    review: PathBuf,
    receipt: PathBuf,
    summary: PathBuf,
    policy: PathBuf,
}

fn read_trimmed(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?
        .trim()
        .to_string())
}

/// Every pinned field of one expected action must equal the actual action's
/// field. `kind`/`blocking` are always pinned; the fixture adds whatever else
/// discriminates the arm (counts, proven bits, reasons).
fn action_matches(
    actual: &serde_json::Value,
    pinned: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    pinned.iter().all(|(field, want)| actual.get(field) == Some(want))
}

struct GateOutcome {
    ground_truth: String,
    expected_decision: String,
    actual_decision: String,
    exit_matched: bool,
}

#[allow(clippy::too_many_lines)]
fn run_gate_case(
    stage: &HermeticStage,
    name: &str,
    case_dir: &Path,
    manifest: &toml::Value,
    report: &mut String,
) -> Result<GateOutcome> {
    let ground_truth = manifest_str(manifest, "ground_truth", name)?;
    if !matches!(ground_truth.as_str(), "seam" | "no-seam" | "unprovable") {
        bail!("case `{name}` declares unknown ground truth `{ground_truth}`");
    }
    if read_trimmed(&case_dir.join("head_sha"))? != HEAD_TOKEN {
        bail!("case `{name}` head_sha must name the {HEAD_TOKEN} binding token");
    }
    let expected_decision = read_trimmed(&case_dir.join("expected_decision"))?;
    if !matches!(expected_decision.as_str(), "pass" | "fail") {
        bail!("case `{name}` expected_decision must be pass or fail");
    }
    let expected_exit = read_trimmed(&case_dir.join("expected_exit"))?;
    if !matches!(expected_exit.as_str(), "0" | "nonzero") {
        bail!("case `{name}` expected_exit must be 0 or nonzero");
    }
    let expected_actions: Vec<serde_json::Value> =
        serde_json::from_str(&fs::read_to_string(case_dir.join("expected_next_actions.json"))?)
            .with_context(|| {
                format!("case `{name}` expected_next_actions.json must be a JSON array")
            })?;

    let paths = stage.stage_gate_case(name, case_dir, manifest)?;
    let output = stage
        .gate_command(&paths, false)?
        .output()
        .with_context(|| format!("case `{name}` quality-gate spawn failed"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let exited_zero = output.status.success();
    let exit_matched = (expected_exit == "0") == exited_zero;

    let receipt: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&paths.receipt)
            .with_context(|| format!("case `{name}` produced no JSON receipt"))?,
    )
    .with_context(|| format!("case `{name}` receipt is not valid JSON"))?;
    let decision = receipt.get("decision").and_then(serde_json::Value::as_str).unwrap_or("unknown");

    let matched = decision == expected_decision && exit_matched;
    report.push_str(&format!(
        "{name} | truth={ground_truth} expected={expected_decision}/{expected_exit} actual={decision}/{} | {}\n",
        if exited_zero { "0" } else { "nonzero" },
        if matched { "MATCH" } else { "MISMATCH" }
    ));
    // A decision flip is bench signal, not an infra error: record it and keep
    // scoring the matrix so one red case cannot hide another. Detailed action
    // and markdown assertions only run on matched cases to avoid cascades.
    if !matched {
        return Ok(GateOutcome {
            ground_truth,
            expected_decision,
            actual_decision: decision.to_string(),
            exit_matched,
        });
    }

    // The terminal lines are also the lane classifier's input contract: a fail
    // must print the grep-able receipt line, a pass must say so plainly.
    if expected_decision == "fail" {
        if !stderr.contains("quality gate failed; see receipt") {
            bail!(
                "case `{name}` fail run must print the terminal receipt line\n{stderr}\n{report}"
            );
        }
    } else if !stdout.contains("quality gate passed") {
        bail!("case `{name}` pass run must print the pass line\n{stdout}\n{report}");
    }
    if receipt.get("mode").and_then(serde_json::Value::as_str) != Some("enforce-new-ripr") {
        bail!("case `{name}` receipt mode must be enforce-new-ripr: {receipt}");
    }
    if receipt.get("head").and_then(serde_json::Value::as_str) != Some(stage.head.as_str()) {
        bail!("case `{name}` receipt must bind the hermetic head: {receipt}");
    }
    let actual_actions = receipt
        .get("next_actions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if actual_actions.len() != expected_actions.len() {
        bail!(
            "case `{name}` next_actions length mismatch: want {}, got {}: {receipt}\n{report}",
            expected_actions.len(),
            actual_actions.len()
        );
    }
    for pinned in &expected_actions {
        let pinned = pinned.as_object().with_context(|| {
            format!("case `{name}` expected_next_actions.json entries must be objects")
        })?;
        if !actual_actions.iter().any(|actual| action_matches(actual, pinned)) {
            bail!("case `{name}` no actual action matches {pinned:?}: {receipt}\n{report}");
        }
    }

    let markdown = fs::read_to_string(&paths.summary)
        .with_context(|| format!("case `{name}` produced no markdown summary"))?;
    if !markdown.contains(&format!("- decision: `{expected_decision}`")) {
        bail!("case `{name}` summary must name the decision\n{markdown}\n{report}");
    }
    if !markdown.contains("- mode: `enforce-new-ripr`") {
        bail!("case `{name}` summary must name the mode\n{markdown}\n{report}");
    }
    if expected_actions.is_empty() {
        if !markdown.contains("- none") {
            bail!("case `{name}` empty-action summary must render `- none`\n{markdown}\n{report}");
        }
    } else {
        for pinned in &expected_actions {
            let kind = pinned.get("kind").and_then(serde_json::Value::as_str).unwrap_or("unknown");
            if !markdown.contains(&format!("### {kind}")) {
                bail!("case `{name}` summary must render `### {kind}`\n{markdown}\n{report}");
            }
        }
    }
    // The NOT_PROVEN label is data-driven: fixtures pinning gap_list_proven
    // false must see it, fixtures pinning true must not.
    let pins_unproven = expected_actions.iter().any(|pinned| {
        pinned.get("gap_list_proven").and_then(serde_json::Value::as_bool) == Some(false)
    });
    let pins_proven = expected_actions.iter().any(|pinned| {
        pinned.get("gap_list_proven").and_then(serde_json::Value::as_bool) == Some(true)
    });
    if pins_unproven && !markdown.contains("NOT_PROVEN") {
        bail!("case `{name}` unproven gap list must be labeled NOT_PROVEN\n{markdown}\n{report}");
    }
    if pins_proven && markdown.contains("NOT_PROVEN") {
        bail!("case `{name}` proven gap list must not be labeled NOT_PROVEN\n{markdown}\n{report}");
    }

    // --check replay: identical verdict, and no staleness finding.
    let check_output = stage
        .gate_command(&paths, true)?
        .output()
        .with_context(|| format!("case `{name}` quality-gate --check spawn failed"))?;
    let check_stderr = String::from_utf8_lossy(&check_output.stderr);
    if (expected_exit == "0") != check_output.status.success() {
        bail!(
            "case `{name}` --check exit mismatch: want `{expected_exit}`, status {}\nstderr:\n{check_stderr}\n{report}",
            check_output.status
        );
    }
    if check_stderr.contains("is stale") {
        bail!("case `{name}` --check must not report staleness\n{check_stderr}\n{report}");
    }

    Ok(GateOutcome {
        ground_truth,
        expected_decision,
        actual_decision: decision.to_string(),
        exit_matched,
    })
}

/// False positive = block on a `no-seam` case. False negative = pass on a
/// `seam` or `unprovable` case.
#[test]
fn e1_gate_matrix_precision_recall() -> Result<()> {
    let stage = HermeticStage::create()?;
    let mut report = String::from("E1 gate matrix (hermetic head binds every receipt)\n");
    report.push_str("case | truth expected actual | verdict\n");
    let mut pass_cases = 0u32;
    let mut fail_cases = 0u32;
    let mut false_positives = 0u32;
    let mut false_negatives = 0u32;
    let mut mismatches = 0u32;

    for (name, dir) in cases_of_kind("gate")? {
        let manifest = read_case_manifest(&dir)?;
        let outcome = run_gate_case(&stage, &name, &dir, &manifest, &mut report)?;
        // A decision/exit flip against the pinned expectation is bench signal
        // even when ground-truth scoring still holds: the pins are part of the
        // oracle, and a silent MISMATCH would let fixture rot hide in a green run.
        if outcome.expected_decision != outcome.actual_decision || !outcome.exit_matched {
            mismatches += 1;
        }
        if outcome.expected_decision == "fail" {
            fail_cases += 1;
        } else {
            pass_cases += 1;
        }
        // The metric scores ACTUAL gate behavior against ground truth: a block
        // on a no-seam case is a false positive, a pass on a seam/unprovable
        // case is a false negative.
        let blocked = outcome.actual_decision == "fail";
        match (outcome.ground_truth.as_str(), blocked) {
            ("no-seam", true) => false_positives += 1,
            ("seam" | "unprovable", false) => false_negatives += 1,
            _ => {}
        }
    }

    #[allow(clippy::cast_precision_loss)]
    let fp_rate = f64::from(false_positives) / f64::from(pass_cases.max(1));
    #[allow(clippy::cast_precision_loss)]
    let fn_rate = f64::from(false_negatives) / f64::from(fail_cases.max(1));
    report.push_str(&format!(
        "FP={false_positives}/{pass_cases} FP_rate={fp_rate:.4} FN={false_negatives}/{fail_cases} FN_rate={fn_rate:.4} mismatches={mismatches}\n"
    ));
    println!("{report}");
    if false_positives != 0 || false_negatives != 0 || mismatches != 0 {
        bail!("E1 gate is imprecise:\n{report}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Harness 2: classifier mapping (unix; the lane classifier is bash-only).
// ---------------------------------------------------------------------------

/// Every file in a case directory must be declared: an undeclared input is
/// either dead weight or a silently ignored oracle.
fn assert_case_file_set(
    name: &str,
    case_dir: &Path,
    inputs: &[String],
    base_input: Option<&str>,
    metadata: &[&str],
) -> Result<()> {
    let mut want: Vec<String> = inputs.to_vec();
    if let Some(base) = base_input {
        want.push(base.to_string());
    }
    want.extend(metadata.iter().map(ToString::to_string));
    want.sort();
    let mut found: Vec<String> = Vec::new();
    for entry in fs::read_dir(case_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            found.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    found.sort();
    if found != want {
        bail!(
            "case `{name}` file set mismatch: found [{}], want [{}]",
            found.join(", "),
            want.join(", ")
        );
    }
    Ok(())
}

#[cfg(unix)]
fn parse_classifier_output(output: &str) -> BTreeMap<String, String> {
    // First occurrence wins, matching the self-test's `sed ... | head -1`.
    let mut fields = BTreeMap::new();
    for line in output.lines() {
        if let Some((key, value)) = line.split_once('=')
            && !key.is_empty()
            && !fields.contains_key(key)
        {
            fields.insert(key.to_string(), value.to_string());
        }
    }
    fields
}

#[cfg(unix)]
fn classifier_script() -> Result<PathBuf> {
    let script = repo_root()?.join("scripts").join("ci").join("classify-ripr-lane-termination");
    if !script.is_file() {
        bail!("classifier script is missing: {}", script.display());
    }
    Ok(script)
}

/// Run the real classifier exactly as `ripr.yml` invokes it (`bash
/// scripts/ci/classify-ripr-lane-termination ...`). Exit status is always 0
/// for a performed classification; misuse (exit 64) is an infra failure.
#[cfg(unix)]
fn classify(
    args: &[String],
    stdin_bytes: Option<&[u8]>,
    case: &str,
) -> Result<BTreeMap<String, String>> {
    use std::process::Stdio;

    let mut command = StdCommand::new("bash");
    command.arg(classifier_script()?).args(args);
    if stdin_bytes.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child =
        command.spawn().with_context(|| format!("case `{case}` classifier spawn failed"))?;
    if let Some(bytes) = stdin_bytes {
        use std::io::Write as _;
        child
            .stdin
            .as_mut()
            .context("classifier stdin unavailable")?
            .write_all(bytes)
            .with_context(|| format!("case `{case}` classifier stdin write failed"))?;
        // EOF the pipe: `head -c` inside the classifier blocks for more input
        // until the write end closes, so waiting first would deadlock.
        drop(child.stdin.take());
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("case `{case}` classifier wait failed"))?;
    if !output.status.success() {
        bail!(
            "case `{case}` classifier exited {} (usage error): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(parse_classifier_output(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(unix)]
fn assert_mapping(
    fields: &BTreeMap<String, String>,
    want_class: &str,
    also_expect: &BTreeMap<String, String>,
    what: &str,
    case: &str,
    report: &mut String,
) -> Result<bool> {
    let actual = fields.get("classification").map(String::as_str).unwrap_or("unknown");
    let mut matched = actual == want_class;
    for (field, want) in also_expect {
        if fields.get(field).map(String::as_str) != Some(want.as_str()) {
            matched = false;
        }
    }
    report.push_str(&format!(
        "{case} {what} | expected={want_class} actual={actual} | {}\n",
        if matched { "MATCH" } else { "MISMATCH" }
    ));
    if !matched {
        report.push_str(&format!("{case} {what} | classifier output: {fields:?}\n"));
    }
    Ok(matched)
}

#[cfg(unix)]
fn manifest_optional_str(manifest: &toml::Value, field: &str) -> Option<String> {
    manifest.get(field).and_then(toml::Value::as_str).map(str::to_string)
}

#[cfg(unix)]
fn manifest_also_expect(manifest: &toml::Value, case: &str) -> Result<BTreeMap<String, String>> {
    let mut pins = BTreeMap::new();
    if let Some(table) = manifest.get("also_expect").and_then(toml::Value::as_table) {
        for (field, value) in table {
            pins.insert(
                field.clone(),
                value
                    .as_str()
                    .with_context(|| format!("case `{case}` also_expect values must be strings"))?
                    .to_string(),
            );
        }
    }
    Ok(pins)
}

/// One classifier case: the mutant mapping plus, when declared, the base
/// mapping proving the flip. Returns (checks, matched).
#[cfg(unix)]
#[allow(clippy::too_many_lines)]
fn run_classifier_case(
    name: &str,
    case_dir: &Path,
    manifest: &toml::Value,
    report: &mut String,
) -> Result<(u32, u32)> {
    let mode = manifest_str(manifest, "mode", name)?;
    if !matches!(mode.as_str(), "log-file" | "log-stdin" | "api-evidence") {
        bail!("case `{name}` declares unknown classifier mode `{mode}`");
    }
    let inputs = manifest_string_list(manifest, "inputs", name)?;
    if inputs.is_empty() {
        bail!("case `{name}` must declare at least one classifier input");
    }
    let base_input = manifest_optional_str(manifest, "base_input");
    let base_expected_class = manifest_optional_str(manifest, "base_expected_class");
    let expected_class = read_trimmed(&case_dir.join("expected_class"))?;
    let also_expect = manifest_also_expect(manifest, name)?;
    assert_case_file_set(
        name,
        case_dir,
        &inputs,
        base_input.as_deref(),
        &["case.toml", "expected_class"],
    )?;

    let stage = tempfile::tempdir().context("creating E1 classifier stage")?;
    for input in inputs.iter().chain(base_input.iter()) {
        fs::copy(case_dir.join(input), stage.path().join(input))
            .with_context(|| format!("case `{name}` staging `{input}` failed"))?;
    }

    let mut checks = 0u32;
    let mut matched = 0u32;
    if mode == "log-file" {
        let log = stage.path().join(&inputs[0]).to_string_lossy().into_owned();
        let max_bytes = manifest.get("max_bytes").and_then(toml::Value::as_integer);
        let mut args = vec![log.clone()];
        if let Some(cap) = max_bytes {
            args.push(cap.to_string());
        }
        let fields = classify(&args, None, name)?;
        checks += 1;
        if assert_mapping(&fields, &expected_class, &also_expect, "mutant", name, report)? {
            matched += 1;
        }
        // Base: the same log fully scanned (no cap) must take the other path.
        if let Some(base_class) = base_expected_class {
            let fields = classify(std::slice::from_ref(&log), None, name)?;
            checks += 1;
            if assert_mapping(&fields, &base_class, &BTreeMap::new(), "base", name, report)? {
                matched += 1;
            }
        }
    } else if mode == "log-stdin" {
        let bytes = fs::read(stage.path().join(&inputs[0]))?;
        let fields = classify(&["-".to_string()], Some(&bytes), name)?;
        checks += 1;
        if assert_mapping(&fields, &expected_class, &also_expect, "mutant", name, report)? {
            matched += 1;
        }
        if let (Some(base_file), Some(base_class)) = (base_input, base_expected_class) {
            let bytes = fs::read(stage.path().join(&base_file))?;
            let fields = classify(&["-".to_string()], Some(&bytes), name)?;
            checks += 1;
            if assert_mapping(&fields, &base_class, &BTreeMap::new(), "base", name, report)? {
                matched += 1;
            }
        }
    } else {
        // api-evidence: without an `annotations` field the mutant run points
        // at an annotations path that was never staged (the loss itself); a
        // case may instead name staged annotations plus a `gap_receipt` lane
        // log, exercising ripr.yml's `--gap-receipt` invocation form. The base
        // run always uses the kept evidence without the flag.
        let steps = stage.path().join(&inputs[0]).to_string_lossy().into_owned();
        let annotations = match manifest_optional_str(manifest, "annotations") {
            Some(file) => stage.path().join(&file).to_string_lossy().into_owned(),
            None => stage.path().join("annotations.json").to_string_lossy().into_owned(),
        };
        let mut args = vec![String::from("--api-evidence"), annotations, steps.clone()];
        if let Some(receipt) = manifest_optional_str(manifest, "gap_receipt") {
            let staged = stage.path().join(&receipt);
            if !staged.is_file() {
                bail!("case `{name}` gap_receipt `{receipt}` was not staged");
            }
            args.push(String::from("--gap-receipt"));
            args.push(staged.to_string_lossy().into_owned());
        }
        let fields = classify(&args, None, name)?;
        checks += 1;
        if assert_mapping(&fields, &expected_class, &also_expect, "mutant", name, report)? {
            matched += 1;
        }
        if let (Some(base_file), Some(base_class)) = (base_input, base_expected_class) {
            let annotations = stage.path().join(&base_file).to_string_lossy().into_owned();
            let fields =
                classify(&[String::from("--api-evidence"), annotations, steps], None, name)?;
            checks += 1;
            if assert_mapping(&fields, &base_class, &BTreeMap::new(), "base", name, report)? {
                matched += 1;
            }
        }
    }
    Ok((checks, matched))
}

#[cfg(unix)]
fn run_classifier_matrix() -> Result<()> {
    let mut report = String::from("E1 classifier mapping (real lane-termination script)\n");
    let mut checks = 0u32;
    let mut matched = 0u32;
    for (name, dir) in cases_of_kind("classifier")? {
        let manifest = read_case_manifest(&dir)?;
        let (case_checks, case_matched) = run_classifier_case(&name, &dir, &manifest, &mut report)?;
        checks += case_checks;
        matched += case_matched;
    }
    #[allow(clippy::cast_precision_loss)]
    let rate = f64::from(matched) / f64::from(checks.max(1));
    report.push_str(&format!("mapping_rate={matched}/{checks} = {rate:.4}\n"));
    println!("{report}");
    if matched != checks {
        bail!("E1 classifier mapping is imprecise:\n{report}");
    }
    Ok(())
}

#[cfg(not(unix))]
fn run_classifier_matrix() -> Result<()> {
    println!(
        "SKIP e1_classifier_mapping: the lane-termination classifier is bash-only; enforced on unix CI"
    );
    Ok(())
}

#[test]
fn e1_classifier_mapping() -> Result<()> {
    run_classifier_matrix()
}
