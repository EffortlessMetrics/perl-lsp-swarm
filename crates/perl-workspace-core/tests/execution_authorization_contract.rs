#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
//! Executable contract for operation execution authorization (#11095).
//!
//! Every test states one authorization law and fails when that law is broken.
//! The two proofs named by the issue as the first red proof are
//! `explicit_action_with_workspace_supplied_executable_is_denied` and
//! `trusted_source_workspace_cannot_authorize_compile_on_save`.

use std::error::Error;
use std::io;

use perl_workspace_core::{
    ActionableAuthority, AuthorizationActor, AuthorizationEvidence, AuthorizationOutcome,
    BoundGenerations, CapabilitySet, ClassifiedInput, ClassifiedInputId,
    EXECUTION_AUTHORIZATION_SCHEMA_VERSION, EnvironmentFingerprint, EnvironmentInputAuthority,
    ExecutionCapability, ExecutionIntent, ExecutionReasonClass, InputDisposition, InputRiskClass,
    OperationProfile, OperationTrustRequirement, PolicyDenial, ProjectEnvironmentSnapshotBuilder,
    SessionOverride, TrustScope, TrustScopeKind, WorkspaceTrust, authorize, operation_registry,
};

fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if condition { Ok(()) } else { Err(io::Error::other(message).into()) }
}

/// Bind generations to a real environment snapshot identity, so the contract is
/// exercised against the #7419 model rather than a fabricated fingerprint.
fn environment_fingerprint(
    workspace: &str,
    generation: u64,
) -> Result<EnvironmentFingerprint, Box<dyn Error>> {
    let snapshot =
        ProjectEnvironmentSnapshotBuilder::new(workspace, generation, WorkspaceTrust::Trusted)
            .build()?;
    Ok(snapshot.fingerprint)
}

fn generations(workspace: &str, policy: u64) -> Result<BoundGenerations, Box<dyn Error>> {
    Ok(BoundGenerations::new(7, policy, 11, environment_fingerprint(workspace, 7)?))
}

fn verified_tool() -> ClassifiedInput {
    ClassifiedInput::new(
        "tool.interpreter",
        InputRiskClass::SelectedVerifiedTool,
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::Accepted,
        None,
        "explicitly_selected_interpreter",
    )
}

fn project_supplied_executable() -> ClassifiedInput {
    ClassifiedInput::new(
        "tool.interpreter",
        InputRiskClass::ProjectExecutableOrCommand,
        EnvironmentInputAuthority::TrustedProjectConfiguration,
        InputDisposition::RequiresSeparateAuthority,
        None,
        "project_supplied_interpreter",
    )
}

fn path_only_tool() -> ClassifiedInput {
    ClassifiedInput::new(
        "tool.formatter",
        InputRiskClass::AmbientPathOrCwd,
        EnvironmentInputAuthority::Ambient,
        InputDisposition::RequiresSeparateAuthority,
        None,
        "resolved_from_path",
    )
}

fn evidence(
    scope: &TrustScope,
    trust: WorkspaceTrust,
    actor: AuthorizationActor,
    generations: &BoundGenerations,
    inputs: Vec<ClassifiedInput>,
) -> AuthorizationEvidence {
    AuthorizationEvidence {
        scope: scope.clone(),
        trust,
        actor,
        generations: generations.clone(),
        inputs,
        session_override: None,
        policy_denials: Vec::new(),
        limitation_codes: Vec::new(),
    }
}

fn intent(
    profile: OperationProfile,
    reason_class: ExecutionReasonClass,
    scope: &TrustScope,
    generations: &BoundGenerations,
    input_ids: Vec<ClassifiedInputId>,
) -> ExecutionIntent {
    ExecutionIntent {
        profile,
        reason_class,
        scope: scope.clone(),
        generations: generations.clone(),
        requested: OperationTrustRequirement::for_profile(profile).required,
        input_ids,
        claim_boundary: "one operation, this scope, these inputs".to_string(),
    }
}

fn ids(inputs: &[ClassifiedInput]) -> Vec<ClassifiedInputId> {
    inputs.iter().map(|input| input.id.clone()).collect()
}

fn has_reason(decision: &perl_workspace_core::ExecutionAuthorizationDecision, code: &str) -> bool {
    decision.reasons().iter().any(|reason| reason.code == code)
}

// ---------------------------------------------------------------------------
// First red proofs named by #11095
// ---------------------------------------------------------------------------

/// An explicit user action does not upgrade a project-controlled executable.
#[test]
fn explicit_action_with_workspace_supplied_executable_is_denied() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 3)?;
    let tool = project_supplied_executable();
    let evidence = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "run.button".to_string() },
        &bound,
        vec![tool.clone()],
    );
    let intent = intent(
        OperationProfile::RunCurrentSavedFile,
        ExecutionReasonClass::ExplicitUserAction,
        &scope,
        &bound,
        vec![tool.id.clone()],
    );

    let decision = authorize(&intent, &evidence);
    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "explicit action over a project-supplied executable must be denied",
    )?;
    require(decision.granted().is_empty(), "a denied decision must grant no capability")?;
    require(
        has_reason(&decision, "project_supplied_executable"),
        "the denial must name the project-supplied executable",
    )?;
    Ok(())
}

/// Trusted source is not trusted cadence: compile-on-save needs its own opt-in.
#[test]
fn trusted_source_workspace_cannot_authorize_compile_on_save() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 3)?;
    let tool = verified_tool();
    let evidence = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "save".to_string() },
        &bound,
        vec![tool.clone()],
    );

    let manual = intent(
        OperationProfile::PerlCompileCurrentSavedFile,
        ExecutionReasonClass::ExplicitUserAction,
        &scope,
        &bound,
        vec![tool.id.clone()],
    );
    let on_save = intent(
        OperationProfile::TrustedCompileOnSave,
        ExecutionReasonClass::TrustedPostSave,
        &scope,
        &bound,
        vec![tool.id.clone()],
    );

    let manual_decision = authorize(&manual, &evidence);
    let on_save_decision = authorize(&on_save, &evidence);

    require(
        manual_decision.outcome() == AuthorizationOutcome::Allowed,
        "a manual compile under the same evidence must be allowed",
    )?;
    require(
        on_save_decision.outcome() != AuthorizationOutcome::Allowed,
        "compile-on-save must not ride on manual-compile authority",
    )?;
    require(
        has_reason(&on_save_decision, "cadence_not_authorized"),
        "compile-on-save must name the missing cadence authority",
    )?;
    require(
        !on_save_decision.granted().contains(ExecutionCapability::PersistentCadence),
        "cadence must not be granted without its own authority",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Fixture matrix
// ---------------------------------------------------------------------------

/// A restricted workspace can still be parsed and indexed.
#[test]
fn restricted_workspace_still_allows_source_analysis() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let evidence =
        evidence(&scope, WorkspaceTrust::Untrusted, AuthorizationActor::None, &bound, Vec::new());
    let intent = intent(
        OperationProfile::SourceAnalysisOnly,
        ExecutionReasonClass::Probe,
        &scope,
        &bound,
        Vec::new(),
    );

    let decision = authorize(&intent, &evidence);
    require(
        decision.outcome() == AuthorizationOutcome::Allowed,
        "source analysis must not require execution authority",
    )?;
    require(
        decision.permits(ExecutionCapability::SourceAnalysis),
        "source analysis capability must be granted",
    )?;
    require(
        !decision.permits(ExecutionCapability::ProjectCodeExecution),
        "source analysis must not carry execution authority",
    )?;
    Ok(())
}

/// Trusted source configuration does not by itself authorize execution.
#[test]
fn trusted_workspace_without_actor_requires_confirmation() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let evidence = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::None,
        &bound,
        vec![tool.clone()],
    );
    let intent = intent(
        OperationProfile::RunCurrentSavedFile,
        ExecutionReasonClass::ProjectRunner,
        &scope,
        &bound,
        vec![tool.id.clone()],
    );

    let decision = authorize(&intent, &evidence);
    require(
        decision.outcome() == AuthorizationOutcome::ConfirmationRequired,
        "trust without an explicit actor must not silently execute project code",
    )?;
    require(
        has_reason(&decision, "no_explicit_actor"),
        "the outcome must name the missing explicit actor",
    )?;
    require(decision.granted().is_empty(), "a non-permitting outcome grants nothing")?;
    Ok(())
}

/// A trusted, explicitly requested run over a user-selected interpreter runs.
#[test]
fn explicit_trusted_run_over_user_selected_interpreter_is_allowed() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let evidence = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
        &bound,
        vec![tool.clone()],
    );
    let intent = intent(
        OperationProfile::RunCurrentSavedFile,
        ExecutionReasonClass::ExplicitUserAction,
        &scope,
        &bound,
        vec![tool.id.clone()],
    );

    let decision = authorize(&intent, &evidence);
    require(decision.outcome() == AuthorizationOutcome::Allowed, "explicit trusted run allowed")?;
    require(
        decision.granted().contains_all(
            &OperationTrustRequirement::for_profile(OperationProfile::RunCurrentSavedFile).required,
        ),
        "every required capability must be granted",
    )?;
    Ok(())
}

/// Prelaunch validation and launching a debuggee are different requirements.
#[test]
fn dap_prelaunch_and_debuggee_requirements_differ() -> Result<(), Box<dyn Error>> {
    let prelaunch =
        OperationTrustRequirement::for_profile(OperationProfile::DapPrelaunchCheck).required;
    let debuggee =
        OperationTrustRequirement::for_profile(OperationProfile::DapDebuggeeOrHelper).required;
    require(
        !prelaunch.contains(ExecutionCapability::ProjectCodeExecution),
        "prelaunch validation must not require project code execution",
    )?;
    require(
        debuggee.contains(ExecutionCapability::ProjectCodeExecution)
            && debuggee.contains(ExecutionCapability::InteractiveSession),
        "launching a debuggee must require execution and an interactive session",
    )?;

    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let evidence = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::None,
        &bound,
        vec![tool.clone()],
    );

    let prelaunch_decision = authorize(
        &intent(
            OperationProfile::DapPrelaunchCheck,
            ExecutionReasonClass::DapPrelaunch,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &evidence,
    );
    let debuggee_decision = authorize(
        &intent(
            OperationProfile::DapDebuggeeOrHelper,
            ExecutionReasonClass::DapPrelaunch,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &evidence,
    );

    require(
        prelaunch_decision.outcome() == AuthorizationOutcome::Allowed,
        "prelaunch validation is allowed on the same evidence",
    )?;
    require(
        debuggee_decision.outcome() != AuthorizationOutcome::Allowed,
        "the same evidence must not launch a debuggee",
    )?;
    Ok(())
}

/// A tool found only on PATH is weaker than an explicitly selected one.
#[test]
fn path_only_tool_is_weaker_than_explicit_selection() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;

    let ambient = path_only_tool();
    let ambient_decision = authorize(
        &intent(
            OperationProfile::ExternalFormatter,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            vec![ambient.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
            &bound,
            vec![ambient],
        ),
    );

    let explicit = verified_tool();
    let explicit_decision = authorize(
        &intent(
            OperationProfile::ExternalFormatter,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            vec![explicit.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
            &bound,
            vec![explicit],
        ),
    );

    require(
        ambient_decision.outcome() == AuthorizationOutcome::ConfirmationRequired,
        "a PATH-resolved tool must require confirmation",
    )?;
    require(
        has_reason(&ambient_decision, "ambient_tool_selection"),
        "the confirmation must name ambient tool selection",
    )?;
    require(
        explicit_decision.outcome() == AuthorizationOutcome::Allowed,
        "an explicitly selected tool is allowed",
    )?;
    Ok(())
}

/// An external absolute include root is withheld; a workspace-relative one is not.
#[test]
fn external_include_root_is_withheld_as_limited() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;

    let external = ClassifiedInput::new(
        "include.root",
        InputRiskClass::ExternalAbsolutePath,
        EnvironmentInputAuthority::WorkspaceConvention,
        InputDisposition::ConfirmationRequired,
        None,
        "external_include_root",
    );
    let contained = ClassifiedInput::new(
        "include.root",
        InputRiskClass::WorkspaceContainedPath,
        EnvironmentInputAuthority::WorkspaceConvention,
        InputDisposition::Accepted,
        None,
        "workspace_include_root",
    );

    let requested = CapabilitySet::new([
        ExecutionCapability::SourceAnalysis,
        ExecutionCapability::ExternalRead,
        ExecutionCapability::OutsideRootPath,
    ]);

    let mut external_intent = intent(
        OperationProfile::ModuleResolutionExternalRead,
        ExecutionReasonClass::Probe,
        &scope,
        &bound,
        vec![external.id.clone()],
    );
    external_intent.requested = requested.clone();
    let external_decision = authorize(
        &external_intent,
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::None,
            &bound,
            vec![external],
        ),
    );

    let mut contained_intent = intent(
        OperationProfile::ModuleResolutionExternalRead,
        ExecutionReasonClass::Probe,
        &scope,
        &bound,
        vec![contained.id.clone()],
    );
    contained_intent.requested = requested;
    let contained_decision = authorize(
        &contained_intent,
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::None,
            &bound,
            vec![contained],
        ),
    );

    require(
        external_decision.outcome() == AuthorizationOutcome::AllowedLimited,
        "an unconfirmed external root yields a limited allow",
    )?;
    require(
        external_decision.omitted().contains(ExecutionCapability::OutsideRootPath),
        "the limited allow must name the withheld outside-root capability",
    )?;
    require(
        !external_decision.permits(ExecutionCapability::OutsideRootPath),
        "a withheld capability must not be permitted",
    )?;
    require(
        contained_decision.outcome() == AuthorizationOutcome::Allowed,
        "a workspace-contained root needs no outside-root authority",
    )?;
    Ok(())
}

/// A symlink or traversal path escaping the root is denied, never confirmed.
#[test]
fn traversal_path_is_denied_not_confirmable() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let escaping = ClassifiedInput::new(
        "include.root",
        InputRiskClass::SymlinkOrTraversalPath,
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::Denied,
        None,
        "symlink_escapes_root",
    );

    let mut probe = intent(
        OperationProfile::ModuleResolutionExternalRead,
        ExecutionReasonClass::Probe,
        &scope,
        &bound,
        vec![escaping.id.clone()],
    );
    probe.requested = CapabilitySet::new([
        ExecutionCapability::SourceAnalysis,
        ExecutionCapability::ExternalRead,
        ExecutionCapability::OutsideRootPath,
    ]);

    let decision = authorize(
        &probe,
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "resolve".to_string() },
            &bound,
            vec![escaping],
        ),
    );

    require(
        !decision.permits(ExecutionCapability::OutsideRootPath),
        "a traversal path must never grant outside-root authority",
    )?;
    require(
        has_reason(&decision, "path_escapes_root"),
        "the decision must name the escaping path",
    )?;
    Ok(())
}

/// Ambient Perl environment is denied; explicit reviewed activation is separate.
#[test]
fn ambient_perl_environment_is_denied_but_explicit_activation_is_not() -> Result<(), Box<dyn Error>>
{
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();

    let ambient_env = ClassifiedInput::new(
        "environment.perl5lib",
        InputRiskClass::AmbientPerlEnvironment,
        EnvironmentInputAuthority::Ambient,
        InputDisposition::Denied,
        None,
        "ambient_perl5lib",
    );
    let explicit_env = ClassifiedInput::new(
        "environment.perl5lib",
        InputRiskClass::AmbientPerlEnvironment,
        EnvironmentInputAuthority::ExplicitEnvironment,
        InputDisposition::Accepted,
        None,
        "reviewed_activation",
    );

    let ambient_decision = authorize(
        &intent(
            OperationProfile::PerlCompileCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            ids(&[tool.clone(), ambient_env.clone()]),
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "compile".to_string() },
            &bound,
            vec![tool.clone(), ambient_env],
        ),
    );
    let explicit_decision = authorize(
        &intent(
            OperationProfile::PerlCompileCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            ids(&[tool.clone(), explicit_env.clone()]),
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "compile".to_string() },
            &bound,
            vec![tool, explicit_env],
        ),
    );

    require(
        ambient_decision.outcome() == AuthorizationOutcome::Denied,
        "ambient PERL5LIB must not supply code-loading authority",
    )?;
    require(
        has_reason(&ambient_decision, "ambient_environment_denied"),
        "the denial must name the ambient environment",
    )?;
    require(
        explicit_decision.outcome() == AuthorizationOutcome::Allowed,
        "an explicitly reviewed activation is a different input and is allowed",
    )?;
    Ok(())
}

/// Policy denial dominates a trusted, explicitly requested execution.
#[test]
fn policy_denial_dominates_trusted_execution() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let mut facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
        &bound,
        vec![tool.clone()],
    );
    facts.policy_denials = vec![PolicyDenial::new(
        "org.policy.no-project-execution",
        CapabilitySet::new([ExecutionCapability::ProjectCodeExecution]),
        "administrator_denied_execution",
    )];

    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &facts,
    );

    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "policy denial must dominate trusted execution",
    )?;
    require(has_reason(&decision, "policy_denied"), "the denial must name the policy")?;
    require(
        decision
            .reasons()
            .iter()
            .any(|reason| reason.actionable_authority == ActionableAuthority::PolicyAdministrator),
        "the explanation must point at the administrator policy",
    )?;
    Ok(())
}

/// A scoped override supplies a capability only while it is current.
#[test]
fn session_override_supplies_capability_until_it_expires() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let ambient = path_only_tool();

    let build = |policy_generation: u64| -> Result<_, Box<dyn Error>> {
        let bound = generations("ws", policy_generation)?;
        let mut facts = evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
            &bound,
            vec![ambient.clone()],
        );
        facts.session_override = Some(SessionOverride {
            override_id: "session.grant.1".to_string(),
            scope: scope.clone(),
            granted_policy_generation: 5,
            expires_after_policy_generation: 6,
            capabilities: CapabilitySet::new([ExecutionCapability::ExecutableTool]),
        });
        let request = intent(
            OperationProfile::ExternalFormatter,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            vec![ambient.id.clone()],
        );
        Ok(authorize(&request, &facts))
    };

    let current = build(5)?;
    let expired = build(9)?;

    require(
        current.outcome() == AuthorizationOutcome::Allowed,
        "a current scoped override supplies the capability",
    )?;
    require(
        has_reason(&current, "granted_by_session_override"),
        "the allow must record that an override supplied it",
    )?;
    require(
        current.revalidation().override_expires_after_policy_generation == Some(6),
        "the decision must carry the override expiry",
    )?;
    require(
        expired.outcome() != AuthorizationOutcome::Allowed,
        "a lapsed override must not supply the capability",
    )?;
    require(
        has_reason(&expired, "session_override_not_current"),
        "the outcome must say the override is not current",
    )?;
    Ok(())
}

/// Roots carrying different generations are different authorization subjects.
#[test]
fn distinct_roots_and_generations_are_distinct_subjects() -> Result<(), Box<dyn Error>> {
    let root_a = TrustScope::editor_workspace("ws").with_root("root-a");
    let root_b = TrustScope::editor_workspace("ws").with_root("root-b");
    let bound_a = generations("ws", 1)?;
    let bound_b = generations("ws", 2)?;

    let tool = verified_tool();
    let decision_a = authorize(
        &intent(
            OperationProfile::RunTests,
            ExecutionReasonClass::TestRun,
            &root_a,
            &bound_a,
            vec![tool.id.clone()],
        ),
        &evidence(
            &root_a,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "test".to_string() },
            &bound_a,
            vec![tool.clone()],
        ),
    );

    require(
        decision_a.outcome() == AuthorizationOutcome::Allowed,
        "the trusted root authorizes its own test run",
    )?;
    require(
        !decision_a.is_current_for(&root_b, &bound_a),
        "a decision for one root must not apply to another",
    )?;
    require(
        !decision_a.is_current_for(&root_a, &bound_b),
        "a decision must not survive a policy generation move",
    )?;
    require(
        decision_a.is_current_for(&root_a, &bound_a),
        "a decision applies to its own root and generations",
    )?;
    Ok(())
}

/// A hermetic CI operation is authorized by CI identity, not workspace trust.
#[test]
fn ci_hermetic_operation_is_independent_of_workspace_trust() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::ci_hermetic("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let decision = authorize(
        &intent(
            OperationProfile::CiHermeticProcess,
            ExecutionReasonClass::CiHermetic,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &evidence(
            &scope,
            // Deliberately undecided: CI authority must not read this at all.
            WorkspaceTrust::Unknown,
            AuthorizationActor::CiIdentity { identity_id: "ci://runner/7".to_string() },
            &bound,
            vec![tool],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Allowed,
        "a CI identity authorizes a hermetic process regardless of workspace trust",
    )?;
    require(
        decision.scope().kind == TrustScopeKind::CiHermetic,
        "the decision stays bound to the hermetic scope",
    )?;
    Ok(())
}

/// An input with unknown provenance is never quietly accepted.
#[test]
fn unknown_provenance_is_not_proven() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let unknown = ClassifiedInput::new(
        "tool.interpreter",
        InputRiskClass::UnknownProvenance,
        EnvironmentInputAuthority::Ambient,
        InputDisposition::UnknownNotProven,
        None,
        "provenance_unavailable",
    );

    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![unknown.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![unknown],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::NotProven,
        "unknown provenance must be not-proven, never accepted",
    )?;
    require(
        has_reason(&decision, "unknown_provenance"),
        "the outcome must name the unknown provenance",
    )?;
    require(decision.granted().is_empty(), "not-proven grants nothing")?;
    Ok(())
}

/// Evidence from a moved generation is stale, not merely weaker.
#[test]
fn moved_generation_makes_evidence_stale() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let requested_at = generations("ws", 1)?;
    let observed_at = generations("ws", 2)?;
    let tool = verified_tool();

    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &requested_at,
            vec![tool.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &observed_at,
            vec![tool],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Stale,
        "generation movement must produce a stale outcome",
    )?;
    require(has_reason(&decision, "generation_moved"), "the outcome must name the movement")?;
    require(decision.granted().is_empty(), "a stale decision grants nothing")?;
    Ok(())
}

/// Identity does not depend on the order facts were collected in.
#[test]
fn identity_is_stable_under_input_ordering() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let env = ClassifiedInput::new(
        "environment.perl5lib",
        InputRiskClass::AmbientPerlEnvironment,
        EnvironmentInputAuthority::ExplicitEnvironment,
        InputDisposition::Accepted,
        None,
        "reviewed_activation",
    );

    let forward = authorize(
        &intent(
            OperationProfile::PerlCompileCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![tool.id.clone(), env.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "compile".to_string() },
            &bound,
            vec![tool.clone(), env.clone()],
        ),
    );
    let reversed = authorize(
        &intent(
            OperationProfile::PerlCompileCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![env.id.clone(), tool.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "compile".to_string() },
            &bound,
            vec![env, tool],
        ),
    );

    require(
        forward.fingerprint() == reversed.fingerprint(),
        "input ordering must not change decision identity",
    )?;
    require(
        forward.public_explanation() == reversed.public_explanation(),
        "input ordering must not change the public explanation",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls
// ---------------------------------------------------------------------------

/// One acceptance does not generalize across operations.
#[test]
fn acceptance_for_one_operation_does_not_authorize_another() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
        &bound,
        vec![tool.clone()],
    );

    let formatter = authorize(
        &intent(
            OperationProfile::ExternalFormatter,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &facts,
    );
    let interactive = authorize(
        &intent(
            OperationProfile::InteractiveExternalSession,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &facts,
    );

    require(
        formatter.outcome() == AuthorizationOutcome::Allowed,
        "the formatter is allowed on this evidence",
    )?;
    require(
        formatter.fingerprint() != interactive.fingerprint(),
        "two operations must not share one decision identity",
    )?;
    require(
        interactive.granted() != formatter.granted()
            || interactive.outcome() != formatter.outcome(),
        "a stronger operation must not inherit the weaker one's authorization",
    )?;
    Ok(())
}

/// An explicit user action does not bypass denied project configuration.
#[test]
fn user_action_does_not_bypass_untrusted_project_configuration() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let decision = authorize(
        &intent(
            OperationProfile::RunProjectCommand,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Untrusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![tool],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "an explicit action cannot authorize an untrusted project command",
    )?;
    require(
        has_reason(&decision, "workspace_untrusted"),
        "the denial must name the missing workspace trust",
    )?;
    Ok(())
}

/// A workspace-scoped setting cannot manufacture user or machine authority.
#[test]
fn workspace_scoped_setting_cannot_grant_user_authority() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    // The setting *claims* to enable an on-save cadence, but its provenance is
    // the workspace itself.
    let workspace_setting = ClassifiedInput::new(
        "cadence.compile_on_save",
        InputRiskClass::WorkspaceScopedSetting,
        EnvironmentInputAuthority::TrustedProjectConfiguration,
        InputDisposition::Accepted,
        None,
        "workspace_requested_on_save",
    );

    let decision = authorize(
        &intent(
            OperationProfile::TrustedCompileOnSave,
            ExecutionReasonClass::TrustedPostSave,
            &scope,
            &bound,
            ids(&[tool.clone(), workspace_setting.clone()]),
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "save".to_string() },
            &bound,
            vec![tool, workspace_setting],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "a workspace-scoped setting must not grant user-scoped cadence authority",
    )?;
    require(
        has_reason(&decision, "workspace_setting_cannot_grant_user_authority"),
        "the denial must name the provenance escalation",
    )?;
    Ok(())
}

/// A user-scoped opt-in does grant the cadence the workspace could not.
#[test]
fn user_scoped_setting_grants_cadence() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let user_setting = ClassifiedInput::new(
        "cadence.compile_on_save",
        InputRiskClass::UserScopedSetting,
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::Accepted,
        None,
        "user_enabled_on_save",
    );

    let decision = authorize(
        &intent(
            OperationProfile::TrustedCompileOnSave,
            ExecutionReasonClass::TrustedPostSave,
            &scope,
            &bound,
            ids(&[tool.clone(), user_setting.clone()]),
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "save".to_string() },
            &bound,
            vec![tool, user_setting],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Allowed,
        "an explicit user-scoped opt-in authorizes the cadence",
    )?;
    require(
        decision.permits(ExecutionCapability::PersistentCadence),
        "the cadence capability must be granted",
    )?;
    Ok(())
}

/// A session override cannot defeat a policy denial.
#[test]
fn session_override_cannot_defeat_policy_denial() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 5)?;
    let tool = verified_tool();
    let mut facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
        &bound,
        vec![tool.clone()],
    );
    facts.policy_denials = vec![PolicyDenial::new(
        "org.policy.no-tools",
        CapabilitySet::new([ExecutionCapability::ExecutableTool]),
        "administrator_denied_tools",
    )];
    facts.session_override = Some(SessionOverride {
        override_id: "session.grant.1".to_string(),
        scope: scope.clone(),
        granted_policy_generation: 0,
        expires_after_policy_generation: u64::MAX,
        capabilities: CapabilitySet::new([ExecutionCapability::ExecutableTool]),
    });

    let decision = authorize(
        &intent(
            OperationProfile::ExternalFormatter,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &facts,
    );

    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "policy denial must beat an unexpired session override",
    )?;
    require(
        !has_reason(&decision, "granted_by_session_override"),
        "an override must not be recorded as supplying a policy-denied capability",
    )?;
    Ok(())
}

/// CI authority is never synthesized from editor workspace trust.
#[test]
fn ci_authority_cannot_be_synthesized_from_editor_trust() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let decision = authorize(
        &intent(
            OperationProfile::CiHermeticProcess,
            ExecutionReasonClass::CiHermetic,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![tool],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "a trusted editor workspace must not authorize a hermetic CI process",
    )?;
    require(
        has_reason(&decision, "ci_authority_not_synthesizable"),
        "the denial must name the missing CI authority",
    )?;
    Ok(())
}

/// A caller cannot declare less authority than the operation uses.
#[test]
fn under_declared_capabilities_are_unsupported() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let mut request = intent(
        OperationProfile::RunCurrentSavedFile,
        ExecutionReasonClass::ExplicitUserAction,
        &scope,
        &bound,
        vec![tool.id.clone()],
    );
    // Ask only for source analysis while running a file.
    request.requested = CapabilitySet::new([ExecutionCapability::SourceAnalysis]);

    let decision = authorize(
        &request,
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![tool],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Unsupported,
        "under-declaring capabilities must not be evaluable as an allow",
    )?;
    require(
        has_reason(&decision, "under_declared_capabilities"),
        "the outcome must name the under-declaration",
    )?;
    require(decision.granted().is_empty(), "an unsupported request grants nothing")?;
    Ok(())
}

/// Every non-permitting outcome grants exactly nothing.
#[test]
fn non_permitting_outcomes_never_grant_capabilities() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let cases = [
        (WorkspaceTrust::Untrusted, AuthorizationActor::None),
        (WorkspaceTrust::Unknown, AuthorizationActor::None),
        (
            WorkspaceTrust::Untrusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
        ),
    ];

    for (trust, actor) in cases {
        let tool = project_supplied_executable();
        let decision = authorize(
            &intent(
                OperationProfile::RunCurrentSavedFile,
                ExecutionReasonClass::ExplicitUserAction,
                &scope,
                &bound,
                vec![tool.id.clone()],
            ),
            &evidence(&scope, trust, actor, &bound, vec![tool]),
        );
        require(
            !decision.outcome().permits_execution(),
            "these fixtures must not permit execution",
        )?;
        require(
            decision.granted().is_empty() && decision.omitted().is_empty(),
            "a non-permitting outcome must carry neither granted nor omitted capabilities",
        )?;
        require(
            !decision.permits(ExecutionCapability::ProjectCodeExecution),
            "a non-permitting outcome permits nothing",
        )?;
    }
    Ok(())
}

/// A public explanation carries no raw path, environment value, or secret.
#[test]
fn public_explanation_is_redacted() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    // These caller-authored strings would leak if any of them reached the
    // public projection.
    let secret = ClassifiedInput::new(
        "environment.token",
        InputRiskClass::SecretBearingValue,
        EnvironmentInputAuthority::Ambient,
        InputDisposition::Denied,
        None,
        "SECRET-VALUE-/home/someone/.perlbrew/bin/perl",
    );
    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![secret.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![secret],
        ),
    );

    let rendered = serde_json::to_string(&decision.public_explanation())?;
    require(
        !rendered.contains("SECRET-VALUE"),
        "a caller explanation code must not reach the public explanation",
    )?;
    require(
        !rendered.contains("/home/someone"),
        "a raw path must not reach the public explanation",
    )?;
    require(
        !rendered.contains("environment.token"),
        "a raw semantic key must not reach the public explanation",
    )?;
    require(
        rendered.contains("non_claim.process_plan_safety"),
        "the public explanation must keep the non-claims visible",
    )?;
    Ok(())
}

/// A blocked public explanation still names the capability and what to change.
#[test]
fn public_explanation_stays_actionable() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = project_supplied_executable();
    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![tool],
        ),
    );

    let explanation = decision.public_explanation();
    require(
        explanation.blocked_capabilities.iter().any(|tag| tag == "executable_tool"),
        "the explanation must name the blocked capability",
    )?;
    require(
        explanation.actionable_authorities.iter().any(|tag| tag == "user_configuration"),
        "the explanation must name the authority the user can act on",
    )?;
    require(
        explanation.schema_version == EXECUTION_AUTHORIZATION_SCHEMA_VERSION,
        "the explanation must carry the schema version",
    )?;
    Ok(())
}

/// A decision that crosses a transport boundary is revalidated, not trusted.
#[test]
fn transported_decision_is_revalidated() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![tool],
        ),
    );

    let encoded = serde_json::to_string(&decision)?;
    let round_tripped: perl_workspace_core::ExecutionAuthorizationDecision =
        serde_json::from_str(&encoded)?;
    require(round_tripped == decision, "a decision must survive an exact round trip")?;

    // Widen the granted set in transit; the fingerprint no longer matches.
    let widened = encoded.replace(
        "\"granted\":[\"source_analysis\",\"executable_tool\",\"project_code_execution\"]",
        "\"granted\":[\"source_analysis\",\"executable_tool\",\"project_code_execution\",\"persistent_cadence\"]",
    );
    require(widened != encoded, "the widening rewrite must actually apply")?;
    let forged: Result<perl_workspace_core::ExecutionAuthorizationDecision, _> =
        serde_json::from_str(&widened);
    require(forged.is_err(), "a widened granted set must fail revalidation")?;
    Ok(())
}

/// The registry is complete, versioned, and free of free-form entries.
#[test]
fn operation_registry_is_complete_and_versioned() -> Result<(), Box<dyn Error>> {
    let registry = operation_registry();
    require(
        registry.len() == OperationProfile::ALL.len(),
        "every reviewed profile must have exactly one registry row",
    )?;

    // Two profiles must never share one requirement identity, or a caller
    // could present the weaker profile's authorization for the stronger one.
    let mut identities: Vec<String> = OperationProfile::ALL
        .iter()
        .map(|profile| OperationTrustRequirement::for_profile(*profile).identity())
        .collect();
    identities.sort();
    let distinct = identities.len();
    identities.dedup();
    require(
        identities.len() == distinct,
        "each profile must have a distinct requirement identity",
    )?;

    for profile in OperationProfile::ALL {
        let requirement = OperationTrustRequirement::for_profile(profile);
        require(
            registry.contains_key(profile.identity_tag()),
            "each profile must be reachable by its stable tag",
        )?;
        require(!requirement.required.is_empty(), "no profile may require zero authority")?;
        require(
            requirement.registry_version == perl_workspace_core::OPERATION_REGISTRY_VERSION,
            "each row must carry the registry version",
        )?;
        require(
            requirement.non_claims.iter().any(|claim| claim == "non_claim.process_plan_safety"),
            "every row must disclaim process-plan safety",
        )?;
        // A silent capability change must move the identity, so a stale
        // authorization cannot be replayed against a redefined profile.
        let mut widened = requirement.clone();
        widened.required = CapabilitySet::new(
            requirement.required.iter().chain([ExecutionCapability::OutsideRootPath]),
        );
        require(
            widened.required == requirement.required
                || widened.identity() != requirement.identity(),
            "changing a profile's required capabilities must change its identity",
        )?;
    }
    Ok(())
}

/// Every execution-bearing capability needs an authority beyond source trust.
#[test]
fn execution_bearing_capabilities_are_never_implicit() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    // Trusted workspace, explicit action, but no inputs at all.
    let facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
        &bound,
        Vec::new(),
    );

    for profile in OperationProfile::ALL {
        let requirement = OperationTrustRequirement::for_profile(profile);
        if !requirement.required.contains(ExecutionCapability::ExecutableTool) {
            continue;
        }
        let decision = authorize(
            &intent(profile, ExecutionReasonClass::ExplicitUserAction, &scope, &bound, Vec::new()),
            &facts,
        );
        require(
            !decision.outcome().permits_execution(),
            "no profile requiring a tool may be allowed with no tool evidence",
        )?;
    }
    Ok(())
}

/// Ambient state is not opt-in: omitting it from the intent cannot widen authority.
///
/// Regression control for a real escalation found in self-review. Declaring
/// only the verified tool, while ambient `PERL5LIB` sat in the evidence, turned
/// a correct `Denied` into `Allowed`.
#[test]
fn omitting_ambient_input_from_intent_does_not_widen_authority() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let ambient_env = ClassifiedInput::new(
        "environment.perl5lib",
        InputRiskClass::AmbientPerlEnvironment,
        EnvironmentInputAuthority::Ambient,
        InputDisposition::Denied,
        None,
        "ambient_perl5lib",
    );
    let facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "compile".to_string() },
        &bound,
        vec![tool.clone(), ambient_env.clone()],
    );

    let declared = authorize(
        &intent(
            OperationProfile::PerlCompileCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![tool.id.clone(), ambient_env.id.clone()],
        ),
        &facts,
    );
    // The same evidence, but the intent names only the tool.
    let undeclared = authorize(
        &intent(
            OperationProfile::PerlCompileCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &facts,
    );

    require(
        declared.outcome() == AuthorizationOutcome::Denied,
        "declaring the ambient environment must deny",
    )?;
    require(
        undeclared.outcome() == AuthorizationOutcome::Denied,
        "omitting the ambient environment must not turn a denial into an allow",
    )?;
    require(
        has_reason(&undeclared, "ambient_environment_denied"),
        "the undeclared case must still name the ambient environment",
    )?;
    Ok(())
}

/// The inescapable-input rule stays narrow: a slot-specific denial does not
/// leak across unrelated operations.
#[test]
fn undeclared_slot_specific_input_does_not_block_another_operation() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    // A denied, project-supplied formatter that this test run does not consume.
    let denied_formatter = ClassifiedInput::new(
        "tool.formatter",
        InputRiskClass::ProjectExecutableOrCommand,
        EnvironmentInputAuthority::TrustedProjectConfiguration,
        InputDisposition::Denied,
        None,
        "project_supplied_formatter",
    );

    let decision = authorize(
        &intent(
            OperationProfile::RunTests,
            ExecutionReasonClass::TestRun,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "test".to_string() },
            &bound,
            vec![tool, denied_formatter],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Allowed,
        "an unrelated denied tool must not block a test run that does not use it",
    )?;
    Ok(())
}

/// A traversal path in scope cannot be escaped by leaving it undeclared.
///
/// Second regression control of the same class as
/// `omitting_ambient_input_from_intent_does_not_widen_authority`, found by an
/// independent review lens. `OutsideRootPath` is blanket authority to leave the
/// workspace root, so granting it while a denied traversal path sits in the
/// evidence would hand a consumer exactly the path that was refused.
#[test]
fn omitting_traversal_path_does_not_grant_outside_root_authority() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let escaping = ClassifiedInput::new(
        "include.root",
        InputRiskClass::SymlinkOrTraversalPath,
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::Denied,
        None,
        "symlink_escapes_root",
    );
    let facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "resolve".to_string() },
        &bound,
        vec![tool.clone(), escaping.clone()],
    );

    let requested = CapabilitySet::new([
        ExecutionCapability::SourceAnalysis,
        ExecutionCapability::ExternalRead,
        ExecutionCapability::OutsideRootPath,
    ]);

    let mut declared = intent(
        OperationProfile::ModuleResolutionExternalRead,
        ExecutionReasonClass::Probe,
        &scope,
        &bound,
        vec![tool.id.clone(), escaping.id.clone()],
    );
    declared.requested = requested.clone();

    // The same evidence, but the intent leaves the traversal path out.
    let mut undeclared = intent(
        OperationProfile::ModuleResolutionExternalRead,
        ExecutionReasonClass::Probe,
        &scope,
        &bound,
        vec![tool.id.clone()],
    );
    undeclared.requested = requested;

    let declared_decision = authorize(&declared, &facts);
    let undeclared_decision = authorize(&undeclared, &facts);

    require(
        !declared_decision.permits(ExecutionCapability::OutsideRootPath),
        "a declared traversal path must not grant outside-root authority",
    )?;
    require(
        !undeclared_decision.permits(ExecutionCapability::OutsideRootPath),
        "omitting the traversal path must not grant outside-root authority either",
    )?;
    require(
        has_reason(&undeclared_decision, "path_escapes_root"),
        "the undeclared case must still name the escaping path",
    )?;
    Ok(())
}
