//! `cargo xtask sync-divergence check` — fail closed on unclassified target commits.

use color_eyre::eyre::{Context, Report, Result, eyre};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const CLASSIFICATIONS: [&str; 5] = [
    "port_to_swarm",
    "already_equivalent_in_swarm",
    "superseded_by_newer_architecture",
    "deliberately_abandoned",
    "release_lineage_only",
];

const LEDGER_SCHEMA_VERSION: u32 = 2;
const RECEIPT_SCHEMA_VERSION: u32 = 2;

/// Arguments for the sync-divergence preflight.
pub struct CheckConfig {
    /// Exact swarm source ref; resolved as the patch-equivalence upstream.
    pub source: String,
    /// Completed reconciliation boundary ref; resolved as the exclusive history floor.
    pub boundary: String,
    /// Release-repo target ref (normally the release repository head).
    pub target: String,
    /// Machine-readable reconciliation ledger.
    pub ledger: PathBuf,
    /// Output source-sync receipt JSON.
    pub receipt: PathBuf,
    /// Repository to run against; defaults to the process working directory.
    pub working_directory: Option<PathBuf>,
}

#[derive(Debug)]
struct Subjects {
    source: SubjectState,
    boundary: SubjectState,
    target: SubjectState,
}

#[derive(Debug)]
struct SubjectState {
    input: String,
    commit: Option<String>,
}

impl Subjects {
    fn from_config(config: &CheckConfig) -> Self {
        let state = |input: &String| SubjectState { input: input.clone(), commit: None };
        Self {
            source: state(&config.source),
            boundary: state(&config.boundary),
            target: state(&config.target),
        }
    }
}

impl From<&Subjects> for ReceiptSubjects {
    fn from(subjects: &Subjects) -> Self {
        let state = |subject: &SubjectState| ReceiptSubject {
            input: subject.input.clone(),
            commit: subject.commit.clone(),
        };
        Self {
            source: state(&subjects.source),
            boundary: state(&subjects.boundary),
            target: state(&subjects.target),
        }
    }
}

#[derive(Debug)]
struct ResolvedShas {
    source: String,
    boundary: String,
    target: String,
}

#[derive(Debug, Deserialize)]
struct Ledger {
    schema_version: u32,
    source: String,
    boundary: String,
    target: String,
    entries: Vec<LedgerEntry>,
}

#[derive(Debug, Deserialize)]
struct LedgerEntry {
    commit: String,
    subject: String,
    classification: String,
    evidence: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: u32,
    subjects: ReceiptSubjects,
    ledger: String,
    target_unique_commits: Vec<ReceiptCommit>,
    excluded_merge_commits: Vec<String>,
    excluded_merge_ancestry: Vec<ExcludedMerge>,
    excluded_release_lineage_commits: Vec<String>,
    accepted_commits: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ReceiptSubjects {
    source: ReceiptSubject,
    boundary: ReceiptSubject,
    target: ReceiptSubject,
}

#[derive(Debug, Serialize)]
struct ReceiptSubject {
    input: String,
    commit: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReceiptCommit {
    commit: String,
    subject: String,
    classification: String,
}

#[derive(Debug, Serialize)]
struct ExcludedMerge {
    commit: String,
    subject: String,
    parents: Vec<String>,
}

#[derive(Debug)]
struct CherryCommit {
    commit: String,
    subject: String,
    parents: Vec<String>,
}

impl CherryCommit {
    fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }
}

/// Run the preflight and write a receipt even when validation fails.
pub fn check(config: CheckConfig) -> Result<()> {
    let directory = config.working_directory.as_deref();
    let mut subjects = Subjects::from_config(&config);

    let shas = match resolve_subjects(&mut subjects, directory) {
        Ok(shas) => shas,
        Err(error) => return fail_with_receipt(&config, &subjects, error),
    };
    if let Err(error) = ensure_boundary_bounds_target(&shas, directory) {
        return fail_with_receipt(&config, &subjects, error);
    }

    let ledger = match load_ledger(&config.ledger) {
        Ok(ledger) => ledger,
        Err(error) => return fail_with_receipt(&config, &subjects, error),
    };
    if let Err(error) = validate_ledger_identity(&ledger, &shas) {
        return fail_with_receipt(&config, &subjects, error);
    }

    let target_unique = match comparison_population(&shas, directory) {
        Ok(commits) => commits,
        Err(error) => return fail_with_receipt(&config, &subjects, error),
    };
    let excluded_merges = match excluded_merge_ancestry(&shas, directory) {
        Ok(merges) => merges,
        Err(error) => return fail_with_receipt(&config, &subjects, error),
    };

    let (receipt, errors) = reconcile(&config, &ledger, &target_unique, excluded_merges, &subjects);
    write_receipt(&config.receipt, &receipt)?;

    if errors.is_empty() {
        println!(
            "sync-divergence: checked {} target-unique non-merge commit(s)",
            receipt.target_unique_commits.len()
        );
        Ok(())
    } else {
        Err(eyre!(
            "sync-divergence preflight failed with {} error(s); see {}",
            errors.len(),
            config.receipt.display()
        ))
    }
}

fn reconcile(
    config: &CheckConfig,
    ledger: &Ledger,
    target_unique: &[CherryCommit],
    excluded_merges: Vec<ExcludedMerge>,
    subjects: &Subjects,
) -> (Receipt, Vec<String>) {
    let mut errors = Vec::new();
    let mut entries = BTreeMap::new();
    for entry in &ledger.entries {
        if entries.contains_key(entry.commit.as_str()) {
            errors.push(format!("commit {} appears more than once", entry.commit));
        } else {
            entries.insert(entry.commit.as_str(), entry);
        }
    }

    let mut ordered = BTreeMap::new();
    for commit in target_unique {
        ordered.insert(commit.commit.as_str(), commit);
    }

    let mut seen = BTreeSet::new();
    let mut receipt_commits = Vec::new();
    let mut excluded_release_lineage_commits = Vec::new();
    let mut accepted_commits = Vec::new();

    for commit in ordered.values() {
        if commit.is_merge() {
            continue;
        }

        let Some(entry) = entries.get(commit.commit.as_str()) else {
            errors.push(format!(
                "target-unique commit {} is missing from the reconciliation ledger",
                commit.commit
            ));
            continue;
        };

        seen.insert(commit.commit.as_str());
        if normalize_subject(&entry.subject) != normalize_subject(&commit.subject) {
            errors.push(format!(
                "ledger subject for {} does not match Git: ledger=`{}` Git=`{}`",
                commit.commit, entry.subject, commit.subject
            ));
        }
        receipt_commits.push(ReceiptCommit {
            commit: commit.commit.clone(),
            subject: commit.subject.clone(),
            classification: entry.classification.clone(),
        });

        if !CLASSIFICATIONS.contains(&entry.classification.as_str()) {
            errors.push(format!(
                "commit {} has invalid classification `{}`",
                commit.commit, entry.classification
            ));
            continue;
        }

        if entry.classification == "release_lineage_only" {
            excluded_release_lineage_commits.push(commit.commit.clone());
        } else {
            accepted_commits.push(commit.commit.clone());
        }
    }

    for entry in &ledger.entries {
        if !has_evidence(entry) {
            errors.push(format!("commit {} has no evidence", entry.commit));
        }
        if !seen.contains(entry.commit.as_str()) {
            errors.push(format!(
                "ledger commit {} is not a non-merge target-unique commit",
                entry.commit
            ));
        }
    }

    let excluded_merge_commits =
        excluded_merges.iter().map(|merge| merge.commit.clone()).collect::<Vec<_>>();
    let receipt = Receipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        subjects: ReceiptSubjects::from(subjects),
        ledger: config.ledger.display().to_string(),
        target_unique_commits: receipt_commits,
        excluded_merge_commits,
        excluded_merge_ancestry: excluded_merges,
        excluded_release_lineage_commits,
        accepted_commits,
        errors: errors.clone(),
    };
    (receipt, errors)
}

fn has_evidence(entry: &LedgerEntry) -> bool {
    entry.evidence.iter().any(|evidence| !evidence.trim().is_empty())
}

fn load_ledger(path: &Path) -> Result<Ledger> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("reading reconciliation ledger {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parsing reconciliation ledger {}", path.display()))?;
    let Some(version) = value.get("schema_version").and_then(serde_json::Value::as_u64) else {
        return Err(eyre!(
            "reconciliation ledger {} has no numeric schema_version",
            path.display()
        ));
    };
    if version != u64::from(LEDGER_SCHEMA_VERSION) {
        return Err(eyre!(
            "unsupported reconciliation ledger schema version {}; version {LEDGER_SCHEMA_VERSION} requires exact source, boundary, and target object ids",
            version
        ));
    }
    let ledger: Ledger = serde_json::from_value(value)
        .with_context(|| format!("parsing reconciliation ledger {}", path.display()))?;
    Ok(ledger)
}

fn validate_ledger_identity(ledger: &Ledger, shas: &ResolvedShas) -> Result<()> {
    if ledger.schema_version != LEDGER_SCHEMA_VERSION {
        return Err(eyre!(
            "unsupported reconciliation ledger schema version {}; version {LEDGER_SCHEMA_VERSION} requires exact source, boundary, and target object ids",
            ledger.schema_version
        ));
    }
    if ledger.source != shas.source
        || ledger.boundary != shas.boundary
        || ledger.target != shas.target
    {
        return Err(eyre!(
            "reconciliation ledger identity does not match the resolved subjects: \
             expected source={} boundary={} target={}",
            shas.source,
            shas.boundary,
            shas.target
        ));
    }
    Ok(())
}

fn resolve_subjects(subjects: &mut Subjects, directory: Option<&Path>) -> Result<ResolvedShas> {
    let source = resolve_subject("source", &subjects.source, directory)?;
    subjects.source.commit = Some(source.clone());
    let boundary = resolve_subject("boundary", &subjects.boundary, directory)?;
    subjects.boundary.commit = Some(boundary.clone());
    let target = resolve_subject("target", &subjects.target, directory)?;
    subjects.target.commit = Some(target.clone());
    Ok(ResolvedShas { source, boundary, target })
}

fn resolve_subject(label: &str, state: &SubjectState, directory: Option<&Path>) -> Result<String> {
    validate_subject_syntax(label, &state.input)?;
    let peeled = format!("{}^{{commit}}", state.input);
    match git_output_in(
        ["rev-parse", "--verify", "--quiet", "--end-of-options", &peeled],
        directory,
    ) {
        Ok(output) => {
            let resolved = output.trim().to_string();
            if resolved.is_empty() {
                return Err(unresolved_subject(label, state, directory));
            }
            Ok(resolved)
        }
        Err(_) => Err(unresolved_subject(label, state, directory)),
    }
}

fn unresolved_subject(label: &str, state: &SubjectState, directory: Option<&Path>) -> Report {
    let peeled = format!("{}^{{commit}}", state.input);
    let stderr = git_stderr_in(["rev-parse", "--verify", "--end-of-options", &peeled], directory)
        .unwrap_or_default();
    if stderr.contains("ambiguous") {
        return eyre!(
            "{label} ref `{}` was ambiguous; pass a full 40-hex object id or an unambiguous ref name",
            state.input
        );
    }
    eyre!("{label} ref `{}` did not resolve to a commit", state.input)
}

fn ensure_boundary_bounds_target(shas: &ResolvedShas, directory: Option<&Path>) -> Result<()> {
    match git_status_in(["merge-base", "--is-ancestor", &shas.boundary, &shas.target], directory)? {
        0 => Ok(()),
        1 => Err(eyre!(
            "reversed subjects: boundary {} is not an ancestor of target {}; the completed reconciliation boundary must bound the target history",
            shas.boundary,
            shas.target
        )),
        code => Err(eyre!("git merge-base --is-ancestor exited with status {code}")),
    }
}

fn comparison_population(
    shas: &ResolvedShas,
    directory: Option<&Path>,
) -> Result<Vec<CherryCommit>> {
    let output = git_output_in(["cherry", &shas.source, &shas.target, &shas.boundary], directory)?;
    let mut commits = BTreeMap::new();
    for commit in parse_cherry_plus_lines(&output) {
        let commit = commit.to_string();
        let subject =
            git_output_in(["show", "-s", "--format=%s", &commit], directory)?.trim().to_string();
        let parents = parse_parent_shas(&git_output_in(
            ["rev-list", "--parents", "-n", "1", &commit],
            directory,
        )?);
        commits.insert(commit.clone(), CherryCommit { commit, subject, parents });
    }
    Ok(commits.into_values().collect())
}

fn excluded_merge_ancestry(
    shas: &ResolvedShas,
    directory: Option<&Path>,
) -> Result<Vec<ExcludedMerge>> {
    let range = format!("{}..{}", shas.boundary, shas.target);
    let output =
        git_output_in(["rev-list", "--topo-order", "--merges", "--parents", &range], directory)?;
    let mut merges = BTreeMap::new();
    for line in output.lines() {
        let mut tokens = line.split_whitespace();
        let Some(commit) = tokens.next() else { continue };
        let parents: Vec<String> = tokens.map(str::to_string).collect();
        if parents.len() < 2 {
            continue;
        }
        let subject =
            git_output_in(["show", "-s", "--format=%s", commit], directory)?.trim().to_string();
        merges.insert(
            commit.to_string(),
            ExcludedMerge { commit: commit.to_string(), subject, parents },
        );
    }
    Ok(merges.into_values().collect())
}

fn parse_cherry_plus_lines(output: &str) -> Vec<&str> {
    output.lines().filter_map(|line| line.strip_prefix("+ ").map(str::trim)).collect()
}

fn parse_parent_shas(rev_list_line: &str) -> Vec<String> {
    rev_list_line.split_whitespace().skip(1).map(str::to_string).collect()
}

fn normalize_subject(subject: &str) -> String {
    subject.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn validate_subject_syntax(label: &str, reference: &str) -> Result<()> {
    if reference.is_empty() || reference.trim().is_empty() {
        return Err(eyre!("{label} subject was missing; pass a resolvable commit-ish"));
    }
    if reference.trim() != reference {
        return Err(eyre!("{label} subject `{reference}` was malformed by surrounding whitespace"));
    }
    if reference.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(eyre!(
            "{label} subject `{reference}` was malformed by embedded whitespace or control characters"
        ));
    }
    if reference.starts_with('-') {
        return Err(eyre!(
            "{label} subject `{reference}` was malformed; refs must not start with '-'"
        ));
    }
    if reference.contains("..") {
        return Err(eyre!(
            "{label} subject `{reference}` was malformed; range expressions are not single commits"
        ));
    }
    Ok(())
}

fn fail_with_receipt(config: &CheckConfig, subjects: &Subjects, error: Report) -> Result<()> {
    let message = format!("{error:#}");
    let receipt = Receipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        subjects: ReceiptSubjects::from(subjects),
        ledger: config.ledger.display().to_string(),
        target_unique_commits: Vec::new(),
        excluded_merge_commits: Vec::new(),
        excluded_merge_ancestry: Vec::new(),
        excluded_release_lineage_commits: Vec::new(),
        accepted_commits: Vec::new(),
        errors: vec![message.clone()],
    };
    write_receipt(&config.receipt, &receipt)?;
    Err(eyre!(
        "sync-divergence preflight failed before comparison: {message}; see {}",
        config.receipt.display()
    ))
}

fn git_output_in<const N: usize>(args: [&str; N], directory: Option<&Path>) -> Result<String> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let output = command.output().context("running git for sync-divergence preflight")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!("git command failed: {stderr}"));
    }
    String::from_utf8(output.stdout).context("git output was not valid UTF-8")
}

fn git_stderr_in<const N: usize>(args: [&str; N], _directory: Option<&Path>) -> Result<String> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(directory) = _directory {
        command.current_dir(directory);
    }
    let output = command.output().context("running git for sync-divergence preflight")?;
    Ok(String::from_utf8_lossy(&output.stderr).to_string())
}

fn git_status_in<const N: usize>(args: [&str; N], directory: Option<&Path>) -> Result<i32> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let output = command.output().context("running git for sync-divergence preflight")?;
    Ok(output.status.code().unwrap_or(-1))
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating receipt directory {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(receipt).context("serializing sync receipt")?;
    fs::write(path, format!("{content}\n"))
        .with_context(|| format!("writing sync receipt {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plus_lines_are_target_unique() -> Result<()> {
        let output = "+ abc\n- def\n  ghi\n";
        assert_eq!(parse_cherry_plus_lines(output), vec!["abc"]);
        Ok(())
    }

    #[test]
    fn parent_shas_skip_the_commit_itself() {
        assert_eq!(parse_parent_shas("abc123 def456 789aaa"), vec!["def456", "789aaa"]);
    }

    #[test]
    fn two_parents_mark_a_merge() {
        let merge = CherryCommit {
            commit: "m".into(),
            subject: "merge".into(),
            parents: vec!["a".into(), "b".into()],
        };
        let regular =
            CherryCommit { commit: "r".into(), subject: "row".into(), parents: vec!["a".into()] };
        assert!(merge.is_merge());
        assert!(!regular.is_merge());
    }

    #[test]
    fn malformed_subjects_fail_closed() -> Result<()> {
        for input in ["", "   ", " HEAD", "HEAD ", "a..b", "-rfw", "two words"] {
            assert!(
                validate_subject_syntax("source", input).is_err(),
                "`{input}` must fail syntax validation"
            );
        }
        validate_subject_syntax("source", "refs/heads/main")?;
        validate_subject_syntax("source", "0123456789abcdef0123456789abcdef01234567")?;
        Ok(())
    }

    #[test]
    fn unresolved_refs_report_missing_not_ambiguous() -> Result<()> {
        let directory = init_fixture_repo()?;
        let state = SubjectState { input: "refs/heads/nope-7968".to_string(), commit: None };
        let error = match resolve_subject("source", &state, Some(directory.path())) {
            Err(error) => error,
            Ok(resolved) => return Err(eyre!("a missing ref must not resolve to {resolved}")),
        };
        let message = format!("{error:#}");
        assert!(message.contains("did not resolve to a commit"), "{message}");
        assert!(!message.contains("ambiguous"), "{message}");
        Ok(())
    }

    #[test]
    fn classifications_are_explicit() {
        assert!(CLASSIFICATIONS.contains(&"port_to_swarm"));
        assert!(CLASSIFICATIONS.contains(&"release_lineage_only"));
        assert!(!CLASSIFICATIONS.contains(&"unclassified"));
    }

    #[test]
    fn subject_comparison_normalizes_whitespace() {
        assert_eq!(
            normalize_subject("fix:   preserve  the subject"),
            normalize_subject("fix: preserve the subject")
        );
    }

    /// A release patch already represented in swarm is suppressed while the
    /// exact source drives patch equivalence and the boundary only floors history.
    #[test]
    fn represented_release_patches_are_not_target_unique() -> Result<()> {
        let fixture = diverged_fixture()?;
        let shas =
            resolved(fixture.swarm_tip.clone(), fixture.base.clone(), fixture.release_tip.clone());
        let rows = comparison_population(&shas, Some(fixture.directory.path()))?;
        let commits: Vec<&str> = rows.iter().map(|row| row.commit.as_str()).collect();
        let mut expected = vec![fixture.floor_tip.clone(), fixture.release_tip.clone()];
        expected.sort();
        assert_eq!(commits, expected, "the cherry-picked patch must be suppressed");
        assert!(!commits.contains(&fixture.picked.as_str()));
        assert!(rows.iter().all(|row| !row.is_merge()));
        Ok(())
    }

    /// The boundary excludes older history regardless of patch equivalence;
    /// the source still suppresses represented patches inside the window.
    #[test]
    fn boundary_limits_history_independent_of_equivalence() -> Result<()> {
        let fixture = diverged_fixture()?;
        let shas = resolved(
            fixture.swarm_tip.clone(),
            fixture.floor_tip.clone(),
            fixture.release_tip.clone(),
        );
        let rows = comparison_population(&shas, Some(fixture.directory.path()))?;
        let commits: Vec<&str> = rows.iter().map(|row| row.commit.as_str()).collect();
        assert_eq!(commits, vec![fixture.release_tip.as_str()]);
        Ok(())
    }

    #[test]
    fn merge_ancestry_is_enumerated_with_parents_and_respects_the_floor() -> Result<()> {
        let directory = init_fixture_repo()?;
        let path = directory.path();
        commit_file(path, "a.txt", "a\n", "base")?;
        let base = rev_parse(path, "main")?;
        run_git_fixture(path, &["checkout", "--quiet", "-b", "feature"])?;
        commit_file(path, "f.txt", "f\n", "feature")?;
        let feature = rev_parse(path, "feature")?;
        run_git_fixture(path, &["checkout", "--quiet", "main"])?;
        commit_file(path, "s.txt", "s\n", "side")?;
        let side = rev_parse(path, "main")?;
        run_git_fixture(path, &["merge", "--quiet", "--no-ff", "-m", "merge feature", "feature"])?;
        let merge = rev_parse(path, "main")?;

        let shas = resolved(base.clone(), base.clone(), merge.clone());
        let merges = excluded_merge_ancestry(&shas, Some(path))?;
        assert_eq!(merges.len(), 1);
        assert_eq!(merges[0].commit, merge);
        assert_eq!(merges[0].parents, vec![side, feature]);
        assert_eq!(merges[0].subject, "merge feature");

        let floored = resolved(base.clone(), merge.clone(), merge.clone());
        assert!(excluded_merge_ancestry(&floored, Some(path))?.is_empty());
        Ok(())
    }

    #[test]
    fn success_receipt_records_resolved_identity() -> Result<()> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        fs::write(
            path.join("ledger.json"),
            format!(
                r#"{{
  "schema_version": 2,
  "source": "{}",
  "boundary": "{}",
  "target": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "classification": "port_to_swarm", "evidence": ["evidence r0"]}},
    {{"commit": "{}", "subject": "release r2", "classification": "release_lineage_only", "evidence": ["evidence r2"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture.floor_tip,
                fixture.release_tip
            ),
        )?;
        let config = CheckConfig {
            source: fixture.swarm_tip.clone(),
            boundary: fixture.base.clone(),
            target: fixture.release_tip.clone(),
            ledger: path.join("ledger.json"),
            receipt: path.join("receipt.json"),
            working_directory: Some(path.to_path_buf()),
        };
        check(config)?;
        let receipt: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path.join("receipt.json"))?)?;
        assert_eq!(receipt["schema_version"], RECEIPT_SCHEMA_VERSION);
        assert_eq!(receipt["subjects"]["source"]["commit"], fixture.swarm_tip.as_str());
        assert_eq!(receipt["subjects"]["boundary"]["commit"], fixture.base.as_str());
        assert_eq!(receipt["subjects"]["target"]["commit"], fixture.release_tip.as_str());
        assert_eq!(receipt["target_unique_commits"].as_array().map(Vec::len), Some(2));
        assert_eq!(receipt["accepted_commits"].as_array().map(Vec::len), Some(1));
        assert_eq!(receipt["excluded_release_lineage_commits"].as_array().map(Vec::len), Some(1));
        assert_eq!(receipt["excluded_merge_commits"].as_array().map(Vec::len), Some(0));
        assert_eq!(receipt["errors"].as_array().map(Vec::len), Some(0));
        Ok(())
    }

    #[test]
    fn reversed_subjects_fail_closed_with_a_receipt() -> Result<()> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        let config = CheckConfig {
            source: fixture.swarm_tip.clone(),
            boundary: fixture.swarm_tip.clone(),
            target: fixture.release_tip.clone(),
            ledger: path.join("ledger.json"),
            receipt: path.join("receipt.json"),
            working_directory: Some(path.to_path_buf()),
        };
        let error = match check(config) {
            Err(error) => error,
            Ok(()) => return Err(eyre!("reversed subjects must fail closed")),
        };
        let message = format!("{error:#}");
        assert!(message.contains("reversed subjects"), "{message}");
        let receipt: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path.join("receipt.json"))?)?;
        assert_eq!(receipt["subjects"]["source"]["commit"], fixture.swarm_tip.as_str());
        assert_eq!(receipt["subjects"]["boundary"]["commit"], fixture.swarm_tip.as_str());
        assert_eq!(receipt["subjects"]["target"]["commit"], fixture.release_tip.as_str());
        assert_eq!(receipt["target_unique_commits"].as_array().map(Vec::len), Some(0));
        assert_eq!(receipt["errors"].as_array().map(Vec::len), Some(1));
        Ok(())
    }

    #[test]
    fn missing_subject_fails_closed_before_ledger_load() -> Result<()> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        let config = CheckConfig {
            source: "refs/heads/nope-7968".to_string(),
            boundary: fixture.base.clone(),
            target: fixture.release_tip.clone(),
            ledger: path.join("missing.json"),
            receipt: path.join("receipt.json"),
            working_directory: Some(path.to_path_buf()),
        };
        assert!(check(config).is_err());
        let receipt: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path.join("receipt.json"))?)?;
        assert_eq!(receipt["subjects"]["source"]["commit"], serde_json::Value::Null);
        assert_eq!(receipt["subjects"]["boundary"]["commit"], serde_json::Value::Null);
        assert_eq!(receipt["subjects"]["target"]["input"], fixture.release_tip.as_str());
        assert_eq!(receipt["errors"].as_array().map(Vec::len), Some(1));
        Ok(())
    }

    #[test]
    fn v1_ledgers_fail_closed_on_schema_version() -> Result<()> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        fs::write(
            path.join("ledger.json"),
            r#"{"schema_version":1,"base":"x","source":"y","target":"z","entries":[]}"#,
        )?;
        let config = CheckConfig {
            source: fixture.swarm_tip.clone(),
            boundary: fixture.base.clone(),
            target: fixture.release_tip.clone(),
            ledger: path.join("ledger.json"),
            receipt: path.join("receipt.json"),
            working_directory: Some(path.to_path_buf()),
        };
        let error = match check(config) {
            Err(error) => error,
            Ok(()) => return Err(eyre!("a v1 ledger must fail closed")),
        };
        let message = format!("{error:#}");
        assert!(
            message.contains("unsupported reconciliation ledger schema version 1"),
            "{message}"
        );
        Ok(())
    }

    #[test]
    fn ledger_identity_must_match_resolved_shas() -> Result<()> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        fs::write(
            path.join("ledger.json"),
            format!(
                r#"{{"schema_version":2,"source":"{}","boundary":"{}","target":"{}","entries":[]}}"#,
                fixture.swarm_tip, "0123456789abcdef0123456789abcdef01234567", fixture.release_tip
            ),
        )?;
        let config = CheckConfig {
            source: fixture.swarm_tip.clone(),
            boundary: fixture.base.clone(),
            target: fixture.release_tip.clone(),
            ledger: path.join("ledger.json"),
            receipt: path.join("receipt.json"),
            working_directory: Some(path.to_path_buf()),
        };
        let error = match check(config) {
            Err(error) => error,
            Ok(()) => return Err(eyre!("an identity mismatch must fail closed")),
        };
        let message = format!("{error:#}");
        assert!(message.contains("does not match the resolved subjects"), "{message}");
        Ok(())
    }

    #[test]
    fn early_failures_write_a_v2_receipt() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let receipt_path = directory.path().join("receipt.json");
        let config = CheckConfig {
            source: "HEAD".to_string(),
            boundary: "HEAD".to_string(),
            target: "HEAD".to_string(),
            ledger: directory.path().join("missing.json"),
            receipt: receipt_path.clone(),
            working_directory: None,
        };
        assert!(check(config).is_err());
        let content = fs::read_to_string(receipt_path)?;
        let receipt: serde_json::Value = serde_json::from_str(&content)?;
        assert_eq!(receipt["schema_version"], RECEIPT_SCHEMA_VERSION);
        assert_eq!(receipt["subjects"]["target"]["input"], "HEAD");
        assert_eq!(receipt["target_unique_commits"].as_array().map(Vec::len), Some(0));
        assert_eq!(receipt["errors"].as_array().map(Vec::len), Some(1));
        Ok(())
    }

    #[test]
    fn reconciliation_rejects_duplicate_rows_and_skips_merges() -> Result<()> {
        let config = CheckConfig {
            source: "source".to_string(),
            boundary: "boundary".to_string(),
            target: "target".to_string(),
            ledger: PathBuf::from("ledger.json"),
            receipt: PathBuf::from("receipt.json"),
            working_directory: None,
        };
        let ledger = Ledger {
            schema_version: LEDGER_SCHEMA_VERSION,
            source: config.source.clone(),
            boundary: config.boundary.clone(),
            target: config.target.clone(),
            entries: vec![
                LedgerEntry {
                    commit: "abc".to_string(),
                    subject: "different subject".to_string(),
                    classification: "not-valid".to_string(),
                    evidence: vec!["   ".to_string()],
                },
                LedgerEntry {
                    commit: "abc".to_string(),
                    subject: "subject".to_string(),
                    classification: "port_to_swarm".to_string(),
                    evidence: vec!["valid evidence".to_string()],
                },
            ],
        };
        let target_unique = vec![
            CherryCommit {
                commit: "abc".to_string(),
                subject: "subject".to_string(),
                parents: vec!["parent".to_string()],
            },
            CherryCommit {
                commit: "zzz".to_string(),
                subject: "merge subject".to_string(),
                parents: vec!["first".to_string(), "second".to_string()],
            },
        ];
        let excluded_merges = vec![ExcludedMerge {
            commit: "zzz".to_string(),
            subject: "merge subject".to_string(),
            parents: vec!["first".to_string(), "second".to_string()],
        }];
        let mut subjects = Subjects::from_config(&config);
        subjects.source.commit = Some("source-sha".to_string());
        subjects.boundary.commit = Some("boundary-sha".to_string());
        subjects.target.commit = Some("target-sha".to_string());

        let (receipt, errors) =
            reconcile(&config, &ledger, &target_unique, excluded_merges, &subjects);
        assert_eq!(receipt.target_unique_commits.len(), 1);
        assert_eq!(receipt.excluded_merge_commits, vec!["zzz"]);
        assert_eq!(receipt.excluded_merge_ancestry.len(), 1);
        assert_eq!(receipt.subjects.source.commit.as_deref(), Some("source-sha"));
        assert!(receipt.accepted_commits.is_empty());
        assert!(errors.iter().any(|error| error.contains("appears more than once")));
        assert!(errors.iter().any(|error| error.contains("invalid classification")));
        assert!(errors.iter().any(|error| error.contains("has no evidence")));
        assert!(errors.iter().any(|error| error.contains("does not match Git")));
        Ok(())
    }

    struct DivergedFixture {
        directory: tempfile::TempDir,
        base: String,
        swarm_tip: String,
        floor_tip: String,
        picked: String,
        release_tip: String,
    }

    fn init_fixture_repo() -> Result<tempfile::TempDir> {
        let directory = tempfile::tempdir()?;
        run_git_fixture(directory.path(), &["init", "--quiet", "--initial-branch=main"])?;
        run_git_fixture(directory.path(), &["config", "user.email", "test@example.com"])?;
        run_git_fixture(directory.path(), &["config", "user.name", "sync-test"])?;
        Ok(directory)
    }

    fn diverged_fixture() -> Result<DivergedFixture> {
        let directory = init_fixture_repo()?;
        let path = directory.path();

        commit_file(path, "ctx.txt", "shared\n", "base")?;
        let base = rev_parse(path, "main")?;

        run_git_fixture(path, &["checkout", "--quiet", "-b", "swarm"])?;
        commit_file(path, "p.txt", "p\n", "swarm adds p")?;
        commit_file(path, "q.txt", "q\n", "swarm adds q")?;
        let swarm_tip = rev_parse(path, "swarm")?;

        run_git_fixture(path, &["checkout", "--quiet", "main"])?;
        commit_file(path, "r0.txt", "r0\n", "release r0")?;
        let floor_tip = rev_parse(path, "main")?;
        run_git_fixture(path, &["cherry-pick", "swarm~1"])?;
        let picked = rev_parse(path, "main")?;
        commit_file(path, "r2.txt", "r2\n", "release r2")?;
        let release_tip = rev_parse(path, "main")?;

        Ok(DivergedFixture { directory, base, swarm_tip, floor_tip, picked, release_tip })
    }

    fn resolved(source: String, boundary: String, target: String) -> ResolvedShas {
        ResolvedShas { source, boundary, target }
    }

    fn run_git_fixture(directory: &Path, args: &[&str]) -> Result<()> {
        let mut command = Command::new("git");
        command.current_dir(directory).args(args);
        let output = command.output()?;
        if output.status.success() {
            return Ok(());
        }
        Err(eyre!("git fixture command failed: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }

    fn commit_file(directory: &Path, name: &str, content: &str, message: &str) -> Result<()> {
        fs::write(directory.join(name), content)?;
        run_git_fixture(directory, &["add", name])?;
        run_git_fixture(directory, &["commit", "--quiet", "-m", message])
    }

    fn rev_parse(directory: &Path, reference: &str) -> Result<String> {
        let mut command = Command::new("git");
        command.current_dir(directory).args(["rev-parse", reference]);
        let output = command.output()?;
        if !output.status.success() {
            return Err(eyre!("fixture rev-parse `{reference}` failed"));
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }
}
