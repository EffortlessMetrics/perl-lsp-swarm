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

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;

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

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;

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

        let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
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

        let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
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

    let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
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

/// Evidence describes one configuration generation. Carrying a `Current`
/// verdict into a later snapshot would let readiness outlive the inputs that
/// justified it, while the candidate stamps the *new* fingerprint. Rules out:
/// accepting evidence that does not describe the snapshot being planned.
#[test]
fn evidence_from_another_snapshot_cannot_make_a_command_ready() -> Result<(), FixtureError> {
    let plan_for = |generation: u64| -> Result<ProjectEnvironmentSnapshot, FixtureError> {
        let root_input = accepted_input("root.workspace");
        let tool_input = accepted_input("tool.make");
        let prove_input = accepted_input("tool.prove");
        let build_input = accepted_input("build.module_build");
        let eumm_input = accepted_input("build.eumm");
        let blib_input = accepted_input("include.blib");
        Ok(ProjectEnvironmentSnapshotBuilder::new(
            WORKSPACE_ID,
            generation,
            WorkspaceTrust::Trusted,
        )
        .with_input(root_input.clone())
        .with_input(tool_input.clone())
        .with_input(prove_input.clone())
        .with_input(build_input.clone())
        .with_input(eumm_input.clone())
        .with_input(blib_input.clone())
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Workspace,
            path(WORKSPACE_PATH),
            root_input.id.clone(),
        ))
        .with_tool_candidate(make_tool("make", tool_input.id.clone()))
        .with_tool_candidate(prove_tool(prove_input.id.clone()))
        .with_build_system(build_fact(BuildSystemKind::ModuleBuild, build_input.id.clone()))
        .with_build_system(build_fact(BuildSystemKind::ExtUtilsMakeMaker, eumm_input.id.clone()))
        .with_include_entry(IncludeEntry::new(
            IncludeEntryRole::BlibLib,
            path("/ws/blib/lib"),
            blib_input.id.clone(),
            0,
        ))
        .build()?)
    };

    let observed_snapshot = plan_for(11)?;
    let later_snapshot = plan_for(12)?;
    assert_ne!(
        observed_snapshot.fingerprint, later_snapshot.fingerprint,
        "the fixture must actually change generation"
    );

    // Everything an adapter could observe, all Current, all correctly located.
    let evidence = GeneratedStateEvidence::for_snapshot(&observed_snapshot)
        .with_observation(
            GeneratedArtifact::Makefile,
            observed(GeneratedStateFreshness::Current, Some("/ws/Makefile")),
        )
        .with_observation(
            GeneratedArtifact::BuildScript,
            observed(GeneratedStateFreshness::Current, Some("/ws/Build")),
        )
        .with_observation(
            GeneratedArtifact::BlibRoots,
            observed(GeneratedStateFreshness::Current, None),
        );

    // Against the snapshot it describes, that evidence is usable.
    let matching = plan_test_commands(&observed_snapshot, &evidence)?;
    assert!(
        matching.ready_candidates().count() > 0,
        "the fixture must be capable of producing ready candidates"
    );

    // Carried into the next generation, nothing generated may read as ready.
    let carried = plan_test_commands(&later_snapshot, &evidence)?;
    for candidate in &carried.candidates {
        if candidate.required_generated_state.is_empty() {
            continue;
        }
        assert_eq!(
            candidate.admission,
            TestCommandAdmission::NotProvenGeneratedState,
            "{:?}/{:?} inherited readiness across a generation change",
            candidate.kind,
            candidate.include_mode
        );
        for requirement in &candidate.required_generated_state {
            assert_eq!(requirement.state, GeneratedStateFreshness::NotProven);
            assert!(requirement.reason_code.starts_with("generated_state.snapshot_mismatch."));
        }
    }
    assert!(
        carried
            .limitations
            .iter()
            .any(|item| item.code == "test_command.generated_state_from_another_snapshot"),
        "the mismatch is recorded, not silent"
    );

    // The source-lib prove candidate needs no generated state and is unaffected.
    let source = candidates_of(&carried.candidates, TestRunnerKind::Prove)
        .into_iter()
        .find(|candidate| candidate.include_mode == TestIncludeMode::SourceLib)
        .ok_or(FixtureError::Missing("source form"))?;
    assert_eq!(source.admission, TestCommandAdmission::Ready);
    Ok(())
}

/// A mismatch invalidates every verdict, not only `Current`. A foreign `Stale`
/// is a claim about a different snapshot, and its path must not survive either:
/// the Module::Build launcher is taken from that field. Rules out: reporting
/// another generation's observation verbatim, or letting an obsolete location
/// shape a command built for this snapshot.
#[test]
fn non_current_evidence_from_another_snapshot_does_not_survive() -> Result<(), FixtureError> {
    let snapshot_for = |generation: u64| -> Result<ProjectEnvironmentSnapshot, FixtureError> {
        let root_input = accepted_input("root.workspace");
        let build_input = accepted_input("build.module_build");
        Ok(ProjectEnvironmentSnapshotBuilder::new(
            WORKSPACE_ID,
            generation,
            WorkspaceTrust::Trusted,
        )
        .with_input(root_input.clone())
        .with_input(build_input.clone())
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Workspace,
            path(WORKSPACE_PATH),
            root_input.id.clone(),
        ))
        .with_build_system(build_fact(BuildSystemKind::ModuleBuild, build_input.id.clone()))
        .build()?)
    };

    let observed_snapshot = snapshot_for(11)?;
    let later_snapshot = snapshot_for(12)?;

    for stale_state in [
        GeneratedStateFreshness::Stale,
        GeneratedStateFreshness::Missing,
        GeneratedStateFreshness::NotProven,
    ] {
        let evidence = GeneratedStateEvidence::for_snapshot(&observed_snapshot).with_observation(
            GeneratedArtifact::BuildScript,
            observed(stale_state, Some("/ws/Build")),
        );

        let carried = plan_test_commands(&later_snapshot, &evidence)?;

        // The obsolete path must not become this snapshot's launcher.
        assert!(
            candidates_of(&carried.candidates, TestRunnerKind::BuildTest).is_empty(),
            "a {stale_state:?} observation from another snapshot supplied a launcher"
        );
        assert!(
            carried
                .limitations
                .iter()
                .any(|item| item.code == "test_command.build_script_location_unknown"),
            "the unusable launcher is reported for {stale_state:?}"
        );
        assert!(
            carried
                .limitations
                .iter()
                .any(|item| item.code == "test_command.generated_state_from_another_snapshot"),
            "the mismatch itself is reported for {stale_state:?}"
        );
    }
    Ok(())
}

/// Two build facts can justify the same command shape. Identity must keep them
/// apart so neither provenance chain is silently dropped by dedup. Rules out:
/// collapsing distinct provenance into one candidate.
#[test]
fn two_build_facts_justifying_one_command_stay_distinct() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.make");
    let build_input = accepted_input("build.eumm");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_input(build_input.clone())
        .with_tool_candidate(make_tool("make", tool_input.id.clone()))
        .with_build_system(BuildSystemFactRef::new(
            BuildSystemKind::ExtUtilsMakeMaker,
            Digest::of("makefile-pl"),
            build_input.id.clone(),
        ))
        .with_build_system(BuildSystemFactRef::new(
            BuildSystemKind::ExtUtilsMakeMaker,
            Digest::of("mymeta"),
            build_input.id.clone(),
        ))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
    let make = candidates_of(&plan.candidates, TestRunnerKind::MakeTest);

    assert_eq!(make.len(), 2, "both build facts keep their own candidate");
    assert_ne!(make[0].id, make[1].id, "distinct provenance means distinct identity");
    assert_ne!(
        make[0].build_system_id, make[1].build_system_id,
        "each candidate names the fact that justified it"
    );
    assert_eq!(make[0].argv, make[1].argv, "the command shape is genuinely the same");
    Ok(())
}

/// `prove -b` needs blib on `@INC`, and a pure-Perl distribution has `blib/lib`
/// with no `blib/arch` at all — arch only exists once something is compiled.
/// Requiring both roles would exclude most distributions. Rules out: treating
/// `BlibRoots` as demanding a complete lib+arch pair.
#[test]
fn a_pure_perl_blib_without_arch_still_offers_the_blib_form() -> Result<(), FixtureError> {
    for role in [IncludeEntryRole::BlibLib, IncludeEntryRole::BlibArch] {
        let (builder, _) = base_builder();
        let tool_input = accepted_input("tool.prove");
        let blib_input = accepted_input("include.blib");
        let snapshot = builder
            .with_input(tool_input.clone())
            .with_input(blib_input.clone())
            .with_tool_candidate(prove_tool(tool_input.id.clone()))
            .with_include_entry(IncludeEntry::new(
                role,
                path("/ws/blib/lib"),
                blib_input.id.clone(),
                0,
            ))
            .build()?;

        let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
        let blib = candidates_of(&plan.candidates, TestRunnerKind::Prove)
            .into_iter()
            .find(|candidate| candidate.include_mode == TestIncludeMode::BlibRoots);

        assert!(blib.is_some(), "{role:?} alone must still offer the blib form");
    }
    Ok(())
}

/// `prove -b` resolves blib relative to its working directory, so only this
/// workspace's own build output can justify the form. Rules out: offering `-b`
/// on the strength of a dependency's blib root, against build output the
/// workspace may not have at all.
#[test]
fn a_dependency_blib_root_does_not_justify_the_blib_form() -> Result<(), FixtureError> {
    for (location, expected) in [
        ("/ws/blib/lib", true),
        ("/ws/blib/arch", true),
        ("/home/dep/.cpanm/work/Foo-1.0/blib/lib", false),
        ("/elsewhere/blib/lib", false),
        ("/ws-other/blib/lib", false),
        ("/ws/blibx/lib", false),
    ] {
        let (builder, _) = base_builder();
        let tool_input = accepted_input("tool.prove");
        let blib_input = accepted_input("include.blib");
        let snapshot = builder
            .with_input(tool_input.clone())
            .with_input(blib_input.clone())
            .with_tool_candidate(prove_tool(tool_input.id.clone()))
            .with_include_entry(IncludeEntry::new(
                IncludeEntryRole::BlibLib,
                path(location),
                blib_input.id.clone(),
                0,
            ))
            .build()?;

        let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
        let has_blib_form = candidates_of(&plan.candidates, TestRunnerKind::Prove)
            .iter()
            .any(|candidate| candidate.include_mode == TestIncludeMode::BlibRoots);

        assert_eq!(has_blib_form, expected, "blib include root at {location}");
    }
    Ok(())
}

/// Where an artifact must sit depends on how the command reaches it. A uniform
/// direct-child rule would reject the conventional `blib/lib`, making located
/// evidence less usable than unlocated evidence. Rules out: more information
/// producing a worse verdict.
#[test]
fn located_blib_evidence_is_usable_at_its_conventional_path() -> Result<(), FixtureError> {
    for (location, expected) in [
        (Some("/ws/blib"), TestCommandAdmission::Ready),
        (Some("/ws/blib/lib"), TestCommandAdmission::Ready),
        (Some("/ws/blib/arch"), TestCommandAdmission::Ready),
        (None, TestCommandAdmission::Ready),
        (Some("/elsewhere/blib/lib"), TestCommandAdmission::NotProvenGeneratedState),
        (Some("/ws/blibx/lib"), TestCommandAdmission::NotProvenGeneratedState),
        (Some("/ws"), TestCommandAdmission::NotProvenGeneratedState),
    ] {
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

        let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
            GeneratedArtifact::BlibRoots,
            observed(GeneratedStateFreshness::Current, location),
        );
        let plan = plan_test_commands(&snapshot, &evidence)?;
        let blib = candidates_of(&plan.candidates, TestRunnerKind::Prove)
            .into_iter()
            .find(|candidate| candidate.include_mode == TestIncludeMode::BlibRoots)
            .ok_or(FixtureError::Missing("blib form"))?;

        assert_eq!(blib.admission, expected, "blib evidence located at {location:?}");
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

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;

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

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
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

    let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
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

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;

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

        let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
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

        let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;

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

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
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
        Ok(plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?)
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

    assert_eq!(
        forward.fingerprint, reverse.fingerprint,
        "insertion order must not change snapshot identity either"
    );

    // One evidence value serves both, which is only sound because the two
    // snapshots are the same content and therefore the same fingerprint.
    let evidence = GeneratedStateEvidence::for_snapshot(&forward).with_observation(
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

    let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
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
        plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot)),
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

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
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

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
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

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;

    assert!(
        plan.limitations
            .iter()
            .any(|item| item.code == "test_command.assumed_default_test_directory")
    );
    Ok(())
}

/// `\` is a legal filename character on POSIX; only `/` is a path separator
/// there. If containment treats `\` as a separator regardless of platform, the
/// sibling `/ws\outside/t` reduces to `outside/t` under the parent `/ws` and
/// silently becomes a Ready command targeting a directory that is neither
/// declared nor beneath the workspace. Rules out: a platform-blind separator
/// set that fabricates a child from a POSIX sibling.
#[test]
fn a_posix_sibling_containing_a_backslash_is_not_a_child() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.prove");
    let test_input = accepted_input("root.test");
    // Parent `/ws` is a POSIX path (no backslash). Child `/ws\outside/t` shares
    // its textual prefix but is a sibling: on POSIX the `\` after `/ws` is part
    // of a filename, so the child sits under `/` and not under `/ws`.
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_input(test_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Test,
            path("/ws\\outside/t"),
            test_input.id.clone(),
        ))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
    let prove = candidates_of(&plan.candidates, TestRunnerKind::Prove);

    for candidate in &prove {
        assert_eq!(
            candidate.argv,
            vec!["-l".to_string(), "t".to_string()],
            "the sibling must fall back to the assumed default, not fabricate `outside/t`"
        );
        assert_eq!(
            candidate.admission,
            TestCommandAdmission::BlockedIncompleteTestRoots,
            "a command that silently substitutes for a declared root is not ready"
        );
    }
    assert!(
        plan.limitations
            .iter()
            .any(|item| item.code == "test_command.test_root_outside_working_directory"),
        "the sibling is reported, not silently dropped"
    );
    Ok(())
}

/// On POSIX, a backslash is a legal filename character, so an emitted argument
/// must preserve it verbatim rather than fabricating a subdirectory. Rules out:
/// a platform-blind separator normalization that turns `wa\it` into `wa/it`.
#[test]
fn a_posix_child_backslash_survives_as_a_filename() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.prove");
    let test_input = accepted_input("root.test");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_input(test_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Test,
            path("/ws/wa\\it/t"),
            test_input.id.clone(),
        ))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
    let prove = candidates_of(&plan.candidates, TestRunnerKind::Prove);
    let candidate = prove.first().ok_or(FixtureError::Missing("prove"))?;

    assert_eq!(
        candidate.argv,
        vec!["-l".to_string(), "wa\\it/t".to_string()],
        "a POSIX filename backslash must not become a path separator"
    );
    Ok(())
}

/// Windows paths remain first-class: the containment rule is driven by the
/// parent's separator style, so a genuine `C:\ws\t` child of `C:\ws` still
/// resolves. Rules out: overcorrecting the POSIX fix into a POSIX-only helper.
#[test]
fn a_windows_backslash_child_still_resolves() -> Result<(), FixtureError> {
    let root_input = accepted_input("root.workspace");
    let tool_input = accepted_input("tool.prove");
    let test_input = accepted_input("root.test");
    let snapshot =
        ProjectEnvironmentSnapshotBuilder::new(WORKSPACE_ID, 11, WorkspaceTrust::Trusted)
            .with_input(root_input.clone())
            .with_input(tool_input.clone())
            .with_input(test_input.clone())
            .with_project_root(ProjectRoot::new(
                ProjectRootRole::Workspace,
                path("C:\\ws"),
                root_input.id.clone(),
            ))
            .with_project_root(ProjectRoot::new(
                ProjectRootRole::Test,
                path("C:\\ws\\t\\basic"),
                test_input.id.clone(),
            ))
            .with_tool_candidate(prove_tool(tool_input.id.clone()))
            .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
    let prove = candidates_of(&plan.candidates, TestRunnerKind::Prove);
    let candidate = prove.first().ok_or(FixtureError::Missing("prove"))?;

    assert_eq!(candidate.argv, vec!["-l".to_string(), "t/basic".to_string()]);
    assert_eq!(candidate.admission, TestCommandAdmission::Ready);
    Ok(())
}

/// Makefile discovery is a property of the launcher, not of `make` generally.
/// Without `-f` / `/F` the launcher finds its makefile *by name*, and the names
/// differ: only GNU make reads `GNUmakefile`, and only `gmake` names GNU make
/// unambiguously — a bare `make` is GNU on Linux and BSD make on a BSD, and BSD
/// make never reads `GNUmakefile`. Rules out: a launcher-blind discovery set,
/// and a bare `make` silently assumed to be the GNU one.
#[test]
fn discoverable_makefile_names_depend_on_the_launcher() -> Result<(), FixtureError> {
    for (launcher, location, expected) in [
        // `gmake` is unambiguously GNU make, which discovers all three.
        ("gmake", "/ws/GNUmakefile", TestCommandAdmission::Ready),
        ("gmake", "/ws/makefile", TestCommandAdmission::Ready),
        ("gmake", "/ws/Makefile", TestCommandAdmission::Ready),
        // A bare `make` does not name an implementation. The portable names
        // every implementation discovers are usable; `GNUmakefile` is not,
        // because BSD make would ignore it and nothing here says which `make`
        // this is. EU::MM generates `Makefile`, so the common case is unaffected.
        ("make", "/ws/makefile", TestCommandAdmission::Ready),
        ("make", "/ws/Makefile", TestCommandAdmission::Ready),
        ("make", "/ws/GNUmakefile", TestCommandAdmission::NotProvenGeneratedState),
        // nmake does not discover the GNU variant, and is Windows-only — so its
        // filesystem is case-insensitive and `MAKEFILE`, the spelling
        // Microsoft's own documentation uses, must be accepted.
        ("nmake", "/ws/GNUmakefile", TestCommandAdmission::NotProvenGeneratedState),
        ("nmake", "/ws/Makefile", TestCommandAdmission::Ready),
        ("nmake", "/ws/makefile", TestCommandAdmission::Ready),
        ("nmake", "/ws/MAKEFILE", TestCommandAdmission::Ready),
        ("nmake", "/ws/MakeFile", TestCommandAdmission::Ready),
        // A case-folded name is *not* accepted for the cross-platform
        // launchers: case-insensitivity is a filesystem property, and only
        // `nmake` pins the filesystem.
        ("gmake", "/ws/MAKEFILE", TestCommandAdmission::NotProvenGeneratedState),
        ("make", "/ws/MAKEFILE", TestCommandAdmission::NotProvenGeneratedState),
        // dmake discovers the portable names only.
        ("dmake", "/ws/GNUmakefile", TestCommandAdmission::NotProvenGeneratedState),
        ("dmake", "/ws/Makefile", TestCommandAdmission::Ready),
    ] {
        let (builder, _) = base_builder();
        let tool_input = accepted_input(&format!("tool.{launcher}"));
        let build_input = accepted_input("build.eumm");
        let snapshot = builder
            .with_input(tool_input.clone())
            .with_input(build_input.clone())
            .with_tool_candidate(make_tool(launcher, tool_input.id.clone()))
            .with_build_system(build_fact(
                BuildSystemKind::ExtUtilsMakeMaker,
                build_input.id.clone(),
            ))
            .build()?;

        let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
            GeneratedArtifact::Makefile,
            observed(GeneratedStateFreshness::Current, Some(location)),
        );
        let plan = plan_test_commands(&snapshot, &evidence)?;
        let make = candidates_of(&plan.candidates, TestRunnerKind::MakeTest);
        let candidate = make.first().ok_or(FixtureError::Missing("make test"))?;

        assert_eq!(
            candidate.admission, expected,
            "admission for `{launcher} test` against evidence at {location}"
        );
    }
    Ok(())
}

/// The Build launcher's observed path *becomes the program*, so it must be able
/// to name a file. The working directory is a directory, and `relative_child`
/// reduces it to `.` — a legitimate answer for a test-root argument, but never a
/// launcher. Rules out: publishing a candidate that would try to execute the
/// workspace directory.
#[test]
fn the_working_directory_itself_is_not_a_build_launcher() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let build_input = accepted_input("build.module_build");
    let snapshot = builder
        .with_input(build_input.clone())
        .with_build_system(build_fact(BuildSystemKind::ModuleBuild, build_input.id.clone()))
        .build()?;

    let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
        GeneratedArtifact::BuildScript,
        observed(GeneratedStateFreshness::Current, Some(WORKSPACE_PATH)),
    );
    let plan = plan_test_commands(&snapshot, &evidence)?;

    assert!(
        candidates_of(&plan.candidates, TestRunnerKind::BuildTest).is_empty(),
        "the workspace directory is not a launcher, so no Build candidate can name it"
    );
    assert!(
        plan.limitations
            .iter()
            .any(|item| item.code == "test_command.build_script_location_unknown"),
        "the entry point is reported as unlocated rather than silently lost"
    );

    // The control: a real script directly in the working directory is still a
    // launcher, so the guard rejects the directory rather than the location.
    let usable = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
        GeneratedArtifact::BuildScript,
        observed(GeneratedStateFreshness::Current, Some("/ws/Build")),
    );
    let usable_plan = plan_test_commands(&snapshot, &usable)?;
    let build = candidates_of(&usable_plan.candidates, TestRunnerKind::BuildTest);
    let candidate = build.first().ok_or(FixtureError::Missing("Build test"))?;
    assert_eq!(candidate.program.normalized, "/ws/Build");
    assert_eq!(candidate.admission, TestCommandAdmission::Ready);
    Ok(())
}

/// The plan fingerprint claims to cover every behaviour-bearing field, and the
/// public receipt publishes redacted path identities. Two plans whose receipts
/// differ must not share a fingerprint, or a fingerprint-keyed cache serves a
/// receipt that no longer matches. Rules out: a fingerprint computed only from
/// the internal `normalized` half of each path.
///
/// The receipt publishes three redacted identities — the program, the working
/// directory, and each required artifact's location — so each is varied on its
/// own. Covering only one would leave the same gap reachable through the others.
#[test]
fn a_receipt_difference_always_moves_the_plan_fingerprint() -> Result<(), FixtureError> {
    /// Which published identity this case moves; every other field is fixed.
    #[derive(Clone, Copy)]
    enum PublishedIdentity {
        Program,
        WorkingDirectory,
        ArtifactLocation,
    }

    let plan_with = |varied: PublishedIdentity,
                     public_id: &str|
     -> Result<TestCommandPlan, FixtureError> {
        let redaction = |field: PublishedIdentity, fixed: &str| {
            if matches!(
                (field, varied),
                (PublishedIdentity::Program, PublishedIdentity::Program)
                    | (PublishedIdentity::WorkingDirectory, PublishedIdentity::WorkingDirectory)
                    | (PublishedIdentity::ArtifactLocation, PublishedIdentity::ArtifactLocation)
            ) {
                public_id.to_string()
            } else {
                fixed.to_string()
            }
        };

        let root_input = accepted_input("root.workspace");
        let tool_input = accepted_input("tool.make");
        let build_input = accepted_input("build.eumm");
        let snapshot =
            ProjectEnvironmentSnapshotBuilder::new(WORKSPACE_ID, 11, WorkspaceTrust::Trusted)
                .with_input(root_input.clone())
                .with_input(tool_input.clone())
                .with_input(build_input.clone())
                .with_project_root(ProjectRoot::new(
                    ProjectRootRole::Workspace,
                    EnvironmentPathRef::new(
                        WORKSPACE_PATH,
                        redaction(PublishedIdentity::WorkingDirectory, "public:ws"),
                    ),
                    root_input.id.clone(),
                ))
                .with_tool_candidate(ToolCandidate::new(
                    ToolCandidateRole::BuildTool,
                    "make",
                    EnvironmentPathRef::new(
                        "/usr/bin/make",
                        redaction(PublishedIdentity::Program, "public:make"),
                    ),
                    tool_input.id.clone(),
                ))
                .with_build_system(build_fact(
                    BuildSystemKind::ExtUtilsMakeMaker,
                    build_input.id.clone(),
                ))
                .build()?;

        let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
            GeneratedArtifact::Makefile,
            GeneratedStateObservation::new(
                GeneratedStateFreshness::Current,
                Some(EnvironmentPathRef::new(
                    "/ws/Makefile",
                    redaction(PublishedIdentity::ArtifactLocation, "public:makefile"),
                )),
                "fixture",
            ),
        );
        Ok(plan_test_commands(&snapshot, &evidence)?)
    };

    for varied in [
        PublishedIdentity::Program,
        PublishedIdentity::WorkingDirectory,
        PublishedIdentity::ArtifactLocation,
    ] {
        let left = plan_with(varied, "public:one")?;
        let right = plan_with(varied, "public:two")?;

        // The premise: only the redacted half moved. If the snapshot fingerprint
        // already separated these, this test would prove nothing about the plan.
        assert_eq!(
            left.environment_fingerprint, right.environment_fingerprint,
            "the snapshot fingerprint does not cover public identities, so the plan must"
        );
        assert_ne!(
            left.public_receipt(),
            right.public_receipt(),
            "the published receipts genuinely differ"
        );
        assert_ne!(
            left.fingerprint, right.fingerprint,
            "a published difference must move the fingerprint that keys it"
        );
    }
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

        let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;

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
            assert_eq!(
                candidate.admission,
                TestCommandAdmission::BlockedIncompleteTestRoots,
                "a command that substitutes `t` for `{detached}` is not ready"
            );
        }
    }
    Ok(())
}

/// The mixed case is the dangerous one: with one expressible root the command
/// looks complete, runs, and passes — while never visiting the detached root the
/// project also declared. Rules out: a `Ready` prove candidate that silently
/// covers a subset of the declared test surface.
#[test]
fn a_partially_expressible_test_surface_is_not_a_ready_command() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.prove");
    let inside_input = accepted_input("root.test.inside");
    let detached_input = accepted_input("root.test.detached");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_input(inside_input.clone())
        .with_input(detached_input.clone())
        .with_tool_candidate(prove_tool(tool_input.id.clone()))
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Test,
            path("/ws/t"),
            inside_input.id.clone(),
        ))
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Test,
            path("/elsewhere/integration"),
            detached_input.id.clone(),
        ))
        .build()?;

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
    let prove = candidates_of(&plan.candidates, TestRunnerKind::Prove);
    let candidate = prove.first().ok_or(FixtureError::Missing("prove"))?;

    // The expressible root is still emitted: the command is well-formed, and
    // withholding it would lose the entry point entirely. What must not happen
    // is calling it ready.
    assert_eq!(candidate.argv, vec!["-l".to_string(), "t".to_string()]);
    assert_eq!(candidate.admission, TestCommandAdmission::BlockedIncompleteTestRoots);
    assert_eq!(candidate.reason_code, "test_command.test_root_outside_working_directory");
    assert!(
        plan.limitations
            .iter()
            .any(|item| item.code == "test_command.test_root_outside_working_directory"),
        "the dropped root stays visible as a limitation as well as an admission"
    );
    Ok(())
}

/// Incomplete coverage is a property of the arguments this plan emits. `make
/// test` and `Build test` pass none, so the build system's own selection is
/// unaffected. Rules out: blocking every runner because one detached root
/// cannot become a `prove` argument.
#[test]
fn a_detached_test_root_does_not_block_build_system_managed_runners() -> Result<(), FixtureError> {
    let (builder, _) = base_builder();
    let tool_input = accepted_input("tool.make");
    let build_input = accepted_input("build.eumm");
    let test_input = accepted_input("root.test.detached");
    let snapshot = builder
        .with_input(tool_input.clone())
        .with_input(build_input.clone())
        .with_input(test_input.clone())
        .with_tool_candidate(make_tool("make", tool_input.id.clone()))
        .with_build_system(build_fact(BuildSystemKind::ExtUtilsMakeMaker, build_input.id.clone()))
        .with_project_root(ProjectRoot::new(
            ProjectRootRole::Test,
            path("/elsewhere/t"),
            test_input.id.clone(),
        ))
        .build()?;

    let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
        GeneratedArtifact::Makefile,
        observed(GeneratedStateFreshness::Current, Some("/ws/Makefile")),
    );
    let plan = plan_test_commands(&snapshot, &evidence)?;
    let make = candidates_of(&plan.candidates, TestRunnerKind::MakeTest);
    let candidate = make.first().ok_or(FixtureError::Missing("make test"))?;

    assert_eq!(candidate.admission, TestCommandAdmission::Ready);
    assert_eq!(candidate.reason_code, "test_command.generated_state_current");
    Ok(())
}

/// `make test` is passed no `-f`, so it discovers its makefile by name in the
/// working directory. A current observation of some other direct child is
/// evidence about a file this command would never read. Rules out: treating any
/// direct child as proof that `make test` can run.
///
/// Uses `gmake` so the full GNU discovery set applies; which *launcher*
/// discovers which names is the separate axis covered by
/// `discoverable_makefile_names_depend_on_the_launcher`. The load-bearing rows
/// here are the near-misses — especially `Makefile.PL`, which every EU::MM
/// distribution contains.
#[test]
fn only_a_name_make_discovers_proves_the_command_is_ready() -> Result<(), FixtureError> {
    for (location, expected) in [
        ("/ws/Makefile", TestCommandAdmission::Ready),
        ("/ws/makefile", TestCommandAdmission::Ready),
        ("/ws/GNUmakefile", TestCommandAdmission::Ready),
        ("/ws/README.md", TestCommandAdmission::NotProvenGeneratedState),
        ("/ws/Makefile.PL", TestCommandAdmission::NotProvenGeneratedState),
        ("/ws/Makefile.old", TestCommandAdmission::NotProvenGeneratedState),
    ] {
        let (builder, _) = base_builder();
        let tool_input = accepted_input("tool.make");
        let build_input = accepted_input("build.eumm");
        let snapshot = builder
            .with_input(tool_input.clone())
            .with_input(build_input.clone())
            .with_tool_candidate(make_tool("gmake", tool_input.id.clone()))
            .with_build_system(build_fact(
                BuildSystemKind::ExtUtilsMakeMaker,
                build_input.id.clone(),
            ))
            .build()?;

        let evidence = GeneratedStateEvidence::for_snapshot(&snapshot).with_observation(
            GeneratedArtifact::Makefile,
            observed(GeneratedStateFreshness::Current, Some(location)),
        );
        let plan = plan_test_commands(&snapshot, &evidence)?;
        let make = candidates_of(&plan.candidates, TestRunnerKind::MakeTest);
        let candidate = make.first().ok_or(FixtureError::Missing("make test"))?;

        assert_eq!(candidate.admission, expected, "admission for evidence naming {location}");
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

    let plan = plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot))?;
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
            plan_test_commands(&snapshot, &GeneratedStateEvidence::for_snapshot(&snapshot)),
            Err(TestCommandPlanError::InvalidSnapshot(_))
        ),
        "a stale fingerprint must not be planned against"
    );
    Ok(())
}
