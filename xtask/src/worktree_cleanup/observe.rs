use super::model::{
    Observation, ObservationState, PlanSummary, PrMatch, ProposedAction, RepositorySubject,
    WORKTREE_CLEANUP_POLICY_VERSION, WORKTREE_CLEANUP_SCHEMA_VERSION, WorktreeActionKind,
    WorktreeClassification, WorktreeCleanupPlan, WorktreeFacts, WorktreePlanEntry,
};
use chrono::{SecondsFormat, Utc};
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::str;

const MAX_INSTRUMENT_DETAIL_BYTES: usize = 512;

#[derive(Debug, Clone)]
pub struct InspectOptions {
    pub git_program: PathBuf,
    pub gh_program: PathBuf,
}

impl Default for InspectOptions {
    fn default() -> Self {
        let gh_program = std::env::var_os("XTASK_WORKTREE_CLEANUP_GH_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("gh"));
        Self {
            git_program: PathBuf::from("git"),
            gh_program,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RawWorktree {
    path: PathBuf,
    head: Option<String>,
    branch: Option<String>,
    locked: bool,
    lock_reason: Option<String>,
    prunable_reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct AdministrativeIndex {
    by_worktree_path: BTreeMap<String, PathBuf>,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
struct Decision {
    classification: WorktreeClassification,
    proposed_action: Option<ProposedAction>,
    reason_tokens: Vec<String>,
    required_preconditions: Vec<String>,
}

#[derive(Debug, Clone)]
struct UnpushedObservation {
    observation: Observation<bool>,
    comparison_ref: Option<String>,
    ahead_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GhPr {
    number: u64,
    #[serde(rename = "headRefOid")]
    head_ref_oid: Option<String>,
}

pub fn inspect(root: &Path) -> Result<WorktreeCleanupPlan> {
    let observed_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    inspect_with_options(root, &observed_at, &InspectOptions::default())
}

pub fn inspect_with_options(
    root: &Path,
    observed_at: &str,
    options: &InspectOptions,
) -> Result<WorktreeCleanupPlan> {
    let requested_root = canonicalize_existing(root)
        .wrap_err_with(|| format!("resolving requested repository root {}", root.display()))?;
    let repository_root = resolve_git_path(
        &requested_root,
        &required_git_stdout(options, &requested_root, &["rev-parse", "--show-toplevel"])
            .wrap_err("resolving repository root")?,
    )?;
    let common_dir = resolve_git_path(
        &requested_root,
        &required_git_stdout(options, &requested_root, &["rev-parse", "--git-common-dir"])
            .wrap_err("resolving git common directory")?,
    )?;
    let source_head = observe_git_stdout(options, &requested_root, &["rev-parse", "HEAD"]);

    let list = required_git_output(
        options,
        &requested_root,
        &["worktree", "list", "--porcelain", "-z"],
    )
    .wrap_err("listing registered worktrees")?;
    let raw_entries = parse_worktree_list(&list.stdout)?;
    if raw_entries.is_empty() {
        bail!("git worktree list returned no registered worktrees");
    }

    let primary_root = raw_entries
        .first()
        .map(|entry| entry.path.clone())
        .ok_or_else(|| color_eyre::eyre::eyre!("git worktree list returned no primary worktree"))?;
    let managed_root = primary_root.join(".claude").join("worktrees");
    let administrative_index = scan_administrative_records(&common_dir);
    let mut entries = Vec::with_capacity(raw_entries.len());

    for (index, raw_entry) in raw_entries.iter().enumerate() {
        entries.push(observe_entry(
            raw_entry,
            index == 0,
            &managed_root,
            &repository_root,
            &administrative_index,
            options,
        ));
    }

    entries.sort_by(|left, right| {
        normalized_path_key(&left.path)
            .cmp(&normalized_path_key(&right.path))
            .then_with(|| left.entry_id.cmp(&right.entry_id))
    });

    let subject = RepositorySubject {
        requested_root,
        repository_root,
        common_dir,
        source_head,
    };
    let summary = PlanSummary::from_entries(&entries);
    let aggregate_classification = WorktreeCleanupPlan::aggregate(&entries);
    let plan_digest = plan_digest(&subject, &entries, &summary, aggregate_classification)?;

    Ok(WorktreeCleanupPlan {
        schema_version: WORKTREE_CLEANUP_SCHEMA_VERSION.to_string(),
        policy_version: WORKTREE_CLEANUP_POLICY_VERSION.to_string(),
        observed_at: observed_at.to_string(),
        subject,
        entries,
        summary,
        aggregate_classification,
        plan_digest,
    })
}

fn observe_entry(
    raw_entry: &RawWorktree,
    primary: bool,
    managed_root: &Path,
    repository_root: &Path,
    administrative_index: &AdministrativeIndex,
    options: &InspectOptions,
) -> WorktreePlanEntry {
    let managed = path_starts_with(&raw_entry.path, managed_root);
    let path_exists = observe_path_exists(&raw_entry.path);
    let administrative_path = administrative_observation(raw_entry, primary, administrative_index);
    let mut facts = WorktreeFacts {
        path_exists,
        administrative_path,
        locked: raw_entry.locked,
        lock_reason: raw_entry.lock_reason.clone(),
        prunable_reason: raw_entry.prunable_reason.clone(),
        dirty: Observation::not_applicable("status not selected"),
        untracked: Observation::not_applicable("status not selected"),
        open_pr: Observation::not_applicable("PR ownership not selected"),
        merged_pr: Observation::not_applicable("merged-PR evidence not selected"),
        unpushed_commits: Observation::not_applicable("unpushed evidence not selected"),
        unpushed_comparison_ref: None,
        unpushed_ahead_count: None,
    };

    if managed && !primary && observed_bool(&facts.path_exists) == Some(true) && !facts.locked {
        let (dirty, untracked) = observe_status(options, &raw_entry.path);
        facts.dirty = dirty;
        facts.untracked = untracked;

        if observed_bool(&facts.dirty) == Some(false)
            && observed_bool(&facts.untracked) == Some(false)
        {
            if let Some(branch) = raw_entry.branch.as_deref() {
                facts.open_pr = observe_pr(options, repository_root, branch, "open");
                if matches!(observed_pr(&facts.open_pr), Some(PrMatch::None)) {
                    facts.merged_pr = observe_pr(options, repository_root, branch, "merged");
                    if matches!(observed_pr(&facts.merged_pr), Some(PrMatch::None)) {
                        let unpushed = observe_unpushed(options, &raw_entry.path);
                        facts.unpushed_commits = unpushed.observation;
                        facts.unpushed_comparison_ref = unpushed.comparison_ref;
                        facts.unpushed_ahead_count = unpushed.ahead_count;
                    }
                }
            }
        }
    }

    let decision = decide_entry(raw_entry, primary, managed, &facts);
    let entry_id = entry_id(raw_entry, primary, managed, &facts);

    WorktreePlanEntry {
        entry_id,
        path: raw_entry.path.clone(),
        managed,
        primary,
        branch: raw_entry.branch.clone(),
        head: raw_entry.head.clone(),
        facts,
        classification: decision.classification,
        proposed_action: decision.proposed_action,
        reason_tokens: decision.reason_tokens,
        required_preconditions: decision.required_preconditions,
    }
}

fn decide_entry(
    raw_entry: &RawWorktree,
    primary: bool,
    managed: bool,
    facts: &WorktreeFacts,
) -> Decision {
    if primary {
        return decision(WorktreeClassification::Keep, "primary_worktree");
    }
    if !managed {
        return decision(WorktreeClassification::Keep, "outside_managed_root");
    }

    match observed_bool(&facts.path_exists) {
        None if facts.path_exists.state == ObservationState::NotProven => {
            return decision(
                WorktreeClassification::NotProven,
                "path_existence_not_proven",
            );
        }
        Some(false) => return missing_path_decision(facts),
        Some(true) => {}
        None => {
            return decision(
                WorktreeClassification::NotProven,
                "path_existence_not_proven",
            );
        }
    }

    if facts.locked {
        return decision(WorktreeClassification::Keep, "worktree_locked");
    }
    if facts.prunable_reason.is_some() {
        return decision(
            WorktreeClassification::Review,
            "prunable_while_path_exists",
        );
    }
    if facts.dirty.state == ObservationState::NotProven
        || facts.untracked.state == ObservationState::NotProven
    {
        return decision(WorktreeClassification::NotProven, "status_not_proven");
    }
    if observed_bool(&facts.dirty) == Some(true) || observed_bool(&facts.untracked) == Some(true) {
        let mut result = decision(WorktreeClassification::Salvage, "worktree_dirty");
        if observed_bool(&facts.untracked) == Some(true) {
            result
                .reason_tokens
                .push("untracked_work_present".to_string());
        }
        return result;
    }
    if raw_entry.branch.is_none() {
        return decision(WorktreeClassification::Review, "detached_head");
    }

    if facts.open_pr.state == ObservationState::NotProven {
        return decision(
            WorktreeClassification::NotProven,
            "open_pr_not_proven",
        );
    }
    if matches!(observed_pr(&facts.open_pr), Some(PrMatch::Match { .. })) {
        return decision(WorktreeClassification::Keep, "open_pr_present");
    }
    if facts.merged_pr.state == ObservationState::NotProven {
        return decision(
            WorktreeClassification::NotProven,
            "merged_pr_not_proven",
        );
    }

    if let Some(PrMatch::Match { head_oid, .. }) = observed_pr(&facts.merged_pr) {
        return match (raw_entry.head.as_deref(), head_oid.as_deref()) {
            (Some(local_head), Some(merged_head)) if local_head == merged_head => {
                cache_only_decision(raw_entry, facts, "merged_pr_at_current_head")
            }
            (Some(_), Some(_)) => {
                decision(WorktreeClassification::Keep, "head_moved_after_merged_pr")
            }
            _ => decision(
                WorktreeClassification::NotProven,
                "merged_head_not_proven",
            ),
        };
    }

    if facts.unpushed_commits.state == ObservationState::NotProven {
        return decision(
            WorktreeClassification::NotProven,
            "unpushed_state_not_proven",
        );
    }
    match observed_bool(&facts.unpushed_commits) {
        Some(true) => decision(
            WorktreeClassification::Salvage,
            "unpushed_commits_present",
        ),
        Some(false) => cache_only_decision(raw_entry, facts, "pushed_clean_worktree"),
        None => decision(
            WorktreeClassification::NotProven,
            "unpushed_state_not_proven",
        ),
    }
}

fn missing_path_decision(facts: &WorktreeFacts) -> Decision {
    match facts.administrative_path.state {
        ObservationState::Observed => {
            let Some(target) = facts.administrative_path.value.clone() else {
                return decision(
                    WorktreeClassification::NotProven,
                    "administrative_record_not_proven",
                );
            };
            Decision {
                classification: WorktreeClassification::Review,
                proposed_action: Some(ProposedAction {
                    kind: WorktreeActionKind::PruneAdministrativeRecord,
                    target,
                    targetable: false,
                }),
                reason_tokens: vec![
                    "worktree_path_missing".to_string(),
                    "administrative_cleanup_requires_review".to_string(),
                ],
                required_preconditions: vec![
                    "repository_identity_matches".to_string(),
                    "administrative_record_still_matches".to_string(),
                    "worktree_path_still_missing".to_string(),
                    "targetable_git_primitive_available".to_string(),
                ],
            }
        }
        ObservationState::NotProven | ObservationState::NotApplicable => decision(
            WorktreeClassification::NotProven,
            "administrative_record_not_proven",
        ),
    }
}

fn cache_only_decision(
    raw_entry: &RawWorktree,
    facts: &WorktreeFacts,
    reason: &str,
) -> Decision {
    if facts.administrative_path.state != ObservationState::Observed
        || facts.administrative_path.value.is_none()
    {
        return decision(
            WorktreeClassification::NotProven,
            "administrative_record_not_proven",
        );
    }

    Decision {
        classification: WorktreeClassification::CacheOnly,
        proposed_action: Some(ProposedAction {
            kind: WorktreeActionKind::RemoveRegisteredWorktree,
            target: raw_entry.path.clone(),
            targetable: true,
        }),
        reason_tokens: vec![
            reason.to_string(),
            "registered_worktree_removal_candidate".to_string(),
        ],
        required_preconditions: vec![
            "repository_identity_matches".to_string(),
            "entry_registration_matches".to_string(),
            "worktree_path_exists".to_string(),
            "branch_and_head_match".to_string(),
            "worktree_unlocked".to_string(),
            "worktree_clean".to_string(),
            "no_untracked_work".to_string(),
            "remote_premise_current".to_string(),
        ],
    }
}

fn decision(classification: WorktreeClassification, reason: &str) -> Decision {
    Decision {
        classification,
        proposed_action: None,
        reason_tokens: vec![reason.to_string()],
        required_preconditions: Vec::new(),
    }
}

fn observed_bool(observation: &Observation<bool>) -> Option<bool> {
    if observation.state == ObservationState::Observed {
        observation.value
    } else {
        None
    }
}

fn observed_pr(observation: &Observation<PrMatch>) -> Option<&PrMatch> {
    if observation.state == ObservationState::Observed {
        observation.value.as_ref()
    } else {
        None
    }
}

fn observe_path_exists(path: &Path) -> Observation<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Observation::observed(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Observation::observed(false)
        }
        Err(error) => Observation::not_proven(format!(
            "could not inspect worktree path {}: {error}",
            path.display()
        )),
    }
}

fn administrative_observation(
    raw_entry: &RawWorktree,
    primary: bool,
    index: &AdministrativeIndex,
) -> Observation<PathBuf> {
    if primary {
        return Observation::not_applicable("primary worktree uses the common git directory");
    }

    let key = normalized_path_key(&raw_entry.path);
    if let Some(path) = index.by_worktree_path.get(&key) {
        return Observation::observed(path.clone());
    }
    if index.errors.is_empty() {
        Observation::not_proven(format!(
            "no administrative record mapped to registered worktree {}",
            raw_entry.path.display()
        ))
    } else {
        Observation::not_proven(format!(
            "administrative record for {} was not proven: {}",
            raw_entry.path.display(),
            index.errors.join("; ")
        ))
    }
}

fn observe_status(
    options: &InspectOptions,
    path: &Path,
) -> (Observation<bool>, Observation<bool>) {
    let output = run_read_only_git(
        options,
        path,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    );
    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            let detail = command_failure_detail("git status", &output);
            return (
                Observation::not_proven(detail.clone()),
                Observation::not_proven(detail),
            );
        }
        Err(error) => {
            let detail = format!("git status instrument failed: {error}");
            return (
                Observation::not_proven(detail.clone()),
                Observation::not_proven(detail),
            );
        }
    };

    let mut dirty = false;
    let mut untracked = false;
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        dirty = true;
        if record.starts_with(b"??") {
            untracked = true;
        }
    }
    (
        Observation::observed(dirty),
        Observation::observed(untracked),
    )
}

fn observe_pr(
    options: &InspectOptions,
    repository_root: &Path,
    branch: &str,
    state: &str,
) -> Observation<PrMatch> {
    let output = Command::new(&options.gh_program)
        .current_dir(repository_root)
        .args([
            "pr",
            "list",
            "--head",
            branch,
            "--state",
            state,
            "--limit",
            "1",
            "--json",
            "number,headRefOid",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            return Observation::not_proven(command_failure_detail(
                &format!("gh pr list --state {state}"),
                &output,
            ));
        }
        Err(error) => {
            return Observation::not_proven(format!(
                "gh PR instrument failed for branch {branch}: {error}"
            ));
        }
    };

    match serde_json::from_slice::<Vec<GhPr>>(&output.stdout) {
        Ok(rows) => match rows.into_iter().next() {
            Some(row) => Observation::observed(PrMatch::Match {
                number: row.number,
                head_oid: row.head_ref_oid,
            }),
            None => Observation::observed(PrMatch::None),
        },
        Err(error) => Observation::not_proven(format!(
            "gh PR response for branch {branch} was not valid JSON: {error}"
        )),
    }
}

fn observe_unpushed(options: &InspectOptions, path: &Path) -> UnpushedObservation {
    let comparison_ref = match resolve_upstream(options, path) {
        Ok(Some(reference)) => Some(reference),
        Ok(None) => match resolve_default_remote_ref(options, path) {
            Ok(reference) => reference,
            Err(error) => {
                return UnpushedObservation {
                    observation: Observation::not_proven(format!(
                        "default remote reference could not be inspected: {error}"
                    )),
                    comparison_ref: None,
                    ahead_count: None,
                };
            }
        },
        Err(error) => {
            return UnpushedObservation {
                observation: Observation::not_proven(format!(
                    "upstream reference could not be inspected: {error}"
                )),
                comparison_ref: None,
                ahead_count: None,
            };
        }
    };

    let Some(comparison_ref) = comparison_ref else {
        return UnpushedObservation {
            observation: Observation::not_proven(
                "no upstream or canonical remote branch could be resolved",
            ),
            comparison_ref: None,
            ahead_count: None,
        };
    };

    let range = format!("{comparison_ref}..HEAD");
    let output = run_read_only_git(options, path, &["rev-list", "--count", &range]);
    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            return UnpushedObservation {
                observation: Observation::not_proven(command_failure_detail(
                    "git rev-list --count",
                    &output,
                )),
                comparison_ref: Some(comparison_ref),
                ahead_count: None,
            };
        }
        Err(error) => {
            return UnpushedObservation {
                observation: Observation::not_proven(format!(
                    "git rev-list instrument failed: {error}"
                )),
                comparison_ref: Some(comparison_ref),
                ahead_count: None,
            };
        }
    };

    let text = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();
    match text.parse::<u64>() {
        Ok(count) => UnpushedObservation {
            observation: Observation::observed(count > 0),
            comparison_ref: Some(comparison_ref),
            ahead_count: Some(count),
        },
        Err(error) => UnpushedObservation {
            observation: Observation::not_proven(format!(
                "git rev-list returned invalid count {text:?}: {error}"
            )),
            comparison_ref: Some(comparison_ref),
            ahead_count: None,
        },
    }
}

fn resolve_upstream(options: &InspectOptions, path: &Path) -> Result<Option<String>> {
    let output = run_read_only_git(
        options,
        path,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{u}",
        ],
    )?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn resolve_default_remote_ref(options: &InspectOptions, path: &Path) -> Result<Option<String>> {
    for candidate in ["origin/main", "origin/master"] {
        let full_ref = format!("refs/remotes/{candidate}");
        let output = run_read_only_git(
            options,
            path,
            &["rev-parse", "--verify", "--quiet", &full_ref],
        )?;
        if output.status.success() {
            return Ok(Some(candidate.to_string()));
        }
    }
    Ok(None)
}

fn scan_administrative_records(common_dir: &Path) -> AdministrativeIndex {
    let mut index = AdministrativeIndex::default();
    let worktrees_dir = common_dir.join("worktrees");
    let read_dir = match fs::read_dir(&worktrees_dir) {
        Ok(read_dir) => read_dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return index,
        Err(error) => {
            index.errors.push(format!(
                "could not read {}: {error}",
                worktrees_dir.display()
            ));
            return index;
        }
    };

    let mut records = Vec::new();
    for entry in read_dir {
        match entry {
            Ok(entry) => records.push(entry.path()),
            Err(error) => index
                .errors
                .push(format!("could not enumerate administrative record: {error}")),
        }
    }
    records.sort_by_key(|path| normalized_path_key(path));

    for record in records {
        let gitdir_path = record.join("gitdir");
        let raw = match fs::read_to_string(&gitdir_path) {
            Ok(raw) => raw,
            Err(error) => {
                index.errors.push(format!(
                    "could not read {}: {error}",
                    gitdir_path.display()
                ));
                continue;
            }
        };
        let value = raw.trim();
        if value.is_empty() {
            index
                .errors
                .push(format!("{} was empty", gitdir_path.display()));
            continue;
        }
        let linked_gitdir = resolve_reported_path(&record, value);
        let worktree_path = linked_gitdir.parent().map(Path::to_path_buf);
        match worktree_path {
            Some(worktree_path) => {
                index
                    .by_worktree_path
                    .insert(normalized_path_key(&worktree_path), record.clone());
            }
            None => index.errors.push(format!(
                "{} did not identify a worktree path",
                gitdir_path.display()
            )),
        }
    }

    index
}

fn parse_worktree_list(raw: &[u8]) -> Result<Vec<RawWorktree>> {
    let mut entries = Vec::new();
    let mut current: Option<RawWorktree> = None;

    for field in raw.split(|byte| *byte == 0) {
        if field.is_empty() {
            flush_raw_entry(&mut current, &mut entries);
            continue;
        }
        let text = str::from_utf8(field).wrap_err("worktree porcelain was not valid UTF-8")?;
        if let Some(value) = text.strip_prefix("worktree ") {
            flush_raw_entry(&mut current, &mut entries);
            current = Some(RawWorktree {
                path: PathBuf::from(value),
                ..RawWorktree::default()
            });
            continue;
        }

        let Some(entry) = current.as_mut() else {
            bail!("worktree porcelain field appeared before a worktree path: {text:?}");
        };
        if let Some(value) = text.strip_prefix("HEAD ") {
            entry.head = Some(value.to_string());
        } else if let Some(value) = text.strip_prefix("branch ") {
            entry.branch = Some(
                value
                    .strip_prefix("refs/heads/")
                    .unwrap_or(value)
                    .to_string(),
            );
        } else if text == "detached" || text == "bare" {
            entry.branch = None;
        } else if let Some(value) = text.strip_prefix("locked") {
            entry.locked = true;
            let reason = value.strip_prefix(' ').unwrap_or("").trim();
            if !reason.is_empty() {
                entry.lock_reason = Some(reason.to_string());
            }
        } else if let Some(value) = text.strip_prefix("prunable") {
            let reason = value.strip_prefix(' ').unwrap_or("").trim();
            entry.prunable_reason = Some(reason.to_string());
        }
    }
    flush_raw_entry(&mut current, &mut entries);
    Ok(entries)
}

fn flush_raw_entry(current: &mut Option<RawWorktree>, entries: &mut Vec<RawWorktree>) {
    if let Some(entry) = current.take() {
        entries.push(entry);
    }
}

fn required_git_stdout(
    options: &InspectOptions,
    cwd: &Path,
    args: &[&str],
) -> Result<String> {
    let output = required_git_output(options, cwd, args)?;
    let value = str::from_utf8(&output.stdout)
        .wrap_err_with(|| format!("git {} returned non-UTF-8 output", args.join(" ")))?;
    Ok(value.trim().to_string())
}

fn required_git_output(
    options: &InspectOptions,
    cwd: &Path,
    args: &[&str],
) -> Result<Output> {
    let output = run_read_only_git(options, cwd, args)?;
    if !output.status.success() {
        bail!(
            "{}",
            command_failure_detail(&format!("git {}", args.join(" ")), &output)
        );
    }
    Ok(output)
}

fn observe_git_stdout(
    options: &InspectOptions,
    cwd: &Path,
    args: &[&str],
) -> Observation<String> {
    match run_read_only_git(options, cwd, args) {
        Ok(output) if output.status.success() => {
            let value = String::from_utf8_lossy(&output.stdout)
                .trim()
                .to_string();
            if value.is_empty() {
                Observation::not_proven(format!("git {} returned no value", args.join(" ")))
            } else {
                Observation::observed(value)
            }
        }
        Ok(output) => Observation::not_proven(command_failure_detail(
            &format!("git {}", args.join(" ")),
            &output,
        )),
        Err(error) => Observation::not_proven(format!(
            "git {} instrument failed: {error}",
            args.join(" ")
        )),
    }
}

fn run_read_only_git(
    options: &InspectOptions,
    cwd: &Path,
    args: &[&str],
) -> Result<Output> {
    if !is_read_only_git_command(args) {
        bail!(
            "refusing mutating or unknown git command in inspection: git {}",
            args.join(" ")
        );
    }
    Command::new(&options.git_program)
        .current_dir(cwd)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .wrap_err_with(|| format!("running git {} in {}", args.join(" "), cwd.display()))
}

fn is_read_only_git_command(args: &[&str]) -> bool {
    match args {
        ["worktree", "list", ..] => true,
        ["rev-parse", ..] => true,
        ["status", ..] => true,
        ["rev-list", ..] => true,
        _ => false,
    }
}

fn command_failure_detail(command: &str, output: &Output) -> String {
    let stderr = bounded_text(&output.stderr);
    if stderr.is_empty() {
        format!("{command} exited with {}", output.status)
    } else {
        format!("{command} exited with {}: {stderr}", output.status)
    }
}

fn bounded_text(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw).trim().to_string();
    if text.len() <= MAX_INSTRUMENT_DETAIL_BYTES {
        return text;
    }
    let mut boundary = MAX_INSTRUMENT_DETAIL_BYTES;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}…", &text[..boundary])
}

fn resolve_git_path(base: &Path, value: &str) -> Result<PathBuf> {
    let candidate = resolve_reported_path(base, value);
    canonicalize_existing(&candidate)
        .wrap_err_with(|| format!("resolving git path {}", candidate.display()))
}

fn resolve_reported_path(base: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        normalize_lexically(&path)
    } else {
        normalize_lexically(&base.join(path))
    }
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).wrap_err_with(|| format!("canonicalizing {}", path.display()))
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    normalized
}

fn path_starts_with(path: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        normalized_path_key(path)
            .to_ascii_lowercase()
            .starts_with(&normalized_path_key(root).to_ascii_lowercase())
    }
    #[cfg(not(windows))]
    {
        normalize_lexically(path).starts_with(normalize_lexically(root))
    }
}

fn normalized_path_key(path: &Path) -> String {
    normalize_lexically(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn entry_id(
    raw_entry: &RawWorktree,
    primary: bool,
    managed: bool,
    facts: &WorktreeFacts,
) -> String {
    #[derive(Serialize)]
    struct EntryIdentity<'a> {
        path: &'a Path,
        primary: bool,
        managed: bool,
        branch: &'a Option<String>,
        head: &'a Option<String>,
        administrative_path: &'a Observation<PathBuf>,
    }

    let identity = EntryIdentity {
        path: &raw_entry.path,
        primary,
        managed,
        branch: &raw_entry.branch,
        head: &raw_entry.head,
        administrative_path: &facts.administrative_path,
    };
    let digest = serde_json::to_vec(&identity)
        .map(|bytes| sha256_hex(&bytes))
        .unwrap_or_else(|_| sha256_hex(normalized_path_key(&raw_entry.path).as_bytes()));
    let short = digest.get(..16).unwrap_or(&digest);
    format!("wt_{short}")
}

fn plan_digest(
    subject: &RepositorySubject,
    entries: &[WorktreePlanEntry],
    summary: &PlanSummary,
    aggregate_classification: WorktreeClassification,
) -> Result<String> {
    #[derive(Serialize)]
    struct SemanticPlan<'a> {
        schema_version: &'static str,
        policy_version: &'static str,
        subject: &'a RepositorySubject,
        entries: &'a [WorktreePlanEntry],
        summary: &'a PlanSummary,
        aggregate_classification: WorktreeClassification,
    }

    let semantic = SemanticPlan {
        schema_version: WORKTREE_CLEANUP_SCHEMA_VERSION,
        policy_version: WORKTREE_CLEANUP_POLICY_VERSION,
        subject,
        entries,
        summary,
        aggregate_classification,
    };
    let bytes = serde_json::to_vec(&semantic).wrap_err("serializing semantic worktree plan")?;
    Ok(format!("sha256:{}", sha256_hex(&bytes)))
}

fn sha256_hex(raw: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(raw);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed_facts() -> WorktreeFacts {
        WorktreeFacts {
            path_exists: Observation::observed(true),
            administrative_path: Observation::observed(PathBuf::from(
                "/repo/.git/worktrees/a",
            )),
            locked: false,
            lock_reason: None,
            prunable_reason: None,
            dirty: Observation::observed(false),
            untracked: Observation::observed(false),
            open_pr: Observation::observed(PrMatch::None),
            merged_pr: Observation::observed(PrMatch::None),
            unpushed_commits: Observation::observed(false),
            unpushed_comparison_ref: Some("origin/main".to_string()),
            unpushed_ahead_count: Some(0),
        }
    }

    fn raw_entry() -> RawWorktree {
        RawWorktree {
            path: PathBuf::from("/repo/.claude/worktrees/a"),
            head: Some("abc123".to_string()),
            branch: Some("impl/a".to_string()),
            locked: false,
            lock_reason: None,
            prunable_reason: None,
        }
    }

    #[test]
    fn parses_nul_terminated_porcelain() -> Result<()> {
        let raw = b"worktree /repo\0HEAD abc\0branch refs/heads/main\0\0worktree /repo/.claude/worktrees/a\0HEAD def\0branch refs/heads/impl/a\0locked active writer\0prunable gitdir missing\0\0";
        let entries = parse_worktree_list(raw)?;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, PathBuf::from("/repo"));
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
        assert_eq!(entries[1].branch.as_deref(), Some("impl/a"));
        assert!(entries[1].locked);
        assert_eq!(entries[1].lock_reason.as_deref(), Some("active writer"));
        assert_eq!(
            entries[1].prunable_reason.as_deref(),
            Some("gitdir missing")
        );
        Ok(())
    }

    #[test]
    fn inspection_git_allowlist_rejects_mutation() {
        assert!(is_read_only_git_command(&[
            "worktree",
            "list",
            "--porcelain",
            "-z"
        ]));
        assert!(is_read_only_git_command(&["status", "--porcelain=v1"]));
        assert!(is_read_only_git_command(&["rev-parse", "HEAD"]));
        assert!(is_read_only_git_command(&[
            "rev-list",
            "--count",
            "main..HEAD"
        ]));
        assert!(!is_read_only_git_command(&["worktree", "prune"]));
        assert!(!is_read_only_git_command(&[
            "worktree",
            "remove",
            "/tmp/a"
        ]));
        assert!(!is_read_only_git_command(&["branch", "-D", "impl/a"]));
        assert!(!is_read_only_git_command(&[
            "update-ref",
            "-d",
            "refs/heads/a"
        ]));
        assert!(!is_read_only_git_command(&[
            "config",
            "core.bare",
            "false"
        ]));
    }

    #[test]
    fn decision_distinguishes_keep_salvage_review_cache_and_not_proven() {
        let entry = raw_entry();
        let facts = observed_facts();
        assert_eq!(
            decide_entry(&entry, true, true, &facts).classification,
            WorktreeClassification::Keep
        );

        let mut dirty = facts.clone();
        dirty.dirty = Observation::observed(true);
        assert_eq!(
            decide_entry(&entry, false, true, &dirty).classification,
            WorktreeClassification::Salvage
        );

        let mut detached = entry.clone();
        detached.branch = None;
        assert_eq!(
            decide_entry(&detached, false, true, &facts).classification,
            WorktreeClassification::Review
        );

        let cache = decide_entry(&entry, false, true, &facts);
        assert_eq!(cache.classification, WorktreeClassification::CacheOnly);
        assert!(
            cache
                .proposed_action
                .as_ref()
                .is_some_and(|action| action.targetable)
        );

        let mut unknown = facts;
        unknown.open_pr = Observation::not_proven("gh unavailable");
        assert_eq!(
            decide_entry(&entry, false, true, &unknown).classification,
            WorktreeClassification::NotProven
        );
    }

    #[test]
    fn merged_pr_only_authorizes_the_exact_current_head() {
        let entry = raw_entry();
        let mut facts = observed_facts();
        facts.merged_pr = Observation::observed(PrMatch::Match {
            number: 7,
            head_oid: Some("abc123".to_string()),
        });
        facts.unpushed_commits = Observation::not_applicable("merged PR selected");
        assert_eq!(
            decide_entry(&entry, false, true, &facts).classification,
            WorktreeClassification::CacheOnly
        );

        facts.merged_pr = Observation::observed(PrMatch::Match {
            number: 7,
            head_oid: Some("older".to_string()),
        });
        assert_eq!(
            decide_entry(&entry, false, true, &facts).classification,
            WorktreeClassification::Keep
        );
    }

    #[test]
    fn missing_path_is_review_only_and_not_authorized() {
        let entry = raw_entry();
        let mut facts = observed_facts();
        facts.path_exists = Observation::observed(false);
        let result = decide_entry(&entry, false, true, &facts);
        assert_eq!(result.classification, WorktreeClassification::Review);
        let action = result.proposed_action.as_ref();
        assert!(action.is_some_and(|action| {
            action.kind == WorktreeActionKind::PruneAdministrativeRecord && !action.targetable
        }));
    }

    #[test]
    fn semantic_plan_digest_is_deterministic() -> Result<()> {
        let entry = raw_entry();
        let facts = observed_facts();
        let decision = decide_entry(&entry, false, true, &facts);
        let entries = vec![WorktreePlanEntry {
            entry_id: entry_id(&entry, false, true, &facts),
            path: entry.path.clone(),
            managed: true,
            primary: false,
            branch: entry.branch.clone(),
            head: entry.head.clone(),
            facts,
            classification: decision.classification,
            proposed_action: decision.proposed_action,
            reason_tokens: decision.reason_tokens,
            required_preconditions: decision.required_preconditions,
        }];
        let subject = RepositorySubject {
            requested_root: PathBuf::from("/repo"),
            repository_root: PathBuf::from("/repo"),
            common_dir: PathBuf::from("/repo/.git"),
            source_head: Observation::observed("abc123".to_string()),
        };
        let summary = PlanSummary::from_entries(&entries);
        let aggregate = WorktreeCleanupPlan::aggregate(&entries);
        let first = plan_digest(&subject, &entries, &summary, aggregate)?;
        let second = plan_digest(&subject, &entries, &summary, aggregate)?;
        assert_eq!(first, second);
        Ok(())
    }
}
