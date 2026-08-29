//! Selection of sufficient child-increment proof through the shared route
//! planner (`ci_route_plan.v1`, #4787/#4789).
//!
//! This module never re-derives lifecycles, denominators, or planned
//! outcomes. It pins one admitted execution identity — the additive
//! `stack_local` profile of #10178 — derives exact-subject selector facts
//! mechanically from the child-only delta plus declared per-gate scopes,
//! and delegates outcome composition to [`crate::ci_route_plan`] compile.
//! A governed active gate without a declared scope can never receive
//! positive proof and therefore compiles into a visible typed error row
//! instead of a scoped no-op or an omission.

use super::{ChildDelta, StackIncrementSubjectV1, sha256_hex, validate_nonempty};
use crate::ci_route_plan::{
    CiRoutePlanV1, CompileRoutePlanInput, GateSelectorInput, RouteDispositionInput,
    RouteExecutionIdentity, RouteProfileExpansionInput, RouteSelectionEvidence, RouteSubjectRef,
    SelectorPlacement, SelectorProof,
};
use serde::{Deserialize, Serialize};

/// The one execution profile permitted for stack-local increment planning.
/// Any other requested profile refuses: candidate-supplied bytes cannot pick
/// their own trusted tier.
pub const STACK_LOCAL_PROFILE: &str = "stack_local";

/// Closed refusal-code vocabulary emitted by stack-plan compilation.
pub const STACK_PLAN_ERROR_CODES: &[&str] =
    &["profile_not_stack_local", "plan_unbound_to_subject", "route_plan_refused"];

/// Typed stack-plan compilation refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackPlanCompileError {
    /// Closed refusal-code identity from [`STACK_PLAN_ERROR_CODES`].
    pub code: String,
    /// Human-readable explanation naming the refused fact.
    pub message: String,
}

impl std::fmt::Display for StackPlanCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for StackPlanCompileError {}

fn refuse_plan(code: &str, message: impl Into<String>) -> StackPlanCompileError {
    debug_assert!(STACK_PLAN_ERROR_CODES.contains(&code), "unknown refusal code {code}");
    StackPlanCompileError { code: code.to_string(), message: message.into() }
}

/// Declared read surface of one governed gate inside this repository.
/// Entries use the same literal-prefix rule as edge scopes: `crates/foo/`
/// matches whole subtrees, other entries match full paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackGateScope {
    /// Governed gate identity this scope belongs to.
    pub gate_id: String,
    /// Declared read-scope entries. An empty list can never produce
    /// positive applicable evidence.
    pub path_prefixes: Vec<String>,
}

/// Everything one stack-local plan needs, projected from checked authorities
/// plus this subject's delta. Pure compile inputs only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackPlanRequest {
    /// Fully validated subject whose exact identities back this plan.
    pub subject: StackIncrementSubjectV1,
    /// #10178 profile expansion projected for [`STACK_LOCAL_PROFILE`]; must
    /// be `Complete` with the governed stack-local denominator.
    pub expansion: RouteProfileExpansionInput,
    /// #10176 resolved dispositions covering every denominator gate.
    pub dispositions: Vec<RouteDispositionInput>,
    /// #10176 authority semantic digest.
    pub disposition_digest: String,
    /// Trusted workflow identity digest.
    pub workflow_digest: String,
    /// Declared per-gate read scopes enabling positive selector evidence.
    pub gate_scopes: Vec<StackGateScope>,
    /// Executable identities per runnable gate.
    pub execution: Vec<RouteExecutionIdentity>,
}

/// Derive mechanical, delta-bound selector facts for every governed gate.
///
/// Positive proof comes only from the exact child-only delta matching a
/// declared scope: no branch name, title, label, check name, or candidate
/// convention participates.
#[must_use]
pub fn derive_selectors(
    delta: &ChildDelta,
    denominator: &[String],
    gate_scopes: &[StackGateScope],
) -> Vec<GateSelectorInput> {
    let mut selectors = Vec::new();
    for gate_id in denominator {
        // A gate without any declared scope keeps no selector input at all,
        // so the shared planner composes a visible selector-evidence error
        // row instead of a silent skip or an invented no-op.
        let Some(scope) = gate_scopes.iter().find(|scope| &scope.gate_id == gate_id) else {
            continue;
        };
        let matched_path = delta
            .paths
            .iter()
            .map(|row| row.path.as_str())
            .find(|path| path_in_scope(path, &scope.path_prefixes));
        let selector = match matched_path {
            Some(path) => GateSelectorInput {
                gate_id: gate_id.clone(),
                placement: SelectorPlacement::Selected,
                role: None,
                reason: format!("delta path {path:?} matches the declared gate scope"),
                proof: Some(SelectorProof::Applicable),
            },
            None => GateSelectorInput {
                gate_id: gate_id.clone(),
                placement: SelectorPlacement::Skipped,
                role: None,
                reason: non_applicability_reason(delta, &scope.path_prefixes),
                proof: Some(SelectorProof::NotApplicableToSubject),
            },
        };
        selectors.push(selector);
    }
    selectors
}

fn non_applicability_reason(delta: &ChildDelta, prefixes: &[String]) -> String {
    if delta.paths.is_empty() && prefixes.is_empty() {
        "the child-only delta is empty".to_string()
    } else {
        "delta matches no declared scope for this gate".to_string()
    }
}

fn path_in_scope(path: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|prefix| super::path_matches_scope_entry(path, prefix))
}

/// Deterministic selector-evidence digest binding every derived selector to
/// the delta fingerprint and the declared scope set.
#[must_use]
pub fn selector_binding_digest(delta_fingerprint: &str, gate_scopes: &[StackGateScope]) -> String {
    let mut input = Vec::new();
    input.extend_from_slice(delta_fingerprint.as_bytes());
    for scope in gate_scopes {
        input.push(b'\n');
        input.extend_from_slice(scope.gate_id.as_bytes());
        for prefix in &scope.path_prefixes {
            input.push(0);
            input.extend_from_slice(prefix.as_bytes());
        }
    }
    sha256_hex(&input)
}

/// Compile one stack-local plan through the shared route planner.
///
/// # Errors
/// Returns a typed refusal when the requested profile is not
/// [`STACK_LOCAL_PROFILE`], the compiled route plan does not bind to this
/// exact subject, or the shared planner itself refuses the projection.
pub fn compile_stack_plan(
    request: StackPlanRequest,
) -> Result<CiRoutePlanV1, StackPlanCompileError> {
    if request.expansion.requested_profile != STACK_LOCAL_PROFILE {
        return Err(refuse_plan(
            "profile_not_stack_local",
            format!(
                "requested profile {:?} is not {STACK_LOCAL_PROFILE}; candidate-supplied \
                 inputs cannot choose another trusted profile",
                request.expansion.requested_profile
            ),
        ));
    }
    validate_nonempty("workflow digest", &request.workflow_digest)
        .map_err(|message| refuse_plan("route_plan_refused", message))?;
    let StackPlanRequest {
        subject,
        expansion,
        dispositions,
        disposition_digest,
        workflow_digest,
        gate_scopes,
        execution,
    } = request;
    let selector_digest = selector_binding_digest(&subject.delta.fingerprint, &gate_scopes);
    let selectors = derive_selectors(&subject.delta, &expansion.denominator, &gate_scopes);
    let expected_subject_digest = super::subject::subject_digest(&subject);
    let route_subject = RouteSubjectRef {
        kind: "stack_increment_child".to_string(),
        head_sha: subject.child.head_sha.clone(),
        base_sha: Some(subject.parent.head_sha.clone()),
        subject_digest: expected_subject_digest.clone(),
    };
    let selection = RouteSelectionEvidence {
        base: subject.parent.head_sha.clone(),
        scope_ok: true,
        fallback_used: false,
        fallback_reason: None,
        package_args: Vec::new(),
        scope: None,
        selector_digest,
    };
    let input = CompileRoutePlanInput {
        subject: route_subject,
        expansion,
        dispositions,
        disposition_digest,
        workflow_digest,
        selectors,
        selection,
        execution,
    };
    let plan = CiRoutePlanV1::compile(input)
        .map_err(|message| refuse_plan("route_plan_refused", message))?;
    if plan.subject.subject_digest != expected_subject_digest {
        return Err(refuse_plan(
            "plan_unbound_to_subject",
            "compiled route plan does not bind to the exact stack subject digest",
        ));
    }
    Ok(plan)
}

/// Stable byte identity of a compiled plan, used by results to bind
/// themselves to the exact selected proof set.
///
/// Note: canonical JSON-Schema publication of route plans is owned by leaf
/// #10179; this digest is a deterministic serde serialization hash used only
/// for exact binding inside the stack contracts.
///
/// # Errors
/// Refuses when serialization of the plain domain payload fails, which would
/// indicate a domain defect surfaced by tests, never runtime data.
pub fn stack_plan_digest(plan: &CiRoutePlanV1) -> Result<String, StackPlanCompileError> {
    serde_json::to_vec(plan).map(|bytes| sha256_hex(&bytes)).map_err(|error| {
        refuse_plan("route_plan_refused", format!("serialization failed: {error}"))
    })
}
