//! Behavioural proof for non-executing test-command planning (#13638).
//!
//! Each test names the wrong implementation it rules out. The load-bearing
//! controls are the ones proving a runner is *not* offered as ready: a tool on
//! `PATH`, a build-system fact, or an inactive input must never by itself make
//! `make test` or `Build test` look runnable.

use perl_workspace_core::{
    BuildSystemFactRef, BuildSystemKind, Digest, EnvironmentBuildError, EnvironmentInput,
    EnvironmentInputAuthority, EnvironmentInputId, EnvironmentInputState, EnvironmentPathRef,
    GeneratedArtifact, GeneratedStateEvidence, GeneratedStateFreshness, GeneratedStateObservation,
    IncludeEntry, IncludeEntryRole, ProjectEnvironmentSnapshot, ProjectEnvironmentSnapshotBuilder,
    ProjectRoot, ProjectRootRole, TEST_COMMAND_PLAN_SCHEMA_VERSION, TestCommandAdmission,
    TestCommandCandidate, TestCommandPlan, TestCommandPlanError, TestIncludeMode, TestRunnerKind,
    ToolCandidate, ToolCandidateRole, WorkspaceTrust, plan_test_commands,
};

/// Local error join so every fixture can use `?` instead of a denied
/// `expect`/`unwrap`. `EnvironmentBuildError` implements no `Error`, so a
/// boxed trait object is not available here.
#[derive(Debug)]
enum FixtureError {
    Environment(EnvironmentBuildError),
    Plan(TestCommandPlanError),
    Json(serde_json::Error),
    Missing(&'static str),
}

impl std::fmt::Display for FixtureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Environment(error) => write!(formatter, "snapshot did not build: {error:?}"),
            Self::Plan(error) => write!(formatter, "plan did not build: {error}"),
            Self::Json(error) => write!(formatter, "wire form did not round-trip: {error}"),
            Self::Missing(what) => write!(formatter, "expected candidate missing: {what}"),
        }
    }
}

impl From<EnvironmentBuildError> for FixtureError {
    fn from(error: EnvironmentBuildError) -> Self {
        Self::Environment(error)
    }
}

impl From<TestCommandPlanError> for FixtureError {
    fn from(error: TestCommandPlanError) -> Self {
        Self::Plan(error)
    }
}

impl From<serde_json::Error> for FixtureError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

const WORKSPACE_ID: &str = "workspace:fixture";
const WORKSPACE_PATH: &str = "/ws";

fn accepted_input(key: &str) -> EnvironmentInput {
    EnvironmentInput::new(
        key,
        EnvironmentInputAuthority::WorkspaceConvention,
        EnvironmentInputState::Accepted,
        format!("source:{key}"),
        Some(Digest::of(key)),
        "fixture",
    )
}

fn input_with_state(key: &str, state: EnvironmentInputState) -> EnvironmentInput {
    EnvironmentInput::new(
        key,
        EnvironmentInputAuthority::WorkspaceConvention,
        state,
        format!("source:{key}"),
        Some(Digest::of(key)),
        "fixture",
    )
}

fn path(normalized: &str) -> EnvironmentPathRef {
    EnvironmentPathRef::new(normalized, format!("public:{}", Digest::of(normalized)))
}

/// A workspace root plus one accepted input, the minimum a plan needs.
fn base_builder() -> (ProjectEnvironmentSnapshotBuilder, EnvironmentInputId) {
    let root_input = accepted_input("root.workspace");
    let root_input_id = root_input.id.clone();
    let builder = ProjectEnvironmentSnapshotBuilder::new(WORKSPACE_ID, 11, WorkspaceTrust::Trusted)
        .with_input(root_input)
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Workspace,
            path(WORKSPACE_PATH),
            root_input_id.clone(),
        ));
    (builder, root_input_id)
}

fn prove_tool(input_id: EnvironmentInputId) -> ToolCandidate {
    ToolCandidate::new(ToolCandidateRole::TestRunner, "prove", path("/usr/bin/prove"), input_id)
}

fn make_tool(logical_name: &str, input_id: EnvironmentInputId) -> ToolCandidate {
    ToolCandidate::new(
        ToolCandidateRole::BuildTool,
        logical_name,
        path(&format!("/usr/bin/{logical_name}")),
        input_id,
    )
}

fn build_fact(kind: BuildSystemKind, input_id: EnvironmentInputId) -> BuildSystemFactRef {
    BuildSystemFactRef::new(kind, Digest::of("build-fact"), input_id)
}

fn observed(state: GeneratedStateFreshness, location: Option<&str>) -> GeneratedStateObservation {
    GeneratedStateObservation::new(state, location.map(path), "fixture")
}

fn candidates_of(
    plan: &[TestCommandCandidate],
    kind: TestRunnerKind,
) -> Vec<&TestCommandCandidate> {
    plan.iter().filter(|candidate| candidate.kind == kind).collect()
}

/// A source-only checkout can run its tests without pretending a configure step
/// happened. Rules out: gating every runner behind generated state.
#[test]
fn source_only_project_offers_prove_without_generated_state() -> Result<(), FixtureError> {
    let (builder, root_input) = base_builder();
    let tool_input = accepted_input("tool.prove");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;

    let prove = candidates_of(&plan.candidates, TestRunnerKind::Prove);
    assert_eq!(prove.len(), 1, "source-only project offers exactly one prove candidate");
    assert_eq!(prove[0].include_mode, TestIncludeMode::SourceLib);
    assert_eq!(prove[0].argv, vec!["-l".to_string(), "t".to_string()]);
    assert_eq!(prove[0].admission, TestCommandAdmission::Ready);
    assert!(prove[0].required_generated_state.is_empty());
    assert_eq!(prove[0].reason_code, "test_command.no_generated_state_required");

    assert!(
        candidates_of(&plan.candidates, TestRunnerKind::MakeTest).is_empty(),
        "no build system means no make candidate"
    );
    assert!(candidates_of(&plan.candidates, TestRunnerKind::BuildTest).is_empty());
    let _ = root_input;
    Ok(())
}

/// The whole point of the claim: an unobserved `Makefile` must not read as
/// runnable. Rules out: treating absent evidence as satisfied.
#[test]
fn unobserved_generated_state_is_not_proven_rather_than_ready() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.make");
    let build_input = accepted_input("build.eumm");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_input(build_input.clone())
        .with_tool_candidate(make_tool("make", tool_input.id.clone()))
        .with_build_system(build_fact(BuildSystemKind::ExtUtilsMakeMaker, build_input.id.clone()))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;

    let make = candidates_of(&plan.candidates, TestRunnerKind::MakeTest);
    assert_eq!(make.len(), 1);
    assert_eq!(make[0].admission, TestCommandAdmission::NotProvenGeneratedState);
    assert_eq!(make[0].reason_code, "test_command.not_proven_generated_state.makefile");
    assert_eq!(
        make[0].required_generated_state[0].state,
        GeneratedStateFreshness::NotProven,
        "an unobserved artifact is NotProven, not Current"
    );
    assert_eq!(plan.ready_candidates().count(), 0);
    Ok(())
}

/// Each freshness verdict maps to its own admission, and only `Current` is
/// ready. Rules out: collapsing missing, stale, and unobserved into one state.
#[test]
fn make_test_admission_tracks_makefile_freshness() -> Result<(), FixtureError> {
    let cases = [
        (
            GeneratedStateFreshness::Current,
            TestCommandAdmission::Ready,
            "test_command.generated_state_current",
        ),
        (
            GeneratedStateFreshness::Missing,
            TestCommandAdmission::BlockedMissingGeneratedState,
            "test_command.missing_generated_state.makefile",
        ),
        (
            GeneratedStateFreshness::Stale,
            TestCommandAdmission::BlockedStaleGeneratedState,
            "test_command.stale_generated_state.makefile",
        ),
        (
            GeneratedStateFreshness::NotProven,
            TestCommandAdmission::NotProvenGeneratedState,
            "test_command.not_proven_generated_state.makefile",
        ),
    ];

    for (freshness, expected_admission, expected_reason) in cases {
        let (builder, _) = base_builder();
        let tool_input = accepted_input("tool.make");
        let build_input = accepted_input("build.eumm");
        let snapshot = builder
            .with_input(tool_input.clone())
            .with_input(build_input.clone())
            .with_tool_candidate(make_tool("make", tool_input.id.clone()))
            .with_build_system(build_fact(
                BuildSystemKind::ExtUtilsMakeMaker,
                build_input.id.clone(),
            ))
            .build()?;

        let evidence = GeneratedStateEvidence::new().with_observation(
            GeneratedArtifact::Makefile,
            observed(freshness, Some("/ws/Makefile")),
        );
        let plan = plan_test_commands(&snapshot, &evidence)?;
        let make = candidates_of(&plan.candidates, TestRunnerKind::MakeTest);

        assert_eq!(make.len(), 1, "{freshness:?} still reports the entry point");
        assert_eq!(make[0].admission, expected_admission, "admission for {freshness:?}");
        assert_eq!(make[0].reason_code, expected_reason, "reason for {freshness:?}");
        assert_eq!(make[0].argv, vec!["test".to_string()]);
    }
    Ok(())
}

/// A freshness verdict is about an artifact at a location; readiness is about
/// the command in the working directory. The emitted argv passes neither
/// `make -C` nor `-f`, so a makefile observed elsewhere is not the one it would
/// read. Rules out: accepting any `Current` makefile as this command's evidence.
#[test]
fn a_makefile_outside_the_working_directory_does_not_make_the_command_ready()
-> Result<(), FixtureError> {
    for (location, expected) in [
        ("/ws/Makefile", TestCommandAdmission::Ready),
        ("/ws/build/Makefile", TestCommandAdmission::NotProvenGeneratedState),
        ("/elsewhere/Makefile", TestCommandAdmission::NotProvenGeneratedState),
    ] {
        let (builder, _) = base_builder();
        let tool_input = accepted_input("tool.make");
        let build_input = accepted_input("build.eumm");
        let snapshot = builder
            .with_input(tool_input.clone())
            .with_input(build_input.clone())
            .with_tool_candidate(make_tool("make", tool_input.id.clone()))
            .with_build_system(build_fact(
                BuildSystemKind::ExtUtilsMakeMaker,
                build_input.id.clone(),
            ))
            .build()?;

        let evidence = GeneratedStateEvidence::new().with_observation(
            GeneratedArtifact::Makefile,
            observed(GeneratedStateFreshness::Current, Some(location)),
        );
        let plan = plan_test_commands(&snapshot, &evidence)?;
        let make = candidates_of(&plan.candidates, TestRunnerKind::MakeTest);

        assert_eq!(make.len(), 1, "{location} still reports the entry point");
        assert_eq!(make[0].admission, expected, "admission for a makefile at {location}");
    }
    Ok(())
}

/// Snapshot paths are validated by the environment builder, but observation
/// paths arrive from the caller and can be deserialized. Rules out: a Ready
/// candidate whose launcher is an empty string.
#[test]
fn an_unusable_evidence_path_cannot_launch_a_ready_candidate() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let build_input = accepted_input("build.module_build");
    let snapshot = builder
        .with_input(build_input.clone())
        .with_build_system(build_fact(BuildSystemKind::ModuleBuild, build_input.id.clone()))
        .build()?;

    let evidence = GeneratedStateEvidence::new().with_observation(
        GeneratedArtifact::BuildScript,
        GeneratedStateObservation::new(
            GeneratedStateFreshness::Current,
            Some(EnvironmentPathRef::new("", "")),
            "fixture",
        ),
    );
    let plan = plan_test_commands(&snapshot, &evidence)?;

    for candidate in &plan.candidates {
        assert!(
            !candidate.program.normalized.is_empty(),
            "a candidate must never advertise an empty launcher"
        );
        assert_ne!(
            candidate.admission,
            TestCommandAdmission::Ready,
            "unusable evidence cannot be ready"
        );
    }
    Ok(())
}

/// A `make` binary on `PATH` is not evidence that this project uses MakeMaker.
/// Rules out: deriving the runner from the tool alone.
#[test]
fn make_tool_without_a_makemaker_fact_offers_no_make_candidate() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.make");
    let build_input = accepted_input("build.module_build");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_input(build_input.clone())
        .with_tool_candidate(make_tool("make", tool_input.id.clone()))
        .with_build_system(build_fact(BuildSystemKind::ModuleBuild, build_input.id.clone()))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;

    assert!(
        candidates_of(&plan.candidates, TestRunnerKind::MakeTest).is_empty(),
        "a Module::Build project must not offer `make test` just because make exists"
    );
    Ok(())
}

/// Platform launcher names come from the recorded tool, never from the host the
/// planner runs on. Rules out: hardcoding `make` and losing `gmake`/`nmake`.
#[test]
fn each_recorded_make_launcher_gets_its_own_candidate() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let gmake_input = accepted_input("tool.gmake");
    let nmake_input = accepted_input("tool.nmake");
    let build_input = accepted_input("build.eumm");
    let snapshot = builder
        .with_input(gmake_input.clone())
        .with_input(nmake_input.clone())
        .with_input(build_input.clone())
        .with_tool_candidate(make_tool("gmake", gmake_input.id.clone()))
        .with_tool_candidate(make_tool("nmake", nmake_input.id.clone()))
        .with_build_system(build_fact(BuildSystemKind::ExtUtilsMakeMaker, build_input.id.clone()))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;
    let make = candidates_of(&plan.candidates, TestRunnerKind::MakeTest);

    assert_eq!(make.len(), 2, "both recorded launchers stay distinct");
    let mut programs: Vec<&str> =
        make.iter().map(|candidate| candidate.program.normalized.as_str()).collect();
    programs.sort_unstable();
    assert_eq!(programs, vec!["/usr/bin/gmake", "/usr/bin/nmake"]);
    assert_ne!(make[0].id, make[1].id, "distinct launchers get distinct identities");
    Ok(())
}

/// `prove -b` is only meaningful once blib roots exist. Rules out: offering the
/// blib form unconditionally, or conflating it with `-l`.
#[test]
fn blib_prove_candidate_appears_only_with_blib_roots() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.prove");
    let blib_input = accepted_input("include.blib");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_input(blib_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .with_include_entry(IncludeEntry::new(
            IncludeEntryRole::BlibLib,
            path("/ws/blib/lib"),
            blib_input.id.clone(),
            0,
        ))
        .build()?;

    let evidence = GeneratedStateEvidence::new().with_observation(
        GeneratedArtifact::BlibRoots,
        observed(GeneratedStateFreshness::Current, None),
    );
    let plan = plan_test_commands(&snapshot, &evidence)?;
    let prove = candidates_of(&plan.candidates, TestRunnerKind::Prove);

    assert_eq!(prove.len(), 2, "source and blib forms are independent candidates");
    let source = prove
        .iter()
        .find(|candidate| candidate.include_mode == TestIncludeMode::SourceLib)
        .ok_or(FixtureError::Missing("source form"))?;
    let blib = prove
        .iter()
        .find(|candidate| candidate.include_mode == TestIncludeMode::BlibRoots)
        .ok_or(FixtureError::Missing("blib form"))?;

    assert_eq!(source.argv, vec!["-l".to_string(), "t".to_string()]);
    assert_eq!(blib.argv, vec!["-b".to_string(), "t".to_string()]);
    assert!(
        source.required_generated_state.is_empty(),
        "the source form never depends on a build step"
    );
    assert_eq!(
        blib.required_generated_state[0].artifact,
        GeneratedArtifact::BlibRoots,
        "the blib form records the build step it needs"
    );
    assert_ne!(source.id, blib.id);
    Ok(())
}

/// Module::Build's launcher is the generated script itself, so an unlocated
/// script yields an explicit limitation, not a guessed `./Build`. Rules out:
/// inventing a launcher path.
#[test]
fn build_test_without_a_located_script_reports_a_limitation() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let build_input = accepted_input("build.module_build");
    let snapshot = builder
        .with_input(build_input.clone())
        .with_build_system(build_fact(BuildSystemKind::ModuleBuild, build_input.id.clone()))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;

    assert!(candidates_of(&plan.candidates, TestRunnerKind::BuildTest).is_empty());
    assert!(
        plan.limitations
            .iter()
            .any(|item| item.code == "test_command.build_script_location_unknown"),
        "the missing entry point stays visible as a limitation"
    );
    Ok(())
}

/// The observed script location becomes the program verbatim, which is how
/// `Build.bat` and `./Build` stay apart. Rules out: normalizing to one shape.
#[test]
fn build_test_uses_the_observed_script_location_verbatim() -> Result<(), FixtureError> {
    for location in ["/ws/Build", "/ws/Build.bat"] {
        let (builder, _) = base_builder();
        let build_input = accepted_input("build.module_build");
        let snapshot = builder
            .with_input(build_input.clone())
            .with_build_system(build_fact(BuildSystemKind::ModuleBuild, build_input.id.clone()))
            .build()?;

        let evidence = GeneratedStateEvidence::new().with_observation(
            GeneratedArtifact::BuildScript,
            observed(GeneratedStateFreshness::Current, Some(location)),
        );
        let plan = plan_test_commands(&snapshot, &evidence)?;
        let build = candidates_of(&plan.candidates, TestRunnerKind::BuildTest);

        assert_eq!(build.len(), 1, "{location} yields one candidate");
        assert_eq!(build[0].program.normalized, location);
        assert_eq!(build[0].argv, vec!["test".to_string()]);
        assert_eq!(build[0].admission, TestCommandAdmission::Ready);
        assert_eq!(build[0].tool_candidate_id, None, "the launcher is not a discovered tool");
    }
    Ok(())
}

/// A denied or superseded input is not authority. Rules out: reading the raw
/// candidate vectors instead of the active projections.
#[test]
fn inactive_inputs_contribute_no_candidates() -> Result<(), FixtureError> {
    for state in [
        EnvironmentInputState::Denied,
        EnvironmentInputState::Unavailable,
        EnvironmentInputState::Superseded,
        EnvironmentInputState::Ambient,
    ] {
        let (builder, _) = base_builder();
        let tool_input = input_with_state("tool.prove", state);
        let snapshot = builder
            .with_input(tool_input.clone())
            .with_tool_candidate(prove_tool(tool_input.id.clone()))
            .build()?;

        let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;

        assert!(
            plan.candidates.is_empty(),
            "a {state:?} input must not produce a runnable entry point"
        );
    }
    Ok(())
}

/// Every candidate is reproducible on its own terms. Rules out: emitting a
/// command without the identity a caller needs to run or explain it.
#[test]
fn every_candidate_carries_exact_reproduction_identity() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.prove");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;
    assert_eq!(plan.schema_version, TEST_COMMAND_PLAN_SCHEMA_VERSION);

    for candidate in &plan.candidates {
        assert_eq!(candidate.working_dir.normalized, WORKSPACE_PATH);
        assert_eq!(
            candidate.environment_fingerprint, snapshot.fingerprint,
            "a candidate is bound to the snapshot that produced it"
        );
        assert_eq!(candidate.configuration_generation, snapshot.configuration_generation);
        assert_eq!(candidate.trust, WorkspaceTrust::Trusted);
        assert_eq!(candidate.authority, EnvironmentInputAuthority::WorkspaceConvention);
        assert_eq!(candidate.input_id, tool_input.id);
        assert!(!candidate.id.is_empty());
        assert!(!candidate.program.normalized.is_empty());
    }
    Ok(())
}

/// A candidate must not outlive the environment it was derived from. Rules out:
/// caching candidates across a snapshot change.
#[test]
fn a_changed_environment_changes_candidate_binding() -> Result<(), FixtureError> {
    fn build_plan(generation: u64) -> Result<TestCommandPlan, FixtureError> {
        let tool_input = accepted_input("tool.prove");
        let root_input = accepted_input("root.workspace");
        let snapshot = ProjectEnvironmentSnapshotBuilder::new(
            WORKSPACE_ID,
            generation,
            WorkspaceTrust::Trusted,
        )
        .with_input(root_input.clone())
        .with_input(tool_input.clone())
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Workspace,
            path(WORKSPACE_PATH),
            root_input.id.clone(),
        ))
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .build()?;
        Ok(plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?)
    }

    let first = build_plan(11)?;
    let second = build_plan(12)?;

    assert_ne!(
        first.environment_fingerprint, second.environment_fingerprint,
        "a new configuration generation is a new environment"
    );
    assert_ne!(first.fingerprint, second.fingerprint, "the plan fingerprint follows it");
    assert_ne!(
        first.candidates[0].environment_fingerprint,
        second.candidates[0].environment_fingerprint
    );
    Ok(())
}

/// Output cannot depend on the order inputs were supplied. Rules out: leaking
/// insertion order into the candidate set or its fingerprint.
#[test]
fn plan_is_independent_of_input_order() -> Result<(), FixtureError> {
    let root_input = accepted_input("root.workspace");
    let prove_input = accepted_input("tool.prove");
    let make_input = accepted_input("tool.make");
    let build_input = accepted_input("build.eumm");

    let forward = ProjectEnvironmentSnapshotBuilder::new(WORKSPACE_ID, 3, WorkspaceTrust::Trusted)
        .with_input(root_input.clone())
        .with_input(prove_input.clone())
        .with_input(make_input.clone())
        .with_input(build_input.clone())
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Workspace,
            path(WORKSPACE_PATH),
            root_input.id.clone(),
        ))
        .with_tool_candidate(prove_tool(prove_input.id.clone()))
        .with_tool_candidate(make_tool("make", make_input.id.clone()))
        .with_build_system(build_fact(BuildSystemKind::ExtUtilsMakeMaker, build_input.id.clone()))
        .build()?;

    let reverse = ProjectEnvironmentSnapshotBuilder::new(WORKSPACE_ID, 3, WorkspaceTrust::Trusted)
        .with_build_system(build_fact(BuildSystemKind::ExtUtilsMakeMaker, build_input.id.clone()))
        .with_tool_candidate(make_tool("make", make_input.id.clone()))
        .with_tool_candidate(prove_tool(prove_input.id.clone()))
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Workspace,
            path(WORKSPACE_PATH),
            root_input.id.clone(),
        ))
        .with_input(build_input)
        .with_input(make_input)
        .with_input(prove_input)
        .with_input(root_input)
        .build()?;

    let evidence = GeneratedStateEvidence::new().with_observation(
        GeneratedArtifact::Makefile,
        observed(GeneratedStateFreshness::Current, Some("/ws/Makefile")),
    );

    let left = plan_test_commands(&forward, &evidence)?;
    let right = plan_test_commands(&reverse, &evidence)?;

    assert_eq!(left, right, "the plan is a function of content, not insertion order");
    assert_eq!(left.candidates.len(), 2);
    Ok(())
}

/// The public receipt is the surface a client sees. Rules out: leaking host
/// layout through a program path, working directory, or artifact location.
#[test]
fn public_receipt_redacts_every_host_path() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.make");
    let build_input = accepted_input("build.eumm");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_input(build_input.clone())
        .with_tool_candidate(make_tool("make", tool_input.id.clone()))
        .with_build_system(build_fact(BuildSystemKind::ExtUtilsMakeMaker, build_input.id.clone()))
        .build()?;

    let evidence = GeneratedStateEvidence::new().with_observation(
        GeneratedArtifact::Makefile,
        observed(GeneratedStateFreshness::Current, Some("/ws/Makefile")),
    );
    let plan = plan_test_commands(&snapshot, &evidence)?;
    let receipt = plan.public_receipt();
    let serialized = serde_json::to_string(&receipt)?;

    assert!(!receipt.candidates.is_empty(), "the fixture must actually exercise the projection");
    for forbidden in ["/usr/bin/make", "/ws/Makefile", WORKSPACE_PATH] {
        assert!(
            !serialized.contains(forbidden),
            "public receipt leaked `{forbidden}`: {serialized}"
        );
    }
    assert!(serialized.contains("\"test\""), "the argv shape stays inspectable");
    assert_eq!(receipt.fingerprint, plan.fingerprint);
    Ok(())
}

/// Without a working directory a command is not reproducible, so planning fails
/// rather than inventing one. Rules out: defaulting to the process CWD.
#[test]
fn planning_without_a_workspace_root_fails_closed() -> Result<(), FixtureError> {
    let tool_input = accepted_input("tool.prove");
    let snapshot = ProjectEnvironmentSnapshotBuilder::new(WORKSPACE_ID, 1, WorkspaceTrust::Trusted)
        .with_input(tool_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .build()?;

    assert_eq!(
        plan_test_commands(&snapshot, &GeneratedStateEvidence::new()),
        Err(TestCommandPlanError::MissingWorkspaceRoot)
    );
    Ok(())
}

/// A declared test root replaces the assumed default and drops the assumption
/// limitation. Rules out: hardcoding `t` regardless of project layout.
#[test]
fn declared_test_roots_replace_the_assumed_default() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.prove");
    let test_input = accepted_input("root.test");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_input(test_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Test,
            path("/ws/xt"),
            test_input.id.clone(),
        ))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;
    let prove = candidates_of(&plan.candidates, TestRunnerKind::Prove);

    assert_eq!(prove[0].argv, vec!["-l".to_string(), "xt".to_string()]);
    assert!(
        !plan
            .limitations
            .iter()
            .any(|item| item.code == "test_command.assumed_default_test_directory"),
        "a declared root is not an assumption"
    );
    Ok(())
}

/// A test root at the workspace itself is a declared root, not a detached one.
/// Rules out: mislabelling it "outside the working directory" and then
/// substituting a different directory than the project declared.
#[test]
fn a_test_root_at_the_workspace_becomes_the_current_directory() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.prove");
    let test_input = accepted_input("root.test");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_input(test_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Test,
            path(WORKSPACE_PATH),
            test_input.id.clone(),
        ))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;
    let prove = candidates_of(&plan.candidates, TestRunnerKind::Prove);

    assert_eq!(prove[0].argv, vec!["-l".to_string(), ".".to_string()]);
    for code in [
        "test_command.test_root_outside_working_directory",
        "test_command.assumed_default_test_directory",
    ] {
        assert!(
            !plan.limitations.iter().any(|item| item.code == code),
            "a root equal to the workspace is neither outside it nor an assumption: {code}"
        );
    }
    Ok(())
}

/// An assumed default must be visible as an assumption. Rules out: silently
/// guessing the test directory.
#[test]
fn an_assumed_test_directory_is_recorded_as_a_limitation() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.prove");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;

    assert!(
        plan.limitations
            .iter()
            .any(|item| item.code == "test_command.assumed_default_test_directory")
    );
    Ok(())
}

/// A test root outside the working directory cannot become a relative argument
/// and must not be silently dropped. Rules out: emitting a wrong or absolute
/// argument for a detached root.
#[test]
fn a_test_root_outside_the_workspace_is_reported_not_guessed() -> Result<(), FixtureError> {
    // `/ws-other/t` shares a textual prefix with the `/ws` working directory but
    // is a sibling, not a child. Reducing it by text alone yields `-other/t`,
    // which `prove` would read as a bundled option rather than a path — so the
    // sibling case is the one that turns a wrong directory into a wrong command.
    for detached in ["/elsewhere/t", "/ws-other/t", "/ws2/t"] {
        let (builder, _) = base_builder();
        let tool_input = accepted_input("tool.prove");
        let test_input = accepted_input("root.test");
        let snapshot = builder
            .with_input(tool_input.clone())
            .with_input(test_input.clone())
            .with_tool_candidate(prove_tool(tool_input.id.clone()))
            .with_project_root(ProjectRoot::new(
                ProjectRootRole::Test,
                path(detached),
                test_input.id.clone(),
            ))
            .build()?;

        let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;

        assert!(
            plan.limitations
                .iter()
                .any(|item| item.code == "test_command.test_root_outside_working_directory"),
            "`{detached}` must be reported, not silently dropped"
        );
        for candidate in &plan.candidates {
            assert_eq!(
                candidate.argv,
                vec!["-l".to_string(), "t".to_string()],
                "`{detached}` must fall back to the assumed default, not a derived argument"
            );
            for argument in candidate.argv.iter().skip(1) {
                assert!(
                    !argument.starts_with('/') && !argument.starts_with('-'),
                    "a derived path argument may be neither absolute nor option-shaped: {argument}"
                );
            }
        }
    }
    Ok(())
}

/// The plan round-trips through its wire form unchanged. Rules out: a
/// projection a consumer cannot actually reconstruct.
#[test]
fn plan_round_trips_through_json() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.prove");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::new())?;
    let encoded = serde_json::to_string(&plan)?;
    let decoded: TestCommandPlan = serde_json::from_str(&encoded)?;

    assert_eq!(plan, decoded);
    Ok(())
}

/// Planning consumes an authoritative snapshot only. Rules out: acting on a
/// forged or tampered snapshot.
#[test]
fn a_tampered_snapshot_is_refused() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.prove");
    let mut snapshot: ProjectEnvironmentSnapshot = builder
        .with_input(tool_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .build()?;

    snapshot.configuration_generation += 1;

    assert!(
        matches!(
            plan_test_commands(&snapshot, &GeneratedStateEvidence::new()),
            Err(TestCommandPlanError::InvalidSnapshot(_))
        ),
        "a stale fingerprint must not be planned against"
    );
    Ok(())
}
