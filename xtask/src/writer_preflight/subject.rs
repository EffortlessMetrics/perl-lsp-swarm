//! The typed writer-preflight subject (#11633).
//!
//! The subject is *what is being checked*, decided entirely by the caller
//! before any evidence exists. It carries identity and intent only — never
//! observations, never verdicts. Checking one subject cannot authorize a
//! mutation of another subject because every decision binds its reasons to
//! these exact fields (`decide` refuses cross-subject bindings through
//! `BranchWorktreeMismatch` / `WrongOrUnknownCandidate`, not by convention).
//!
//! Identity strings (common dir, remote URL, branch names, owner ids) are
//! opaque comparison tokens supplied by adapters. The core never parses
//! their shape: no machine path convention becomes repository policy here
//! (#11633 falsifier 11) — adapters normalize spelling, the domain compares
//! values.

use serde::{Deserialize, Serialize};

/// Wire/schema version of the writer-preflight domain (#11633). Bump only
/// through a classified revision of #11633; consumers (#11634/#11635/#11636)
/// reject mismatched versions instead of guessing.
pub const WRITER_PREFLIGHT_SCHEMA_VERSION: u32 = 1;

/// Repository-level identity shared by the subject and the observed fact.
/// Compared field-wise: `common_dir` exactly; `canonical_remote` only when
/// the subject supplies it (an adapter may observe more than the caller
/// declared, never less than the caller required).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryIdentity {
    /// Git common-dir identity, normalized by the adapter.
    pub common_dir: String,
    /// Canonical remote URL when the caller pins one.
    pub canonical_remote: Option<String>,
}

/// The requested transition. Read-only and mutating subjects have
/// mechanically distinct required-fact sets in `decide`; a read-only
/// verification can never authorize a mutating transition because the
/// operation participates in the decision itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterPreflightOperation {
    ReadOnly,
    Create,
    Resume,
    Mutate,
}

impl WriterPreflightOperation {
    /// True for every operation that could produce a writer/mutation.
    pub fn is_mutating(self) -> bool {
        !matches!(self, WriterPreflightOperation::ReadOnly)
    }

    /// Stable token used by projections; identical to the serde form so
    /// human and JSON renderings cannot disagree (#11633 falsifier 13).
    pub fn as_str(self) -> &'static str {
        match self {
            WriterPreflightOperation::ReadOnly => "read_only",
            WriterPreflightOperation::Create => "create",
            WriterPreflightOperation::Resume => "resume",
            WriterPreflightOperation::Mutate => "mutate",
        }
    }
}

/// The claim the requested transition would serve. `branch` is the exact
/// candidate branch name; an empty name makes the mutation subject unknown
/// (`decide` reports `WrongOrUnknownCandidate` — load-bearing per the
/// decision laws). `worktree_path` is the requested/target worktree location
/// when the caller pins one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimIdentity {
    pub issue: Option<String>,
    pub branch: String,
    pub worktree_path: Option<String>,
}

/// Selected heavy-build/capacity demand. Selecting a requirement makes
/// capacity a required fact; leaving it `None` keeps ordinary host load
/// advisory-only (#11633 decision law: unrelated load is not a universal
/// mutation denial).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityRequirement {
    FocusedBuild,
    HeavyBuild,
}

/// One exact preflight subject (#11633 "Subject identity"). All fields are
/// caller-supplied intent; absence (`None`) means "not supplied", which the
/// decision core treats as "this constraint is not part of the subject", not
/// as "satisfied".
///
/// # Consumer seams (successor graph from #11633)
///
/// - #11634 resolves this struct from `cargo xtask work preflight [subject args]`
///   and gathers one [`crate::writer_preflight::WriterPreflightObservationSet`]
///   for it, then calls `decide` exactly once.
/// - #11635 re-checks `subject_digest` equality immediately before mutation
///   (compare-and-mutate): the digest binds the admitted subject.
/// - #11636 parity packets key every cell by this subject plus the decision
///   digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterPreflightSubject {
    pub repository: RepositoryIdentity,
    pub operation: WriterPreflightOperation,
    pub claim: ClaimIdentity,
    /// Expected base commit; compared against the observed base with git's
    /// abbreviated-SHA prefix rule (mirrors #3957's `sha_matches`).
    pub expected_base_sha: Option<String>,
    /// Expected current candidate/head identity; a mismatch is
    /// `WrongOrUnknownCandidate`, never silently accepted.
    pub candidate_head_sha: Option<String>,
    /// Expected current writer/mutation owner where supplied. A same-
    /// candidate writer owned by THIS identity is re-entry, not collision.
    pub expected_writer_owner: Option<String>,
    /// Selected capacity demand, if any.
    pub capacity_requirement: Option<CapacityRequirement>,
    /// Executor-policy identity (#9548 executor-owned process-local Cargo
    /// configuration) when the caller declares one.
    pub executor_policy: Option<String>,
}

impl WriterPreflightSubject {
    /// A mutating subject is known only when it names a non-empty candidate
    /// branch. An unknown mutation subject is load-bearing: `decide`
    /// blocks it with `WrongOrUnknownCandidate` rather than inventing a
    /// target.
    pub fn mutation_subject_known(&self) -> bool {
        !self.claim.branch.trim().is_empty()
    }
}
