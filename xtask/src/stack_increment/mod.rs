//! Exact parent-head → child-head stack increment proof (#11229 S1).
//!
//! This module owns the two stack-local contracts —
//! `stack_increment_subject.v1` and `stack_increment_result.v1` — plus their
//! admission validators, the child-only semantic delta, mechanical selector
//! derivation over the shared route planner, and one normalized advisory
//! fan-in result.
//!
//! The stack-local subject is deliberately distinct from protected-main
//! landing: every compiled subject and every compiled result carries
//! [`PROTECTED_MAIN_NOT_EVALUATED`], and any other value fails validation.
//! A green [`ContextStatus::CurrentSuccess`] can therefore never satisfy
//! merge readiness by construction.
//!
//! Authorities consumed (never re-derived here):
//!
//! - `ci_route_plan.v1` (#4787/#4789): governed denominators, selector
//!   composition, planned outcomes;
//! - `ci_route_profile.v1` (#10178): the additive `stack_local` profile;
//! - `gate_disposition.v1` (#10176): lifecycle/quarantine facts, projected
//!   in by the caller;
//! - `git_ancestry.v1`: read-only history relation evidence.
//!
//! Pure domain functions here perform no filesystem, clock, network, or Git
//! access; thin `_live` adapters bind them to a checked-out repository.

mod delta;
mod plan;
mod result;
mod subject;
#[cfg(test)]
mod tests;

pub use delta::{
    ChildDelta, DeltaPath, DeltaStatus, check_declared_scope, compute_delta_from_trees,
    delta_fingerprint,
};
pub use plan::{
    STACK_LOCAL_PROFILE, StackGateScope, StackPlanCompileError, StackPlanRequest,
    compile_stack_plan, derive_selectors, stack_plan_digest,
};
pub use result::{
    ChildIncrementStatus, ContextStatus, ParentPrerequisiteState, RunIdentity,
    StackIncrementResultV1, StackObservation, StackResultCompileError, StackResultInput,
    StackRowResult, compile_result, render_explanation, validate_result,
};
pub use subject::{
    EdgeKind, RelatedHistory, StackEdgeDeclaration, StackEndpoint, StackIncrementSubjectV1,
    StackSubjectCompileError, StackSubjectInput, TrustContext, compile_subject,
    parse_stack_edge_declaration, refresh_currentness, resolve_endpoint_live, subject_digest,
    validate_subject,
};

/// Contract identity of the stack-local increment subject.
pub const STACK_INCREMENT_SUBJECT_SCHEMA: &str = "stack_increment_subject.v1";

/// Contract identity of the normalized stack-local result.
pub const STACK_INCREMENT_RESULT_SCHEMA: &str = "stack_increment_result.v1";

/// Producer identity stamped into both contracts.
pub const STACK_INCREMENT_PRODUCER: &str = "xtask::stack_increment";

/// The only legal protected-main state for a stack-local artifact. Any other
/// value is rejected: a stack-local proof can never promote itself into a
/// protected-target evaluation.
pub const PROTECTED_MAIN_NOT_EVALUATED: &str = "NOT_EVALUATED_PROTECTED_MAIN";

/// Stable name of the advisory review context published for a stack-local
/// result. Advisory only: never a protected-main required context.
pub const STACK_ADVISORY_CONTEXT_NAME: &str = "Stack Increment Proof";

/// Literal token starting the one machine-readable declaration that admits a
/// parent/child edge. Branch names, titles, labels, and filenames never
/// contribute admission evidence: without this explicit declaration the
/// compile is refused.
pub const STACK_EDGE_DECLARATION_PREFIX: &str = "stack-edge:";

/// Validate a 40-character lowercase hexadecimal SHA string.
pub(crate) fn validate_sha40(field: &str, value: &str) -> Result<(), String> {
    if value.len() == 40
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value.bytes().all(|byte| !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(format!("{field} must be a full 40-character lowercase hexadecimal SHA"))
    }
}

/// Validate a 64-character lowercase hexadecimal SHA-256 digest string.
pub(crate) fn validate_sha256_hex(field: &str, value: &str) -> Result<(), String> {
    if value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value.bytes().all(|byte| !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(format!("{field} must be a full 64-character lowercase hexadecimal SHA-256 digest"))
    }
}

/// Reject empty or whitespace-only text fields.
pub(crate) fn validate_nonempty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() { Err(format!("{field} must be non-empty")) } else { Ok(()) }
}

/// Component-wise subtree test for one declared scope entry.
///
/// A scope entry ending in `/` names a whole subtree: the candidate path must
/// equal the subtree root itself or extend it by at least one full path
/// component. Every other entry matches only the exact full path. The
/// component boundary (never a bare string prefix) is load-bearing: a string
/// `starts_with` test lets an undeclared sibling such as `crates/foobar`
/// pass a `crates/foo/` declaration (#13360 root cause 8).
pub(crate) fn path_matches_scope_entry(path: &str, scope: &str) -> bool {
    match scope.strip_suffix('/') {
        Some(subtree) => {
            path.strip_prefix(subtree).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        }
        None => path == scope,
    }
}

/// Compute the SHA-256 hex digest of a byte slice.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let summary = hasher.finalize();
    summary.iter().map(|byte| format!("{byte:02x}")).collect()
}
