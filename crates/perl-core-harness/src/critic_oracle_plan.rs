//! Build a validated [`ProcessPlan`] for repository-only Perl::Critic oracle runs (#6987).
//!
//! The pinned compatibility oracle executes only through the shared process
//! domain: structured argv, reviewed environment projection, bounded capture,
//! deadline, process-tree termination, and subject identities derived from the
//! #6984 [`OracleSubject`]. Product runtime crates must not construct these
//! plans.

use std::path::Path;
use std::time::Duration;

use perl_subprocess_runtime::process::{
    AmbientInheritance, AuthorizationEvidence, AuthorizationStrength, CaptureBudget, ClaimBoundary,
    CwdPolicy, DeadlinePolicy, EnvironmentProjection, EvidenceFreshness, ExecutableIdentity,
    ExecutionProfile, OperationId, OwnerDomain, PlanId, PrivatePath, ProcessPlan,
    ResolutionProvenance, SchemaVersion, SubjectIdentity, SubjectReference, TerminationPolicy,
};

use crate::critic_oracle::{OracleInvocation, OracleSubject, OracleSubjectError};

/// Stable operation identity for every Perl::Critic conformance oracle plan.
pub const CRITIC_ORACLE_OPERATION_ID: &str = "perl-critic-conformance-oracle";

/// Default wall-clock bound for one oracle observation.
pub const DEFAULT_ORACLE_DEADLINE: Duration = Duration::from_secs(120);

/// Default stdout/stderr capture budget for one oracle observation (8 MiB).
pub const DEFAULT_ORACLE_CAPTURE_BYTES: u64 = 8 * 1024 * 1024;

/// Private filesystem inputs required to materialize one oracle [`ProcessPlan`].
#[derive(Debug, Clone)]
pub struct CriticOracleProcessSpec {
    /// Exact #6984 subject identity (paths never enter the subject digest).
    pub subject: OracleSubject,
    /// Resolved absolute `perlcritic` executable chosen before spawn.
    pub perlcritic_executable: PrivatePath,
    /// Source file passed to Perl::Critic.
    pub source_file: PrivatePath,
    /// Profile file (`-profile` / `.perlcriticrc`) passed to Perl::Critic.
    pub profile_file: PrivatePath,
    /// Exact working directory for the child process.
    pub working_directory: PrivatePath,
    /// Wall-clock deadline enforced by the supervisor lane.
    pub deadline: Duration,
    /// Per-stream capture budget enforced by the supervisor lane.
    pub capture_budget_bytes: u64,
}

impl CriticOracleProcessSpec {
    /// Construct a spec with the reviewed default deadline and capture bounds.
    #[must_use]
    pub fn with_defaults(
        subject: OracleSubject,
        perlcritic_executable: PrivatePath,
        source_file: PrivatePath,
        profile_file: PrivatePath,
        working_directory: PrivatePath,
    ) -> Self {
        Self {
            subject,
            perlcritic_executable,
            source_file,
            profile_file,
            working_directory,
            deadline: DEFAULT_ORACLE_DEADLINE,
            capture_budget_bytes: DEFAULT_ORACLE_CAPTURE_BYTES,
        }
    }
}

/// Plan construction failure before validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CriticOraclePlanError {
    /// The oracle subject failed redaction or shape validation.
    InvalidSubject(OracleSubjectError),
    /// A required private path was not absolute.
    RelativePath {
        /// Which path field was relative.
        field: &'static str,
    },
    /// Capture budget exceeded the process-domain maximum.
    CaptureBudgetTooLarge {
        /// Requested byte budget.
        requested: u64,
        /// Maximum allowed byte budget.
        maximum: u64,
    },
    /// The constructed plan was rejected by the process-domain validator.
    PlanRejected(String),
}

impl std::fmt::Display for CriticOraclePlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSubject(error) => write!(formatter, "invalid oracle subject: {error}"),
            Self::RelativePath { field } => {
                write!(formatter, "oracle plan field {field} must be an absolute path")
            }
            Self::CaptureBudgetTooLarge { requested, maximum } => {
                write!(formatter, "oracle capture budget {requested} exceeds maximum {maximum}")
            }
            Self::PlanRejected(reason) => {
                write!(formatter, "oracle process plan rejected: {reason}")
            }
        }
    }
}

impl std::error::Error for CriticOraclePlanError {}

/// Build the structured process plan for one pinned Perl::Critic oracle run.
///
/// # Errors
///
/// Returns when the subject, filesystem inputs, or process-domain validation fail.
pub fn build_critic_oracle_process_plan(
    spec: &CriticOracleProcessSpec,
) -> Result<ProcessPlan, CriticOraclePlanError> {
    spec.subject.validate().map_err(CriticOraclePlanError::InvalidSubject)?;
    require_absolute("perlcritic_executable", spec.perlcritic_executable.expose())?;
    require_absolute("source_file", spec.source_file.expose())?;
    require_absolute("profile_file", spec.profile_file.expose())?;
    require_absolute("working_directory", spec.working_directory.expose())?;

    if spec.capture_budget_bytes > perl_subprocess_runtime::process::MAX_CAPTURE_BUDGET_BYTES {
        return Err(CriticOraclePlanError::CaptureBudgetTooLarge {
            requested: spec.capture_budget_bytes,
            maximum: perl_subprocess_runtime::process::MAX_CAPTURE_BUDGET_BYTES,
        });
    }

    let identity = spec.subject.identity();
    let subject_digest = spec.subject.digest();
    let environment = EnvironmentProjection::new(
        identity.environment_digest.clone(),
        AmbientInheritance::DenyAll,
    );
    let executable = ExecutableIdentity::resolved(
        "perlcritic",
        spec.perlcritic_executable.clone(),
        ResolutionProvenance::ConfiguredAbsolutePath,
    );
    let argv = build_argv(spec.subject.invocation(), &spec.profile_file, &spec.source_file)?;
    let subject = SubjectIdentity {
        root: Some(current_reference(identity.root_identity)),
        source: Some(current_reference(identity.source_digest)),
        configuration: Some(current_reference(identity.profile_digest)),
        toolchain: Some(current_reference(format!(
            "{}+{}+{}",
            identity.perl_identity, identity.critic_identity, identity.fixture_digest
        ))),
    };
    let authorization = AuthorizationEvidence::new(
        SchemaVersion::new(1),
        format!("conformance:#6987:{subject_digest}"),
        EvidenceFreshness::Current,
        AuthorizationStrength::HermeticNoAmbientInput,
    );
    let capture = CaptureBudget::bounded(spec.capture_budget_bytes);
    let plan = ProcessPlan::builder(
        PlanId::new(format!("critic-oracle:{subject_digest}")),
        OperationId::new(CRITIC_ORACLE_OPERATION_ID),
        OwnerDomain::ExternalCritic,
        ExecutionProfile::HermeticProbe,
        executable,
        environment,
    )
    .argv(argv)
    .cwd(CwdPolicy::ExactDirectory(spec.working_directory.clone()))
    .stdout_budget(capture)
    .stderr_budget(capture)
    .deadline(DeadlinePolicy::Wall(spec.deadline))
    .termination(TerminationPolicy::ProcessTree {
        graceful: Duration::from_millis(500),
        then_forced: true,
    })
    .subject(subject)
    .authorization(authorization)
    .claim_boundary(ClaimBoundary::linux_only())
    .build();

    match plan.validate() {
        Ok(validated) => Ok(validated.plan().clone()),
        Err(rejection) => Err(CriticOraclePlanError::PlanRejected(rejection.to_string())),
    }
}

fn require_absolute(field: &'static str, path: &Path) -> Result<(), CriticOraclePlanError> {
    if path.is_absolute() { Ok(()) } else { Err(CriticOraclePlanError::RelativePath { field }) }
}

fn current_reference(reference: impl Into<String>) -> SubjectReference {
    SubjectReference::new(reference, EvidenceFreshness::Current)
}

fn build_argv(
    invocation: &OracleInvocation,
    profile_file: &PrivatePath,
    source_file: &PrivatePath,
) -> Result<Vec<String>, CriticOraclePlanError> {
    let mut argv = Vec::new();
    argv.push("--severity".to_string());
    argv.push(invocation.severity.to_string());
    if let Some(theme) = &invocation.theme {
        argv.push("--theme".to_string());
        argv.push(theme.clone());
    }
    for policy in &invocation.include {
        argv.push("--include".to_string());
        argv.push(policy.clone());
    }
    for policy in &invocation.exclude {
        argv.push("--exclude".to_string());
        argv.push(policy.clone());
    }
    for option in &invocation.options {
        argv.push(option.clone());
    }
    argv.push("--profile".to_string());
    argv.push(path_to_argv(profile_file, "profile_file")?);
    argv.push(path_to_argv(source_file, "source_file")?);
    Ok(argv)
}

fn path_to_argv(path: &PrivatePath, field: &'static str) -> Result<String, CriticOraclePlanError> {
    path.expose().to_str().map(str::to_string).ok_or_else(|| {
        CriticOraclePlanError::PlanRejected(format!("oracle {field} path is not UTF-8"))
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use perl_subprocess_runtime::process::{FakeSupervisor, ProcessSupervisor, ScriptedRun};
    use perl_test_must::must;

    use super::*;
    use crate::critic_oracle::{OracleInvocation, OracleSubject, OracleSubjectIdentity};

    fn sample_subject() -> OracleSubject {
        OracleSubject::new(
            OracleSubjectIdentity {
                perl_identity: "perl-5.40.2-build-a".into(),
                critic_identity: "perlcritic-1.152".into(),
                fixture_digest: "sha256:fixture".into(),
                root_identity: "root-a".into(),
                source_digest: "sha256:source".into(),
                profile_digest: "sha256:profile".into(),
                environment_digest: "sha256:environment".into(),
                process_schema: "process-plan.v1".into(),
                parser_schema: "perlcritic-parser.v2".into(),
            },
            OracleInvocation {
                severity: 3,
                theme: Some("core".to_string()),
                include: vec!["Core".to_string()],
                exclude: vec!["Documentation".to_string()],
                options: vec!["--verbose".to_string()],
            },
        )
        .expect("sample subject must validate")
    }

    fn absolute(path: &str) -> PrivatePath {
        PrivatePath::new(PathBuf::from(path))
    }

    fn sample_spec(subject: OracleSubject) -> CriticOracleProcessSpec {
        CriticOracleProcessSpec::with_defaults(
            subject,
            absolute("/opt/conformance/bin/perlcritic"),
            absolute("/tmp/conformance/fixtures/sample.pl"),
            absolute("/tmp/conformance/profiles/project.perlcriticrc"),
            absolute("/tmp/conformance/fixtures"),
        )
    }

    #[test]
    fn validated_plan_uses_process_supervisor_port() {
        let plan = must(build_critic_oracle_process_plan(&sample_spec(sample_subject())));
        assert_eq!(plan.owner(), OwnerDomain::ExternalCritic);
        assert_eq!(plan.operation().as_str(), CRITIC_ORACLE_OPERATION_ID);
        assert_eq!(plan.profile(), ExecutionProfile::HermeticProbe);
        assert!(matches!(plan.environment().inheritance(), AmbientInheritance::DenyAll));
        assert!(matches!(
            plan.termination(),
            TerminationPolicy::ProcessTree { then_forced: true, .. }
        ));
        assert!(!plan.argv().iter().any(|arg| arg.contains(' ')));
        let validated = must(plan.clone().validate());
        let supervisor = FakeSupervisor::new();
        must(supervisor.script_run(ScriptedRun::exiting(1)));
        let mut handle = match supervisor.start(validated) {
            Ok(handle) => handle,
            Err(result) => panic!("supervisor refused start: {:?}", result.disposition()),
        };
        let result = handle.wait();
        assert_eq!(result.plan_id().as_str(), plan.plan_id().as_str());
        assert_eq!(supervisor.recorded_plans().len(), 1);
    }

    #[test]
    fn subject_digest_movement_changes_plan_identity() {
        let first = must(build_critic_oracle_process_plan(&sample_spec(sample_subject())));
        let mut second_subject = sample_subject();
        second_subject = OracleSubject::new(
            OracleSubjectIdentity {
                profile_digest: "sha256:profile-b".into(),
                ..second_subject.identity()
            },
            second_subject.invocation().clone(),
        )
        .expect("second subject must validate");
        let second = must(build_critic_oracle_process_plan(&sample_spec(second_subject)));
        assert_ne!(first.semantic_fingerprint(), second.semantic_fingerprint());
    }

    #[test]
    fn relative_paths_are_rejected_before_validation() {
        let error = build_critic_oracle_process_plan(&CriticOracleProcessSpec::with_defaults(
            sample_subject(),
            PrivatePath::new("relative/perlcritic"),
            absolute("/tmp/conformance/fixtures/sample.pl"),
            absolute("/tmp/conformance/profiles/project.perlcriticrc"),
            absolute("/tmp/conformance/fixtures"),
        ))
        .expect_err("relative executable must be rejected");
        assert_eq!(error, CriticOraclePlanError::RelativePath { field: "perlcritic_executable" });
    }
}
