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
    EvidenceLimitation, ExecutionCapability, ExecutionIntent, ExecutionReasonClass,
    InputDisposition, InputRiskClass, MAX_CLAIM_BOUNDARY_LEN, MAX_IDENTIFIER_LEN, OperationProfile,
    OperationTrustRequirement, PolicyDenial, ProjectEnvironmentSnapshotBuilder, RequiredScope,
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
        limitations: Vec::new(),
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

/// A requested capability beyond the profile's requirement is withheld, not granted.
///
/// `SourceAnalysisOnly` requires only source analysis, so `OutsideRootPath`
/// here is a genuine extra: the operation still proceeds, with the extra named
/// as omitted. (`ModuleResolutionExternalRead` can no longer serve this fixture
/// — it now *requires* outside-root authority, so an unconfirmed path blocks it
/// outright rather than limiting it.)
#[test]
fn requested_extra_capability_is_withheld_as_limited() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let unconfirmed = ClassifiedInput::new(
        "include.root",
        InputRiskClass::ExternalAbsolutePath,
        EnvironmentInputAuthority::WorkspaceConvention,
        InputDisposition::ConfirmationRequired,
        None,
        "external_include_root",
    );

    let mut request = intent(
        OperationProfile::SourceAnalysisOnly,
        ExecutionReasonClass::Probe,
        &scope,
        &bound,
        vec![unconfirmed.id.clone()],
    );
    request.requested = CapabilitySet::new([
        ExecutionCapability::SourceAnalysis,
        ExecutionCapability::OutsideRootPath,
    ]);

    let decision = authorize(
        &request,
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::None,
            &bound,
            vec![unconfirmed],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::AllowedLimited,
        "an unconfirmed extra yields a limited allow",
    )?;
    require(
        decision.omitted().contains(ExecutionCapability::OutsideRootPath),
        "the limited allow must name the withheld capability",
    )?;
    require(
        !decision.permits(ExecutionCapability::OutsideRootPath),
        "a withheld capability must not be permitted",
    )?;
    require(
        decision.permits(ExecutionCapability::SourceAnalysis),
        "the required capability is still granted",
    )?;
    Ok(())
}

/// An unconfirmed external root now blocks the profile that requires it.
#[test]
fn unconfirmed_external_root_blocks_module_resolution() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let unconfirmed = ClassifiedInput::new(
        "include.root",
        InputRiskClass::ExternalAbsolutePath,
        EnvironmentInputAuthority::WorkspaceConvention,
        InputDisposition::ConfirmationRequired,
        None,
        "external_include_root",
    );
    let decision = authorize(
        &intent(
            OperationProfile::ModuleResolutionExternalRead,
            ExecutionReasonClass::Probe,
            &scope,
            &bound,
            vec![unconfirmed.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::None,
            &bound,
            vec![unconfirmed],
        ),
    );
    require(
        decision.outcome() == AuthorizationOutcome::ConfirmationRequired,
        "outside-root authority is required here, so an unconfirmed path blocks it",
    )?;
    require(decision.granted().is_empty(), "a non-permitting outcome grants nothing")?;
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
            bound_input_ids: Vec::new(),
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

/// An override's own scope is validated, like every other identity here.
///
/// `SessionOverride`'s fields are public, so a caller can supply one whose
/// scope was never checked. Every other identity in the evidence is validated
/// and bounded; skipping this one leaves an unbounded caller-authored string
/// feeding `stable_id`, which is exactly the bound `MAX_IDENTIFIER_LEN` exists
/// to hold.
#[test]
fn override_with_an_invalid_scope_is_rejected_by_validation() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 5)?;
    let mut facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
        &bound,
        vec![path_only_tool()],
    );
    facts.session_override = Some(SessionOverride {
        override_id: "session.grant.1".to_string(),
        // Empty workspace id: rejected everywhere else, previously unchecked here.
        scope: TrustScope::editor_workspace(""),
        granted_policy_generation: 5,
        expires_after_policy_generation: 6,
        capabilities: CapabilitySet::new([ExecutionCapability::ExecutableTool]),
        bound_input_ids: Vec::new(),
    });

    require(
        facts.validate().is_err(),
        "evidence carrying an override with an invalid scope must not validate",
    )?;
    Ok(())
}

/// An override minted for another scope grants nothing here.
///
/// This is the reason a malformed override scope cannot widen a decision:
/// `is_current_for` requires the override's scope to equal the evidence scope,
/// and the evidence scope is itself validated. So a scope that fails
/// validation can never match one that passes, and the override falls out as
/// not-current rather than granting. Pinning it means the validation above is
/// defence in depth rather than the only thing standing between a forged
/// override and an allow.
#[test]
fn override_for_a_foreign_scope_does_not_grant() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let foreign = TrustScope::editor_workspace("someone-elses-workspace");
    let bound = generations("ws", 5)?;
    let ambient = path_only_tool();

    let mut facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
        &bound,
        vec![ambient.clone()],
    );
    facts.session_override = Some(SessionOverride {
        override_id: "session.grant.1".to_string(),
        scope: foreign,
        granted_policy_generation: 5,
        expires_after_policy_generation: 6,
        capabilities: CapabilitySet::new([ExecutionCapability::ExecutableTool]),
        bound_input_ids: Vec::new(),
    });

    require(facts.validate().is_ok(), "the foreign scope is well-formed, just not ours")?;

    let decision = authorize(
        &intent(
            OperationProfile::ExternalFormatter,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            vec![ambient.id.clone()],
        ),
        &facts,
    );

    require(
        decision.outcome() != AuthorizationOutcome::Allowed,
        "an override minted for another workspace must not authorize this one",
    )?;
    require(
        !decision.permits(ExecutionCapability::ExecutableTool),
        "the foreign override must not supply the capability",
    )?;
    require(
        has_reason(&decision, "session_override_not_current"),
        "the outcome must say the override is not current for this scope",
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
        bound_input_ids: Vec::new(),
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

    // `Unsupported`, not `Denied`: the operation cannot run in this scope at
    // all, so there is no authority here for anyone to grant. The security
    // property is unchanged — nothing is granted either way.
    require(
        decision.outcome() == AuthorizationOutcome::Unsupported,
        "a trusted editor workspace must not authorize a hermetic CI process",
    )?;
    require(
        !decision.outcome().permits_execution() && decision.granted().is_empty(),
        "a scope the profile does not admit grants nothing",
    )?;
    require(
        has_reason(&decision, "ci_authority_not_synthesizable"),
        "the outcome must name the missing CI authority",
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

/// The *scope's own* identity is redacted too, not just input-derived values.
///
/// `public_explanation_is_redacted` above uses a benign workspace id (`"ws"`),
/// so it never challenges the one field the projection copies from the caller.
/// A workspace id is very often a filesystem path — that is the natural thing
/// for an editor to use — and #11095 requires the public identity to carry
/// "stable classes, digests, generations, and reason codes, *not* raw
/// executable paths", with negative control 11 failing when a raw path enters
/// the public explanation. A caller obligation stated in prose does not satisfy
/// a negative control, so the projection has to enforce it.
#[test]
fn a_path_shaped_workspace_id_is_not_published_raw() -> Result<(), Box<dyn Error>> {
    let path_id = "/home/alice/clients/acme-confidential/checkout";
    let scope = TrustScope::editor_workspace(path_id);
    let bound = generations(path_id, 1)?;
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

    require(
        decision.outcome() == AuthorizationOutcome::Allowed,
        "a path-shaped workspace id is well-formed; this is about projection, not rejection",
    )?;

    let explanation = decision.public_explanation();
    let rendered = serde_json::to_string(&explanation)?;
    require(
        !rendered.contains(path_id),
        "the raw workspace path must not reach the public explanation",
    )?;
    require(
        !rendered.contains("acme-confidential"),
        "no identifying segment of the workspace path may survive into the public record",
    )?;
    require(
        !rendered.contains("/home/alice"),
        "no directory prefix of the workspace path may survive either",
    )?;

    // Redaction must not cost attribution: the same workspace still yields the
    // same published identity, and a different one yields a different identity.
    let other = TrustScope::editor_workspace("/home/alice/clients/other/checkout");
    let other_bound = generations("/home/alice/clients/other/checkout", 1)?;
    let other_tool = verified_tool();
    let other_decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &other,
            &other_bound,
            vec![other_tool.id.clone()],
        ),
        &evidence(
            &other,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &other_bound,
            vec![other_tool],
        ),
    );
    require(
        explanation.workspace == decision.public_explanation().workspace,
        "the published workspace identity must be stable for one workspace",
    )?;
    require(
        explanation.workspace != other_decision.public_explanation().workspace,
        "two different workspaces must not collapse to one published identity",
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
    // Insert a capability the decision does not grant, without hard-coding the
    // rest of the set — the registry decides what a profile grants, and this
    // control must not need editing when that changes.
    require(
        !decision.permits(ExecutionCapability::PersistentCadence),
        "the fixture must not already grant the capability being forged in",
    )?;
    let widened = encoded.replace("\"granted\":[", "\"granted\":[\"persistent_cadence\",");
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

    // Two rows must never share one requirement identity, or a caller could
    // present the weaker row's authorization for the stronger one.
    //
    // Comparing whole profiles would be vacuous: `identity()` folds in
    // `profile.identity_tag()`, which is injective by construction, so distinct
    // profiles have distinct identities no matter what capabilities they carry.
    // Hold the profile fixed instead and vary only the capability set and scope,
    // which is the collision that would actually matter.
    let base = OperationTrustRequirement::for_profile(OperationProfile::ExternalFormatter);
    let mut configs: Vec<(Vec<&str>, RequiredScope)> = Vec::new();
    let mut identities: Vec<String> = Vec::new();
    for extra in ExecutionCapability::ALL {
        for scope in [
            RequiredScope::EditorWorkspaceOnly,
            RequiredScope::CiHermeticOnly,
            RequiredScope::EitherScope,
        ] {
            let mut row = base.clone();
            row.required = CapabilitySet::new(base.required.iter().chain([extra]));
            row.scope = scope;
            configs.push((row.required.tags(), scope));
            identities.push(row.identity());
        }
    }

    // Distinct capability-set/scope configurations must have distinct
    // identities. Counting distinct configs rather than rows keeps the
    // assertion honest: adding a capability the base already requires, or
    // re-selecting its own scope, legitimately reproduces an earlier row.
    configs.sort();
    configs.dedup();
    identities.sort();
    identities.dedup();
    require(
        identities.len() == configs.len(),
        "requirement identity must distinguish every capability set and scope",
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

/// A verified interpreter must not mask a project-supplied executable declared
/// alongside it.
///
/// Found by an independent security lens. `ExecutableTool` covers every tool an
/// operation invokes, so the evaluator has to look for a disqualifying input
/// before an enabling one. This needs no dishonest intent: both inputs are
/// declared truthfully.
#[test]
fn verified_tool_does_not_mask_a_project_supplied_executable() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let interpreter = verified_tool();
    let wrapper = ClassifiedInput::new(
        "tool.wrapper_script",
        InputRiskClass::ProjectExecutableOrCommand,
        EnvironmentInputAuthority::TrustedProjectConfiguration,
        InputDisposition::RequiresSeparateAuthority,
        None,
        "project_supplied_wrapper",
    );

    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![interpreter.id.clone(), wrapper.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![interpreter, wrapper],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "a project-supplied wrapper must deny even beside a verified interpreter",
    )?;
    require(
        has_reason(&decision, "project_supplied_executable"),
        "the denial must name the project-supplied executable",
    )?;
    require(
        !decision.permits(ExecutionCapability::ExecutableTool),
        "tool authority must not be granted",
    )?;
    Ok(())
}

/// Most-restrictive-wins holds *within* the verified-tool class, not only
/// across risk classes.
///
/// Two tools are selected for one operation by the same authority: one
/// accepted, one explicitly denied. The denial is not about a different risk
/// class, so the earlier project-executable and ambient-PATH guards never see
/// it — only a fold over the verified tools' own dispositions does. Granting
/// here would let an accepted interpreter carry a refused peer into execution.
#[test]
fn denied_verified_tool_is_not_masked_by_an_accepted_peer() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let interpreter = verified_tool();
    let refused = ClassifiedInput::new(
        "tool.refused_helper",
        InputRiskClass::SelectedVerifiedTool,
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::Denied,
        None,
        "user_refused_this_tool",
    );

    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            ids(&[interpreter.clone(), refused.clone()]),
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![interpreter, refused],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "a denied verified tool must deny even beside an accepted one",
    )?;
    require(
        !decision.permits(ExecutionCapability::ExecutableTool),
        "tool authority must not be granted while a selected tool is refused",
    )?;
    require(
        has_reason(&decision, "verified_tool_not_accepted"),
        "the denial must name the refused tool rather than the accepted peer",
    )?;
    Ok(())
}

/// The same fold, for the cadence setting class.
///
/// A user who enables compile-on-save in one setting and disables it in
/// another has not authorized a persistent cadence. Reading only for an
/// enabling setting turns an explicit refusal into repeated execution.
#[test]
fn denied_cadence_setting_is_not_masked_by_an_accepted_peer() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let enabled = ClassifiedInput::new(
        "cadence.compile_on_save",
        InputRiskClass::UserScopedSetting,
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::Accepted,
        None,
        "user_enabled_on_save",
    );
    let disabled = ClassifiedInput::new(
        "cadence.compile_on_save_for_this_language",
        InputRiskClass::UserScopedSetting,
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::Denied,
        None,
        "user_disabled_on_save",
    );

    let decision = authorize(
        &intent(
            OperationProfile::TrustedCompileOnSave,
            ExecutionReasonClass::TrustedPostSave,
            &scope,
            &bound,
            ids(&[tool.clone(), enabled.clone(), disabled.clone()]),
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "save".to_string() },
            &bound,
            vec![tool, enabled, disabled],
        ),
    );

    require(
        decision.outcome() != AuthorizationOutcome::Allowed,
        "a denied cadence setting must not be overridden by an enabling peer",
    )?;
    require(
        !decision.permits(ExecutionCapability::PersistentCadence),
        "the cadence capability must be withheld while a setting refuses it",
    )?;
    require(
        has_reason(&decision, "cadence_setting_not_accepted"),
        "the outcome must name the refusing setting",
    )?;
    Ok(())
}

/// An ambient PATH tool is not masked by a verified tool either.
#[test]
fn verified_tool_does_not_mask_an_ambient_path_tool() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let interpreter = verified_tool();
    let ambient = path_only_tool();

    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![interpreter.id.clone(), ambient.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![interpreter, ambient],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::ConfirmationRequired,
        "an ambient PATH tool beside a verified one still needs confirmation",
    )?;
    require(
        has_reason(&decision, "ambient_tool_selection"),
        "the outcome must name the ambient selection",
    )?;
    Ok(())
}

/// A properly confirmed external absolute path is granted.
///
/// Positive control for the outside-root branch: without it, collapsing every
/// external path to `ConfirmationRequired` would pass unnoticed.
#[test]
fn confirmed_external_absolute_path_is_granted() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let confirmed = ClassifiedInput::new(
        "include.root",
        InputRiskClass::ExternalAbsolutePath,
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::Accepted,
        None,
        "user_selected_external_root",
    );

    let mut request = intent(
        OperationProfile::ModuleResolutionExternalRead,
        ExecutionReasonClass::Probe,
        &scope,
        &bound,
        vec![confirmed.id.clone()],
    );
    request.requested = CapabilitySet::new([
        ExecutionCapability::SourceAnalysis,
        ExecutionCapability::ExternalRead,
        ExecutionCapability::OutsideRootPath,
    ]);

    let decision = authorize(
        &request,
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "resolve".to_string() },
            &bound,
            vec![confirmed],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Allowed,
        "an explicitly selected external root is allowed",
    )?;
    require(
        decision.permits(ExecutionCapability::OutsideRootPath),
        "outside-root authority must actually be granted here",
    )?;
    Ok(())
}

/// Every generation is load-bearing for staleness, not just the policy one.
#[test]
fn each_generation_independently_stales_evidence() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let tool = verified_tool();
    let base = generations("ws", 1)?;

    let moved = [
        BoundGenerations::new(
            base.configuration_generation + 1,
            base.policy_generation,
            base.source_generation,
            base.environment_fingerprint.clone(),
        ),
        BoundGenerations::new(
            base.configuration_generation,
            base.policy_generation + 1,
            base.source_generation,
            base.environment_fingerprint.clone(),
        ),
        BoundGenerations::new(
            base.configuration_generation,
            base.policy_generation,
            base.source_generation + 1,
            base.environment_fingerprint.clone(),
        ),
        // A different environment snapshot identity, from a real snapshot.
        BoundGenerations::new(
            base.configuration_generation,
            base.policy_generation,
            base.source_generation,
            environment_fingerprint("other-workspace", 7)?,
        ),
    ];

    for observed in moved {
        require(observed != base, "each fixture must actually move one generation")?;
        let decision = authorize(
            &intent(
                OperationProfile::RunCurrentSavedFile,
                ExecutionReasonClass::ExplicitUserAction,
                &scope,
                &base,
                vec![tool.id.clone()],
            ),
            &evidence(
                &scope,
                WorkspaceTrust::Trusted,
                AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
                &observed,
                vec![tool.clone()],
            ),
        );
        require(
            decision.outcome() == AuthorizationOutcome::Stale,
            "moving any single generation must stale the evidence",
        )?;
    }
    Ok(())
}

/// Evidence carrying one input identity twice cannot be evaluated.
#[test]
fn duplicate_input_identity_is_rejected() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let mut facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
        &bound,
        vec![tool.clone(), tool.clone()],
    );

    require(facts.validate().is_err(), "duplicate input identities must fail validation")?;

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
        decision.outcome() == AuthorizationOutcome::NotProven,
        "unevaluable evidence must never produce an allow",
    )?;

    facts.inputs.truncate(1);
    require(facts.validate().is_ok(), "the same evidence without the duplicate is valid")?;
    Ok(())
}

/// The decision fingerprint covers the generations it is bound to.
#[test]
fn tampering_bound_generations_fails_revalidation() -> Result<(), Box<dyn Error>> {
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
    // Re-date the decision to a later policy generation without re-deriving it.
    let tampered = encoded.replace("\"policy_generation\":1", "\"policy_generation\":99");
    require(tampered != encoded, "the tamper rewrite must actually apply")?;
    let forged: Result<perl_workspace_core::ExecutionAuthorizationDecision, _> =
        serde_json::from_str(&tampered);
    require(forged.is_err(), "a decision re-dated to another generation must fail revalidation")?;
    Ok(())
}

/// A limited allow cannot be widened into the capability it withheld.
#[test]
fn tampering_allowed_limited_split_fails_revalidation() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let unconfirmed = ClassifiedInput::new(
        "include.root",
        InputRiskClass::ExternalAbsolutePath,
        EnvironmentInputAuthority::WorkspaceConvention,
        InputDisposition::ConfirmationRequired,
        None,
        "external_include_root",
    );
    let mut request = intent(
        OperationProfile::SourceAnalysisOnly,
        ExecutionReasonClass::Probe,
        &scope,
        &bound,
        vec![unconfirmed.id.clone()],
    );
    request.requested = CapabilitySet::new([
        ExecutionCapability::SourceAnalysis,
        ExecutionCapability::OutsideRootPath,
    ]);
    let decision = authorize(
        &request,
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::None,
            &bound,
            vec![unconfirmed],
        ),
    );
    require(
        decision.outcome() == AuthorizationOutcome::AllowedLimited,
        "this fixture must produce a limited allow",
    )?;

    let encoded = serde_json::to_string(&decision)?;
    // Move the withheld capability out of `omitted` and into `granted`.
    let tampered = encoded
        .replace("\"omitted\":[\"outside_root_path\"]", "\"omitted\":[]")
        .replace("\"granted\":[", "\"granted\":[\"outside_root_path\",");
    require(tampered != encoded, "the widening rewrite must actually apply")?;
    let forged: Result<perl_workspace_core::ExecutionAuthorizationDecision, _> =
        serde_json::from_str(&tampered);
    require(forged.is_err(), "widening a limited allow must fail revalidation")?;
    Ok(())
}

/// A decision from another schema version is rejected, not reinterpreted.
#[test]
fn foreign_schema_version_is_rejected() -> Result<(), Box<dyn Error>> {
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
    let next_version = EXECUTION_AUTHORIZATION_SCHEMA_VERSION + 1;
    let bumped = encoded.replace(
        &format!("\"schema_version\":{EXECUTION_AUTHORIZATION_SCHEMA_VERSION}"),
        &format!("\"schema_version\":{next_version}"),
    );
    require(bumped != encoded, "the version rewrite must actually apply")?;
    let foreign: Result<perl_workspace_core::ExecutionAuthorizationDecision, _> =
        serde_json::from_str(&bumped);
    require(foreign.is_err(), "a foreign schema version must be rejected")?;
    Ok(())
}

/// Running a Perl file is at least as much execution as compiling one, so it
/// must require the same environment authority.
#[test]
fn perl_launching_profiles_require_environment_authority() -> Result<(), Box<dyn Error>> {
    // Registry invariant: executing project code means the ambient environment
    // influences what that code loads.
    for profile in OperationProfile::ALL {
        let required = OperationTrustRequirement::for_profile(profile).required;
        if required.contains(ExecutionCapability::ProjectCodeExecution) {
            require(
                required.contains(ExecutionCapability::EnvironmentCodeLoading),
                "a profile that executes project code must require environment authority",
            )?;
        }
    }

    // Behavioral control: a denied ambient PERL5LIB must block running a file.
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
    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![tool.id.clone(), ambient_env.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![tool, ambient_env],
        ),
    );
    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "a denied ambient PERL5LIB must block running a Perl file",
    )?;
    Ok(())
}

/// An explicit environment that was not accepted must not fall through to the
/// no-environment-present branch.
#[test]
fn unaccepted_explicit_environment_does_not_reach_the_fallback() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();

    for (disposition, expected) in [
        (InputDisposition::Denied, AuthorizationOutcome::Denied),
        (InputDisposition::ConfirmationRequired, AuthorizationOutcome::ConfirmationRequired),
        (InputDisposition::UnknownNotProven, AuthorizationOutcome::NotProven),
        (InputDisposition::RequiresSeparateAuthority, AuthorizationOutcome::Denied),
    ] {
        let explicit_env = ClassifiedInput::new(
            "environment.perl5lib",
            InputRiskClass::AmbientPerlEnvironment,
            EnvironmentInputAuthority::ExplicitEnvironment,
            disposition,
            None,
            "reviewed_activation_not_accepted",
        );
        let decision = authorize(
            &intent(
                OperationProfile::PerlCompileCurrentSavedFile,
                ExecutionReasonClass::ExplicitUserAction,
                &scope,
                &bound,
                vec![tool.id.clone(), explicit_env.id.clone()],
            ),
            &evidence(
                &scope,
                WorkspaceTrust::Trusted,
                AuthorizationActor::ExplicitUserAction { action_id: "compile".to_string() },
                &bound,
                vec![tool.clone(), explicit_env],
            ),
        );
        require(
            decision.outcome() == expected,
            "an unaccepted explicit environment must carry its disposition, not the fallback",
        )?;
        require(
            !decision.permits(ExecutionCapability::EnvironmentCodeLoading),
            "code-loading authority must not be granted from an unaccepted environment",
        )?;
    }
    Ok(())
}

/// An intent naming an input the evidence does not carry cannot be evaluated.
#[test]
fn intent_input_absent_from_evidence_is_not_proven() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let interpreter = verified_tool();
    // Declared by the intent, but belonging to different evidence.
    let foreign_wrapper = ClassifiedInput::new(
        "tool.wrapper_script",
        InputRiskClass::ProjectExecutableOrCommand,
        EnvironmentInputAuthority::TrustedProjectConfiguration,
        InputDisposition::RequiresSeparateAuthority,
        None,
        "wrapper_from_other_evidence",
    );

    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![interpreter.id.clone(), foreign_wrapper.id.clone()],
        ),
        // Evidence carries only the interpreter.
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![interpreter],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::NotProven,
        "an unresolved declared input must not be silently dropped",
    )?;
    require(decision.granted().is_empty(), "nothing is granted on unresolved evidence")?;
    Ok(())
}

/// Policy denials that differ only in the capabilities they deny still yield a
/// stable evidence identity regardless of order.
#[test]
fn policy_denial_ordering_does_not_change_identity() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    // Same policy_id and reason_code, different denied capability sets.
    let first = PolicyDenial::new(
        "org.policy.shared",
        CapabilitySet::new([ExecutionCapability::ExecutableTool]),
        "administrator_denied",
    );
    let second = PolicyDenial::new(
        "org.policy.shared",
        CapabilitySet::new([ExecutionCapability::InteractiveSession]),
        "administrator_denied",
    );

    let build = |denials: Vec<PolicyDenial>| {
        let mut facts = evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![tool.clone()],
        );
        facts.policy_denials = denials;
        facts
    };

    let forward = build(vec![first.clone(), second.clone()]);
    let reversed = build(vec![second, first]);
    require(
        forward.identity() == reversed.identity(),
        "policy-denial ordering must not change evidence identity",
    )?;
    Ok(())
}

/// Evidence that could not establish an authority fact fails closed.
#[test]
fn unresolved_evidence_limitation_is_not_proven() -> Result<(), Box<dyn Error>> {
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
    facts.limitations = vec![EvidenceLimitation::new(
        "tool_identity_unverified",
        ExecutionCapability::ExecutableTool,
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
        decision.outcome() == AuthorizationOutcome::NotProven,
        "an unresolved limitation on a required capability must fail closed",
    )?;
    require(has_reason(&decision, "evidence_limitation"), "the outcome must name the limitation")?;
    Ok(())
}

/// A scoped override supplies missing authority; it does not overrule a denial.
///
/// Found by an independent review lens. An override that could convert a denied
/// input into a grant would be a way to click past every input classification —
/// project-supplied executables, ambient Perl environments, escaping paths.
#[test]
fn session_override_cannot_cure_a_denied_input() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 5)?;
    let wrapper = ClassifiedInput::new(
        "tool.wrapper_script",
        InputRiskClass::ProjectExecutableOrCommand,
        EnvironmentInputAuthority::TrustedProjectConfiguration,
        InputDisposition::RequiresSeparateAuthority,
        None,
        "project_supplied_wrapper",
    );
    let mut facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
        &bound,
        vec![wrapper.clone()],
    );
    facts.session_override = Some(SessionOverride {
        override_id: "session.grant.1".to_string(),
        scope: scope.clone(),
        granted_policy_generation: 0,
        expires_after_policy_generation: u64::MAX,
        capabilities: CapabilitySet::new([ExecutionCapability::ExecutableTool]),
        bound_input_ids: Vec::new(),
    });

    let decision = authorize(
        &intent(
            OperationProfile::ExternalFormatter,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            vec![wrapper.id.clone()],
        ),
        &facts,
    );

    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "a current override must not convert a denied input into a grant",
    )?;
    require(
        !has_reason(&decision, "granted_by_session_override"),
        "the override must not be recorded as supplying a denied capability",
    )?;

    // Control: the same override still supplies a merely-unestablished
    // capability, so the rule narrows overrides rather than disabling them.
    let ambient = path_only_tool();
    let mut curable = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
        &bound,
        vec![ambient.clone()],
    );
    curable.session_override = facts.session_override.clone();
    let allowed = authorize(
        &intent(
            OperationProfile::ExternalFormatter,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            vec![ambient.id.clone()],
        ),
        &curable,
    );
    require(
        allowed.outcome() == AuthorizationOutcome::Allowed,
        "an override must still supply a capability that was only unestablished",
    )?;
    Ok(())
}

/// A classified input cannot keep an approved identity while changing content.
#[test]
fn forged_classified_input_identity_is_rejected() -> Result<(), Box<dyn Error>> {
    let approved = verified_tool();
    let encoded = serde_json::to_string(&approved)?;
    require(
        serde_json::from_str::<ClassifiedInput>(&encoded).is_ok(),
        "an untampered input must round-trip",
    )?;

    // Keep the approved id, swap the risk class to a dangerous one.
    let forged = encoded.replace(
        "\"risk_class\":\"selected_verified_tool\"",
        "\"risk_class\":\"project_executable_or_command\"",
    );
    require(forged != encoded, "the tamper rewrite must actually apply")?;
    require(
        serde_json::from_str::<ClassifiedInput>(&forged).is_err(),
        "an id that does not match its own fields must be rejected",
    )?;

    // Same for a disposition swap.
    let relabelled = encoded.replace("\"disposition\":\"accepted\"", "\"disposition\":\"denied\"");
    require(relabelled != encoded, "the disposition rewrite must actually apply")?;
    require(
        serde_json::from_str::<ClassifiedInput>(&relabelled).is_err(),
        "a relabelled disposition must be rejected",
    )?;
    Ok(())
}

/// Each external path carries its own disposition, and the most restrictive wins.
///
/// Found by an independent review lens: collapsing every unaccepted external
/// path into `ConfirmationRequired` turned an explicit refusal into a
/// misleading actionable prompt, and made unknown provenance read as merely
/// unconfirmed.
#[test]
fn external_path_dispositions_are_preserved_and_most_restrictive_wins() -> Result<(), Box<dyn Error>>
{
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;

    let external = |name: &str, authority, disposition| {
        ClassifiedInput::new(
            name,
            InputRiskClass::ExternalAbsolutePath,
            authority,
            disposition,
            None,
            "external_include_root",
        )
    };

    let decide = |inputs: Vec<ClassifiedInput>| {
        let mut request = intent(
            OperationProfile::ModuleResolutionExternalRead,
            ExecutionReasonClass::Probe,
            &scope,
            &bound,
            inputs.iter().map(|input| input.id.clone()).collect(),
        );
        request.requested = CapabilitySet::new([
            ExecutionCapability::SourceAnalysis,
            ExecutionCapability::ExternalRead,
            ExecutionCapability::OutsideRootPath,
        ]);
        authorize(
            &request,
            &evidence(
                &scope,
                WorkspaceTrust::Trusted,
                AuthorizationActor::ExplicitUserAction { action_id: "resolve".to_string() },
                &bound,
                inputs,
            ),
        )
    };

    // A refusal stays a refusal, not a prompt.
    let denied = decide(vec![external(
        "include.denied",
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::Denied,
    )]);
    require(
        has_reason(&denied, "external_path_denied"),
        "a denied external path must be reported as denied, not unconfirmed",
    )?;
    require(
        !denied.permits(ExecutionCapability::OutsideRootPath),
        "a denied external path grants nothing",
    )?;

    // Unknown provenance stays unproven rather than becoming a prompt.
    let unknown = decide(vec![external(
        "include.unknown",
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::UnknownNotProven,
    )]);
    require(
        !unknown.permits(ExecutionCapability::OutsideRootPath),
        "an unproven external path grants nothing",
    )?;

    // Mixed inputs: an accepted path beside a denied one must not soften it.
    let mixed = decide(vec![
        external(
            "include.ok",
            EnvironmentInputAuthority::UserConfiguration,
            InputDisposition::Accepted,
        ),
        external(
            "include.denied",
            EnvironmentInputAuthority::UserConfiguration,
            InputDisposition::Denied,
        ),
    ]);
    require(
        has_reason(&mixed, "external_path_denied"),
        "the most restrictive external path must decide",
    )?;
    require(
        !mixed.permits(ExecutionCapability::OutsideRootPath),
        "an accepted path must not rescue a denied one",
    )?;

    // Control: all accepted under explicit user authority is still granted.
    let accepted = decide(vec![external(
        "include.ok",
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::Accepted,
    )]);
    require(
        accepted.permits(ExecutionCapability::OutsideRootPath),
        "an explicitly selected external root is still granted",
    )?;
    Ok(())
}

/// A hermetic scope names the CI identity, not workspace trust, as the remedy.
///
/// Workspace trust is deliberately ignored under a CI scope, so advising an
/// operator to grant it would leave the denial exactly where it was.
#[test]
fn ci_scope_denials_name_the_ci_identity_as_actionable() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::ci_hermetic("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();

    // A hermetic scope with no CI identity: authority is absent.
    let decision = authorize(
        &intent(
            OperationProfile::CiHermeticProcess,
            ExecutionReasonClass::CiHermetic,
            &scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &evidence(&scope, WorkspaceTrust::Trusted, AuthorizationActor::None, &bound, vec![tool]),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "a hermetic scope without a CI identity is denied",
    )?;
    require(
        has_reason(&decision, "no_ci_identity"),
        "the denial must name the missing CI identity",
    )?;
    require(
        !has_reason(&decision, "workspace_untrusted"),
        "workspace trust is ignored in a hermetic scope and must not be advised",
    )?;
    require(
        decision
            .reasons()
            .iter()
            .any(|reason| reason.actionable_authority == ActionableAuthority::CiIdentity),
        "the actionable authority must be the CI identity",
    )?;
    let explanation = decision.public_explanation();
    require(
        explanation.actionable_authorities.iter().any(|tag| tag == "ci_identity"),
        "the public explanation must point at the CI identity",
    )?;
    Ok(())
}

/// An accepted reviewed activation does not mask a denied one beside it.
#[test]
fn accepted_activation_does_not_mask_a_denied_peer() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let accepted = ClassifiedInput::new(
        "environment.perl5lib",
        InputRiskClass::AmbientPerlEnvironment,
        EnvironmentInputAuthority::ExplicitEnvironment,
        InputDisposition::Accepted,
        None,
        "reviewed_activation_accepted",
    );
    let denied = ClassifiedInput::new(
        "environment.perl5opt",
        InputRiskClass::AmbientPerlEnvironment,
        EnvironmentInputAuthority::ExplicitEnvironment,
        InputDisposition::Denied,
        None,
        "reviewed_activation_denied",
    );

    let decision = authorize(
        &intent(
            OperationProfile::PerlCompileCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![tool.id.clone(), accepted.id.clone(), denied.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "compile".to_string() },
            &bound,
            vec![tool, accepted, denied],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "a denied activation must not be masked by an accepted peer",
    )?;
    require(
        !decision.permits(ExecutionCapability::EnvironmentCodeLoading),
        "code-loading authority must be withheld",
    )?;
    Ok(())
}

/// An input relabelled in place cannot keep its approved identity.
#[test]
fn relabelled_input_fails_evidence_validation() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let mut tool = verified_tool();
    require(tool.identity_matches_fields(), "a freshly built input matches its own fields")?;

    // Relabel in place, keeping the approved identity.
    tool.risk_class = InputRiskClass::ProjectExecutableOrCommand;
    require(
        !tool.identity_matches_fields(),
        "a relabelled input must no longer match its identity",
    )?;

    let facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
        &bound,
        vec![tool.clone()],
    );
    require(facts.validate().is_err(), "evidence carrying a relabelled input is invalid")?;

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
        decision.outcome() == AuthorizationOutcome::NotProven,
        "a relabelled input must never reach an allow",
    )?;
    Ok(())
}

/// An absent scope component cannot alias a present-but-empty one.
#[test]
fn empty_scope_component_cannot_alias_the_absent_case() -> Result<(), Box<dyn Error>> {
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let absent = TrustScope::editor_workspace("ws");
    let empty_root = TrustScope::editor_workspace("ws").with_root("");

    let facts = |scope: &TrustScope| {
        evidence(
            scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![tool.clone()],
        )
    };

    // The present-but-empty component is refused rather than silently aliasing.
    require(facts(&absent).validate().is_ok(), "an absent root is valid")?;
    require(
        facts(&empty_root).validate().is_err(),
        "a present-but-empty root must be rejected, not encoded as absent",
    )?;

    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &empty_root,
            &bound,
            vec![tool.id.clone()],
        ),
        &facts(&empty_root),
    );
    require(
        decision.outcome() == AuthorizationOutcome::NotProven,
        "an unevaluable scope must never reach an allow",
    )?;

    // A real root remains distinguishable from an absent one.
    let real_root = TrustScope::editor_workspace("ws").with_root("root-a");
    require(
        facts(&real_root).identity() != facts(&absent).identity(),
        "a named root must not share the absent root's identity",
    )?;
    Ok(())
}

/// Reading outside the workspace implies using a path outside the root.
#[test]
fn external_read_profiles_require_outside_root_authority() -> Result<(), Box<dyn Error>> {
    for profile in OperationProfile::ALL {
        let required = OperationTrustRequirement::for_profile(profile).required;
        if required.contains(ExecutionCapability::ExternalRead) {
            require(
                required.contains(ExecutionCapability::OutsideRootPath),
                "a profile that reads outside the workspace must require outside-root authority",
            )?;
        }
    }

    // Behavioral control: a denied external path now blocks module resolution
    // even when the caller does not separately ask for outside-root authority.
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let denied = ClassifiedInput::new(
        "include.root",
        InputRiskClass::ExternalAbsolutePath,
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::Denied,
        None,
        "external_include_root_denied",
    );
    let decision = authorize(
        &intent(
            OperationProfile::ModuleResolutionExternalRead,
            ExecutionReasonClass::Probe,
            &scope,
            &bound,
            vec![denied.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "resolve".to_string() },
            &bound,
            vec![denied],
        ),
    );
    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "a denied external path must block the profile that reads external roots",
    )?;
    Ok(())
}

/// Caller-supplied identity material is bounded, as the contract advertises.
#[test]
fn caller_supplied_identity_material_is_bounded() -> Result<(), Box<dyn Error>> {
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let oversized = "x".repeat(MAX_IDENTIFIER_LEN + 1);

    // A scope identity beyond the published bound is refused.
    let wide_scope = TrustScope::editor_workspace(oversized.clone());
    let facts = evidence(
        &wide_scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
        &bound,
        vec![tool.clone()],
    );
    require(facts.validate().is_err(), "an oversized workspace id must be rejected")?;

    // And it never reaches an allow.
    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &wide_scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &facts,
    );
    require(
        decision.outcome() == AuthorizationOutcome::NotProven,
        "unbounded identity material must never reach an allow",
    )?;

    // An oversized claim boundary is refused by the intent validator.
    let scope = TrustScope::editor_workspace("ws");
    let mut wide_claim = intent(
        OperationProfile::RunCurrentSavedFile,
        ExecutionReasonClass::ExplicitUserAction,
        &scope,
        &bound,
        vec![tool.id.clone()],
    );
    wide_claim.claim_boundary = "y".repeat(MAX_CLAIM_BOUNDARY_LEN + 1);
    require(wide_claim.validate().is_err(), "an oversized claim boundary must be rejected")?;

    // Control: values at the bound are accepted.
    let exact_scope = TrustScope::editor_workspace("z".repeat(MAX_IDENTIFIER_LEN));
    let exact = evidence(
        &exact_scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
        &bound,
        vec![tool],
    );
    require(exact.validate().is_ok(), "a value exactly at the bound is accepted")?;
    Ok(())
}

/// An unproven input asks for provenance, not for configuration.
///
/// Advising a user to change a setting cannot establish where an input came
/// from, so the remedy has to track the finding rather than the code path.
#[test]
fn unproven_inputs_name_provenance_as_the_remedy() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();

    // An external path whose provenance could not be established.
    let unproven_path = ClassifiedInput::new(
        "include.root",
        InputRiskClass::ExternalAbsolutePath,
        EnvironmentInputAuthority::UserConfiguration,
        InputDisposition::UnknownNotProven,
        None,
        "external_root_provenance_unknown",
    );
    let mut request = intent(
        OperationProfile::SourceAnalysisOnly,
        ExecutionReasonClass::Probe,
        &scope,
        &bound,
        vec![unproven_path.id.clone()],
    );
    request.requested = CapabilitySet::new([
        ExecutionCapability::SourceAnalysis,
        ExecutionCapability::OutsideRootPath,
    ]);
    let path_decision = authorize(
        &request,
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::None,
            &bound,
            vec![unproven_path],
        ),
    );
    require(
        path_decision
            .reasons()
            .iter()
            .any(|reason| reason.actionable_authority == ActionableAuthority::InputProvenance),
        "an unproven external path must ask for provenance",
    )?;
    require(
        !has_reason(&path_decision, "external_path_unconfirmed"),
        "an unproven path is not merely unconfirmed",
    )?;

    // A reviewed activation whose provenance could not be established.
    let unproven_env = ClassifiedInput::new(
        "environment.perl5lib",
        InputRiskClass::AmbientPerlEnvironment,
        EnvironmentInputAuthority::ExplicitEnvironment,
        InputDisposition::UnknownNotProven,
        None,
        "activation_provenance_unknown",
    );
    let env_decision = authorize(
        &intent(
            OperationProfile::PerlCompileCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            vec![tool.id.clone(), unproven_env.id.clone()],
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "compile".to_string() },
            &bound,
            vec![tool, unproven_env],
        ),
    );
    require(
        env_decision.outcome() == AuthorizationOutcome::NotProven,
        "an unproven activation is not-proven",
    )?;
    require(
        env_decision
            .reasons()
            .iter()
            .any(|reason| reason.actionable_authority == ActionableAuthority::InputProvenance),
        "an unproven activation must ask for provenance",
    )?;
    Ok(())
}

/// A grant issued for one input does not authorize a different one.
///
/// Found by an independent security lens: a capability is coarser than the
/// thing a user actually confirmed, so an unbound grant for one ambient `PATH`
/// executable would silently cover any other ambient executable for the rest of
/// the generation window.
#[test]
fn bound_session_override_does_not_leak_to_another_input() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 5)?;
    let approved = path_only_tool();
    let other = ClassifiedInput::new(
        "tool.linter",
        InputRiskClass::AmbientPathOrCwd,
        EnvironmentInputAuthority::Ambient,
        InputDisposition::RequiresSeparateAuthority,
        None,
        "resolved_from_path",
    );

    // The grant names the input it was issued for.
    let grant = SessionOverride {
        override_id: "session.grant.1".to_string(),
        scope: scope.clone(),
        granted_policy_generation: 0,
        expires_after_policy_generation: u64::MAX,
        capabilities: CapabilitySet::new([ExecutionCapability::ExecutableTool]),
        bound_input_ids: vec![approved.id.clone()],
    };

    let decide = |input: &ClassifiedInput| {
        let mut facts = evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
            &bound,
            vec![input.clone()],
        );
        facts.session_override = Some(grant.clone());
        authorize(
            &intent(
                OperationProfile::ExternalFormatter,
                ExecutionReasonClass::ExternalTool,
                &scope,
                &bound,
                vec![input.id.clone()],
            ),
            &facts,
        )
    };

    require(
        decide(&approved).outcome() == AuthorizationOutcome::Allowed,
        "the grant covers the input it was issued for",
    )?;
    require(
        decide(&other).outcome() != AuthorizationOutcome::Allowed,
        "the grant must not authorize a different ambient executable",
    )?;
    require(
        !has_reason(&decide(&other), "granted_by_session_override"),
        "an uncovered input must not record an override grant",
    )?;

    // A grant's binding is part of its identity, so two differently-bound
    // grants cannot share an evidence fingerprint.
    let mut unbound = grant.clone();
    unbound.bound_input_ids = Vec::new();
    let with = |over: SessionOverride| {
        let mut facts = evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
            &bound,
            vec![approved.clone()],
        );
        facts.session_override = Some(over);
        facts.identity()
    };
    require(
        with(grant) != with(unbound),
        "a grant's input binding must be part of the evidence identity",
    )?;
    Ok(())
}

/// A grant bound to an input does not authorize an operation that names none.
///
/// `all` over an empty slice is vacuously true, so this is the hole the
/// input-binding fix left behind: without a non-empty requirement, a grant
/// issued for one specific tool would cover an operation declaring no inputs.
///
/// The bound input here is a `SelectedVerifiedTool` on purpose — an ambient
/// input would be pulled in by `applies_regardless_of_intent` and the relevant
/// set would never actually be empty.
#[test]
fn bound_session_override_does_not_authorize_an_inputless_operation() -> Result<(), Box<dyn Error>>
{
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 5)?;
    let approved = verified_tool();

    let mut facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
        &bound,
        vec![approved.clone()],
    );
    facts.session_override = Some(SessionOverride {
        override_id: "session.grant.1".to_string(),
        scope: scope.clone(),
        granted_policy_generation: 0,
        expires_after_policy_generation: u64::MAX,
        capabilities: CapabilitySet::new([ExecutionCapability::ExecutableTool]),
        bound_input_ids: vec![approved.id.clone()],
    });

    // The operation declares no inputs, so the grant covers nothing it consumes.
    let inputless = authorize(
        &intent(
            OperationProfile::ExternalFormatter,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            Vec::new(),
        ),
        &facts,
    );
    require(
        inputless.outcome() != AuthorizationOutcome::Allowed,
        "a bound grant must not cover an operation that names no inputs",
    )?;
    require(
        !has_reason(&inputless, "granted_by_session_override"),
        "an inputless operation must not record an override grant",
    )?;

    // Control: declaring the bound input is still allowed.
    let covered = authorize(
        &intent(
            OperationProfile::ExternalFormatter,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            vec![approved.id.clone()],
        ),
        &facts,
    );
    require(
        covered.outcome() == AuthorizationOutcome::Allowed,
        "the operation that consumes the bound input is still allowed",
    )?;
    Ok(())
}

/// A rejected request does not publish the material that got it rejected.
#[test]
fn rejected_scope_does_not_reach_the_public_explanation() -> Result<(), Box<dyn Error>> {
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let oversized = "x".repeat(MAX_IDENTIFIER_LEN + 1);
    let wide_scope = TrustScope::editor_workspace(oversized.clone());

    let decision = authorize(
        &intent(
            OperationProfile::RunCurrentSavedFile,
            ExecutionReasonClass::ExplicitUserAction,
            &wide_scope,
            &bound,
            vec![tool.id.clone()],
        ),
        &evidence(
            &wide_scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![tool],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::NotProven,
        "an unbounded identifier is unevaluable",
    )?;

    let rendered = serde_json::to_string(&decision.public_explanation())?;
    require(
        !rendered.contains(&oversized),
        "the rejected identifier must not reach the public explanation",
    )?;
    require(
        !rendered.contains(&"x".repeat(MAX_IDENTIFIER_LEN)),
        "no run of the rejected identifier may survive into the public record",
    )?;
    require(
        decision.scope().workspace_id.len() <= MAX_IDENTIFIER_LEN,
        "the decision's own scope must stay within the published bound",
    )?;
    // And a decision carrying an unusable scope cannot round-trip.
    require(decision.validate().is_ok(), "the sanitized decision is itself valid")?;
    Ok(())
}

/// A *denied* ambient tool must not soften into a prompt an override can satisfy.
///
/// `evaluate_executable_tool` answers `ConfirmationRequired` for any
/// `AmbientPathOrCwd` input without consulting that input's own disposition.
/// That is the right answer for an unreviewed ambient tool, and the wrong one
/// for a refused one: `session_override_cannot_cure_a_denied_input` holds that
/// an override cannot revive a denial, but a denial softened to a prompt is no
/// longer a denial, so the override revives it after all.
#[test]
fn a_denied_ambient_tool_is_not_revived_by_an_override() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 5)?;
    let refused_ambient = ClassifiedInput::new(
        "tool.formatter",
        InputRiskClass::AmbientPathOrCwd,
        EnvironmentInputAuthority::Ambient,
        InputDisposition::Denied,
        None,
        "user_refused_this_path_tool",
    );

    let mut facts = evidence(
        &scope,
        WorkspaceTrust::Trusted,
        AuthorizationActor::ExplicitUserAction { action_id: "format".to_string() },
        &bound,
        vec![refused_ambient.clone()],
    );
    facts.session_override = Some(SessionOverride {
        override_id: "session.grant.1".to_string(),
        scope: scope.clone(),
        granted_policy_generation: 5,
        expires_after_policy_generation: 6,
        capabilities: CapabilitySet::new([ExecutionCapability::ExecutableTool]),
        bound_input_ids: Vec::new(),
    });

    let decision = authorize(
        &intent(
            OperationProfile::ExternalFormatter,
            ExecutionReasonClass::ExternalTool,
            &scope,
            &bound,
            vec![refused_ambient.id.clone()],
        ),
        &facts,
    );

    require(
        decision.outcome() != AuthorizationOutcome::Allowed,
        "a refused ambient tool must not be authorized by a session override",
    )?;
    require(
        !decision.permits(ExecutionCapability::ExecutableTool),
        "the refused tool must not receive tool authority",
    )?;
    Ok(())
}

/// Blanket authority to leave the root is not granted on an empty evidence set.
///
/// `OutsideRootPath` is, in this module's own words, "blanket authority to
/// leave the root". `module_resolution_external_read` exists to read an
/// external root, so an intent for it that declares no external path at all
/// has supplied no path for the decision to judge. Granting on the empty set
/// is the vacuous-truth shape already fixed once in `covers_inputs`.
#[test]
fn outside_root_authority_is_not_granted_without_a_path() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;

    let decision = authorize(
        &intent(
            OperationProfile::ModuleResolutionExternalRead,
            ExecutionReasonClass::ExplicitUserAction,
            &scope,
            &bound,
            Vec::new(),
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "resolve".to_string() },
            &bound,
            Vec::new(),
        ),
    );

    require(
        !decision.permits(ExecutionCapability::OutsideRootPath),
        "leaving the root must not be granted when no external path was classified",
    )?;
    Ok(())
}

/// A denied project configuration blocks an operation that reads configuration.
///
/// `ProjectConfiguration` is decided from workspace trust alone and never looks
/// at inputs, so this only fails closed today because every profile that needs
/// it also needs `ExecutableTool`, and a project config file classifies as
/// `ProjectExecutableOrCommand`. That is a property of the registry rows rather
/// than of the rule, which is the fragility this PR already named as its
/// recurring root cause. Pinning the behaviour keeps a future row from quietly
/// losing it.
#[test]
fn a_denied_project_configuration_blocks_the_operation() -> Result<(), Box<dyn Error>> {
    let scope = TrustScope::editor_workspace("ws");
    let bound = generations("ws", 1)?;
    let tool = verified_tool();
    let refused_config = ClassifiedInput::new(
        "configuration.project_runner",
        InputRiskClass::ProjectExecutableOrCommand,
        EnvironmentInputAuthority::TrustedProjectConfiguration,
        InputDisposition::Denied,
        None,
        "user_refused_this_configuration",
    );

    let decision = authorize(
        &intent(
            OperationProfile::RunProjectCommand,
            ExecutionReasonClass::ProjectRunner,
            &scope,
            &bound,
            ids(&[tool.clone(), refused_config.clone()]),
        ),
        &evidence(
            &scope,
            WorkspaceTrust::Trusted,
            AuthorizationActor::ExplicitUserAction { action_id: "run".to_string() },
            &bound,
            vec![tool, refused_config],
        ),
    );

    require(
        decision.outcome() == AuthorizationOutcome::Denied,
        "a refused project configuration must deny the operation",
    )?;
    require(
        decision.granted().is_empty(),
        "a denied outcome grants exactly nothing, configuration authority included",
    )?;
    Ok(())
}
