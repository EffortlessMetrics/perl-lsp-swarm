//! Intent/diff closeout evidence gate.

use crate::tasks::change_set::{ArtifactIdentity, ChangeSet, resolve_change_set};
use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

const DEFAULT_POLICY_PATH: &str = ".ci/policies/intent-diff-rules.toml";
const DEFAULT_RECEIPT_PATH: &str = "target/receipts/intent-diff-gate.json";

static CODE_FIX_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?i)\b(fix|bugfix|regression|activation)\b"));
static DOCS_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?i)\b(docs?|documentation|readme)\b"));
static SCAFFOLD_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?i)\b(scaffold|partial|wip|follow[- ]up)\b"));
static CLOSES_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s*#(\d+)\b"));

#[derive(Debug, Clone)]
pub struct IntentDiffGateConfig {
    pub pr: Option<u64>,
    pub fixture: Option<PathBuf>,
    pub receipt: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct PolicyFile {
    #[serde(default)]
    defaults: PolicyDefaults,
    #[serde(default)]
    issues: BTreeMap<String, IssueRule>,
    #[serde(default)]
    components: BTreeMap<String, ComponentRule>,
}

#[derive(Debug, Deserialize)]
struct PolicyDefaults {
    #[serde(default = "default_fail")]
    docs_only_code_fix: GateLevel,
    #[serde(default = "default_fail")]
    scaffold_closeout: GateLevel,
    #[serde(default = "default_warn")]
    docs_claim_code_change: GateLevel,
}

impl Default for PolicyDefaults {
    fn default() -> Self {
        Self {
            docs_only_code_fix: default_fail(),
            scaffold_closeout: default_fail(),
            docs_claim_code_change: default_warn(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct IssueRule {
    #[serde(default)]
    expected_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ComponentRule {
    #[serde(default)]
    expected_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum GateLevel {
    Warn,
    Fail,
}

fn default_fail() -> GateLevel {
    GateLevel::Fail
}

fn default_warn() -> GateLevel {
    GateLevel::Warn
}

/// Typed source-state for the PR change-set observation.
///
/// Replaces the prior implicit "whatever `gh pr view --json files` returned"
/// contract — which silently truncated to the first 100 changed files
/// (#15384). Only `Complete` may support a closeout verdict; the other
/// states must produce a `Fail` violation whenever the PR body references a
/// closing keyword (see `evaluate`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SourceState {
    /// The full changed-file set was resolved from the immutable PR
    /// change-set observation (#8042 producer), e.g. via
    /// `change_set::resolve_change_set(baseRefOid, headRefOid)`.
    Complete {
        /// Resolved base commit SHA recorded alongside the file list.
        base_sha: Option<String>,
        /// Resolved head commit SHA recorded alongside the file list.
        head_sha: Option<String>,
    },
    /// Some change-set source was obtained but the observation is known to
    /// be incomplete (e.g. `gh pr view --json files` truncated, or a
    /// `--fixture` deliberately loaded a partial prefix). The file list
    /// MAY be a useful upper-bound for advisory checks but cannot authorise
    /// a closeout verdict.
    Partial { reason: String },
    /// No change-set source could be observed (`gh pr view` failed, the
    /// PR head was not available locally for `git diff`, etc.). Cannot
    /// authorise any verdict.
    Unavailable { reason: String },
}

impl Default for SourceState {
    fn default() -> Self {
        // Fixture mode defaults to `Complete`: the fixture author asserted
        // the listed files are the full PR change set. Live `--pr` mode
        // must set the source state explicitly based on the resolved
        // observation.
        SourceState::Complete { base_sha: None, head_sha: None }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct FixtureInput {
    title: String,
    body: String,
    changed_files: Vec<String>,
    #[serde(default)]
    evidence: FixtureEvidence,
    /// Optional explicit source state — defaults to `Complete` for fixture
    /// tests where the author asserts the file list is the full PR set.
    /// Live `--pr` mode always sets this explicitly from the observation.
    #[serde(default)]
    source_state: SourceState,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct FixtureEvidence {
    #[serde(default)]
    test_updated: bool,
    #[serde(default)]
    behavior_receipt: bool,
    #[serde(default)]
    override_approved: bool,
}

#[derive(Debug)]
struct PrInput {
    title: String,
    body: String,
    changed_files: Vec<String>,
    source_state: SourceState,
    evidence: FixtureEvidence,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum Verdict {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
struct Receipt {
    claimed_component: Option<String>,
    claimed_closeout_issues: Vec<u64>,
    expected_paths: Vec<String>,
    actual_paths: Vec<String>,
    source_state: SourceState,
    evidence: ReceiptEvidence,
    verdict: Verdict,
    violations: Vec<Violation>,
}

#[derive(Debug, Clone, Serialize)]
struct ReceiptEvidence {
    target_path_touched: bool,
    test_updated: bool,
    behavior_receipt: bool,
    override_approved: bool,
}

#[derive(Debug, Clone, Serialize)]
struct Violation {
    code: String,
    level: GateLevel,
    message: String,
}

pub fn run(config: IntentDiffGateConfig) -> Result<()> {
    if config.pr.is_some() == config.fixture.is_some() {
        bail!("Provide exactly one input mode: --pr <N> or --fixture <json>");
    }

    let root = project_root()?;
    let policy = read_policy(&root.join(DEFAULT_POLICY_PATH))?;

    let input = if let Some(pr) = config.pr {
        load_pr_from_gh(pr, &root)?
    } else {
        load_fixture(
            config
                .fixture
                .as_ref()
                .ok_or_else(|| color_eyre::eyre::eyre!("missing fixture path"))?,
        )?
    };

    let receipt = evaluate(&input, &policy);
    let receipt_path = config.receipt.unwrap_or_else(|| root.join(DEFAULT_RECEIPT_PATH));
    write_receipt(&receipt_path, &receipt)?;

    println!("intent-diff-gate verdict: {:?}", receipt.verdict);
    println!("receipt: {}", receipt_path.display());
    for violation in &receipt.violations {
        println!("- [{:?}] {} ({})", violation.level, violation.message, violation.code);
    }

    if matches!(receipt.verdict, Verdict::Fail) {
        bail!("intent-diff-gate failed");
    }

    Ok(())
}

fn read_policy(path: &Path) -> Result<PolicyFile> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

fn load_fixture(path: &Path) -> Result<PrInput> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("reading fixture {}", path.display()))?;
    let fixture: FixtureInput = serde_json::from_str(&raw)
        .with_context(|| format!("parsing fixture {}", path.display()))?;
    Ok(PrInput {
        title: fixture.title,
        body: fixture.body,
        changed_files: fixture.changed_files,
        source_state: fixture.source_state,
        evidence: fixture.evidence,
    })
}

/// GitHub CLI metadata payload for `cargo xtask intent-diff-gate --pr N`.
///
/// The prior implementation (`load_pr_from_gh` pre-#15384) requested
/// `files` from `gh pr view --json` and silently truncated to the first
/// 100 entries. Per #15384 the loader must instead obtain the PR's
/// `baseRefOid`/`headRefOid` and resolve the **complete** changed-file
/// set through `change_set::resolve_change_set` (#8042 producer), which
/// uses a local `git diff <base>..<head>` and is not subject to GitHub's
/// pagination cap.
#[derive(Debug, Deserialize)]
struct GhPrMetadata {
    title: String,
    body: String,
    #[serde(rename = "baseRefOid")]
    base_ref_oid: String,
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
}

fn load_pr_from_gh(pr: u64, root: &Path) -> Result<PrInput> {
    let output = Command::new("gh")
        .args(["pr", "view", &pr.to_string(), "--json", "title,body,baseRefOid,headRefOid"])
        .output()
        .context("running gh pr view (metadata only; file list resolved via change_set)")?;

    if !output.status.success() {
        bail!("gh pr view failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }

    let parsed: GhPrMetadata =
        serde_json::from_slice(&output.stdout).context("parsing gh PR metadata payload")?;

    if parsed.base_ref_oid.is_empty() || parsed.head_ref_oid.is_empty() {
        bail!(
            "gh pr view returned empty baseRefOid/headRefOid for PR {} (base={:?}, head={:?})",
            pr,
            parsed.base_ref_oid,
            parsed.head_ref_oid
        );
    }

    // Resolve the complete PR change-set through the #8042 producer
    // (`change_set::resolve_change_set`). This is a `git diff base..head`
    // against the local repo — there is no 100-file pagination cap, so the
    // returned `changed_paths` is the complete PR set (#15384).
    //
    // The PR head must be available locally (e.g. via `gh pr checkout` or
    // a fetched refspec). When it isn't, report the source as unavailable
    // rather than failing before `evaluate`: the verdict machinery turns an
    // unavailable source into a structured `Fail` receipt instead of leaving
    // no receipt at all. We deliberately do NOT fall back to
    // `gh pr view --json files` (which would re-introduce the truncation bug).
    let resolution = resolve_change_set(
        ArtifactIdentity::CommitRange {
            base: parsed.base_ref_oid.clone(),
            head: parsed.head_ref_oid.clone(),
        },
        root,
    );
    let (changed_files, source_state) =
        pr_input_source(pr, &parsed.base_ref_oid, &parsed.head_ref_oid, resolution);

    Ok(PrInput {
        title: parsed.title,
        body: parsed.body,
        changed_files,
        source_state,
        evidence: FixtureEvidence::default(),
    })
}

/// Map a change-set resolution onto the loader's file list and source
/// state. A failed resolution (e.g. the PR head is not in the local clone)
/// yields an empty list with an `Unavailable` source rather than failing
/// before `evaluate`, so the verdict machinery still writes a structured
/// `Fail` receipt instead of leaving no receipt at all.
fn pr_input_source<E: std::fmt::Display>(
    pr: u64,
    base_ref_oid: &str,
    head_ref_oid: &str,
    resolution: Result<ChangeSet, E>,
) -> (Vec<String>, SourceState) {
    match resolution {
        Ok(changeset) => (
            changeset.changed_paths,
            SourceState::Complete { base_sha: changeset.base_sha, head_sha: changeset.head_sha },
        ),
        Err(err) => {
            let reason = format!(
                "resolving complete PR change-set for #{} via git diff {}..{} failed: {} — \
                 ensure the PR head is fetched locally (e.g. `gh pr checkout {}` \
                 or `git fetch origin pull/{}/head:pr-{}`)",
                pr, base_ref_oid, head_ref_oid, err, pr, pr, pr
            );
            (Vec::new(), SourceState::Unavailable { reason })
        }
    }
}

fn evaluate(input: &PrInput, policy: &PolicyFile) -> Receipt {
    let combined = format!("{}\n{}", input.title, input.body);
    let claimed_code_fix = regex_matches(&CODE_FIX_RE, &combined);
    let claimed_docs = regex_matches(&DOCS_RE, &input.title);
    let scaffold_claim = regex_matches(&SCAFFOLD_RE, &combined);

    let actual_paths = normalize_paths(&input.changed_files);
    let docs_only = actual_paths.iter().all(|p| is_doc_path(p));
    let production_changed = actual_paths.iter().any(|p| !is_doc_path(p) && !is_test_path(p));
    let test_updated = input.evidence.test_updated || actual_paths.iter().any(|p| is_test_path(p));

    let closing_issues = extract_closing_issues(&combined);
    let claimed_component = infer_component(&combined);

    let mut expected = BTreeSet::new();
    for issue in &closing_issues {
        if let Some(rule) = policy.issues.get(&issue.to_string()) {
            for path in &rule.expected_paths {
                expected.insert(path.clone());
            }
        }
    }
    if let Some(component) = claimed_component.as_deref()
        && let Some(rule) = policy.components.get(component)
    {
        for path in &rule.expected_paths {
            expected.insert(path.clone());
        }
    }
    let expected_paths: Vec<String> = expected.into_iter().collect();
    let target_path_touched = expected_paths
        .iter()
        .any(|needle| actual_paths.iter().any(|actual| path_matches(actual, needle)));

    let mut violations = Vec::new();

    // #15384: an incomplete or unavailable PR change-set source cannot
    // authorise any closeout verdict. We surface this before the generic
    // `closeout_without_evidence` check below so the failure mode names the
    // root cause (truncation / missing head) rather than masking it as a
    // missing-evidence verdict.
    if !closing_issues.is_empty() && !source_state_supports_closeout(&input.source_state) {
        let detail = match &input.source_state {
            SourceState::Partial { reason } => format!("partial change-set ({reason})"),
            SourceState::Unavailable { reason } => format!("unavailable change-set ({reason})"),
            SourceState::Complete { .. } => unreachable!(),
        };
        violations.push(Violation {
            code: "closeout_with_incomplete_source".to_string(),
            level: GateLevel::Fail,
            message: format!(
                "PR uses a closing keyword but the change-set source is {detail}; \
                 resolve the complete base..head diff before authorising closeout"
            ),
        });
    }

    if claimed_code_fix && docs_only {
        violations.push(Violation {
            code: "docs_only_code_fix_claim".to_string(),
            level: policy.defaults.docs_only_code_fix,
            message: "PR claims a code fix but only docs changed".to_string(),
        });
    }

    if claimed_docs && production_changed {
        violations.push(Violation {
            code: "docs_claim_with_prod_changes".to_string(),
            level: policy.defaults.docs_claim_code_change,
            message: "Docs-focused title but production code changed".to_string(),
        });
    }

    if !(closing_issues.is_empty()
        || target_path_touched
        || test_updated
        || input.evidence.behavior_receipt
        || input.evidence.override_approved)
    {
        violations.push(Violation {
            code: "closeout_without_evidence".to_string(),
            level: GateLevel::Fail,
            message: "Closeout keyword used without target-path/test/receipt/override evidence"
                .to_string(),
        });
    }

    if scaffold_claim && !closing_issues.is_empty() {
        violations.push(Violation {
            code: "scaffold_with_closing_keyword".to_string(),
            level: policy.defaults.scaffold_closeout,
            message: "Scaffold/partial PR should not use closing keywords".to_string(),
        });
    }

    let verdict = if violations.iter().any(|v| matches!(v.level, GateLevel::Fail)) {
        Verdict::Fail
    } else if violations.iter().any(|v| matches!(v.level, GateLevel::Warn)) {
        Verdict::Warn
    } else {
        Verdict::Pass
    };

    Receipt {
        claimed_component,
        claimed_closeout_issues: closing_issues,
        expected_paths,
        actual_paths,
        source_state: input.source_state.clone(),
        evidence: ReceiptEvidence {
            target_path_touched,
            test_updated,
            behavior_receipt: input.evidence.behavior_receipt,
            override_approved: input.evidence.override_approved,
        },
        verdict,
        violations,
    }
}

/// `true` iff the source state carries a complete PR change-set observation
/// (#8042 producer — full `git diff base..head`, no pagination cap).
fn source_state_supports_closeout(state: &SourceState) -> bool {
    matches!(state, SourceState::Complete { .. })
}

fn normalize_paths(paths: &[String]) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for path in paths {
        let normalized = path.replace('\\', "/");
        set.insert(normalized);
    }
    set.into_iter().collect()
}

fn extract_closing_issues(text: &str) -> Vec<u64> {
    let mut issues = BTreeSet::new();
    if let Ok(regex) = &*CLOSES_RE {
        for caps in regex.captures_iter(text) {
            if let Some(m) = caps.get(1)
                && let Ok(value) = m.as_str().parse::<u64>()
            {
                issues.insert(value);
            }
        }
    }
    issues.into_iter().collect()
}

fn regex_matches(regex: &LazyLock<Result<Regex, regex::Error>>, text: &str) -> bool {
    match &**regex {
        Ok(compiled) => compiled.is_match(text),
        Err(_) => false,
    }
}

fn infer_component(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("vs code") && lower.contains("activation") {
        return Some("vscode_activation".to_string());
    }
    None
}

fn is_doc_path(path: &str) -> bool {
    path.starts_with("docs/") || path.ends_with(".md")
}

fn is_test_path(path: &str) -> bool {
    path.contains("/test") || path.contains("/tests/")
}

fn path_matches(actual: &str, expected: &str) -> bool {
    actual == expected || actual.starts_with(expected)
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let payload = serde_json::to_string_pretty(receipt).context("serializing receipt")?;
    fs::write(path, format!("{payload}\n")).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Result<PolicyFile> {
        toml::from_str(
            r#"
[defaults]
docs_only_code_fix = "fail"
scaffold_closeout = "fail"
docs_claim_code_change = "warn"

[issues."6747"]
expected_paths = ["vscode-extension/package.json", "crates/perl-lsp-rs/tests/"]

[components.vscode_activation]
expected_paths = ["vscode-extension/package.json", "crates/perl-lsp-rs/tests/"]
"#,
        )
        .context("valid inline policy")
    }

    fn complete() -> SourceState {
        SourceState::Complete { base_sha: Some("base".into()), head_sha: Some("head".into()) }
    }

    #[test]
    fn docs_only_fix_claim_fails() -> Result<()> {
        let input = PrInput {
            title: "fix(vscode): VS Code activation bug".to_string(),
            body: "Fixes #6747".to_string(),
            changed_files: vec!["docs/notes.md".to_string()],
            source_state: complete(),
            evidence: FixtureEvidence::default(),
        };

        let receipt = evaluate(&input, &policy()?);
        assert!(matches!(receipt.verdict, Verdict::Fail));
        Ok(())
    }

    #[test]
    fn partial_refs_passes() -> Result<()> {
        let input = PrInput {
            title: "feat(ci): partial scaffold".to_string(),
            body: "Refs #6747".to_string(),
            changed_files: vec!["docs/ci/new-gate.md".to_string()],
            source_state: complete(),
            evidence: FixtureEvidence::default(),
        };

        let receipt = evaluate(&input, &policy()?);
        assert!(matches!(receipt.verdict, Verdict::Pass));
        Ok(())
    }

    #[test]
    fn closeout_with_target_path_passes() -> Result<()> {
        let input = PrInput {
            title: "fix(vscode): activation regression".to_string(),
            body: "Closes #6747".to_string(),
            changed_files: vec!["vscode-extension/package.json".to_string()],
            source_state: complete(),
            evidence: FixtureEvidence::default(),
        };

        let receipt = evaluate(&input, &policy()?);
        assert!(matches!(receipt.verdict, Verdict::Pass));
        Ok(())
    }

    /// #15384 regression guard: when the change-set source is `Partial`
    /// (e.g. `gh pr view --json files` truncated to its first 100 entries
    /// and the production code lives at file 101), a closing keyword MUST
    /// fail the verdict rather than silently authorising closeout against
    /// the truncated prefix.
    #[test]
    fn closeout_with_partial_source_fails() -> Result<()> {
        let input = PrInput {
            title: "fix(vscode): activation regression".to_string(),
            body: "Closes #6747".to_string(),
            // First 100 entries are docs — but file 101 is the actual
            // production change. The truncated `files` prefix makes the PR
            // look docs-only and the target path `vscode-extension/package.json`
            // is missing entirely from this observation.
            changed_files: (0..100).map(|i| format!("docs/page-{i:03}.md")).collect(),
            source_state: SourceState::Partial {
                reason: "gh pr view --json files truncated at 100 entries".into(),
            },
            evidence: FixtureEvidence::default(),
        };

        let receipt = evaluate(&input, &policy()?);
        assert!(matches!(receipt.verdict, Verdict::Fail));
        assert!(
            receipt.violations.iter().any(|v| v.code == "closeout_with_incomplete_source"),
            "expected closeout_with_incomplete_source violation, got: {:?}",
            receipt.violations
        );
        // The new violation must be the load-bearing one — the truncated
        // prefix cannot satisfy the generic target-path/test/receipt rule
        // either, so both `closeout_with_incomplete_source` and
        // `closeout_without_evidence` may fire; the source-state one must
        // always be present.
        Ok(())
    }

    /// #15384 mirror of the above for `Unavailable` (e.g. `gh pr view`
    /// failed or the PR head is not in the local clone).
    #[test]
    fn closeout_with_unavailable_source_fails() -> Result<()> {
        let input = PrInput {
            title: "fix(vscode): activation regression".to_string(),
            body: "Fixes #6747".to_string(),
            changed_files: vec![],
            source_state: SourceState::Unavailable {
                reason: "gh pr view failed: PR not found".into(),
            },
            evidence: FixtureEvidence::default(),
        };

        let receipt = evaluate(&input, &policy()?);
        assert!(matches!(receipt.verdict, Verdict::Fail));
        assert!(
            receipt.violations.iter().any(|v| v.code == "closeout_with_incomplete_source"),
            "expected closeout_with_incomplete_source violation, got: {:?}",
            receipt.violations
        );
        Ok(())
    }

    /// #15384 guard: a PR with no closing keyword is unaffected by the
    /// new source-state rule — an incomplete change-set observation may
    /// still drive advisory checks (docs-only claim, etc.) without
    /// forcing a `Fail`.
    #[test]
    fn partial_source_without_closeout_does_not_force_fail() -> Result<()> {
        let input = PrInput {
            title: "feat(ci): scaffold new gate".to_string(),
            body: "Refs #6747 — partial work in progress.".to_string(),
            changed_files: vec!["docs/ci/new-gate.md".to_string()],
            source_state: SourceState::Partial {
                reason: "gh pr view --json files truncated at 100 entries".into(),
            },
            evidence: FixtureEvidence::default(),
        };

        let receipt = evaluate(&input, &policy()?);
        assert!(
            !matches!(receipt.verdict, Verdict::Fail),
            "partial source without closeout must not force Fail; got: {:?}",
            receipt.violations
        );
        Ok(())
    }

    /// #15384 guard: the typed `SourceState` is serialised into the
    /// receipt so downstream consumers can distinguish a `Complete`
    /// observation from a truncated `Partial` one (e.g. for advisory
    /// dashboards, or to fail-closed at a higher gate).
    #[test]
    fn receipt_carries_source_state() -> Result<()> {
        let input = PrInput {
            title: "feat(ci): trivial".to_string(),
            body: "Refs #6747".to_string(),
            changed_files: vec!["docs/notes.md".to_string()],
            source_state: SourceState::Complete {
                base_sha: Some("deadbeef".into()),
                head_sha: Some("feedface".into()),
            },
            evidence: FixtureEvidence::default(),
        };

        let receipt = evaluate(&input, &policy()?);
        let payload = serde_json::to_value(&receipt).context("serialise receipt")?;
        let source = payload
            .get("source_state")
            .ok_or_else(|| color_eyre::eyre::eyre!("receipt.source_state missing"))?;
        let kind = source
            .get("kind")
            .and_then(|k| k.as_str())
            .ok_or_else(|| color_eyre::eyre::eyre!("receipt.source_state.kind missing"))?;
        assert_eq!(kind, "complete");
        let base_sha = source.get("base_sha").and_then(|k| k.as_str());
        let head_sha = source.get("head_sha").and_then(|k| k.as_str());
        assert_eq!(base_sha, Some("deadbeef"));
        assert_eq!(head_sha, Some("feedface"));
        Ok(())
    }

    /// The live `gh pr view --json title,body,baseRefOid,headRefOid`
    /// payload uses camelCase keys; without renames every `--pr`
    /// invocation fails deserialization with a missing-field error.
    #[test]
    fn gh_pr_metadata_deserializes_camel_case_keys() -> Result<()> {
        let raw = serde_json::json!({
            "title": "fix(intent-diff-gate): bind closeout",
            "body": "Closes #15384",
            "baseRefOid": "aaa",
            "headRefOid": "bbb",
        });
        let parsed: GhPrMetadata =
            serde_json::from_value(raw).context("camelCase gh payload must deserialize")?;
        assert_eq!(parsed.base_ref_oid, "aaa");
        assert_eq!(parsed.head_ref_oid, "bbb");
        Ok(())
    }

    /// A failed change-set resolution (e.g. PR head missing locally)
    /// maps to an empty file list with an `Unavailable` source so
    /// `evaluate` still writes a structured `Fail` receipt.
    #[test]
    fn failed_resolution_maps_to_unavailable_source() -> Result<()> {
        let (files, state) = pr_input_source(
            15855,
            "aaa",
            "bbb",
            Err::<ChangeSet, _>("head bbb not in local clone"),
        );
        assert!(files.is_empty());
        match state {
            SourceState::Unavailable { reason } => {
                assert!(
                    reason.contains("15855") && reason.contains("bbb"),
                    "reason must name the PR and head, got: {reason}"
                );
            }
            other => panic!("expected Unavailable, got: {other:?}"),
        }
        Ok(())
    }
}
