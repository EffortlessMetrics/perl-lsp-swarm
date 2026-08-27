//! Pure checked adapter from complete effective-invocation observations to
//! canonical invocation plan projections (#12284 → #8492/#4827).
//!
//! The adapter validates each observed field independently, retains order,
//! and produces one deterministic digest-stamped projection. It can never:
//! derive cwd or include roots from a source path; supply TestInit from a
//! profile table; normalize away wrapper/interpreter order; copy missing
//! fields from an expected plan; borrow values from a sibling invocation; use
//! a direct-probe plan; or call the runner or inspect the prepared filesystem.
//! A partial observation may produce a diagnostic field comparison against an
//! expected subject, never an authoritative plan.

use crate::invocation_trace::model::{
    CanonicalInvocationProjection, EffectiveInvocationFields, EffectiveInvocationRow, FieldKey,
    FieldStateRef, InvocationAuthority, InvocationObservationState, ProjectionRecord,
    ProjectionRejectionKind, RowSubjectBinding, TaintMode, TraceSubjectIdentity, Utf8Switch,
    canonical_projection_digest,
};
use crate::runner_model::RunnerKind;
use serde::Serialize;

/// Neutral TestInit spelling for the unreachable no-value path; a projection
/// reaching this point already proved every field observed.
fn standard_test_init() -> crate::invocation_trace::model::TestInitClass {
    crate::invocation_trace::model::TestInitClass::Standard
}

/// The exact expected-subject binding a projection is checked against. It
/// carries identities only — never invocation values — so an expected plan can
/// never lend content to an incomplete observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedInvocationBinding {
    /// Canonical member identity the projection must retain.
    pub member_path: String,
    /// Runner route the projection must retain.
    pub runner: RunnerKind,
    /// Target identity.
    pub target_id: String,
    /// Environment-variant target when applicable.
    pub variant_target_id: Option<String>,
    /// Instrumentation subject when applicable.
    pub instrumentation_id: Option<String>,
}

impl ExpectedInvocationBinding {
    /// Derive the binding from the receipt subject and one row's member.
    pub fn from_subject(
        subject: &TraceSubjectIdentity,
        row: &RowSubjectBinding,
    ) -> ExpectedInvocationBinding {
        ExpectedInvocationBinding {
            member_path: row.parent_member_path.clone(),
            runner: row.runner,
            target_id: subject.target_id.clone(),
            variant_target_id: subject.variant_target_id.clone(),
            instrumentation_id: subject.instrumentation_id.clone(),
        }
    }
}

/// Typed rejection of one canonical plan projection. Every failure mode of
/// the adapter is one named variant; no rejection is a bare string.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum ProjectionRejection {
    /// The row is not `observed_complete`; partial or failed observations
    /// never project.
    ObservationNotComplete {
        /// The row's derived state.
        state: InvocationObservationState,
    },
    /// The row's frame was not accepted, so its identity is unusable.
    FrameNotAccepted,
    /// A behavior-bearing field is not `observed` and can never be
    /// synthesized from source, profile, or the expected plan.
    FieldNotObserved {
        /// First field (declaration order) that is not observed.
        field: FieldKey,
    },
    /// An observed value failed its independent validation law.
    InvalidObservedValue {
        /// Field whose value is invalid.
        field: FieldKey,
        /// Why the value is invalid.
        reason: String,
    },
    /// The row's subject binding disagrees with the expected subject.
    SubjectMismatch {
        /// What disagreed.
        detail: String,
    },
    /// Direct-probe routes carry a distinct authority and never project
    /// through the upstream observation contract.
    DirectProbeAuthority,
}

impl ProjectionRejection {
    /// Rejection kind retained per row.
    pub fn kind(&self) -> ProjectionRejectionKind {
        match self {
            ProjectionRejection::ObservationNotComplete { .. } => {
                ProjectionRejectionKind::ObservationNotComplete
            }
            ProjectionRejection::FrameNotAccepted => ProjectionRejectionKind::FrameNotAccepted,
            ProjectionRejection::FieldNotObserved { .. } => {
                ProjectionRejectionKind::FieldNotObserved
            }
            ProjectionRejection::InvalidObservedValue { .. } => {
                ProjectionRejectionKind::InvalidObservedValue
            }
            ProjectionRejection::SubjectMismatch { .. } => ProjectionRejectionKind::SubjectMismatch,
            ProjectionRejection::DirectProbeAuthority => {
                ProjectionRejectionKind::DirectProbeAuthority
            }
        }
    }
}

/// Result of one canonical plan projection attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectionOutcome {
    /// The observation was complete and every field validated; the retained
    /// plan is authoritative for exactly this invocation and `digest` binds
    /// its exact content.
    Projected {
        /// The complete projection.
        projection: Box<CanonicalInvocationProjection>,
        /// Deterministic digest over the projection's exact content.
        digest: String,
    },
    /// The attempt failed with one typed rejection.
    Rejected(ProjectionRejection),
}

impl ProjectionOutcome {
    /// Compact per-row record retaining the typed outcome.
    pub fn record(&self) -> ProjectionRecord {
        match self {
            ProjectionOutcome::Projected { digest, .. } => {
                ProjectionRecord::Projected { digest: digest.clone() }
            }
            ProjectionOutcome::Rejected(rejection) => {
                ProjectionRecord::Rejected { reason: rejection.kind() }
            }
        }
    }

    /// True when the projection was accepted.
    pub fn is_projected(&self) -> bool {
        matches!(self, ProjectionOutcome::Projected { .. })
    }
}

/// Diagnostic comparison of one observation against an expected subject.
/// This is a field-level report only: it can never be turned into a plan, and
/// an expected value can never be relabelled `observed` through it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedFieldComparison {
    /// Field the entry describes.
    pub field: FieldKey,
    /// Typed comparison result.
    pub result: ExpectedFieldResult,
}

/// Typed result of one diagnostic field comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpectedFieldResult {
    /// The observed value equals the expected value.
    ObservedEqual,
    /// The observed value differs from the expected value.
    ObservedDifferent,
    /// The field was observed but the expectation carries no value for it;
    /// nothing can agree or disagree.
    NoExpectation,
    /// The field was not observed; the expected value stays an expectation,
    /// never evidence.
    NotObserved,
    /// The field does not apply to this invocation shape.
    NotApplicable,
    /// The field is ambiguous, malformed, or instrument-failed.
    Unresolved,
}

/// Diagnostic comparison usable for partial rows. Produces only typed
/// field-level results; constructs no plan and upgrades no field.
pub fn compare_expected(
    fields: &EffectiveInvocationFields,
    expected_values: &ExpectedInvocationValues,
) -> Vec<ExpectedFieldComparison> {
    let mut comparisons = Vec::new();
    for key in FieldKey::ALL {
        let result = match fields.state_of(key) {
            FieldStateRef::Observed => {
                if !expected_values.carries(key) {
                    ExpectedFieldResult::NoExpectation
                } else if expected_values.matches(key, fields) {
                    ExpectedFieldResult::ObservedEqual
                } else {
                    ExpectedFieldResult::ObservedDifferent
                }
            }
            FieldStateRef::NotApplicable => ExpectedFieldResult::NotApplicable,
            FieldStateRef::NotObserved => ExpectedFieldResult::NotObserved,
            FieldStateRef::Ambiguous
            | FieldStateRef::Malformed
            | FieldStateRef::InstrumentFailure => ExpectedFieldResult::Unresolved,
        };
        comparisons.push(ExpectedFieldComparison { field: key, result });
    }
    comparisons
}

/// Expected-side values for diagnostic comparison. These are expectations,
/// never evidence: they are readable here but cannot enter a projection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExpectedInvocationValues {
    /// Expected run cwd.
    pub run_cwd: Option<String>,
    /// Expected ordered include roots.
    pub include_roots: Option<Vec<String>>,
    /// Expected TestInit class.
    pub test_init: Option<String>,
    /// Expected taint mode.
    pub taint_mode: Option<String>,
    /// Expected script path.
    pub script_path: Option<String>,
}

impl ExpectedInvocationValues {
    fn carries(&self, key: FieldKey) -> bool {
        match key {
            FieldKey::RunCwd => self.run_cwd.is_some(),
            FieldKey::IncludeRoots => self.include_roots.is_some(),
            FieldKey::TestInit => self.test_init.is_some(),
            FieldKey::TaintMode => self.taint_mode.is_some(),
            FieldKey::ScriptPath => self.script_path.is_some(),
            _ => false,
        }
    }

    fn matches(&self, key: FieldKey, fields: &EffectiveInvocationFields) -> bool {
        match key {
            FieldKey::RunCwd => self.run_cwd.as_deref().is_some_and(|expected| {
                fields.run_cwd.observed().map(String::as_str) == Some(expected)
            }),
            FieldKey::IncludeRoots => self
                .include_roots
                .as_ref()
                .is_some_and(|expected| fields.include_roots.observed() == Some(expected)),
            FieldKey::TestInit => self.test_init.as_deref().is_some_and(|spelling| {
                spelling
                    == match fields.test_init.observed() {
                        Some(value) => test_init_spelling(value),
                        None => "",
                    }
            }),
            FieldKey::TaintMode => self.taint_mode.as_deref().is_some_and(|spelling| {
                spelling
                    == match fields.taint_mode.observed() {
                        Some(value) => taint_mode_spelling(value),
                        None => "",
                    }
            }),
            FieldKey::ScriptPath => self.script_path.as_deref().is_some_and(|expected| {
                fields.script_path.observed().map(String::as_str) == Some(expected)
            }),
            _ => true,
        }
    }
}

fn test_init_spelling(value: &crate::invocation_trace::model::TestInitClass) -> &'static str {
    use crate::invocation_trace::model::TestInitClass::*;
    match value {
        Standard => "standard",
        U1 => "u1",
        U2 => "u2",
        U2t => "u2t",
        A => "a",
        Nc => "nc",
    }
}

fn taint_mode_spelling(value: &crate::invocation_trace::model::TaintMode) -> &'static str {
    use crate::invocation_trace::model::TaintMode::*;
    match value {
        None => "none",
        TaintWarnings => "taint_warnings",
        TaintMode => "taint_mode",
    }
}

/// Project one complete effective-invocation observation into its canonical
/// plan projection. Every field is re-validated independently; order is
/// retained verbatim; nothing outside the row's own `observed` values can
/// enter the plan.
pub fn project_effective_invocation(
    row: &EffectiveInvocationRow,
    binding: &ExpectedInvocationBinding,
) -> ProjectionOutcome {
    if row.subject.runner == RunnerKind::DirectFallback {
        return ProjectionOutcome::Rejected(ProjectionRejection::DirectProbeAuthority);
    }
    if !row.disposition.is_accepted() {
        return ProjectionOutcome::Rejected(ProjectionRejection::FrameNotAccepted);
    }
    if row.state != InvocationObservationState::ObservedComplete {
        return ProjectionOutcome::Rejected(ProjectionRejection::ObservationNotComplete {
            state: row.state,
        });
    }
    if let Some(field) = row.fields.first_not_observed() {
        return ProjectionOutcome::Rejected(ProjectionRejection::FieldNotObserved { field });
    }
    if let Some(rejection) = subject_rejection(&row.subject, binding) {
        return ProjectionOutcome::Rejected(ProjectionRejection::SubjectMismatch {
            detail: rejection,
        });
    }
    // The projected member comes from the observed field, so the observation
    // itself must name exactly the member its frame proved binding for.
    if row.fields.member_identity.observed().map(String::as_str)
        != Some(row.subject.parent_member_path.as_str())
    {
        return ProjectionOutcome::Rejected(ProjectionRejection::SubjectMismatch {
            detail: format!(
                "observed member identity {:?} disagrees with the frame member binding {:?}",
                row.fields.member_identity.observed(),
                row.subject.parent_member_path
            ),
        });
    }
    if let Some((field, reason)) = validate_observed_values(&row.fields) {
        return ProjectionOutcome::Rejected(ProjectionRejection::InvalidObservedValue {
            field,
            reason,
        });
    }

    let authority = match row.subject.runner {
        RunnerKind::Test => InvocationAuthority::UpstreamTest,
        RunnerKind::Harness => InvocationAuthority::UpstreamHarness,
        RunnerKind::DirectFallback => {
            return ProjectionOutcome::Rejected(ProjectionRejection::DirectProbeAuthority);
        }
    };
    let projection = CanonicalInvocationProjection {
        authority,
        member_path: row.fields.member_identity.observed().cloned().unwrap_or_default(),
        source_form: row
            .fields
            .source_form
            .observed()
            .copied()
            .unwrap_or(crate::runner_model::SourceForm::DotT),
        script_path: row.fields.script_path.observed().cloned().unwrap_or_default(),
        script_role: row
            .fields
            .script_role
            .observed()
            .copied()
            .unwrap_or(crate::invocation_trace::model::ScriptRole::Other),
        run_cwd: row.fields.run_cwd.observed().cloned().unwrap_or_default(),
        return_directory: row.fields.return_directory.observed().cloned().unwrap_or_default(),
        interpreter_switches: row
            .fields
            .interpreter_switches
            .observed()
            .cloned()
            .unwrap_or_default(),
        include_roots: row.fields.include_roots.observed().cloned().unwrap_or_default(),
        test_init: row.fields.test_init.observed().copied().unwrap_or_else(standard_test_init),
        taint_mode: row.fields.taint_mode.observed().copied().unwrap_or(TaintMode::None),
        utf8_mode: row.fields.utf8_mode.observed().copied().unwrap_or(Utf8Switch::None),
        wrapper_arguments: row.fields.wrapper_arguments.observed().cloned().unwrap_or_default(),
        script_arguments: row.fields.script_arguments.observed().cloned().unwrap_or_default(),
        environment_sha256: row
            .fields
            .environment
            .observed()
            .map(|environment| environment.sha256.clone())
            .unwrap_or_default(),
        scheduling: row.fields.scheduling.observed().cloned().unwrap_or_default(),
    };
    match canonical_projection_digest(&projection) {
        Ok(digest) => ProjectionOutcome::Projected { projection: Box::new(projection), digest },
        Err(_) => ProjectionOutcome::Rejected(ProjectionRejection::InvalidObservedValue {
            field: FieldKey::UpstreamOperation,
            reason: "projection serialization failed".to_string(),
        }),
    }
}

fn subject_rejection(
    row_subject: &RowSubjectBinding,
    binding: &ExpectedInvocationBinding,
) -> Option<String> {
    if row_subject.parent_member_path != binding.member_path {
        return Some(format!(
            "row member {} does not match the expected member {}",
            row_subject.parent_member_path, binding.member_path
        ));
    }
    if row_subject.runner != binding.runner {
        return Some(format!(
            "row runner {:?} does not match the expected runner {:?}",
            row_subject.runner, binding.runner
        ));
    }
    if row_subject.target_id != binding.target_id {
        return Some(format!(
            "row target {} does not match the expected target {}",
            row_subject.target_id, binding.target_id
        ));
    }
    if row_subject.variant_target_id != binding.variant_target_id {
        return Some("row variant target disagrees with the expected subject".to_string());
    }
    if row_subject.instrumentation_id != binding.instrumentation_id {
        return Some("row instrumentation subject disagrees with the expected subject".to_string());
    }
    None
}

/// Independent per-field validation laws for observed values. These run on
/// the projection path only; a drifted value can never borrow validity from
/// the expected subject.
fn validate_observed_values(fields: &EffectiveInvocationFields) -> Option<(FieldKey, String)> {
    if let Some(path) = fields.script_path.observed()
        && let Err(reason) = validate_relative_component_path(path, "script path")
    {
        return Some((FieldKey::ScriptPath, reason));
    }
    if let Some(path) = fields.member_identity.observed()
        && let Err(reason) = validate_relative_component_path(path, "member identity")
    {
        return Some((FieldKey::MemberIdentity, reason));
    }
    if let Some(directory) = fields.run_cwd.observed()
        && let Err(reason) = validate_relative_component_path(directory, "run cwd")
    {
        return Some((FieldKey::RunCwd, reason));
    }
    if let Some(directory) = fields.return_directory.observed()
        && let Err(reason) = validate_relative_component_path(directory, "return directory")
    {
        return Some((FieldKey::ReturnDirectory, reason));
    }
    if let Some(roots) = fields.include_roots.observed()
        && let Some(reason) = roots.iter().find_map(|root| validate_include_root(root).err())
    {
        return Some((FieldKey::IncludeRoots, reason));
    }
    if let Some(switches) = fields.interpreter_switches.observed()
        && let Some(reason) = first_invalid_argument(switches, "interpreter switch")
    {
        return Some((FieldKey::InterpreterSwitches, reason));
    }
    if let Some(arguments) = fields.wrapper_arguments.observed()
        && let Some(reason) = first_invalid_argument(arguments, "wrapper argument")
    {
        return Some((FieldKey::WrapperArguments, reason));
    }
    if let Some(arguments) = fields.script_arguments.observed()
        && let Some(reason) = first_invalid_argument(arguments, "script argument")
    {
        return Some((FieldKey::ScriptArguments, reason));
    }
    if let Some(operation) = fields.upstream_operation.observed()
        && (operation.is_empty() || operation.len() > 256)
    {
        return Some((
            FieldKey::UpstreamOperation,
            "upstream operation must be 1-256 characters".to_string(),
        ));
    }
    if let Some(environment) = fields.environment.observed() {
        let mut canonical = String::new();
        for (key, value) in &environment.variables {
            canonical.push_str(key);
            canonical.push('=');
            canonical.push_str(value);
            canonical.push('\n');
        }
        let expected = crate::build::sha256_bytes(canonical.as_bytes());
        if environment.sha256 != expected {
            return Some((
                FieldKey::Environment,
                "environment identity digest does not bind the retained variables".to_string(),
            ));
        }
    }
    None
}

fn first_invalid_argument(values: &[String], label: &str) -> Option<String> {
    values.iter().find_map(|value| {
        if value.is_empty() || value.len() > 4096 {
            return Some(format!("{label} entries must be nonempty and at most 4096 characters"));
        }
        if value.chars().any(|character| character.is_control()) {
            return Some(format!("{label} entries must not contain control characters"));
        }
        None
    })
}

fn validate_relative_component_path(path: &str, label: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > 1024 {
        return Err(format!("{label} must be nonempty and at most 1024 characters"));
    }
    if looks_absolute(path) {
        return Err(format!(
            "{label} {path} is absolute; observed invocation identity must stay checkout-root \
             independent"
        ));
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(format!(
                "{label} must be simple prepared-tree-relative components: {path}"
            ));
        }
    }
    Ok(())
}

/// Include roots are argv spellings relative to the invocation cwd (upstream
/// `t/TEST` genuinely applies `-I../lib` from `t/`), so `.`/`..` components
/// are legitimate. Only absolute host paths and malformed bytes are invalid.
fn validate_include_root(root: &str) -> Result<(), String> {
    if root.is_empty() || root.len() > 1024 {
        return Err("include roots must be nonempty and at most 1024 characters".to_string());
    }
    if looks_absolute(root) {
        return Err(format!(
            "include root {root} is absolute; observed invocation identity must stay \
             checkout-root independent"
        ));
    }
    if root.chars().any(|character| character.is_control()) {
        return Err("include roots must not contain control characters".to_string());
    }
    Ok(())
}

fn looks_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with('\\')
        || (bytes.first().is_some_and(|byte| byte.is_ascii_alphabetic())
            && bytes.get(1) == Some(&b':'))
}

#[cfg(test)]
mod contract_tests {
    //! Focused unit proof for the adapter's own laws: every rejection branch
    //! is reached directly, observed values validate independently, and the
    //! diagnostic comparison never upgrades a field.

    use super::{
        ExpectedFieldResult, ExpectedInvocationValues, ProjectionOutcome, ProjectionRejection,
        compare_expected, project_effective_invocation,
    };
    use crate::invocation_trace::model::{
        EffectiveInvocationField, FieldKey, InvocationObservationState, ScriptRole,
        TraceRowDisposition,
    };
    use crate::invocation_trace::test_support::{TraceFixture, all_observed_fields};
    use crate::runner_model::{RunnerKind, SourceForm};
    use color_eyre::eyre::Result;

    fn fixture() -> Result<TraceFixture> {
        TraceFixture::new("component_base", "t/base/if.t\n")
    }

    #[test]
    fn complete_rows_project_and_partial_rows_are_rejected_typed() -> Result<()> {
        let fixture = fixture()?;
        let mut row = fixture.row("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
        row.state = InvocationObservationState::ObservedComplete;
        let binding = fixture.expected_binding(&row);
        let projected = project_effective_invocation(&row, &binding);
        assert!(projected.is_projected());
        assert_eq!(
            projected.record(),
            crate::invocation_trace::model::ProjectionRecord::Projected {
                digest: match &projected {
                    ProjectionOutcome::Projected { digest, .. } => digest.clone(),
                    ProjectionOutcome::Rejected(_) => String::new(),
                }
            }
        );

        row.state = InvocationObservationState::ObservedPartial;
        assert_eq!(
            project_effective_invocation(&row, &binding),
            ProjectionOutcome::Rejected(ProjectionRejection::ObservationNotComplete {
                state: InvocationObservationState::ObservedPartial
            })
        );
        row.state = InvocationObservationState::NotProven;
        assert!(matches!(
            project_effective_invocation(&row, &binding),
            ProjectionOutcome::Rejected(ProjectionRejection::ObservationNotComplete { .. })
        ));
        Ok(())
    }

    #[test]
    fn missing_fields_are_never_synthesized_from_the_expected_plan() -> Result<()> {
        let fixture = fixture()?;
        // The cwd would be trivially derivable from the script path — the
        // adapter must reject instead of deriving it.
        let mut fields = all_observed_fields("t/base/if.t");
        fields.run_cwd =
            EffectiveInvocationField::NotObserved { reason: "cwd not captured".to_string() };
        let mut row = fixture.row("t/base/if.t", 0, fields);
        row.state = InvocationObservationState::ObservedComplete;
        let binding = fixture.expected_binding(&row);
        assert_eq!(
            project_effective_invocation(&row, &binding),
            ProjectionOutcome::Rejected(ProjectionRejection::FieldNotObserved {
                field: FieldKey::RunCwd
            })
        );
        Ok(())
    }

    #[test]
    fn direct_probe_routes_and_unaccepted_frames_are_rejected() -> Result<()> {
        let fixture = fixture()?;
        let mut row = fixture.row("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
        row.state = InvocationObservationState::ObservedComplete;
        let mut binding = fixture.expected_binding(&row);
        binding.runner = RunnerKind::DirectFallback;
        row.subject.runner = RunnerKind::DirectFallback;
        assert_eq!(
            project_effective_invocation(&row, &binding),
            ProjectionOutcome::Rejected(ProjectionRejection::DirectProbeAuthority)
        );

        let mut row = fixture.row("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
        row.state = InvocationObservationState::ObservedComplete;
        row.disposition = TraceRowDisposition::DuplicateRowId { row_id: row.row_id.clone() };
        let binding = fixture.expected_binding(&row);
        assert_eq!(
            project_effective_invocation(&row, &binding),
            ProjectionOutcome::Rejected(ProjectionRejection::FrameNotAccepted)
        );
        Ok(())
    }

    #[test]
    fn observed_values_validate_independently() -> Result<()> {
        let fixture = fixture()?;
        for (field, mutated) in [
            (FieldKey::ScriptPath, "/abs/script.t"),
            (FieldKey::RunCwd, "a/../b"),
            (FieldKey::IncludeRoots, "C:/lib"),
            (FieldKey::InterpreterSwitches, "ctrl\tswitch"),
        ] {
            let mut fields = all_observed_fields("t/base/if.t");
            match field {
                FieldKey::ScriptPath => {
                    fields.script_path =
                        EffectiveInvocationField::Observed { value: mutated.to_string() };
                }
                FieldKey::RunCwd => {
                    fields.run_cwd =
                        EffectiveInvocationField::Observed { value: mutated.to_string() };
                }
                FieldKey::IncludeRoots => {
                    fields.include_roots =
                        EffectiveInvocationField::Observed { value: vec![mutated.to_string()] };
                }
                FieldKey::InterpreterSwitches => {
                    fields.interpreter_switches =
                        EffectiveInvocationField::Observed { value: vec![mutated.to_string()] };
                }
                _ => {}
            }
            let mut row = fixture.row("t/base/if.t", 0, fields);
            row.state = InvocationObservationState::ObservedComplete;
            let binding = fixture.expected_binding(&row);
            match project_effective_invocation(&row, &binding) {
                ProjectionOutcome::Rejected(ProjectionRejection::InvalidObservedValue {
                    field: rejected_field,
                    ..
                }) => assert_eq!(rejected_field, field),
                other => {
                    panic!("expected invalid-observed-value rejection for {field:?}: {other:?}")
                }
            }
        }
        Ok(())
    }

    #[test]
    fn subject_mismatch_is_rejected_not_repaired() -> Result<()> {
        let fixture = fixture()?;
        let mut row = fixture.row("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
        row.state = InvocationObservationState::ObservedComplete;
        let mut binding = fixture.expected_binding(&row);
        binding.target_id = "component_comp".to_string();
        assert!(matches!(
            project_effective_invocation(&row, &binding),
            ProjectionOutcome::Rejected(ProjectionRejection::SubjectMismatch { .. })
        ));
        Ok(())
    }

    #[test]
    fn diagnostic_comparison_never_upgrades_a_field() {
        let mut fields = all_observed_fields("t/base/if.t");
        fields.run_cwd =
            EffectiveInvocationField::NotObserved { reason: "not captured".to_string() };
        fields.taint_mode = EffectiveInvocationField::Ambiguous {
            candidates: Vec::new(),
            reason: "two shebang lines".to_string(),
        };
        let expected = ExpectedInvocationValues {
            run_cwd: Some("t".to_string()),
            include_roots: Some(vec!["lib".to_string()]),
            script_path: Some("t/base/if.t".to_string()),
            ..ExpectedInvocationValues::default()
        };
        let comparisons = compare_expected(&fields, &expected);
        let by_key = |key: FieldKey| {
            comparisons.iter().find(|entry| entry.field == key).map(|entry| entry.result.clone())
        };
        assert_eq!(by_key(FieldKey::RunCwd), Some(ExpectedFieldResult::NotObserved));
        assert_eq!(by_key(FieldKey::TaintMode), Some(ExpectedFieldResult::Unresolved));
        assert_eq!(by_key(FieldKey::ScriptPath), Some(ExpectedFieldResult::ObservedEqual));
        // The comparison is a report: fields keep their states untouched.
        assert!(!fields.run_cwd.is_observed());
        assert!(fields.script_path.is_observed());
    }

    #[test]
    fn row_subject_helper_binds_the_reviewed_vocabulary() {
        let fields = all_observed_fields("t/base/if.t");
        assert_eq!(fields.script_role.observed(), Some(&ScriptRole::Base));
        assert_eq!(fields.source_form.observed(), Some(&SourceForm::DotT));
        let subject =
            crate::invocation_trace::test_support::row_subject_for("session-1", "t/base/if.t");
        assert_eq!(subject.trace_session_id, "session-1");
        assert_eq!(subject.parent_member_path, "t/base/if.t");
        assert_eq!(subject.runner, RunnerKind::Test);
    }
}
