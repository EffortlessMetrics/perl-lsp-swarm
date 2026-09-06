//! Non-Rust file inventory and policy enforcement for the file-policy rollout.
//!
//! ## Commands
//!
//! - `cargo xtask non-rust inventory` — walks `git ls-files`, classifies
//!   tracked files as Rust or non-Rust, looks each non-Rust file up in
//!   `policy/non-rust-allowlist.toml`, and emits:
//!   - `target/policy/non-rust-inventory.md` — human-readable markdown table.
//!   - `target/policy/non-rust-inventory.json` — machine-readable JSON array.
//!
//!   (Does **not** modify `docs/policy/NON_RUST_INVENTORY.md`.)
//!
//! - `cargo xtask non-rust inventory --write` — runs the inventory scan and
//!   additionally overwrites `docs/policy/NON_RUST_INVENTORY.md` with the
//!   regenerated content. This is the deliberate write path; use it when the
//!   committed snapshot needs to be refreshed.
//!
//! - `cargo xtask non-rust check [--mode <mode>] [--json <path>] [--allowlist <path>]` —
//!   classify tracked files against the allowlist and report violations.
//!   Modes: `advisory` (default, always exit 0), `blocking-allowlist` (exit 1 on
//!   unallowlisted files or expired entries), `blocking-strict` (also fail on stale
//!   `review_after`, duplicate ids, absolute/backslashed paths, broad globs without
//!   `broad_glob_reason`).
//!
//! The inventory is **read-only** — it never mutates the allowlist.
//!
//! Refs: #8174, #8566.

use chrono::{NaiveDate, Utc};
use color_eyre::eyre::{Context, Result, bail, eyre};
use glob::{MatchOptions, Pattern};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

// ---------------------------------------------------------------------------
// Allowlist schema (mirrors `policy/non-rust-allowlist.toml`)
// ---------------------------------------------------------------------------

/// Top-level structure of `policy/non-rust-allowlist.toml`.
#[derive(Debug, Deserialize)]
pub struct Allowlist {
    #[serde(default)]
    pub allow: Vec<AllowEntry>,
}

/// A single `[[allow]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AllowEntry {
    pub id: String,
    /// Glob pattern (mutually exclusive with `path`).
    pub glob: Option<String>,
    /// Exact path (mutually exclusive with `glob`).
    pub path: Option<String>,
    pub kind: String,
    pub language: String,
    pub surface: String,
    pub classification: String,
    pub owner: String,
    pub reason: String,
    #[serde(default)]
    pub covered_by: Vec<String>,
    pub created: String,
    pub review_after: String,
    pub expires: Option<String>,
    pub broad_glob_reason: Option<String>,
    #[serde(default)]
    pub retired: bool,
}

// ---------------------------------------------------------------------------
// Inventory output schema
// ---------------------------------------------------------------------------

/// Classification of a single tracked file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    /// Repo-relative path (forward slashes, no leading `./`).
    pub path: String,
    /// File extension without the leading dot, or empty string for
    /// files with no extension.
    pub extension: String,
    /// `"rust"` for Rust-family files; the allowlist `classification`
    /// value for non-Rust files that are allowlisted; `"unclassified"`
    /// otherwise.
    pub category: String,
    /// Whether the file matches at least one non-retired allowlist entry.
    pub allowlisted: bool,
    /// The first matching allowlist entry, if any.
    pub entry: Option<AllowEntry>,
}

/// Exact-tree policy comparison used by trusted CI. The evaluator is sourced
/// from the trusted checkout; candidate trees are read only as Git objects.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExactTreePolicyReceipt {
    pub schema_version: u32,
    pub event_name: Option<String>,
    pub repository: Option<String>,
    pub base_sha: String,
    pub base_tree_sha: Option<String>,
    pub subject_sha: String,
    pub subject_tree_sha: Option<String>,
    pub pr_head_sha: Option<String>,
    pub base_allowlist_blob_sha: Option<String>,
    pub subject_allowlist_blob_sha: Option<String>,
    pub base_unclassified_count: usize,
    pub subject_unclassified_count: usize,
    pub new_unclassified_paths: Vec<String>,
    pub outcome: String,
    pub failure_stage: Option<String>,
    pub error: Option<String>,
    /// UTC date used when evaluating expiring allowlist entries.
    pub evaluation_date: String,
    pub evaluator_commit: Option<String>,
    pub evaluator_tree: Option<String>,
    pub inventory_markdown_path: Option<String>,
    pub inventory_markdown_size: Option<u64>,
    pub inventory_markdown_sha256: Option<String>,
    pub inventory_json_path: Option<String>,
    pub inventory_json_size: Option<u64>,
    pub inventory_json_sha256: Option<String>,
}

// ---------------------------------------------------------------------------
// Rust-family classifier
// ---------------------------------------------------------------------------

/// Returns `true` when the path is a Rust-family file that does not require
/// an allowlist entry.
pub fn is_rust_file(path: &str) -> bool {
    // Source and build artefacts that are fully Rust-owned.
    if path.ends_with(".rs") {
        return true;
    }
    // Well-known filenames (no extension or fixed name).
    let basename = path.rsplit('/').next().unwrap_or(path);
    matches!(
        basename,
        "Cargo.toml" | "Cargo.lock" | "rust-toolchain.toml" | "clippy.toml" | "rustfmt.toml"
    )
}

// ---------------------------------------------------------------------------
// Allowlist loading and glob matching
// ---------------------------------------------------------------------------

/// Load `policy/non-rust-allowlist.toml` from the workspace root.
pub fn load_allowlist(root: &Path) -> Result<Allowlist> {
    let path = root.join("policy/non-rust-allowlist.toml");
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

struct PreparedAllowEntry<'a> {
    entry: &'a AllowEntry,
    glob: Option<Pattern>,
}

fn prepare_allow_entries(entries: &[AllowEntry]) -> Vec<PreparedAllowEntry<'_>> {
    prepare_allow_entries_at(entries, Utc::now().date_naive())
}

fn prepare_allow_entries_at(
    entries: &[AllowEntry],
    evaluation_date: NaiveDate,
) -> Vec<PreparedAllowEntry<'_>> {
    let mut prepared = Vec::new();
    for entry in entries {
        if entry.retired
            || entry
                .expires
                .as_deref()
                .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
                .is_some_and(|expires| expires < evaluation_date)
        {
            continue;
        }
        let glob = match entry.glob.as_deref() {
            Some(glob_str) => match Pattern::new(glob_str) {
                Ok(pattern) => Some(pattern),
                Err(_) => continue,
            },
            None => None,
        };
        prepared.push(PreparedAllowEntry { entry, glob });
    }
    prepared
}

fn validate_exact_allow_entries(entries: &[AllowEntry]) -> Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    for entry in entries.iter().filter(|entry| !entry.retired) {
        if !ids.insert(&entry.id) {
            bail!("duplicate allowlist entry id {}", entry.id);
        }
        if entry.kind.trim().is_empty()
            || entry.language.trim().is_empty()
            || entry.surface.trim().is_empty()
            || entry.classification.trim().is_empty()
            || entry.owner.trim().is_empty()
            || entry.reason.trim().is_empty()
        {
            bail!("allowlist entry {} has empty required metadata", entry.id);
        }
        match (entry.glob.as_deref(), entry.path.as_deref()) {
            (Some(_), Some(_)) => bail!("allowlist entry {} sets both glob and path", entry.id),
            (None, None) => bail!("allowlist entry {} has no glob or path", entry.id),
            (Some(pattern), None) => {
                Pattern::new(pattern)
                    .with_context(|| format!("invalid glob in allowlist entry {}", entry.id))?;
            }
            (None, Some(path)) if path.starts_with('/') || path.contains('\\') => {
                bail!("invalid path in allowlist entry {}", entry.id)
            }
            (None, Some(_)) => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Entry-level coherence (#9680/#9993)
// ---------------------------------------------------------------------------

/// Git conflict-marker line prefixes. Textual conflict resolution can leave
/// these inside a multi-line TOML string, where the parser still accepts the
/// file; markers left at the top level already fail TOML parsing. Git writes
/// markers at column 0, so the raw line prefix is matched and indented or
/// commented mentions are not flagged.
const POLICY_CONFLICT_MARKER_PREFIXES: &[&str] = &["<<<<<<< ", ">>>>>>> ", "||||||| "];

fn is_policy_conflict_marker(line: &str) -> bool {
    line == "======="
        || POLICY_CONFLICT_MARKER_PREFIXES.iter().any(|prefix| line.starts_with(prefix))
}

/// Return 1-based line numbers that begin a Git conflict marker.
fn policy_conflict_marker_lines(text: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| is_policy_conflict_marker(line))
        .map(|(index, _)| index + 1)
        .collect()
}

/// Shared wording for the mispaired-provenance finding so the raw-bytes,
/// policy-table, and typed entry surfaces report one identical message shape.
fn mispaired_provenance_message(
    first_id: &str,
    first_matcher: &str,
    id: &str,
    matcher: &str,
) -> String {
    format!(
        "entries {first_id:?} and {id:?} carry an identical `reason` under different matchers ({first_matcher:?} vs {matcher:?}); provenance is mispaired"
    )
}

/// Detect the mispaired-provenance shape produced by textual conflict
/// resolution (#9680): two non-retired entries with different matchers that
/// carry a byte-identical `reason`. The file stays valid TOML with green
/// schema, duplicate, and coverage checks while its identity join is wrong.
///
/// The allowlist schema carries no issue/authority field beyond `id` today,
/// so the subject join is derived from the identity-bearing `reason` field
/// rather than from prose structure. Retired entries are exempt: a
/// historical row may deliberately record the disposition of an accepted
/// entry without being live policy.
fn mispaired_provenance_conflicts(entries: &[&toml::map::Map<String, toml::Value>]) -> Vec<String> {
    let mut conflicts = Vec::new();
    let mut seen: std::collections::BTreeMap<&str, (&str, &str)> =
        std::collections::BTreeMap::new();
    for entry in entries {
        let retired = entry.get("retired").and_then(toml::Value::as_bool).unwrap_or(false);
        if retired {
            continue;
        }
        let Some(reason) = entry.get("reason").and_then(toml::Value::as_str) else {
            continue;
        };
        if reason.trim().is_empty() {
            continue;
        }
        let id = entry.get("id").and_then(toml::Value::as_str).unwrap_or("<unnamed>");
        let matcher = entry
            .get("glob")
            .or_else(|| entry.get("path"))
            .and_then(toml::Value::as_str)
            .unwrap_or("<none>");
        match seen.entry(reason) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert((id, matcher));
            }
            std::collections::btree_map::Entry::Occupied(slot) => {
                let (first_id, first_matcher) = slot.get();
                if *first_matcher != matcher {
                    conflicts.push(mispaired_provenance_message(
                        first_id,
                        first_matcher,
                        id,
                        matcher,
                    ));
                }
            }
        }
    }
    conflicts
}

fn validate_exact_policy_bytes(policy: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(policy).context("allowlist is not UTF-8")?;
    let marker_lines = policy_conflict_marker_lines(text);
    if !marker_lines.is_empty() {
        bail!("allowlist contains Git conflict markers at lines {marker_lines:?}");
    }
    let value: toml::Value = toml::from_str(text).context("parsing allowlist policy")?;
    let entries = value
        .get("allow")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| eyre!("allowlist must define an allow array"))?;
    let mut errors = Vec::new();
    validate_allow_document_entries(entries, &mut errors);
    if !errors.is_empty() {
        bail!("{}", errors.join("; "));
    }
    Ok(())
}

/// Match options for every allowlist glob.
///
/// `require_literal_separator` stops `*` and `?` at `/`, so a single-segment
/// matcher governs exactly the directory it names: `.changes/unreleased/*.yaml`
/// covers the fragments sitting directly in that directory, and a nested path
/// such as `.changes/unreleased/archive/old.yaml` cannot inherit the entry.
/// Entries that genuinely own a tree opt in explicitly with `**`.
///
/// The `glob` crate's default (used by the bare `Pattern::matches`) lets `*`
/// cross `/`, which silently widens every single-segment entry into a
/// whole-tree entry and contradicts the documented schema in
/// `docs/policy/NON_RUST_POLICY.md`. Refs: #9994.
const POLICY_MATCH_OPTIONS: MatchOptions = MatchOptions {
    case_sensitive: true,
    require_literal_separator: true,
    require_literal_leading_dot: false,
};

/// Match one repo-relative path against an allowlist glob.
///
/// Every allowlist matcher goes through here so glob breadth is decided in one
/// place instead of per call site.
fn glob_matches_path(pattern: &Pattern, path: &str) -> bool {
    pattern.matches_with(path, POLICY_MATCH_OPTIONS)
}

fn find_matching_prepared_entry<'a>(
    file_path: &str,
    entries: &[PreparedAllowEntry<'a>],
) -> Option<&'a AllowEntry> {
    for prepared in entries {
        let matched = if let Some(pattern) = prepared.glob.as_ref() {
            glob_matches_path(pattern, file_path)
        } else if let Some(ref exact) = prepared.entry.path {
            exact == file_path
        } else {
            false
        };
        if matched {
            return Some(prepared.entry);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// git ls-files
// ---------------------------------------------------------------------------

/// Run `git ls-files` from `root` and return a sorted list of repo-relative
/// paths (forward slashes, no leading `./`).
pub fn list_tracked_files(root: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .with_context(|| "running `git ls-files -z`")?;
    if !output.status.success() {
        return Err(eyre!("`git ls-files -z` failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    let mut files: Vec<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            let path = String::from_utf8(path.to_vec())
                .with_context(|| "`git ls-files -z` produced a non-UTF-8 path")?;
            Ok(path.trim_start_matches("./").replace('\\', "/"))
        })
        .collect::<Result<_>>()?;
    files.sort_unstable();
    files.dedup();
    Ok(files)
}

fn git_object(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr)
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("no diagnostic")
            .chars()
            .take(240)
            .collect::<String>();
        bail!("git {} failed: {detail}", args.join(" "));
    }
    Ok(output.stdout)
}

fn tree_paths(root: &Path, sha: &str) -> Result<Vec<String>> {
    let raw = git_object(root, &["ls-tree", "-r", "-z", "--name-only", sha])?;
    let mut paths = raw
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8(p.to_vec()).context("tree contains a non-UTF-8 path"))
        .collect::<Result<Vec<_>>>()?;
    paths.sort();
    if paths.windows(2).any(|pair| pair[0] == pair[1]) {
        bail!("tree {sha} contains duplicate paths");
    }
    Ok(paths)
}

fn tree_file(root: &Path, sha: &str, path: &str) -> Result<(String, Vec<u8>)> {
    let spec = format!("{sha}:{path}");
    let bytes = git_object(root, &["show", &spec])?;
    let listing = git_object(root, &["ls-tree", sha, "--", path])?;
    let text = String::from_utf8(listing).context("tree listing is not UTF-8")?;
    let object_sha = text
        .split_whitespace()
        .nth(2)
        .ok_or_else(|| eyre!("tree {sha} does not contain {path}"))?;
    Ok((object_sha.to_string(), bytes))
}

fn classify_tree_at(
    root: &Path,
    sha: &str,
    evaluation_date: NaiveDate,
) -> Result<(Vec<FileRecord>, String)> {
    let (blob_sha, policy) = tree_file(root, sha, "policy/non-rust-allowlist.toml")?;
    validate_exact_policy_bytes(&policy)?;
    let allowlist: Allowlist =
        toml::from_str(std::str::from_utf8(&policy).context("allowlist is not UTF-8")?)
            .with_context(|| format!("parsing policy/non-rust-allowlist.toml from {sha}"))?;
    validate_exact_allow_entries(&allowlist.allow)?;
    let prepared = prepare_allow_entries_at(&allowlist.allow, evaluation_date);
    let records = tree_paths(root, sha)?
        .iter()
        .map(|path| classify_file_with_prepared(path, &prepared))
        .collect::<Vec<_>>();
    Ok((records, blob_sha))
}

fn expired_paths_in_tree(
    root: &Path,
    sha: &str,
    evaluation_date: NaiveDate,
) -> Result<Vec<String>> {
    let (_, policy) = tree_file(root, sha, "policy/non-rust-allowlist.toml")?;
    let allowlist: Allowlist = toml::from_str(std::str::from_utf8(&policy)?)?;
    let expired = allowlist
        .allow
        .iter()
        .filter(|entry| {
            !entry.retired
                && entry
                    .expires
                    .as_deref()
                    .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
                    .is_some_and(|date| date < evaluation_date)
        })
        .filter_map(|entry| {
            if let Some(glob) = entry.glob.as_deref() {
                Pattern::new(glob).ok().map(|pattern| (Some(pattern), None, entry.id.as_str()))
            } else {
                entry.path.as_deref().map(|path| (None, Some(path), entry.id.as_str()))
            }
        })
        .collect::<Vec<_>>();
    let active = prepare_allow_entries_at(&allowlist.allow, evaluation_date);
    let mut paths = tree_paths(root, sha)?
        .into_iter()
        .filter(|path| {
            !is_rust_file(path)
                && expired.iter().any(|(glob, exact, _)| {
                    glob.as_ref().is_some_and(|pattern| glob_matches_path(pattern, path))
                        || exact.is_some_and(|candidate| candidate == path)
                })
                && find_matching_prepared_entry(path, &active).is_none()
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn validate_subject_workflow(root: &Path, base_sha: &str, subject_sha: &str) -> Result<()> {
    let base_listing =
        git_object(root, &["ls-tree", base_sha, "--", ".github/workflows/non-rust-policy.yml"])?;
    let listing =
        git_object(root, &["ls-tree", subject_sha, "--", ".github/workflows/non-rust-policy.yml"])?;
    if listing.is_empty() && base_listing.is_empty() {
        return Ok(());
    }
    if listing.is_empty() {
        bail!("subject workflow removes the trusted exact-tree policy workflow");
    }
    let (_, bytes) = tree_file(root, subject_sha, ".github/workflows/non-rust-policy.yml")?;
    let subject_matches_base = if !base_listing.is_empty() {
        let (_, base_bytes) = tree_file(root, base_sha, ".github/workflows/non-rust-policy.yml")?;
        base_bytes == bytes
    } else {
        false
    };
    let contract_version = |text: &str| -> Result<u64> {
        text.lines()
            .find_map(|line| line.trim().strip_prefix("# contract-version:"))
            .map(str::trim)
            .ok_or_else(|| eyre!("trusted workflow is missing # contract-version metadata"))?
            .parse::<u64>()
            .context("trusted workflow contract-version must be an integer")
    };
    let text = String::from_utf8(bytes).context("subject workflow is not UTF-8")?;
    if !base_listing.is_empty() {
        let (_, base_bytes) = tree_file(root, base_sha, ".github/workflows/non-rust-policy.yml")?;
        let base_text = String::from_utf8(base_bytes).context("base workflow is not UTF-8")?;
        let base_version = contract_version(&base_text)?;
        let subject_version = contract_version(&text)?;
        if !subject_matches_base && subject_version != base_version.saturating_add(1) {
            bail!(
                "trusted workflow changes require exactly one contract-version increment (base {base_version}, subject {subject_version})"
            );
        }
    } else {
        contract_version(&text)?;
    }
    for required in [
        "pull_request_target:",
        "merge_group:",
        "push:",
        "workflow_dispatch:",
        "permissions:\n  contents: read",
        "ref: ${{ env.EVALUATOR_SHA }}",
        "BASE_SHA:",
        "SUBJECT_SHA:",
        "PR_HEAD_SHA:",
        "PR_NUMBER:",
        "merge-base --is-ancestor",
        "SUBJECT_SHA^1",
        "refs/heads/non-rust-policy-subject^{commit}",
        "--base-sha \"$BASE_SHA\"",
        "--subject-sha \"$SUBJECT_SHA\"",
        "Non-Rust policy exact-tree",
        "persist-credentials: false",
        "actions/upload-artifact@",
        "if: always()",
    ] {
        if !text.contains(required) {
            bail!("subject workflow weakens trusted contract: missing {required}");
        }
    }
    if text.contains("actions/checkout@") && !text.contains("ref: ${{ env.EVALUATOR_SHA }}") {
        bail!("subject workflow must checkout the trusted evaluator SHA");
    }
    if text.matches("actions/checkout@").count() != 1 {
        bail!("subject workflow must contain exactly one trusted checkout");
    }
    if text.matches("permissions:").count() != 1 {
        bail!("subject workflow must define exactly one top-level read-only permissions block");
    }
    // Validate the load-bearing steps structurally, so comments or unrelated
    // jobs cannot satisfy the trusted-base contract.
    let yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&text).context("parsing trusted workflow YAML")?;
    let key = |name: &str| serde_yaml_ng::Value::String(name.to_string());
    let jobs = yaml
        .as_mapping()
        .and_then(|mapping| mapping.get(key("jobs")))
        .and_then(serde_yaml_ng::Value::as_mapping)
        .ok_or_else(|| eyre!("trusted workflow must define jobs mapping"))?;
    let job = jobs
        .get(key("exact-tree"))
        .and_then(serde_yaml_ng::Value::as_mapping)
        .ok_or_else(|| eyre!("trusted workflow must define exact-tree job"))?;
    let steps = job
        .get(key("steps"))
        .and_then(serde_yaml_ng::Value::as_sequence)
        .ok_or_else(|| eyre!("exact-tree job must define steps sequence"))?;
    // Inspect executable Cargo commands structurally. The trusted contract
    // step contains this validator's own examples and forbidden-pattern
    // source text; scanning serialized YAML would mistake that source for a
    // candidate-controlled invocation.
    let forbidden = [
        "pull_request.head",
        "pull_request.head.sha",
        "github.event.pull_request.head",
        "refs/pull/${{",
    ];
    let is_contract_step = |step: &&serde_yaml_ng::Value| {
        step.as_mapping().is_some_and(|map| {
            map.get(key("id")).and_then(serde_yaml_ng::Value::as_str)
                == Some("verify-trusted-workflow-contract")
                && map.get(key("name")).and_then(serde_yaml_ng::Value::as_str)
                    == Some("Verify trusted workflow contract")
        })
    };
    let reserved_id_count = steps
        .iter()
        .filter(|step| {
            step.as_mapping()
                .and_then(|map| map.get(key("id")))
                .and_then(serde_yaml_ng::Value::as_str)
                == Some("verify-trusted-workflow-contract")
        })
        .count();
    if reserved_id_count != 1 {
        bail!("trusted workflow must define exactly one reserved contract-verification step ID");
    }
    let contract_steps = steps.iter().filter(is_contract_step).collect::<Vec<_>>();
    if contract_steps.len() != 1 {
        bail!("trusted workflow must define exactly one stable contract-verification step");
    }
    for step in steps {
        let map =
            step.as_mapping().ok_or_else(|| eyre!("trusted workflow step must be a mapping"))?;
        if is_contract_step(&step) {
            continue;
        }
        // Candidate-controlled refs can be smuggled through `env`/`with` and
        // expanded by an otherwise innocuous `run` command. Inspect those
        // executable inputs as well as the command body.
        for key_name in ["env", "with"] {
            if map.get(key(key_name)).is_some_and(|value| {
                serde_yaml_ng::to_string(value)
                    .is_ok_and(|text| forbidden.iter().any(|token| text.contains(token)))
            }) {
                bail!("subject workflow must not execute candidate source");
            }
        }
        let Some(run) = map.get(key("run")).and_then(serde_yaml_ng::Value::as_str) else {
            continue;
        };
        let lines = run.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            let normalized = line.trim();
            let mut words = normalized.split_whitespace();
            if words.next() != Some("cargo") || words.next() != Some("run") {
                continue;
            }
            let mut command = normalized.to_string();
            for continuation in lines.iter().skip(index + 1) {
                let stripped = continuation.trim();
                if stripped.starts_with("--") || command.ends_with('\\') {
                    command.push('\n');
                    command.push_str(stripped);
                } else {
                    break;
                }
            }
            if forbidden.iter().any(|token| command.contains(token)) {
                bail!("subject workflow must not execute candidate source");
            }
        }
    }
    let bind_run = steps
        .iter()
        .find(|step| {
            step.as_mapping()
                .and_then(|map| map.get(key("id")))
                .and_then(serde_yaml_ng::Value::as_str)
                == Some("bind")
        })
        .and_then(|step| step.get(key("run")))
        .and_then(serde_yaml_ng::Value::as_str)
        .ok_or_else(|| eyre!("trusted workflow must define an id: bind run step"))?;
    let bind_run_contains = |needle: &str| bind_run.contains(needle);
    if !steps.iter().any(|step| {
        let Some(map) = step.as_mapping() else { return false };
        map.get(key("uses"))
            .and_then(serde_yaml_ng::Value::as_str)
            .is_some_and(|uses| uses.starts_with("actions/checkout@"))
            && map
                .get(key("with"))
                .and_then(serde_yaml_ng::Value::as_mapping)
                .and_then(|with| with.get(key("ref")))
                .and_then(serde_yaml_ng::Value::as_str)
                .is_some_and(|reference| reference.contains("env.EVALUATOR_SHA"))
    }) {
        bail!("trusted workflow must checkout EVALUATOR_SHA in its checkout step");
    }
    let direct_subject_fetch = bind_run_contains("git fetch --no-tags origin \"$SUBJECT_SHA\"");
    let synthetic_pr_subject = bind_run_contains(
        "refs/pull/$PR_NUMBER/head:refs/remotes/origin/non-rust-policy-pr-head",
    ) && bind_run_contains(
        "git rev-parse refs/remotes/origin/non-rust-policy-pr-head^{commit}",
    ) && bind_run_contains(
        "test \"$(git rev-parse refs/remotes/origin/non-rust-policy-pr-head^{commit})\" = \"$PR_HEAD_SHA\"",
    ) && bind_run_contains(
        "git merge-tree --write-tree \"$BASE_SHA\" \"$PR_HEAD_SHA\"",
    ) && bind_run_contains(
        "git commit-tree \"$merge_tree\" -p \"$BASE_SHA\" -p \"$PR_HEAD_SHA\"",
    ) && bind_run_contains("GIT_AUTHOR_DATE=2000-01-01T00:00:00Z")
        && bind_run_contains("GIT_COMMITTER_DATE=2000-01-01T00:00:00Z");
    if (!direct_subject_fetch && !synthetic_pr_subject)
        || !bind_run_contains("git update-ref refs/heads/non-rust-policy-subject")
    {
        bail!("trusted workflow must bind the exact SUBJECT_SHA Git object");
    }
    let evaluator_steps = steps
        .iter()
        .filter_map(|step| {
            let map = step.as_mapping()?;
            let name = map.get(key("name")).and_then(serde_yaml_ng::Value::as_str)?;
            (name == "Run trusted exact-tree evaluator").then_some(map)
        })
        .collect::<Vec<_>>();
    if evaluator_steps.len() != 1 {
        bail!("trusted workflow must define exactly one named evaluator step");
    }
    let evaluator = evaluator_steps[0];
    let evaluator_run = evaluator
        .get(key("run"))
        .and_then(serde_yaml_ng::Value::as_str)
        .ok_or_else(|| eyre!("trusted evaluator step must execute a run command"))?;
    if !evaluator_run.contains("cargo run --locked -p xtask -- non-rust exact-tree") {
        bail!("trusted evaluator step must execute the exact-tree command");
    }
    for argument in [
        "--base-sha \"$BASE_SHA\"",
        "--subject-sha \"$SUBJECT_SHA\"",
        "--event-name \"$GITHUB_EVENT_NAME\"",
        "--repository \"$GITHUB_REPOSITORY\"",
    ] {
        if !evaluator_run.contains(argument) {
            bail!("trusted evaluator step is missing exact argument {argument}");
        }
    }
    let condition = evaluator
        .get(key("if"))
        .and_then(serde_yaml_ng::Value::as_str)
        .ok_or_else(|| eyre!("trusted evaluator step must gate on identity binding"))?;
    if condition.trim() != "steps.bind.outcome == 'success'" {
        bail!("trusted evaluator step must use the exact identity-binding condition");
    }
    if !steps.iter().any(|step| {
        let Some(map) = step.as_mapping() else { return false };
        map.get(key("uses"))
            .and_then(serde_yaml_ng::Value::as_str)
            .is_some_and(|uses| uses.starts_with("actions/upload-artifact@"))
            && map
                .get(key("if"))
                .and_then(serde_yaml_ng::Value::as_str)
                .is_some_and(|condition| condition.contains("always()"))
    }) {
        bail!("trusted workflow must upload evidence with if: always()");
    }
    for step in steps {
        if let Some(action) = step
            .as_mapping()
            .and_then(|map| map.get(key("uses")))
            .and_then(serde_yaml_ng::Value::as_str)
        {
            if !(action.starts_with("actions/checkout@")
                || action.starts_with("dtolnay/rust-toolchain@")
                || action.starts_with("taiki-e/install-action@")
                || action.starts_with("Swatinem/rust-cache@")
                || action.starts_with("actions/upload-artifact@"))
            {
                bail!("subject workflow adds an unapproved action: {action}");
            }
            let reference = action
                .split_once('@')
                .and_then(|(_, value)| value.split_whitespace().next())
                .ok_or_else(|| {
                    eyre!("subject workflow action is missing an immutable reference: {action}")
                })?;
            if reference.len() != 40 || !reference.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                bail!("subject workflow action must use a full 40-hex commit SHA: {action}");
            }
        }
    }
    let executable_text = steps
        .iter()
        .filter_map(|step| {
            let map = step.as_mapping()?;
            (!is_contract_step(&step)).then_some(map)
        })
        .filter_map(|step| step.get(key("run")))
        .filter_map(serde_yaml_ng::Value::as_str)
        .collect::<Vec<_>>();
    for forbidden in [
        "git show",
        "git cat-file",
        "git archive",
        "git checkout",
        "git clone",
        "git read-tree",
        "git reset",
        "git restore",
        "git worktree",
        "source ",
        " . ./",
        "./$",
        " . ",
        "bash <",
        "sh <",
        "curl ",
        "wget ",
        "| bash",
        "| sh",
        "eval ",
        "refs/pull/${{",
        "git fetch .*head",
    ] {
        if executable_text.iter().any(|run| run.contains(forbidden)) && forbidden != "refs/pull/" {
            bail!(
                "subject workflow must not execute or import candidate-derived content: {forbidden}"
            );
        }
    }
    Ok(())
}

/// Compare policy classification of two immutable Git trees and emit an
/// exact-SHA receipt. Existing debt is preserved; only newly unclassified
/// subject paths fail.
pub fn non_rust_exact_tree(
    root: &Path,
    base_sha: &str,
    subject_sha: &str,
    pr_head_sha: Option<&str>,
    receipt_path: &Path,
    event_name: Option<&str>,
    repository: Option<&str>,
) -> Result<()> {
    if receipt_path.exists() {
        fs::remove_file(receipt_path).context("removing stale exact-tree receipt")?;
    }
    let result = non_rust_exact_tree_inner(
        root,
        base_sha,
        subject_sha,
        pr_head_sha,
        receipt_path,
        event_name,
        repository,
    );
    if let Err(error) = &result {
        let error_text = error.to_string();
        let failure_stage = if error_text.contains("allowlist") || error_text.contains("parsing") {
            "policy"
        } else if error_text.contains("writing") || error_text.contains("output") {
            "projection"
        } else if error_text.contains("rev-parse") || error_text.contains("ancestry") {
            "identity"
        } else {
            "evaluation"
        };
        let receipt = ExactTreePolicyReceipt {
            schema_version: 2,
            event_name: event_name.map(str::to_string),
            repository: repository.map(str::to_string),
            base_sha: base_sha.to_string(),
            base_tree_sha: None,
            subject_sha: subject_sha.to_string(),
            subject_tree_sha: None,
            pr_head_sha: pr_head_sha.map(str::to_string),
            base_allowlist_blob_sha: None,
            subject_allowlist_blob_sha: None,
            base_unclassified_count: 0,
            subject_unclassified_count: 0,
            new_unclassified_paths: Vec::new(),
            outcome: "fail".to_string(),
            failure_stage: Some(failure_stage.to_string()),
            error: Some(error_text),
            evaluation_date: Utc::now().date_naive().to_string(),
            evaluator_commit: git_object(root, &["rev-parse", "HEAD"])
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .map(|s| s.trim().to_string()),
            evaluator_tree: git_object(root, &["rev-parse", "HEAD^{tree}"])
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .map(|s| s.trim().to_string()),
            inventory_markdown_path: None,
            inventory_markdown_size: None,
            inventory_markdown_sha256: None,
            inventory_json_path: None,
            inventory_json_size: None,
            inventory_json_sha256: None,
        };
        if !receipt_path.exists() {
            if let Some(parent) = receipt_path.parent().filter(|p| !p.as_os_str().is_empty()) {
                fs::create_dir_all(parent)?;
            }
            fs::write(receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
        }
    }
    result
}

fn non_rust_exact_tree_inner(
    root: &Path,
    base_sha: &str,
    subject_sha: &str,
    pr_head_sha: Option<&str>,
    receipt_path: &Path,
    event_name: Option<&str>,
    repository: Option<&str>,
) -> Result<()> {
    let base_ref = format!("{base_sha}^{{commit}}");
    let subject_ref = format!("{subject_sha}^{{commit}}");
    let base_commit = String::from_utf8(git_object(root, &["rev-parse", "--verify", &base_ref])?)?
        .trim()
        .to_string();
    let subject_commit =
        String::from_utf8(git_object(root, &["rev-parse", "--verify", &subject_ref])?)?
            .trim()
            .to_string();
    if let Some(pr_head) = pr_head_sha {
        let ancestor = Command::new("git")
            .args(["merge-base", "--is-ancestor", pr_head, &subject_commit])
            .current_dir(root)
            .status()
            .context("checking PR head ancestry")?;
        if !ancestor.success() {
            bail!("subject {subject_commit} does not contain PR head {pr_head}");
        }
    }
    let topology = Command::new("git")
        .args(["merge-base", "--is-ancestor", &base_commit, &subject_commit])
        .current_dir(root)
        .status()
        .context("checking base ancestry")?;
    if !topology.success() {
        bail!("subject {subject_commit} is not based on base {base_commit}");
    }
    if matches!(event_name, Some("pull_request_target") | Some("merge_group")) {
        let first_parent =
            String::from_utf8(git_object(root, &["rev-parse", &format!("{subject_commit}^1")])?)?
                .trim()
                .to_string();
        if first_parent != base_commit {
            bail!("subject first parent {first_parent} is not base {base_commit}");
        }
    }
    let base_tree_ref = format!("{base_commit}^{{tree}}");
    let subject_tree_ref = format!("{subject_commit}^{{tree}}");
    let base_tree_sha =
        String::from_utf8(git_object(root, &["rev-parse", &base_tree_ref])?)?.trim().to_string();
    let subject_tree_sha =
        String::from_utf8(git_object(root, &["rev-parse", &subject_tree_ref])?)?.trim().to_string();
    validate_subject_workflow(root, &base_commit, &subject_commit)?;
    let evaluation_date = Utc::now().date_naive();
    let (base_records, base_allowlist_blob_sha) =
        classify_tree_at(root, &base_commit, evaluation_date)?;
    let (subject_records, subject_allowlist_blob_sha) =
        classify_tree_at(root, &subject_commit, evaluation_date)?;
    let base_unclassified = base_records
        .iter()
        .filter(|r| r.category == "unclassified")
        .map(|r| r.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let subject_unclassified = subject_records
        .iter()
        .filter(|r| r.category == "unclassified")
        .map(|r| r.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut new_unclassified_paths =
        subject_unclassified.difference(&base_unclassified).cloned().collect::<Vec<_>>();
    let base_expired_paths = expired_paths_in_tree(root, &base_commit, evaluation_date)?
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    for path in expired_paths_in_tree(root, &subject_commit, evaluation_date)? {
        // Preserve inherited expired debt, and do not flag paths that remain
        // covered by another active entry (their subject record is classified).
        if subject_unclassified.contains(&path)
            && !base_expired_paths.contains(&path)
            && !new_unclassified_paths.contains(&path)
        {
            new_unclassified_paths.push(path);
        }
    }
    new_unclassified_paths.sort();
    let markdown = render_markdown(&subject_records);
    let output_dir = root.join("target/policy");
    fs::create_dir_all(&output_dir).context("creating exact-tree output directory")?;
    fs::write(output_dir.join("non-rust-inventory.md"), &markdown)
        .context("writing exact-tree Markdown")?;
    fs::write(
        output_dir.join("non-rust-inventory.json"),
        serde_json::to_vec_pretty(&subject_records)?,
    )
    .context("writing exact-tree JSON")?;
    let markdown_sha256 = Sha256::digest(markdown.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let json_bytes = serde_json::to_vec_pretty(&subject_records)?;
    let json_sha256 =
        Sha256::digest(&json_bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let receipt = ExactTreePolicyReceipt {
        schema_version: 2,
        event_name: event_name.map(str::to_string),
        repository: repository.map(str::to_string),
        base_sha: base_commit,
        base_tree_sha: Some(base_tree_sha),
        subject_sha: subject_commit,
        subject_tree_sha: Some(subject_tree_sha),
        pr_head_sha: pr_head_sha.map(str::to_string),
        base_allowlist_blob_sha: Some(base_allowlist_blob_sha),
        subject_allowlist_blob_sha: Some(subject_allowlist_blob_sha),
        base_unclassified_count: base_unclassified.len(),
        subject_unclassified_count: subject_unclassified.len(),
        outcome: if new_unclassified_paths.is_empty() { "pass" } else { "fail" }.to_string(),
        new_unclassified_paths: new_unclassified_paths.clone(),
        failure_stage: None,
        error: None,
        evaluation_date: evaluation_date.to_string(),
        evaluator_commit: git_object(root, &["rev-parse", "HEAD"])
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .map(|s| s.trim().to_string()),
        evaluator_tree: git_object(root, &["rev-parse", "HEAD^{tree}"])
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .map(|s| s.trim().to_string()),
        inventory_markdown_path: Some("target/policy/non-rust-inventory.md".to_string()),
        inventory_markdown_size: Some(markdown.len() as u64),
        inventory_markdown_sha256: Some(markdown_sha256),
        inventory_json_path: Some("target/policy/non-rust-inventory.json".to_string()),
        inventory_json_size: Some(json_bytes.len() as u64),
        inventory_json_sha256: Some(json_sha256),
    };
    if let Some(parent) = receipt_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    if !new_unclassified_paths.is_empty() {
        bail!("newly unclassified paths: {}", new_unclassified_paths.join(", "));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Core inventory logic
// ---------------------------------------------------------------------------

#[cfg(test)]
fn classify_file(path: &str, entries: &[AllowEntry]) -> FileRecord {
    let prepared = prepare_allow_entries(entries);
    classify_file_with_prepared(path, &prepared)
}

fn classify_file_with_prepared(path: &str, entries: &[PreparedAllowEntry<'_>]) -> FileRecord {
    let extension = path
        .rsplit('/')
        .next()
        .and_then(|file_name| file_name.rsplit_once('.'))
        .filter(|(stem, ext)| !stem.is_empty() && !ext.is_empty())
        .map(|(_, ext)| ext)
        .unwrap_or("")
        .to_string();

    if is_rust_file(path) {
        return FileRecord {
            path: path.to_string(),
            extension,
            category: "rust".to_string(),
            allowlisted: false,
            entry: None,
        };
    }

    match find_matching_prepared_entry(path, entries) {
        Some(e) => FileRecord {
            path: path.to_string(),
            extension,
            category: e.classification.clone(),
            allowlisted: true,
            entry: Some(e.clone()),
        },
        None => FileRecord {
            path: path.to_string(),
            extension,
            category: "unclassified".to_string(),
            allowlisted: false,
            entry: None,
        },
    }
}

/// Build a full inventory from `root`.
pub fn build_inventory(root: &Path) -> Result<Vec<FileRecord>> {
    let allowlist = load_allowlist(root)?;
    let tracked = list_tracked_files(root)?;
    let prepared = prepare_allow_entries(&allowlist.allow);

    let records: Vec<FileRecord> =
        tracked.iter().map(|p| classify_file_with_prepared(p, &prepared)).collect();
    Ok(records)
}

// ---------------------------------------------------------------------------
// Markdown renderer
// ---------------------------------------------------------------------------

/// Render the inventory as a Markdown document.
pub fn render_markdown(records: &[FileRecord]) -> String {
    let total = records.len();
    let rust_count = records.iter().filter(|r| r.category == "rust").count();
    let non_rust: Vec<&FileRecord> = records.iter().filter(|r| r.category != "rust").collect();
    let allowlisted_count = non_rust.iter().filter(|r| r.allowlisted).count();
    let unclassified_count = non_rust.iter().filter(|r| !r.allowlisted).count();

    // Group non-Rust files by category for a summary table.
    let mut by_category: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &non_rust {
        *by_category.entry(r.category.as_str()).or_insert(0) += 1;
    }

    let mut out = String::new();
    out.push_str("# Non-Rust File Inventory\n\n");
    out.push_str("> Generated by `cargo xtask non-rust inventory`. Do not edit by hand.\n\n");
    out.push_str("## Summary\n\n");
    out.push_str(&format!(
        "| Metric | Count |\n|---|---|\n\
         | Total tracked files | {total} |\n\
         | Rust-family files | {rust_count} |\n\
         | Non-Rust files | {} |\n\
         | Allowlisted | {allowlisted_count} |\n\
         | Unclassified | {unclassified_count} |\n\n",
        non_rust.len()
    ));

    out.push_str("## Non-Rust files by category\n\n");
    out.push_str("| Category | Count |\n|---|---|\n");
    for (cat, count) in &by_category {
        out.push_str(&format!("| {cat} | {count} |\n"));
    }
    out.push('\n');

    if unclassified_count > 0 {
        out.push_str("## Unclassified files\n\n");
        out.push_str(
            "> These files have no matching allowlist entry. Add an entry to \
             `policy/non-rust-allowlist.toml` or run `cargo xtask non-rust propose`.\n\n",
        );
        out.push_str("| Path | Extension |\n|---|---|\n");
        for r in non_rust.iter().filter(|r| !r.allowlisted) {
            out.push_str(&format!(
                "| `{}` | `{}` |\n",
                escape_markdown_cell(&r.path),
                escape_markdown_cell(&r.extension)
            ));
        }
        out.push('\n');
    }

    out.push_str("## Allowlisted non-Rust files\n\n");
    out.push_str("| Path | Category | Entry id | Owner |\n|---|---|---|---|\n");
    for r in non_rust.iter().filter(|r| r.allowlisted) {
        let (id, owner) =
            r.entry.as_ref().map(|e| (e.id.as_str(), e.owner.as_str())).unwrap_or(("", ""));
        out.push_str(&format!(
            "| `{}` | {} | `{}` | {} |\n",
            escape_markdown_cell(&r.path),
            r.category,
            escape_markdown_cell(id),
            escape_markdown_cell(owner)
        ));
    }
    out.push('\n');

    out.push_str("## See also\n\n");
    out.push_str(
        "- [FILE_POLICY.md](FILE_POLICY.md) — the doctrine.\n\
         - [NON_RUST_POLICY.md](NON_RUST_POLICY.md) — the schema.\n\
         - [POLICY_ALLOWLISTS.md](POLICY_ALLOWLISTS.md) — all seven ledgers.\n",
    );

    out
}

/// Fail closed when the rendered projection is self-inconsistent.
///
/// One tracked file must project to exactly one row (#1800 review): a
/// duplicate file path in any table — or a path listed under both the
/// unclassified and allowlisted tables — means the projection was produced
/// from more than one policy/output pass and every row-count consumer sees a
/// contradictory denominator. The summary must also agree with the emitted
/// row counts, so stale summary totals cannot survive a regeneration.
pub(crate) fn verify_inventory_projection(markdown: &str) -> Result<()> {
    let mut seen_paths = std::collections::BTreeSet::new();
    let mut summary_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut section_rows: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    let mut section = "";

    for line in markdown.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            section = line.trim_start_matches('#').trim();
            continue;
        }
        let Some(cells) = parse_markdown_cells(line) else { continue };
        if cells.len() == 2 && section == "Summary" {
            if let Ok(count) = cells[1].parse::<usize>() {
                summary_counts.insert(cells[0].clone(), count);
            }
            continue;
        }
        let Some(path) = cells
            .first()
            .and_then(|cell| cell.strip_prefix('`').and_then(|path| path.strip_suffix('`')))
            .map(str::to_string)
        else {
            continue;
        };
        if !seen_paths.insert(path.clone()) {
            bail!(
                "non-Rust inventory projection emits duplicate file rows for `{path}`; \
                 regenerate from a single pass with `cargo xtask non-rust inventory`"
            );
        }
        *section_rows.entry(section).or_insert(0) += 1;
    }

    if let (Some(&allowlisted), Some(&rows)) =
        (summary_counts.get("Allowlisted"), section_rows.get("Allowlisted non-Rust files"))
        && allowlisted != rows
    {
        bail!(
            "non-Rust inventory summary reports {allowlisted} allowlisted files but the \
             table projects {rows} rows; regenerate the summary with the same pass"
        );
    }
    if let (Some(&unclassified), Some(&rows)) =
        (summary_counts.get("Unclassified"), section_rows.get("Unclassified files"))
        && unclassified != rows
    {
        bail!(
            "non-Rust inventory summary reports {unclassified} unclassified files but the \
             table projects {rows} rows; regenerate the summary with the same pass"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Write one current-tree inventory projection to ignored workflow evidence.
///
/// Both the observation command and the merge check use this path so success
/// and failure inspect the same Markdown/JSON representation.
fn write_inventory_outputs(root: &Path, records: &[FileRecord]) -> Result<()> {
    let target_dir = root.join("target/policy");
    fs::create_dir_all(&target_dir)
        .with_context(|| format!("creating {}", target_dir.display()))?;

    let md_path = target_dir.join("non-rust-inventory.md");
    let json_path = target_dir.join("non-rust-inventory.json");

    let markdown = render_markdown(records);
    verify_inventory_projection(&markdown)
        .with_context(|| "generated non-Rust inventory projection is self-inconsistent")?;
    fs::write(&md_path, &markdown).with_context(|| format!("writing {}", md_path.display()))?;
    println!("  wrote {}", md_path.display());

    let json =
        serde_json::to_string_pretty(records).with_context(|| "serialising inventory to JSON")?;
    fs::write(&json_path, &json).with_context(|| format!("writing {}", json_path.display()))?;
    println!("  wrote {}", json_path.display());

    Ok(())
}

/// Entry point for `cargo xtask non-rust inventory`.
///
/// Writes only current-tree evidence under `target/policy/`; no tracked file is
/// read as authority or modified. Use [`non_rust_inventory_write_docs`] only
/// when deliberately publishing the default-branch reader reference.
pub fn non_rust_inventory(root: &Path) -> Result<()> {
    println!("Building non-Rust file inventory...");

    let records = build_inventory(root)?;
    write_inventory_outputs(root, &records)?;

    let total = records.len();
    let rust_count = records.iter().filter(|record| record.category == "rust").count();
    let non_rust_count = total - rust_count;
    let allowlisted = records.iter().filter(|record| record.allowlisted).count();
    let unclassified = non_rust_count - allowlisted;

    println!(
        "\nInventory complete: {total} tracked files\n\
         - Rust-family:   {rust_count}\n\
         - Non-Rust:      {non_rust_count}\n\
         - Allowlisted:   {allowlisted}\n\
         - Unclassified:  {unclassified}"
    );

    Ok(())
}

/// Publish `docs/policy/NON_RUST_INVENTORY.md` from the current tree.
///
/// This deliberate `--write` path first writes the ignored evidence under
/// `target/policy/`, then copies the Markdown to the tracked default-branch
/// reader reference. The published copy is not an input to branch validity.
pub fn non_rust_inventory_write_docs(root: &Path) -> Result<()> {
    non_rust_inventory(root)?;

    let target_md = root.join("target/policy/non-rust-inventory.md");
    let markdown = fs::read_to_string(&target_md)
        .with_context(|| format!("reading generated inventory from {}", target_md.display()))?;

    let docs_path = root.join("docs/policy/NON_RUST_INVENTORY.md");
    if let Some(parent) = docs_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&docs_path, &markdown).with_context(|| format!("writing {}", docs_path.display()))?;
    println!("  wrote {}", docs_path.display());

    Ok(())
}

/// Evaluate the current tracked tree against the non-Rust allowlist.
///
/// The allowlist and current tree own the verdict. The check always emits
/// Markdown/JSON evidence before applying the merge-base ratchet, warns on
/// existing unclassified debt, and rejects only newly added unclassified
/// paths. It never reads or rewrites the tracked publication reference.
pub fn non_rust_inventory_check(root: &Path) -> Result<()> {
    let baseline = resolve_inventory_baseline(root);
    non_rust_inventory_check_with_baseline(root, baseline.as_deref())
}

fn non_rust_inventory_check_with_baseline(root: &Path, baseline: Option<&str>) -> Result<()> {
    let mut policy_errors = Vec::new();
    validate_policy_table(
        &root.join("policy/non-rust-allowlist.toml"),
        "allow",
        true,
        &mut policy_errors,
    );
    if !policy_errors.is_empty() {
        bail!("non-Rust allowlist validation failed: {}", policy_errors.join("; "));
    }

    let records = build_inventory(root)?;
    write_inventory_outputs(root, &records)?;

    let unclassified: Vec<&FileRecord> =
        records.iter().filter(|record| record.category == "unclassified").collect();
    if !unclassified.is_empty() {
        eprintln!(
            "warning: current non-Rust inventory has {} unclassified tracked file(s); \
             inherited entries are tolerated, but newly added entries are rejected",
            unclassified.len()
        );
        for record in unclassified.iter().take(5) {
            eprintln!("warning: unclassified path: {}", record.path);
        }
    }

    if let Some(baseline) = baseline {
        let added_paths = added_paths_since(root, baseline)?;
        let newly_unclassified: Vec<&FileRecord> = unclassified
            .iter()
            .copied()
            .filter(|record| added_paths.iter().any(|path| path == &record.path))
            .collect();
        if !newly_unclassified.is_empty() {
            let paths = newly_unclassified
                .iter()
                .map(|record| record.path.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "newly added tracked non-Rust file(s) are unclassified: {paths}; \
                 add entries to policy/non-rust-allowlist.toml before merging"
            );
        }
    } else if std::env::var_os("CI").is_some() {
        // Without a baseline the new-path ratchet cannot run. Locally that is a
        // warning; in CI it would silently pass every unclassified addition, so
        // the required gate fails closed instead (#14688).
        bail!(
            "cannot resolve a merge baseline in CI; the newly added unclassified-path \
             ratchet did not run (fetch with full history so origin/main resolves)"
        );
    } else {
        eprintln!(
            "warning: cannot resolve a merge baseline; current-tree evidence was emitted, \
             but inherited and newly added unclassified debt could not be distinguished"
        );
    }

    println!("Non-Rust inventory policy check passed for the current tracked tree");
    Ok(())
}

fn resolve_inventory_baseline(root: &Path) -> Option<String> {
    let mut candidates = Vec::new();
    if let Ok(scope_base) = std::env::var("CI_SCOPE_BASE") {
        candidates.push(scope_base);
    }
    if let Ok(base_ref) = std::env::var("GITHUB_BASE_REF") {
        candidates.push(format!("origin/{base_ref}"));
        candidates.push(base_ref);
    }
    candidates.extend(["origin/main".to_string(), "HEAD^".to_string()]);

    candidates.into_iter().find(|candidate| {
        Command::new("git")
            .args(["rev-parse", "--verify", candidate])
            .current_dir(root)
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

fn added_paths_since(root: &Path, baseline: &str) -> Result<Vec<String>> {
    let range = format!("{baseline}..HEAD");
    // `--no-renames` keeps a renamed or copied file visible as an addition at
    // its destination path; with rename detection a classified file moved to
    // an unclassified path would never reach the ratchet.
    let output = Command::new("git")
        .args(["diff", "--name-only", "--no-renames", "--diff-filter=A", "-z", baseline, "HEAD"])
        .current_dir(root)
        .output()
        .with_context(|| {
            format!("running `git diff --name-only --no-renames --diff-filter=A -z {range}`")
        })?;
    if !output.status.success() {
        return Err(eyre!(
            "`git diff --name-only --no-renames --diff-filter=A -z {range}` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            let path = String::from_utf8(path.to_vec())
                .with_context(|| "git diff produced a non-UTF-8 path")?;
            Ok(path.replace('\\', "/"))
        })
        .collect()
}

/// Escape a literal value for embedding in one Markdown table cell so the
/// rendered row keeps exactly one cell per column: a literal `|` inside a
/// value would otherwise split the row. Backslashes are intentionally left
/// unchanged because values may be rendered inside Markdown code spans. The
/// pipe escape is reversed by [`parse_markdown_cells`].
fn escape_markdown_cell(value: &str) -> String {
    value.replace('|', "\\|")
}

/// Split one rendered table row into its raw cell values, honoring
/// [`escape_markdown_cell`] pipe escapes and trimming the surrounding
/// whitespace the renderer emits. Only the marker immediately before a pipe
/// is consumed; all other backslashes are preserved verbatim. Returns `None`
/// for lines outside table rows.
fn parse_markdown_cells(line: &str) -> Option<Vec<String>> {
    let rest = line.trim().strip_prefix("| ")?;
    // Remove exactly the table delimiter. Using `trim_end_matches('|')` would
    // also remove a literal pipe when a caller provides a row without the
    // renderer's separating whitespace.
    let trimmed = rest.trim_end().strip_suffix('|')?;
    let mut cells = Vec::new();
    let mut current = String::new();
    for ch in trimmed.chars() {
        match ch {
            '|' => {
                if current.ends_with('\\') {
                    current.pop();
                    current.push('|');
                } else {
                    cells.push(current.trim().to_string());
                    current = String::new();
                }
            }
            _ => current.push(ch),
        }
    }
    cells.push(current.trim().to_string());
    Some(cells)
}

// ---------------------------------------------------------------------------
// validate-policy — schema-only non-Rust policy validation
// ---------------------------------------------------------------------------

const REQUIRED_ALLOW_FIELDS: &[&str] = &[
    "id",
    "kind",
    "language",
    "surface",
    "classification",
    "owner",
    "reason",
    "covered_by",
    "created",
    "review_after",
];

const ALLOWED_ALLOW_FIELDS: &[&str] = &[
    "id",
    "glob",
    "path",
    "kind",
    "language",
    "surface",
    "classification",
    "owner",
    "reason",
    "covered_by",
    "created",
    "review_after",
    "expires",
    "broad_glob_reason",
    "retired",
    "generated_by",
];

const KNOWN_CLASSIFICATIONS: &[&str] =
    &["production", "test", "tooling", "config", "documentation", "generated"];

const COVERAGE_REQUIRING_CLASSIFICATIONS: &[&str] = &["production", "test", "tooling"];

/// Configuration for `cargo xtask non-rust validate-policy`.
pub struct ValidateNonRustPolicyConfig {
    /// Override the default allowlist path (`policy/non-rust-allowlist.toml`).
    pub allowlist_path: std::path::PathBuf,
    /// Override the default debt path (`policy/non-rust-debt.toml`).
    pub debt_path: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonRustPolicyValidation {
    pub allow_entries: usize,
    pub debt_entries: usize,
    pub errors: Vec<String>,
}

impl NonRustPolicyValidation {
    fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Validate the non-Rust allowlist and debt TOML schema without walking git.
///
/// This is the Rust successor to `scripts/policy/validate_non_rust_allowlist.py`:
/// fast schema validation lives next to the main file-policy engine, while the
/// compatibility script only delegates to this command.
pub fn validate_non_rust_policy(config: ValidateNonRustPolicyConfig) -> Result<()> {
    let validation = validate_non_rust_policy_files(&config.allowlist_path, &config.debt_path);

    if validation.is_ok() {
        let allow_word = if validation.allow_entries == 1 { "entry" } else { "entries" };
        let debt_word = if validation.debt_entries == 1 { "entry" } else { "entries" };
        println!(
            "OK: validated {} allow {} and {} debt {}.",
            validation.allow_entries, allow_word, validation.debt_entries, debt_word
        );
        return Ok(());
    }

    eprintln!("FAIL: {} non-Rust policy validation error(s):", validation.errors.len());
    for error in &validation.errors {
        eprintln!("  - {error}");
    }
    Err(eyre!("non-Rust policy validation failed with {} error(s)", validation.errors.len()))
}

fn validate_non_rust_policy_files(
    allowlist_path: &std::path::Path,
    debt_path: &std::path::Path,
) -> NonRustPolicyValidation {
    let mut errors = Vec::new();
    let allow_entries = validate_policy_table(allowlist_path, "allow", true, &mut errors);
    let debt_entries = validate_policy_table(debt_path, "debt", false, &mut errors);
    NonRustPolicyValidation { allow_entries, debt_entries, errors }
}

fn validate_policy_table(
    path: &std::path::Path,
    table_name: &str,
    strict_allow_schema: bool,
    errors: &mut Vec<String>,
) -> usize {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            errors.push(format!("FAIL: read {}: {err}", path.display()));
            return 0;
        }
    };
    // Scan the raw source before parsing and table lookup: a syntactically
    // valid policy without the selected table must not bypass marker checks.
    let marker_lines = policy_conflict_marker_lines(&text);
    if !marker_lines.is_empty() {
        errors.push(format!("{}: Git conflict markers at lines {marker_lines:?}", path.display()));
    }

    let data = match toml::from_str::<toml::Value>(&text) {
        Ok(data) => data,
        Err(err) => {
            errors.push(format!("FAIL: parse {}: {err}", path.display()));
            return 0;
        }
    };

    let Some(entries) = data.get(table_name) else {
        return 0;
    };
    let Some(entries) = entries.as_array() else {
        errors.push(format!("{}: `{table_name}` must be a list of tables", path.display()));
        return 0;
    };

    validate_policy_entries(entries, table_name, strict_allow_schema, errors);
    entries.len()
}

/// Canonical allow-array schema, identity, matcher uniqueness, and provenance.
///
/// Exact-tree receipts and ordinary `validate-policy` both walk this path so a
/// schema-field change cannot be applied to only one surface.
fn validate_allow_document_entries(entries: &[toml::Value], errors: &mut Vec<String>) {
    validate_policy_entries(entries, "allow", true, errors);
}

fn validate_policy_entries(
    entries: &[toml::Value],
    table_name: &str,
    strict_allow_schema: bool,
    errors: &mut Vec<String>,
) {
    let mut seen_ids: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen_matchers: BTreeMap<String, String> = BTreeMap::new();
    let mut coherence_tables: Vec<&toml::map::Map<String, toml::Value>> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(table) = entry.as_table() else {
            errors.push(format!("`{table_name}` entry #{index} must be a table"));
            continue;
        };
        coherence_tables.push(table);
        if strict_allow_schema {
            validate_allow_schema_entry(table, index, errors);
        }

        let entry_id = table
            .get("id")
            .and_then(toml::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("<unnamed entry #{index}>"));

        if let Some(id) = table.get("id").and_then(toml::Value::as_str)
            && let Some(previous) = seen_ids.insert(id.to_string(), index)
        {
            errors.push(format!("{id}: duplicate id (also at index {previous})"));
        }

        let matcher = table.get("glob").or_else(|| table.get("path")).and_then(toml::Value::as_str);
        if let Some(matcher) = matcher
            && let Some(previous_id) = seen_matchers.insert(matcher.to_string(), entry_id.clone())
        {
            errors.push(format!(
                "{entry_id}: duplicate matcher `{matcher}` (also used by id `{previous_id}`)"
            ));
        }
    }

    if table_name == "allow" {
        for conflict in mispaired_provenance_conflicts(&coherence_tables) {
            errors.push(format!("mispaired provenance: {conflict}"));
        }
    }
}

fn validate_allow_schema_entry(
    entry: &toml::map::Map<String, toml::Value>,
    index: usize,
    errors: &mut Vec<String>,
) {
    let entry_id = entry.get("id").and_then(toml::Value::as_str).unwrap_or("<unnamed entry>");
    let fallback_id = format!("<unnamed entry #{index}>");
    let entry_id = if entry_id == "<unnamed entry>" { fallback_id.as_str() } else { entry_id };

    let has_glob = entry.contains_key("glob");
    let has_path = entry.contains_key("path");
    if has_glob && has_path {
        errors.push(format!("{entry_id}: cannot set both `glob` and `path`"));
    }
    if !has_glob && !has_path {
        errors.push(format!("{entry_id}: must set either `glob` or `path`"));
    }

    if let Some(matcher) =
        entry.get("glob").or_else(|| entry.get("path")).and_then(toml::Value::as_str)
    {
        validate_repo_relative_matcher(entry_id, matcher, errors);
        if has_glob && is_policy_broad_glob(matcher) {
            let has_reason = entry
                .get("broad_glob_reason")
                .and_then(toml::Value::as_str)
                .is_some_and(|reason| !reason.trim().is_empty());
            if !has_reason {
                errors.push(format!(
                    "{entry_id}: glob `{matcher}` is broad; declare `broad_glob_reason`"
                ));
            }
        }
        if has_glob && Pattern::new(matcher).is_err() {
            errors.push(format!("{entry_id}: invalid glob `{matcher}`"));
        }
    }

    for field in REQUIRED_ALLOW_FIELDS {
        if !entry.contains_key(*field) {
            errors.push(format!("{entry_id}: missing required field `{field}`"));
        }
    }

    for field in entry.keys() {
        if !ALLOWED_ALLOW_FIELDS.contains(&field.as_str()) {
            errors.push(format!("{entry_id}: unknown field `{field}`"));
        }
    }

    if let Some(classification) = entry.get("classification").and_then(toml::Value::as_str)
        && !KNOWN_CLASSIFICATIONS.contains(&classification)
    {
        errors.push(format!(
            "{entry_id}: classification `{classification}` not in {:?}",
            KNOWN_CLASSIFICATIONS
        ));
    }

    validate_covered_by(entry_id, entry, errors);
    validate_policy_dates(entry_id, entry, errors);

    if let Some(retired) = entry.get("retired")
        && retired.as_bool().is_none()
    {
        errors.push(format!("{entry_id}: `retired` must be a boolean"));
    }
}

fn validate_repo_relative_matcher(entry_id: &str, matcher: &str, errors: &mut Vec<String>) {
    if matcher.starts_with("./") || matcher.starts_with('/') {
        errors.push(format!(
            "{entry_id}: matcher `{matcher}` must be repo-relative without leading `./` or `/`"
        ));
    }
    if matcher.contains('\\') {
        errors.push(format!("{entry_id}: matcher `{matcher}` contains Windows backslashes"));
    }
    if matcher.trim() != matcher {
        errors.push(format!("{entry_id}: matcher `{matcher}` has surrounding whitespace"));
    }
}

fn validate_covered_by(
    entry_id: &str,
    entry: &toml::map::Map<String, toml::Value>,
    errors: &mut Vec<String>,
) {
    let covered_by = entry.get("covered_by");
    let covered_by_strings = covered_by
        .and_then(toml::Value::as_array)
        .map(|items| items.iter().all(|item| item.as_str().is_some()));
    if covered_by.is_some() && covered_by_strings != Some(true) {
        errors.push(format!("{entry_id}: `covered_by` must be a list of strings"));
        return;
    }

    let classification = entry.get("classification").and_then(toml::Value::as_str);
    let coverage_required = classification
        .is_some_and(|classification| COVERAGE_REQUIRING_CLASSIFICATIONS.contains(&classification));
    let coverage_empty = covered_by.and_then(toml::Value::as_array).is_none_or(Vec::is_empty);
    if coverage_required && coverage_empty {
        let classification = classification.unwrap_or("unknown");
        errors.push(format!(
            "{entry_id}: classification `{classification}` requires at least one `covered_by` entry"
        ));
    }
}

fn validate_policy_dates(
    entry_id: &str,
    entry: &toml::map::Map<String, toml::Value>,
    errors: &mut Vec<String>,
) {
    let created = parse_policy_date(entry_id, entry, "created", errors);
    let review_after = parse_policy_date(entry_id, entry, "review_after", errors);
    let expires = if entry.contains_key("expires") {
        parse_policy_date(entry_id, entry, "expires", errors)
    } else {
        None
    };

    if let (Some(created), Some(review_after)) = (created, review_after)
        && review_after <= created
    {
        errors.push(format!("{entry_id}: `review_after` must be after `created`"));
    }
    if let (Some(created), Some(expires)) = (created, expires)
        && expires <= created
    {
        errors.push(format!("{entry_id}: `expires` must be after `created`"));
    }
}

fn parse_policy_date(
    entry_id: &str,
    entry: &toml::map::Map<String, toml::Value>,
    field: &str,
    errors: &mut Vec<String>,
) -> Option<chrono::NaiveDate> {
    let value = entry.get(field)?;
    let Some(value) = value.as_str() else {
        errors.push(format!("{entry_id}: `{field}` must be a YYYY-MM-DD string"));
        return None;
    };
    match chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        Ok(date) => Some(date),
        Err(_) => {
            errors.push(format!("{entry_id}: `{field}` is not a real date: {value:?}"));
            None
        }
    }
}

/// Broad-glob heuristic for policy schema validation. Mirrors the original
/// Python gate and intentionally catches more than the strict enforcement
/// broad-glob helper.
///
/// Breadth is decided by `**`, the only token that crosses a directory
/// boundary, and it counts wherever it appears — `**/x`, `x/**`, and
/// `x/**/y` all reach an arbitrary-depth tree. Testing only the leading and
/// trailing forms let an internal `**` (`fixtures/pkt/**/*.json`) span a tree
/// while the validator called it narrow and waived `broad_glob_reason`.
///
/// `*.md` used to be listed here because the loose matcher let its `*` cross
/// `/`, making it a whole-repository grab; with segment-aware matching
/// (#9994) it reaches only the repository root, so demanding a
/// `broad_glob_reason` for it would force a misleading justification onto a
/// genuinely narrow rule. Refs: #9994, #14583.
fn is_policy_broad_glob(glob_str: &str) -> bool {
    glob_str.split('/').any(|segment| segment == "**")
}

// ---------------------------------------------------------------------------
// check-file-policy — enforcement subcommand
// ---------------------------------------------------------------------------

/// Operating mode for `cargo xtask non-rust check`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckFilePolicyMode {
    /// Report only — never exit with a non-zero code.
    Advisory,
    /// Fail when any non-Rust file has no allowlist entry, or any entry has
    /// an expired `expires` date. Does not check `review_after`.
    BlockingAllowlist,
    /// `blocking-allowlist` plus: stale `review_after`, duplicate entry ids,
    /// absolute or backslash paths, and broad globs without `broad_glob_reason`.
    BlockingStrict,
}

impl std::fmt::Display for CheckFilePolicyMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckFilePolicyMode::Advisory => write!(f, "advisory"),
            CheckFilePolicyMode::BlockingAllowlist => write!(f, "blocking-allowlist"),
            CheckFilePolicyMode::BlockingStrict => write!(f, "blocking-strict"),
        }
    }
}

/// Configuration for `cargo xtask non-rust check`.
pub struct CheckFilePolicyConfig {
    /// Operating mode.
    pub mode: CheckFilePolicyMode,
    /// If `Some(path)`, write the JSON receipt to this file.
    pub json_output: Option<std::path::PathBuf>,
    /// Override the default allowlist path (`policy/non-rust-allowlist.toml`).
    pub allowlist_path: Option<std::path::PathBuf>,
    /// Override the workspace root used for `git ls-files`.
    /// When `None`, the binary resolves `project_root()` at runtime.
    /// Intended as a test seam — production invocations omit this.
    pub root_override: Option<std::path::PathBuf>,
}

/// A single policy violation found during `check-file-policy`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyViolation {
    /// Machine-readable violation kind.
    pub kind: String,
    /// Human-readable description.
    pub message: String,
    /// Path of the file or entry involved (if applicable).
    pub path: Option<String>,
    /// Allowlist entry id involved (if applicable).
    pub entry_id: Option<String>,
}

/// JSON receipt emitted by `cargo xtask non-rust check`.
#[derive(Debug, Serialize, Deserialize)]
pub struct FilePolicyReceipt {
    /// Always 1 for this schema generation.
    pub schema_version: u32,
    /// Mode used for this run.
    pub mode: String,
    /// Total number of tracked files (Rust + non-Rust).
    pub total_tracked: usize,
    /// Number of non-Rust files.
    pub non_rust: usize,
    /// Number of non-Rust files with no allowlist entry.
    pub unclassified: usize,
    /// Number of allowlist entries with an expired `expires` date.
    pub expired: usize,
    /// Number of allowlist entries with a stale `review_after` date (past today).
    pub stale_review_after: usize,
    /// Number of duplicate entry ids across the allowlist.
    pub duplicate_ids: usize,
    /// Number of non-retired allowlist entries that match no tracked file.
    pub unused_entries: usize,
    /// Violations that fail the selected mode.
    pub violations: Vec<PolicyViolation>,
}

/// Check whether a date string (YYYY-MM-DD) is in the past relative to today.
fn is_past_date(date_str: &str) -> bool {
    // Parse YYYY-MM-DD by splitting on '-'.
    let parts: Vec<&str> = date_str.trim().split('-').collect();
    if parts.len() != 3 {
        // Malformed date — treat as in the past so it gets flagged.
        return true;
    }
    let (Ok(y), Ok(m), Ok(d)) =
        (parts[0].parse::<u32>(), parts[1].parse::<u32>(), parts[2].parse::<u32>())
    else {
        return true;
    };
    // Use chrono if available; otherwise fall back to a manual comparison
    // against the compile-time UTC date (good enough for CI).
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // Approximate: days since epoch → year/month/day (Gregorian).
    let days = secs / 86400;
    // Epoch = 1970-01-01.
    let (ey, em, ed) = days_to_ymd(days);
    (y, m, d) < (ey, em, ed)
}

/// Convert days-since-Unix-epoch to (year, month, day) using the proleptic
/// Gregorian calendar. Accurate for years 1970–2200 (sufficient for policy).
fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // Algorithm: Julian Day Number method.
    let jdn = days + 2_440_588; // Unix epoch = JDN 2440588
    let a = jdn + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - 146097 * b / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - 1461 * d / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = 100 * b + d - 4800 + m / 10;
    (year as u32, month as u32, day as u32)
}

fn expired_entry_count(entries: &[AllowEntry]) -> usize {
    entries
        .iter()
        .filter(|entry| !entry.retired)
        .filter(|entry| entry.expires.as_deref().is_some_and(is_past_date))
        .count()
}

fn stale_review_after_count(entries: &[AllowEntry]) -> usize {
    entries
        .iter()
        .filter(|entry| !entry.retired)
        .filter(|entry| !entry.review_after.is_empty() && is_past_date(&entry.review_after))
        .count()
}

fn duplicate_id_count(entries: &[AllowEntry]) -> usize {
    let mut seen_ids: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in entries {
        *seen_ids.entry(entry.id.as_str()).or_insert(0) += 1;
    }
    seen_ids.values().filter(|count| **count > 1).count()
}

fn entry_matches_any_tracked_file(entry: &AllowEntry, tracked: &[String]) -> bool {
    if let Some(path) = entry.path.as_deref() {
        return tracked.iter().any(|tracked_path| tracked_path == path);
    }
    if let Some(glob_str) = entry.glob.as_deref() {
        let Ok(pattern) = Pattern::new(glob_str) else {
            return false;
        };
        return tracked.iter().any(|tracked_path| glob_matches_path(&pattern, tracked_path));
    }
    false
}

fn unused_entry_count(entries: &[AllowEntry], tracked: &[String]) -> usize {
    entries
        .iter()
        .filter(|entry| !entry.retired)
        .filter(|entry| entry.glob.is_some() ^ entry.path.is_some())
        .filter(|entry| !entry_matches_any_tracked_file(entry, tracked))
        .count()
}

fn render_policy_report_markdown(receipt: &FilePolicyReceipt) -> String {
    let mut out = String::new();
    out.push_str("# Non-Rust File Policy Report\n\n");
    out.push_str("> Generated by `cargo xtask check-file-policy`. Do not edit by hand.\n\n");
    out.push_str("## Summary\n\n");
    out.push_str("| Metric | Value |\n|---|---:|\n");
    out.push_str(&format!("| Mode | `{}` |\n", receipt.mode));
    out.push_str(&format!("| Total tracked | {} |\n", receipt.total_tracked));
    out.push_str(&format!("| Non-Rust | {} |\n", receipt.non_rust));
    out.push_str(&format!("| Unclassified | {} |\n", receipt.unclassified));
    out.push_str(&format!("| Expired entries | {} |\n", receipt.expired));
    out.push_str(&format!("| Stale review_after | {} |\n", receipt.stale_review_after));
    out.push_str(&format!("| Duplicate ids | {} |\n", receipt.duplicate_ids));
    out.push_str(&format!("| Unused entries | {} |\n", receipt.unused_entries));
    out.push_str(&format!("| Violations | {} |\n\n", receipt.violations.len()));

    if receipt.violations.is_empty() {
        out.push_str("## Violations\n\nNo violations for the selected mode.\n");
        return out;
    }

    out.push_str("## Violations\n\n");
    out.push_str("| Kind | Location | Entry | Message |\n|---|---|---|---|\n");
    for violation in &receipt.violations {
        let path = violation.path.as_deref().unwrap_or("");
        let entry_id = violation.entry_id.as_deref().unwrap_or("");
        out.push_str(&format!(
            "| `{}` | `{}` | `{}` | {} |\n",
            violation.kind, path, entry_id, violation.message
        ));
    }
    out
}

/// Run all allowlist-level validations and return violations.
fn check_allowlist_entries(
    entries: &[AllowEntry],
    mode: CheckFilePolicyMode,
    tracked: &[String],
) -> Vec<PolicyViolation> {
    let mut violations: Vec<PolicyViolation> = Vec::new();

    // --- Duplicate id check (strict only) ---
    if mode == CheckFilePolicyMode::BlockingStrict {
        let mut seen_ids: BTreeMap<&str, usize> = BTreeMap::new();
        for entry in entries {
            *seen_ids.entry(entry.id.as_str()).or_insert(0) += 1;
        }
        for (id, count) in &seen_ids {
            if *count > 1 {
                violations.push(PolicyViolation {
                    kind: "duplicate-id".to_string(),
                    message: format!("Allowlist entry id {id:?} appears {count} times"),
                    path: None,
                    entry_id: Some(id.to_string()),
                });
            }
        }
    }

    // --- Mispaired provenance (#9680/#9993), blocking in enforcement modes ---
    if mode != CheckFilePolicyMode::Advisory {
        let mut seen_reasons: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
        for entry in entries.iter().filter(|entry| !entry.retired) {
            if entry.reason.trim().is_empty() {
                continue;
            }
            let matcher = entry.glob.as_deref().or(entry.path.as_deref()).unwrap_or("<none>");
            match seen_reasons.entry(entry.reason.as_str()) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert((entry.id.as_str(), matcher));
                }
                std::collections::btree_map::Entry::Occupied(slot) => {
                    let (first_id, first_matcher) = slot.get();
                    if *first_matcher != matcher {
                        violations.push(PolicyViolation {
                            kind: "mispaired-provenance".to_string(),
                            message: mispaired_provenance_message(
                                first_id,
                                first_matcher,
                                &entry.id,
                                matcher,
                            ),
                            path: None,
                            entry_id: Some(entry.id.clone()),
                        });
                    }
                }
            }
        }
    }

    for entry in entries {
        if entry.retired {
            continue;
        }

        let has_glob = entry.glob.is_some();
        let has_path = entry.path.is_some();

        // --- Blocking-allowlist+ entry validity checks ---
        if mode != CheckFilePolicyMode::Advisory {
            if let Some(ref expires) = entry.expires
                && is_past_date(expires)
            {
                violations.push(PolicyViolation {
                    kind: "expired-entry".to_string(),
                    message: format!("Entry {:?} has expired (expires={})", entry.id, expires),
                    path: None,
                    entry_id: Some(entry.id.clone()),
                });
            }

            if !has_glob && !has_path {
                violations.push(PolicyViolation {
                    kind: "missing-matcher".to_string(),
                    message: format!("Entry {:?} must define `path` or `glob`", entry.id),
                    path: None,
                    entry_id: Some(entry.id.clone()),
                });
            }
            if has_glob && has_path {
                violations.push(PolicyViolation {
                    kind: "multiple-matchers".to_string(),
                    message: format!("Entry {:?} must not define both `path` and `glob`", entry.id),
                    path: None,
                    entry_id: Some(entry.id.clone()),
                });
            }
            if let Some(glob_str) = entry.glob.as_deref()
                && Pattern::new(glob_str).is_err()
            {
                violations.push(PolicyViolation {
                    kind: "invalid-glob".to_string(),
                    message: format!("Entry {:?} has invalid glob {:?}", entry.id, glob_str),
                    path: Some(glob_str.to_string()),
                    entry_id: Some(entry.id.clone()),
                });
            }
            if entry.kind.trim().is_empty() {
                violations.push(PolicyViolation {
                    kind: "missing-kind".to_string(),
                    message: format!("Entry {:?} is missing required field `kind`", entry.id),
                    path: None,
                    entry_id: Some(entry.id.clone()),
                });
            }
            if entry.language.trim().is_empty() {
                violations.push(PolicyViolation {
                    kind: "missing-language".to_string(),
                    message: format!("Entry {:?} is missing required field `language`", entry.id),
                    path: None,
                    entry_id: Some(entry.id.clone()),
                });
            }
            if entry.owner.trim().is_empty() {
                violations.push(PolicyViolation {
                    kind: "missing-owner".to_string(),
                    message: format!("Entry {:?} is missing required field `owner`", entry.id),
                    path: None,
                    entry_id: Some(entry.id.clone()),
                });
            }
            if entry.reason.trim().is_empty() {
                violations.push(PolicyViolation {
                    kind: "missing-reason".to_string(),
                    message: format!("Entry {:?} is missing required field `reason`", entry.id),
                    path: None,
                    entry_id: Some(entry.id.clone()),
                });
            }
            if entry.surface.trim().is_empty() {
                violations.push(PolicyViolation {
                    kind: "missing-surface".to_string(),
                    message: format!("Entry {:?} is missing required field `surface`", entry.id),
                    path: None,
                    entry_id: Some(entry.id.clone()),
                });
            }
            if entry.classification.trim().is_empty() {
                violations.push(PolicyViolation {
                    kind: "missing-classification".to_string(),
                    message: format!(
                        "Entry {:?} is missing required field `classification`",
                        entry.id
                    ),
                    path: None,
                    entry_id: Some(entry.id.clone()),
                });
            }
            if entry.covered_by.is_empty() {
                violations.push(PolicyViolation {
                    kind: "missing-covered-by".to_string(),
                    message: format!("Entry {:?} is missing required field `covered_by`", entry.id),
                    path: None,
                    entry_id: Some(entry.id.clone()),
                });
            }
        }

        // The following checks are strict-only.
        if mode != CheckFilePolicyMode::BlockingStrict {
            continue;
        }

        // --- Stale review_after ---
        if !entry.review_after.is_empty() && is_past_date(&entry.review_after) {
            violations.push(PolicyViolation {
                kind: "stale-review-after".to_string(),
                message: format!(
                    "Entry {:?} review_after={} is in the past",
                    entry.id, entry.review_after
                ),
                path: None,
                entry_id: Some(entry.id.clone()),
            });
        }

        // --- Unused entries ---
        if has_glob ^ has_path && !entry_matches_any_tracked_file(entry, tracked) {
            violations.push(PolicyViolation {
                kind: "unused-entry".to_string(),
                message: format!("Entry {:?} matches no tracked file", entry.id),
                path: entry.path.clone().or_else(|| entry.glob.clone()),
                entry_id: Some(entry.id.clone()),
            });
        }

        // --- Absolute or backslash paths ---
        let path_or_glob = entry.glob.as_deref().or(entry.path.as_deref()).unwrap_or("");
        if path_or_glob.starts_with('/') {
            violations.push(PolicyViolation {
                kind: "absolute-path".to_string(),
                message: format!("Entry {:?} uses an absolute path: {:?}", entry.id, path_or_glob),
                path: Some(path_or_glob.to_string()),
                entry_id: Some(entry.id.clone()),
            });
        }
        if path_or_glob.contains('\\') {
            violations.push(PolicyViolation {
                kind: "backslash-path".to_string(),
                message: format!(
                    "Entry {:?} uses backslashes in path: {:?}",
                    entry.id, path_or_glob
                ),
                path: Some(path_or_glob.to_string()),
                entry_id: Some(entry.id.clone()),
            });
        }

        // --- Broad glob without reason ---
        // Emptiness is judged the same way schema validation judges it: a
        // blank or whitespace-only justification is an absent one, or a broad
        // glob could satisfy enforcement with `broad_glob_reason = ""`.
        if let Some(ref glob_str) = entry.glob
            && is_policy_broad_glob(glob_str)
            && entry.broad_glob_reason.as_deref().is_none_or(|reason| reason.trim().is_empty())
        {
            violations.push(PolicyViolation {
                kind: "broad-glob-no-reason".to_string(),
                message: format!(
                    "Entry {:?} has a broad glob {:?} but no `broad_glob_reason`",
                    entry.id, glob_str
                ),
                path: Some(glob_str.clone()),
                entry_id: Some(entry.id.clone()),
            });
        }
    }

    violations
}

/// Entry point for `cargo xtask non-rust check`.
pub fn check_file_policy(root: &std::path::Path, config: CheckFilePolicyConfig) -> Result<()> {
    // Resolve effective workspace root (allows test seam override).
    let effective_root: std::path::PathBuf =
        if let Some(ref r) = config.root_override { r.clone() } else { root.to_path_buf() };
    let root = effective_root.as_path();

    // Load the raw allowlist before deserializing it so blocking enforcement
    // cannot bypass conflict-marker detection by going through the typed
    // loader. The structural validation path performs the same raw scan.
    let allowlist_path = config
        .allowlist_path
        .clone()
        .unwrap_or_else(|| root.join("policy/non-rust-allowlist.toml"));
    let allowlist_text = fs::read_to_string(&allowlist_path)
        .with_context(|| format!("reading {}", allowlist_path.display()))?;
    let marker_lines = policy_conflict_marker_lines(&allowlist_text);

    let allowlist: Allowlist = toml::from_str(&allowlist_text)
        .with_context(|| format!("parsing {}", allowlist_path.display()))?;

    let entries = &allowlist.allow;

    // Build inventory.
    let tracked = list_tracked_files(root)?;
    let prepared = prepare_allow_entries(entries);

    let mut violations: Vec<PolicyViolation> = Vec::new();

    if !marker_lines.is_empty() && config.mode != CheckFilePolicyMode::Advisory {
        violations.push(PolicyViolation {
            kind: "conflict-marker".to_string(),
            message: format!("Git conflict markers at lines {marker_lines:?}"),
            path: Some(allowlist_path.display().to_string()),
            entry_id: None,
        });
    }

    // --- Per-file classification ---
    let mut non_rust_count = 0usize;
    let mut unclassified_count = 0usize;

    for path in &tracked {
        let record = classify_file_with_prepared(path, &prepared);
        if record.category == "rust" {
            continue;
        }
        non_rust_count += 1;
        if !record.allowlisted {
            unclassified_count += 1;
            if config.mode != CheckFilePolicyMode::Advisory {
                violations.push(PolicyViolation {
                    kind: "unallowlisted-file".to_string(),
                    message: format!("Non-Rust file {path:?} has no allowlist entry"),
                    path: Some(path.clone()),
                    entry_id: None,
                });
            }
        }
    }

    // --- Allowlist entry checks ---
    let entry_violations = check_allowlist_entries(entries, config.mode, &tracked);
    let expired_count = expired_entry_count(entries);
    let stale_review_after_count = stale_review_after_count(entries);
    let duplicate_ids_count = duplicate_id_count(entries);
    let unused_entries = unused_entry_count(entries, &tracked);
    violations.extend(entry_violations);

    // --- Build receipt ---
    let receipt = FilePolicyReceipt {
        schema_version: 1,
        mode: config.mode.to_string(),
        total_tracked: tracked.len(),
        non_rust: non_rust_count,
        unclassified: unclassified_count,
        expired: expired_count,
        stale_review_after: stale_review_after_count,
        duplicate_ids: duplicate_ids_count,
        unused_entries,
        violations: violations.clone(),
    };

    // --- Emit output ---
    let json =
        serde_json::to_string_pretty(&receipt).context("failed to serialize policy receipt")?;

    let json_path = config
        .json_output
        .clone()
        .unwrap_or_else(|| root.join("target/policy/file-policy-report.json"));
    let markdown_path = if config.json_output.is_none() {
        Some(root.join("target/policy/file-policy-report.md"))
    } else {
        None
    };

    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&json_path, &json)
        .with_context(|| format!("writing receipt to {}", json_path.display()))?;
    println!("  wrote {}", json_path.display());

    if let Some(markdown_path) = markdown_path {
        if let Some(parent) = markdown_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let markdown = render_policy_report_markdown(&receipt);
        fs::write(&markdown_path, &markdown)
            .with_context(|| format!("writing report to {}", markdown_path.display()))?;
        println!("  wrote {}", markdown_path.display());
    }

    // Human-readable summary.
    println!("check-file-policy (mode: {})", config.mode);
    println!(
        "  total tracked: {}  non-Rust: {}  unclassified: {}",
        receipt.total_tracked, receipt.non_rust, receipt.unclassified
    );
    println!(
        "  expired entries: {}  stale review_after: {}  unused entries: {}",
        expired_count, stale_review_after_count, unused_entries
    );
    if violations.is_empty() {
        println!("  result: OK — no violations");
    } else {
        println!("  result: {} violation(s)", violations.len());
        for v in &violations {
            let loc = v.path.as_deref().or(v.entry_id.as_deref()).unwrap_or("");
            println!(
                "    [{}] {}{}",
                v.kind,
                if loc.is_empty() { String::new() } else { format!("{loc}: ") },
                v.message
            );
        }
    }

    // Decide exit code based on mode.
    if config.mode != CheckFilePolicyMode::Advisory && !violations.is_empty() {
        bail!("file policy check found {} violation(s)", violations.len());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Proposal generator — `cargo xtask non-rust propose`
// ---------------------------------------------------------------------------

/// Grouping strategy for `cargo xtask non-rust propose`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProposeGroupBy {
    /// Group by top-level directory (default).
    Directory,
    /// Group by file extension.
    Extension,
}

/// Configuration for `cargo xtask non-rust propose`.
pub struct ProposeConfig {
    /// Output directory (defaults to `target/policy`).
    pub output_dir: std::path::PathBuf,
    /// How to group unclassified files.
    pub group_by: ProposeGroupBy,
    /// Override the workspace root used for `git ls-files` (test seam).
    pub root_override: Option<std::path::PathBuf>,
}

/// Return today's date as `YYYY-MM-DD` using Unix timestamp arithmetic.
pub fn today_ymd() -> (u32, u32, u32) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = secs / 86400;
    days_to_ymd(days)
}

/// Add `n` days to a `(year, month, day)` tuple using the Julian Day Number
/// method. Accurate for years 1970-2200.
pub fn add_days(ymd: (u32, u32, u32), n: u32) -> (u32, u32, u32) {
    // Convert (y, m, d) → JDN using the standard proleptic Gregorian formula.
    // All arithmetic is signed to avoid underflow.
    let (year, month, day) = (ymd.0 as i64, ymd.1 as i64, ymd.2 as i64);
    let a = (14 - month) / 12;
    let y = year + 4800 - a;
    let m = month + 12 * a - 3;
    let jdn = day + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32045;
    // JDN for Unix epoch (1970-01-01) = 2440588.
    let unix_days = (jdn - 2_440_588 + n as i64) as u64;
    days_to_ymd(unix_days)
}

/// Format a `(year, month, day)` tuple as `YYYY-MM-DD`.
pub fn fmt_ymd(ymd: (u32, u32, u32)) -> String {
    format!("{:04}-{:02}-{:02}", ymd.0, ymd.1, ymd.2)
}

/// Heuristic: infer classification from a top-level directory name.
fn classify_dir(dir: &str) -> &'static str {
    match dir {
        "docs" | "doc" | "book" | "guide" | "guides" | "wiki" | "website" | "pages" => "docs",
        "test" | "tests" | "t" | "spec" | "specs" | "fixtures" | "test_corpus" | "test-corpus" => {
            "test"
        }
        "vendor" | "third_party" | "third-party" | "extern" | "external" => "vendor",
        "scripts" | "bin" | "tools" | "tool" | "ci" | ".ci" | ".github" | "xtask" => "build",
        "data" | "assets" | "static" | "public" | "resources" | "corpus" | "samples" => "data",
        "vscode-extension" | "vscode" | "editor" | "editors" => "data",
        _ => "tbd",
    }
}

/// Heuristic: infer classification from a file extension.
fn classify_ext(ext: &str) -> &'static str {
    match ext {
        "md" | "rst" | "txt" | "adoc" | "asciidoc" => "docs",
        "toml" | "yaml" | "yml" | "json" | "ron" | "json5" => "build",
        "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" => "build",
        "py" | "js" | "ts" | "rb" | "pl" | "pm" | "lua" | "tcl" => "build",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" => "data",
        "woff" | "woff2" | "ttf" | "eot" | "otf" => "data",
        "pdf" | "docx" | "xlsx" | "pptx" => "docs",
        "nix" | "lock" | "makefile" | "mk" | "cmake" => "build",
        "html" | "css" | "scss" | "less" => "data",
        "proto" | "thrift" | "avsc" => "data",
        "csv" | "tsv" | "parquet" => "data",
        "" => "tbd",
        _ => "tbd",
    }
}

/// Entry point for `cargo xtask non-rust propose`.
///
/// Reads the current inventory, groups unclassified files by the chosen
/// strategy, and writes two output files:
///
/// - `<output_dir>/non-rust-proposed-allowlist.toml` — draft allowlist entries.
/// - `<output_dir>/non-rust-proposal.md` — human-readable summary.
///
/// The canonical `policy/non-rust-allowlist.toml` is NEVER modified.
pub fn non_rust_propose(root: &Path, config: ProposeConfig) -> Result<()> {
    let effective_root: std::path::PathBuf =
        if let Some(ref r) = config.root_override { r.clone() } else { root.to_path_buf() };
    let root = effective_root.as_path();

    println!("Building inventory for proposal generation...");

    let allowlist = load_allowlist(root)?;
    let tracked = list_tracked_files(root)?;
    let prepared = prepare_allow_entries(&allowlist.allow);

    // Collect unclassified non-Rust files.
    let unclassified: Vec<String> = tracked
        .iter()
        .filter_map(|p| {
            let record = classify_file_with_prepared(p, &prepared);
            if record.category == "unclassified" { Some(p.clone()) } else { None }
        })
        .collect();

    println!("  {} unclassified files to group", unclassified.len());

    // Group files.
    let groups: BTreeMap<String, Vec<String>> = match config.group_by {
        ProposeGroupBy::Directory => group_by_directory(&unclassified),
        ProposeGroupBy::Extension => group_by_extension(&unclassified),
    };

    let today = today_ymd();
    let review_after = add_days(today, 30);
    let today_str = fmt_ymd(today);
    let review_after_str = fmt_ymd(review_after);

    // Build proposed AllowEntry list.
    let mut entries: Vec<AllowEntry> = Vec::new();
    for (group_key, files) in &groups {
        let (glob_pattern, entry_id) = match config.group_by {
            ProposeGroupBy::Directory => {
                // "(root)" is a virtual key for files that have no parent directory.
                // Their glob is simply "*" (all root-level files).
                let glob = if group_key == "(root)" {
                    "*".to_string()
                } else {
                    format!("{group_key}/**/*")
                };
                let sanitized = group_key
                    .chars()
                    .map(|c| if c == '/' || c == '.' || c == '(' || c == ')' { '-' } else { c })
                    .collect::<String>()
                    .to_lowercase();
                let id = format!("proposed-dir-{sanitized}");
                (glob, id)
            }
            ProposeGroupBy::Extension => {
                let glob = if group_key.is_empty() {
                    // Files with no extension — list individually or use a tbd glob.
                    "**/*".to_string()
                } else {
                    format!("**/*.{group_key}")
                };
                let id = if group_key.is_empty() {
                    "proposed-ext-no-extension".to_string()
                } else {
                    format!("proposed-ext-{}", group_key.to_lowercase())
                };
                (glob, id)
            }
        };

        let classification = match config.group_by {
            ProposeGroupBy::Directory => {
                let top = group_key.split('/').next().unwrap_or(group_key.as_str());
                classify_dir(top)
            }
            ProposeGroupBy::Extension => classify_ext(group_key.as_str()),
        };

        let reason = match config.group_by {
            ProposeGroupBy::Directory => {
                format!("auto-proposed: {} files in {}/", files.len(), group_key)
            }
            ProposeGroupBy::Extension => {
                let ext_label = if group_key.is_empty() {
                    "(no extension)".to_string()
                } else {
                    format!(".{group_key}")
                };
                format!("auto-proposed: {} {} files", files.len(), ext_label)
            }
        };

        let broad_glob_reason = Some(
            "auto-proposed bulk classification — refine per-directory before promotion".to_string(),
        );

        entries.push(AllowEntry {
            id: entry_id,
            glob: Some(glob_pattern.clone()),
            path: None,
            kind: "non-rust".to_string(),
            language: "mixed".to_string(),
            surface: "unclassified".to_string(),
            classification: classification.to_string(),
            owner: "TBD".to_string(),
            reason,
            covered_by: vec![glob_pattern],
            created: today_str.clone(),
            review_after: review_after_str.clone(),
            expires: None,
            broad_glob_reason,
            retired: false,
        });
    }

    // Write output files.
    fs::create_dir_all(&config.output_dir)
        .with_context(|| format!("creating {}", config.output_dir.display()))?;

    let toml_path = config.output_dir.join("non-rust-proposed-allowlist.toml");
    let md_path = config.output_dir.join("non-rust-proposal.md");

    let toml_content = render_proposed_toml(&entries, config.group_by, today_str.as_str())?;
    fs::write(&toml_path, &toml_content)
        .with_context(|| format!("writing {}", toml_path.display()))?;
    println!("  wrote {}", toml_path.display());

    let md_content = render_proposal_markdown(&groups, &entries, config.group_by, &unclassified);
    fs::write(&md_path, &md_content).with_context(|| format!("writing {}", md_path.display()))?;
    println!("  wrote {}", md_path.display());

    println!(
        "\nProposal complete: {} unclassified files → {} groups\n\
         Review {} and {} before promoting to policy/non-rust-allowlist.toml",
        unclassified.len(),
        groups.len(),
        toml_path.display(),
        md_path.display()
    );

    Ok(())
}

/// Group files by their top-level directory component.
fn group_by_directory(files: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files {
        let top_dir = file.split('/').next().unwrap_or(file.as_str());
        // If a file has no directory component, group under "(root)".
        let key = if file.contains('/') { top_dir.to_string() } else { "(root)".to_string() };
        groups.entry(key).or_default().push(file.clone());
    }
    groups
}

/// Group files by their file extension (without leading dot).
fn group_by_extension(files: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files {
        let basename = file.rsplit('/').next().unwrap_or(file.as_str());
        let ext = basename
            .rsplit_once('.')
            .filter(|(stem, ext)| !stem.is_empty() && !ext.is_empty())
            .map(|(_, e)| e)
            .unwrap_or("")
            .to_lowercase();
        groups.entry(ext).or_default().push(file.clone());
    }
    groups
}

/// Render the proposed allowlist as TOML.
fn render_proposed_toml(
    entries: &[AllowEntry],
    group_by: ProposeGroupBy,
    today: &str,
) -> Result<String> {
    let group_label = match group_by {
        ProposeGroupBy::Directory => "directory",
        ProposeGroupBy::Extension => "extension",
    };

    let mut out = String::new();
    out.push_str("# Non-Rust Proposed Allowlist\n");
    out.push_str("#\n");
    out.push_str("# AUTO-GENERATED by `cargo xtask non-rust propose`.\n");
    out.push_str("# DO NOT edit directly. Review each entry and promote to\n");
    out.push_str("# policy/non-rust-allowlist.toml after setting owner/surface/classification.\n");
    out.push_str("#\n");
    out.push_str(&format!("# Generated: {today}\n"));
    out.push_str(&format!("# Grouped by: {group_label}\n"));
    out.push_str("#\n");
    out.push_str("# Fields marked TBD MUST be set by a human reviewer\n");
    out.push_str("# before promoting any entry into the canonical ledger.\n\n");

    out.push_str("schema_version = 1\n");
    out.push_str("policy = \"non-rust-allowlist\"\n");
    out.push_str("owner = \"TBD\"\n");
    out.push_str("status = \"proposed\"\n");
    out.push_str(&format!("updated = \"{today}\"\n\n"));

    out.push_str("[defaults]\n");
    out.push_str("rust_is_default = true\n");
    out.push_str("xtask_is_default_for_repo_automation = true\n");
    out.push_str("new_non_rust_requires_review = true\n");
    out.push_str("broad_globs_require_reason = true\n");
    out.push_str("coverage_required_for_production_surfaces = true\n\n");

    for entry in entries {
        out.push_str("[[allow]]\n");
        out.push_str(&format!("id = {:?}\n", entry.id));
        if let Some(ref g) = entry.glob {
            out.push_str(&format!("glob = {:?}\n", g));
        }
        if let Some(ref p) = entry.path {
            out.push_str(&format!("path = {:?}\n", p));
        }
        out.push_str(&format!("kind = {:?}\n", entry.kind));
        out.push_str(&format!("language = {:?}\n", entry.language));
        out.push_str(&format!("surface = {:?}\n", entry.surface));
        out.push_str(&format!("classification = {:?}\n", entry.classification));
        out.push_str(&format!("owner = {:?}\n", entry.owner));
        out.push_str(&format!("reason = {:?}\n", entry.reason));
        // covered_by array
        out.push_str("covered_by = [");
        for (i, c) in entry.covered_by.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{c:?}"));
        }
        out.push_str("]\n");
        out.push_str(&format!("created = {:?}\n", entry.created));
        out.push_str(&format!("review_after = {:?}\n", entry.review_after));
        if let Some(ref bgr) = entry.broad_glob_reason {
            out.push_str(&format!("broad_glob_reason = {:?}\n", bgr));
        }
        out.push('\n');
    }

    Ok(out)
}

/// Render a human-readable markdown summary of the proposal.
fn render_proposal_markdown(
    groups: &BTreeMap<String, Vec<String>>,
    entries: &[AllowEntry],
    group_by: ProposeGroupBy,
    all_unclassified: &[String],
) -> String {
    let group_label = match group_by {
        ProposeGroupBy::Directory => "directory",
        ProposeGroupBy::Extension => "extension",
    };

    // Extension breakdown for summary.
    let mut ext_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for file in all_unclassified {
        let basename = file.rsplit('/').next().unwrap_or(file.as_str());
        let ext = basename.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        *ext_counts.entry(ext).or_insert(0) += 1;
    }

    let mut out = String::new();
    out.push_str("# Non-Rust Allowlist Proposal\n\n");
    out.push_str("> AUTO-GENERATED by `cargo xtask non-rust propose`. Do not edit by hand.\n");
    out.push_str("> Review each group, set `owner`/`surface`/`classification`, then promote\n");
    out.push_str("> to `policy/non-rust-allowlist.toml`.\n\n");

    out.push_str("## Summary\n\n");
    out.push_str(&format!(
        "| Metric | Value |\n|---|---|\n\
         | Unclassified files | {} |\n\
         | Groups ({group_label}) | {} |\n\
         | Proposed entries | {} |\n\n",
        all_unclassified.len(),
        groups.len(),
        entries.len(),
    ));

    out.push_str("## Top extensions\n\n");
    out.push_str("| Extension | Count |\n|---|---|\n");
    let mut ext_vec: Vec<(&&str, &usize)> = ext_counts.iter().collect();
    ext_vec.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (ext, count) in ext_vec.iter().take(20) {
        let label = if ext.is_empty() { "(no ext)" } else { ext };
        out.push_str(&format!("| `.{label}` | {count} |\n"));
    }
    out.push('\n');

    let migration_candidates: Vec<_> = groups
        .iter()
        .filter_map(|(group_key, files)| {
            group_migration_rule(files).map(|rule| (group_key, files, rule))
        })
        .collect();

    out.push_str("## Rust migration candidates\n\n");
    if migration_candidates.is_empty() {
        out.push_str("No unclassified repo-automation scripts were detected in this proposal.\n\n");
    } else {
        out.push_str(
            "These groups contain non-Rust automation that should be reviewed for conversion \
             into Rust-owned tooling before broad allowlist promotion.\n\n",
        );
        out.push_str(
            "| Group | Files | Recommended destination | Rationale |\n|---|---:|---|---|\n",
        );
        for (group_key, files, rule) in &migration_candidates {
            out.push_str(&format!(
                "| `{group_key}` | {} | `{}` | {} |\n",
                files.len(),
                rule.target,
                rule.rationale
            ));
        }
        out.push('\n');
    }

    out.push_str(&format!("## Groups by {group_label}\n\n"));
    for (group_key, files) in groups {
        let entry_id = entries
            .iter()
            .find(|e| {
                e.reason.contains(&format!("{}/", group_key))
                    || e.reason.contains(group_key.as_str())
            })
            .map(|e| e.id.as_str())
            .unwrap_or("—");
        out.push_str(&format!("### `{group_key}` ({} files)\n\n", files.len()));
        out.push_str(&format!("- Proposed entry: `{entry_id}`\n"));
        out.push_str("- `owner`: TBD — must be set before promotion\n");
        out.push_str("- `surface`: unclassified — must be refined\n");
        if let Some(rule) = group_migration_rule(files) {
            out.push_str(&format!(
                "- Rust migration review: {} Target: `{}`.\n",
                rule.rationale, rule.target
            ));
        }
        // Show first 10 files as examples.
        if !files.is_empty() {
            out.push_str("- Sample files:\n");
            for f in files.iter().take(10) {
                out.push_str(&format!("  - `{f}`\n"));
            }
            if files.len() > 10 {
                out.push_str(&format!("  - … and {} more\n", files.len() - 10));
            }
        }
        out.push('\n');
    }

    out.push_str("## Next steps\n\n");
    out.push_str("1. Review `target/policy/non-rust-proposed-allowlist.toml`.\n");
    out.push_str("2. For each entry: set `owner`, `surface`, refine `classification`.\n");
    out.push_str("3. Copy approved entries into `policy/non-rust-allowlist.toml`.\n");
    out.push_str("4. Run `cargo xtask check-file-policy --mode advisory` to verify.\n");
    out.push_str("5. Do NOT promote entries with `owner = \"TBD\"`.\n");

    out
}

// ---------------------------------------------------------------------------
// Migration candidate finder — `cargo xtask non-rust migration-candidates`
// ---------------------------------------------------------------------------

/// Output format for non-Rust migration candidate reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationCandidateFormat {
    /// Human-readable Markdown.
    Markdown,
    /// Machine-readable JSON.
    Json,
}

/// Configuration for `cargo xtask non-rust migration-candidates`.
pub struct MigrationCandidatesConfig {
    /// Output format.
    pub format: MigrationCandidateFormat,
    /// Optional output path. Prints to stdout when omitted.
    pub output: Option<std::path::PathBuf>,
    /// Maximum number of candidates to include.
    pub limit: Option<usize>,
    /// Override the workspace root used for `git ls-files` (test seam).
    pub root_override: Option<std::path::PathBuf>,
}

/// One non-Rust file that looks like a Rust migration candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationCandidate {
    /// Repo-relative path.
    pub path: String,
    /// Language family inferred from extension and filename.
    pub language: String,
    /// Suggested Rust-owned destination.
    pub target: String,
    /// Stable priority bucket for review ordering.
    pub priority: String,
    /// Why the file belongs in the suggested target.
    pub rationale: String,
}

#[derive(Clone, Copy)]
struct MigrationRule {
    language: &'static str,
    target: &'static str,
    priority: &'static str,
    rationale: &'static str,
}

/// Entry point for `cargo xtask non-rust migration-candidates`.
///
/// The command is intentionally read-only. It identifies script-style tooling
/// that is already in a Rust-owned architectural lane (for example corpus
/// tooling belongs in `perl-corpus`, repo automation belongs in `xtask`) and
/// emits a deterministic review queue for future focused migration PRs.
pub fn non_rust_migration_candidates(root: &Path, config: MigrationCandidatesConfig) -> Result<()> {
    let effective_root: std::path::PathBuf =
        if let Some(ref r) = config.root_override { r.clone() } else { root.to_path_buf() };

    let mut candidates = collect_migration_candidates(effective_root.as_path())?;
    if let Some(limit) = config.limit {
        candidates.truncate(limit);
    }

    let rendered = match config.format {
        MigrationCandidateFormat::Markdown => render_migration_candidates_markdown(&candidates),
        MigrationCandidateFormat::Json => serde_json::to_string_pretty(&candidates)?,
    };

    if let Some(output) = config.output {
        if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&output, rendered).with_context(|| format!("writing {}", output.display()))?;
        println!("wrote {} migration candidates to {}", candidates.len(), output.display());
    } else {
        println!("{rendered}");
    }

    Ok(())
}

fn collect_migration_candidates(root: &Path) -> Result<Vec<MigrationCandidate>> {
    let tracked = list_tracked_files(root)?;
    let mut candidates: Vec<MigrationCandidate> = tracked
        .iter()
        .filter_map(|path| {
            migration_rule_for_path(path).map(|rule| candidate_from_rule(path, rule))
        })
        .collect();

    candidates.sort_by(|a, b| {
        priority_rank(&a.priority)
            .cmp(&priority_rank(&b.priority))
            .then_with(|| a.target.cmp(&b.target))
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(candidates)
}

fn candidate_from_rule(path: &str, rule: MigrationRule) -> MigrationCandidate {
    MigrationCandidate {
        path: path.to_string(),
        language: rule.language.to_string(),
        target: rule.target.to_string(),
        priority: rule.priority.to_string(),
        rationale: rule.rationale.to_string(),
    }
}

fn priority_rank(priority: &str) -> u8 {
    match priority {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    }
}

fn migration_rule_for_path(path: &str) -> Option<MigrationRule> {
    let language = script_language(path)?;

    if path.starts_with("tools/corpus_") || path == "tools/add_metadata.py" {
        return Some(MigrationRule {
            language,
            target: "crates/perl-corpus",
            priority: "high",
            rationale: "Corpus linting, indexing, and metadata helpers belong with the Rust corpus crate and its `perl-corpus` CLI.",
        });
    }

    if path.starts_with("benchmarks/scripts/") {
        return Some(MigrationRule {
            language,
            target: "xtask benchmark/metrics tasks",
            priority: "medium",
            rationale: "Benchmark orchestration and result formatting should share Rust workspace metadata, receipts, and error handling through xtask.",
        });
    }

    if path.starts_with("ci/") {
        return Some(MigrationRule {
            language,
            target: "xtask policy/check tasks",
            priority: "medium",
            rationale: "CI policy checks should use the same Rust policy modules that local agent and gate commands exercise.",
        });
    }

    if path.starts_with("scripts/ci/") || path.starts_with("scripts/policy/") {
        return Some(MigrationRule {
            language,
            target: "xtask policy/check tasks",
            priority: "medium",
            rationale: "Policy and CI receipts are core repository automation and should be implemented as typed xtask tasks.",
        });
    }

    if path.starts_with("scripts/") && path.ends_with(".py") {
        return Some(MigrationRule {
            language,
            target: "xtask tasks",
            priority: "medium",
            rationale: "Python repository automation should migrate to xtask when it does not require a Python-specific ecosystem API.",
        });
    }

    if path.starts_with("bin/") || path.starts_with("tools/") {
        return Some(MigrationRule {
            language,
            target: "xtask tasks",
            priority: "medium",
            rationale: "Repository helper scripts should migrate to typed xtask tasks when they do not require a language-specific ecosystem API.",
        });
    }

    if path == "install.sh" || path == "install.ps1" {
        return Some(MigrationRule {
            language,
            target: "install-surface checks",
            priority: "low",
            rationale: "Installer validation and shared install-surface logic should live in Rust-owned release checks.",
        });
    }

    if path.starts_with("scripts/check-") || path.starts_with("scripts/validate-") {
        return Some(MigrationRule {
            language,
            target: "xtask policy/check tasks",
            priority: "low",
            rationale: "Shell validation wrappers are candidates for typed xtask checks once their external command surface is stable.",
        });
    }

    None
}

fn group_migration_rule(files: &[String]) -> Option<MigrationRule> {
    files.iter().find_map(|file| migration_rule_for_path(file))
}

fn script_language(path: &str) -> Option<&'static str> {
    let basename = path.rsplit('/').next().unwrap_or(path);
    let ext = basename.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    match ext {
        "py" => Some("python"),
        "sh" | "bash" | "zsh" | "fish" => Some("shell"),
        "ps1" | "bat" | "cmd" => Some("shell"),
        "js" => Some("javascript"),
        "ts" => Some("typescript"),
        "rb" => Some("ruby"),
        "pl" => Some("perl"),
        _ => None,
    }
}

fn render_migration_candidates_markdown(candidates: &[MigrationCandidate]) -> String {
    let mut by_priority: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_target: BTreeMap<&str, usize> = BTreeMap::new();
    for candidate in candidates {
        *by_priority.entry(candidate.priority.as_str()).or_default() += 1;
        *by_target.entry(candidate.target.as_str()).or_default() += 1;
    }

    let mut out = String::new();
    out.push_str("# Non-Rust Migration Candidates\n\n");
    out.push_str("> AUTO-GENERATED by `cargo xtask non-rust migration-candidates`.\n");
    out.push_str("> Use this as a review queue; migrate one concern per PR.\n\n");

    out.push_str("## Summary\n\n");
    out.push_str("| Metric | Value |\n|---|---|\n");
    out.push_str(&format!("| Candidates | {} |\n", candidates.len()));
    out.push_str(&format!("| Targets | {} |\n", by_target.len()));
    out.push('\n');

    out.push_str("## By priority\n\n");
    out.push_str("| Priority | Count |\n|---|---:|\n");
    for priority in ["high", "medium", "low"] {
        if let Some(count) = by_priority.get(priority) {
            out.push_str(&format!("| {priority} | {count} |\n"));
        }
    }
    out.push('\n');

    out.push_str("## By target\n\n");
    out.push_str("| Target | Count |\n|---|---:|\n");
    for (target, count) in by_target {
        out.push_str(&format!("| `{target}` | {count} |\n"));
    }
    out.push('\n');

    out.push_str("## Candidates\n\n");
    out.push_str("| Priority | Path | Language | Target | Rationale |\n");
    out.push_str("|---|---|---|---|---|\n");
    for candidate in candidates {
        out.push_str(&format!(
            "| {} | `{}` | {} | `{}` | {} |\n",
            candidate.priority,
            candidate.path,
            candidate.language,
            candidate.target,
            candidate.rationale
        ));
    }

    out
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::ensure;

    fn make_entry(
        id: &str,
        glob: Option<&str>,
        path: Option<&str>,
        classification: &str,
    ) -> AllowEntry {
        AllowEntry {
            id: id.to_string(),
            glob: glob.map(str::to_string),
            path: path.map(str::to_string),
            kind: "test".to_string(),
            language: "mixed".to_string(),
            surface: "test".to_string(),
            classification: classification.to_string(),
            owner: "test".to_string(),
            reason: "test".to_string(),
            covered_by: vec![],
            created: "2026-01-01".to_string(),
            review_after: "2026-06-01".to_string(),
            expires: None,
            broad_glob_reason: None,
            retired: false,
        }
    }

    fn violation_kinds(violations: &[PolicyViolation]) -> Vec<&str> {
        violations.iter().map(|violation| violation.kind.as_str()).collect()
    }

    /// Two-schema-valid entries whose `reason` prose was spliced from one row
    /// into the other by textual conflict resolution (#9680).
    fn mispaired_allowlist_fixture() -> String {
        r#"
schema_version = 1
policy = "non-rust-allowlist"

[[allow]]
id = "entry-a"
path = "docs/a.md"
kind = "doc"
language = "markdown"
surface = "docs"
classification = "documentation"
owner = "docs"
reason = "Documents the alpha subsystem with its full user contract."
covered_by = ["manual review"]
created = "2026-01-01"
review_after = "2026-06-01"

[[allow]]
id = "entry-b"
path = "docs/b.md"
kind = "doc"
language = "markdown"
surface = "docs"
classification = "documentation"
owner = "docs"
reason = "Documents the alpha subsystem with its full user contract."
covered_by = ["manual review"]
created = "2026-01-01"
review_after = "2026-06-01"
"#
        .to_string()
    }

    #[test]
    fn mispaired_provenance_fails_exact_policy_bytes() -> Result<()> {
        let err =
            validate_exact_policy_bytes(mispaired_allowlist_fixture().as_bytes()).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("provenance is mispaired"), "{message}");
        assert!(message.contains("entry-a") && message.contains("entry-b"), "{message}");
        Ok(())
    }

    #[test]
    fn distinct_reasons_pass_exact_policy_bytes() -> Result<()> {
        let policy = mispaired_allowlist_fixture().replacen(
            "reason = \"Documents the alpha subsystem with its full user contract.\"",
            "reason = \"Documents the beta subsystem with its own operator guide.\"",
            1,
        );
        validate_exact_policy_bytes(policy.as_bytes())?;
        Ok(())
    }

    #[test]
    fn retired_duplicate_reason_passes_exact_policy_bytes() -> Result<()> {
        let policy = mispaired_allowlist_fixture()
            .replace("id = \"entry-b\"", "id = \"entry-b\"\nretired = true");
        validate_exact_policy_bytes(policy.as_bytes())?;
        Ok(())
    }

    #[test]
    fn conflict_marker_inside_multiline_string_fails_exact_policy_bytes() -> Result<()> {
        let policy = mispaired_allowlist_fixture().replace(
            "reason = \"Documents the alpha subsystem with its full user contract.\"",
            "reason = \"\"\"\nKeeps the alpha subsystem documented.\n<<<<<<< HEAD\nKeeps the gamma subsystem documented.\n=======\n>>>>>>> feature-branch\n\"\"\"",
        );
        let err = validate_exact_policy_bytes(policy.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("conflict markers"), "{}", err);
        Ok(())
    }

    #[test]
    fn ordinary_multiline_reason_separator_is_not_a_conflict_marker() -> Result<()> {
        let policy = mispaired_allowlist_fixture().replacen(
            "reason = \"Documents the alpha subsystem with its full user contract.\"",
            "reason = \"\"\"\n======= Overview\nDescribes the alpha subsystem.\n\"\"\"",
            1,
        );
        let policy = policy.replacen(
            "reason = \"Documents the alpha subsystem with its full user contract.\"",
            "reason = \"Documents the beta subsystem with its own operator guide.\"",
            1,
        );
        validate_exact_policy_bytes(policy.as_bytes())?;
        Ok(())
    }

    #[test]
    fn mispaired_provenance_blocks_enforcement_modes_and_spares_advisory() -> Result<()> {
        let allowlist: Allowlist = toml::from_str(&mispaired_allowlist_fixture())?;
        let entries = allowlist.allow;
        let blocking =
            check_allowlist_entries(&entries, CheckFilePolicyMode::BlockingAllowlist, &[]);
        assert!(violation_kinds(&blocking).contains(&"mispaired-provenance"));
        let strict = check_allowlist_entries(&entries, CheckFilePolicyMode::BlockingStrict, &[]);
        assert!(violation_kinds(&strict).contains(&"mispaired-provenance"));
        let advisory = check_allowlist_entries(&entries, CheckFilePolicyMode::Advisory, &[]);
        assert!(!violation_kinds(&advisory).contains(&"mispaired-provenance"));
        Ok(())
    }

    #[test]
    fn policy_table_path_reports_mispaired_provenance_and_conflict_markers() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let allowlist_path = temp.path().join("allowlist.toml");
        std::fs::write(&allowlist_path, mispaired_allowlist_fixture())?;
        let mut errors = Vec::new();
        validate_policy_table(&allowlist_path, "allow", true, &mut errors);
        assert!(errors.iter().any(|error| error.contains("provenance is mispaired")), "{errors:?}");

        let marker_path = temp.path().join("markers.toml");
        std::fs::write(
            &marker_path,
            "[[allow]]\nid = \"entry-a\"\nreason = \"\"\"\nline\n<<<<<<< HEAD\n\"\"\"\n",
        )?;
        let mut marker_errors = Vec::new();
        validate_policy_table(&marker_path, "allow", false, &mut marker_errors);
        assert!(
            marker_errors.iter().any(|error| error.contains("conflict markers")),
            "{marker_errors:?}"
        );
        Ok(())
    }

    #[test]
    fn policy_table_path_scans_markers_before_missing_table_return() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("missing-allow.toml");
        std::fs::write(&path, "# no allow table\n<<<<<<< HEAD\n")?;
        let mut errors = Vec::new();
        assert_eq!(validate_policy_table(&path, "allow", true, &mut errors), 0);
        assert!(errors.iter().any(|error| error.contains("conflict markers")), "{errors:?}");
        Ok(())
    }

    #[test]
    fn expiry_is_valid_on_expiration_date_and_excluded_afterward() -> Result<()> {
        let mut entry = make_entry("expiring", None, Some("docs/a.txt"), "documentation");
        entry.expires = Some("2026-08-31".to_string());
        let entries = [entry];
        let same_day = prepare_allow_entries_at(
            &entries,
            NaiveDate::from_ymd_opt(2026, 8, 31).ok_or_else(|| eyre!("invalid date"))?,
        );
        assert!(find_matching_prepared_entry("docs/a.txt", &same_day).is_some());
        let next_day = prepare_allow_entries_at(
            &entries,
            NaiveDate::from_ymd_opt(2026, 9, 1).ok_or_else(|| eyre!("invalid date"))?,
        );
        assert!(find_matching_prepared_entry("docs/a.txt", &next_day).is_none());
        Ok(())
    }

    #[test]
    fn exact_path_expiry_does_not_use_glob_matching() -> Result<()> {
        let mut entry = make_entry("literal", None, Some("docs/[a].txt"), "documentation");
        entry.expires = Some("2026-01-01".to_string());
        let entries = [entry];
        let active = prepare_allow_entries_at(
            &entries,
            NaiveDate::from_ymd_opt(2026, 2, 1).ok_or_else(|| eyre!("invalid date"))?,
        );
        assert!(find_matching_prepared_entry("docs/[a].txt", &active).is_none());
        assert!(find_matching_prepared_entry("docs/a.txt", &active).is_none());
        Ok(())
    }

    fn write_fixture(root: &Path, relative: &str, contents: &str) -> Result<()> {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating fixture dir {}", parent.display()))?;
        }
        fs::write(&path, contents).with_context(|| format!("writing fixture {}", path.display()))
    }

    fn run_git(root: &Path, args: &[&str]) -> Result<()> {
        let output = std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .with_context(|| format!("running git {}", args.join(" ")))?;
        if output.status.success() {
            return Ok(());
        }

        Err(eyre!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr)))
    }

    fn init_tracked_fixture(root: &Path, files: &[(&str, &str)]) -> Result<Vec<String>> {
        run_git(root, &["init", "-q"])?;
        for (path, contents) in files {
            write_fixture(root, path, contents)?;
            run_git(root, &["add", path])?;
        }
        list_tracked_files(root)
    }

    fn readme_allowlist_toml() -> Result<String> {
        let mut entry = make_entry("readme", None, Some("README.md"), "documentation");
        entry.covered_by = vec!["README.md".to_string()];
        entry.reason = "Fixture documentation.".to_string();
        entry.review_after = "2999-01-01".to_string();
        let entry_toml = toml::to_string(&entry).context("serializing readme allowlist fixture")?;
        Ok(format!("[[allow]]\n{entry_toml}"))
    }

    fn write_readme_allowlist(root: &Path, relative: &str) -> Result<std::path::PathBuf> {
        let path = root.join(relative);
        write_fixture(root, relative, &readme_allowlist_toml()?)?;
        Ok(path)
    }

    #[test]
    fn readme_allowlist_fixture_round_trips_through_policy_schema() -> Result<()> {
        let allowlist_toml = readme_allowlist_toml()?;
        let allowlist: Allowlist = toml::from_str(&allowlist_toml)?;
        let entry =
            allowlist.allow.first().ok_or_else(|| eyre!("expected readme allowlist entry"))?;

        assert_eq!(allowlist.allow.len(), 1);
        assert_eq!(entry.id, "readme");
        assert_eq!(entry.path.as_deref(), Some("README.md"));
        assert_eq!(entry.covered_by, vec!["README.md".to_string()]);
        Ok(())
    }

    // --- migration candidate finder ---

    #[test]
    fn migration_rule_routes_corpus_tools_to_perl_corpus() -> Result<()> {
        let candidate = migration_rule_for_path("tools/corpus_lint.py")
            .ok_or_else(|| eyre!("expected corpus lint tool to be a migration candidate"))?;

        assert_eq!(candidate.language, "python");
        assert_eq!(candidate.priority, "high");
        assert_eq!(candidate.target, "crates/perl-corpus");
        assert!(candidate.rationale.contains("perl-corpus"));
        Ok(())
    }

    #[test]
    fn migration_rule_routes_ci_shell_checks_to_xtask_policy() -> Result<()> {
        let candidate = migration_rule_for_path("ci/check_doc_hygiene.sh")
            .ok_or_else(|| eyre!("expected CI shell check to be a migration candidate"))?;

        assert_eq!(candidate.language, "shell");
        assert_eq!(candidate.priority, "medium");
        assert_eq!(candidate.target, "xtask policy/check tasks");
        Ok(())
    }

    #[test]
    fn migration_rule_ignores_data_and_rust_files() {
        assert!(migration_rule_for_path("test_corpus/basic_constructs.pl").is_none());
        assert!(migration_rule_for_path("xtask/src/main.rs").is_none());
    }

    #[test]
    fn render_migration_candidates_markdown_summarizes_targets() {
        let candidates = vec![candidate_from_rule(
            "tools/corpus_index.py",
            MigrationRule {
                language: "python",
                target: "crates/perl-corpus",
                priority: "high",
                rationale: "Corpus indexing belongs in the Rust corpus CLI.",
            },
        )];

        let report = render_migration_candidates_markdown(&candidates);

        assert!(report.contains("# Non-Rust Migration Candidates"));
        assert!(report.contains("| Candidates | 1 |"));
        assert!(report.contains("`tools/corpus_index.py`"));
        assert!(report.contains("`crates/perl-corpus`"));
    }

    // --- is_rust_file ---

    #[test]
    fn rust_extension_is_rust() {
        assert!(is_rust_file("src/main.rs"));
        assert!(is_rust_file("crates/foo/src/lib.rs"));
    }

    #[test]
    fn rust_well_known_names_are_rust() {
        assert!(is_rust_file("Cargo.toml"));
        assert!(is_rust_file("path/to/Cargo.toml"));
        assert!(is_rust_file("Cargo.lock"));
        assert!(is_rust_file("rust-toolchain.toml"));
        assert!(is_rust_file("clippy.toml"));
        assert!(is_rust_file("rustfmt.toml"));
    }

    #[test]
    fn non_rust_files_return_false() {
        assert!(!is_rust_file("README.md"));
        assert!(!is_rust_file("justfile"));
        assert!(!is_rust_file("flake.nix"));
        assert!(!is_rust_file("features.toml"));
        assert!(!is_rust_file(".github/workflows/ci.yml"));
        assert!(!is_rust_file("test_corpus/foo.pl"));
    }

    // --- classify_file ---

    #[test]
    fn classify_rs_file_as_rust() {
        let rec = classify_file("src/lib.rs", &[]);
        assert_eq!(rec.category, "rust");
        assert!(!rec.allowlisted);
        assert!(rec.entry.is_none());
    }

    #[test]
    fn classify_unknown_file_as_unclassified() {
        let rec = classify_file("strange/file.xyz", &[]);
        assert_eq!(rec.category, "unclassified");
        assert!(!rec.allowlisted);
    }

    #[test]
    fn classify_exact_path_match() {
        let entries = vec![make_entry("e1", None, Some("justfile"), "tooling")];
        let rec = classify_file("justfile", &entries);
        assert_eq!(rec.category, "tooling");
        assert!(rec.allowlisted);
        assert_eq!(rec.entry.as_ref().map(|e| e.id.as_str()), Some("e1"));
    }

    #[test]
    fn exact_path_does_not_match_other_paths() {
        let entries = vec![make_entry("e1", None, Some("justfile"), "tooling")];
        let rec = classify_file("other-file", &entries);
        assert_eq!(rec.category, "unclassified");
        assert!(!rec.allowlisted);
    }

    #[test]
    fn classify_glob_match() {
        let entries = vec![make_entry("docs", Some("docs/**"), None, "documentation")];
        let rec = classify_file("docs/policy/FILE_POLICY.md", &entries);
        assert_eq!(rec.category, "documentation");
        assert!(rec.allowlisted);
    }

    #[test]
    fn glob_does_not_match_outside_tree() {
        let entries = vec![make_entry("docs", Some("docs/**"), None, "documentation")];
        let rec = classify_file("README.md", &entries);
        assert_eq!(rec.category, "unclassified");
        assert!(!rec.allowlisted);
    }

    #[test]
    fn retired_entry_is_skipped() {
        let mut entry = make_entry("retired", Some("docs/**"), None, "documentation");
        entry.retired = true;
        let entries = vec![entry];
        let rec = classify_file("docs/policy/FILE_POLICY.md", &entries);
        assert_eq!(rec.category, "unclassified", "retired entry must not match");
    }

    // --- glob breadth: `*` is one segment, `**` is a tree (#9994) ---

    /// The repository root, for proofs that must bind to the shipped policy
    /// rather than a fixture copy of it.
    fn repo_root() -> Result<std::path::PathBuf> {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| eyre!("{} has no workspace-root parent", manifest_dir.display()))
    }

    /// The single production rule that governs unreleased Changie fragments.
    fn changie_fragment_entry() -> Result<AllowEntry> {
        let allowlist = load_allowlist(&repo_root()?)?;
        allowlist
            .allow
            .into_iter()
            .find(|entry| entry.id == CHANGIE_FRAGMENT_ENTRY_ID)
            .ok_or_else(|| eyre!("allowlist is missing entry {CHANGIE_FRAGMENT_ENTRY_ID}"))
    }

    const CHANGIE_FRAGMENT_ENTRY_ID: &str = "non-rust-changelog-fragments";

    #[test]
    fn single_star_glob_stops_at_a_directory_separator() {
        let entries = vec![make_entry("fragments", Some("a/b/*.yaml"), None, "documentation")];

        assert!(classify_file("a/b/one.yaml", &entries).allowlisted, "direct child must match");
        assert!(
            !classify_file("a/b/nested/one.yaml", &entries).allowlisted,
            "`*` must not cross `/`: a nested path cannot inherit a single-segment entry"
        );
        assert!(
            !classify_file("a/b/nested/deeper/one.yaml", &entries).allowlisted,
            "`*` must not cross `/` at any depth"
        );
    }

    #[test]
    fn double_star_glob_still_owns_a_whole_tree() {
        // Negative control for the fix above: tree-shaped entries must keep
        // matching nested paths, or the matcher change would silently strip
        // coverage from every `**` entry in the ledger.
        let entries = vec![make_entry("docs", Some("docs/**"), None, "documentation")];

        assert!(classify_file("docs/policy/FILE_POLICY.md", &entries).allowlisted);
        assert!(classify_file("docs/a/b/c/deep.md", &entries).allowlisted);
    }

    #[test]
    fn leading_double_star_glob_matches_root_and_nested_paths() {
        // `**/*.md` is the breadth `non-rust-root-governance-docs` carries.
        let entries = vec![make_entry("markdown", Some("**/*.md"), None, "documentation")];

        assert!(classify_file("README.md", &entries).allowlisted, "root-level markdown");
        assert!(classify_file("book/src/intro.md", &entries).allowlisted, "nested markdown");
    }

    #[test]
    fn nested_markdown_keeps_the_coverage_the_loose_matcher_used_to_supply() -> Result<()> {
        // Tightening `*` to one segment would have dropped every nested
        // markdown file out of the ledger, because the entry that covered them
        // was written `*.md` and only reached them through the loose matcher.
        // The breadth is now declared instead of accidental; this proves the
        // fix did not quietly shrink what the policy governs.
        let allowlist = load_allowlist(&repo_root()?)?;
        let prepared = prepare_allow_entries(&allowlist.allow);

        for nested in [
            ".claude/skills/deliver-pr/SKILL.md",
            "docs/policy/NON_RUST_POLICY.md",
            "book/src/SUMMARY.md",
        ] {
            assert!(
                find_matching_prepared_entry(nested, &prepared).is_some(),
                "{nested} must stay governed by the allowlist"
            );
        }
        Ok(())
    }

    // --- one typed rule governs the Changie fragment class (#9994) ---

    #[test]
    fn changie_rule_governs_exactly_the_fragment_directory() -> Result<()> {
        let entry = changie_fragment_entry()?;
        let entries = vec![entry];

        // Ordinary fragments the producer creates are covered with no edit here.
        for fragment in [
            ".changes/unreleased/product-1-Added-101010.yaml",
            ".changes/unreleased/product-14466-Fixed-000000.yaml",
        ] {
            assert!(
                classify_file(fragment, &entries).allowlisted,
                "{fragment} must be governed by the typed rule"
            );
        }

        // Nothing outside the exact fragment path class may inherit the rule.
        for outsider in [
            ".changes/unreleased/nested/product-1-Added-101010.yaml",
            ".changes/unreleased/archive/2025/product-1-Added-101010.yaml",
            ".changes/product-1-Added-101010.yaml",
            ".changes/v0.1.0.yaml",
            "CHANGELOG.md",
            "docs/product-1-Added-101010.yaml",
            "crates/perl-parser/fixtures/product-1-Added-101010.yaml",
        ] {
            assert!(
                !classify_file(outsider, &entries).allowlisted,
                "{outsider} must not inherit the Changie fragment rule"
            );
        }

        // The rule is bound to the producer's extension, not to YAML generally.
        assert!(
            !classify_file(".changes/unreleased/product-1-Added-101010.yml", &entries).allowlisted,
            "`.yml` is not the fragment extension Changie writes"
        );
        assert!(
            !classify_file(".changes/unreleased/.gitkeep", &entries).allowlisted,
            "the directory keeper is not a fragment and is covered on its own terms"
        );
        Ok(())
    }

    #[test]
    fn changie_rule_is_narrow_enough_to_need_no_broad_glob_justification() -> Result<()> {
        let entry = changie_fragment_entry()?;
        let glob = entry.glob.as_deref().ok_or_else(|| eyre!("fragment rule must be a glob"))?;

        assert!(
            !is_policy_broad_glob(glob),
            "{glob} must stay a narrow path class; a broad matcher would let unrelated files in"
        );
        assert!(
            entry.broad_glob_reason.is_none(),
            "a narrow rule must not carry a decorative broad-glob justification"
        );
        Ok(())
    }

    #[test]
    fn broad_glob_detection_tracks_segment_aware_matching() {
        // `**` is the only token that crosses a directory boundary, so it is
        // the only thing that makes a matcher broad.
        for broad in [
            "**/*.md",
            "**/*.pl",
            "docs/**",
            "**/LICENSE-*",
            "**",
            // `**` counts wherever it sits. Checking only the leading and
            // trailing forms let an internal `**` span a tree while the
            // validator waived `broad_glob_reason`.
            "docs/**/README.md",
            "fixtures/agent_review_packet/**/*.json",
            "crates/perl-parser/**/*.disabled",
        ] {
            assert!(is_policy_broad_glob(broad), "{broad} reaches a whole tree");
        }

        // Single-segment matchers govern one directory. Requiring a
        // broad-glob justification for these would force a misleading
        // rationale onto a narrow rule — and would block #14583 from
        // narrowing the markdown entry back to repository-root governance
        // docs, whose matcher is exactly `*.md`.
        for narrow in
            ["*.md", ".changes/unreleased/*.yaml", "badges/*.json", "crates/*/features_sot.toml"]
        {
            assert!(!is_policy_broad_glob(narrow), "{narrow} governs one directory level");
        }
    }

    #[test]
    fn both_production_validators_agree_on_tree_glob_breadth() {
        // `validate-policy` and `check-file-policy --mode blocking-strict` are
        // both advertised as enforcing the broad-glob justification rule. They
        // used to consult different predicates, so an entry could be refused by
        // one and waved through by the other.
        for glob in ["docs/**/README.md", "fixtures/pkt/**/*.json", "docs/**", "**/*.md"] {
            // A blank justification is treated as absent on both sides;
            // otherwise `broad_glob_reason = ""` would satisfy enforcement
            // while schema validation still refused the entry.
            for reason in [None, Some(String::new()), Some("   \n".to_string())] {
                let mut entry = make_entry("tree", Some(glob), None, "documentation");
                entry.broad_glob_reason = reason.clone();

                let strict = check_allowlist_entries(
                    std::slice::from_ref(&entry),
                    CheckFilePolicyMode::BlockingStrict,
                    &[],
                );
                assert!(
                    violation_kinds(&strict).contains(&"broad-glob-no-reason"),
                    "blocking-strict must refuse {glob} with justification {reason:?}"
                );
            }

            assert!(
                is_policy_broad_glob(glob),
                "schema validation must agree that {glob} is broad"
            );

            // Positive control: a real justification satisfies enforcement, so
            // the rule cannot be passing by refusing everything.
            let mut justified = make_entry("tree", Some(glob), None, "documentation");
            justified.broad_glob_reason = Some("Bounded by one owned tree.".to_string());
            let accepted = check_allowlist_entries(
                std::slice::from_ref(&justified),
                CheckFilePolicyMode::BlockingStrict,
                &[],
            );
            assert!(
                !violation_kinds(&accepted).contains(&"broad-glob-no-reason"),
                "a justified {glob} must not be refused"
            );
        }
    }

    #[test]
    fn broad_changes_tree_glob_is_rejected_without_a_reason() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let allowlist = temp.path().join("allow.toml");
        let debt = temp.path().join("debt.toml");
        fs::write(
            &allowlist,
            r#"
schema_version = 1
policy = "non-rust-allowlist"

[[allow]]
id = "non-rust-changes-tree"
glob = ".changes/**"
kind = "release_metadata"
language = "yaml"
surface = "release"
classification = "documentation"
owner = "release/ci"
reason = "Everything under the changes directory."
covered_by = []
created = "2026-05-13"
review_after = "2026-11-13"
"#,
        )?;
        fs::write(&debt, "debt = []\n")?;

        let validation = validate_non_rust_policy_files(&allowlist, &debt);

        assert!(
            validation.errors.iter().any(|error| error.contains("broad_glob_reason")),
            "a `.changes/**` tree grab must be refused, not silently accepted: {:?}",
            validation.errors
        );
        Ok(())
    }

    #[test]
    fn fragment_path_coverage_is_not_a_claim_about_fragment_content() -> Result<()> {
        // The path rule says only "this path class is accounted for". It must
        // not become an implicit content exemption: a hand-written file that
        // never went through `cargo change` still matches the path class, and
        // is still the changelog gate's to reject. `validate_fragment` owns
        // that verdict and has its own negative suite in `changelog.rs`.
        use crate::tasks::changelog::{ChangieConfig, Fragment, validate_fragment};

        let entry = changie_fragment_entry()?;
        let entries = vec![entry.clone()];
        let hand_written = ".changes/unreleased/not-a-real-fragment.yaml";

        assert!(
            classify_file(hand_written, &entries).allowlisted,
            "the path class must stay checkable rather than routing around the content gate"
        );
        assert!(
            entry.covered_by.iter().any(|check| check == "cargo xtask changelog check"),
            "the entry must name the gate that actually validates fragment content"
        );

        // Drive the production validator with the production `.changie.yaml`,
        // not a fixture copy of either: the claim is that the path rule leaves
        // the real gate reachable, so a copy would prove nothing about it.
        let config: ChangieConfig =
            serde_yaml_ng::from_str(&fs::read_to_string(repo_root()?.join(".changie.yaml"))?)?;

        let malformed: Fragment = serde_yaml_ng::from_str(concat!(
            "project: not-a-project\n",
            "kind: NotAKind\n",
            "body: tiny\n",
            "custom:\n",
            "  PR: \"nonsense\"\n",
        ))?;
        let findings = validate_fragment(&malformed, &config);
        assert!(
            findings.iter().any(|f| f.contains("unknown project")),
            "the real gate must still reject an unknown project: {findings:?}"
        );
        assert!(
            findings.iter().any(|f| f.contains("unknown kind")),
            "the real gate must still reject an unknown kind: {findings:?}"
        );
        assert!(
            findings.iter().any(|f| f.contains("body")),
            "the real gate must still reject an under-length body: {findings:?}"
        );
        assert!(
            findings.iter().any(|f| f.contains("PR")),
            "the real gate must still reject a non-numeric PR reference: {findings:?}"
        );

        // Positive control: without it, a validator that rejected everything
        // would satisfy the assertions above.
        let well_formed: Fragment = serde_yaml_ng::from_str(concat!(
            "project: product\n",
            "component: Developer experience\n",
            "kind: Changed\n",
            "body: A sufficiently long changelog body line for the gate.\n",
            // An RFC 3339 `time:` is part of well-formedness, not decoration:
            // the renderer unmarshals it as a timestamp and crashes repo-wide
            // without one (#13484).
            "time: 2026-08-30T12:34:56Z\n",
            "custom:\n",
            "  PR: \"14588\"\n",
            "  Breaking: \"no\"\n",
        ))?;
        assert!(
            validate_fragment(&well_formed, &config).is_empty(),
            "a well-formed fragment must pass the same production validator"
        );
        Ok(())
    }

    #[test]
    fn no_per_fragment_allowlist_rows_remain() -> Result<()> {
        let allowlist = load_allowlist(&repo_root()?)?;

        let per_fragment: Vec<&str> = allowlist
            .allow
            .iter()
            .filter(|entry| entry.id != CHANGIE_FRAGMENT_ENTRY_ID)
            .filter(|entry| {
                let matcher = entry.glob.as_deref().or(entry.path.as_deref()).unwrap_or_default();
                matcher.starts_with(".changes/unreleased/")
            })
            .map(|entry| entry.id.as_str())
            .collect();

        assert!(
            per_fragment.is_empty(),
            "individual fragments must not earn allowlist rows; found {per_fragment:?}"
        );
        Ok(())
    }

    #[test]
    fn every_tracked_fragment_resolves_to_the_single_typed_rule() -> Result<()> {
        let root = repo_root()?;
        let allowlist = load_allowlist(&root)?;
        let prepared = prepare_allow_entries(&allowlist.allow);

        let mut fragments: Vec<String> = list_tracked_files(&root)?
            .into_iter()
            .filter(|path| path.starts_with(".changes/unreleased/") && path.ends_with(".yaml"))
            .collect();

        // A release drains the directory, so the tracked population is
        // legitimately empty some of the time. Always exercise a synthetic
        // fragment as well, so this proof can never pass vacuously on the
        // release commit.
        fragments.push(".changes/unreleased/product-1-Added-101010.yaml".to_string());

        for fragment in &fragments {
            let owner = find_matching_prepared_entry(fragment, &prepared)
                .ok_or_else(|| eyre!("{fragment} is not covered by any allowlist entry"))?;
            assert_eq!(
                owner.id, CHANGIE_FRAGMENT_ENTRY_ID,
                "{fragment} must be governed by the typed rule, not by {}",
                owner.id
            );
        }
        Ok(())
    }

    #[test]
    fn draining_the_fragment_directory_leaves_no_stale_policy_row() -> Result<()> {
        // A release renders and deletes every unreleased fragment. Because the
        // rule names a path class rather than instances, the ledger needs no
        // edit; the entry simply stops matching. This is the property that a
        // per-fragment row could not have.
        let entry = changie_fragment_entry()?;
        let entries = vec![entry.clone()];

        assert!(
            classify_file(".changes/unreleased/product-1-Added-101010.yaml", &entries).allowlisted,
            "covered before the release drains the directory"
        );
        assert!(
            !entry_matches_any_tracked_file(&entry, &[".changes/unreleased/.gitkeep".to_string()]),
            "after the drain the rule matches nothing and leaves no per-file receipt behind"
        );
        Ok(())
    }

    #[test]
    fn two_independent_fragment_additions_do_not_touch_the_allowlist() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let entry = changie_fragment_entry()?;
        let allowlist_toml =
            format!("[[allow]]\n{}", toml::to_string(&entry).context("serializing rule")?);

        run_git(root, &["init", "-q"])?;
        run_git(root, &["config", "user.email", "fixture@example.invalid"])?;
        run_git(root, &["config", "user.name", "fixture"])?;
        // A global `commit.gpgSign=true` with no usable key is inherited here
        // and would abort the fixture commits before any matcher or merge
        // behaviour is exercised, turning a host setting into a test failure.
        run_git(root, &["config", "commit.gpgsign", "false"])?;
        // Name the base branch without depending on `git init -b` or on the
        // host's `init.defaultBranch`.
        run_git(root, &["checkout", "-q", "-b", "base"])?;
        write_fixture(root, "policy/non-rust-allowlist.toml", &allowlist_toml)?;
        write_fixture(root, ".changes/unreleased/.gitkeep", "")?;
        run_git(root, &["add", "."])?;
        run_git(root, &["commit", "-qm", "base"])?;

        let first = ".changes/unreleased/product-101-Added-101010.yaml";
        let second = ".changes/unreleased/product-202-Fixed-202020.yaml";

        for (branch, fragment) in [("candidate-a", first), ("candidate-b", second)] {
            run_git(root, &["checkout", "-q", "-b", branch, "base"])?;
            write_fixture(root, fragment, "project: product\nkind: Added\nbody: fixture\n")?;
            run_git(root, &["add", fragment])?;
            run_git(root, &["commit", "-qm", branch])?;
        }

        // Neither candidate edited the shared policy authority: that is the
        // whole point of the typed rule. #9680's conflicts came from both
        // candidates appending a row to this one file.
        for branch in ["candidate-a", "candidate-b"] {
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(["diff", "--name-only", "base", branch])
                .output()
                .context("diffing candidate against base")?;
            let changed = String::from_utf8_lossy(&output.stdout);
            assert!(
                !changed.contains("policy/non-rust-allowlist.toml"),
                "adding a fragment must not mutate the allowlist; {branch} changed: {changed}"
            );
        }

        run_git(root, &["checkout", "-q", "candidate-a"])?;
        run_git(root, &["merge", "-q", "--no-edit", "candidate-b"])?;

        let entries = vec![entry];
        for fragment in [first, second] {
            assert!(root.join(fragment).exists(), "{fragment} must survive the merge");
            assert!(
                classify_file(fragment, &entries).allowlisted,
                "{fragment} must be governed after the merge with no allowlist edit"
            );
        }
        Ok(())
    }

    // --- extension extraction ---

    #[test]
    fn extension_extracted_correctly() {
        let rec = classify_file("foo/bar.md", &[]);
        assert_eq!(rec.extension, "md");
    }

    #[test]
    fn file_without_extension() {
        let rec = classify_file("justfile", &[]);
        assert_eq!(rec.extension, "");
        assert!(!rec.allowlisted);
    }

    // --- JSON round-trip ---

    #[test]
    fn file_record_serde_round_trip() -> Result<()> {
        let record = FileRecord {
            path: "justfile".to_string(),
            extension: String::new(),
            category: "tooling".to_string(),
            allowlisted: true,
            entry: Some(make_entry("e1", None, Some("justfile"), "tooling")),
        };
        let json = serde_json::to_string(&record)?;
        let back: FileRecord = serde_json::from_str(&json)?;
        assert_eq!(record, back);
        Ok(())
    }

    #[test]
    fn build_inventory_reads_git_tracked_files_and_workspace_allowlist() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let tracked = init_tracked_fixture(
            temp.path(),
            &[
                ("README.md", "# Fixture\n"),
                ("src/lib.rs", "pub fn marker() {}\n"),
                ("scripts/tool.py", "print('fixture')\n"),
            ],
        )?;
        assert!(tracked.iter().any(|path| path == "README.md"));
        assert!(tracked.iter().any(|path| path == "scripts/tool.py"));
        assert!(tracked.iter().any(|path| path == "src/lib.rs"));
        write_readme_allowlist(temp.path(), "policy/non-rust-allowlist.toml")?;

        let records = build_inventory(temp.path())?;
        let readme = records
            .iter()
            .find(|record| record.path == "README.md")
            .ok_or_else(|| eyre!("missing README.md record"))?;
        let rust = records
            .iter()
            .find(|record| record.path == "src/lib.rs")
            .ok_or_else(|| eyre!("missing src/lib.rs record"))?;
        let script = records
            .iter()
            .find(|record| record.path == "scripts/tool.py")
            .ok_or_else(|| eyre!("missing scripts/tool.py record"))?;

        assert_eq!(readme.category, "documentation");
        assert!(readme.allowlisted);
        assert_eq!(rust.category, "rust");
        assert_eq!(script.category, "unclassified");
        assert!(!script.allowlisted);
        Ok(())
    }

    #[test]
    fn non_rust_inventory_writes_target_outputs_and_write_docs_publishes_reference() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let tracked = init_tracked_fixture(temp.path(), &[("README.md", "# Fixture\n")])?;
        assert_eq!(tracked, vec!["README.md".to_string()]);
        let allowlist_path = write_readme_allowlist(temp.path(), "policy/non-rust-allowlist.toml")?;
        assert!(allowlist_path.exists());

        non_rust_inventory(temp.path())?;

        let target_markdown = temp.path().join("target/policy/non-rust-inventory.md");
        let target_json = temp.path().join("target/policy/non-rust-inventory.json");
        let docs_markdown = temp.path().join("docs/policy/NON_RUST_INVENTORY.md");
        let markdown = fs::read_to_string(&target_markdown)
            .with_context(|| format!("reading {}", target_markdown.display()))?;
        let json = fs::read_to_string(&target_json)
            .with_context(|| format!("reading {}", target_json.display()))?;

        assert!(markdown.contains("# Non-Rust File Inventory"));
        assert!(json.contains("\"path\": \"README.md\""));
        // The plain scan is read-only w.r.t. tracked files: the committed
        // reference is published only by the explicit write-docs path.
        assert!(
            !docs_markdown.exists(),
            "default inventory must not create {}",
            docs_markdown.display()
        );

        non_rust_inventory_write_docs(temp.path())?;
        let docs = fs::read_to_string(&docs_markdown)
            .with_context(|| format!("reading {}", docs_markdown.display()))?;
        assert_eq!(markdown, docs);
        Ok(())
    }

    #[test]
    fn non_rust_inventory_check_ignores_stale_publication_and_emits_evidence() -> Result<()> {
        let temp = tempfile::tempdir()?;
        init_tracked_fixture(temp.path(), &[("README.md", "# Fixture\n")])?;
        write_readme_allowlist(temp.path(), "policy/non-rust-allowlist.toml")?;
        write_fixture(
            temp.path(),
            "docs/policy/NON_RUST_INVENTORY.md",
            "deliberately stale and structurally invalid\n",
        )?;
        run_git(temp.path(), &["add", "."])?;
        run_git(
            temp.path(),
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "commit",
                "-qm",
                "published reference baseline",
            ],
        )?;

        let docs_path = temp.path().join("docs/policy/NON_RUST_INVENTORY.md");
        let before = fs::read(&docs_path)?;
        non_rust_inventory_check_with_baseline(temp.path(), Some("HEAD"))?;

        assert_eq!(
            fs::read(&docs_path)?,
            before,
            "the branch check must not rewrite the published reference"
        );
        let markdown = fs::read_to_string(temp.path().join("target/policy/non-rust-inventory.md"))?;
        let json = fs::read_to_string(temp.path().join("target/policy/non-rust-inventory.json"))?;
        assert!(markdown.contains("docs/policy/NON_RUST_INVENTORY.md"));
        assert!(json.contains("docs/policy/NON_RUST_INVENTORY.md"));
        Ok(())
    }

    /// A tracked path containing `|` is rendered escaped in table cells; the
    /// cell parser must reverse the escape so evidence rows name the raw path.
    #[test]
    fn markdown_path_cells_round_trip_pipe_escapes() -> Result<()> {
        for raw in ["docs/a|b.md", "docs\\a|b.md"] {
            let escaped = escape_markdown_cell(raw);
            let row = format!("| `{escaped}` | documentation | `ab` | owner |\n");
            let cells = parse_markdown_cells(&row)
                .ok_or_else(|| eyre!("rendered row must parse: {row}"))?;
            assert_eq!(cells[0], format!("`{raw}`"));
        }
        Ok(())
    }

    #[test]
    fn markdown_metadata_cells_round_trip_odd_backslashes_before_pipe() {
        for count in 1..=3 {
            let prefix = "\\".repeat(count);
            let id = format!("id-{prefix}|suffix");
            let owner = format!("owner-{prefix}|suffix");
            let row = format!(
                "| `path.md` | documentation | `{}` | {} |\n",
                escape_markdown_cell(&id),
                escape_markdown_cell(&owner)
            );
            let cells = parse_markdown_cells(&row).expect("rendered row must parse");
            assert_eq!(cells.len(), 4, "metadata backslashes must not add cells");
            assert_eq!(cells[2], format!("`{id}`"));
            assert_eq!(cells[3], owner);
        }
    }

    #[test]
    fn markdown_metadata_cells_round_trip_trailing_literal_pipe() {
        let id = "id|";
        let owner = "owner|";
        let row = format!(
            "| `path.md` | documentation | `{}` | {} |\n",
            escape_markdown_cell(id),
            escape_markdown_cell(owner)
        );
        let cells = parse_markdown_cells(&row).expect("rendered row must parse");
        assert_eq!(cells, vec!["`path.md`", "documentation", "`id|`", "owner|"]);

        // The codec must also preserve a final literal pipe when there is no
        // whitespace before the table delimiter.
        let compact = "| `path.md`|documentation|`id\\|`|owner\\||";
        let cells = parse_markdown_cells(compact).expect("compact row must parse");
        assert_eq!(cells, vec!["`path.md`", "documentation", "`id|`", "owner|"]);
    }

    #[test]
    fn non_rust_inventory_check_accepts_unclassified_files() -> Result<()> {
        let temp = tempfile::tempdir()?;
        init_tracked_fixture(
            temp.path(),
            &[("README.md", "# Fixture\n"), ("scripts/tool.py", "print('fixture')\n")],
        )?;
        write_readme_allowlist(temp.path(), "policy/non-rust-allowlist.toml")?;
        non_rust_inventory_check(temp.path())?;
        Ok(())
    }

    #[test]
    fn non_rust_inventory_check_rejects_new_unclassified_files_and_retains_evidence() -> Result<()>
    {
        let temp = tempfile::tempdir()?;
        init_tracked_fixture(
            temp.path(),
            &[("README.md", "# Fixture\n"), ("scripts/existing.py", "print('fixture')\n")],
        )?;
        write_readme_allowlist(temp.path(), "policy/non-rust-allowlist.toml")?;
        run_git(temp.path(), &["add", "."])?;
        run_git(
            temp.path(),
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "commit",
                "-qm",
                "baseline",
            ],
        )?;

        write_fixture(temp.path(), "scripts/new.py", "print('new')\n")?;
        run_git(temp.path(), &["add", "scripts/new.py"])?;
        run_git(
            temp.path(),
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "commit",
                "-qm",
                "candidate",
            ],
        )?;

        let error = non_rust_inventory_check_with_baseline(temp.path(), Some("HEAD^"))
            .err()
            .ok_or_else(|| eyre!("new unclassified file must fail"))?;
        ensure!(
            error.to_string().contains("scripts/new.py"),
            "failure must name the newly unclassified path: {error}"
        );

        let markdown = fs::read_to_string(temp.path().join("target/policy/non-rust-inventory.md"))?;
        let json = fs::read_to_string(temp.path().join("target/policy/non-rust-inventory.json"))?;
        assert!(markdown.contains("scripts/new.py"));
        assert!(json.contains("scripts/new.py"));
        Ok(())
    }

    /// A classified file renamed to an unclassified path is a newly added
    /// unclassified path: rename detection must not hide it from the ratchet.
    #[test]
    fn non_rust_inventory_check_rejects_rename_into_unclassified_path() -> Result<()> {
        let temp = tempfile::tempdir()?;
        init_tracked_fixture(
            temp.path(),
            &[("README.md", "# Fixture\n"), ("scripts/existing.py", "print('fixture')\n")],
        )?;
        write_readme_allowlist(temp.path(), "policy/non-rust-allowlist.toml")?;
        run_git(temp.path(), &["add", "."])?;
        run_git(
            temp.path(),
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "commit",
                "-qm",
                "baseline",
            ],
        )?;

        run_git(temp.path(), &["mv", "README.md", "scripts/README.md"])?;
        run_git(
            temp.path(),
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "commit",
                "-qm",
                "rename",
            ],
        )?;

        let error = non_rust_inventory_check_with_baseline(temp.path(), Some("HEAD^"))
            .err()
            .ok_or_else(|| eyre!("rename into an unclassified path must fail"))?;
        ensure!(
            error.to_string().contains("scripts/README.md"),
            "failure must name the renamed destination path: {error}"
        );
        Ok(())
    }

    #[test]
    fn non_rust_inventory_check_rejects_invalid_allowlist_classification() -> Result<()> {
        let temp = tempfile::tempdir()?;
        init_tracked_fixture(temp.path(), &[("README.md", "# Fixture\n")])?;
        write_fixture(
            temp.path(),
            "policy/non-rust-allowlist.toml",
            &readme_allowlist_toml()?
                .replace("classification = \"documentation\"", "classification = \"toolng\""),
        )?;

        assert!(non_rust_inventory_check(temp.path()).is_err());
        Ok(())
    }

    #[test]
    fn blocking_check_file_policy_rejects_conflict_markers_in_raw_allowlist() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let _tracked = init_tracked_fixture(temp.path(), &[("README.md", "# Fixture\n")])?;
        let allowlist = temp.path().join("allow.toml");
        std::fs::write(
            &allowlist,
            "[[allow]]\nid = \"readme\"\npath = \"README.md\"\nkind = \"documentation\"\nlanguage = \"markdown\"\nowner = \"team\"\nreason = \"\"\"\n<<<<<<< HEAD\n\"\"\"\nsurface = \"docs\"\nclassification = \"documentation\"\ncovered_by = [\"fixture\"]\ncreated = \"2026-01-01\"\nreview_after = \"2099-01-01\"\n",
        )?;

        let result = check_file_policy(
            temp.path(),
            CheckFilePolicyConfig {
                mode: CheckFilePolicyMode::BlockingAllowlist,
                json_output: None,
                allowlist_path: Some(allowlist),
                root_override: Some(temp.path().to_path_buf()),
            },
        );
        let error = result.expect_err("blocking policy accepted raw conflict marker");
        assert!(
            error.to_string().contains("violation"),
            "blocking policy did not report a violation: {error:?}"
        );
        Ok(())
    }

    #[test]
    fn check_file_policy_advisory_writes_receipt_and_markdown_report() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let tracked = init_tracked_fixture(
            temp.path(),
            &[("README.md", "# Fixture\n"), ("scripts/tool.py", "print('fixture')\n")],
        )?;
        assert!(tracked.iter().any(|path| path == "scripts/tool.py"));
        let allowlist = write_readme_allowlist(temp.path(), "allow.toml")?;

        check_file_policy(
            temp.path(),
            CheckFilePolicyConfig {
                mode: CheckFilePolicyMode::Advisory,
                json_output: None,
                allowlist_path: Some(allowlist),
                root_override: Some(temp.path().to_path_buf()),
            },
        )?;

        let receipt_path = temp.path().join("target/policy/file-policy-report.json");
        let report_path = temp.path().join("target/policy/file-policy-report.md");
        let receipt_text = fs::read_to_string(&receipt_path)
            .with_context(|| format!("reading {}", receipt_path.display()))?;
        let report = fs::read_to_string(&report_path)
            .with_context(|| format!("reading {}", report_path.display()))?;
        let receipt: FilePolicyReceipt = serde_json::from_str(&receipt_text)?;

        assert_eq!(receipt.mode, "advisory");
        assert_eq!(receipt.total_tracked, 2);
        assert_eq!(receipt.non_rust, 2);
        assert_eq!(receipt.unclassified, 1);
        assert!(receipt.violations.is_empty());
        assert!(report.contains("| Unclassified | 1 |"));
        Ok(())
    }

    #[test]
    fn validate_non_rust_policy_wrapper_reports_success_and_failure() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let allowlist = temp.path().join("allow.toml");
        let debt = temp.path().join("debt.toml");
        let allowlist_text = readme_allowlist_toml()?;
        fs::write(&allowlist, &allowlist_text)
            .with_context(|| format!("writing {}", allowlist.display()))?;
        fs::write(&debt, "debt = []\n").with_context(|| format!("writing {}", debt.display()))?;

        validate_non_rust_policy(ValidateNonRustPolicyConfig {
            allowlist_path: allowlist.clone(),
            debt_path: debt.clone(),
        })?;

        fs::write(&allowlist, "[[allow]]\nid = \"broken\"\n")
            .with_context(|| format!("rewriting {}", allowlist.display()))?;
        let result = validate_non_rust_policy(ValidateNonRustPolicyConfig {
            allowlist_path: allowlist,
            debt_path: debt,
        });

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn validate_non_rust_policy_accepts_current_schema_extensions() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let allowlist = temp.path().join("allow.toml");
        let debt = temp.path().join("debt.toml");
        fs::write(
            &allowlist,
            r#"
[[allow]]
id = "generated-badge"
glob = "badges/*.json"
kind = "generated_badge_endpoint"
language = "json"
surface = "docs"
classification = "generated"
owner = "release/ci"
reason = "Generated badge data."
generated_by = "python3 scripts/generate-badges.py"
covered_by = ["python3 scripts/generate-badges.py --check"]
created = "2026-05-13"
review_after = "2026-08-13"
"#,
        )?;
        fs::write(&debt, "# empty debt ledger\n")?;

        let validation = validate_non_rust_policy_files(&allowlist, &debt);

        assert_eq!(validation.allow_entries, 1);
        assert_eq!(validation.debt_entries, 0);
        assert!(validation.errors.is_empty(), "unexpected errors: {:?}", validation.errors);
        Ok(())
    }

    #[test]
    fn debt_entries_may_reuse_reason_across_matchers() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let debt = temp.path().join("debt.toml");
        fs::write(
            &debt,
            r#"
[[debt]]
id = "debt-a"
path = "legacy/a.py"
reason = "classification pending"

[[debt]]
id = "debt-b"
path = "legacy/b.py"
reason = "classification pending"
"#,
        )?;
        let mut errors = Vec::new();
        let count = validate_policy_table(&debt, "debt", false, &mut errors);
        assert_eq!(count, 2);
        assert!(!errors.iter().any(|error| error.contains("mispaired provenance")), "{errors:?}");
        Ok(())
    }
    #[test]
    fn validate_non_rust_policy_reports_schema_errors() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let allowlist = temp.path().join("allow.toml");
        let debt = temp.path().join("debt.toml");
        fs::write(
            &allowlist,
            r#"
[[allow]]
id = "bad"
glob = "docs/**"
path = "docs/README.md"
kind = "documentation"
language = "markdown"
surface = "docs"
classification = "production"
owner = "docs"
reason = "Broken fixture."
covered_by = []
created = "2026-05-13"
review_after = "2026-05-13"
unknown = "field"
"#,
        )?;
        fs::write(&debt, "debt = []\n")?;

        let validation = validate_non_rust_policy_files(&allowlist, &debt);

        assert!(
            validation.errors.iter().any(|error| error.contains("cannot set both")),
            "missing matcher conflict error: {:?}",
            validation.errors
        );
        assert!(
            validation.errors.iter().any(|error| error.contains("requires at least one")),
            "missing coverage requirement error: {:?}",
            validation.errors
        );
        assert!(
            validation.errors.iter().any(|error| error.contains("unknown field")),
            "missing unknown field error: {:?}",
            validation.errors
        );
        assert!(
            validation.errors.iter().any(|error| error.contains("review_after")),
            "missing date ordering error: {:?}",
            validation.errors
        );
        Ok(())
    }

    #[test]
    fn validate_non_rust_policy_reports_matcher_and_field_shape_errors() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let allowlist = temp.path().join("allow.toml");
        let debt = temp.path().join("debt.toml");
        fs::write(
            &allowlist,
            r#"
[[allow]]
id = "missing-matcher"
kind = "documentation"
language = "markdown"
surface = "docs"
classification = "documentation"
owner = "docs"
reason = "No matcher."
covered_by = []
created = "2026-05-13"
review_after = "2026-08-13"

[[allow]]
id = "bad-shapes"
glob = './docs\**'
kind = "documentation"
language = "markdown"
surface = "docs"
classification = "surprise"
owner = "docs"
reason = "Bad schema shapes."
covered_by = "not-a-list"
created = "not-a-date"
review_after = 20260813
retired = "no"

[[allow]]
id = "first-readme"
path = "README.md"
kind = "documentation"
language = "markdown"
surface = "docs"
classification = "documentation"
owner = "docs"
reason = "First readme entry."
covered_by = []
created = "2026-05-13"
review_after = "2026-08-13"

[[allow]]
id = "second-readme"
path = "README.md"
kind = "documentation"
language = "markdown"
surface = "docs"
classification = "documentation"
owner = "docs"
reason = "Duplicate matcher."
covered_by = []
created = "2026-05-13"
review_after = "2026-08-13"
"#,
        )?;
        fs::write(&debt, "debt = []\n")?;

        let validation = validate_non_rust_policy_files(&allowlist, &debt);

        for expected in [
            "must set either `glob` or `path`",
            "without leading `./`",
            "contains Windows backslashes",
            "classification `surprise`",
            "`covered_by` must be a list of strings",
            "`created` is not a real date",
            "`review_after` must be a YYYY-MM-DD string",
            "`retired` must be a boolean",
            "duplicate matcher `README.md`",
        ] {
            assert!(
                validation.errors.iter().any(|error| error.contains(expected)),
                "missing {expected:?} in {:?}",
                validation.errors
            );
        }
        Ok(())
    }

    fn canonical_allow_document(entries: &str) -> String {
        format!("schema_version = 1\npolicy = \"non-rust-allowlist\"\n{entries}")
    }

    fn valid_allow_entry(id: &str, path: &str, reason: &str) -> String {
        format!(
            r#"
[[allow]]
id = "{id}"
path = "{path}"
kind = "doc"
language = "markdown"
surface = "docs"
classification = "documentation"
owner = "docs"
reason = "{reason}"
covered_by = ["manual review"]
created = "2026-01-01"
review_after = "2026-06-01"
"#
        )
    }

    fn validate_shared_allow_schema_surfaces(document: &str) -> Result<(String, Vec<String>)> {
        let exact = validate_exact_policy_bytes(document.as_bytes())
            .err()
            .map(|err| err.to_string())
            .unwrap_or_default();
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("allow.toml");
        fs::write(&path, document)?;
        let mut ordinary = Vec::new();
        let _ = validate_policy_table(&path, "allow", true, &mut ordinary);
        Ok((exact, ordinary))
    }

    fn assert_shared_allow_schema_rejects(document: &str, needles: &[&str]) -> Result<()> {
        let (exact, ordinary) = validate_shared_allow_schema_surfaces(document)?;
        ensure!(!exact.is_empty(), "exact-tree accepted malformed allowlist: {document}");
        ensure!(!ordinary.is_empty(), "ordinary policy accepted malformed allowlist: {document}");
        for needle in needles {
            ensure!(exact.contains(needle), "exact-tree missed {needle:?} in {exact:?}");
            ensure!(
                ordinary.iter().any(|error| error.contains(*needle)),
                "ordinary policy missed {needle:?} in {ordinary:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn valid_allowlist_passes_exact_tree_and_ordinary_schema_surfaces() -> Result<()> {
        let document = canonical_allow_document(&format!(
            "{}{}",
            valid_allow_entry("entry-a", "docs/a.md", "Documents alpha."),
            valid_allow_entry("entry-b", "docs/b.md", "Documents beta.")
        ));
        validate_exact_policy_bytes(document.as_bytes())?;
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("allow.toml");
        fs::write(&path, &document)?;
        let mut errors = Vec::new();
        assert_eq!(validate_policy_table(&path, "allow", true, &mut errors), 2);
        ensure!(errors.is_empty(), "unexpected ordinary errors: {errors:?}");
        Ok(())
    }

    #[test]
    fn malformed_allow_fields_fail_exact_tree_and_ordinary_schema_surfaces() -> Result<()> {
        let valid =
            canonical_allow_document(&valid_allow_entry("ok", "docs/ok.md", "Valid control row."));

        assert_shared_allow_schema_rejects(
            &valid.replace(
                "review_after = \"2026-06-01\"\n",
                "review_after = \"2026-06-01\"\nunknown = \"field\"\n",
            ),
            &["unknown field"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("path = \"docs/ok.md\"", "glob = \"docs/**\"\npath = \"docs/ok.md\""),
            &["cannot set both"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("path = \"docs/ok.md\"\n", ""),
            &["must set either `glob` or `path`"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("path = \"docs/ok.md\"", "path = \"./docs/ok.md\""),
            &["without leading `./`"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("path = \"docs/ok.md\"", "path = \"docs\\\\ok.md\""),
            &["Windows backslashes"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("path = \"docs/ok.md\"", "path = \" docs/ok.md \""),
            &["surrounding whitespace"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("classification = \"documentation\"", "classification = \"surprise\""),
            &["classification `surprise`"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("covered_by = [\"manual review\"]", "covered_by = \"manual review\""),
            &["`covered_by` must be a list of strings"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("covered_by = [\"manual review\"]\n", ""),
            &["missing required field `covered_by`"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid
                .replace("classification = \"documentation\"", "classification = \"production\"")
                .replace("covered_by = [\"manual review\"]", "covered_by = []"),
            &["requires at least one `covered_by` entry"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("created = \"2026-01-01\"", "created = \"not-a-date\""),
            &["`created` is not a real date"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("review_after = \"2026-06-01\"", "review_after = \"2026-01-01\""),
            &["`review_after` must be after `created`"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace(
                "review_after = \"2026-06-01\"\n",
                "review_after = \"2026-06-01\"\nexpires = \"2026-01-01\"\n",
            ),
            &["`expires` must be after `created`"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("owner = \"docs\"\n", ""),
            &["missing required field `owner`"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace(
                "review_after = \"2026-06-01\"\n",
                "review_after = \"2026-06-01\"\nretired = \"no\"\n",
            ),
            &["`retired` must be a boolean"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("path = \"docs/ok.md\"", "glob = \"[\""),
            &["invalid glob"],
        )?;
        assert_shared_allow_schema_rejects(
            &valid.replace("path = \"docs/ok.md\"", "glob = \"docs/**\""),
            &["broad_glob_reason"],
        )?;

        let duplicate_matcher = canonical_allow_document(&format!(
            "{}{}",
            valid_allow_entry("first", "README.md", "First readme."),
            valid_allow_entry("second", "README.md", "Second readme.")
        ));
        assert_shared_allow_schema_rejects(&duplicate_matcher, &["duplicate matcher `README.md`"])?;

        let duplicate_id = canonical_allow_document(&format!(
            "{}{}",
            valid_allow_entry("same-id", "docs/a.md", "First id."),
            valid_allow_entry("same-id", "docs/b.md", "Second id.")
        ));
        assert_shared_allow_schema_rejects(&duplicate_id, &["duplicate id"])?;
        Ok(())
    }

    #[test]
    fn documentation_empty_covered_by_is_not_a_schema_error_on_either_surface() -> Result<()> {
        let document = canonical_allow_document(
            &valid_allow_entry("docs-empty", "docs/ok.md", "Docs may omit coverage items.")
                .replace("covered_by = [\"manual review\"]", "covered_by = []"),
        );
        validate_exact_policy_bytes(document.as_bytes())?;
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("allow.toml");
        fs::write(&path, &document)?;
        let mut errors = Vec::new();
        assert_eq!(validate_policy_table(&path, "allow", true, &mut errors), 1);
        ensure!(errors.is_empty(), "unexpected ordinary errors: {errors:?}");
        Ok(())
    }

    #[test]
    fn check_allowlist_entries_reports_blocking_and_strict_violations() {
        let mut expired = make_entry("expired", None, Some("README.md"), "documentation");
        expired.expires = Some("2026-01-01".to_string());

        let mut empty_fields = make_entry("empty-fields", None, None, "");
        empty_fields.kind.clear();
        empty_fields.language.clear();
        empty_fields.owner.clear();
        empty_fields.reason.clear();
        empty_fields.surface.clear();
        empty_fields.classification.clear();

        let invalid_glob = make_entry("invalid-glob", Some("["), None, "documentation");

        let blocking = check_allowlist_entries(
            &[expired, empty_fields, invalid_glob],
            CheckFilePolicyMode::BlockingAllowlist,
            &[],
        );
        let blocking_kinds = violation_kinds(&blocking);
        for expected in [
            "expired-entry",
            "missing-matcher",
            "missing-kind",
            "missing-language",
            "missing-owner",
            "missing-reason",
            "missing-surface",
            "missing-classification",
            "missing-covered-by",
            "invalid-glob",
        ] {
            assert!(
                blocking_kinds.contains(&expected),
                "missing {expected:?} in {:?}",
                blocking_kinds
            );
        }

        let mut stale = make_entry("dup", None, Some("/absolute/path.md"), "documentation");
        stale.review_after = "2026-01-01".to_string();
        stale.covered_by = vec!["fixture".to_string()];
        let mut duplicate = make_entry("dup", None, Some("docs\\guide.md"), "documentation");
        duplicate.covered_by = vec!["fixture".to_string()];
        let mut broad = make_entry("broad", Some("**/*"), None, "documentation");
        broad.covered_by = vec!["fixture".to_string()];
        let mut unused = make_entry("unused", None, Some("missing.md"), "documentation");
        unused.covered_by = vec!["fixture".to_string()];

        let strict = check_allowlist_entries(
            &[stale, duplicate, broad, unused],
            CheckFilePolicyMode::BlockingStrict,
            &["README.md".to_string()],
        );
        let strict_kinds = violation_kinds(&strict);
        for expected in [
            "duplicate-id",
            "stale-review-after",
            "unused-entry",
            "absolute-path",
            "backslash-path",
            "broad-glob-no-reason",
        ] {
            assert!(strict_kinds.contains(&expected), "missing {expected:?} in {:?}", strict_kinds);
        }
    }

    #[test]
    fn render_policy_report_markdown_lists_violation_details() {
        let receipt = FilePolicyReceipt {
            schema_version: 1,
            mode: "blocking-strict".to_string(),
            total_tracked: 3,
            non_rust: 2,
            unclassified: 1,
            expired: 1,
            stale_review_after: 1,
            duplicate_ids: 0,
            unused_entries: 1,
            violations: vec![PolicyViolation {
                kind: "unused-entry".to_string(),
                message: "Entry matches no tracked file".to_string(),
                path: Some("missing.md".to_string()),
                entry_id: Some("unused".to_string()),
            }],
        };

        let report = render_policy_report_markdown(&receipt);

        assert!(report.contains("# Non-Rust File Policy Report"));
        assert!(report.contains("| Mode | `blocking-strict` |"));
        assert!(report.contains("| Violations | 1 |"));
        assert!(report.contains("| `unused-entry` | `missing.md` | `unused` |"));
    }

    #[test]
    fn render_policy_report_markdown_reports_empty_violation_set() {
        let receipt = FilePolicyReceipt {
            schema_version: 1,
            mode: "advisory".to_string(),
            total_tracked: 1,
            non_rust: 0,
            unclassified: 0,
            expired: 0,
            stale_review_after: 0,
            duplicate_ids: 0,
            unused_entries: 0,
            violations: Vec::new(),
        };

        let report = render_policy_report_markdown(&receipt);

        assert!(report.contains("No violations for the selected mode."));
    }

    #[test]
    fn proposal_renderers_show_grouping_and_migration_guidance() -> Result<()> {
        let unclassified = vec![
            "scripts/ci/check-status.py".to_string(),
            "README.md".to_string(),
            "docs/guide.md".to_string(),
        ];
        let directory_groups = group_by_directory(&unclassified);
        let extension_groups = group_by_extension(&unclassified);

        assert_eq!(
            directory_groups.get("(root)").ok_or_else(|| eyre!("missing root directory group"))?,
            &vec!["README.md".to_string()]
        );
        assert_eq!(
            extension_groups.get("md").ok_or_else(|| eyre!("missing markdown group"))?.len(),
            2
        );

        let mut entry = make_entry("proposed-dir-scripts", Some("scripts/**/*"), None, "build");
        entry.owner = "TBD".to_string();
        entry.surface = "unclassified".to_string();
        entry.broad_glob_reason = Some("bulk proposal".to_string());
        entry.covered_by = vec!["scripts/**/*".to_string()];

        let proposed_toml = render_proposed_toml(
            std::slice::from_ref(&entry),
            ProposeGroupBy::Directory,
            "2026-06-19",
        )?;
        assert!(proposed_toml.contains("status = \"proposed\""));
        assert!(proposed_toml.contains("id = \"proposed-dir-scripts\""));
        assert!(proposed_toml.contains("broad_glob_reason = \"bulk proposal\""));

        let proposal = render_proposal_markdown(
            &directory_groups,
            &[entry],
            ProposeGroupBy::Directory,
            &unclassified,
        );
        assert!(proposal.contains("# Non-Rust Allowlist Proposal"));
        assert!(proposal.contains("Rust migration candidates"));
        assert!(proposal.contains("xtask policy/check tasks"));
        assert!(proposal.contains("`scripts/ci/check-status.py`"));
        Ok(())
    }

    #[test]
    fn date_and_classifier_helpers_cover_policy_edge_cases() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        assert_eq!(add_days((1970, 1, 1), 31), (1970, 2, 1));
        assert_eq!(fmt_ymd((2026, 6, 9)), "2026-06-09");
        assert!(is_past_date("not-a-date"));
        assert!(is_policy_broad_glob("docs/**"));
        assert!(is_policy_broad_glob("**/*"));
        assert!(!is_policy_broad_glob("docs/*.md"));
        assert_eq!(classify_dir("docs"), "docs");
        assert_eq!(classify_dir("scripts"), "build");
        assert_eq!(classify_dir("unknown"), "tbd");
        assert_eq!(classify_ext("py"), "build");
        assert_eq!(classify_ext("png"), "data");
        assert_eq!(classify_ext("mystery"), "tbd");
        assert_eq!(script_language("tools/check.ps1"), Some("shell"));
        assert_eq!(script_language("docs/readme.md"), None);
    }

    // --- render_markdown smoke ---

    #[test]
    fn render_markdown_contains_summary_heading() {
        let records = vec![
            FileRecord {
                path: "src/lib.rs".to_string(),
                extension: "rs".to_string(),
                category: "rust".to_string(),
                allowlisted: false,
                entry: None,
            },
            FileRecord {
                path: "README.md".to_string(),
                extension: "md".to_string(),
                category: "documentation".to_string(),
                allowlisted: true,
                entry: Some(make_entry("e1", Some("*.md"), None, "documentation")),
            },
            FileRecord {
                path: "unknown.xyz".to_string(),
                extension: "xyz".to_string(),
                category: "unclassified".to_string(),
                allowlisted: false,
                entry: None,
            },
        ];
        let md = render_markdown(&records);
        assert!(md.contains("# Non-Rust File Inventory"), "missing H1");
        assert!(md.contains("## Summary"), "missing Summary section");
        assert!(md.contains("## Unclassified files"), "missing Unclassified section");
        assert!(md.contains("## Allowlisted non-Rust files"), "missing Allowlisted section");
    }
    // --- projection self-consistency (#1800 review) ---

    fn record(path: &str, category: &str, allowlisted: bool) -> FileRecord {
        FileRecord {
            path: path.to_string(),
            extension: "json".to_string(),
            category: category.to_string(),
            allowlisted,
            entry: None,
        }
    }

    /// A projection that emits one file twice - the duplicated-row defect the
    /// review filed - must fail closed instead of shipping contradictory
    /// denominators.
    #[test]
    fn projection_with_duplicate_rows_fails_closed() {
        let records = vec![
            record("archive/a.json", "documentation", true),
            record("docs/b.md", "documentation", true),
            record("archive/a.json", "documentation", true),
        ];
        let markdown = render_markdown(&records);
        let error = verify_inventory_projection(&markdown).expect_err("duplicate rows must fail");
        assert!(
            error.to_string().contains("duplicate file rows for `archive/a.json`"),
            "unexpected error: {error}"
        );
    }

    /// A path projected under both the unclassified and allowlisted tables is
    /// also a duplicate row and fails closed.
    #[test]
    fn projection_with_cross_table_path_fails_closed() {
        let records = vec![
            record("docs/b.md", "documentation", true),
            record("docs/b.md", "unclassified", false),
        ];
        let markdown = render_markdown(&records);
        assert!(
            verify_inventory_projection(&markdown).is_err(),
            "a path in both tables must fail closed"
        );
    }

    /// Summary totals that disagree with the emitted table rows - the stale
    /// summary the review filed - must fail closed.
    #[test]
    fn projection_with_stale_summary_fails_closed() {
        let records = vec![record("docs/b.md", "documentation", true)];
        let mut markdown = render_markdown(&records);
        markdown = markdown.replace("| Allowlisted | 1 |", "| Allowlisted | 19 |");
        let error = verify_inventory_projection(&markdown).expect_err("stale summary must fail");
        assert!(
            error.to_string().contains("allowlisted files but the table projects"),
            "unexpected error: {error}"
        );
    }

    /// A single-pass projection with unique rows and matching summary totals
    /// verifies cleanly.
    #[test]
    fn single_pass_projection_verifies_cleanly() {
        let records = vec![
            record("docs/b.md", "documentation", true),
            record("archive/a.json", "documentation", true),
            record("notes/c.txt", "unclassified", false),
        ];
        let markdown = render_markdown(&records);
        assert!(
            verify_inventory_projection(&markdown).is_ok(),
            "consistent projection must verify"
        );
    }

    fn commit_fixture(root: &Path, message: &str) -> Result<String> {
        run_git(root, &["config", "user.email", "fixture@example.invalid"])?;
        run_git(root, &["config", "user.name", "fixture"])?;
        run_git(root, &["add", "-A"])?;
        run_git(root, &["commit", "-qm", message])?;
        let sha = git_object(root, &["rev-parse", "HEAD"])?;
        Ok(String::from_utf8(sha)?.trim().to_string())
    }

    fn exact_fixture() -> Result<(tempfile::TempDir, String)> {
        let temp = tempfile::tempdir()?;
        init_tracked_fixture(
            temp.path(),
            &[
                ("README.md", "fixture\n"),
                ("policy/non-rust-allowlist.toml", &readme_allowlist_toml()?),
                (
                    ".github/workflows/non-rust-policy.yml",
                    include_str!("../../../.github/workflows/non-rust-policy.yml"),
                ),
            ],
        )?;
        let base = commit_fixture(temp.path(), "base")?;
        Ok((temp, base))
    }

    #[test]
    fn exact_tree_rejects_new_unclassified_path_and_writes_receipt() -> Result<()> {
        let (temp, base) = exact_fixture()?;
        write_fixture(temp.path(), "notes.txt", "new\n")?;
        let subject = commit_fixture(temp.path(), "unclassified")?;
        let receipt = temp.path().join("receipt.json");
        let error = non_rust_exact_tree(temp.path(), &base, &subject, None, &receipt, None, None)
            .expect_err("new unclassified path must fail");
        assert!(error.to_string().contains("notes.txt"));
        let receipt: ExactTreePolicyReceipt = serde_json::from_str(&fs::read_to_string(receipt)?)?;
        assert_eq!(receipt.new_unclassified_paths, vec!["notes.txt"]);
        assert_eq!(receipt.outcome, "fail");
        Ok(())
    }

    #[test]
    fn exact_tree_catches_rename_and_allowlist_regressions() -> Result<()> {
        let (temp, base) = exact_fixture()?;
        run_git(temp.path(), &["mv", "README.md", "notes.txt"])?;
        let subject = commit_fixture(temp.path(), "rename")?;
        let error = non_rust_exact_tree(
            temp.path(),
            &base,
            &subject,
            None,
            &temp.path().join("rename.json"),
            None,
            None,
        )
        .expect_err("rename into an unclassified path must fail");
        assert!(error.to_string().contains("notes.txt"));

        let (temp, base) = exact_fixture()?;
        write_fixture(temp.path(), "policy/non-rust-allowlist.toml", "allow = []\n")?;
        let subject = commit_fixture(temp.path(), "allowlist regression")?;
        let error = non_rust_exact_tree(
            temp.path(),
            &base,
            &subject,
            None,
            &temp.path().join("allowlist.json"),
            None,
            None,
        )
        .expect_err("removing allowlist coverage must fail");
        assert!(error.to_string().contains("README.md"));
        Ok(())
    }

    #[test]
    fn exact_tree_rejects_malformed_and_unrelated_subject_identity() -> Result<()> {
        let (temp, base) = exact_fixture()?;
        let error = non_rust_exact_tree(
            temp.path(),
            "not-a-sha",
            &base,
            None,
            &temp.path().join("bad.json"),
            None,
            None,
        )
        .expect_err("malformed base must fail");
        assert!(error.to_string().contains("git rev-parse"));
        let error = non_rust_exact_tree(
            temp.path(),
            &base,
            &base,
            Some("deadbeef"),
            &temp.path().join("head.json"),
            None,
            None,
        )
        .expect_err("unrelated PR head must fail");
        assert!(error.to_string().contains("does not contain PR head"));
        Ok(())
    }

    #[test]
    fn exact_tree_accepts_unchanged_trusted_workflow() -> Result<()> {
        let (temp, base) = exact_fixture()?;
        write_fixture(temp.path(), "README.md", "updated fixture\n")?;
        let subject = commit_fixture(temp.path(), "documentation-only change")?;
        let receipt = temp.path().join("unchanged-workflow.json");

        non_rust_exact_tree(temp.path(), &base, &subject, None, &receipt, None, None)?;
        Ok(())
    }

    #[test]
    fn trusted_workflow_rejects_alternate_candidate_checkout_or_import() -> Result<()> {
        for injected in [
            "\n          git checkout \"$SUBJECT_SHA\"\n",
            "\n          git worktree add candidate \"$SUBJECT_SHA\"\n",
            "\n          source ./candidate.sh\n",
        ] {
            let (temp, base) = exact_fixture()?;
            let workflow_path = ".github/workflows/non-rust-policy.yml";
            let workflow = fs::read_to_string(temp.path().join(workflow_path))?;
            // A changed trusted workflow must increment its contract-version by
            // exactly one before later checks run; follow that protocol so the
            // injection itself is what gets rejected.
            let version_marker = "# contract-version: ";
            let version = workflow
                .lines()
                .find_map(|line| line.trim().strip_prefix(version_marker))
                .and_then(|value| value.trim().parse::<u64>().ok())
                .expect("fixture workflow must carry a contract-version");
            let workflow = workflow.replacen(
                &format!("{version_marker}{version}"),
                &format!("{version_marker}{}", version + 1),
                1,
            );
            let workflow = workflow.replacen(
                "      - name: Upload exact-tree receipt",
                &(injected.to_string() + "      - name: Upload exact-tree receipt"),
                1,
            );
            write_fixture(temp.path(), workflow_path, &workflow)?;
            let subject = commit_fixture(temp.path(), "untrusted workflow")?;
            let error = validate_subject_workflow(temp.path(), &base, &subject)
                .expect_err("candidate checkout/import must be rejected");
            assert!(
                error.to_string().contains("must not execute or import candidate-derived content"),
                "unexpected error: {error}"
            );
        }
        Ok(())
    }

    #[test]
    fn trusted_workflow_scans_executable_cargo_runs_not_validator_source() -> Result<()> {
        let (temp, base) = exact_fixture()?;
        let workflow_path = ".github/workflows/non-rust-policy.yml";
        let workflow = fs::read_to_string(temp.path().join(workflow_path))?;
        let workflow = workflow.replacen(
            "          run_bodies = []\n",
            "          # cargo run github.event.pull_request.head.sha\n          run_bodies = []\n",
            1,
        );
        let version_marker = "# contract-version: ";
        let version = workflow
            .lines()
            .find_map(|line| line.trim().strip_prefix(version_marker))
            .and_then(|value| value.trim().parse::<u64>().ok())
            .expect("fixture workflow must carry a contract-version");
        let workflow = workflow.replacen(
            &format!("{version_marker}{version}"),
            &format!("{version_marker}{}", version + 1),
            1,
        );
        write_fixture(temp.path(), workflow_path, &workflow)?;
        let subject = commit_fixture(temp.path(), "validator source comment")?;
        validate_subject_workflow(temp.path(), &base, &subject)?;

        let (temp, base) = exact_fixture()?;
        let workflow_path = ".github/workflows/non-rust-policy.yml";
        let workflow = fs::read_to_string(temp.path().join(workflow_path))?;
        let version = workflow
            .lines()
            .find_map(|line| line.trim().strip_prefix(version_marker))
            .and_then(|value| value.trim().parse::<u64>().ok())
            .expect("fixture workflow must carry a contract-version");
        let workflow = workflow.replacen(
            &format!("{version_marker}{version}"),
            &format!("{version_marker}{}", version + 1),
            1,
        );
        let workflow = workflow.replacen(
            "run: cargo run --locked -p xtask -- non-rust exact-tree",
            "run: cargo run --locked -p xtask -- non-rust exact-tree --subject-sha ${{ github.event.pull_request.head.sha }}",
            1,
        );
        write_fixture(temp.path(), workflow_path, &workflow)?;
        let subject = commit_fixture(temp.path(), "candidate interpolation")?;
        let error = validate_subject_workflow(temp.path(), &base, &subject)
            .expect_err("candidate-derived evaluator interpolation must fail closed");
        assert!(
            error.to_string().contains("must not execute candidate source"),
            "unexpected error: {error}"
        );
        Ok(())
    }

    #[test]
    fn trusted_workflow_rejects_candidate_refs_in_step_inputs_and_whitespace_variants() -> Result<()>
    {
        let workflow_path = ".github/workflows/non-rust-policy.yml";
        let version_marker = "# contract-version: ";
        for injected in [
            "\n      - name: candidate ref input\n        env:\n          REF: refs/pull/${{ github.event.pull_request.number }}/head\n        run: echo \"$REF\"\n",
            "\n      - name: candidate ref command\n        run: cargo  run --locked -p xtask -- non-rust exact-tree --subject-sha ${{ github.event.pull_request.head.sha }}\n",
        ] {
            let (temp, base) = exact_fixture()?;
            let workflow = fs::read_to_string(temp.path().join(workflow_path))?;
            let version = workflow
                .lines()
                .find_map(|line| line.trim().strip_prefix(version_marker))
                .and_then(|value| value.trim().parse::<u64>().ok())
                .expect("fixture workflow must carry a contract-version");
            let workflow = workflow.replacen(
                &format!("{version_marker}{version}"),
                &format!("{version_marker}{}", version + 1),
                1,
            );
            let workflow = workflow.replacen(
                "      - name: Upload exact-tree receipt",
                &(injected.to_string() + "      - name: Upload exact-tree receipt"),
                1,
            );
            write_fixture(temp.path(), workflow_path, &workflow)?;
            let subject = commit_fixture(temp.path(), "candidate ref bypass")?;
            let error = validate_subject_workflow(temp.path(), &base, &subject)
                .expect_err("candidate-controlled ref must fail closed");
            assert!(
                error.to_string().contains("must not execute candidate source"),
                "unexpected error: {error}"
            );
        }
        Ok(())
    }

    #[test]
    fn trusted_workflow_rejects_duplicate_reserved_contract_id() -> Result<()> {
        let (temp, base) = exact_fixture()?;
        let workflow_path = ".github/workflows/non-rust-policy.yml";
        let workflow = fs::read_to_string(temp.path().join(workflow_path))?;
        let version_marker = "# contract-version: ";
        let version = workflow
            .lines()
            .find_map(|line| line.trim().strip_prefix(version_marker))
            .and_then(|value| value.trim().parse::<u64>().ok())
            .ok_or_else(|| eyre!("fixture workflow must carry a contract-version"))?;
        let workflow = workflow.replacen(
            &format!("{version_marker}{version}"),
            &format!("{version_marker}{}", version + 1),
            1,
        );
        let duplicate = "      - id: verify-trusted-workflow-contract\n        name: Unexpected duplicate\n        run: echo duplicate\n";
        let workflow = workflow.replacen(
            "      - name: Upload exact-tree receipt",
            &(duplicate.to_string() + "      - name: Upload exact-tree receipt"),
            1,
        );
        write_fixture(temp.path(), workflow_path, &workflow)?;
        let subject = commit_fixture(temp.path(), "duplicate reserved workflow id")?;
        let error = validate_subject_workflow(temp.path(), &base, &subject)
            .expect_err("duplicate reserved workflow ID must fail closed");
        assert!(
            error.to_string().contains("exactly one reserved contract-verification step ID"),
            "unexpected error: {error}"
        );
        Ok(())
    }

    #[test]
    fn exact_tree_workflow_keeps_trusted_shadow_contract() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".github/workflows/non-rust-policy.yml");
        let workflow = fs::read_to_string(root)?;
        for required in [
            "pull_request_target:",
            "merge_group:",
            "push:",
            "workflow_dispatch:",
            "permissions:\n  contents: read",
            "ref: ${{ env.EVALUATOR_SHA }}",
            "merge-base --is-ancestor",
            "Non-Rust policy exact-tree",
            "if: always()",
            "persist-credentials: false",
        ] {
            assert!(workflow.contains(required), "workflow missing `{required}`");
        }
        assert!(!workflow.contains("actions/checkout@v"), "actions must be pinned");
        Ok(())
    }
}
