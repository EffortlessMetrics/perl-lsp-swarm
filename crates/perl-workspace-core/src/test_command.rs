//! Deterministic, non-executing test-command planning.
//!
//! Traditional Perl distributions expose several materially different test
//! entry points: direct TAP execution with `prove`, an ExtUtils::MakeMaker
//! `make test` target, and a Module::Build `Build test` target. Choosing one by
//! filename alone is unsafe: generated runners can be missing or stale,
//! platform command names differ, `prove -l` and `prove -b` imply different
//! include roots, and a command without its exact working directory and
//! environment identity is not reproducible.
//!
//! This module turns one accepted [`ProjectEnvironmentSnapshot`] plus
//! caller-supplied generated-state evidence into independent, typed
//! [`TestCommandCandidate`]s. It plans; it never selects, authorizes, or runs
//! anything, and it never touches the filesystem.
//!
//! # Authority boundary
//!
//! - Which generated artifacts exist and whether they are current is *consumed*
//!   as [`GeneratedStateEvidence`], not derived here. Metadata authority and
//!   root discovery belong to the environment adapter.
//! - Selecting a candidate, asking the user, and validating a process plan
//!   belong to the test-execution layer.
//! - Absent evidence never becomes a runnable candidate: it is reported as
//!   [`TestCommandAdmission::NotProvenGeneratedState`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Digest;
use crate::environment::{
    BuildSystemFactRef, BuildSystemKind, EnvironmentBuildError, EnvironmentFingerprint,
    EnvironmentInputAuthority, EnvironmentInputId, EnvironmentInputState, EnvironmentLimitation,
    EnvironmentPathRef, IncludeEntryRole, ProjectEnvironmentSnapshot, ProjectRootRole,
    ToolCandidateRole, WorkspaceTrust,
};

/// Schema version for [`TestCommandPlan`].
pub const TEST_COMMAND_PLAN_SCHEMA_VERSION: u32 = 1;

const TEST_COMMAND_ID_DOMAIN: &str = "project_environment.test_command.v1";

/// Test directory used when the snapshot names no test root.
const DEFAULT_TEST_DIRECTORY: &str = "t";

/// Relative expression of a test root that is the working directory itself.
const CURRENT_DIRECTORY: &str = ".";

/// Build-output directory `prove -b` resolves relative to its working directory.
const BLIB_DIRECTORY: &str = "blib";

/// Runner family invoked by a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestRunnerKind {
    /// Direct TAP execution through `prove`.
    Prove,
    /// The `test` target of an ExtUtils::MakeMaker generated `Makefile`.
    MakeTest,
    /// The `test` action of a Module::Build generated `Build` script.
    BuildTest,
}

impl TestRunnerKind {
    const fn identity_tag(self) -> &'static str {
        match self {
            Self::Prove => "prove",
            Self::MakeTest => "make_test",
            Self::BuildTest => "build_test",
        }
    }

    const fn sort_rank(self) -> u8 {
        match self {
            Self::Prove => 0,
            Self::MakeTest => 1,
            Self::BuildTest => 2,
        }
    }
}

/// Which include roots the runner will see.
///
/// `prove -l` and `prove -b` are not interchangeable: `-l` puts the source
/// `lib/` on `@INC`, while `-b` puts the generated `blib/lib` and `blib/arch`
/// there. Only the latter can supply compiled XS artifacts, and only the latter
/// requires a build step to have run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestIncludeMode {
    /// Source `lib/` roots (`prove -l`).
    SourceLib,
    /// Generated `blib/` roots (`prove -b`).
    BlibRoots,
    /// The build system supplies its own include roots.
    BuildSystemManaged,
}

impl TestIncludeMode {
    const fn identity_tag(self) -> &'static str {
        match self {
            Self::SourceLib => "source_lib",
            Self::BlibRoots => "blib_roots",
            Self::BuildSystemManaged => "build_system_managed",
        }
    }

    const fn sort_rank(self) -> u8 {
        match self {
            Self::SourceLib => 0,
            Self::BlibRoots => 1,
            Self::BuildSystemManaged => 2,
        }
    }
}

/// A generated artifact a runner needs before it can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedArtifact {
    /// The `Makefile` produced by running `Makefile.PL`.
    Makefile,
    /// The `Build` (or `Build.bat`) script produced by running `Build.PL`.
    BuildScript,
    /// The populated `blib/lib` and `blib/arch` roots.
    BlibRoots,
}

impl GeneratedArtifact {
    const fn identity_tag(self) -> &'static str {
        match self {
            Self::Makefile => "makefile",
            Self::BuildScript => "build_script",
            Self::BlibRoots => "blib_roots",
        }
    }

    /// The authoring input whose change invalidates this artifact.
    #[must_use]
    pub const fn producer(self) -> &'static str {
        match self {
            Self::Makefile => "Makefile.PL",
            Self::BuildScript => "Build.PL",
            Self::BlibRoots => "build step",
        }
    }
}

/// Freshness of one generated artifact, as observed by the environment adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedStateFreshness {
    /// Present and current with respect to its producer.
    Current,
    /// Present but superseded by a change to its producer.
    Stale,
    /// Not present.
    Missing,
    /// Presence or freshness could not be established.
    NotProven,
}

impl GeneratedStateFreshness {
    const fn identity_tag(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Missing => "missing",
            Self::NotProven => "not_proven",
        }
    }
}

/// One observation about a generated artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedStateObservation {
    /// Observed freshness.
    pub state: GeneratedStateFreshness,
    /// Location of the artifact, when one was observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<EnvironmentPathRef>,
    /// Stable machine-readable explanation for this observation.
    pub reason_code: String,
}

impl GeneratedStateObservation {
    /// Record one observation.
    #[must_use]
    pub fn new(
        state: GeneratedStateFreshness,
        path: Option<EnvironmentPathRef>,
        reason_code: impl Into<String>,
    ) -> Self {
        Self { state, path, reason_code: reason_code.into() }
    }
}

/// Caller-supplied generated-state evidence, bound to the snapshot it describes.
///
/// An artifact with no recorded observation reads as
/// [`GeneratedStateFreshness::NotProven`], never as current, so planning fails
/// closed on an empty evidence set.
///
/// The binding is not decoration. Observing a project produces facts about one
/// configuration generation; carrying those facts into a later generation would
/// let a `Current` verdict outlive the inputs that justified it — the emitted
/// candidate would stamp the *new* snapshot's fingerprint while its readiness
/// came from the old one. [`plan_test_commands`] therefore refuses to treat
/// evidence from another snapshot as current.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedStateEvidence {
    /// Snapshot these observations were gathered against.
    observed_for: EnvironmentFingerprint,
    observations: BTreeMap<GeneratedArtifact, GeneratedStateObservation>,
}

impl GeneratedStateEvidence {
    /// Empty evidence gathered against one exact snapshot.
    ///
    /// Every artifact reads as `NotProven` until an observation is recorded.
    #[must_use]
    pub fn for_snapshot(snapshot: &ProjectEnvironmentSnapshot) -> Self {
        Self { observed_for: snapshot.fingerprint.clone(), observations: BTreeMap::new() }
    }

    /// The snapshot these observations describe.
    #[must_use]
    pub fn observed_for(&self) -> &EnvironmentFingerprint {
        &self.observed_for
    }

    /// Record one artifact observation, replacing any previous one.
    #[must_use]
    pub fn with_observation(
        mut self,
        artifact: GeneratedArtifact,
        observation: GeneratedStateObservation,
    ) -> Self {
        self.observations.insert(artifact, observation);
        self
    }

    /// Observation for one artifact, when recorded.
    #[must_use]
    pub fn observation(&self, artifact: GeneratedArtifact) -> Option<&GeneratedStateObservation> {
        self.observations.get(&artifact)
    }

    fn requirement(&self, artifact: GeneratedArtifact) -> GeneratedStateRequirement {
        self.observations.get(&artifact).map_or_else(
            || GeneratedStateRequirement {
                artifact,
                state: GeneratedStateFreshness::NotProven,
                path: None,
                reason_code: "generated_state.unobserved".to_string(),
            },
            |observation| GeneratedStateRequirement {
                artifact,
                state: observation.state,
                path: observation.path.clone(),
                reason_code: observation.reason_code.clone(),
            },
        )
    }
}

/// One generated-state precondition attached to a candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedStateRequirement {
    /// Required artifact.
    pub artifact: GeneratedArtifact,
    /// Observed freshness of that artifact.
    pub state: GeneratedStateFreshness,
    /// Observed location, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<EnvironmentPathRef>,
    /// Stable explanation for the observation.
    pub reason_code: String,
}

/// Whether a candidate's generated-state preconditions are satisfied.
///
/// This reports readiness only. Authorizing and running a candidate remain the
/// test-execution layer's decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestCommandAdmission {
    /// Every required artifact is present and current.
    Ready,
    /// A required artifact is absent.
    BlockedMissingGeneratedState,
    /// A required artifact is present but stale.
    BlockedStaleGeneratedState,
    /// A required artifact's freshness could not be established.
    NotProvenGeneratedState,
    /// A declared test root could not be expressed as an argument to this
    /// command, so running it would cover less than the project declared.
    ///
    /// The command itself is well-formed; what is missing is coverage, and
    /// silently running a subset of the declared test surface is the failure
    /// this state exists to prevent.
    BlockedIncompleteTestRoots,
}

impl TestCommandAdmission {
    const fn identity_tag(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::BlockedMissingGeneratedState => "blocked_missing_generated_state",
            Self::BlockedStaleGeneratedState => "blocked_stale_generated_state",
            Self::NotProvenGeneratedState => "not_proven_generated_state",
            Self::BlockedIncompleteTestRoots => "blocked_incomplete_test_roots",
        }
    }

    /// Whether the candidate's generated-state preconditions all hold.
    ///
    /// This is not an authorization to execute.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Rank used to surface the most consequential blocker.
    ///
    /// Incomplete test-root coverage ranks above every generated-state blocker
    /// because it is the only one that would still *run*. A missing or stale
    /// artifact stops the command and says why; an under-covering command
    /// succeeds while testing less than the project declared, so it is the
    /// verdict a caller most needs to see.
    const fn severity(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::NotProvenGeneratedState => 1,
            Self::BlockedStaleGeneratedState => 2,
            Self::BlockedMissingGeneratedState => 3,
            Self::BlockedIncompleteTestRoots => 4,
        }
    }
}

/// One planned, non-authorized test command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestCommandCandidate {
    /// Stable candidate identity.
    pub id: String,
    /// Runner family.
    pub kind: TestRunnerKind,
    /// Include roots the runner will see.
    pub include_mode: TestIncludeMode,
    /// Program to launch. Never interpolated into [`Self::argv`].
    pub program: EnvironmentPathRef,
    /// Arguments after the program.
    ///
    /// Every element is workspace-relative by construction; absolute paths are
    /// rejected at plan time so the public projection cannot leak host layout.
    pub argv: Vec<String>,
    /// Exact directory the command must run in.
    pub working_dir: EnvironmentPathRef,
    /// Identity of the environment snapshot this candidate was derived from.
    pub environment_fingerprint: EnvironmentFingerprint,
    /// Configuration generation of that snapshot.
    pub configuration_generation: u64,
    /// Workspace trust carried for the authorization layer; not enforced here.
    pub trust: WorkspaceTrust,
    /// Authority of the input that supplied this runner.
    pub authority: EnvironmentInputAuthority,
    /// Input that supplied this runner.
    pub input_id: EnvironmentInputId,
    /// Tool candidate that supplied the program, when a discovered tool did.
    ///
    /// A Module::Build launcher is the generated script itself, so it has no
    /// tool candidate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_candidate_id: Option<String>,
    /// Build-system fact that justified this runner, when one did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_system_id: Option<String>,
    /// Generated artifacts this candidate needs, with observed freshness.
    pub required_generated_state: Vec<GeneratedStateRequirement>,
    /// Whether those preconditions hold.
    pub admission: TestCommandAdmission,
    /// Stable explanation for the admission decision.
    pub reason_code: String,
}

/// A deterministic set of independent test-command candidates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestCommandPlan {
    /// Plan schema version.
    pub schema_version: u32,
    /// Workspace the plan belongs to.
    pub workspace_id: String,
    /// Identity of the environment snapshot the plan was derived from.
    pub environment_fingerprint: EnvironmentFingerprint,
    /// Configuration generation of that snapshot.
    pub configuration_generation: u64,
    /// Candidates in deterministic order.
    pub candidates: Vec<TestCommandCandidate>,
    /// Explicit limitations discovered while planning.
    pub limitations: Vec<EnvironmentLimitation>,
    /// Deterministic fingerprint over all behavior-bearing fields.
    pub fingerprint: Digest,
}

impl TestCommandPlan {
    /// Candidates whose generated-state preconditions hold.
    ///
    /// Readiness is not authorization; the caller still owns policy.
    pub fn ready_candidates(&self) -> impl Iterator<Item = &TestCommandCandidate> {
        self.candidates.iter().filter(|candidate| candidate.admission.is_ready())
    }

    /// Redacted public projection.
    #[must_use]
    pub fn public_receipt(&self) -> PublicTestCommandPlan {
        PublicTestCommandPlan {
            schema_version: self.schema_version,
            workspace_id: self.workspace_id.clone(),
            environment_fingerprint: self.environment_fingerprint.clone(),
            configuration_generation: self.configuration_generation,
            candidates: self
                .candidates
                .iter()
                .map(|candidate| PublicTestCommandCandidate {
                    id: candidate.id.clone(),
                    kind: candidate.kind,
                    include_mode: candidate.include_mode,
                    program_public_id: candidate.program.public_id.clone(),
                    argv: candidate.argv.clone(),
                    working_dir_public_id: candidate.working_dir.public_id.clone(),
                    trust: candidate.trust,
                    authority: candidate.authority,
                    input_id: candidate.input_id.clone(),
                    required_generated_state: candidate
                        .required_generated_state
                        .iter()
                        .map(|requirement| PublicGeneratedStateRequirement {
                            artifact: requirement.artifact,
                            state: requirement.state,
                            path_public_id: requirement
                                .path
                                .as_ref()
                                .map(|path| path.public_id.clone()),
                            reason_code: requirement.reason_code.clone(),
                        })
                        .collect(),
                    admission: candidate.admission,
                    reason_code: candidate.reason_code.clone(),
                })
                .collect(),
            limitation_codes: self.limitations.iter().map(|item| item.code.clone()).collect(),
            fingerprint: self.fingerprint.clone(),
        }
    }
}

/// Redacted test-command plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicTestCommandPlan {
    /// Plan schema version.
    pub schema_version: u32,
    /// Workspace identity.
    pub workspace_id: String,
    /// Environment snapshot identity.
    pub environment_fingerprint: EnvironmentFingerprint,
    /// Configuration generation.
    pub configuration_generation: u64,
    /// Redacted candidates.
    pub candidates: Vec<PublicTestCommandCandidate>,
    /// Limitation codes without internal detail.
    pub limitation_codes: Vec<String>,
    /// Exact plan fingerprint.
    pub fingerprint: Digest,
}

/// Redacted test-command candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicTestCommandCandidate {
    /// Candidate identity.
    pub id: String,
    /// Runner family.
    pub kind: TestRunnerKind,
    /// Include roots the runner will see.
    pub include_mode: TestIncludeMode,
    /// Redacted program identity.
    pub program_public_id: String,
    /// Workspace-relative arguments; safe to publish by construction.
    pub argv: Vec<String>,
    /// Redacted working-directory identity.
    pub working_dir_public_id: String,
    /// Workspace trust.
    pub trust: WorkspaceTrust,
    /// Authority of the supplying input.
    pub authority: EnvironmentInputAuthority,
    /// Supplying input.
    pub input_id: EnvironmentInputId,
    /// Redacted generated-state preconditions.
    pub required_generated_state: Vec<PublicGeneratedStateRequirement>,
    /// Admission decision.
    pub admission: TestCommandAdmission,
    /// Stable explanation code.
    pub reason_code: String,
}

/// Redacted generated-state precondition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicGeneratedStateRequirement {
    /// Required artifact.
    pub artifact: GeneratedArtifact,
    /// Observed freshness.
    pub state: GeneratedStateFreshness,
    /// Redacted artifact identity, when observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_public_id: Option<String>,
    /// Stable explanation code.
    pub reason_code: String,
}

/// Error returned while planning test commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestCommandPlanError {
    /// The supplied snapshot did not satisfy its own invariants.
    InvalidSnapshot(EnvironmentBuildError),
    /// No active workspace root exists, so no working directory can be named.
    MissingWorkspaceRoot,
    /// More than one active workspace root exists, so no *single* working
    /// directory can be named. A candidate stamps one working directory it must
    /// run in; silently picking the first would drop every other root's tree
    /// from the plan without saying so.
    AmbiguousWorkspaceRoots,
    /// An argument would have leaked an absolute path into a public receipt.
    AbsolutePathInArgv {
        /// Candidate runner family.
        kind: TestRunnerKind,
        /// The offending argument.
        argument: String,
    },
}

impl std::fmt::Display for TestCommandPlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSnapshot(error) => {
                write!(formatter, "environment snapshot is not authoritative: {error:?}")
            }
            Self::MissingWorkspaceRoot => {
                formatter.write_str("no active workspace root supplies a working directory")
            }
            Self::AmbiguousWorkspaceRoots => formatter.write_str(
                "more than one active workspace root exists, so no single working \
                 directory can be named",
            ),
            Self::AbsolutePathInArgv { kind, argument } => write!(
                formatter,
                "{} argument `{argument}` is an absolute path",
                kind.identity_tag()
            ),
        }
    }
}

impl std::error::Error for TestCommandPlanError {}

/// Plan every independent test command the environment supports.
///
/// Candidate generation is separate from authorization and execution: this
/// function reads no files, spawns no process, and repairs no stale state. A
/// runner whose generated state is missing, stale, or unobserved is still
/// reported, with an explicit reason, so a caller can explain the gap instead of
/// silently losing the entry point.
///
/// # Errors
///
/// Returns [`TestCommandPlanError`] when the snapshot is not authoritative, when
/// no active workspace root names a working directory, or when a constructed
/// argument would leak an absolute path.
pub fn plan_test_commands(
    snapshot: &ProjectEnvironmentSnapshot,
    evidence: &GeneratedStateEvidence,
) -> Result<TestCommandPlan, TestCommandPlanError> {
    snapshot.validate().map_err(TestCommandPlanError::InvalidSnapshot)?;

    let working_dir = workspace_working_dir(snapshot)?;
    let mut limitations = Vec::new();

    // Evidence gathered against another snapshot cannot speak for this one.
    let evidence_matches_snapshot = evidence.observed_for() == &snapshot.fingerprint;
    if !evidence_matches_snapshot {
        limitations.push(EnvironmentLimitation {
            code: "test_command.generated_state_from_another_snapshot".to_string(),
            detail: format!(
                "generated-state evidence was gathered against snapshot {} but this plan \
                 describes {}, so no generated artifact can be treated as current",
                evidence.observed_for(),
                snapshot.fingerprint
            ),
            input_id: None,
        });
    }
    let test_roots = relative_test_directories(snapshot, &working_dir, &mut limitations);

    let mut candidates = Vec::new();

    // A blib *role* is not enough. `prove -b` resolves `blib/lib` and
    // `blib/arch` relative to the directory it runs in, so only this working
    // directory's own build output can justify the form. A dependency's blib
    // root is a legitimate include entry describing a different tree, and
    // offering `-b` on its strength would run against build output the
    // workspace may not even have.
    let mut declared_blib_role = false;
    let mut blib_available = false;
    for entry in snapshot.active_include_entries() {
        if !matches!(entry.role, IncludeEntryRole::BlibLib | IncludeEntryRole::BlibArch) {
            continue;
        }
        declared_blib_role = true;
        if is_workspace_blib(&working_dir, &entry.path.normalized) {
            blib_available = true;
        }
    }

    for tool in snapshot.active_tool_candidates() {
        let Some(authority) = input_authority(snapshot, &tool.input_id) else {
            continue;
        };

        match classify_runner(&tool.role, &tool.logical_name) {
            Some(RunnerShape::Prove) => {
                let mut source_argv = vec!["-l".to_string()];
                source_argv.extend(test_roots.arguments.iter().cloned());
                candidates.push(build_candidate(
                    snapshot,
                    &working_dir,
                    TestRunnerKind::Prove,
                    TestIncludeMode::SourceLib,
                    tool.executable.clone(),
                    source_argv,
                    authority,
                    tool.input_id.clone(),
                    Some(tool.id.clone()),
                    None,
                    Vec::new(),
                    test_roots.coverage,
                )?);

                if blib_available {
                    let mut blib_argv = vec!["-b".to_string()];
                    blib_argv.extend(test_roots.arguments.iter().cloned());
                    candidates.push(build_candidate(
                        snapshot,
                        &working_dir,
                        TestRunnerKind::Prove,
                        TestIncludeMode::BlibRoots,
                        tool.executable.clone(),
                        blib_argv,
                        authority,
                        tool.input_id.clone(),
                        Some(tool.id.clone()),
                        None,
                        vec![bind_requirement_to_working_dir(
                            evidence.requirement(GeneratedArtifact::BlibRoots),
                            &working_dir,
                            evidence_matches_snapshot,
                            None,
                        )],
                        test_roots.coverage,
                    )?);
                }
            }
            Some(RunnerShape::Make(flavor)) => {
                for build in active_build_systems(snapshot, BuildFamily::MakeMaker) {
                    candidates.push(build_candidate(
                        snapshot,
                        &working_dir,
                        TestRunnerKind::MakeTest,
                        TestIncludeMode::BuildSystemManaged,
                        tool.executable.clone(),
                        vec!["test".to_string()],
                        authority,
                        tool.input_id.clone(),
                        Some(tool.id.clone()),
                        Some(build.id.clone()),
                        vec![bind_requirement_to_working_dir(
                            evidence.requirement(GeneratedArtifact::Makefile),
                            &working_dir,
                            evidence_matches_snapshot,
                            Some(flavor),
                        )],
                        // The `test` target chooses its own files from the
                        // generated makefile; this plan passes no test roots, so
                        // an inexpressible root does not narrow this command.
                        TestRootCoverage::Complete,
                    )?);
                }
            }
            None => {}
        }
    }

    // Withholding the `-b` form is correct, but it must not be silent. The
    // project declared a blib role and receives no `-b` candidate, and a caller
    // needs to know that an entry point was considered and rejected rather than
    // never existing. The reason is deliberately cause-agnostic: a dependency's
    // tree, a detached path, and a casing the producer did not normalize are all
    // "this is not the tree `prove -b` would resolve", and distinguishing them
    // would need filesystem facts this module does not have.
    //
    // Reported only when a `prove` candidate was actually emitted, because this
    // limitation explains why *that* candidate has no blib variant. Gating on an
    // emitted candidate rather than on a discovered `prove` tool also covers a
    // tool whose input carries no authority: in both cases no prove form exists,
    // so blaming the blib root would send a caller to fix something that still
    // could not produce a command.
    //
    // A project that declared no blib role at all lost nothing, so it gets no
    // limitation — `a_project_with_no_blib_role_reports_no_blib_limitation`.
    if declared_blib_role
        && !blib_available
        && candidates.iter().any(|candidate| candidate.kind == TestRunnerKind::Prove)
    {
        limitations.push(EnvironmentLimitation {
            code: "test_command.no_workspace_blib_root".to_string(),
            detail: "an active include entry carries a blib role, but none of them is this \
                     working directory's own `blib` tree, so the `prove -b` form is not offered"
                .to_string(),
            input_id: None,
        });
    }

    // The Module::Build launcher is the generated script itself, so its program
    // comes from generated-state evidence rather than from a discovered tool.
    // That is what keeps `./Build` and `Build.bat` apart without guessing a
    // platform or probing the host.
    let build_script = bind_requirement_to_working_dir(
        evidence.requirement(GeneratedArtifact::BuildScript),
        &working_dir,
        evidence_matches_snapshot,
        None,
    );
    for build in active_build_systems(snapshot, BuildFamily::ModuleBuild) {
        let Some(authority) = input_authority(snapshot, &build.input_id) else {
            continue;
        };
        // A path that cannot name an executable is not a launcher, so it is
        // treated exactly like an unobserved one rather than becoming the
        // program of a candidate that could never run. An empty path names
        // nothing; the working directory names a directory, not a script.
        let launcher = build_script.path.clone().filter(|path| {
            !path.normalized.is_empty()
                && !path.public_id.is_empty()
                && !is_working_directory_itself(&working_dir, &path.normalized)
        });

        match launcher {
            Some(program) => candidates.push(build_candidate(
                snapshot,
                &working_dir,
                TestRunnerKind::BuildTest,
                TestIncludeMode::BuildSystemManaged,
                program,
                vec!["test".to_string()],
                authority,
                build.input_id.clone(),
                None,
                Some(build.id.clone()),
                vec![build_script.clone()],
                // As with `make test`, the generated script selects its own
                // files; no test root reaches this argv.
                TestRootCoverage::Complete,
            )?),
            None => limitations.push(EnvironmentLimitation {
                code: "test_command.build_script_location_unknown".to_string(),
                detail: format!(
                    "Module::Build fact {} declares a Build test entry point, but no generated \
                     Build script location was observed, so no launcher shape can be named",
                    build.id
                ),
                input_id: Some(build.input_id.clone()),
            }),
        }
    }

    candidates.sort_by(|left, right| {
        left.authority
            .precedence_rank()
            .cmp(&right.authority.precedence_rank())
            .then(left.kind.sort_rank().cmp(&right.kind.sort_rank()))
            .then(left.include_mode.sort_rank().cmp(&right.include_mode.sort_rank()))
            .then(left.id.cmp(&right.id))
    });
    candidates.dedup_by(|left, right| left.id == right.id);

    limitations.sort();
    limitations.dedup();

    let fingerprint = compute_plan_fingerprint(
        snapshot.workspace_id.as_str(),
        &snapshot.fingerprint,
        snapshot.configuration_generation,
        &candidates,
        &limitations,
    );

    Ok(TestCommandPlan {
        schema_version: TEST_COMMAND_PLAN_SCHEMA_VERSION,
        workspace_id: snapshot.workspace_id.clone(),
        environment_fingerprint: snapshot.fingerprint.clone(),
        configuration_generation: snapshot.configuration_generation,
        candidates,
        limitations,
        fingerprint,
    })
}

enum RunnerShape {
    Prove,
    Make(MakeFlavor),
}

#[derive(Clone, Copy)]
enum BuildFamily {
    MakeMaker,
    ModuleBuild,
}

/// Classify a discovered tool by its logical name.
///
/// Platform launcher differences (`make` / `gmake` / `dmake` / `nmake`) are read
/// from the name the environment adapter recorded; nothing is guessed from the
/// host this planner happens to run on.
fn classify_runner(role: &ToolCandidateRole, logical_name: &str) -> Option<RunnerShape> {
    match role {
        ToolCandidateRole::TestRunner if logical_name == "prove" => Some(RunnerShape::Prove),
        ToolCandidateRole::TestRunner | ToolCandidateRole::BuildTool => {
            let flavor = match logical_name {
                // Only `gmake` names GNU make. A bare `make` is GNU on Linux
                // and BSD make on a BSD; treating it as GNU would guess the
                // host, which the doctrine above forbids.
                "gmake" => MakeFlavor::Gnu,
                "make" => MakeFlavor::Portable,
                "nmake" => MakeFlavor::Nmake,
                "dmake" => MakeFlavor::Dmake,
                _ => return None,
            };
            Some(RunnerShape::Make(flavor))
        }
        _ => None,
    }
}

fn active_build_systems(
    snapshot: &ProjectEnvironmentSnapshot,
    family: BuildFamily,
) -> Vec<&BuildSystemFactRef> {
    snapshot
        .build_systems
        .iter()
        .filter(|build| {
            snapshot.input_state(&build.input_id).is_some_and(EnvironmentInputState::is_active)
        })
        .filter(|build| {
            matches!(
                (family, &build.kind),
                (BuildFamily::MakeMaker, BuildSystemKind::ExtUtilsMakeMaker)
                    | (BuildFamily::ModuleBuild, BuildSystemKind::ModuleBuild)
            )
        })
        .collect()
}

fn input_authority(
    snapshot: &ProjectEnvironmentSnapshot,
    input_id: &EnvironmentInputId,
) -> Option<EnvironmentInputAuthority> {
    snapshot.inputs.iter().find(|input| input.id == *input_id).map(|input| input.authority)
}

fn workspace_working_dir(
    snapshot: &ProjectEnvironmentSnapshot,
) -> Result<EnvironmentPathRef, TestCommandPlanError> {
    // Ambiguity is about *directories*, not records. `ProjectRoot` identity
    // includes its `input_id`, so one directory declared by two inputs — say
    // workspace-configured and also detected by convention — is two records
    // that both survive dedup while still naming a single working directory.
    // Counting records would reject that plan outright.
    //
    // The whole `EnvironmentPathRef` is compared, not just `normalized`: the
    // redacted half reaches the public receipt, so two records agreeing on the
    // directory but disagreeing on its published identity would make the
    // receipt depend on which record was picked. That is a real ambiguity.
    let mut workspace_roots =
        snapshot.active_project_roots().filter(|root| root.role == ProjectRootRole::Workspace);
    let first = workspace_roots.next().ok_or(TestCommandPlanError::MissingWorkspaceRoot)?;
    if workspace_roots.any(|root| root.path != first.path) {
        return Err(TestCommandPlanError::AmbiguousWorkspaceRoots);
    }
    Ok(first.path.clone())
}

/// Whether emitted arguments cover every test root the project declared.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TestRootCoverage {
    /// Every active test root became an argument — or none was declared, and
    /// the conventional default stands in for the project's own choice.
    Complete,
    /// At least one active test root could not be expressed, so the arguments
    /// describe less than the declared test surface.
    Incomplete,
}

/// Workspace-relative test roots, in deterministic order, with their coverage.
///
/// Only a test root that is actually inside the working directory can become a
/// relative argument. Anything else would need path arithmetic this module
/// deliberately does not own, so it is dropped with a recorded limitation
/// rather than emitted as a wrong argument.
///
/// Dropping it also makes the remaining arguments an incomplete description of
/// the declared test surface, and the coverage verdict carries that fact to
/// admission. A limitation alone would not: a caller reading a `Ready` prove
/// candidate would run a command that passes while testing less than the
/// project declared.
struct TestRootArguments {
    arguments: Vec<String>,
    coverage: TestRootCoverage,
}

fn relative_test_directories(
    snapshot: &ProjectEnvironmentSnapshot,
    working_dir: &EnvironmentPathRef,
    limitations: &mut Vec<EnvironmentLimitation>,
) -> TestRootArguments {
    let mut directories = Vec::new();
    let mut had_unrelatable_root = false;

    for root in snapshot.active_project_roots() {
        if root.role != ProjectRootRole::Test {
            continue;
        }
        match relative_child(&working_dir.normalized, &root.path.normalized) {
            Some(relative) => directories.push(relative),
            None => had_unrelatable_root = true,
        }
    }

    directories.sort();
    directories.dedup();

    if had_unrelatable_root {
        limitations.push(EnvironmentLimitation {
            code: "test_command.test_root_outside_working_directory".to_string(),
            detail: "at least one active test root is not inside the workspace working directory, \
                     so it cannot become a workspace-relative argument"
                .to_string(),
            input_id: None,
        });
    }

    if directories.is_empty() {
        limitations.push(EnvironmentLimitation {
            code: "test_command.assumed_default_test_directory".to_string(),
            detail: format!(
                "no active test root was supplied, so the conventional `{DEFAULT_TEST_DIRECTORY}` \
                 directory is assumed"
            ),
            input_id: None,
        });
        directories.push(DEFAULT_TEST_DIRECTORY.to_string());
    }

    let coverage = if had_unrelatable_root {
        TestRootCoverage::Incomplete
    } else {
        TestRootCoverage::Complete
    };

    TestRootArguments { arguments: directories, coverage }
}

/// Which launcher will invoke `make test`.
///
/// The launcher decides which makefile filenames it will *discover* when no
/// `-f` / `/F` is passed. Applying GNU make's defaults to `nmake` or `dmake`
/// would let evidence at `GNUmakefile` mark a command Ready that would never
/// read that file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MakeFlavor {
    /// `gmake` — unambiguously GNU make. Discovers `GNUmakefile`, `makefile`,
    /// then `Makefile`.
    Gnu,
    /// A bare `make`, whose implementation is not named. On Linux it is
    /// normally GNU make; on a BSD it is BSD make, which never reads
    /// `GNUmakefile`. Nothing in the snapshot says which, and this module does
    /// not guess from the host it happens to run on, so only the names every
    /// implementation discovers count.
    Portable,
    /// Microsoft nmake. Windows-only, but Windows filesystems can be
    /// case-sensitive per directory and the snapshot records no case fact, so
    /// discovery is exact — see [`MakeFlavor::discovers`].
    Nmake,
    /// Digital Mars dmake.
    Dmake,
}

impl MakeFlavor {
    /// Whether this launcher will discover the makefile at `name` without a
    /// filename passed on the command line.
    ///
    /// Matching is always exact. Case sensitivity is a property of the
    /// *filesystem*, not the launcher, and no snapshot fact records it: Windows
    /// filesystems can be case-sensitive per directory (the NTFS flag used for
    /// WSL interop), and a case-sensitive directory anywhere makes a
    /// differently cased name a file the launcher's default lookup would not
    /// find. Folding case for `nmake` would therefore claim a readiness this
    /// module cannot prove, so a differently cased observation stays
    /// [`GeneratedStateFreshness::NotProven`] — the fail-closed default — until
    /// a filesystem-case fact exists to justify anything looser.
    ///
    /// The launchers differ only in *which* default names they look for:
    /// GNU make also reads `GNUmakefile`; the rest read the conventional
    /// `Makefile` / `makefile` that ExtUtils::MakeMaker emits.
    fn discovers(self, name: &str) -> bool {
        match self {
            Self::Gnu => matches!(name, "Makefile" | "makefile" | "GNUmakefile"),
            Self::Portable | Self::Dmake | Self::Nmake => matches!(name, "Makefile" | "makefile"),
        }
    }
}

/// Whether an observed path is the working directory itself, not a file in it.
///
/// `relative_child` reduces the directory to `.`, which is a legitimate answer
/// for a test-root argument — `prove -l .` runs the tests there. It is never a
/// legitimate launcher: a directory cannot be executed, so a candidate whose
/// program is `.` could not run and must not be published.
fn is_working_directory_itself(working_dir: &EnvironmentPathRef, candidate: &str) -> bool {
    relative_child(&working_dir.normalized, candidate)
        .is_some_and(|relative| relative == CURRENT_DIRECTORY)
}

/// Whether a path is this working directory's own `blib` tree.
///
/// `prove -b` puts `blib/lib` and `blib/arch` on `@INC` relative to the
/// directory it runs in, so only a `blib` beneath that directory can justify or
/// satisfy the blib form. Accepts the tree root and anything inside it —
/// `blib`, `blib/lib`, `blib/arch` — and rejects a sibling that merely starts
/// with the same letters, such as `blibx`.
fn is_workspace_blib(working_dir: &EnvironmentPathRef, candidate: &str) -> bool {
    relative_child(&working_dir.normalized, candidate).is_some_and(|relative| {
        relative == BLIB_DIRECTORY || relative.starts_with(&format!("{BLIB_DIRECTORY}/"))
    })
}

/// Which characters this platform recognizes as a path separator.
///
/// Windows accepts both `/` and `\\`; POSIX accepts only `/`, and a backslash
/// is a legal filename character there. The snapshot does not carry a platform
/// tag, so the parent's own shape decides: a `\\` in the parent means the
/// producer emitted a Windows path and the child is read the same way; a parent
/// with only forward slashes is POSIX, and treating `\\` as a separator there
/// would fabricate a child from a sibling whose name literally contains one.
const fn path_separators(parent: &str) -> &'static [char] {
    // `str::contains(char)` is not `const`, so scan by bytes; a backslash is
    // ASCII, so a byte scan is equivalent to a char scan for this predicate.
    let bytes = parent.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            return &['/', '\\'];
        }
        index += 1;
    }
    &['/']
}

/// Strip a normalized parent prefix, returning the child portion.
///
/// Returns `None` when `child` is not under `parent`. Both inputs are already
/// normalized by the environment producer, so this is a segment-boundary prefix
/// test rather than a path canonicalizer.
///
/// Containment is a *segment* relationship, not a textual one: `/ws-other` is a
/// sibling of `/ws`, so the remainder must begin at a separator. Without that
/// check `/ws-other/t` would reduce to `-other/t` — not merely the wrong
/// directory, but an argument a runner reads as an option rather than a path.
///
/// A genuinely contained directory whose name starts with `-` has the same
/// parsing hazard but is not the same problem: it is a real declared root, so it
/// is encoded as `./-name` rather than refused. Dropping it would fall back to
/// the conventional default and silently run a *different* directory than the
/// project declared.
///
/// Which characters count as separators is decided by the parent — see
/// [`path_separators`]. On POSIX a `\\` in the child is a filename character,
/// so `/ws\\outside/t` is a sibling of `/ws`, not a child.
fn relative_child(parent: &str, child: &str) -> Option<String> {
    let separators = path_separators(parent);
    let trimmed_parent = parent.trim_end_matches(separators);
    let remainder = child.strip_prefix(trimmed_parent)?;

    // The child *is* the parent. Relative to the working directory that is the
    // current directory, which is a usable argument — reporting it as
    // unrelatable would substitute a different directory than the one declared.
    if remainder.is_empty() {
        return Some(CURRENT_DIRECTORY.to_string());
    }

    // Only a separator opens a new segment. `starts_with` on a `&[char]`
    // checks the first char against each in turn.
    if !trimmed_parent.is_empty() && !remainder.starts_with(separators) {
        return None;
    }

    let relative = remainder.trim_start_matches(separators);
    if relative.is_empty() {
        return Some(CURRENT_DIRECTORY.to_string());
    }
    if is_absolute_path(relative) {
        return None;
    }

    // On Windows both separators are legal; normalize to `/` so the emitted
    // argument is uniform. On POSIX a `\\` is a filename character and must
    // survive verbatim — replacing it would silently rename a file.
    let relative =
        if separators.contains(&'\\') { relative.replace('\\', "/") } else { relative.to_string() };
    if relative.starts_with('-') {
        return Some(format!("{CURRENT_DIRECTORY}/{relative}"));
    }
    Some(relative)
}

#[allow(clippy::too_many_arguments)]
fn build_candidate(
    snapshot: &ProjectEnvironmentSnapshot,
    working_dir: &EnvironmentPathRef,
    kind: TestRunnerKind,
    include_mode: TestIncludeMode,
    program: EnvironmentPathRef,
    argv: Vec<String>,
    authority: EnvironmentInputAuthority,
    input_id: EnvironmentInputId,
    tool_candidate_id: Option<String>,
    build_system_id: Option<String>,
    required_generated_state: Vec<GeneratedStateRequirement>,
    test_root_coverage: TestRootCoverage,
) -> Result<TestCommandCandidate, TestCommandPlanError> {
    for argument in &argv {
        if is_absolute_path(argument) {
            return Err(TestCommandPlanError::AbsolutePathInArgv {
                kind,
                argument: argument.clone(),
            });
        }
    }

    let (admission, reason_code) = admit(&required_generated_state, test_root_coverage);

    // Provenance is part of identity, not decoration hung off it. Two build
    // facts can justify the same command shape; if the tool and build-fact
    // identities were excluded here the two candidates would share an id and
    // one would be silently dropped by the dedup below, discarding a provenance
    // chain a consumer needs to explain the offer. This follows the sibling
    // model in `environment.rs`, where the same physical path may still be
    // several materially distinct occurrences.
    let mut identity_fields: Vec<&str> = vec![
        kind.identity_tag(),
        include_mode.identity_tag(),
        program.normalized.as_str(),
        working_dir.normalized.as_str(),
        input_id.as_str(),
        tool_candidate_id.as_deref().unwrap_or(""),
        build_system_id.as_deref().unwrap_or(""),
    ];
    identity_fields.extend(argv.iter().map(String::as_str));
    let id = stable_id(TEST_COMMAND_ID_DOMAIN, &identity_fields);

    Ok(TestCommandCandidate {
        id,
        kind,
        include_mode,
        program,
        argv,
        working_dir: working_dir.clone(),
        environment_fingerprint: snapshot.fingerprint.clone(),
        configuration_generation: snapshot.configuration_generation,
        trust: snapshot.trust,
        authority,
        input_id,
        tool_candidate_id,
        build_system_id,
        required_generated_state,
        admission,
        reason_code,
    })
}

/// Re-bind an observed artifact to the command that will actually consume it.
///
/// A freshness verdict is about an artifact at a location; readiness is about
/// the command in `working_dir`. `make test` reads the makefile in its working
/// directory and `./Build test` runs the script there, so an artifact observed
/// anywhere else is not the one the emitted argv would use. An observation whose
/// path is structurally unusable is not evidence at all.
///
/// Either way the requirement degrades to [`GeneratedStateFreshness::NotProven`]
/// rather than being accepted as current: the artifact may well be fresh, but
/// this command's readiness is unproven.
fn bind_requirement_to_working_dir(
    mut requirement: GeneratedStateRequirement,
    working_dir: &EnvironmentPathRef,
    evidence_matches_snapshot: bool,
    // Only the [`GeneratedArtifact::Makefile`] arm consults the launcher, so
    // this is `None` for `BuildScript` and `BlibRoots`. A `None` reaching the
    // Makefile arm is a caller wiring bug; treated as undiscoverable so a
    // misroute cannot produce a Ready candidate.
    make_flavor: Option<MakeFlavor>,
) -> GeneratedStateRequirement {
    let artifact = requirement.artifact.identity_tag();
    let downgrade = |requirement: &mut GeneratedStateRequirement, suffix: &str| {
        requirement.state = GeneratedStateFreshness::NotProven;
        requirement.reason_code = format!("generated_state.{suffix}.{artifact}");
    };

    // Checked before the state-specific paths below, because a mismatch
    // invalidates every verdict, not only `Current`. A foreign `Stale` or
    // `Missing` is not a weaker claim about this snapshot — it is a claim about
    // a different one, and reporting it verbatim would attribute another
    // generation's observation to this plan. The path goes with it: the
    // Module::Build launcher is taken from this field, so leaving it would let
    // an obsolete location shape a command built for the current snapshot.
    if !evidence_matches_snapshot {
        downgrade(&mut requirement, "snapshot_mismatch");
        requirement.path = None;
        return requirement;
    }

    if requirement.state != GeneratedStateFreshness::Current {
        return requirement;
    }

    match requirement.path.as_ref() {
        Some(path) if path.normalized.is_empty() || path.public_id.is_empty() => {
            downgrade(&mut requirement, "unusable_path");
        }
        Some(path) => {
            // The artifact must sit directly in the working directory: the
            // emitted argv passes neither `make -C/-f` nor a script path.
            // Where the artifact must sit depends on how the command reaches
            // it, so this is per-artifact rather than one blanket rule. A
            // uniform direct-child test would reject the conventional
            // `blib/lib`, making *located* blib evidence less usable than
            // unlocated evidence — more information producing a worse verdict.
            let contained = match requirement.artifact {
                // `make test` reads the makefile in its working directory and
                // `./Build test` runs the script there; neither is given a
                // path, so the artifact must be directly in that directory.
                // `make test` is passed no `-f` (or `/F`), so it discovers its
                // makefile *by name* — and the name it discovers depends on the
                // launcher. `nmake` looks for `MAKEFILE`, not `GNUmakefile`; a
                // launcher-blind discovery set would mark `nmake test` Ready
                // against a file that command would never read.
                GeneratedArtifact::Makefile => make_flavor.is_some_and(|flavor| {
                    relative_child(&working_dir.normalized, &path.normalized)
                        .is_some_and(|relative| flavor.discovers(&relative))
                }),
                // The Build script is different: its path becomes the program,
                // so the command reads exactly what was observed and the file
                // name carries no meaning. Requiring one would reject the
                // legitimate `Build.bat` variant.
                //
                // Whether the path can name a *file* at all is a separate
                // question, answered once at the launcher rather than repeated
                // here: `relative_child` reduces the working directory itself to
                // `.`, and a candidate whose program is a directory is dropped
                // before it is built.
                GeneratedArtifact::BuildScript => {
                    relative_child(&working_dir.normalized, &path.normalized)
                        .is_some_and(|relative| !relative.contains('/'))
                }
                // `prove -b` resolves `blib/lib` and `blib/arch` relative to
                // the working directory, so an observed root is usable exactly
                // when it lies within that `blib` tree.
                GeneratedArtifact::BlibRoots => is_workspace_blib(working_dir, &path.normalized),
            };
            if !contained {
                downgrade(&mut requirement, "outside_working_directory");
            }
        }
        None if requirement.artifact != GeneratedArtifact::BlibRoots => {
            // A makefile or launcher cannot be bound to this command without a
            // location; `blib` is a fixed directory relative to the runner.
            downgrade(&mut requirement, "unlocated");
        }
        None => {}
    }

    requirement
}

/// Decide admission from the weakest precondition.
///
/// A missing artifact is a stronger statement than a stale one, and an
/// unobserved artifact must never read as ready. Incomplete test-root coverage
/// is not a generated-state fact at all, but it reaches the same verdict: a
/// command that would test less than the project declared is not ready either.
fn admit(
    requirements: &[GeneratedStateRequirement],
    test_root_coverage: TestRootCoverage,
) -> (TestCommandAdmission, String) {
    let mut worst = TestCommandAdmission::Ready;
    let mut reason = "test_command.no_generated_state_required".to_string();

    for requirement in requirements {
        let (candidate_admission, suffix) = match requirement.state {
            GeneratedStateFreshness::Current => continue,
            GeneratedStateFreshness::Missing => {
                (TestCommandAdmission::BlockedMissingGeneratedState, "missing_generated_state")
            }
            GeneratedStateFreshness::Stale => {
                (TestCommandAdmission::BlockedStaleGeneratedState, "stale_generated_state")
            }
            GeneratedStateFreshness::NotProven => {
                (TestCommandAdmission::NotProvenGeneratedState, "not_proven_generated_state")
            }
        };

        if candidate_admission.severity() > worst.severity() {
            worst = candidate_admission;
            reason = format!("test_command.{suffix}.{}", requirement.artifact.identity_tag());
        }
    }

    if worst.is_ready() && !requirements.is_empty() {
        reason = "test_command.generated_state_current".to_string();
    }

    // Applied after the loop so it can outrank every generated-state verdict
    // without the loop having to special-case a non-artifact precondition.
    if test_root_coverage == TestRootCoverage::Incomplete
        && TestCommandAdmission::BlockedIncompleteTestRoots.severity() > worst.severity()
    {
        worst = TestCommandAdmission::BlockedIncompleteTestRoots;
        reason = "test_command.test_root_outside_working_directory".to_string();
    }

    (worst, reason)
}

/// Whether an argument is an absolute path on either POSIX or Windows.
///
/// Planning runs on one host but the snapshot it consumes may describe another,
/// so both shapes are rejected regardless of the compilation target.
fn is_absolute_path(value: &str) -> bool {
    if value.starts_with('/') || value.starts_with('\\') {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn compute_plan_fingerprint(
    workspace_id: &str,
    environment_fingerprint: &EnvironmentFingerprint,
    configuration_generation: u64,
    candidates: &[TestCommandCandidate],
    limitations: &[EnvironmentLimitation],
) -> Digest {
    let mut material = String::new();
    push_field(&mut material, "schema", &TEST_COMMAND_PLAN_SCHEMA_VERSION.to_string());
    push_field(&mut material, "workspace", workspace_id);
    push_field(&mut material, "environment", environment_fingerprint.as_str());
    push_field(&mut material, "generation", &configuration_generation.to_string());

    for candidate in candidates {
        push_field(&mut material, "candidate.id", candidate.id.as_str());
        push_field(&mut material, "candidate.kind", candidate.kind.identity_tag());
        push_field(&mut material, "candidate.include", candidate.include_mode.identity_tag());
        push_field(&mut material, "candidate.program", candidate.program.normalized.as_str());
        // The redacted half is hashed alongside the internal one because
        // `public_receipt` publishes it. A fingerprint that moved only with
        // `normalized` would let two materially different receipts share a key,
        // and a fingerprint-keyed cache would then serve a receipt that no
        // longer matches the plan it claims to describe.
        push_field(&mut material, "candidate.program.public", candidate.program.public_id.as_str());
        for argument in &candidate.argv {
            push_field(&mut material, "candidate.arg", argument.as_str());
        }
        push_field(&mut material, "candidate.cwd", candidate.working_dir.normalized.as_str());
        push_field(&mut material, "candidate.cwd.public", candidate.working_dir.public_id.as_str());
        push_field(&mut material, "candidate.trust", trust_tag(candidate.trust));
        push_field(&mut material, "candidate.input", candidate.input_id.as_str());
        push_field(
            &mut material,
            "candidate.tool",
            candidate.tool_candidate_id.as_deref().unwrap_or(""),
        );
        push_field(
            &mut material,
            "candidate.build",
            candidate.build_system_id.as_deref().unwrap_or(""),
        );
        for requirement in &candidate.required_generated_state {
            push_field(&mut material, "candidate.need", requirement.artifact.identity_tag());
            push_field(&mut material, "candidate.need.state", requirement.state.identity_tag());
            push_field(
                &mut material,
                "candidate.need.path",
                requirement.path.as_ref().map_or("", |path| path.normalized.as_str()),
            );
            push_field(
                &mut material,
                "candidate.need.path.public",
                requirement.path.as_ref().map_or("", |path| path.public_id.as_str()),
            );
            push_field(&mut material, "candidate.need.reason", requirement.reason_code.as_str());
        }
        push_field(&mut material, "candidate.admission", candidate.admission.identity_tag());
        push_field(&mut material, "candidate.reason", candidate.reason_code.as_str());
    }

    for limitation in limitations {
        push_field(&mut material, "limitation.code", limitation.code.as_str());
        push_field(&mut material, "limitation.detail", limitation.detail.as_str());
    }

    Digest::of(&material)
}

const fn trust_tag(trust: WorkspaceTrust) -> &'static str {
    match trust {
        WorkspaceTrust::Trusted => "trusted",
        WorkspaceTrust::Untrusted => "untrusted",
        WorkspaceTrust::Unknown => "unknown",
    }
}

fn push_field(output: &mut String, tag: &str, value: &str) {
    output.push_str(tag.len().to_string().as_str());
    output.push(':');
    output.push_str(tag);
    output.push(':');
    output.push_str(value.len().to_string().as_str());
    output.push(':');
    output.push_str(value);
}

fn stable_id(domain: &str, fields: &[&str]) -> String {
    let mut material = String::new();
    push_field(&mut material, "domain", domain);
    for field in fields {
        push_field(&mut material, "field", field);
    }
    format!("{domain}:fnv64:{:016x}", crate::fnv1a(material.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::{
        EnvironmentInput, ProjectEnvironmentSnapshotBuilder, ProjectRoot, ToolCandidateRole,
    };

    #[test]
    fn absolute_paths_are_recognized_on_both_platform_shapes() {
        for absolute in ["/t", "/ws/t", "\\t", "C:/ws/t", "c:\\ws\\t", "Z:/x"] {
            assert!(is_absolute_path(absolute), "`{absolute}` is absolute");
        }
        for relative in ["t", "xt/author", "t/../t", "", "C:", "C:x", "cd/e"] {
            assert!(!is_absolute_path(relative), "`{relative}` is relative");
        }
    }

    #[test]
    fn relative_child_strips_only_a_real_parent_prefix() {
        assert_eq!(relative_child("/ws", "/ws/t").as_deref(), Some("t"));
        assert_eq!(relative_child("/ws/", "/ws/xt/author").as_deref(), Some("xt/author"));
        assert_eq!(relative_child("C:\\ws", "C:\\ws\\t").as_deref(), Some("t"));

        assert_eq!(relative_child("/ws", "/elsewhere/t"), None, "unrelated root");
    }

    /// A test root at the workspace itself is not unrelatable — relative to the
    /// working directory it is the current directory, and saying so is more
    /// honest than substituting a different directory.
    #[test]
    fn a_root_equal_to_the_parent_is_the_current_directory() {
        assert_eq!(relative_child("/ws", "/ws").as_deref(), Some("."));
        assert_eq!(relative_child("/ws/", "/ws").as_deref(), Some("."));
        assert_eq!(relative_child("/ws", "/ws/").as_deref(), Some("."));
        assert_eq!(relative_child("C:\\ws", "C:\\ws").as_deref(), Some("."));
    }

    /// A textual prefix is not containment. `/ws-other` is a sibling of `/ws`,
    /// not a child, and the remainder it would yield (`-other/t`) is not even a
    /// path — `prove` would read a leading `-` as a bundled option.
    #[test]
    fn a_sibling_sharing_a_textual_prefix_is_not_a_child() {
        for sibling in ["/ws-other/t", "/ws2/t", "/wsX", "/ws.bak/t"] {
            assert_eq!(
                relative_child("/ws", sibling),
                None,
                "`{sibling}` is a sibling of `/ws`, not a child"
            );
        }
        assert_eq!(relative_child("C:\\ws", "C:\\ws-other\\t"), None);
    }

    /// A directory whose name starts with `-` would be parsed as an option, but
    /// it is still a declared root: encode it as `./-name` rather than dropping
    /// it, which would silently run the conventional default instead.
    #[test]
    fn a_leading_dash_directory_is_encoded_not_dropped() {
        assert_eq!(relative_child("/ws", "/ws/-weird").as_deref(), Some("./-weird"));
        assert_eq!(relative_child("/ws", "/ws/-l").as_deref(), Some("./-l"));
        assert_eq!(
            relative_child("/ws", "/ws/--exec=echo").as_deref(),
            Some("./--exec=echo"),
            "an option-shaped name with a value is still just a directory"
        );
        assert_eq!(relative_child("/ws", "/ws/t").as_deref(), Some("t"), "control");

        // The encoding is only reached for genuine children; a sibling whose
        // remainder merely looks option-shaped is still refused.
        assert_eq!(relative_child("/ws", "/ws-other/t"), None);
    }

    /// The argv guard is defence in depth for future argument sources; the
    /// current callers cannot reach it, so it is proven directly.
    #[test]
    fn an_absolute_argument_is_refused_before_it_can_reach_a_receipt()
    -> Result<(), EnvironmentBuildError> {
        let input = EnvironmentInput::new(
            "tool.prove",
            EnvironmentInputAuthority::WorkspaceConvention,
            EnvironmentInputState::Accepted,
            "fixture",
            None,
            "fixture",
        );
        let input_id = input.id.clone();
        let root = ProjectRoot::new(
            ProjectRootRole::Workspace,
            EnvironmentPathRef::new("/ws", "public:ws"),
            input_id.clone(),
        );
        let snapshot =
            ProjectEnvironmentSnapshotBuilder::new("workspace:fixture", 1, WorkspaceTrust::Trusted)
                .with_input(input)
                .with_project_root(root)
                .build()?;

        let working_dir = EnvironmentPathRef::new("/ws", "public:ws");
        let outcome = build_candidate(
            &snapshot,
            &working_dir,
            TestRunnerKind::Prove,
            TestIncludeMode::SourceLib,
            EnvironmentPathRef::new("/usr/bin/prove", "public:prove"),
            vec!["-l".to_string(), "/ws/t".to_string()],
            EnvironmentInputAuthority::WorkspaceConvention,
            input_id,
            None,
            None,
            Vec::new(),
            TestRootCoverage::Complete,
        );

        assert_eq!(
            outcome,
            Err(TestCommandPlanError::AbsolutePathInArgv {
                kind: TestRunnerKind::Prove,
                argument: "/ws/t".to_string(),
            }),
            "an absolute argument is refused before it can reach a receipt"
        );
        Ok(())
    }

    #[test]
    fn admission_reports_the_worst_requirement() {
        let requirement = |artifact, state| GeneratedStateRequirement {
            artifact,
            state,
            path: None,
            reason_code: "fixture".to_string(),
        };

        let complete = TestRootCoverage::Complete;

        assert_eq!(admit(&[], complete).0, TestCommandAdmission::Ready);
        assert_eq!(
            admit(
                &[requirement(GeneratedArtifact::Makefile, GeneratedStateFreshness::Current)],
                complete
            )
            .0,
            TestCommandAdmission::Ready
        );

        // Missing outranks both stale and unobserved, whatever the order.
        let mixed = [
            requirement(GeneratedArtifact::BlibRoots, GeneratedStateFreshness::NotProven),
            requirement(GeneratedArtifact::Makefile, GeneratedStateFreshness::Missing),
            requirement(GeneratedArtifact::BuildScript, GeneratedStateFreshness::Stale),
        ];
        let (admission, reason) = admit(&mixed, complete);
        assert_eq!(admission, TestCommandAdmission::BlockedMissingGeneratedState);
        assert_eq!(reason, "test_command.missing_generated_state.makefile");

        let mut reversed = mixed;
        reversed.reverse();
        assert_eq!(admit(&reversed, complete), (admission, reason));
    }

    /// Incomplete coverage is the one verdict that survives every artifact
    /// blocker, because it is the only one describing a command that would run.
    #[test]
    fn incomplete_test_roots_outrank_every_generated_state_verdict() {
        let requirement = |artifact, state| GeneratedStateRequirement {
            artifact,
            state,
            path: None,
            reason_code: "fixture".to_string(),
        };
        let incomplete = TestRootCoverage::Incomplete;

        let (admission, reason) = admit(&[], incomplete);
        assert_eq!(admission, TestCommandAdmission::BlockedIncompleteTestRoots);
        assert_eq!(reason, "test_command.test_root_outside_working_directory");

        for state in [
            GeneratedStateFreshness::Current,
            GeneratedStateFreshness::Missing,
            GeneratedStateFreshness::Stale,
            GeneratedStateFreshness::NotProven,
        ] {
            assert_eq!(
                admit(&[requirement(GeneratedArtifact::BlibRoots, state)], incomplete).0,
                TestCommandAdmission::BlockedIncompleteTestRoots,
                "{state:?} must not mask an under-covering command"
            );
        }
    }

    #[test]
    fn only_recorded_launcher_names_classify_as_runners() {
        assert!(matches!(
            classify_runner(&ToolCandidateRole::TestRunner, "prove"),
            Some(RunnerShape::Prove)
        ));
        // A bare `make` must not classify as GNU: the name does not say which
        // implementation it is, and BSD make discovers a different set.
        assert!(matches!(
            classify_runner(&ToolCandidateRole::BuildTool, "make"),
            Some(RunnerShape::Make(MakeFlavor::Portable))
        ));
        assert!(matches!(
            classify_runner(&ToolCandidateRole::BuildTool, "gmake"),
            Some(RunnerShape::Make(MakeFlavor::Gnu))
        ));
        assert!(matches!(
            classify_runner(&ToolCandidateRole::BuildTool, "nmake"),
            Some(RunnerShape::Make(MakeFlavor::Nmake))
        ));
        assert!(matches!(
            classify_runner(&ToolCandidateRole::BuildTool, "dmake"),
            Some(RunnerShape::Make(MakeFlavor::Dmake))
        ));
        assert!(classify_runner(&ToolCandidateRole::TestRunner, "yath").is_none());
        assert!(classify_runner(&ToolCandidateRole::Formatter, "prove").is_none());
        assert!(classify_runner(&ToolCandidateRole::BuildTool, "cmake").is_none());
    }
}
