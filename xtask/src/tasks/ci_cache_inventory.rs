//! Active CI cache inventory + `ci_cache_receipt.v1` (#9177).
//!
//! Static-source inventory of every `Swatinem/rust-cache` and `actions/cache`
//! (including `/restore` and `/save`) step reachable from
//! `.github/workflows/**`, plus the two composite actions under
//! `.github/actions/**` that embed a cache action. This changes no cache
//! behavior: it classifies reachability, save authority, and byte
//! provenance already present in source, and grants no save authority of
//! its own.
//!
//! Claim boundary: active cache consumer/writer identity, reachability, and
//! authority evidence only. This does not observe live restore/save
//! telemetry (there is no CI hook wired up for that), does not change any
//! `save-if`, key, path, job `if:`, or workflow `on:`, and does not repair
//! any writer it finds unguarded — repair is #13927's / the leaf owner's
//! claim. `docs-pr-build.yml`'s unguarded `actions/cache` is recorded
//! honestly here, not fixed.
//!
//! Reachability primitives are reused from [`super::workflow_policy_lint`],
//! not duplicated: `workflow_on`, `triggers`, `is_pull_request`,
//! `is_pull_request_target`, `condition_excludes_pull_request`,
//! `split_top_level`, `strip_outer_parentheses`. Candidate-event exclusion
//! is *not* reused from the lint crate: it is re-asked per candidate event
//! by `condition_statically_excludes_candidate_events` (the lint crate's
//! answer only proves `pull_request` exclusion, and `merge_group` is a
//! candidate event here).
//!
//! Interpretive choices this deriver makes where the build spec leaves the
//! exact algorithm unstated (documented here so a reviewer can challenge
//! them specifically):
//!
//! - `executed_subject` is derived purely from the workflow's `on:` trigger
//!   set, by descending priority: `candidate` (any PR-adjacent trigger) >
//!   `merge_result` (`merge_group`) > `default_branch` (`push`) >
//!   `fixed_trusted_ref` (`workflow_dispatch`/`schedule`) > `other`.
//! - a composite action's cache reference lives in `families` when the
//!   composite has an active caller (composite steps run inline within
//!   their callers, so the site is active behavior and must be in the
//!   active denominator), and in `dormant` only when the composite has no
//!   active caller — matching `docs/ci/cache-policy.md`. The row `id`
//!   scheme (`<composite-action-path>/<ordinal>`) carries the `job:
//!   composite-action` marker, and a referenced composite (e.g.
//!   `main-history-event.yml` uses `setup-rust`) borrows its caller's
//!   trigger set for reachability; an unreferenced composite
//!   (`setup-perl-lsp`, referenced only from documentation) gets
//!   `statically_dead` with a note explaining there is no active caller.
//! - `payload_class` for `Swatinem/rust-cache` is always `cargo_target`
//!   (the action manages the registry, git-dep, and target caches together
//!   internally and exposes no `path:` to inspect); for `actions/cache`
//!   family actions it is keyword-matched off the configured `path:` list.
//! - `key_authority` depends on reachability, not only key text: a row the
//!   candidate cannot reach at all (anything other than
//!   `candidate_event_reachability: reachable`) is `trusted` regardless of
//!   its key; on a reachable row, a key referencing `hashFiles(`,
//!   `github.head_ref`, `github.event.pull_request`, `github.event.number`,
//!   `github.sha`, `github.ref`, or `inputs.` is candidate-influenceable —
//!   `candidate`, or `mixed` when it also carries a demonstrably trusted
//!   dimension (`runner.os`, or a literal prefix outside any `${{ }}`
//!   expression) — and an empty key is `not_proven`. This is the falsifier
//!   #9177 names: a candidate-controlled or mixed key must never read
//!   `trusted` merely because it lacks the literal substrings
//!   `pull_request`/`head_ref`.
//! - `ref_literal_guard`/`SaveAuthoritySource::RefGuard` require a
//!   *positive* equality against a canonical default-branch ref
//!   (`github.ref == 'refs/heads/main'`/`'refs/heads/master'`, either quote
//!   style, whitespace-insensitive, searched through the condition's
//!   `||`/`&&` structure; or `github.ref_name ==
//!   github.event.repository.default_branch`) — never a bare substring
//!   match on `github.ref`, which would also bless a negated guard like
//!   `github.ref != 'refs/heads/main'` (saves on everything *except* main).
//! - `writer_disposition::not_candidate_reachable` (not `trusted_guarded`)
//!   is returned when the trigger set has no candidate-controllable event
//!   at all: a trusted *event set* is not a save *guard*, and conflating the
//!   two would let a consumer filtering on `trusted_guarded` wrongly absorb
//!   a row whose own `save_authority_source` reads `absent`.
//! - `writer_disposition::candidate_writer_violation` is reserved for
//!   events under which the candidate tree is *demonstrably* the executed
//!   subject (`pull_request`, `pull_request_target`, `merge_group`,
//!   `issue_comment`, `pull_request_review`, `pull_request_review_comment`).
//!   `workflow_run`/`workflow_call` stay in the reachability axis's
//!   candidate-event set (this per-file walk cannot prove they are safe),
//!   but they run the default branch's own workflow definition — the actual
//!   checked-out subject is caller/artifact-resolved, not statically
//!   knowable — so an unguarded writer reachable *only* through one of them
//!   is `not_proven`, with `artifact_provenance` naming the statically
//!   knowable producer (`on.workflow_run.workflows:`) when there is one.
//!   The two axes, reachability and disposition, staying independent is the
//!   point.
//! - `cached_byte_provenance` is `trusted_tree` for `trusted_guarded`,
//!   `reviewed_exception`, `statically_excluded`, and
//!   `not_candidate_reachable`; `candidate_tree` for
//!   `candidate_writer_violation` and for the `pull_request_target`
//!   ref-only-guard rule; `not_proven` for every other `not_proven` row, and
//!   — because a restore-only step writes nothing and a statically dead
//!   step never executes — also for `restore_only` and `statically_dead`.
//! - There is no live CI telemetry hook. `observations[]` is therefore
//!   always emitted with `not_proven`/`unknown`/`unavailable` defaults per
//!   family; nothing here ever auto-labels a hit as work avoided.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use serde_yaml_ng::{Mapping, Value};

use crate::tasks::workflow_policy_lint::{
    condition_excludes_pull_request, is_pull_request_target, split_top_level,
    strip_outer_parentheses, triggers, workflow_on,
};
use crate::utils::project_root;

/// Schema id for the emitted receipt (`schemas/ci_cache_receipt.v1.schema.json`).
pub const SCHEMA: &str = "ci_cache_receipt.v1";
/// Schema id for the checked-in manifest.
pub const INVENTORY_SCHEMA: &str = "ci_cache_inventory.v1";
/// The only schema version this producer knows how to emit and validate.
pub const SUPPORTED_API_VERSION: &str = "v1";

/// Fail loudly when a caller pins a schema version this producer cannot
/// emit, instead of silently handing over a shape it does not parse.
fn ensure_supported_api_version(api_version: &str) -> Result<()> {
    if api_version != SUPPORTED_API_VERSION {
        bail!(
            "unsupported --api-version `{api_version}`: this producer pins \
             `{SUPPORTED_API_VERSION}` (receipt {SCHEMA}, manifest {INVENTORY_SCHEMA})"
        );
    }
    Ok(())
}
/// Checked-in durable inventory this task derives and diffs against.
pub const MANIFEST: &str = ".ci/ci-cache/cache-inventory.v1.json";

const SCHEMA_PATH: &str = "schemas/ci_cache_receipt.v1.schema.json";
const WORKFLOWS_DIR: &str = ".github/workflows";
const ACTIONS_DIR: &str = ".github/actions";
const OWNER_ISSUE: &str = "#9177";
const POLICY_DOC: &str = "docs/ci/cache-policy.md";
const CLAIM_BOUNDARY: &str = "Active cache consumer/writer identity, reachability, and \
authority evidence only; this artifact changes no cache behavior and grants no save \
authority.";

/// Events from which a candidate (an open PR, or content it controls) can
/// cause a workflow run to start. Mirrors `docs/ci/cache-policy.md` and the
/// build spec for #9177 — deliberately broader than
/// `workflow_policy_lint::PR_CONTROLLED_TRIGGERS` (that list also excludes
/// `merge_group`, which this inventory must treat as candidate-adjacent).
const CANDIDATE_EVENTS: &[&str] = &[
    "pull_request",
    "pull_request_target",
    "merge_group",
    "workflow_call",
    "workflow_run",
    "issue_comment",
    "pull_request_review",
    "pull_request_review_comment",
];

/// Events under which the candidate tree is *demonstrably* the executed
/// subject. `workflow_run`/`workflow_call` stay in [`CANDIDATE_EVENTS`] for
/// the reachability axis (this per-file walk cannot prove they are safe),
/// but they run the default branch's own workflow definition — the actual
/// checked-out ref is caller/artifact-resolved and not statically knowable,
/// so an unguarded writer reachable *only* through one of them is
/// `not_proven`, not a `candidate_writer_violation`. The two axes
/// (reachability vs. disposition) staying independent is the point.
const DEMONSTRABLE_CANDIDATE_TREE_EVENTS: &[&str] = &[
    "pull_request",
    "pull_request_target",
    "merge_group",
    "issue_comment",
    "pull_request_review",
    "pull_request_review_comment",
];

// ---------------------------------------------------------------------------
// Row schema
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    RestoreOnly,
    SaveOnly,
    RestoreAndSave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateEventReachability {
    Reachable,
    StaticallyExcluded,
    StaticallyDead,
    NotCandidateEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutedSubject {
    Candidate,
    MergeResult,
    DefaultBranch,
    FixedTrustedRef,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveAuthoritySource {
    EventGuard,
    RefGuard,
    EventAndRefGuard,
    JobStaticExclusion,
    ContentSuccessOnly,
    Absent,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadClass {
    RegistryGitDependency,
    CargoTarget,
    CompilerObject,
    CorpusSetup,
    ToolDownload,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAuthority {
    Trusted,
    Candidate,
    Mixed,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CachedByteProvenance {
    TrustedTree,
    CandidateTree,
    DownloadedArtifact,
    Mixed,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterDisposition {
    TrustedGuarded,
    CandidateWriterViolation,
    StaticallyExcluded,
    StaticallyDead,
    RestoreOnly,
    ReviewedException,
    /// The workflow's trigger set contains no candidate-controllable event
    /// at all (`push`/`schedule`/`workflow_dispatch` only): there is no save
    /// *guard* here, because there is no candidate-reachable event to guard
    /// against. Distinct from `trusted_guarded`, which asserts an actual
    /// guard was checked and holds — conflating the two would let a
    /// consumer filtering on `trusted_guarded` wrongly absorb a row whose
    /// own `save_authority_source` reads `absent`.
    NotCandidateReachable,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionKind {
    Swatinem,
    Cache,
    CacheRestore,
    CacheSave,
}

impl ActionKind {
    /// A step is a cache site iff its `uses` begins `Swatinem/rust-cache` or
    /// `actions/cache` (including the `/restore` and `/save` subpaths).
    /// Checked most-specific-first so `actions/cache/restore` is never
    /// mis-read as bare `actions/cache`.
    fn from_uses(uses: &str) -> Option<Self> {
        if uses.starts_with("actions/cache/restore") {
            Some(Self::CacheRestore)
        } else if uses.starts_with("actions/cache/save") {
            Some(Self::CacheSave)
        } else if uses.starts_with("actions/cache") {
            Some(Self::Cache)
        } else if uses.starts_with("Swatinem/rust-cache") {
            Some(Self::Swatinem)
        } else {
            None
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Swatinem => "Swatinem/rust-cache",
            Self::Cache => "actions/cache",
            Self::CacheRestore => "actions/cache/restore",
            Self::CacheSave => "actions/cache/save",
        }
    }

    fn capability(self) -> Capability {
        match self {
            Self::CacheRestore => Capability::RestoreOnly,
            Self::CacheSave => Capability::SaveOnly,
            Self::Cache | Self::Swatinem => Capability::RestoreAndSave,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyDimensions {
    pub key: Option<String>,
    pub shared_key: Option<String>,
    pub restore_keys: Option<Vec<String>>,
}

/// One row: one active (or dormant) cache site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheFamilyRow {
    pub id: String,
    pub workflow: String,
    pub job: String,
    pub step_name: String,
    pub action: String,
    pub action_ref: String,
    pub capability: Capability,
    pub trigger_event_set: Vec<String>,
    pub candidate_event_reachability: CandidateEventReachability,
    pub executed_subject: ExecutedSubject,
    pub job_condition: Option<String>,
    pub step_condition: Option<String>,
    pub save_condition: Option<String>,
    pub save_authority_source: SaveAuthoritySource,
    pub payload_class: PayloadClass,
    pub cached_paths: Vec<String>,
    pub key_dimensions: KeyDimensions,
    pub key_authority: KeyAuthority,
    pub cached_byte_provenance: CachedByteProvenance,
    pub artifact_provenance: Option<String>,
    pub writer_disposition: WriterDisposition,
    pub repair_owner: Option<String>,
    pub notes: Option<String>,
}

/// Result of deriving the cache-site inventory from one workflows/actions tree.
#[derive(Debug, Clone, Default)]
pub struct DerivedInventory {
    pub families: Vec<CacheFamilyRow>,
    pub dormant: Vec<CacheFamilyRow>,
}

// ---------------------------------------------------------------------------
// Reachability / classification helpers
// ---------------------------------------------------------------------------

fn strip_expr_wrapper(condition: &str) -> &str {
    let condition = condition.trim();
    condition
        .strip_prefix("${{")
        .and_then(|inner| inner.strip_suffix("}}"))
        .map(str::trim)
        .unwrap_or(condition)
}

fn excludes_pull_request(condition: Option<&str>) -> bool {
    let Some(condition) = condition else {
        return false;
    };
    condition_excludes_pull_request(strip_expr_wrapper(condition))
}

/// The job/step `if:` is a top-level `&&` chain containing a literal `false`
/// operand (so `false && X`), or is the bare literal `false` on its own —
/// classified before PR-exclusion, per the build spec.
fn is_statically_dead(condition: Option<&str>) -> bool {
    let Some(condition) = condition else {
        return false;
    };
    let condition = strip_expr_wrapper(condition);
    let Some(condition) = strip_outer_parentheses(condition) else {
        return false;
    };
    let Some(terms) = split_top_level(condition, "&&") else {
        return false;
    };
    terms.iter().any(|term| {
        let term = strip_outer_parentheses(term).unwrap_or(term);
        term.trim() == "false"
    })
}

/// Inventory-specific exclusion proof: the condition must be provably false
/// under *every* candidate event present in the workflow's trigger set.
/// [`super::workflow_policy_lint::condition_excludes_pull_request`] answers
/// a narrower question — it treats a `merge_group` anchor as trusted because
/// merge-group content passed review — but `merge_group` is itself a
/// candidate event in this inventory, so promoting that answer to
/// `statically_excluded` would drop a merge-group-only cache site from the
/// candidate denominator. This predicate re-asks the exclusion question per
/// candidate trigger. When the workflow's trigger set has no candidate event
/// at all this returns false, so reachability resolves to the more precise
/// `not_candidate_event` instead of shadowing it with `statically_excluded`.
fn condition_statically_excludes_candidate_events(
    condition: Option<&str>,
    trigger_event_set: &[String],
) -> bool {
    let Some(condition) = condition else {
        return false;
    };
    let condition = strip_expr_wrapper(condition);
    let candidate_triggers: Vec<&String> = trigger_event_set
        .iter()
        .filter(|event| CANDIDATE_EVENTS.contains(&event.as_str()))
        .collect();
    if candidate_triggers.is_empty() {
        return false;
    }
    candidate_triggers.iter().all(|event| condition_excludes_candidate_event(condition, event))
}

/// True when the condition cannot hold when `github.event_name == event`.
/// Sound only for the equality-anchored subset of expressions: any other
/// term fails the proof and the site stays reachable (conservative).
fn condition_excludes_candidate_event(condition: &str, event: &str) -> bool {
    let Some(condition) = strip_outer_parentheses(condition) else {
        return false;
    };
    let Some(branches) = split_top_level(condition, "||") else {
        return false;
    };
    !branches.is_empty()
        && branches.iter().all(|branch| branch_excludes_candidate_event(branch, event))
}

fn branch_excludes_candidate_event(branch: &str, event: &str) -> bool {
    let Some(branch) = strip_outer_parentheses(branch) else {
        return false;
    };
    let Some(or_branches) = split_top_level(branch, "||") else {
        return false;
    };
    if or_branches.len() > 1 {
        return or_branches.iter().all(|branch| branch_excludes_candidate_event(branch, event));
    }
    let Some(terms) = split_top_level(branch, "&&") else {
        return false;
    };
    !terms.is_empty() && terms.iter().any(|term| term_excludes_candidate_event(term, event))
}

fn term_excludes_candidate_event(term: &str, event: &str) -> bool {
    let Some(stripped_once) = strip_outer_parentheses(term) else {
        return false;
    };
    let normalized: String = stripped_once.chars().filter(|ch| !ch.is_whitespace()).collect();
    if let Some(rest) = normalized.strip_prefix("github.event_name==") {
        // The remainder must be exactly one quoted literal — an `||`/`&&`
        // chain merely *starting* with an equality is not a single anchor,
        // so reject any second quote inside the span.
        let bytes = rest.as_bytes();
        if bytes.len() >= 2
            && (bytes[0] == b'\'' || bytes[0] == b'"')
            && bytes[bytes.len() - 1] == bytes[0]
            && !rest[1..rest.len() - 1].contains(&rest[0..1])
        {
            let anchor = &rest[1..rest.len() - 1];
            return anchor != event;
        }
    }
    // A parenthesized sub-condition excludes the event when the sub-condition
    // itself provably cannot hold under it (same recursive shape as the lint
    // crate's `term_has_trusted_event_anchor`).
    stripped_once != term && condition_excludes_candidate_event(stripped_once, event)
}

fn executed_subject_for(trigger_event_set: &[String]) -> ExecutedSubject {
    let has = |name: &str| trigger_event_set.iter().any(|t| t == name);
    if has("pull_request")
        || has("pull_request_target")
        || has("issue_comment")
        || has("pull_request_review")
        || has("pull_request_review_comment")
    {
        ExecutedSubject::Candidate
    } else if has("merge_group") {
        ExecutedSubject::MergeResult
    } else if has("push") {
        ExecutedSubject::DefaultBranch
    } else if has("workflow_dispatch") || has("schedule") {
        ExecutedSubject::FixedTrustedRef
    } else {
        ExecutedSubject::Other
    }
}

fn is_candidate_event(trigger_event_set: &[String]) -> bool {
    trigger_event_set.iter().any(|t| CANDIDATE_EVENTS.contains(&t.as_str()))
}

/// The workflow names declared under `on.workflow_run.workflows:`, if any —
/// the statically-knowable producer(s) of a `workflow_run` trigger.
fn workflow_run_producers(doc: &Value) -> Vec<String> {
    let Some(on) = workflow_on(doc).and_then(Value::as_mapping) else {
        return Vec::new();
    };
    mapping_get(on, "workflow_run")
        .and_then(Value::as_mapping)
        .and_then(|m| mapping_get(m, "workflows"))
        .and_then(Value::as_sequence)
        .map(|seq| seq.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// A row reachable only through `workflow_run`/`workflow_call` (never a
/// [`DEMONSTRABLE_CANDIDATE_TREE_EVENTS`] member) has an executed subject
/// that is resolved by the caller or a downloaded artifact, not by this
/// workflow file. Name the statically-knowable producer when there is one,
/// so `not_proven` carries an explanation instead of a bare unknown.
fn indirect_artifact_provenance(doc: &Value, trigger_event_set: &[String]) -> Option<String> {
    if has_demonstrable_candidate_event(trigger_event_set) || !is_candidate_event(trigger_event_set)
    {
        return None;
    }
    let producers = workflow_run_producers(doc);
    if !producers.is_empty() {
        return Some(format!("workflow_run producer: {}", producers.join(", ")));
    }
    if trigger_event_set.iter().any(|t| t == "workflow_run") {
        return Some(
            "workflow_run producer: not statically named (no on.workflow_run.workflows list)"
                .to_string(),
        );
    }
    if trigger_event_set.iter().any(|t| t == "workflow_call") {
        return Some(
            "workflow_call producer: caller-defined; not statically knowable from this workflow file"
                .to_string(),
        );
    }
    None
}

/// A *positive* equality against a canonical default-branch ref —
/// `github.ref == 'refs/heads/main'`/`'refs/heads/master'` (either quote
/// style, whitespace-insensitive), or
/// `github.ref_name == github.event.repository.default_branch` — searched
/// through the condition's `||`/`&&` structure. The connective semantics
/// are load-bearing:
/// - a top-level `&&` chain is ref-guarded when *any* term establishes the
///   equality (all terms must hold, so the equality is forced);
/// - a top-level `||` expression is ref-guarded only when *every* branch
///   establishes the equality — one guarded branch cannot constrain its
///   siblings, since `A || B` is true whenever `B` alone is true (e.g.
///   `github.ref == 'refs/heads/main' || github.event_name == 'pull_request'`
///   is true on pull requests with a candidate ref, so it guards nothing).
///
/// A condition that merely *mentions* a ref (including a negated check like
/// `github.ref != 'refs/heads/main'`, which saves on every branch except
/// main — the opposite of a guard) does not count: `github.ref_name`
/// containing `github.ref` as a substring is exactly why this matches
/// normalized full comparisons, never a bare substring.
fn ref_literal_guard(condition: &str) -> bool {
    has_canonical_ref_equality(condition)
}

fn has_canonical_ref_equality(condition: &str) -> bool {
    let Some(condition) = strip_outer_parentheses(condition) else {
        return false;
    };
    if let Some(branches) = split_top_level(condition, "||")
        && branches.len() > 1
    {
        return branches.iter().all(|branch| has_canonical_ref_equality(branch));
    }
    if let Some(terms) = split_top_level(condition, "&&")
        && terms.len() > 1
    {
        return terms.iter().any(|term| has_canonical_ref_equality(term));
    }
    is_canonical_ref_equality_term(condition)
}

fn is_canonical_ref_equality_term(term: &str) -> bool {
    let Some(term) = strip_outer_parentheses(term) else {
        return false;
    };
    let normalized: String = term.chars().filter(|ch| !ch.is_whitespace()).collect();
    matches!(
        normalized.as_str(),
        "github.ref=='refs/heads/main'"
            | "github.ref==\"refs/heads/main\""
            | "github.ref=='refs/heads/master'"
            | "github.ref==\"refs/heads/master\""
            | "github.ref_name==github.event.repository.default_branch"
            // The `format()` spelling of the line above: `github.ref` carries the
            // full `refs/heads/<name>` form, so comparing it against
            // `format('refs/heads/{0}', <default branch>)` is the same canonical
            // default-branch equality, and is in fact more robust than the
            // `main`/`master` literals because it follows the repository's actual
            // default branch. Accepting one spelling and rejecting the other would
            // understate a stronger guard's authority.
            | "github.ref==format('refs/heads/{0}',github.event.repository.default_branch)"
            | "github.ref==format(\"refs/heads/{0}\",github.event.repository.default_branch)"
    )
}

fn content_success_pattern(condition: &str) -> bool {
    condition.contains("steps.")
        || condition.contains("success(")
        || condition.contains("failure(")
        || condition.contains("always(")
        || condition.contains(".outcome")
        || condition.contains(".conclusion")
}

/// Classify a save-capable step's effective save guard. `absent` when there
/// is no guard at all (`Swatinem/rust-cache` defaults `save-if: true`, and
/// `actions/cache`/`actions/cache/save` have no separate save-if, so an
/// unconditional step is an unconditional writer).
fn classify_save_authority(save_condition: Option<&str>) -> SaveAuthoritySource {
    let Some(raw) = save_condition else {
        return SaveAuthoritySource::Absent;
    };
    let condition = strip_expr_wrapper(raw);
    if condition.trim() == "false" {
        // A literal `false` save-if is a stronger, degenerate case of an
        // event guard: it is PR-false (and every-case-false) by construction,
        // e.g. RIPR's hosted/fallback jobs and corpus-ratchet-bounded, which
        // intentionally never save from their own step and rely on a
        // separate seed/warm job instead.
        return SaveAuthoritySource::EventGuard;
    }
    if condition.trim() == "true" {
        // An explicit unconditional `save-if: true` behaves identically to
        // no guard at all: always saves, on every event including a
        // candidate's own. Treat it exactly like `absent` rather than
        // letting it land in the weaker `not_proven` bucket.
        return SaveAuthoritySource::Absent;
    }
    let has_event = excludes_pull_request(Some(raw));
    let has_ref = ref_literal_guard(condition);
    match (has_event, has_ref) {
        (true, true) => SaveAuthoritySource::EventAndRefGuard,
        (true, false) => SaveAuthoritySource::EventGuard,
        (false, true) => SaveAuthoritySource::RefGuard,
        (false, false) => {
            if content_success_pattern(condition) {
                SaveAuthoritySource::ContentSuccessOnly
            } else {
                SaveAuthoritySource::NotProven
            }
        }
    }
}

/// Does the trigger set include an event under which the candidate tree is
/// demonstrably the executed subject? See
/// [`DEMONSTRABLE_CANDIDATE_TREE_EVENTS`].
fn has_demonstrable_candidate_event(trigger_event_set: &[String]) -> bool {
    trigger_event_set.iter().any(|t| DEMONSTRABLE_CANDIDATE_TREE_EVENTS.contains(&t.as_str()))
}

fn resolve_writer_disposition(
    capability: Capability,
    reachability: CandidateEventReachability,
    trigger_event_set: &[String],
    save_authority_source: SaveAuthoritySource,
) -> WriterDisposition {
    match reachability {
        CandidateEventReachability::StaticallyDead => WriterDisposition::StaticallyDead,
        CandidateEventReachability::StaticallyExcluded => WriterDisposition::StaticallyExcluded,
        _ if capability == Capability::RestoreOnly => WriterDisposition::RestoreOnly,
        // No candidate-controllable event exists to guard against here —
        // this is not the same claim as "a guard was checked and holds"
        // (`trusted_guarded`), even though `save_authority_source` on this
        // row often reads `absent`.
        CandidateEventReachability::NotCandidateEvent => WriterDisposition::NotCandidateReachable,
        CandidateEventReachability::Reachable => {
            let pull_request_target = is_pull_request_target(trigger_event_set);
            match save_authority_source {
                SaveAuthoritySource::RefGuard if pull_request_target => {
                    WriterDisposition::NotProven
                }
                SaveAuthoritySource::RefGuard | SaveAuthoritySource::EventAndRefGuard => {
                    WriterDisposition::TrustedGuarded
                }
                SaveAuthoritySource::EventGuard => WriterDisposition::ReviewedException,
                SaveAuthoritySource::Absent | SaveAuthoritySource::ContentSuccessOnly => {
                    if has_demonstrable_candidate_event(trigger_event_set) {
                        WriterDisposition::CandidateWriterViolation
                    } else {
                        // Reachable only via `workflow_run`/`workflow_call`:
                        // the run executes the default branch's own
                        // workflow definition, and the actual checked-out
                        // subject is caller/artifact-resolved, not
                        // statically knowable — an unguarded writer here is
                        // unproven, not a demonstrated candidate write.
                        WriterDisposition::NotProven
                    }
                }
                SaveAuthoritySource::JobStaticExclusion | SaveAuthoritySource::NotProven => {
                    WriterDisposition::NotProven
                }
            }
        }
    }
}

fn resolve_cached_byte_provenance(
    writer_disposition: WriterDisposition,
    trigger_event_set: &[String],
    save_authority_source: SaveAuthoritySource,
) -> CachedByteProvenance {
    match writer_disposition {
        WriterDisposition::CandidateWriterViolation => CachedByteProvenance::CandidateTree,
        WriterDisposition::NotProven => {
            if is_pull_request_target(trigger_event_set)
                && save_authority_source == SaveAuthoritySource::RefGuard
            {
                CachedByteProvenance::CandidateTree
            } else {
                CachedByteProvenance::NotProven
            }
        }
        // A restore-only step writes nothing, so this row establishes
        // nothing about the provenance of what it restores; a statically
        // dead step never executes at all. Neither is `trusted_tree`.
        WriterDisposition::RestoreOnly | WriterDisposition::StaticallyDead => {
            CachedByteProvenance::NotProven
        }
        WriterDisposition::TrustedGuarded
        | WriterDisposition::ReviewedException
        | WriterDisposition::StaticallyExcluded
        | WriterDisposition::NotCandidateReachable => CachedByteProvenance::TrustedTree,
    }
}

fn payload_class_for(action: ActionKind, cached_paths: &[String]) -> PayloadClass {
    if action == ActionKind::Swatinem {
        return PayloadClass::CargoTarget;
    }
    let joined = cached_paths.join(" ").to_ascii_lowercase();
    if joined.contains("corpus") {
        PayloadClass::CorpusSetup
    } else if joined.contains("registry")
        || joined.contains(".cargo/git")
        || joined.contains("/git")
    {
        PayloadClass::RegistryGitDependency
    } else if joined.contains("bin/") {
        PayloadClass::ToolDownload
    } else if joined.contains("target") {
        PayloadClass::CompilerObject
    } else {
        PayloadClass::Other
    }
}

/// Substrings that make a key/shared-key/restore-key string
/// candidate-influenceable: a candidate controls the content `hashFiles`
/// hashes (e.g. `Cargo.lock` on an open PR), its own head ref/PR number/sha,
/// or an arbitrary `workflow_dispatch` input.
const CANDIDATE_KEY_MARKERS: &[&str] = &[
    "hashFiles(",
    "github.head_ref",
    "github.event.pull_request",
    "github.event.number",
    "github.sha",
    "github.ref",
    "inputs.",
];

/// A demonstrably trusted dimension inside an otherwise candidate-tainted
/// key: `runner.os` (platform, not candidate-chosen), or a literal prefix
/// segment outside any `${{ ... }}` expression (a fixed namespace, not
/// candidate-chosen either).
fn has_trusted_key_dimension(text: &str) -> bool {
    if text.contains("runner.os") {
        return true;
    }
    literal_key_remainder(text).chars().any(char::is_alphanumeric)
}

/// `text` with every `${{ ... }}` expression stripped out, leaving only the
/// literal characters a workflow author wrote around them.
fn literal_key_remainder(text: &str) -> String {
    let mut remainder = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("${{") {
        remainder.push_str(&rest[..start]);
        match rest[start..].find("}}") {
            Some(end) => rest = &rest[start + end + 2..],
            None => return remainder,
        }
    }
    remainder.push_str(rest);
    remainder
}

/// `key_authority` depends on reachability as well as key text: a
/// candidate-influenceable key on a row the candidate cannot reach at all
/// (anything other than `candidate_event_reachability: reachable`) is not a
/// live exposure, so it reads `trusted`. On a reachable row, a
/// `hashFiles(...)`/head-ref/sha/input-derived key is exactly the
/// falsifier #9177 names — a candidate save keyed on candidate-influenced
/// content — and must never read `trusted` merely because the literal
/// substrings `pull_request`/`head_ref` are absent from the text.
fn key_authority_for(
    reachability: CandidateEventReachability,
    key_dimensions: &KeyDimensions,
) -> KeyAuthority {
    if reachability != CandidateEventReachability::Reachable {
        return KeyAuthority::Trusted;
    }
    let mut text = String::new();
    if let Some(key) = &key_dimensions.key {
        text.push_str(key);
    }
    if let Some(shared) = &key_dimensions.shared_key {
        text.push_str(shared);
    }
    if let Some(restore) = &key_dimensions.restore_keys {
        text.push_str(&restore.join(" "));
    }
    if text.is_empty() {
        return KeyAuthority::NotProven;
    }
    let candidate_influenced = CANDIDATE_KEY_MARKERS.iter().any(|marker| text.contains(marker));
    if !candidate_influenced {
        return KeyAuthority::Trusted;
    }
    if has_trusted_key_dimension(&text) { KeyAuthority::Mixed } else { KeyAuthority::Candidate }
}

// ---------------------------------------------------------------------------
// YAML value helpers
// ---------------------------------------------------------------------------

fn mapping_get<'a>(map: &'a Mapping, key: &str) -> Option<&'a Value> {
    map.get(Value::String(key.to_string()))
}

fn value_as_condition_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        other => Some(format!("{other:?}")),
    }
}

fn condition_of(map: &Mapping) -> Option<String> {
    mapping_get(map, "if").and_then(value_as_condition_string)
}

fn with_value<'a>(step_map: &'a Mapping, key: &str) -> Option<&'a Value> {
    mapping_get(step_map, "with").and_then(Value::as_mapping).and_then(|m| mapping_get(m, key))
}

fn with_string(step_map: &Mapping, key: &str) -> Option<String> {
    with_value(step_map, key).and_then(value_as_condition_string)
}

fn multi_value_list(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => {
            s.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
        }
        Value::Sequence(seq) => seq.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

fn key_dimensions_for(step_map: &Mapping) -> KeyDimensions {
    let restore_keys =
        with_value(step_map, "restore-keys").map(multi_value_list).filter(|v| !v.is_empty());
    KeyDimensions {
        key: with_string(step_map, "key"),
        shared_key: with_string(step_map, "shared-key"),
        restore_keys,
    }
}

fn cached_paths_for(action: ActionKind, step_map: &Mapping) -> Vec<String> {
    if action == ActionKind::Swatinem {
        return Vec::new();
    }
    with_value(step_map, "path").map(multi_value_list).unwrap_or_default()
}

fn save_condition_for(
    action: ActionKind,
    step_map: &Mapping,
    step_condition: Option<&str>,
) -> Option<String> {
    match action {
        ActionKind::Swatinem => with_string(step_map, "save-if"),
        ActionKind::Cache | ActionKind::CacheSave => step_condition.map(str::to_string),
        ActionKind::CacheRestore => None,
    }
}

fn action_ref_of(uses: &str) -> String {
    uses.split_once('@').map(|(_, pin)| pin.to_string()).unwrap_or_default()
}

fn step_name_of(step_map: &Mapping, index: usize) -> String {
    mapping_get(step_map, "name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("#{index}"))
}

fn workflow_stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| name.to_string())
}

fn sorted_unique(values: &[String]) -> Vec<String> {
    let set: BTreeSet<String> = values.iter().cloned().collect();
    set.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Direct-in-workflow classification (the 57-row layer)
// ---------------------------------------------------------------------------

/// Classify one already-parsed workflow document's direct `uses:` cache
/// steps. `name` is the workflow file name (used for the id/`workflow`
/// fields and the stem).
pub fn classify_workflow(name: &str, doc: &Value) -> Vec<CacheFamilyRow> {
    let stem = workflow_stem(name);
    let trigger_list = triggers(doc);
    let trigger_event_set = sorted_unique(&trigger_list);
    let executed_subject = executed_subject_for(&trigger_event_set);
    // Only meaningful when the trigger set is candidate-adjacent solely
    // through `workflow_run`/`workflow_call` (never through a demonstrable
    // candidate-tree event); see `indirect_artifact_provenance`.
    let indirect_provenance = indirect_artifact_provenance(doc, &trigger_event_set);

    let Some(jobs) =
        doc.as_mapping().and_then(|root| mapping_get(root, "jobs")).and_then(Value::as_mapping)
    else {
        return Vec::new();
    };

    let mut rows = Vec::new();
    for (job_key, job_value) in jobs {
        let Some(job_name) = job_key.as_str() else {
            continue;
        };
        let Some(job_map) = job_value.as_mapping() else {
            continue;
        };
        let job_condition = condition_of(job_map);
        let job_statically_dead = is_statically_dead(job_condition.as_deref());
        // Inventory-specific: exclusion must hold for *every* candidate event
        // in this workflow's trigger set, not just `pull_request` (see
        // `condition_statically_excludes_candidate_events`).
        let job_statically_excluded = !job_statically_dead
            && condition_statically_excludes_candidate_events(
                job_condition.as_deref(),
                &trigger_event_set,
            );

        let Some(steps) = mapping_get(job_map, "steps").and_then(Value::as_sequence) else {
            continue;
        };
        let mut ordinal = 0usize;
        for step in steps {
            let Some(step_map) = step.as_mapping() else {
                continue;
            };
            let Some(uses) = mapping_get(step_map, "uses").and_then(Value::as_str) else {
                continue;
            };
            let Some(action) = ActionKind::from_uses(uses) else {
                continue;
            };

            let step_condition = condition_of(step_map);
            let step_statically_dead =
                job_statically_dead || is_statically_dead(step_condition.as_deref());
            let step_statically_excluded = !step_statically_dead
                && (job_statically_excluded
                    || condition_statically_excludes_candidate_events(
                        step_condition.as_deref(),
                        &trigger_event_set,
                    ));

            let reachability = if step_statically_dead {
                CandidateEventReachability::StaticallyDead
            } else if step_statically_excluded {
                CandidateEventReachability::StaticallyExcluded
            } else if !is_candidate_event(&trigger_event_set) {
                CandidateEventReachability::NotCandidateEvent
            } else {
                CandidateEventReachability::Reachable
            };

            let save_condition = save_condition_for(action, step_map, step_condition.as_deref());
            let save_authority_source = if step_statically_excluded && !step_statically_dead {
                SaveAuthoritySource::JobStaticExclusion
            } else {
                classify_save_authority(save_condition.as_deref())
            };

            let writer_disposition = resolve_writer_disposition(
                action.capability(),
                reachability,
                &trigger_event_set,
                save_authority_source,
            );
            let cached_byte_provenance = resolve_cached_byte_provenance(
                writer_disposition,
                &trigger_event_set,
                save_authority_source,
            );

            let cached_paths = cached_paths_for(action, step_map);
            let key_dimensions = key_dimensions_for(step_map);

            let repair_owner = if writer_disposition == WriterDisposition::CandidateWriterViolation
            {
                Some("needs-follow-up-issue".to_string())
            } else {
                None
            };
            let notes = if job_statically_dead {
                Some(
                    "job if: is a top-level && chain containing literal false; the job never runs for any event"
                        .to_string(),
                )
            } else if step_statically_dead {
                Some(
                    "step if: is a top-level && chain containing literal false; the step never runs for any event"
                        .to_string(),
                )
            } else if writer_disposition == WriterDisposition::CandidateWriterViolation {
                Some(
                    "no save guard on a candidate-reachable writer; recorded honestly, not repaired here (#9177)"
                        .to_string(),
                )
            } else if reachability == CandidateEventReachability::Reachable
                && indirect_provenance.is_some()
            {
                Some(
                    "reachable only via workflow_run/workflow_call; the executed subject is caller/artifact-resolved, not the candidate tree, so this is not_proven rather than a demonstrated candidate write"
                        .to_string(),
                )
            } else {
                None
            };
            let artifact_provenance = if reachability == CandidateEventReachability::Reachable {
                indirect_provenance.clone()
            } else {
                None
            };

            rows.push(CacheFamilyRow {
                id: format!("{stem}/{job_name}/{ordinal}"),
                workflow: name.to_string(),
                job: job_name.to_string(),
                step_name: step_name_of(step_map, ordinal),
                action: action.label().to_string(),
                action_ref: action_ref_of(uses),
                capability: action.capability(),
                trigger_event_set: trigger_event_set.clone(),
                candidate_event_reachability: reachability,
                executed_subject,
                job_condition: job_condition.clone(),
                step_condition,
                save_condition,
                save_authority_source,
                payload_class: payload_class_for(action, &cached_paths),
                cached_paths,
                key_authority: key_authority_for(reachability, &key_dimensions),
                key_dimensions,
                cached_byte_provenance,
                artifact_provenance,
                writer_disposition,
                repair_owner,
                notes,
            });
            ordinal += 1;
        }
    }
    rows
}

// ---------------------------------------------------------------------------
// Composite-action (dormant) classification
// ---------------------------------------------------------------------------

/// One composite action's own qualifying cache step, plus enough of the
/// step body to classify it once we know whether an active caller exists.
struct CompositeCacheStep<'a> {
    action_path: String,
    action: ActionKind,
    uses: String,
    step_map: &'a Mapping,
    ordinal: usize,
}

fn composite_cache_steps<'a>(action_dir_name: &str, doc: &'a Value) -> Vec<CompositeCacheStep<'a>> {
    let Some(steps) = doc
        .as_mapping()
        .and_then(|root| mapping_get(root, "runs"))
        .and_then(Value::as_mapping)
        .and_then(|runs| mapping_get(runs, "steps"))
        .and_then(Value::as_sequence)
    else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut ordinal = 0usize;
    for step in steps {
        let Some(step_map) = step.as_mapping() else {
            continue;
        };
        let Some(uses) = mapping_get(step_map, "uses").and_then(Value::as_str) else {
            continue;
        };
        let Some(action) = ActionKind::from_uses(uses) else {
            continue;
        };
        found.push(CompositeCacheStep {
            action_path: format!("{ACTIONS_DIR}/{action_dir_name}/action.yml"),
            action,
            uses: uses.to_string(),
            step_map,
            ordinal,
        });
        ordinal += 1;
    }
    found
}

/// A caller's `uses:` referencing a composite action directory, tolerant of
/// the local (`./.github/actions/<name>`) and remote
/// (`owner/repo/.github/actions/<name>@ref`) forms actually used in this
/// repository.
fn caller_uses_composite(uses: &str, action_dir_name: &str) -> bool {
    let needle = format!("actions/{action_dir_name}");
    uses.contains(&needle)
}

struct ActiveCaller {
    workflow: String,
    job: String,
    trigger_event_set: Vec<String>,
}

fn find_active_caller(
    action_dir_name: &str,
    workflow_docs: &[(String, Value)],
) -> Option<ActiveCaller> {
    for (name, doc) in workflow_docs {
        let trigger_event_set = sorted_unique(&triggers(doc));
        let Some(jobs) =
            doc.as_mapping().and_then(|root| mapping_get(root, "jobs")).and_then(Value::as_mapping)
        else {
            continue;
        };
        for (job_key, job_value) in jobs {
            let Some(job_name) = job_key.as_str() else {
                continue;
            };
            let Some(job_map) = job_value.as_mapping() else {
                continue;
            };
            let Some(steps) = mapping_get(job_map, "steps").and_then(Value::as_sequence) else {
                continue;
            };
            for step in steps {
                let Some(step_map) = step.as_mapping() else {
                    continue;
                };
                let Some(uses) = mapping_get(step_map, "uses").and_then(Value::as_str) else {
                    continue;
                };
                if caller_uses_composite(uses, action_dir_name) {
                    return Some(ActiveCaller {
                        workflow: name.clone(),
                        job: job_name.to_string(),
                        trigger_event_set: trigger_event_set.clone(),
                    });
                }
            }
        }
    }
    None
}

/// Classify one composite action's cache steps, split by whether the action
/// has an active caller: a called composite runs inline inside its caller,
/// so its rows are active-site (`families`) rows; only an unreferenced
/// composite is `dormant`. Returns `(active, dormant)`.
fn classify_composite_action(
    action_dir_name: &str,
    doc: &Value,
    workflow_docs: &[(String, Value)],
) -> (Vec<CacheFamilyRow>, Vec<CacheFamilyRow>) {
    let steps = composite_cache_steps(action_dir_name, doc);
    if steps.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let caller = find_active_caller(action_dir_name, workflow_docs);
    let mut active = Vec::new();
    let mut dormant = Vec::new();
    for step in steps {
        let row = build_composite_row(action_dir_name, &step, caller.as_ref());
        if caller.is_some() {
            active.push(row);
        } else {
            dormant.push(row);
        }
    }
    (active, dormant)
}

fn build_composite_row(
    action_dir_name: &str,
    step: &CompositeCacheStep<'_>,
    caller: Option<&ActiveCaller>,
) -> CacheFamilyRow {
    let action = step.action;
    let step_condition = condition_of(step.step_map);
    let cached_paths = cached_paths_for(action, step.step_map);
    let key_dimensions = key_dimensions_for(step.step_map);
    let save_condition = save_condition_for(action, step.step_map, step_condition.as_deref());

    let (trigger_event_set, reachability, notes) = match caller {
        None => (
            Vec::new(),
            CandidateEventReachability::StaticallyDead,
            format!(
                "unreferenced composite action ({action_dir_name}); zero active callers found under {WORKFLOWS_DIR}"
            ),
        ),
        Some(caller) => {
            let reachability = if !is_candidate_event(&caller.trigger_event_set) {
                CandidateEventReachability::NotCandidateEvent
            } else {
                CandidateEventReachability::Reachable
            };
            (
                caller.trigger_event_set.clone(),
                reachability,
                format!(
                    "reached via composite action {ACTIONS_DIR}/{action_dir_name} called by {}/{}",
                    caller.workflow, caller.job
                ),
            )
        }
    };

    let save_authority_source = classify_save_authority(save_condition.as_deref());
    let writer_disposition = resolve_writer_disposition(
        action.capability(),
        reachability,
        &trigger_event_set,
        save_authority_source,
    );
    let cached_byte_provenance = resolve_cached_byte_provenance(
        writer_disposition,
        &trigger_event_set,
        save_authority_source,
    );

    CacheFamilyRow {
        id: format!("{}/{}", step.action_path, step.ordinal),
        workflow: step.action_path.clone(),
        job: "composite-action".to_string(),
        step_name: step_name_of(step.step_map, step.ordinal),
        action: action.label().to_string(),
        action_ref: action_ref_of(&step.uses),
        capability: action.capability(),
        trigger_event_set: trigger_event_set.clone(),
        candidate_event_reachability: reachability,
        executed_subject: executed_subject_for(&trigger_event_set),
        job_condition: None,
        step_condition,
        save_condition,
        save_authority_source,
        payload_class: payload_class_for(action, &cached_paths),
        cached_paths,
        key_authority: key_authority_for(reachability, &key_dimensions),
        key_dimensions,
        cached_byte_provenance,
        artifact_provenance: None,
        writer_disposition,
        repair_owner: None,
        notes: Some(notes),
    }
}

// ---------------------------------------------------------------------------
// Directory-level derivation (fixture seam)
// ---------------------------------------------------------------------------

fn read_workflow_docs(dir: &Path) -> Result<Vec<(String, Value)>> {
    if !dir.is_dir() {
        bail!("workflow directory {} does not exist", dir.display());
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry.with_context(|| format!("reading entry in {}", dir.display()))?.path();
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if ext != "yml" && ext != "yaml" {
            continue;
        }
        paths.push(path);
    }
    paths.sort();
    if paths.is_empty() {
        bail!("workflow directory {} contains no workflow files", dir.display());
    }
    let mut docs = Vec::with_capacity(paths.len());
    for path in paths {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading workflow {}", path.display()))?;
        let doc: Value = serde_yaml_ng::from_str(&raw)
            .with_context(|| format!("parsing workflow YAML {}", path.display()))?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown.yml").to_string();
        docs.push((name, doc));
    }
    Ok(docs)
}

fn read_composite_actions(dir: &Path) -> Result<Vec<(String, Value)>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry.with_context(|| format!("reading entry in {}", dir.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !path.join("action.yml").is_file() && !path.join("action.yaml").is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        names.push(name.to_string());
    }
    names.sort();
    let mut docs = Vec::with_capacity(names.len());
    for name in names {
        let action_yml = dir.join(&name).join("action.yml");
        let action_yaml = dir.join(&name).join("action.yaml");
        let path = if action_yml.is_file() { action_yml } else { action_yaml };
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading composite action {}", path.display()))?;
        let doc: Value = serde_yaml_ng::from_str(&raw)
            .with_context(|| format!("parsing composite action YAML {}", path.display()))?;
        docs.push((name, doc));
    }
    Ok(docs)
}

/// Derive the active-cache-site inventory from an on-disk workflows dir
/// (required) and composite-actions dir (optional). Fixture seam for tests.
pub fn derive_families_in_dirs(
    workflows: &Path,
    actions: Option<&Path>,
) -> Result<DerivedInventory> {
    let workflow_docs = read_workflow_docs(workflows)?;

    let mut families = Vec::new();
    for (name, doc) in &workflow_docs {
        families.extend(classify_workflow(name, doc));
    }
    families.sort_by(|a, b| a.id.cmp(&b.id));

    let mut dormant = Vec::new();
    if let Some(actions_dir) = actions {
        let composite_docs = read_composite_actions(actions_dir)?;
        for (name, doc) in &composite_docs {
            let (active, uncalled) = classify_composite_action(name, doc, &workflow_docs);
            // A composite cache with an active caller IS active behavior:
            // composite steps run inline within their callers, so the row
            // belongs in the active denominator (`families`). `dormant` is
            // reserved for composites with no active caller, per
            // `docs/ci/cache-policy.md`.
            families.extend(active);
            dormant.extend(uncalled);
        }
    }
    dormant.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(DerivedInventory { families, dormant })
}

/// Derive the inventory from the real repository tree.
pub fn derive_repo_families() -> Result<DerivedInventory> {
    let root = project_root()?;
    let actions_dir = root.join(ACTIONS_DIR);
    let actions_dir = actions_dir.is_dir().then_some(actions_dir);
    derive_families_in_dirs(&root.join(WORKFLOWS_DIR), actions_dir.as_deref())
}

// ---------------------------------------------------------------------------
// Denominator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Denominator {
    pub active_total: usize,
    pub candidate_reachable: usize,
    pub candidate_reachable_writers: usize,
    pub direct_save_writers: usize,
    pub restore_only: usize,
    pub dormant: usize,
}

/// Every count here is derived from the rows at emit time; nothing here is a
/// hand-edited or hard-coded total.
pub fn compute_denominator(inventory: &DerivedInventory) -> Denominator {
    let active_total = inventory.families.len();
    let candidate_reachable = inventory
        .families
        .iter()
        .filter(|row| row.candidate_event_reachability == CandidateEventReachability::Reachable)
        .count();
    let candidate_reachable_writers = inventory
        .families
        .iter()
        .filter(|row| {
            row.candidate_event_reachability == CandidateEventReachability::Reachable
                && row.capability != Capability::RestoreOnly
        })
        .count();
    let direct_save_writers = inventory
        .families
        .iter()
        .filter(|row| {
            row.capability != Capability::RestoreOnly
                && (row.action == ActionKind::Cache.label()
                    || row.action == ActionKind::CacheSave.label())
        })
        .count();
    let restore_only =
        inventory.families.iter().filter(|row| row.capability == Capability::RestoreOnly).count();
    Denominator {
        active_total,
        candidate_reachable,
        candidate_reachable_writers,
        direct_save_writers,
        restore_only,
        dormant: inventory.dormant.len(),
    }
}

// ---------------------------------------------------------------------------
// Receipt
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptSubject {
    pub workflow_tree_digest: String,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
    pub run_id: Option<String>,
    pub run_attempt: Option<String>,
    pub workflow_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentStatus {
    Ok,
    Unavailable,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instrument {
    pub status: InstrumentStatus,
    pub parser: String,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreClass {
    Exact,
    Prefix,
    Miss,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveResult {
    Saved,
    Skipped,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkAvoided {
    NotProven,
    DependencyNetwork,
    Compilation,
    SetupCorpus,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheObservation {
    pub family_id: String,
    pub restore_class: RestoreClass,
    pub save_eligible: bool,
    pub save_result: SaveResult,
    pub save_authority_reason: String,
    pub bytes_restored: Option<u64>,
    pub bytes_saved: Option<u64>,
    pub restore_elapsed_ms: Option<u64>,
    pub save_elapsed_ms: Option<u64>,
    pub compiler_cache_stats: String,
    pub work_avoided: WorkAvoided,
    pub limitations: Vec<String>,
}

fn save_authority_reason_str(source: SaveAuthoritySource) -> &'static str {
    match source {
        SaveAuthoritySource::EventGuard => "event_guard",
        SaveAuthoritySource::RefGuard => "ref_guard",
        SaveAuthoritySource::EventAndRefGuard => "event_and_ref_guard",
        SaveAuthoritySource::JobStaticExclusion => "job_static_exclusion",
        SaveAuthoritySource::ContentSuccessOnly => "content_success_only",
        SaveAuthoritySource::Absent => "absent",
        SaveAuthoritySource::NotProven => "not_proven",
    }
}

/// `save_eligible` separates "this step's configuration permits a save from
/// this run" from "a save occurred" (`save_result`). A literal `save-if:
/// false` proves the former false even without live telemetry — the step
/// can never enter its save path — so such a row (and any restore-only or
/// statically dead site) must not advertise eligibility.
fn save_config_permits_save(row: &CacheFamilyRow) -> bool {
    row.capability != Capability::RestoreOnly
        && row.candidate_event_reachability != CandidateEventReachability::StaticallyDead
        && !row
            .save_condition
            .as_deref()
            .is_some_and(|raw| strip_expr_wrapper(raw).trim() == "false")
}

/// One observation per family, always defaulted `not_proven`/`unknown`/
/// `unavailable` — there is no live CI telemetry hook wired to this task, so
/// nothing here may auto-label a restore/save as work avoided (#9177 negative
/// control 9).
fn observation_for(row: &CacheFamilyRow) -> CacheObservation {
    CacheObservation {
        family_id: row.id.clone(),
        restore_class: RestoreClass::Unknown,
        save_eligible: save_config_permits_save(row),
        save_result: SaveResult::Unknown,
        save_authority_reason: save_authority_reason_str(row.save_authority_source).to_string(),
        bytes_restored: None,
        bytes_saved: None,
        restore_elapsed_ms: None,
        save_elapsed_ms: None,
        compiler_cache_stats: "unavailable".to_string(),
        work_avoided: WorkAvoided::NotProven,
        limitations: vec![
            "no live CI restore/save telemetry hook; static source derivation only".to_string(),
        ],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiCacheReceipt {
    pub schema_version: &'static str,
    pub claim_boundary: &'static str,
    pub subject: ReceiptSubject,
    pub instrument: Instrument,
    pub denominator: Denominator,
    pub families: Vec<CacheFamilyRow>,
    pub observations: Vec<CacheObservation>,
    pub ok: bool,
    pub errors: Vec<String>,
}

/// Build the receipt for a successfully-derived inventory.
pub fn build_receipt(inventory: &DerivedInventory, workflow_tree_digest: String) -> CiCacheReceipt {
    let denominator = compute_denominator(inventory);
    let observations = inventory.families.iter().map(observation_for).collect();
    let instrument = Instrument {
        status: InstrumentStatus::Ok,
        parser: "serde_yaml_ng".to_string(),
        limitations: vec![
            "static source derivation only; no live restore/save telemetry".to_string(),
        ],
    };
    CiCacheReceipt {
        schema_version: SCHEMA,
        claim_boundary: CLAIM_BOUNDARY,
        subject: ReceiptSubject {
            workflow_tree_digest,
            base_sha: None,
            head_sha: std::env::var("GITHUB_SHA")
                .ok()
                .filter(|sha| sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())),
            run_id: std::env::var("GITHUB_RUN_ID").ok(),
            run_attempt: std::env::var("GITHUB_RUN_ATTEMPT").ok(),
            workflow_ref: std::env::var("GITHUB_REF").ok(),
        },
        instrument,
        denominator,
        families: inventory.families.clone(),
        observations,
        ok: true,
        errors: Vec::new(),
    }
}

/// Build the receipt shape for a failed/unavailable instrument (parser
/// failure, missing workflows dir, …). Per the build spec, this must never
/// be represented as `status: ok` with an empty `families` array read as "no
/// caches".
pub fn build_failed_receipt(reason: &str) -> CiCacheReceipt {
    CiCacheReceipt {
        schema_version: SCHEMA,
        claim_boundary: CLAIM_BOUNDARY,
        subject: ReceiptSubject {
            workflow_tree_digest: "0".repeat(64),
            base_sha: None,
            head_sha: None,
            run_id: None,
            run_attempt: None,
            workflow_ref: None,
        },
        instrument: Instrument {
            status: InstrumentStatus::Failed,
            parser: "serde_yaml_ng".to_string(),
            limitations: vec![reason.to_string()],
        },
        denominator: Denominator::default(),
        families: Vec::new(),
        observations: Vec::new(),
        ok: false,
        errors: vec![reason.to_string()],
    }
}

// ---------------------------------------------------------------------------
// Digest
// ---------------------------------------------------------------------------

/// Content digest of the workflows dir (and composite-actions dir, when
/// given): a sha256 over the sorted `(relative_path, content)` pairs. Used
/// as `subject.workflow_tree_digest` so a receipt can be checked for
/// staleness against the tree it claims to describe.
pub fn workflow_tree_digest_for(workflows: &Path, actions: Option<&Path>) -> Result<String> {
    use sha2::{Digest, Sha256};

    let mut entries: Vec<(String, String)> = Vec::new();
    for (name, _) in read_workflow_docs(workflows)? {
        let raw = fs::read_to_string(workflows.join(&name))
            .with_context(|| format!("reading {} for digest", name))?;
        entries.push((format!("{WORKFLOWS_DIR}/{name}"), raw));
    }
    if let Some(actions_dir) = actions {
        for (name, _) in read_composite_actions(actions_dir)? {
            let action_yml = actions_dir.join(&name).join("action.yml");
            let action_yaml = actions_dir.join(&name).join("action.yaml");
            let path = if action_yml.is_file() { action_yml } else { action_yaml };
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("reading {} for digest", path.display()))?;
            entries.push((format!("{ACTIONS_DIR}/{name}/action.yml"), raw));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (path, content) in entries {
        hasher.update(path.as_bytes());
        hasher.update([0u8]);
        hasher.update(content.as_bytes());
        hasher.update([0u8]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

// ---------------------------------------------------------------------------
// Diff (drift gate)
// ---------------------------------------------------------------------------

fn row_id(value: &JsonValue) -> Option<String> {
    value.get("id").and_then(JsonValue::as_str).map(str::to_string)
}

/// Compare derived rows against one checked-in array (`families` or
/// `dormant`), by row `id`. Reports a missing row, a stale row for a site
/// that no longer exists, and any field-level drift on a matching row.
pub fn diff_inventory(derived: &[CacheFamilyRow], checked_in: &JsonValue) -> Vec<String> {
    let mut diffs = Vec::new();
    let empty: Vec<JsonValue> = Vec::new();
    let checked_in_rows: &Vec<JsonValue> = checked_in.as_array().unwrap_or(&empty);

    let mut checked_in_by_id: std::collections::BTreeMap<String, &JsonValue> =
        std::collections::BTreeMap::new();
    for row in checked_in_rows {
        if let Some(id) = row_id(row) {
            checked_in_by_id.insert(id, row);
        }
    }

    let mut derived_ids: BTreeSet<String> = BTreeSet::new();
    for row in derived {
        derived_ids.insert(row.id.clone());
        let derived_value = match serde_json::to_value(row) {
            Ok(value) => value,
            Err(error) => {
                diffs.push(format!("{}: failed to serialize derived row: {error}", row.id));
                continue;
            }
        };
        match checked_in_by_id.get(&row.id) {
            None => diffs.push(format!("missing manifest row for active cache site: {}", row.id)),
            Some(existing) => {
                let Some(existing_obj) = existing.as_object() else {
                    diffs.push(format!("{}: checked-in row is not an object", row.id));
                    continue;
                };
                let Some(derived_obj) = derived_value.as_object() else {
                    continue;
                };
                for (field, derived_field_value) in derived_obj {
                    let checked_field_value =
                        existing_obj.get(field).cloned().unwrap_or(JsonValue::Null);
                    if *derived_field_value != checked_field_value {
                        diffs.push(format!(
                            "{}: field `{field}` drifted (derived={derived_field_value}, manifest={checked_field_value})",
                            row.id
                        ));
                    }
                }
                // The manifest is claimed to *equal* the derived inventory,
                // not merely contain it: a field the deriver no longer emits
                // (obsolete or fabricated in the checked-in row) must also
                // fail the drift gate, or regeneration would silently drop
                // it while `--check` stayed green.
                for field in existing_obj.keys() {
                    if !derived_obj.contains_key(field) {
                        diffs.push(format!(
                            "{}: manifest field `{field}` is not part of the derived row (obsolete or fabricated; regenerate the manifest)",
                            row.id
                        ));
                    }
                }
            }
        }
    }

    for id in checked_in_by_id.keys() {
        if !derived_ids.contains(id) {
            diffs.push(format!("manifest row for a cache site that no longer exists: {id}"));
        }
    }

    diffs.sort();
    diffs
}

// ---------------------------------------------------------------------------
// Manifest I/O
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheInventoryManifest {
    schema_version: &'static str,
    claim_boundary: &'static str,
    owner_issue: &'static str,
    policy: &'static str,
    consumers: Vec<&'static str>,
    families: Vec<CacheFamilyRow>,
    dormant: Vec<CacheFamilyRow>,
}

fn manifest_document(inventory: &DerivedInventory) -> CacheInventoryManifest {
    CacheInventoryManifest {
        schema_version: INVENTORY_SCHEMA,
        claim_boundary: CLAIM_BOUNDARY,
        owner_issue: OWNER_ISSUE,
        policy: POLICY_DOC,
        consumers: vec!["#13927", "#9178"],
        families: inventory.families.clone(),
        dormant: inventory.dormant.clone(),
    }
}

fn load_manifest(path: &Path) -> Result<JsonValue> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("reading manifest {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing manifest JSON {}", path.display()))
}

/// Self-check the emitted receipt against its own schema before writing it
/// anywhere: without this, `schemas/ci_cache_receipt.v1.schema.json`'s
/// `additionalProperties: false` and `required` lists are documentation
/// only, because the Rust structs never fail to serialize on a shape drift.
fn validate_receipt_against_schema(root: &Path, receipt: &CiCacheReceipt) -> Result<()> {
    let schema_raw = fs::read_to_string(root.join(SCHEMA_PATH))
        .with_context(|| format!("reading {SCHEMA_PATH}"))?;
    let schema: JsonValue = serde_json::from_str(&schema_raw)
        .with_context(|| format!("parsing {SCHEMA_PATH} as JSON"))?;
    let validator =
        jsonschema::validator_for(&schema).with_context(|| format!("compiling {SCHEMA_PATH}"))?;
    let receipt_value =
        serde_json::to_value(receipt).with_context(|| "serialize receipt for schema check")?;
    let errors: Vec<String> = validator
        .iter_errors(&receipt_value)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    if !errors.is_empty() {
        bail!("emitted receipt violates {SCHEMA_PATH}:\n{}", errors.join("\n"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CLI entry point
// ---------------------------------------------------------------------------

/// Derive the repository's cache inventory, and either:
/// - `--check`: diff the derived truth against the checked-in manifest and
///   fail on any drift;
/// - write the receipt to `--receipt <path>` (or stdout by default), and
///   (re)write the manifest to `--manifest <path>` (defaults to
///   [`MANIFEST`]) when not checking.
///
/// `api_version` (CLI: `--api-version`, default `v1`) pins the schema
/// version the caller expects; anything other than [`SUPPORTED_API_VERSION`]
/// fails loudly instead of emitting an unparseable shape.
pub fn run(
    check: bool,
    receipt: Option<PathBuf>,
    manifest: Option<PathBuf>,
    api_version: &str,
) -> Result<()> {
    ensure_supported_api_version(api_version)?;
    let root = project_root()?;
    let manifest_path = manifest.unwrap_or_else(|| root.join(MANIFEST));

    let inventory = match derive_repo_families() {
        Ok(inventory) => inventory,
        Err(error) => {
            // An instrument failure must stay representable, not silently
            // read as "no caches": write the failed-instrument receipt shape
            // before still failing the command. Publication is best-effort
            // (the command is already failing), but a suppressed write
            // failure must not be silent — a consumer waiting on the
            // artifact deserves a diagnostic.
            if let Some(path) = &receipt {
                let failed =
                    build_failed_receipt(&format!("deriving the CI cache inventory: {error}"));
                match serde_json::to_string_pretty(&failed) {
                    Ok(failed_json) => {
                        if let Some(parent) = path.parent() {
                            if let Err(create_error) = fs::create_dir_all(parent) {
                                eprintln!(
                                    "warning: could not create {} for the ci-cache failure receipt: {create_error}",
                                    parent.display()
                                );
                            }
                        }
                        if let Err(write_error) = fs::write(path, failed_json) {
                            eprintln!(
                                "warning: failed to publish the ci-cache failure receipt to {}: {write_error}",
                                path.display()
                            );
                        }
                    }
                    Err(serialize_error) => {
                        eprintln!(
                            "warning: failed to serialize the ci-cache failure receipt: {serialize_error}"
                        );
                    }
                }
            }
            return Err(error).with_context(|| "deriving the CI cache inventory");
        }
    };
    let actions_dir = root.join(ACTIONS_DIR);
    let actions_dir = actions_dir.is_dir().then_some(actions_dir);
    let workflow_tree_digest =
        workflow_tree_digest_for(&root.join(WORKFLOWS_DIR), actions_dir.as_deref())?;

    if check {
        let checked_in = load_manifest(&manifest_path)?;
        let mut diffs = diff_inventory(
            &inventory.families,
            checked_in.get("families").unwrap_or(&JsonValue::Null),
        );
        diffs.extend(diff_inventory(
            &inventory.dormant,
            checked_in.get("dormant").unwrap_or(&JsonValue::Null),
        ));
        if !diffs.is_empty() {
            bail!(
                "ci-cache-inventory --check found {} drift(s) against {}:\n{}",
                diffs.len(),
                manifest_path.display(),
                diffs.join("\n")
            );
        }
        println!(
            "ci-cache-inventory --check: {} active site(s), {} dormant site(s), no drift against {}",
            inventory.families.len(),
            inventory.dormant.len(),
            manifest_path.display()
        );
        return Ok(());
    }

    let manifest_document = manifest_document(&inventory);
    let manifest_json = serde_json::to_string_pretty(&manifest_document)
        .with_context(|| "serialize ci-cache-inventory manifest")?;
    if let Some(parent) = manifest_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&manifest_path, format!("{manifest_json}\n"))
        .with_context(|| format!("writing {}", manifest_path.display()))?;

    let cache_receipt = build_receipt(&inventory, workflow_tree_digest);
    validate_receipt_against_schema(&root, &cache_receipt)?;
    let receipt_json = serde_json::to_string_pretty(&cache_receipt)
        .with_context(|| "serialize ci-cache-inventory receipt")?;
    match receipt {
        Some(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            fs::write(&path, &receipt_json)
                .with_context(|| format!("writing {}", path.display()))?;
            println!(
                "ci-cache-inventory: {} active site(s), {} dormant site(s) -> {}",
                inventory.families.len(),
                inventory.dormant.len(),
                path.display()
            );
        }
        None => {
            let mut stdout = io::stdout().lock();
            stdout
                .write_all(receipt_json.as_bytes())
                .and_then(|()| stdout.write_all(b"\n"))
                .with_context(|| "write ci-cache-inventory receipt to stdout")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Active CI cache inventory contract (#9177).
    //!
    //! Proves the checked-in `.ci/ci-cache/cache-inventory.v1.json` matches
    //! the derived truth of the real workflow/action tree (the drift gate),
    //! that the emitted receipt validates against
    //! `schemas/ci_cache_receipt.v1.schema.json`, several named real-source
    //! classifications, and the build spec's twelve numbered negative
    //! controls — each asserted for the *specific* reason it must fail, not
    //! merely that it fails.
    //!
    //! This is a static YAML oracle. It does not prove runtime restore/save
    //! provenance, GitHub expression evaluation, or live eviction — those
    //! stay `not_proven` in the receipt itself.
    //!
    //! Run with `cargo test -p xtask --bin xtask tasks::ci_cache_inventory
    //! --locked`. This lives as an inline `#[cfg(test)] mod tests` — house
    //! style for `xtask/src/tasks/*.rs` (157 of the ~200 modules there carry
    //! one) — rather than a separate `xtask/tests/*.rs` integration test, so
    //! it needs no parallel lib-crate compilation of this file or its
    //! `workflow_policy_lint` reachability primitives.

    use super::*;
    use serde_json::json;

    fn project_root() -> PathBuf {
        crate::utils::project_root().expect("resolve project root")
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir fixture parent");
        }
        fs::write(path, content).expect("write fixture file");
    }

    fn fixture_workflows_dir(
        tmp: &tempfile::TempDir,
        workflow_name: &str,
        content: &str,
    ) -> PathBuf {
        let workflows = tmp.path().join(".github/workflows");
        write_file(&workflows.join(workflow_name), content);
        workflows
    }

    fn find_row<'a>(rows: &'a [CacheFamilyRow], workflow: &str, job: &str) -> &'a CacheFamilyRow {
        rows.iter()
            .find(|row| row.workflow == workflow && row.job == job)
            .unwrap_or_else(|| panic!("no row for {workflow}/{job} in {rows:#?}"))
    }

    #[test]
    fn schema_ids_are_versioned() {
        assert_eq!(SCHEMA, "ci_cache_receipt.v1");
        assert_eq!(INVENTORY_SCHEMA, "ci_cache_inventory.v1");
    }

    #[test]
    fn denominator_has_no_hard_coded_totals() {
        // The denominator is a pure function of the rows: an empty inventory
        // must produce an all-zero denominator, never a fixed literal.
        let empty = DerivedInventory::default();
        let denominator = compute_denominator(&empty);
        assert_eq!(denominator, Denominator::default());
    }

    #[test]
    fn action_kind_prefers_the_most_specific_match() {
        assert!(matches!(
            ActionKind::from_uses("actions/cache/restore@abc"),
            Some(ActionKind::CacheRestore)
        ));
        assert!(matches!(
            ActionKind::from_uses("actions/cache/save@abc"),
            Some(ActionKind::CacheSave)
        ));
        assert!(matches!(ActionKind::from_uses("actions/cache@abc"), Some(ActionKind::Cache)));
        assert!(matches!(
            ActionKind::from_uses("Swatinem/rust-cache@abc"),
            Some(ActionKind::Swatinem)
        ));
        assert!(ActionKind::from_uses("mozilla-actions/sccache-action@abc").is_none());
        assert!(ActionKind::from_uses("actions/checkout@abc").is_none());
    }

    // -----------------------------------------------------------------------
    // Positive controls
    // -----------------------------------------------------------------------

    #[test]
    fn checked_in_manifest_matches_derived_truth() {
        let root = project_root();
        let workflows = root.join(".github/workflows");
        let actions = root.join(".github/actions");
        let inventory = derive_families_in_dirs(&workflows, Some(&actions))
            .expect("derive real repo inventory");

        let manifest_raw = fs::read_to_string(root.join(".ci/ci-cache/cache-inventory.v1.json"))
            .expect("read checked-in cache inventory manifest");
        let manifest: JsonValue = serde_json::from_str(&manifest_raw).expect("parse manifest JSON");

        let mut diffs = diff_inventory(
            &inventory.families,
            manifest.get("families").unwrap_or(&JsonValue::Null),
        );
        diffs.extend(diff_inventory(
            &inventory.dormant,
            manifest.get("dormant").unwrap_or(&JsonValue::Null),
        ));
        assert!(
            diffs.is_empty(),
            "checked-in cache inventory drifted from derived truth; run `cargo xtask ci-cache-inventory` to \
             regenerate:\n{}",
            diffs.join("\n")
        );
    }

    #[test]
    fn receipt_validates_against_schema() {
        let root = project_root();
        let workflows = root.join(".github/workflows");
        let actions = root.join(".github/actions");
        let inventory =
            derive_families_in_dirs(&workflows, Some(&actions)).expect("derive inventory");
        let digest = workflow_tree_digest_for(&workflows, Some(&actions))
            .expect("compute workflow tree digest");
        let receipt = build_receipt(&inventory, digest);
        let receipt_json = serde_json::to_value(&receipt).expect("serialize receipt");

        let schema_raw = fs::read_to_string(root.join("schemas/ci_cache_receipt.v1.schema.json"))
            .expect("read ci_cache_receipt schema");
        let schema: JsonValue = serde_json::from_str(&schema_raw).expect("parse schema JSON");
        let validator =
            jsonschema::validator_for(&schema).expect("compile ci_cache_receipt schema");
        let errors: Vec<String> =
            validator.iter_errors(&receipt_json).map(|error| error.to_string()).collect();
        assert!(errors.is_empty(), "receipt violates its own schema: {errors:?}");
    }

    #[test]
    fn failed_instrument_receipt_also_validates_against_schema() {
        let root = project_root();
        let receipt = build_failed_receipt("fixture: parser failure");
        let receipt_json = serde_json::to_value(&receipt).expect("serialize failed receipt");

        let schema_raw = fs::read_to_string(root.join("schemas/ci_cache_receipt.v1.schema.json"))
            .expect("read ci_cache_receipt schema");
        let schema: JsonValue = serde_json::from_str(&schema_raw).expect("parse schema JSON");
        let validator =
            jsonschema::validator_for(&schema).expect("compile ci_cache_receipt schema");
        let errors: Vec<String> =
            validator.iter_errors(&receipt_json).map(|error| error.to_string()).collect();
        assert!(errors.is_empty(), "failed-instrument receipt violates its own schema: {errors:?}");
    }

    #[test]
    fn ci_nightly_test_coverage_and_fuzz_are_statically_excluded_not_violations() {
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        for job in ["test-coverage", "fuzz"] {
            let row = find_row(&inventory.families, "ci-nightly.yml", job);
            assert_eq!(
                row.candidate_event_reachability,
                CandidateEventReachability::StaticallyExcluded,
                "ci-nightly.yml `{job}` must stay statically excluded: {row:#?}"
            );
            assert_ne!(
                row.writer_disposition,
                WriterDisposition::CandidateWriterViolation,
                "ci-nightly.yml `{job}` must never read as a candidate writer violation: {row:#?}"
            );
        }
    }

    #[test]
    fn ripr_hosted_and_fallback_save_guard_is_reviewed_exception() {
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        for job in ["ripr-github", "ripr-fallback"] {
            let row = find_row(&inventory.families, "ripr.yml", job);
            assert_eq!(
                row.writer_disposition,
                WriterDisposition::ReviewedException,
                "ripr.yml `{job}` save guard must be a reviewed exception, not a violation: {row:#?}"
            );
        }
    }

    #[test]
    fn post_merge_status_is_not_candidate_event() {
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        let row = find_row(&inventory.families, "post-merge-status.yml", "generate");
        assert_eq!(row.candidate_event_reachability, CandidateEventReachability::NotCandidateEvent);
    }

    #[test]
    fn post_merge_status_disposition_is_not_candidate_reachable_not_trusted_guarded() {
        // A trusted *event set* is not a save *guard*: `push`/`schedule`/
        // `workflow_dispatch`-only rows have no candidate event to guard
        // against, so they must not read as `trusted_guarded` (which claims
        // an actual guard was checked and holds) even when their own
        // `save_authority_source` is `absent`.
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        let row = find_row(&inventory.families, "post-merge-status.yml", "generate");
        assert_eq!(row.writer_disposition, WriterDisposition::NotCandidateReachable);
        assert_ne!(row.writer_disposition, WriterDisposition::TrustedGuarded);
    }

    #[test]
    fn em_ci_routed_rust_ref_only_guard_is_trusted_guarded() {
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        for job in ["rust-small-github", "rust-small-fallback"] {
            let row = find_row(&inventory.families, "em-ci-routed-rust.yml", job);
            assert_eq!(
                row.writer_disposition,
                WriterDisposition::TrustedGuarded,
                "em-ci-routed-rust.yml `{job}` ref-only guard (no pull_request_target) must be trusted_guarded: {row:#?}"
            );
            assert_eq!(row.save_authority_source, SaveAuthoritySource::RefGuard);
        }
    }

    #[test]
    fn docs_pr_build_unguarded_cache_is_recorded_as_a_violation() {
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        let row = find_row(&inventory.families, "docs-pr-build.yml", "build");
        assert_eq!(
            row.writer_disposition,
            WriterDisposition::CandidateWriterViolation,
            "docs-pr-build.yml's unguarded actions/cache must be recorded honestly: {row:#?}"
        );
        assert_eq!(row.save_authority_source, SaveAuthoritySource::Absent);
    }

    #[test]
    fn vscode_current_source_linux_smoke_unguarded_cache_is_a_true_violation() {
        // pull_request-reachable (branches+paths) + workflow_dispatch, no
        // job/step `if:`, Swatinem with no `save-if`, checking out
        // `${{ env.PERL_LSP_SMOKE_SUBJECT_SHA }}` — candidate content on a
        // PR. Same class as docs-pr-build: pull_request demonstrably makes
        // the candidate tree the executed subject, so an unguarded writer
        // here is a real, not a crying-wolf, violation.
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        let row = find_row(
            &inventory.families,
            "vscode-current-source-linux-smoke.yml",
            "current-source-linux-smoke",
        );
        assert_eq!(
            row.writer_disposition,
            WriterDisposition::CandidateWriterViolation,
            "vscode-current-source-linux-smoke.yml's unguarded Swatinem cache on a pull_request-reachable job must be a true violation: {row:#?}"
        );
        assert_eq!(row.save_authority_source, SaveAuthoritySource::Absent);
    }

    #[test]
    fn post_publish_smoke_unguarded_cache_is_not_proven_not_a_violation() {
        // Trigger set is `workflow_dispatch` + `workflow_run` (narrowed to
        // `workflows: ["Publish to crates.io"]`, `types: [completed]`) —
        // no `pull_request`-family event at all. The job checks out
        // `${{ needs.resolve-version.outputs.subject }}`, resolved from a
        // publication-receipt artifact the publish run writes only after
        // confirming the crates are live: the executed subject is a
        // published release commit, not candidate content. `workflow_run`
        // does not statically establish the candidate tree as the executed
        // subject, so an unguarded writer reachable only through it is
        // `not_proven` — not a demonstrated candidate write, and not
        // `trusted_guarded` either, since it genuinely has no save guard.
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        let row = find_row(&inventory.families, "post-publish-smoke.yml", "smoke");
        assert_eq!(row.candidate_event_reachability, CandidateEventReachability::Reachable);
        assert_eq!(row.save_authority_source, SaveAuthoritySource::Absent);
        assert_ne!(
            row.writer_disposition,
            WriterDisposition::CandidateWriterViolation,
            "post-publish-smoke.yml is reachable only via workflow_run, whose executed subject is \
             artifact-resolved, not the candidate tree — it must not read as a demonstrated violation: {row:#?}"
        );
        assert_eq!(row.writer_disposition, WriterDisposition::NotProven);
        assert!(
            row.artifact_provenance.as_deref().is_some_and(|p| p.contains("Publish to crates.io")),
            "the statically-knowable workflow_run producer must be named: {row:#?}"
        );
    }

    // -----------------------------------------------------------------------
    // Negative controls (build spec, numbered 1-12)
    // -----------------------------------------------------------------------

    const SAMPLE_CACHE_WORKFLOW: &str = r#"
on:
  pull_request:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
        with:
          shared-key: sample-${{ hashFiles('Cargo.lock') }}
          save-if: ${{ github.ref == 'refs/heads/main' }}
"#;

    /// 1. An active cache action with no manifest row must be reported as drift.
    #[test]
    fn negative_control_1_active_site_missing_from_manifest_is_drift() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "sample.yml", SAMPLE_CACHE_WORKFLOW);
        let inventory =
            derive_families_in_dirs(&workflows, None).expect("derive fixture inventory");
        assert_eq!(inventory.families.len(), 1);

        let diffs = diff_inventory(&inventory.families, &json!([]));
        assert!(
            diffs.iter().any(|diff| diff.contains("missing manifest row for active cache site")),
            "expected a missing-row drift error, got: {diffs:?}"
        );
    }

    const SETUP_RUST_WITH_SCCACHE: &str = r#"
name: 'Setup Rust'
inputs:
  sccache:
    default: 'false'
runs:
  using: 'composite'
  steps:
    - name: Cache cargo dependencies
      uses: Swatinem/rust-cache@c19371144df3bb44fab255c43d04cbc2ab54d1c4
      with:
        shared-key: 'perl-lsp'
    - name: Install sccache
      if: inputs.sccache == 'true'
      uses: mozilla-actions/sccache-action@2e7f9ec7921547d4b46598398ca573513895d0bd
"#;

    const CALLER_USES_SETUP_RUST: &str = r#"
on:
  push:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: ./.github/actions/setup-rust
        with:
          cache: true
"#;

    /// 2. The optional `sccache` step inside `setup-rust` must never be counted
    /// as an active (or dormant) cache site, even though the composite action
    /// that embeds it has an active caller. The composite's Swatinem step
    /// itself has an active caller, so it is an active-site row (`families`),
    /// not dormant.
    #[test]
    fn negative_control_2_dormant_optional_sccache_is_never_a_cache_site() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "caller.yml", CALLER_USES_SETUP_RUST);
        let actions_dir = tmp.path().join(".github/actions");
        write_file(&actions_dir.join("setup-rust/action.yml"), SETUP_RUST_WITH_SCCACHE);

        let inventory =
            derive_families_in_dirs(&workflows, Some(&actions_dir)).expect("derive inventory");
        let all_actions: Vec<&str> = inventory
            .families
            .iter()
            .chain(inventory.dormant.iter())
            .map(|row| row.action.as_str())
            .collect();
        assert!(
            !all_actions.iter().any(|action| action.contains("sccache")),
            "sccache must never be classified as a cache site: {all_actions:?}"
        );
        assert_eq!(
            inventory.families.len(),
            1,
            "exactly the Swatinem step must be classified, as an active site: {:#?}",
            inventory.families
        );
        assert_eq!(
            inventory.dormant.len(),
            0,
            "a called composite's cache row must not land in dormant: {:#?}",
            inventory.dormant
        );
    }

    const RUN_UNIQUE_CHECKPOINT_WORKFLOW: &str = r#"
on:
  push:
    branches: [main]
jobs:
  corpus:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9
        with:
          path: |
            target/corpus
            target/corpus-ckpt-${{ github.run_id }}
          key: corpus-${{ github.run_id }}
"#;

    /// 3. A run-unique checkpoint output path must not be silently dropped from
    /// a row: a manifest that only lists the stable path is drift.
    #[test]
    fn negative_control_3_run_unique_output_path_omission_is_drift() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "corpus.yml", RUN_UNIQUE_CHECKPOINT_WORKFLOW);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let row = &inventory.families[0];
        assert_eq!(row.cached_paths.len(), 2, "fixture must expose both paths: {row:#?}");

        let mut truncated = serde_json::to_value(row).expect("serialize row");
        truncated["cached_paths"] = json!(["target/corpus"]);
        let diffs = diff_inventory(&inventory.families, &json!([truncated]));
        assert!(
            diffs.iter().any(|diff| diff.contains("field `cached_paths` drifted")),
            "expected cached_paths drift, got: {diffs:?}"
        );
    }

    /// 4. A direct `actions/cache/save` writer omitted from the manifest must
    /// drift, and it must be visible in `direct_save_writers`, not silently
    /// folded into the Swatinem-implicit count.
    #[test]
    fn negative_control_4_direct_save_writer_omission_is_drift_and_counted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "corpus.yml", RUN_UNIQUE_CHECKPOINT_WORKFLOW);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let denominator = compute_denominator(&inventory);
        assert_eq!(
            denominator.direct_save_writers, 1,
            "direct actions/cache/save must be counted: {denominator:?}"
        );

        let diffs = diff_inventory(&inventory.families, &json!([]));
        assert!(
            diffs.iter().any(|diff| diff.contains("missing manifest row for active cache site"))
        );
    }

    const PULL_REQUEST_TARGET_WORKFLOW: &str = r#"
on:
  pull_request_target:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
        with:
          shared-key: pr-target-${{ hashFiles('Cargo.lock') }}
"#;

    /// 5. Losing `pull_request_target` reachability (a manifest row claiming
    /// `not_candidate_event` for a `pull_request_target`-triggered site) must
    /// drift.
    #[test]
    fn negative_control_5_pull_request_target_reachability_loss_is_drift() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "target.yml", PULL_REQUEST_TARGET_WORKFLOW);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let row = &inventory.families[0];
        assert_eq!(row.candidate_event_reachability, CandidateEventReachability::Reachable);

        let mut understated = serde_json::to_value(row).expect("serialize row");
        understated["candidate_event_reachability"] = json!("not_candidate_event");
        let diffs = diff_inventory(&inventory.families, &json!([understated]));
        assert!(
            diffs.iter().any(|diff| diff.contains("field `candidate_event_reachability` drifted")),
            "expected reachability drift, got: {diffs:?}"
        );
    }

    /// 6. A canonical-looking ref-only guard under `pull_request_target` must
    /// classify as `not_proven`, never `trusted_guarded` — `github.ref` reports
    /// the *base* ref under `pull_request_target`, not proof of which tree ran.
    #[test]
    fn negative_control_6_pull_request_target_ref_only_guard_is_not_proven() {
        let workflow = r#"
on:
  pull_request_target:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
        with:
          shared-key: pr-target-ref-${{ hashFiles('Cargo.lock') }}
          save-if: ${{ github.ref == 'refs/heads/main' }}
"#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "target.yml", workflow);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let row = &inventory.families[0];
        assert_eq!(row.save_authority_source, SaveAuthoritySource::RefGuard);
        assert_ne!(
            row.writer_disposition,
            WriterDisposition::TrustedGuarded,
            "ref-only guard under pull_request_target must never be trusted_guarded: {row:#?}"
        );
        assert_eq!(row.writer_disposition, WriterDisposition::NotProven);
    }

    /// 6b. A *negated* ref guard (`!=`) saves on every branch except main —
    /// the opposite of a guard — and must never classify as `trusted_guarded`
    /// merely because the condition mentions `github.ref`.
    #[test]
    fn negative_control_6b_negated_ref_guard_is_never_trusted_guarded() {
        let workflow = r#"
on:
  pull_request:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
        with:
          shared-key: negated-${{ hashFiles('Cargo.lock') }}
          save-if: ${{ github.ref != 'refs/heads/main' }}
"#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "negated.yml", workflow);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let row = &inventory.families[0];
        assert_ne!(
            row.save_authority_source,
            SaveAuthoritySource::RefGuard,
            "a negated ref comparison saves on everything except main, the opposite of a guard, \
             and must not be recognized as one: {row:#?}"
        );
        assert_ne!(
            row.writer_disposition,
            WriterDisposition::TrustedGuarded,
            "a negated ref guard must never read as trusted_guarded: {row:#?}"
        );
    }

    /// Positive counterpart to 6b: hardening the ref-guard test must not
    /// downgrade the `format()` spelling of a canonical default-branch
    /// equality. `pr-candidate-set.yml` uses exactly this shape, and it is a
    /// stronger guard than the `main`/`master` literals because it follows the
    /// repository's real default branch — recognizing one spelling and not the
    /// other would understate its authority.
    #[test]
    fn format_spelled_default_branch_ref_equality_is_a_ref_guard() {
        let workflow = r#"
on:
  pull_request:
    branches: [main]
  schedule:
    - cron: '0 3 * * *'
  workflow_dispatch: {}
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
        with:
          shared-key: formatted-${{ hashFiles('Cargo.lock') }}
          save-if: ${{ (github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) }}
"#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "formatted.yml", workflow);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let row = &inventory.families[0];
        assert_eq!(
            row.save_authority_source,
            SaveAuthoritySource::EventAndRefGuard,
            "an event guard combined with a format()-spelled default-branch ref equality \
             carries both dimensions: {row:#?}"
        );
        assert_eq!(
            row.writer_disposition,
            WriterDisposition::TrustedGuarded,
            "a guard with both a PR-false event anchor and a canonical ref equality is \
             trusted_guarded: {row:#?}"
        );
    }

    /// The real `pr-candidate-set.yml` row is the production instance of the
    /// shape above; pin it so a future ref-guard change cannot silently
    /// downgrade it.
    #[test]
    fn pr_candidate_set_format_guard_is_trusted_guarded() {
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        let row = find_row(&inventory.families, "pr-candidate-set.yml", "reconcile");
        assert_eq!(
            row.save_authority_source,
            SaveAuthoritySource::EventAndRefGuard,
            "pr-candidate-set.yml guards on schedule/dispatch AND the default-branch ref: {row:#?}"
        );
        assert_eq!(row.writer_disposition, WriterDisposition::TrustedGuarded, "{row:#?}");
    }

    /// 7. A checked-in row asserting trusted producer state (`trusted_tree` +
    /// a fabricated producer) for a real candidate-writer violation must be
    /// rejected as drift, not accepted as a laundering of candidate bytes.
    #[test]
    fn negative_control_7_fabricated_trusted_provenance_is_rejected() {
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        let row = find_row(&inventory.families, "docs-pr-build.yml", "build");
        assert_eq!(row.cached_byte_provenance, CachedByteProvenance::CandidateTree);

        let mut laundered = serde_json::to_value(row).expect("serialize row");
        laundered["cached_byte_provenance"] = json!("trusted_tree");
        laundered["artifact_provenance"] = json!("release-orchestration.yml/publish");
        let diffs = diff_inventory(&inventory.families, &json!([laundered]));
        assert!(
            diffs.iter().any(|diff| diff.contains("field `cached_byte_provenance` drifted")),
            "expected cached_byte_provenance drift, got: {diffs:?}"
        );
        assert!(diffs.iter().any(|diff| diff.contains("field `artifact_provenance` drifted")));
    }

    /// 8. An ambiguous save guard (no event, ref, or content-success anchor)
    /// must classify as `not_proven`, never `trusted_guarded`.
    #[test]
    fn negative_control_8_ambiguous_save_authority_is_not_proven() {
        let workflow = r#"
on:
  pull_request:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
        with:
          shared-key: ambiguous-${{ hashFiles('Cargo.lock') }}
          save-if: ${{ inputs.custom_flag == 'yes' }}
"#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "ambiguous.yml", workflow);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let row = &inventory.families[0];
        assert_eq!(row.save_authority_source, SaveAuthoritySource::NotProven);
        assert_ne!(row.writer_disposition, WriterDisposition::TrustedGuarded);
        assert_eq!(row.writer_disposition, WriterDisposition::NotProven);
    }

    /// 9. Nothing in the receipt path may auto-label a restore/save as work
    /// avoided: every observation must stay `not_proven`, because there is no
    /// live telemetry hook behind it.
    #[test]
    fn negative_control_9_work_avoided_never_auto_labels() {
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        let digest = "0".repeat(64);
        let receipt = build_receipt(&inventory, digest);
        assert!(!receipt.observations.is_empty());
        for observation in &receipt.observations {
            assert_eq!(
                observation.work_avoided,
                WorkAvoided::NotProven,
                "observation for {} must stay not_proven: {observation:#?}",
                observation.family_id
            );
            assert_eq!(observation.compiler_cache_stats, "unavailable");
        }
    }

    /// 10. The manifest never stores a hand-coded denominator total, and
    /// changing exactly one row's classification changes only that row's diff
    /// plus the derived counts — never a hidden second row.
    #[test]
    fn negative_control_10_no_hard_coded_denominator_and_isolated_row_drift() {
        let root = project_root();
        let manifest_raw = fs::read_to_string(root.join(".ci/ci-cache/cache-inventory.v1.json"))
            .expect("read checked-in manifest");
        let manifest: JsonValue = serde_json::from_str(&manifest_raw).expect("parse manifest JSON");
        let top_level = manifest.as_object().expect("manifest must be a JSON object");
        for forbidden in ["denominator", "active_total", "total", "count"] {
            assert!(
                !top_level.contains_key(forbidden),
                "manifest must not store a hand-coded `{forbidden}` field"
            );
        }

        // Two independent sites, so isolation can be proven: mutating one must
        // never move the other's row diff.
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "sample.yml", SAMPLE_CACHE_WORKFLOW);
        write_file(&workflows.join("other.yml"), SAMPLE_CACHE_WORKFLOW);
        let before = derive_families_in_dirs(&workflows, None).expect("derive before");
        assert_eq!(before.families.len(), 2, "fixture must expose two independent cache sites");

        let mut mutated = before.clone();
        // Flip only the first row's capability (as if its `uses:` changed from
        // `actions/cache` to `actions/cache/restore`): this moves
        // `restore_only` and `candidate_reachable_writers`, and it must not
        // touch the second row at all.
        mutated.families[0].capability = Capability::RestoreOnly;

        let before_denominator = compute_denominator(&before);
        let after_denominator = compute_denominator(&mutated);
        assert_ne!(
            before_denominator, after_denominator,
            "mutating one row's capability must move the derived denominator"
        );

        let before_json = serde_json::to_value(&before.families).expect("serialize before");
        let diffs = diff_inventory(&mutated.families, &before_json);
        assert_eq!(diffs.len(), 1, "exactly one row must differ, got: {diffs:?}");
        assert!(
            diffs[0].contains(&mutated.families[0].id),
            "the drifted row must be the mutated one: {diffs:?}"
        );
    }

    /// 11. A stale receipt (built against a workflow tree that has since
    /// changed) must carry a different `workflow_tree_digest` — a consumer
    /// diffing digests must be able to reject it rather than accept it as
    /// current.
    #[test]
    fn negative_control_11_stale_tree_digest_mismatch_is_detectable() {
        let tmp_a = tempfile::tempdir().expect("tempdir a");
        let workflows_a = fixture_workflows_dir(&tmp_a, "sample.yml", SAMPLE_CACHE_WORKFLOW);
        let digest_a = workflow_tree_digest_for(&workflows_a, None).expect("digest a");

        let tmp_b = tempfile::tempdir().expect("tempdir b");
        let mutated = SAMPLE_CACHE_WORKFLOW.replace("sample-", "sample-v2-");
        let workflows_b = fixture_workflows_dir(&tmp_b, "sample.yml", &mutated);
        let digest_b = workflow_tree_digest_for(&workflows_b, None).expect("digest b");

        assert_ne!(digest_a, digest_b, "a changed workflow tree must change the digest");

        let digest_a_again = workflow_tree_digest_for(&workflows_a, None).expect("digest a again");
        assert_eq!(digest_a, digest_a_again, "an unchanged tree must reproduce the same digest");
    }

    /// 12. A parser/instrument failure must error out of derivation — it must
    /// never be silently represented as `status: ok` with an empty `families`
    /// array, which would read as "no caches" instead of "no evidence".
    #[test]
    fn negative_control_12_instrument_failure_never_reads_as_no_caches() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing_workflows = tmp.path().join(".github/workflows");
        let result = derive_families_in_dirs(&missing_workflows, None);
        assert!(
            result.is_err(),
            "a missing workflows dir must error, not return an empty Ok inventory"
        );

        let malformed_dir = tmp.path().join(".github/workflows-malformed");
        write_file(&malformed_dir.join("broken.yml"), "on: [pull_request\njobs: {");
        let result = derive_families_in_dirs(&malformed_dir, None);
        assert!(
            result.is_err(),
            "malformed workflow YAML must error, not return an empty Ok inventory"
        );

        let failed = build_failed_receipt("fixture: parser failure");
        assert_eq!(failed.instrument.status, InstrumentStatus::Failed);
        assert!(failed.families.is_empty());
        assert!(!failed.ok, "a failed instrument must never report `ok: true`");
        assert!(!failed.errors.is_empty());
    }

    // Sanity: capability/action-kind facts the classification rules pin.
    #[test]
    fn swatinem_with_no_save_if_is_save_capable_and_absent_authority() {
        let workflow = r#"
on:
  pull_request:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
"#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "default.yml", workflow);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let row = &inventory.families[0];
        assert_eq!(row.capability, Capability::RestoreAndSave);
        assert_eq!(row.save_authority_source, SaveAuthoritySource::Absent);
        assert_eq!(row.writer_disposition, WriterDisposition::CandidateWriterViolation);
    }

    #[test]
    fn empty_derived_inventory_is_the_default_struct() {
        let empty = DerivedInventory::default();
        assert!(empty.families.is_empty());
        assert!(empty.dormant.is_empty());
    }

    // -----------------------------------------------------------------------
    // Repair-round additions: key authority must weigh reachability, not just
    // key text (candidate-controlled `hashFiles` keys must never be `trusted`).
    // -----------------------------------------------------------------------

    #[test]
    fn candidate_reachable_hashfiles_key_is_not_trusted() {
        // #9177's own falsifier: a candidate-reachable Swatinem key derived
        // from `hashFiles('**/Cargo.lock')` is candidate-influenced (the
        // candidate controls `Cargo.lock` on its own PR). The literal
        // substrings `pull_request`/`head_ref` are absent from this text, so
        // a text-only check would wrongly call it `trusted`.
        let root = project_root();
        let inventory = derive_families_in_dirs(&root.join(".github/workflows"), None)
            .expect("derive inventory");
        let row = find_row(&inventory.families, "docs-pr-build.yml", "build");
        assert_eq!(row.candidate_event_reachability, CandidateEventReachability::Reachable);
        assert_ne!(
            row.key_authority,
            KeyAuthority::Trusted,
            "a candidate-reachable hashFiles-derived key must never read as trusted: {row:#?}"
        );
    }

    #[test]
    fn non_reachable_hashfiles_key_stays_trusted() {
        // The same candidate-influenceable key text on a row the candidate
        // cannot reach at all is not a live exposure: `key_authority` must
        // depend on reachability, not merely scan the key text in isolation.
        let workflow = r#"
on:
  push:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
        with:
          shared-key: push-only-${{ hashFiles('Cargo.lock') }}
"#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "push-only.yml", workflow);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let row = &inventory.families[0];
        assert_eq!(row.candidate_event_reachability, CandidateEventReachability::NotCandidateEvent);
        assert_eq!(row.key_authority, KeyAuthority::Trusted);
    }

    // -----------------------------------------------------------------------
    // Review-repair regressions (#15581 review dispositions)
    // -----------------------------------------------------------------------

    #[test]
    fn or_branch_without_a_ref_guard_does_not_bless_the_expression() {
        // `A || B` is true whenever B alone is true, so one guarded branch
        // cannot constrain its siblings: a PR-true branch next to a ref
        // equality must NOT read as a ref guard, or the row reads
        // `trusted_guarded` while the candidate saves.
        assert!(!ref_literal_guard(
            "github.ref == 'refs/heads/main' || github.event_name == 'pull_request'"
        ));
        // Every branch guarding is still a guard.
        assert!(ref_literal_guard(
            "github.ref == 'refs/heads/main' || github.ref == format(\"refs/heads/{0}\", \
             github.event.repository.default_branch)"
        ));
        // AND semantics are unchanged: any term establishing the equality
        // forces it (all terms must hold).
        assert!(ref_literal_guard(
            "github.event_name == 'push' && github.ref == 'refs/heads/main'"
        ));
    }

    const MERGE_GROUP_ONLY_JOB_WORKFLOW: &str = r#"
on:
  pull_request:
  merge_group:
jobs:
  merge-group-cache:
    if: github.event_name == 'merge_group'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/cache@abf7c9fde3ce4c7b1f2a2b9a2b8e2b9a2b8e2b9a
        with:
          path: target/merge-group
          key: merge-group-${{ github.sha }}
"#;

    #[test]
    fn merge_group_only_job_in_a_mixed_trigger_workflow_is_not_statically_excluded() {
        // `merge_group` is itself a candidate event: a job gated to it in a
        // workflow that also runs on pull_request is candidate-reachable and
        // must not read as `statically_excluded` (the lint crate's PR-only
        // exclusion answer is narrower than this inventory's question).
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "mixed.yml", MERGE_GROUP_ONLY_JOB_WORKFLOW);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let row = find_row(&inventory.families, "mixed.yml", "merge-group-cache");
        assert_eq!(row.candidate_event_reachability, CandidateEventReachability::Reachable);
        assert_eq!(row.writer_disposition, WriterDisposition::CandidateWriterViolation);
    }

    #[test]
    fn condition_anchored_off_the_candidate_event_set_is_statically_excluded() {
        // A `push`-anchored job in a [pull_request, merge_group] workflow is
        // provably false under every candidate trigger in that set.
        let workflow = MERGE_GROUP_ONLY_JOB_WORKFLOW
            .replace("if: github.event_name == 'merge_group'", "if: github.event_name == 'push'");
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "mixed.yml", &workflow);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let row = find_row(&inventory.families, "mixed.yml", "merge-group-cache");
        assert_eq!(
            row.candidate_event_reachability,
            CandidateEventReachability::StaticallyExcluded
        );
    }

    #[test]
    fn or_chain_starting_with_an_event_equality_is_not_one_anchor() {
        // `(pull_request || merge_group) && outputs...` can hold under BOTH
        // candidate events, so it excludes nothing: the anchor parser must
        // reject an `||` chain that merely starts with an equality instead
        // of reading the whole chain as one quoted literal.
        assert!(!condition_excludes_candidate_event(
            "(github.event_name == 'pull_request' || \
                 github.event_name == 'merge_group') && needs.draft.outputs.run == 'true'",
            "pull_request"
        ));
        assert!(!condition_excludes_candidate_event(
            "(github.event_name == 'pull_request' || \
                 github.event_name == 'merge_group') && needs.draft.outputs.run == 'true'",
            "merge_group"
        ));
        // A job anchored to the other candidate event excludes that one
        // direction only: a PR-anchored condition cannot exclude a PR.
        assert!(condition_excludes_candidate_event(
            "github.event_name == 'pull_request' && needs.draft.outputs.run == 'true'",
            "merge_group"
        ));
        assert!(!condition_excludes_candidate_event(
            "github.event_name == 'pull_request' && needs.draft.outputs.run == 'true'",
            "pull_request"
        ));
    }

    const UNCALLED_COMPOSITE_WITH_CACHE: &str = r#"
name: 'Uncalled'
runs:
  using: 'composite'
  steps:
    - name: Cache cargo dependencies
      uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
      with:
        shared-key: 'perl-lsp'
"#;

    /// A `push`-only workflow whose single unguarded Swatinem step is a real
    /// row (never a cache site by text-scan alone), used as filler so fixture
    /// derivations contain exactly one known active site.
    const PUSH_ONLY_UNGUARDED: &str = r#"
on:
  push:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
        with:
          shared-key: push-only-${{ hashFiles('Cargo.lock') }}
"#;

    #[test]
    fn uncalled_composite_cache_row_stays_dormant() {
        // `dormant` is reserved for composites with no active caller, per
        // `docs/ci/cache-policy.md`.
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "unrelated.yml", PUSH_ONLY_UNGUARDED);
        let actions_dir = tmp.path().join(".github/actions");
        write_file(&actions_dir.join("setup-uncalled/action.yml"), UNCALLED_COMPOSITE_WITH_CACHE);

        let inventory =
            derive_families_in_dirs(&workflows, Some(&actions_dir)).expect("derive inventory");
        assert_eq!(
            inventory.families.len(),
            1,
            "only the workflow's own site: {:#?}",
            inventory.families
        );
        assert_eq!(
            inventory.dormant.len(),
            1,
            "the uncalled composite row: {:#?}",
            inventory.dormant
        );
        let dormant = &inventory.dormant[0];
        assert_eq!(dormant.job, "composite-action");
        assert_eq!(
            dormant.candidate_event_reachability,
            CandidateEventReachability::StaticallyDead,
            "an unreferenced composite cache site never runs: {dormant:#?}"
        );
    }

    #[test]
    fn literal_save_if_false_row_is_not_save_eligible() {
        // A literal `save-if: false` proves the step can never enter its
        // save path; advertising `save_eligible: true` for it would let a
        // consumer count rows that can never save.
        let workflow = r#"
on:
  pull_request:
jobs:
  never-saves:
    runs-on: ubuntu-latest
    steps:
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6
        with:
          save-if: ${{ false }}
"#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "never-saves.yml", workflow);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let row = find_row(&inventory.families, "never-saves.yml", "never-saves");
        let observation = observation_for(row);
        assert!(
            !observation.save_eligible,
            "a literal save-if: false row must not advertise save eligibility: {observation:#?}"
        );
        assert_eq!(observation.save_result, SaveResult::Unknown);
    }

    #[test]
    fn manifest_extra_field_is_drift() {
        // The manifest is claimed to *equal* the derived inventory: a field
        // the deriver no longer emits (obsolete or fabricated) must fail
        // the drift gate instead of silently surviving regeneration.
        let tmp = tempfile::tempdir().expect("tempdir");
        let workflows = fixture_workflows_dir(&tmp, "sample.yml", PUSH_ONLY_UNGUARDED);
        let inventory = derive_families_in_dirs(&workflows, None).expect("derive inventory");
        let mut fabricated = serde_json::to_value(&inventory.families[0]).expect("serialize row");
        fabricated["writer_disposition_override"] = json!("trusted_guarded");

        let diffs = diff_inventory(&inventory.families, &json!([fabricated]));
        assert!(
            diffs.iter().any(|diff| {
                diff.contains(
                    "manifest field `writer_disposition_override` is not part of the derived row",
                )
            }),
            "a fabricated manifest field must be reported as drift, got: {diffs:?}"
        );
    }

    #[test]
    fn api_version_pin_rejects_unknown_versions() {
        assert!(ensure_supported_api_version("v1").is_ok());
        let error = ensure_supported_api_version("v2").expect_err("v2 must fail loudly");
        assert!(
            error.to_string().contains("unsupported --api-version `v2`"),
            "the error must name the rejected pin: {error}"
        );
    }
}
