//! `cargo xtask sync-divergence` — fail closed on unclassified target commits
//! and validate the typed reconciliation ledger against comparison reality.

use color_eyre::eyre::{Context, Report, Result, eyre};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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

/// Path segments that mark runtime, product, or test work; `release_lineage_only`
/// may never cover them.
const PRODUCT_PATH_SEGMENTS: [&str; 9] =
    ["src", "lib", "bin", "t", "test", "tests", "testing", "examples", "xt"];

/// File extensions that mark runtime, product, or test work; `.t` is the
/// standard Perl test file extension.
const PRODUCT_PATH_EXTENSIONS: [&str; 8] = ["rs", "c", "h", "hpp", "cpp", "pm", "pl", "t"];

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

/// Arguments for scaffolding a v2 reconciliation ledger skeleton.
pub struct ScaffoldConfig {
    /// Exact swarm source ref; resolved as the patch-equivalence upstream.
    pub source: String,
    /// Completed reconciliation boundary ref; resolved as the exclusive history floor.
    pub boundary: String,
    /// Release-repo target ref (normally the release repository head).
    pub target: String,
    /// Output reconciliation ledger path.
    pub ledger: PathBuf,
    /// Repository to run against; defaults to the process working directory.
    pub working_directory: Option<PathBuf>,
}

/// The single verdict a completed validation may emit. Shared with
/// `publication_sync` so the release-sync surfaces keep one verdict vocabulary
/// instead of each redefining `pass`/`blocked`/`not_proven`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Verdict {
    Pass,
    Blocked,
    NotProven,
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Verdict::Pass => "pass",
            Verdict::Blocked => "blocked",
            Verdict::NotProven => "not_proven",
        };
        write!(formatter, "{text}")
    }
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
    fn from_inputs(source: &str, boundary: &str, target: &str) -> Self {
        let state = |input: &str| SubjectState { input: input.to_string(), commit: None };
        Self { source: state(source), boundary: state(boundary), target: state(target) }
    }

    fn from_config(config: &CheckConfig) -> Self {
        Self::from_inputs(&config.source, &config.boundary, &config.target)
    }
}

impl From<&Subjects> for ReceiptSubjects {
    fn from(subjects: &Subjects) -> Self {
        let state = |subject: &SubjectState, role: &'static str| ReceiptSubject {
            role,
            input: subject.input.clone(),
            commit: subject.commit.clone(),
        };
        Self {
            source: state(&subjects.source, "patch_equivalence_upstream"),
            boundary: state(&subjects.boundary, "history_limit"),
            target: state(&subjects.target, "release_head"),
        }
    }
}

#[derive(Debug)]
struct ResolvedShas {
    source: String,
    boundary: String,
    target: String,
}

/// The typed reconciliation ledger (schema v2). Unknown fields fail closed so
/// a stale or foreign ledger shape cannot masquerade as v2.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema_version: u32,
    source: String,
    boundary: String,
    target: String,
    population_digest: String,
    #[serde(default)]
    verdict: Option<Verdict>,
    #[serde(default)]
    blockers: Vec<String>,
    entries: Vec<LedgerEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerEntry {
    commit: String,
    subject: String,
    /// `null` is an explicit unresolved row; anything outside the five
    /// terminal tokens fails closed.
    disposition: Option<String>,
    #[serde(default)]
    source_commit: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    rationale: Option<String>,
    #[serde(default)]
    changed_paths: Vec<String>,
    #[serde(default)]
    evidence: Vec<String>,
    #[serde(default)]
    release_sync_effect: Option<String>,
    #[serde(default)]
    blocking_decisions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: u32,
    subjects: ReceiptSubjects,
    ledger: String,
    verdict: Verdict,
    population_digest: String,
    target_unique_commits: Vec<ReceiptCommit>,
    excluded_merge_commits: Vec<String>,
    excluded_merge_ancestry: Vec<ExcludedMerge>,
    excluded_release_lineage_commits: Vec<String>,
    accepted_commits: Vec<String>,
    unresolved_commits: Vec<String>,
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
    role: &'static str,
    input: String,
    commit: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReceiptCommit {
    commit: String,
    subject: String,
    /// `null` marks an explicitly unresolved row.
    classification: Option<String>,
}

#[derive(Debug, Serialize)]
struct ExcludedMerge {
    commit: String,
    subject: String,
    parents: Vec<String>,
}

#[derive(Debug, Clone)]
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

/// Run the preflight, validate the reconciliation ledger, and write a receipt
/// even when validation fails.
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
    if let Err(error) = ensure_source_not_contained_in_target(&shas, directory) {
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

    let (receipt, errors) =
        reconcile(&config, &ledger, &target_unique, excluded_merges, &subjects, directory);
    write_json(&config.receipt, &receipt)?;

    match receipt.verdict {
        Verdict::Pass => {
            println!(
                "sync-divergence: reconciled {} target-unique non-merge commit(s)",
                receipt.target_unique_commits.len()
            );
            Ok(())
        }
        Verdict::Blocked => Err(eyre!(
            "sync-divergence reconciliation is blocked: {} unresolved decision(s) remain; see {}",
            receipt.unresolved_commits.len() + ledger.blockers.len(),
            config.receipt.display()
        )),
        Verdict::NotProven => Err(eyre!(
            "sync-divergence reconciliation is not proven: {} error(s); see {}",
            errors.len(),
            config.receipt.display()
        )),
    }
}

/// Emit a reconciliation ledger skeleton: one unresolved row per computed
/// target-unique non-merge commit, with real subjects and changed paths and
/// no invented terminal disposition.
pub fn scaffold(config: ScaffoldConfig) -> Result<()> {
    if config.ledger.exists() {
        return Err(eyre!(
            "scaffold ledger {} already exists; move or delete it first — \
             scaffolding must never silently overwrite an in-progress reconciliation ledger",
            config.ledger.display()
        ));
    }
    let directory = config.working_directory.as_deref();
    let mut subjects = Subjects::from_inputs(&config.source, &config.boundary, &config.target);

    let shas = resolve_subjects(&mut subjects, directory)?;
    ensure_boundary_bounds_target(&shas, directory)?;
    ensure_source_not_contained_in_target(&shas, directory)?;
    let target_unique = comparison_population(&shas, directory)?;

    let mut entries = Vec::new();
    for commit in &target_unique {
        let changed_paths = commit_changed_paths(&commit.commit, directory)?;
        entries.push(LedgerEntry {
            commit: commit.commit.clone(),
            subject: commit.subject.clone(),
            disposition: None,
            source_commit: None,
            owner: None,
            rationale: None,
            changed_paths,
            evidence: Vec::new(),
            release_sync_effect: None,
            blocking_decisions: Vec::new(),
        });
    }

    let ledger = Ledger {
        schema_version: LEDGER_SCHEMA_VERSION,
        source: shas.source,
        boundary: shas.boundary,
        target: shas.target,
        population_digest: compute_population_digest(&target_unique),
        verdict: None,
        blockers: Vec::new(),
        entries,
    };
    write_json(&config.ledger, &ledger)?;
    println!(
        "sync-divergence: scaffolded {} unresolved reconciliation row(s) into {}",
        ledger.entries.len(),
        config.ledger.display()
    );
    Ok(())
}

#[derive(Debug, PartialEq)]
enum RowKind {
    Terminal(String),
    Unresolved,
}

fn row_kind(entry: &LedgerEntry) -> RowKind {
    match entry.disposition.as_deref() {
        Some(classification) if CLASSIFICATIONS.contains(&classification) => {
            RowKind::Terminal(classification.to_string())
        }
        // An invalid token is not a terminal disposition, so the row is
        // nonterminal; the token error itself fails the verdict.
        _ => RowKind::Unresolved,
    }
}

fn reconcile(
    config: &CheckConfig,
    ledger: &Ledger,
    target_unique: &[CherryCommit],
    excluded_merges: Vec<ExcludedMerge>,
    subjects: &Subjects,
    directory: Option<&Path>,
) -> (Receipt, Vec<String>) {
    let mut errors = Vec::new();

    let digest = compute_population_digest(target_unique);
    if ledger.population_digest != digest {
        errors.push(format!(
            "ledger population digest {} does not match the comparison digest {}; the ledger is stale",
            ledger.population_digest, digest
        ));
    }
    for blocker in &ledger.blockers {
        if blocker.trim().is_empty() {
            errors.push("ledger blockers contain an empty decision".to_string());
        }
    }

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
    let mut unresolved_commits = Vec::new();

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
            classification: entry.disposition.clone(),
        });

        match row_kind(entry) {
            RowKind::Terminal(classification) => {
                if classification == "release_lineage_only" {
                    excluded_release_lineage_commits.push(commit.commit.clone());
                } else {
                    accepted_commits.push(commit.commit.clone());
                }
            }
            RowKind::Unresolved => unresolved_commits.push(commit.commit.clone()),
        }
    }

    for entry in &ledger.entries {
        validate_entry(entry, &shas_from(subjects), directory, &mut errors);
        if !seen.contains(entry.commit.as_str()) {
            errors.push(format!(
                "ledger commit {} is not a non-merge target-unique commit",
                entry.commit
            ));
        }
    }

    let mut verdict = if !errors.is_empty() {
        Verdict::NotProven
    } else if unresolved_commits.len() + ledger.blockers.len() > 0 {
        Verdict::Blocked
    } else {
        Verdict::Pass
    };
    if let Some(claimed) = ledger.verdict
        && claimed != verdict
    {
        errors.push(format!(
            "ledger claims verdict {claimed} but validation derived {verdict}; the ledger claim is stale or dishonest"
        ));
        verdict = Verdict::NotProven;
    }

    let excluded_merge_commits =
        excluded_merges.iter().map(|merge| merge.commit.clone()).collect::<Vec<_>>();
    let receipt = Receipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        subjects: ReceiptSubjects::from(subjects),
        ledger: config.ledger.display().to_string(),
        verdict,
        population_digest: digest,
        target_unique_commits: receipt_commits,
        excluded_merge_commits,
        excluded_merge_ancestry: excluded_merges,
        excluded_release_lineage_commits,
        accepted_commits,
        unresolved_commits,
        errors: errors.clone(),
    };
    (receipt, errors)
}

fn shas_from(subjects: &Subjects) -> ResolvedShas {
    let commit = |state: &SubjectState| state.commit.clone().unwrap_or_default();
    ResolvedShas {
        source: commit(&subjects.source),
        boundary: commit(&subjects.boundary),
        target: commit(&subjects.target),
    }
}

/// Enforce the per-entry disposition rules against the ledger's own claims.
fn validate_entry(
    entry: &LedgerEntry,
    shas: &ResolvedShas,
    directory: Option<&Path>,
    errors: &mut Vec<String>,
) {
    if let Some(token) = entry.disposition.as_deref()
        && !CLASSIFICATIONS.contains(&token)
    {
        errors.push(format!(
            "commit {} has invalid classification `{token}`; only the five terminal dispositions exist",
            entry.commit
        ));
    }
    verify_changed_paths(entry, directory, errors);

    let RowKind::Terminal(classification) = row_kind(entry) else {
        return;
    };

    // An undeclared footprint evades both the diff-tree comparison and the
    // product/test guard below, so every terminal row must declare its real
    // changed paths. The scaffold always populates them.
    if entry.changed_paths.is_empty() {
        errors.push(format!(
            "commit {} classified `{classification}` declares no changed_paths; every terminal row must declare its changed paths so the diff-tree comparison and product/test guard cannot be evaded",
            entry.commit
        ));
    }
    if !has_evidence(entry) {
        errors.push(format!("commit {} has no evidence", entry.commit));
    }
    if !entry.blocking_decisions.is_empty() {
        errors.push(format!(
            "commit {} carries blocking decisions but claims terminal disposition `{}`; an unresolved decision is not a sixth disposition",
            entry.commit, classification
        ));
    }

    match classification.as_str() {
        "port_to_swarm" | "already_equivalent_in_swarm" => {
            require_source_ancestor(entry, &classification, shas, directory, errors)
        }
        "superseded_by_newer_architecture" => {
            let named = entry.owner.as_deref().map(str::trim).unwrap_or_default();
            if named.is_empty() {
                errors.push(format!(
                    "commit {} classified `superseded_by_newer_architecture` must name the current architecture owner",
                    entry.commit
                ));
            }
        }
        "deliberately_abandoned" => {
            let stated = entry.rationale.as_deref().map(str::trim).unwrap_or_default();
            if stated.is_empty() {
                errors.push(format!(
                    "commit {} classified `deliberately_abandoned` must state the rejected behavior and rationale",
                    entry.commit
                ));
            }
        }
        "release_lineage_only" => {
            if let Some(path) =
                entry.changed_paths.iter().find(|path| is_product_or_test_path(path))
            {
                errors.push(format!(
                    "commit {} classified `release_lineage_only` covers product or test path `{path}`; lineage-only cannot exclude runtime, product, or test work",
                    entry.commit
                ));
            }
        }
        _ => {}
    }
}

/// A port or equivalent must point at an exact commit reachable from the
/// declared swarm source; planned-but-unmerged or unreachable identities fail.
fn require_source_ancestor(
    entry: &LedgerEntry,
    classification: &str,
    shas: &ResolvedShas,
    directory: Option<&Path>,
    errors: &mut Vec<String>,
) {
    let Some(source_commit) = entry.source_commit.as_deref() else {
        errors.push(format!(
            "commit {} classified `{classification}` requires the exact swarm `source_commit` it ports to or matches",
            entry.commit
        ));
        return;
    };
    if let Err(error) = ensure_reachable_from_source(source_commit, &shas.source, directory) {
        errors.push(format!("commit {}: {error}", entry.commit));
    }
}

fn ensure_reachable_from_source(
    candidate: &str,
    source: &str,
    directory: Option<&Path>,
) -> Result<()> {
    validate_subject_syntax("source_commit", candidate)?;
    let resolved = resolve_peeled_commit("source_commit", candidate, directory)?;
    match git_status_in(["merge-base", "--is-ancestor", &resolved, source], directory)? {
        0 => Ok(()),
        1 => Err(eyre!(
            "`source_commit` `{candidate}` is not reachable from the declared swarm source {source}; a port must exist in the source before the ledger can accept it"
        )),
        code => Err(eyre!("git merge-base --is-ancestor exited with status {code}")),
    }
}

/// Declared changed paths are verified against Git so a stale or cross-commit
/// row cannot borrow another commit's footprint.
fn verify_changed_paths(entry: &LedgerEntry, directory: Option<&Path>, errors: &mut Vec<String>) {
    if entry.changed_paths.is_empty() {
        return;
    }
    if let Err(error) = validate_subject_syntax("commit", &entry.commit) {
        errors.push(format!(
            "ledger changed paths for {} could not be verified: {error:#}",
            entry.commit
        ));
        return;
    }
    match commit_changed_paths(&entry.commit, directory) {
        Ok(actual) => {
            let mut claimed = entry.changed_paths.clone();
            claimed.sort();
            claimed.dedup();
            if claimed != actual {
                errors.push(format!(
                    "ledger changed paths for {} do not match Git: ledger=`{claimed:?}` Git=`{actual:?}`",
                    entry.commit
                ));
            }
        }
        Err(error) => errors.push(format!(
            "ledger changed paths for {} could not be verified: {error:#}",
            entry.commit
        )),
    }
}

fn commit_changed_paths(commit: &str, directory: Option<&Path>) -> Result<Vec<String>> {
    let output = git_output_in(
        ["diff-tree", "--no-commit-id", "--name-only", "-r", "--root", commit],
        directory,
    )?;
    let mut paths: Vec<String> =
        output.lines().filter(|line| !line.trim().is_empty()).map(str::to_string).collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// True when a repository path carries runtime, product, or test work. Shared
/// with `publication_sync` so "this exclusion hides product work" has one
/// definition across the release-sync surfaces.
pub(crate) fn is_product_or_test_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    if normalized.split('/').any(|segment| PRODUCT_PATH_SEGMENTS.contains(&segment)) {
        return true;
    }
    match normalized.rsplit_once('.') {
        Some((_, extension)) => {
            PRODUCT_PATH_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        }
        None => false,
    }
}

/// Canonical digest over the target-unique non-merge population; the ledger
/// must reproduce it exactly or it is stale. Sorting makes the digest independent
/// of caller ordering.
fn compute_population_digest(target_unique: &[CherryCommit]) -> String {
    let mut rows: Vec<(&str, String)> = target_unique
        .iter()
        .map(|commit| (commit.commit.as_str(), normalize_subject(&commit.subject)))
        .collect();
    rows.sort();
    let mut hasher = Sha256::new();
    for (commit, subject) in rows {
        hasher.update(commit.as_bytes());
        hasher.update(b" ");
        hasher.update(subject.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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

/// Resolve every subject independently so a failure receipt records each
/// subject's actual outcome; a receipt must never record `commit: null` for a
/// subject whose resolution was never attempted.
fn resolve_subjects(subjects: &mut Subjects, directory: Option<&Path>) -> Result<ResolvedShas> {
    let attempt = |label: &str, state: &mut SubjectState| -> Result<String> {
        let resolved = resolve_subject(label, state, directory)?;
        state.commit = Some(resolved.clone());
        Ok(resolved)
    };
    let source = attempt("source", &mut subjects.source);
    let boundary = attempt("boundary", &mut subjects.boundary);
    let target = attempt("target", &mut subjects.target);

    let mut errors = Vec::new();
    let source = record_outcome(source, &mut errors);
    let boundary = record_outcome(boundary, &mut errors);
    let target = record_outcome(target, &mut errors);

    match (source, boundary, target) {
        (Some(source), Some(boundary), Some(target)) => {
            Ok(ResolvedShas { source, boundary, target })
        }
        _ => Err(eyre!(errors.join("; "))),
    }
}

fn record_outcome(outcome: Result<String>, errors: &mut Vec<String>) -> Option<String> {
    outcome.map_err(|error| errors.push(format!("{error:#}"))).ok()
}

/// Resolve one commit-ish to a full object id through the quiet+loud probe:
/// quiet `rev-parse --verify` resolves the id while suppressing git's
/// refname-ambiguity warning, so the loud re-probe fails closed when a
/// branch/tag collision would otherwise resolve by silent internal
/// precedence. Every commit-ish identity in this task (subjects and ledger
/// `source_commit`s) must go through this one helper.
fn resolve_peeled_commit(label: &str, reference: &str, directory: Option<&Path>) -> Result<String> {
    let peeled = format!("{reference}^{{commit}}");
    match git_output_in(
        ["rev-parse", "--verify", "--quiet", "--end-of-options", &peeled],
        directory,
    ) {
        Ok(output) => {
            let resolved = output.trim().to_string();
            if resolved.is_empty() {
                return Err(eyre!("{label} ref `{reference}` did not resolve to a commit"));
            }
            // Quiet mode suppresses git's refname-ambiguity warning, so a
            // branch+tag name collision would silently resolve by internal
            // precedence. A loud re-probe fails closed on that case.
            let loud_stderr = git_stderr_in(["rev-parse", "--end-of-options", &peeled], directory)
                .unwrap_or_default();
            if loud_stderr.contains("is ambiguous") {
                return Err(eyre!(
                    "{label} ref `{reference}` was ambiguous; pass a full 40-hex object id or an unambiguous ref name"
                ));
            }
            Ok(resolved)
        }
        Err(_) => {
            let stderr =
                git_stderr_in(["rev-parse", "--verify", "--end-of-options", &peeled], directory)
                    .unwrap_or_default();
            if stderr.contains("ambiguous") {
                Err(eyre!(
                    "{label} ref `{reference}` was ambiguous; pass a full 40-hex object id or an unambiguous ref name"
                ))
            } else {
                Err(eyre!("{label} ref `{reference}` did not resolve to a commit"))
            }
        }
    }
}

fn resolve_subject(label: &str, state: &SubjectState, directory: Option<&Path>) -> Result<String> {
    validate_subject_syntax(label, &state.input)?;
    resolve_peeled_commit(label, &state.input, directory)
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

/// Fail closed when the swarm source is already contained in the target: the
/// subjects are reversed (swarm passed as the target) or reconciliation is
/// already complete, and either way there is no honest target-unique set.
fn ensure_source_not_contained_in_target(
    shas: &ResolvedShas,
    directory: Option<&Path>,
) -> Result<()> {
    match git_status_in(["merge-base", "--is-ancestor", &shas.source, &shas.target], directory)? {
        0 => Err(eyre!(
            "reversed subjects: source {} is already contained in target {}; the swarm source must not be reachable from the release head (swarm may have been passed as the target)",
            shas.source,
            shas.target
        )),
        1 => Ok(()),
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
        verdict: Verdict::NotProven,
        population_digest: String::new(),
        target_unique_commits: Vec::new(),
        excluded_merge_commits: Vec::new(),
        excluded_merge_ancestry: Vec::new(),
        excluded_release_lineage_commits: Vec::new(),
        accepted_commits: Vec::new(),
        unresolved_commits: Vec::new(),
        errors: vec![message.clone()],
    };
    write_json(&config.receipt, &receipt)?;
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

fn git_stderr_in<const N: usize>(args: [&str; N], directory: Option<&Path>) -> Result<String> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(directory) = directory {
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

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating receipt directory {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(value).context("serializing sync receipt")?;
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

    /// An abbreviated object id matching multiple objects fails closed as
    /// ambiguous rather than resolving to an arbitrary commit.
    #[test]
    fn ambiguous_subjects_fail_closed() -> Result<()> {
        let directory = init_fixture_repo()?;
        let prefix = find_ambiguous_prefix(directory.path())?;
        let state = SubjectState { input: prefix.clone(), commit: None };
        let error = match resolve_subject("source", &state, Some(directory.path())) {
            Err(error) => error,
            Ok(resolved) => {
                return Err(eyre!("ambiguous prefix `{prefix}` resolved to {resolved}"));
            }
        };
        let message = format!("{error:#}");
        assert!(message.contains("ambiguous"), "{message}");
        Ok(())
    }

    /// Quiet `rev-parse --verify` resolves a branch+tag name collision by
    /// internal precedence while suppressing git's ambiguity warning; the
    /// loud re-probe must fail closed instead of picking an arbitrary side.
    #[test]
    fn ambiguous_refnames_fail_closed_instead_of_silent_precedence() -> Result<()> {
        let directory = init_fixture_repo()?;
        let path = directory.path();
        commit_file(path, "a.txt", "a\n", "base")?;
        run_git_fixture(path, &["branch", "dup-7968"])?;
        commit_file(path, "b.txt", "b\n", "second")?;
        run_git_fixture(path, &["tag", "dup-7968"])?;

        let state = SubjectState { input: "dup-7968".to_string(), commit: None };
        let error = match resolve_subject("source", &state, Some(path)) {
            Err(error) => error,
            Ok(resolved) => {
                return Err(eyre!("an ambiguous refname must not silently resolve to {resolved}"));
            }
        };
        let message = format!("{error:#}");
        assert!(message.contains("ambiguous"), "{message}");
        Ok(())
    }

    fn find_ambiguous_prefix(path: &Path) -> Result<String> {
        use std::io::Write;
        let mut seen = std::collections::HashMap::new();
        for index in 0..100_000u32 {
            let mut child = Command::new("git")
                .current_dir(path)
                .args(["hash-object", "-w", "--stdin"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .context("spawning git hash-object")?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(format!("blob-{index}\n").as_bytes())?;
            }
            let output = child.wait_with_output()?;
            if !output.status.success() {
                return Err(eyre!("git hash-object failed in fixture"));
            }
            let sha = String::from_utf8(output.stdout)
                .map_err(|error| eyre!("hash-object output was not UTF-8: {error}"))?;
            let sha = sha.trim().to_string();
            let prefix = sha[..4].to_string();
            match seen.get(&prefix) {
                Some(other) if *other != sha => return Ok(prefix),
                Some(_) => {}
                None => {
                    seen.insert(prefix, sha);
                }
            }
        }
        Err(eyre!("no ambiguous prefix found in fixture"))
    }

    /// Swarm passed as the target (source/target swapped) fails closed even
    /// though the boundary legitimately bounds both histories.
    #[test]
    fn swarm_passed_as_target_fails_closed() -> Result<()> {
        let directory = init_fixture_repo()?;
        let path = directory.path();
        commit_file(path, "ctx.txt", "shared\n", "base")?;
        let boundary = rev_parse(path, "main")?;
        commit_file(path, "r.txt", "r\n", "release work")?;
        let release_tip = rev_parse(path, "main")?;
        run_git_fixture(path, &["checkout", "--quiet", "-b", "swarm", &release_tip])?;
        commit_file(path, "s.txt", "s\n", "swarm work")?;
        let swarm_tip = rev_parse(path, "swarm")?;

        let swapped = resolved(release_tip.clone(), boundary.clone(), swarm_tip.clone());
        let error = match ensure_source_not_contained_in_target(&swapped, Some(path)) {
            Err(error) => error,
            Ok(()) => return Err(eyre!("swarm passed as the target must fail closed")),
        };
        let message = format!("{error:#}");
        assert!(message.contains("reversed subjects"), "{message}");

        // The honest direction (swarm source, release target) passes the guard.
        let honest = resolved(swarm_tip, boundary, release_tip);
        ensure_source_not_contained_in_target(&honest, Some(path))?;
        Ok(())
    }

    /// The swarm-as-target swap fails the whole preflight end to end and the
    /// failure is recorded in the receipt, not just in the guard helper.
    #[test]
    fn swarm_passed_as_target_fails_the_preflight_with_a_receipt() -> Result<()> {
        let directory = init_fixture_repo()?;
        let path = directory.path();
        commit_file(path, "ctx.txt", "shared\n", "base")?;
        let boundary = rev_parse(path, "main")?;
        commit_file(path, "r.txt", "r\n", "release work")?;
        let release_tip = rev_parse(path, "main")?;
        run_git_fixture(path, &["checkout", "--quiet", "-b", "swarm", &release_tip])?;
        commit_file(path, "s.txt", "s\n", "swarm work")?;
        let swarm_tip = rev_parse(path, "swarm")?;

        let config = CheckConfig {
            source: release_tip.clone(),
            boundary: boundary.clone(),
            target: swarm_tip.clone(),
            ledger: path.join("ledger.json"),
            receipt: path.join("receipt.json"),
            working_directory: Some(path.to_path_buf()),
        };
        let error = match check(config) {
            Err(error) => error,
            Ok(()) => return Err(eyre!("swarm passed as the target must fail the preflight")),
        };
        let message = format!("{error:#}");
        assert!(message.contains("reversed subjects"), "{message}");
        let receipt: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path.join("receipt.json"))?)?;
        assert_eq!(receipt["subjects"]["source"]["commit"], release_tip.as_str());
        assert_eq!(receipt["subjects"]["target"]["commit"], swarm_tip.as_str());
        assert_eq!(receipt["target_unique_commits"].as_array().map(Vec::len), Some(0));
        assert_eq!(receipt["errors"].as_array().map(Vec::len), Some(1));
        assert_eq!(receipt["verdict"], "not_proven");
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

    /// The canonical population digest is deterministic and content-sensitive.
    #[test]
    fn population_digest_is_deterministic_and_content_sensitive() {
        let first =
            CherryCommit { commit: "aaa".into(), subject: "one".into(), parents: vec!["p".into()] };
        let second =
            CherryCommit { commit: "bbb".into(), subject: "two".into(), parents: vec!["p".into()] };
        let ab = compute_population_digest(&[first.clone(), second]);
        let ba_input =
            CherryCommit { commit: "bbb".into(), subject: "two".into(), parents: vec!["p".into()] };
        let ba = compute_population_digest(&[ba_input, first.clone()]);
        assert_eq!(ab, ba, "digest must be order-insensitive over sorted commits");
        let changed = CherryCommit {
            commit: "aaa".into(),
            subject: "one changed".into(),
            parents: vec!["p".into()],
        };
        assert_ne!(
            compute_population_digest(std::slice::from_ref(&first)),
            compute_population_digest(std::slice::from_ref(&changed)),
            "a subject change must change the digest"
        );
    }

    #[test]
    fn product_path_heuristic_flags_runtime_and_test_work() {
        for path in [
            "crates/perl-parser/src/lib.rs",
            "src/main.c",
            "include/perl.h",
            "t/class.t",
            "release/smoke.t",
            "tests/smoke.pl",
            "bin/tool",
            "lib/Util.pm",
            r"windows\src\lib.rs",
        ] {
            assert!(is_product_or_test_path(path), "`{path}` must count as product/test work");
        }
        for path in [
            "docs/releases/v1.md",
            "CHANGELOG.md",
            "README",
            "ci/pipeline.yml",
            ".github/workflows/ci.yml",
            "notes.txt",
        ] {
            assert!(!is_product_or_test_path(path), "`{path}` must not count as product/test work");
        }
    }

    /// A fully classified ledger passes and the receipt records the derived
    /// verdict, the population digest, and the consumed v2 receipt identity.
    #[test]
    fn success_receipt_records_resolved_identity_and_pass_verdict() -> Result<()> {
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
  "population_digest": "{}",
  "blockers": [],
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": ["r0.txt"], "evidence": ["release-lineage receipt"], "release_sync_effect": "stays release-side"}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "changed_paths": ["r2.txt"], "evidence": ["ported into the swarm source"], "release_sync_effect": "carried by the source"}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.release_tip,
                fixture.swarm_tip,
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
        assert_eq!(
            receipt["subjects"]["source"]["role"].as_str(),
            Some("patch_equivalence_upstream")
        );
        assert_eq!(receipt["subjects"]["boundary"]["commit"], fixture.base.as_str());
        assert_eq!(receipt["subjects"]["boundary"]["role"].as_str(), Some("history_limit"));
        assert_eq!(receipt["subjects"]["target"]["commit"], fixture.release_tip.as_str());
        assert_eq!(receipt["subjects"]["target"]["role"].as_str(), Some("release_head"));
        assert_eq!(receipt["verdict"], "pass");
        assert_eq!(receipt["population_digest"], fixture_digest(&fixture)?.as_str());
        assert_eq!(receipt["target_unique_commits"].as_array().map(Vec::len), Some(2));
        assert_eq!(receipt["accepted_commits"].as_array().map(Vec::len), Some(1));
        assert_eq!(receipt["excluded_release_lineage_commits"].as_array().map(Vec::len), Some(1));
        assert_eq!(receipt["excluded_merge_commits"].as_array().map(Vec::len), Some(0));
        assert_eq!(receipt["unresolved_commits"].as_array().map(Vec::len), Some(0));
        assert_eq!(receipt["errors"].as_array().map(Vec::len), Some(0));
        Ok(())
    }

    fn diverged_ledger_json(fixture: &DivergedFixture) -> Result<String> {
        Ok(format!(
            r#"{{
  "schema_version": 2,
  "source": "{}",
  "boundary": "{}",
  "target": "{}",
  "population_digest": "{}",
  "blockers": [],
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": ["r0.txt"], "evidence": ["release-lineage receipt"], "release_sync_effect": "stays release-side"}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "changed_paths": ["r2.txt"], "evidence": ["ported into the swarm source"], "release_sync_effect": "carried by the source"}}
  ]
}}"#,
            fixture.swarm_tip,
            fixture.base,
            fixture.release_tip,
            fixture_digest(fixture)?,
            fixture.floor_tip,
            fixture.release_tip,
            fixture.swarm_tip,
        ))
    }

    /// Receipts are byte-deterministic across runs against the same repository.
    #[test]
    fn receipts_are_byte_deterministic_across_runs() -> Result<()> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        fs::write(path.join("ledger.json"), diverged_ledger_json(&fixture)?)?;
        let config = |receipt: &str| CheckConfig {
            source: fixture.swarm_tip.clone(),
            boundary: fixture.base.clone(),
            target: fixture.release_tip.clone(),
            ledger: path.join("ledger.json"),
            receipt: path.join(receipt),
            working_directory: Some(path.to_path_buf()),
        };
        check(config("receipt-1.json"))?;
        check(config("receipt-2.json"))?;
        assert_eq!(fs::read(path.join("receipt-1.json"))?, fs::read(path.join("receipt-2.json"))?);
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
        // Every subject is resolved independently: the failed source records
        // null, while the valid boundary and target record their resolved
        // immutable commits rather than a never-attempted null.
        assert_eq!(receipt["subjects"]["source"]["commit"], serde_json::Value::Null);
        assert_eq!(receipt["subjects"]["boundary"]["commit"], fixture.base.as_str());
        assert_eq!(receipt["subjects"]["target"]["commit"], fixture.release_tip.as_str());
        assert_eq!(receipt["subjects"]["target"]["input"], fixture.release_tip.as_str());
        assert_eq!(receipt["errors"].as_array().map(Vec::len), Some(1));
        Ok(())
    }

    /// Multiple invalid subjects are all reported, and the one valid subject
    /// still records its resolved commit.
    #[test]
    fn every_subject_resolution_is_attempted_and_recorded() -> Result<()> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        let config = CheckConfig {
            source: "refs/heads/nope-source-7968".to_string(),
            boundary: "refs/heads/nope-boundary-7968".to_string(),
            target: fixture.release_tip.clone(),
            ledger: path.join("missing.json"),
            receipt: path.join("receipt.json"),
            working_directory: Some(path.to_path_buf()),
        };
        let error = match check(config) {
            Err(error) => error,
            Ok(()) => return Err(eyre!("invalid subjects must fail closed")),
        };
        let message = format!("{error:#}");
        assert!(message.contains("nope-source-7968"), "{message}");
        assert!(message.contains("nope-boundary-7968"), "{message}");
        let receipt: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path.join("receipt.json"))?)?;
        assert_eq!(receipt["subjects"]["source"]["commit"], serde_json::Value::Null);
        assert_eq!(receipt["subjects"]["boundary"]["commit"], serde_json::Value::Null);
        assert_eq!(receipt["subjects"]["target"]["commit"], fixture.release_tip.as_str());
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
                r#"{{"schema_version":2,"source":"{}","boundary":"{}","target":"{}","population_digest":"x","entries":[]}}"#,
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
        assert_eq!(receipt["verdict"], "not_proven");
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
        let ledger = Ledger {
            schema_version: LEDGER_SCHEMA_VERSION,
            source: config.source.clone(),
            boundary: config.boundary.clone(),
            target: config.target.clone(),
            population_digest: compute_population_digest(&target_unique),
            verdict: None,
            blockers: Vec::new(),
            entries: vec![
                LedgerEntry {
                    commit: "abc".to_string(),
                    subject: "different subject".to_string(),
                    disposition: Some("release_lineage_only".to_string()),
                    source_commit: None,
                    owner: None,
                    rationale: None,
                    changed_paths: Vec::new(),
                    evidence: vec!["   ".to_string()],
                    release_sync_effect: None,
                    blocking_decisions: Vec::new(),
                },
                LedgerEntry {
                    commit: "abc".to_string(),
                    subject: "subject".to_string(),
                    disposition: Some("port_to_swarm".to_string()),
                    source_commit: None,
                    owner: None,
                    rationale: None,
                    changed_paths: Vec::new(),
                    evidence: vec!["valid evidence".to_string()],
                    release_sync_effect: None,
                    blocking_decisions: Vec::new(),
                },
            ],
        };
        let excluded_merges = vec![ExcludedMerge {
            commit: "zzz".to_string(),
            subject: "merge subject".to_string(),
            parents: vec!["first".to_string(), "second".to_string()],
        }];
        let mut subjects = Subjects::from_inputs("source", "boundary", "target");
        subjects.source.commit = Some("source-sha".to_string());
        subjects.boundary.commit = Some("boundary-sha".to_string());
        subjects.target.commit = Some("target-sha".to_string());

        let (receipt, errors) =
            reconcile(&config, &ledger, &target_unique, excluded_merges, &subjects, None);
        assert_eq!(receipt.target_unique_commits.len(), 1);
        assert_eq!(receipt.excluded_merge_commits, vec!["zzz"]);
        assert_eq!(receipt.excluded_merge_ancestry.len(), 1);
        assert_eq!(receipt.subjects.source.commit.as_deref(), Some("source-sha"));
        assert!(receipt.accepted_commits.is_empty());
        assert_eq!(receipt.verdict, Verdict::NotProven);
        assert!(errors.iter().any(|error| error.contains("appears more than once")));
        assert!(errors.iter().any(|error| error.contains("has no evidence")));
        assert!(errors.iter().any(|error| error.contains("does not match Git")));
        assert!(
            errors.iter().any(|error| error.contains("requires the exact swarm `source_commit`"))
        );
        Ok(())
    }

    /// Population completeness is exact: a missing row fails as not proven.
    #[test]
    fn missing_ledger_row_fails_as_not_proven() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "evidence": ["ported"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.release_tip,
                fixture.swarm_tip,
            ),
        )?;
        let error = run_check_expect_error(&fixture, path)?;
        assert!(format!("{error:#}").contains("not proven"));
        let receipt = receipt_value(path)?;
        assert_eq!(receipt["verdict"], "not_proven");
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("missing from the reconciliation ledger"), "{joined}");
        Ok(())
    }

    /// A suppressed commit must not sneak into the ledger as a row.
    #[test]
    fn extra_ledger_row_for_suppressed_commit_fails() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": ["r0.txt"], "evidence": ["release-lineage receipt"]}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "evidence": ["ported"]}},
    {{"commit": "{}", "subject": "cherry-picked p", "disposition": "deliberately_abandoned", "rationale": "already represented", "evidence": ["represented upstream"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.release_tip,
                fixture.swarm_tip,
                fixture.picked,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("not a non-merge target-unique commit"), "{joined}");
        Ok(())
    }

    /// A port whose `source_commit` exists but is not reachable from the
    /// declared swarm source is a planned-but-unmerged port and fails closed.
    #[test]
    fn planned_but_unmerged_port_fails_closed() -> Result<()> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        run_git_fixture(path, &["checkout", "--quiet", "-b", "parallel", &fixture.base])?;
        commit_file(path, "later.txt", "later\n", "parallel work not yet in swarm")?;
        let planned = rev_parse(path, "parallel")?;

        fs::write(
            path.join("ledger.json"),
            format!(
                r#"{{
  "schema_version": 2,
  "source": "{}",
  "boundary": "{}",
  "target": "{}",
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": ["r0.txt"], "evidence": ["release-lineage receipt"]}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "evidence": ["claimed port"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.release_tip,
                planned,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        assert_eq!(receipt["verdict"], "not_proven");
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("not reachable from the declared swarm source"), "{joined}");
        Ok(())
    }

    /// An equivalent SHA that resolves to nothing fails closed.
    #[test]
    fn unreachable_equivalent_sha_fails_closed() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": ["r0.txt"], "evidence": ["release-lineage receipt"]}},
    {{"commit": "{}", "subject": "release r2", "disposition": "already_equivalent_in_swarm", "source_commit": "0123456789abcdef0123456789abcdef01234567", "evidence": ["claimed equivalent"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.release_tip,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("did not resolve to a commit"), "{joined}");
        Ok(())
    }

    /// Terminal dispositions require explicit evidence.
    #[test]
    fn terminal_row_without_evidence_fails() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": ["r0.txt"], "evidence": []}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "evidence": ["ported"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.release_tip,
                fixture.swarm_tip,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("has no evidence"), "{joined}");
        Ok(())
    }

    /// A sixth terminal token is rejected; only the five dispositions exist.
    #[test]
    fn sixth_terminal_token_fails() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "deferred_to_next_train", "evidence": ["wishful thinking"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.picked,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("invalid classification `deferred_to_next_train`"), "{joined}");
        Ok(())
    }

    /// A terminal row may not simultaneously carry open blocking decisions.
    #[test]
    fn blocking_decision_in_accepted_row_fails() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "port_to_swarm", "source_commit": "{}", "blocking_decisions": ["dap-architecture"], "evidence": ["ported"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.swarm_tip,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        let joined = receipt_error_text(&receipt);
        assert!(
            joined.contains("carries blocking decisions but claims terminal disposition"),
            "{joined}"
        );
        Ok(())
    }

    /// An explicitly unresolved row keeps the ledger out of trouble: the
    /// verdict is blocked, the row stays out of accepted commits, and the
    /// receipt names it.
    #[test]
    fn unresolved_rows_force_a_blocked_verdict() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": null, "blocking_decisions": ["dap-architecture"], "changed_paths": ["r0.txt"]}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "changed_paths": ["r2.txt"], "evidence": ["ported"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.release_tip,
                fixture.swarm_tip,
            ),
        )?;
        let error = run_check_expect_error(&fixture, path)?;
        let message = format!("{error:#}");
        assert!(message.contains("blocked"), "{message}");
        let receipt = receipt_value(path)?;
        assert_eq!(receipt["verdict"], "blocked");
        assert_eq!(receipt["unresolved_commits"], serde_json::json!([fixture.floor_tip]));
        assert_eq!(receipt["accepted_commits"], serde_json::json!([fixture.release_tip]));
        assert_eq!(receipt["errors"].as_array().map(Vec::len), Some(0));
        Ok(())
    }

    /// Superseded rows must name the current architecture owner.
    #[test]
    fn superseded_row_without_owner_fails() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "superseded_by_newer_architecture", "evidence": ["discriminating comparison receipt"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("must name the current architecture owner"), "{joined}");
        Ok(())
    }

    /// Abandoned rows must state the rejected behavior and rationale.
    #[test]
    fn abandoned_row_without_rationale_fails() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "deliberately_abandoned", "evidence": ["some evidence"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("rejected behavior and rationale"), "{joined}");
        Ok(())
    }

    /// Lineage-only classification cannot cover runtime, product, or test work.
    #[test]
    fn lineage_only_over_product_code_fails() -> Result<()> {
        let fixture = diverged_fixture_with_product_commit()?;
        let path = fixture.directory.path();
        let product_tip = rev_parse(path, "main")?;
        fs::write(
            path.join("ledger.json"),
            format!(
                r#"{{
  "schema_version": 2,
  "source": "{}",
  "boundary": "{}",
  "target": "{}",
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": ["r0.txt"], "evidence": ["release-lineage receipt"]}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "evidence": ["ported"]}},
    {{"commit": "{}", "subject": "release r3 runtime", "disposition": "release_lineage_only", "changed_paths": ["src/helper.rs"], "evidence": ["claiming lineage"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                product_tip,
                fixture_digest_at(&fixture, &product_tip)?,
                fixture.floor_tip,
                fixture.release_tip,
                fixture.swarm_tip,
                product_tip,
            ),
        )?;
        run_check_expect_error_at(&fixture, path, &product_tip)?;
        let receipt = receipt_value(path)?;
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("covers product or test path `src/helper.rs`"), "{joined}");
        Ok(())
    }

    /// A hand-written terminal row with empty changed_paths cannot borrow the
    /// early-return in the diff-tree comparison to also skip the product/test
    /// guard: an undeclared footprint fails closed.
    #[test]
    fn terminal_lineage_row_without_changed_paths_fails() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": [], "evidence": ["release-lineage receipt"]}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "changed_paths": ["r2.txt"], "evidence": ["ported"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.release_tip,
                fixture.swarm_tip,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("declares no changed_paths"), "{joined}");
        Ok(())
    }

    /// A ledger `source_commit` that hits a branch+tag refname collision must
    /// fail closed on ambiguity exactly like subject resolution, instead of
    /// quietly resolving by git's internal refname precedence.
    #[test]
    fn ambiguous_source_commit_fails_closed_like_subjects() -> Result<()> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        commit_file(path, "a.txt", "a\n", "base")?;
        run_git_fixture(path, &["branch", "dup-port-7969"])?;
        commit_file(path, "b.txt", "b\n", "second")?;
        run_git_fixture(path, &["tag", "dup-port-7969"])?;

        fs::write(
            path.join("ledger.json"),
            format!(
                r#"{{
  "schema_version": 2,
  "source": "{}",
  "boundary": "{}",
  "target": "{}",
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": ["r0.txt"], "evidence": ["release-lineage receipt"]}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "dup-port-7969", "changed_paths": ["r2.txt"], "evidence": ["ported"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.release_tip,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let joined = receipt_error_text(&receipt_value(path)?);
        assert!(joined.contains("source_commit ref `dup-port-7969` was ambiguous"), "{joined}");
        Ok(())
    }

    /// Scaffolding refuses to silently overwrite an existing ledger file; an
    /// in-progress reconciliation is never clobbered by a fresh skeleton.
    #[test]
    fn scaffold_refuses_to_overwrite_an_existing_ledger() -> Result<()> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        fs::write(path.join("scaffold.json"), "{\n  \"keep\": \"me\"\n}\n")?;
        let config = ScaffoldConfig {
            source: fixture.swarm_tip.clone(),
            boundary: fixture.base.clone(),
            target: fixture.release_tip.clone(),
            ledger: path.join("scaffold.json"),
            working_directory: Some(path.to_path_buf()),
        };
        let error = match scaffold(config) {
            Err(error) => error,
            Ok(()) => return Err(eyre!("scaffold must not overwrite an existing ledger")),
        };
        let message = format!("{error:#}");
        assert!(message.contains("already exists"), "{message}");
        assert!(message.contains("move or delete"), "{message}");
        assert_eq!(
            fs::read_to_string(path.join("scaffold.json"))?,
            "{\n  \"keep\": \"me\"\n}\n",
            "the existing ledger file must be untouched"
        );
        Ok(())
    }

    /// Duplicate rows fail the end-to-end preflight, giving the duplicate
    /// population case the same end-to-end coverage as the missing-row and
    /// extra-row fixtures.
    #[test]
    fn duplicate_ledger_rows_fail_end_to_end() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": ["r0.txt"], "evidence": ["release-lineage receipt"]}},
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": ["r0.txt"], "evidence": ["duplicate of the row above"]}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "evidence": ["ported"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.floor_tip,
                fixture.release_tip,
                fixture.swarm_tip,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        assert_eq!(receipt["verdict"], "not_proven");
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("appears more than once"), "{joined}");
        Ok(())
    }

    /// An old v2-shaped ledger whose entries classify through a
    /// `classification` key is rejected by `deny_unknown_fields` instead of
    /// parsing into rows that silently lose their dispositions.
    #[test]
    fn old_v2_classification_key_fails_closed() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "classification": "release_lineage_only"}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
            ),
        )?;
        let error = run_check_expect_error(&fixture, path)?;
        let message = format!("{error:#}");
        assert!(message.contains("unknown field `classification`"), "{message}");
        Ok(())
    }

    /// A stale digest fails closed: the ledger must describe this comparison.
    #[test]
    fn stale_population_digest_fails() -> Result<()> {
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
  "population_digest": "deadbeef",
  "entries": []
}}"#,
                fixture.swarm_tip, fixture.base, fixture.release_tip,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("does not match the comparison digest"), "{joined}");
        Ok(())
    }

    /// Declared changed paths are verified against Git; borrowed footprints fail.
    #[test]
    fn changed_paths_drift_fails() -> Result<()> {
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
  "population_digest": "{}",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": "release_lineage_only", "changed_paths": ["someone-elses-file.txt"], "evidence": ["release-lineage receipt"]}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "evidence": ["ported"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.release_tip,
                fixture.swarm_tip,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("do not match Git"), "{joined}");
        Ok(())
    }

    /// A ledger claiming `pass` while carrying an unresolved row is dishonest
    /// and fails as not proven.
    #[test]
    fn verdict_claim_drift_fails() -> Result<()> {
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
  "population_digest": "{}",
  "verdict": "pass",
  "entries": [
    {{"commit": "{}", "subject": "release r0", "disposition": null, "blocking_decisions": ["dap-architecture"]}},
    {{"commit": "{}", "subject": "release r2", "disposition": "port_to_swarm", "source_commit": "{}", "changed_paths": ["r2.txt"], "evidence": ["ported"]}}
  ]
}}"#,
                fixture.swarm_tip,
                fixture.base,
                fixture.release_tip,
                fixture_digest(&fixture)?,
                fixture.floor_tip,
                fixture.release_tip,
                fixture.swarm_tip,
            ),
        )?;
        run_check_expect_error(&fixture, path)?;
        let receipt = receipt_value(path)?;
        assert_eq!(receipt["verdict"], "not_proven");
        let joined = receipt_error_text(&receipt);
        assert!(joined.contains("claims verdict pass but validation derived blocked"), "{joined}");
        Ok(())
    }

    /// The scaffold creates one explicitly unresolved row per population
    /// commit with real subjects and changed paths, inventing no terminal
    /// disposition; validating the scaffold yields a blocked verdict.
    #[test]
    fn scaffold_creates_only_unresolved_rows_and_validates_blocked() -> Result<()> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        let config = ScaffoldConfig {
            source: fixture.swarm_tip.clone(),
            boundary: fixture.base.clone(),
            target: fixture.release_tip.clone(),
            ledger: path.join("scaffold.json"),
            working_directory: Some(path.to_path_buf()),
        };
        scaffold(config)?;
        let scaffolded: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path.join("scaffold.json"))?)?;
        assert_eq!(scaffolded["schema_version"], LEDGER_SCHEMA_VERSION);
        assert_eq!(scaffolded["population_digest"], fixture_digest(&fixture)?.as_str());
        let entries = scaffolded["entries"].as_array().cloned().unwrap_or_default();
        assert_eq!(entries.len(), 2);
        for entry in &entries {
            assert_eq!(entry["disposition"], serde_json::Value::Null);
            assert_eq!(entry["evidence"].as_array().map(Vec::len), Some(0));
            assert_eq!(entry["blocking_decisions"].as_array().map(Vec::len), Some(0));
        }
        assert!(entries.iter().any(|entry| entry["commit"] == fixture.floor_tip.as_str()));
        assert!(entries.iter().any(|entry| entry["commit"] == fixture.release_tip.as_str()));
        assert!(
            !fs::read_to_string(path.join("scaffold.json"))?.contains(CLASSIFICATIONS[0]),
            "the scaffold must not invent a terminal disposition"
        );

        let check_config = CheckConfig {
            source: fixture.swarm_tip.clone(),
            boundary: fixture.base.clone(),
            target: fixture.release_tip.clone(),
            ledger: path.join("scaffold.json"),
            receipt: path.join("receipt.json"),
            working_directory: Some(path.to_path_buf()),
        };
        let error = match check(check_config) {
            Err(error) => error,
            Ok(()) => return Err(eyre!("an unclassified scaffold must not pass")),
        };
        let message = format!("{error:#}");
        assert!(message.contains("blocked"), "{message}");
        let receipt = receipt_value(path)?;
        assert_eq!(receipt["verdict"], "blocked");
        assert_eq!(receipt["unresolved_commits"].as_array().map(Vec::len), Some(2));
        Ok(())
    }

    fn run_check_expect_error(fixture: &DivergedFixture, path: &Path) -> Result<Report> {
        run_check_expect_error_at(fixture, path, &fixture.release_tip)
    }

    fn run_check_expect_error_at(
        fixture: &DivergedFixture,
        path: &Path,
        target: &str,
    ) -> Result<Report> {
        let config = CheckConfig {
            source: fixture.swarm_tip.clone(),
            boundary: fixture.base.clone(),
            target: target.to_string(),
            ledger: path.join("ledger.json"),
            receipt: path.join("receipt.json"),
            working_directory: Some(path.to_path_buf()),
        };
        match check(config) {
            Err(error) => Ok(error),
            Ok(()) => Err(eyre!("expected the reconciliation check to fail")),
        }
    }

    fn receipt_value(path: &Path) -> Result<serde_json::Value> {
        Ok(serde_json::from_str(&fs::read_to_string(path.join("receipt.json"))?)?)
    }

    fn receipt_error_text(receipt: &serde_json::Value) -> String {
        receipt["errors"]
            .as_array()
            .map(|errors| {
                errors.iter().filter_map(|value| value.as_str()).collect::<Vec<_>>().join("; ")
            })
            .unwrap_or_default()
    }

    fn fixture_population_at(fixture: &DivergedFixture, target: &str) -> Result<Vec<CherryCommit>> {
        let shas = resolved(fixture.swarm_tip.clone(), fixture.base.clone(), target.to_string());
        comparison_population(&shas, Some(fixture.directory.path()))
    }

    fn fixture_digest(fixture: &DivergedFixture) -> Result<String> {
        fixture_digest_at(fixture, &fixture.release_tip)
    }

    fn fixture_digest_at(fixture: &DivergedFixture, target: &str) -> Result<String> {
        Ok(compute_population_digest(&fixture_population_at(fixture, target)?))
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

    /// A diverged fixture plus one release commit touching real product code,
    /// for the lineage-only-over-product negative.
    fn diverged_fixture_with_product_commit() -> Result<DivergedFixture> {
        let fixture = diverged_fixture()?;
        let path = fixture.directory.path();
        run_git_fixture(path, &["checkout", "--quiet", "main"])?;
        commit_file(path, "src/helper.rs", "helper\n", "release r3 runtime")?;
        Ok(fixture)
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
        if let Some(parent) = directory.join(name).parent() {
            fs::create_dir_all(parent)?;
        }
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
