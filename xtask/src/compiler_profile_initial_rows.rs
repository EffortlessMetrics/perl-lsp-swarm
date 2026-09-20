//! Checked initial row inventory for the four maintained compiler operating
//! profiles (#12330, train row COMP-PROFILE-C02, parent #12176).
//!
//! This module pins the exact initial rows of `compiler_local_lexical.v1`,
//! `compiler_static_project.v1`, `compiler_bounded_execution.v1`, and
//! `compiler_maintained_code_intelligence.v1` by instantiating the #12186
//! vocabulary in [`crate::compiler_profile_contract`].  It deliberately adds
//! no second type system, no manifest or file syntax, no serde derives, no
//! receipt adaptation, and no candidate evaluation: #12187 consumes these
//! in-memory definitions without transcription and owns representation at
//! rest; #12177 owns evaluation.
//!
//! Row identity law (from #12330): a row ID is semantic compatibility
//! identity, not display text or ordering.  Changing a proposition,
//! applicability, subject, evidence class, work law, allowed limitation,
//! legacy exit, or claim ceiling requires a new row/profile version; row
//! order and formatting cannot change identity (the pinned
//! [`CompilerProfileDefinition::semantic_fingerprint`] digests in the tests
//! are the mechanical gate).  Issue links in owner strings are navigation
//! and ownership only; accepted durable receipt identities are evidence
//! authority.  #8722 may later provide fuller integrated publication, but it
//! is not a prerequisite to the bounded local lexical rows and cannot
//! redefine the #12291/#12139-#12141 series, subjects, or denominators.

use anyhow::Result;

use crate::compiler_profile_contract::{
    AllowedLimitation, ClaimCeiling, ClaimFamily, CompilerProfileDefinition, CompilerProfileId,
    CompilerProfileImport, CompilerProfileRow, CompilerProfileRowId, CompilerProfileVersion,
    CompletenessRequirement, CoverageRule, CurrentnessRule, EvidenceRequirement, InvalidationInput,
    InvalidationKind, LegacyExitRequirement, Obligation, OwnerAndWakeEvent, ProofClass,
    RowDisposition, SourceTier, SubjectRef, SubjectSelector, WorkRequirement, WorkScope,
};

// ---------------------------------------------------------------------------
// Canonical owner map (#12330): existing owners are referenced, never
// re-implemented.  These strings are navigation/ownership identifiers only.
// ---------------------------------------------------------------------------

/// Bounded selected upstream observation of the `t/TEST` base series.
const OWNER_UPSTREAM_BASE: &str =
    "#12291/#12139 bounded selected upstream observation (t/TEST base series)";
/// Bounded selected upstream observation of the `t/TEST` comp series.
const OWNER_UPSTREAM_COMP: &str =
    "#12291/#12140 bounded selected upstream observation (t/TEST comp series)";
/// Bounded selected upstream observation of the `t/TEST` run series.
const OWNER_UPSTREAM_RUN: &str =
    "#12291/#12141 bounded selected upstream observation (t/TEST run series)";
/// Whole bounded selected upstream observation series.
const OWNER_UPSTREAM_SERIES: &str =
    "#12291/#12139-#12141 bounded selected upstream observation series";
/// Accepted general-semantic debt retirement.
const OWNER_SEMANTIC_DEBT: &str = "#12117-#12120/#12165 semantic debt retirement";
/// Compiler fact production (parse/semantic/PIR).
const OWNER_COMPILER_FACTS: &str = "#11665-#11670/#5214/#12109-#12111/#12191/#2660 compiler facts";
/// Local product proof.
const OWNER_LOCAL_PROOF: &str = "#8669/#12156/#12157/#12079 local product proof";
/// World/graph/currentness.
const OWNER_WORLD_GRAPH: &str = "#4772/#4746/#2425/#2493/#8797/#5241/#8820 world/graph/currentness";
/// Cross-file/refactor/representative proof.
const OWNER_CROSS_FILE: &str = "#6232/#7430/#9370 cross-file/refactor/representative proof";
/// Bounded EIR.
const OWNER_BOUNDED_EIR: &str = "#4770/#4773/#4775/#4777/#4779/#2447 bounded EIR";
/// Gold/oracle.
const OWNER_GOLD_ORACLE: &str = "#4760-#4767 gold/oracle";
/// Editor static boundary.
const OWNER_EDITOR_BOUNDARY: &str = "#7422 editor static boundary";
/// Packaged process.
const OWNER_PACKAGED: &str = "#6720/#6744/#7133/#6056 packaged process";
/// Installed client.
const OWNER_INSTALLED_CLIENT: &str = "#4346/#6739/#7122 installed client";
/// Work/performance.
const OWNER_WORK_PERF: &str = "#9311/#9316/#9321 work/performance";
/// Topology/route/nonzero work.
const OWNER_TOPOLOGY: &str = "#12125-#12129 topology/route/nonzero work";
/// Compiler-profile train (profile identity and claim-ceiling rows).
const OWNER_PROFILE_TRAIN: &str = "#12176 compiler-profile train";
/// Later full integrated publication (never a prerequisite to bounded rows).
const OWNER_INTEGRATED_PUBLICATION: &str = "#8722 full integrated publication";

/// Default wake event: an accepted receipt re-opens, or a semantic row field
/// changes (which itself requires version movement).
const WAKE: &str = "owning accepted receipt series re-opened or a semantic row field changed";

const LOCAL_ID: &str = "compiler_local_lexical";
const PROJECT_ID: &str = "compiler_static_project";
const EXECUTION_ID: &str = "compiler_bounded_execution";
const MAINTAINED_ID: &str = "compiler_maintained_code_intelligence";
const V1: &str = "v1";

// ---------------------------------------------------------------------------
// Construction helpers
// ---------------------------------------------------------------------------

fn owner(reference: &str) -> Result<OwnerAndWakeEvent> {
    OwnerAndWakeEvent::new(reference, WAKE)
}

fn local(subject: &str) -> Result<SubjectSelector> {
    Ok(SubjectSelector::LocalLexical(SubjectRef::new(subject)?))
}

fn project(subject: &str) -> Result<SubjectSelector> {
    Ok(SubjectSelector::StaticProject(SubjectRef::new(subject)?))
}

fn cross_file(subject: &str) -> Result<SubjectSelector> {
    Ok(SubjectSelector::CrossFileExternal(SubjectRef::new(subject)?))
}

fn execution(subject: &str) -> Result<SubjectSelector> {
    Ok(SubjectSelector::BoundedExecution(SubjectRef::new(subject)?))
}

fn packaged(subject: &str) -> Result<SubjectSelector> {
    Ok(SubjectSelector::PackagedArtifact(SubjectRef::new(subject)?))
}

fn installed(subject: &str) -> Result<SubjectSelector> {
    Ok(SubjectSelector::InstalledHostEnvironment(SubjectRef::new(subject)?))
}

fn client(subject: &str) -> Result<SubjectSelector> {
    Ok(SubjectSelector::ActualClientSurface(SubjectRef::new(subject)?))
}

fn bounded(boundary: &str) -> CoverageRule {
    CoverageRule::Bounded { boundary: boundary.to_owned() }
}

fn invalidate(kind: InvalidationKind, detail: &str) -> Result<InvalidationInput> {
    InvalidationInput::new(kind, detail)
}

/// Base row: required disposition, source-locked exhaustive correctness,
/// observed-evidence ceiling, no legacy exit, one source invalidation input.
/// Every row below starts here and then names its exact deviations; no field
/// is ever left absent, so a row missing any required field cannot construct
/// or validate.
fn base_row(
    id: &str,
    family: ClaimFamily,
    subject: SubjectSelector,
    tier: SourceTier,
    axes: &[ProofClass],
    owner_reference: &str,
) -> Result<CompilerProfileRow> {
    Ok(CompilerProfileRow {
        id: CompilerProfileRowId::new(id)?,
        disposition: RowDisposition::Required,
        subject,
        evidence: EvidenceRequirement::new(family, tier, axes.iter().copied().collect())?,
        completeness: CompletenessRequirement {
            currentness: CurrentnessRule::SourceLocked,
            coverage: CoverageRule::Exhaustive,
        },
        work: WorkRequirement::Correctness,
        limitations: Vec::new(),
        legacy_exit: LegacyExitRequirement::NONE,
        ceiling: ClaimCeiling::ObservedEvidence,
        invalidation: vec![invalidate(
            InvalidationKind::Source,
            "the named subject's source or accepted receipt basis changed",
        )?],
        owner: owner(owner_reference)?,
    })
}

const LEGACY_EXIT_FULL: LegacyExitRequirement = LegacyExitRequirement {
    replacement_currentness: Obligation::Required,
    old_path_absence: Obligation::Required,
    recurrence_proof: Obligation::Required,
};

// ---------------------------------------------------------------------------
// compiler_local_lexical.v1 (22 rows, no imports)
// ---------------------------------------------------------------------------

fn local_rows() -> Result<Vec<CompilerProfileRow>> {
    let mut rows = Vec::with_capacity(22);

    // Candidate/source/toolchain identity.
    let mut row = base_row(
        "lexical.candidate-toolchain-identity",
        ClaimFamily::ExactProcess,
        local(
            "exact candidate source tree, strict bundle, and toolchain identity under observation",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_LOCAL_PROOF,
    )?;
    row.invalidation.push(invalidate(
        InvalidationKind::Dependency,
        "the toolchain or strict bundle pin changed",
    )?);
    rows.push(row);

    // Selected base parse observation (#12291/#12139 bounded packet).
    let mut row = base_row(
        "lexical.observation-base-parse",
        ClaimFamily::ParserInternal,
        local("selected t/TEST base parse observation (#12291/#12139 bounded packet)")?,
        SourceTier::ExactProcess,
        &[ProofClass::CuratedExpectation],
        OWNER_UPSTREAM_BASE,
    )?;
    row.completeness.coverage = bounded(
        "#12291 bounded selected t/TEST base parse series; #8722 may later widen publication but cannot redefine this series, subject, or denominator",
    );
    rows.push(row);

    // Selected base compile observation.
    let mut row = base_row(
        "lexical.observation-base-compile",
        ClaimFamily::ParserInternal,
        local("selected t/TEST base compile observation (#12291/#12139 bounded packet)")?,
        SourceTier::ExactProcess,
        &[ProofClass::CuratedExpectation],
        OWNER_UPSTREAM_BASE,
    )?;
    row.completeness.coverage = bounded(
        "#12291 bounded selected t/TEST base compile series; #8722 may later widen publication but cannot redefine this series, subject, or denominator",
    );
    rows.push(row);

    // Selected comp parse observation.
    let mut row = base_row(
        "lexical.observation-comp-parse",
        ClaimFamily::ParserInternal,
        local("selected t/TEST comp parse observation (#12291/#12140 bounded packet)")?,
        SourceTier::ExactProcess,
        &[ProofClass::CuratedExpectation],
        OWNER_UPSTREAM_COMP,
    )?;
    row.completeness.coverage = bounded(
        "#12291 bounded selected t/TEST comp parse series; #8722 may later widen publication but cannot redefine this series, subject, or denominator",
    );
    rows.push(row);

    // Selected comp compile observation.
    let mut row = base_row(
        "lexical.observation-comp-compile",
        ClaimFamily::ParserInternal,
        local("selected t/TEST comp compile observation (#12291/#12140 bounded packet)")?,
        SourceTier::ExactProcess,
        &[ProofClass::CuratedExpectation],
        OWNER_UPSTREAM_COMP,
    )?;
    row.completeness.coverage = bounded(
        "#12291 bounded selected t/TEST comp compile series; #8722 may later widen publication but cannot redefine this series, subject, or denominator",
    );
    rows.push(row);

    // Selected run parse observation.
    let mut row = base_row(
        "lexical.observation-run-parse",
        ClaimFamily::ParserInternal,
        local("selected t/TEST run parse observation (#12291/#12141 bounded packet)")?,
        SourceTier::ExactProcess,
        &[ProofClass::CuratedExpectation],
        OWNER_UPSTREAM_RUN,
    )?;
    row.completeness.coverage = bounded(
        "#12291 bounded selected t/TEST run parse series; #8722 may later widen publication but cannot redefine this series, subject, or denominator",
    );
    rows.push(row);

    // Selected run compile observation.
    let mut row = base_row(
        "lexical.observation-run-compile",
        ClaimFamily::ParserInternal,
        local("selected t/TEST run compile observation (#12291/#12141 bounded packet)")?,
        SourceTier::ExactProcess,
        &[ProofClass::CuratedExpectation],
        OWNER_UPSTREAM_RUN,
    )?;
    row.completeness.coverage = bounded(
        "#12291 bounded selected t/TEST run compile series; #8722 may later widen publication but cannot redefine this series, subject, or denominator",
    );
    rows.push(row);

    // Observed invocation/process validity.
    let mut row = base_row(
        "lexical.invocation-process-validity",
        ClaimFamily::ExactProcess,
        local("observed invocation and process validity of the bounded selected observation runs")?,
        SourceTier::ExactProcess,
        &[ProofClass::CuratedExpectation],
        OWNER_UPSTREAM_SERIES,
    )?;
    row.completeness.coverage =
        bounded("#12291 bounded selected observation runs (base/comp/run parse+compile)");
    rows.push(row);

    // Accepted general-semantic debt retirement.
    let mut row = base_row(
        "lexical.semantic-debt-retirement-accepted",
        ClaimFamily::LegacyExit,
        local("accepted retirement of the general-semantic debt named by #12117-#12120/#12165")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_SEMANTIC_DEBT,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.legacy_exit = LEGACY_EXIT_FULL;
    rows.push(row);

    // Accepted parser generation.
    let mut row = base_row(
        "lexical.parser-generation-accepted",
        ClaimFamily::ParserInternal,
        local("accepted parser generation for the local lexical grammar")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_COMPILER_FACTS,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    // Accepted semantic snapshot.
    let mut row = base_row(
        "lexical.semantic-snapshot-accepted",
        ClaimFamily::ParserInternal,
        local("accepted semantic snapshot for the local lexical subject")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_COMPILER_FACTS,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    // Current PIR lexical contribution.
    rows.push(base_row(
        "lexical.pir-lexical-contribution",
        ClaimFamily::ParserInternal,
        local("current PIR lexical contribution for the local lexical subject")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_COMPILER_FACTS,
    )?);

    // External compiler-backed references.
    let mut row = base_row(
        "lexical.external-references-compiler-backed",
        ClaimFamily::Provider,
        local("external compiler-backed references for the local lexical subject")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_LOCAL_PROOF,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    // Complete occurrence denominator.
    let mut row = base_row(
        "lexical.occurrence-denominator-complete",
        ClaimFamily::Provider,
        local("complete occurrence denominator for local lexical reference and rename subjects")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_LOCAL_PROOF,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    // prepareRename/rename authorization.
    let mut row = base_row(
        "lexical.rename-authorization",
        ClaimFamily::Edit,
        local("prepareRename/rename authorization for local lexical symbols")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_LOCAL_PROOF,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    // Independent WorkspaceEdit application.
    let mut row = base_row(
        "lexical.workspace-edit-application",
        ClaimFamily::Edit,
        local("independent WorkspaceEdit application of an authorized local rename")?,
        SourceTier::Source,
        &[ProofClass::EvaluatedWork],
        OWNER_LOCAL_PROOF,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    // Zero request-time compiler construction.
    let mut row = base_row(
        "lexical.zero-request-time-compiler-construction",
        ClaimFamily::Provider,
        local("zero request-time compiler construction on governed local lexical requests")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_LOCAL_PROOF,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    // Zero admitted legacy/source-scan work.
    let mut row = base_row(
        "lexical.zero-legacy-source-scan-work",
        ClaimFamily::LegacyExit,
        local("zero admitted legacy or source-scan work on the local lexical path")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_SEMANTIC_DEBT,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.legacy_exit = LEGACY_EXIT_FULL;
    rows.push(row);

    // Lifecycle/currentness/cleanup.
    let mut row = base_row(
        "lexical.lifecycle-currentness-cleanup",
        ClaimFamily::Provider,
        local("local lexical document lifecycle, currentness, and cleanup")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_LOCAL_PROOF,
    )?;
    row.invalidation.push(invalidate(
        InvalidationKind::Dependency,
        "a dependency of the local lexical document lifecycle changed",
    )?);
    rows.push(row);

    // Required mutation execution.
    let mut row = base_row(
        "lexical.mutation-execution-required",
        ClaimFamily::TestReachability,
        local("required mutation execution over the local lexical proof surface")?,
        SourceTier::Source,
        &[ProofClass::EvaluatedWork],
        OWNER_TOPOLOGY,
    )?;
    row.work = WorkRequirement::Production(WorkScope::new(
        "non-zero mutation execution over the local lexical proof surface",
    )?);
    rows.push(row);

    // Exact external perllsp process.
    let mut row = base_row(
        "lexical.exact-perllsp-process",
        ClaimFamily::ExactProcess,
        local("exact external perllsp process behavior for the local lexical surface")?,
        SourceTier::ExactProcess,
        &[ProofClass::CuratedExpectation],
        OWNER_LOCAL_PROOF,
    )?;
    row.completeness.currentness = CurrentnessRule::HostObserved;
    row.invalidation.push(invalidate(
        InvalidationKind::HostEnvironment,
        "the host environment of the observed perllsp process changed",
    )?);
    rows.push(row);

    // Local-profile claim ceiling.
    let mut row = base_row(
        "lexical.claim-ceiling",
        ClaimFamily::PublicClaim,
        local(
            "maximum claim wording of compiler_local_lexical.v1: bounded accepted local lexical \
             compatibility; no project, execution, packaged, installed, client, support, or \
             release claim",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_PROFILE_TRAIN,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    Ok(rows)
}

/// Checked `compiler_local_lexical.v1`: bounded local-lexical profile with no
/// imports and no long-horizon prerequisites (#8722 is not a prerequisite).
pub fn compiler_local_lexical_v1() -> Result<CompilerProfileDefinition> {
    Ok(CompilerProfileDefinition {
        id: CompilerProfileId::new(LOCAL_ID)?,
        version: CompilerProfileVersion::new(V1)?,
        purpose: "bounded local-lexical compiler operating profile over the #12291/#12139-#12141 \
                  selected upstream observations and the local compiler-backed product proof"
            .to_owned(),
        change_reason: "initial checked row inventory for #12330".to_owned(),
        owner: owner(OWNER_PROFILE_TRAIN)?,
        imports: Vec::new(),
        rows: local_rows()?,
        limitations: vec![AllowedLimitation::new(
            "lim.local-lexical-scope",
            "local lexical analysis",
            "bounded to local lexical subjects; project, execution, packaged, installed, and \
             client claims belong to higher profiles",
            OWNER_PROFILE_TRAIN,
        )?],
    })
}

// ---------------------------------------------------------------------------
// compiler_static_project.v1 (19 own rows + verbatim local import)
// ---------------------------------------------------------------------------

fn project_rows() -> Result<Vec<CompilerProfileRow>> {
    let mut rows = Vec::with_capacity(19);

    // Exact imported local profile.
    let mut row = base_row(
        "project.import-local-profile-exact",
        ClaimFamily::ProjectWorld,
        project(
            "exact import of compiler_local_lexical v1 by identity, version, and semantic digest",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_PROFILE_TRAIN,
    )?;
    row.invalidation.push(invalidate(
        InvalidationKind::Dependency,
        "the imported lower profile semantic digest changed",
    )?);
    rows.push(row);

    // Accepted ProjectModel/CompilerWorld snapshot.
    let mut row = base_row(
        "project.world-snapshot-accepted",
        ClaimFamily::ProjectWorld,
        project("accepted ProjectModel/CompilerWorld snapshot for the open workspace")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_WORLD_GRAPH,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the accepted snapshot",
    )?);
    rows.push(row);

    // Root/project/profile/source-generation closure.
    let mut row = base_row(
        "project.root-project-profile-source-generation-closure",
        ClaimFamily::ProjectWorld,
        project("root, project, profile, and source-generation closure of the project model")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_WORLD_GRAPH,
    )?;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the closure evidence",
    )?);
    rows.push(row);

    // Compile-time module/dependency graph.
    let mut row = base_row(
        "project.module-dependency-graph",
        ClaimFamily::ProjectWorld,
        project("compile-time module and dependency graph of the project")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_WORLD_GRAPH,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation
        .push(invalidate(InvalidationKind::Dependency, "a module or dependency edge changed")?);
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the graph evidence",
    )?);
    rows.push(row);

    // SCC schedule.
    let mut row = base_row(
        "project.scc-schedule",
        ClaimFamily::ProjectWorld,
        project("strongly-connected-component schedule over the module/dependency graph")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_WORLD_GRAPH,
    )?;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::Dependency,
        "a module or dependency edge changed the SCC decomposition",
    )?);
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the schedule evidence",
    )?);
    rows.push(row);

    // Private implementation transition.
    let mut row = base_row(
        "project.private-implementation-transition",
        ClaimFamily::ProjectWorld,
        project("private implementation transition semantics in the dependency graph")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_WORLD_GRAPH,
    )?;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation
        .push(invalidate(InvalidationKind::Dependency, "a private implementation edge changed")?);
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the transition evidence",
    )?);
    rows.push(row);

    // Public interface transition.
    let mut row = base_row(
        "project.public-interface-transition",
        ClaimFamily::ProjectWorld,
        project("public interface transition semantics in the dependency graph")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_WORLD_GRAPH,
    )?;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation
        .push(invalidate(InvalidationKind::Dependency, "a public interface edge changed")?);
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the transition evidence",
    )?);
    rows.push(row);

    // Reverse-dependency invalidation closure.
    let mut row = base_row(
        "project.reverse-dependency-invalidation-closure",
        ClaimFamily::ProjectWorld,
        project("reverse-dependency invalidation closure for changed modules")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_WORLD_GRAPH,
    )?;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::Dependency,
        "a dependency changed and must invalidate its reverse closure",
    )?);
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the invalidation evidence",
    )?);
    rows.push(row);

    // Stale publication rejection.
    let mut row = base_row(
        "project.stale-publication-rejection",
        ClaimFamily::ProjectWorld,
        project("rejection of stale fact publication against the current world model")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_WORLD_GRAPH,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model advanced past the published facts",
    )?);
    rows.push(row);

    // Multi-root and close/reopen currentness.
    let mut row = base_row(
        "project.multi-root-close-reopen-currentness",
        ClaimFamily::ProjectWorld,
        project("multi-root and close/reopen currentness of the project/world model")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_WORLD_GRAPH,
    )?;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "a root was added, removed, closed, or reopened",
    )?);
    rows.push(row);

    // Compiler-world-backed cross-file definition.
    let mut row = base_row(
        "project.cross-file-definition",
        ClaimFamily::Provider,
        cross_file("compiler-world-backed cross-file definition")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_CROSS_FILE,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the cross-file definition evidence",
    )?);
    rows.push(row);

    // Compiler-world-backed cross-file references.
    let mut row = base_row(
        "project.cross-file-references",
        ClaimFamily::Provider,
        cross_file("compiler-world-backed cross-file references")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_CROSS_FILE,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the cross-file references evidence",
    )?);
    rows.push(row);

    // Complete-or-refuse cross-file rename.
    let mut row = base_row(
        "project.cross-file-rename-complete-or-refuse",
        ClaimFamily::Edit,
        cross_file("complete-or-refuse cross-file rename authorization")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_CROSS_FILE,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the cross-file rename evidence",
    )?);
    rows.push(row);

    // Independent cross-file edit application.
    let mut row = base_row(
        "project.cross-file-edit-application",
        ClaimFamily::Edit,
        cross_file("independent cross-file edit application of an authorized rename")?,
        SourceTier::Source,
        &[ProofClass::EvaluatedWork],
        OWNER_CROSS_FILE,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the cross-file edit evidence",
    )?);
    rows.push(row);

    // Representative project lifecycle.
    let mut row = base_row(
        "project.representative-project-lifecycle",
        ClaimFamily::ProjectWorld,
        project("representative project lifecycle over the world model")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_CROSS_FILE,
    )?;
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.completeness.coverage =
        bounded("representative project lifecycle named by #9370; not every possible project");
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the lifecycle evidence",
    )?);
    rows.push(row);

    // Cold-equivalence correctness.
    let mut row = base_row(
        "project.cold-equivalence-correctness",
        ClaimFamily::ProjectWorld,
        project("cold-equivalence correctness between warm incremental and cold recompute")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_WORLD_GRAPH,
    )?;
    row.work = WorkRequirement::OracleOrCold(WorkScope::new(
        "cold-path recompute of the representative project",
    )?);
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the cold-equivalence evidence",
    )?);
    rows.push(row);

    // Production reuse/recompute work.
    let mut row = base_row(
        "project.reuse-recompute-work",
        ClaimFamily::ProjectWorld,
        project("production reuse and recompute work over the representative project")?,
        SourceTier::Source,
        &[ProofClass::EvaluatedWork],
        OWNER_WORK_PERF,
    )?;
    row.work = WorkRequirement::Production(WorkScope::new(
        "non-zero reuse/recompute work over the representative project",
    )?);
    row.completeness.currentness = CurrentnessRule::ProjectWorldCurrent;
    row.invalidation.push(invalidate(
        InvalidationKind::WorldModel,
        "the project/world model changed under the work evidence",
    )?);
    rows.push(row);

    // Bounded project performance/resource envelope.
    let mut row = base_row(
        "project.performance-resource-envelope",
        ClaimFamily::Performance,
        project("bounded project performance and resource envelope")?,
        SourceTier::Source,
        &[ProofClass::EvaluatedWork],
        OWNER_WORK_PERF,
    )?;
    row.work = WorkRequirement::PerformanceResource(WorkScope::new(
        "measured resource envelope over the representative project",
    )?);
    row.completeness.coverage =
        bounded("representative project and host class named by #9311/#9316/#9321");
    rows.push(row);

    // Static-project claim ceiling.
    let mut row = base_row(
        "project.claim-ceiling",
        ClaimFamily::PublicClaim,
        project(
            "maximum claim wording of compiler_static_project.v1: bounded accepted \
             static-project compatibility; no execution, packaged, installed, client, support, \
             or release claim",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_PROFILE_TRAIN,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    Ok(rows)
}

/// Checked `compiler_static_project.v1`: imports `compiler_local_lexical.v1`
/// verbatim and adds the project/world, cross-file, and work envelope rows.
pub fn compiler_static_project_v1() -> Result<CompilerProfileDefinition> {
    let lower = compiler_local_lexical_v1()?;
    let mut rows = lower.rows.clone();
    rows.extend(project_rows()?);
    let mut limitations = lower.limitations.clone();
    limitations.push(AllowedLimitation::new(
        "lim.static-project-bound",
        "static project analysis",
        "static project facts only; bounded execution claims belong to \
         compiler_bounded_execution.v1",
        OWNER_PROFILE_TRAIN,
    )?);
    Ok(CompilerProfileDefinition {
        id: CompilerProfileId::new(PROJECT_ID)?,
        version: CompilerProfileVersion::new(V1)?,
        purpose: "static project compiler operating profile over the accepted \
                  ProjectModel/CompilerWorld snapshot and compiler-world-backed cross-file proof"
            .to_owned(),
        change_reason: "initial checked row inventory for #12330".to_owned(),
        owner: owner(OWNER_PROFILE_TRAIN)?,
        imports: vec![CompilerProfileImport::for_profile(&lower)?],
        rows,
        limitations,
    })
}

// ---------------------------------------------------------------------------
// compiler_bounded_execution.v1 (19 own rows + verbatim project import)
// ---------------------------------------------------------------------------

fn execution_rows() -> Result<Vec<CompilerProfileRow>> {
    let mut rows = Vec::with_capacity(19);

    // Exact executable profile/version/hash.
    let mut row = base_row(
        "execution.executable-profile-identity",
        ClaimFamily::ExactProcess,
        execution("exact executable profile, version, and content hash under bounded execution")?,
        SourceTier::ExactProcess,
        &[ProofClass::CuratedExpectation],
        OWNER_BOUNDED_EIR,
    )?;
    row.completeness.currentness = CurrentnessRule::ExecutionBounded;
    rows.push(row);

    // Unsupported-fact catalog identity.
    rows.push(base_row(
        "execution.unsupported-fact-catalog-identity",
        ClaimFamily::Execution,
        execution(
            "identity of the explicit unsupported-fact catalog admitted to bounded execution",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_BOUNDED_EIR,
    )?);

    // PackageSubTable admitted effect denominator.
    rows.push(base_row(
        "execution.package-subtable-effect-denominator",
        ClaimFamily::Execution,
        execution("PackageSubTable admitted effect denominator for bounded execution")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_BOUNDED_EIR,
    )?);

    // Canonical compiler-fact→EIR lowering.
    rows.push(base_row(
        "execution.compiler-fact-eir-lowering",
        ClaimFamily::Execution,
        execution("canonical compiler-fact to EIR lowering for admitted effects")?,
        SourceTier::Source,
        &[ProofClass::EirMechanism],
        OWNER_BOUNDED_EIR,
    )?);

    // EIR verification.
    rows.push(base_row(
        "execution.eir-verification",
        ClaimFamily::Execution,
        execution("EIR verification of the lowered admitted effects")?,
        SourceTier::Source,
        &[ProofClass::EirMechanism],
        OWNER_BOUNDED_EIR,
    )?);

    // Bounded deterministic evaluation.
    let mut row = base_row(
        "execution.bounded-deterministic-evaluation",
        ClaimFamily::Execution,
        execution("bounded deterministic evaluation of verified EIR")?,
        SourceTier::ExactProcess,
        &[ProofClass::EirMechanism, ProofClass::EvaluatedWork],
        OWNER_BOUNDED_EIR,
    )?;
    row.completeness.currentness = CurrentnessRule::ExecutionBounded;
    rows.push(row);

    // Hard limit/resource policy.
    rows.push(base_row(
        "execution.hard-limit-resource-policy",
        ClaimFamily::Execution,
        execution("hard limit and resource policy bounding deterministic evaluation")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_BOUNDED_EIR,
    )?);

    // Independently reviewed curated gold.
    rows.push(base_row(
        "execution.curated-gold-reviewed",
        ClaimFamily::Execution,
        execution("independently reviewed curated gold expectations for bounded execution")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_GOLD_ORACLE,
    )?);

    // Hermetic real-Perl oracle.
    let mut row = base_row(
        "execution.hermetic-real-perl-oracle",
        ClaimFamily::Execution,
        execution("hermetic real-Perl oracle for admitted bounded execution effects")?,
        SourceTier::ExactProcess,
        &[ProofClass::RealPerlOracle],
        OWNER_GOLD_ORACLE,
    )?;
    row.work = WorkRequirement::OracleOrCold(WorkScope::new(
        "hermetic real-Perl oracle runs inside the bounded envelope",
    )?);
    row.completeness.currentness = CurrentnessRule::ExecutionBounded;
    row.invalidation.push(invalidate(
        InvalidationKind::Oracle,
        "the oracle basis changed under the agreement evidence",
    )?);
    rows.push(row);

    // EIR agreement with gold.
    let mut row = base_row(
        "execution.eir-gold-agreement",
        ClaimFamily::Execution,
        execution(
            "agreement between EIR evaluation and the curated gold for the admitted denominator",
        )?,
        SourceTier::Source,
        &[ProofClass::EirMechanism, ProofClass::CuratedExpectation],
        OWNER_GOLD_ORACLE,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    // EIR agreement with oracle.
    let mut row = base_row(
        "execution.eir-oracle-agreement",
        ClaimFamily::Execution,
        execution(
            "agreement between EIR evaluation and the hermetic real-Perl oracle for the \
             admitted denominator",
        )?,
        SourceTier::ExactProcess,
        &[ProofClass::EirMechanism, ProofClass::RealPerlOracle],
        OWNER_GOLD_ORACLE,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::ExecutionBounded;
    row.invalidation.push(invalidate(
        InvalidationKind::Oracle,
        "the oracle basis changed under the agreement evidence",
    )?);
    rows.push(row);

    // Selected upstream row denominator.
    rows.push(base_row(
        "execution.selected-upstream-row-denominator",
        ClaimFamily::Execution,
        execution(
            "selected upstream row denominator admitted to bounded execution \
             (#12291/#12139-#12141 selection)",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_UPSTREAM_SERIES,
    )?);

    // Nonzero EIR work.
    let mut row = base_row(
        "execution.nonzero-eir-work",
        ClaimFamily::Execution,
        execution("non-zero EIR lowering, verification, and evaluation work")?,
        SourceTier::Source,
        &[ProofClass::EvaluatedWork],
        OWNER_TOPOLOGY,
    )?;
    row.work = WorkRequirement::Production(WorkScope::new(
        "non-zero EIR lowering, verification, and evaluation work over the admitted denominator",
    )?);
    rows.push(row);

    // Nonzero TAP work.
    let mut row = base_row(
        "execution.nonzero-tap-work",
        ClaimFamily::Execution,
        execution("non-zero TAP harness execution work over the admitted denominator")?,
        SourceTier::ExactProcess,
        &[ProofClass::EvaluatedWork],
        OWNER_TOPOLOGY,
    )?;
    row.work = WorkRequirement::Production(WorkScope::new(
        "non-zero TAP harness execution work over the admitted denominator",
    )?);
    row.completeness.currentness = CurrentnessRule::ExecutionBounded;
    rows.push(row);

    // Zero legacy selected-scaffold calls for migrated rows.
    let mut row = base_row(
        "execution.zero-legacy-scaffold-calls",
        ClaimFamily::LegacyExit,
        execution("zero legacy selected-scaffold calls for migrated bounded-execution rows")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_BOUNDED_EIR,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.legacy_exit = LEGACY_EXIT_FULL;
    rows.push(row);

    // No project execution from governed editor requests.
    let mut row = base_row(
        "execution.no-project-execution-from-editor-requests",
        ClaimFamily::Execution,
        execution("no project execution is reachable from governed editor requests")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_EDITOR_BOUNDARY,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    // editor_runtime_dependency=false.
    let mut row = base_row(
        "execution.editor-runtime-dependency-false",
        ClaimFamily::Execution,
        execution("editor_runtime_dependency is false for the governed editor surface")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_EDITOR_BOUNDARY,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    // Explicit dynamic/magic/XS/tie/ambient boundaries.
    let mut row = base_row(
        "execution.dynamic-boundaries-explicit",
        ClaimFamily::Execution,
        execution("explicit dynamic/magic/XS/tie/ambient boundaries of bounded execution")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_BOUNDED_EIR,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.coverage = CoverageRule::ExplicitlyPartial {
        remainder: "dynamic, magic, XS, tie, and ambient constructs are outside the admitted \
                    envelope"
            .to_owned(),
    };
    rows.push(row);

    // Bounded-execution claim ceiling.
    let mut row = base_row(
        "execution.claim-ceiling",
        ClaimFamily::PublicClaim,
        execution(
            "maximum claim wording of compiler_bounded_execution.v1: bounded accepted execution \
             compatibility inside the admitted envelope; no packaged, installed, client, \
             support, or release claim",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_PROFILE_TRAIN,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    rows.push(row);

    Ok(rows)
}

/// Checked `compiler_bounded_execution.v1`: imports
/// `compiler_static_project.v1` verbatim and adds the bounded EIR, gold,
/// oracle, denominator, work, and editor-boundary rows.
pub fn compiler_bounded_execution_v1() -> Result<CompilerProfileDefinition> {
    let lower = compiler_static_project_v1()?;
    let mut rows = lower.rows.clone();
    rows.extend(execution_rows()?);
    let mut limitations = lower.limitations.clone();
    limitations.push(AllowedLimitation::new(
        "lim.bounded-execution-envelope",
        "bounded execution",
        "dynamic, magic, XS, tie, and ambient constructs are outside the admitted envelope \
         (#2447 unsupported-fact catalog)",
        OWNER_PROFILE_TRAIN,
    )?);
    Ok(CompilerProfileDefinition {
        id: CompilerProfileId::new(EXECUTION_ID)?,
        version: CompilerProfileVersion::new(V1)?,
        purpose: "bounded execution compiler operating profile over the admitted EIR \
                  denominator with curated gold, hermetic real-Perl oracle, and evaluated work"
            .to_owned(),
        change_reason: "initial checked row inventory for #12330".to_owned(),
        owner: owner(OWNER_PROFILE_TRAIN)?,
        imports: vec![CompilerProfileImport::for_profile(&lower)?],
        rows,
        limitations,
    })
}

// ---------------------------------------------------------------------------
// compiler_maintained_code_intelligence.v1 (19 own rows + verbatim execution import)
// ---------------------------------------------------------------------------

fn maintained_rows() -> Result<Vec<CompilerProfileRow>> {
    let mut rows = Vec::with_capacity(19);

    // Exact imported lower-profile identities.
    let mut row = base_row(
        "intelligence.import-lower-profile-identities-exact",
        ClaimFamily::ProjectWorld,
        cross_file(
            "exact import of compiler_bounded_execution v1 (and transitively every lower \
             profile) by identity, version, and semantic digest",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_PROFILE_TRAIN,
    )?;
    row.invalidation.push(invalidate(
        InvalidationKind::Dependency,
        "an imported lower profile semantic digest changed",
    )?);
    rows.push(row);

    // Maintained Perl/compiler/upstream series denominator.
    rows.push(base_row(
        "intelligence.upstream-series-denominator",
        ClaimFamily::Execution,
        cross_file("maintained Perl/compiler/upstream series denominator selected for the maintained profile")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_UPSTREAM_SERIES,
    )?);

    // Selected provider/refactor rows.
    let mut row = base_row(
        "intelligence.selected-provider-refactor-rows",
        ClaimFamily::Provider,
        cross_file("selected provider and refactor rows of the maintained profile")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_CROSS_FILE,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.coverage =
        bounded("selected provider/refactor rows named by #6232/#7430/#9370");
    rows.push(row);

    // Release-shaped package identity.
    let mut row = base_row(
        "intelligence.release-shaped-package-identity",
        ClaimFamily::Packaged,
        packaged("release-shaped package identity of the maintained profile artifact")?,
        SourceTier::Packaged,
        &[ProofClass::CuratedExpectation],
        OWNER_PACKAGED,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::HostObserved;
    row.invalidation.push(invalidate(
        InvalidationKind::HostEnvironment,
        "the packaged artifact or its host environment changed",
    )?);
    rows.push(row);

    // Contained binary/process identity.
    let mut row = base_row(
        "intelligence.contained-binary-process-identity",
        ClaimFamily::Packaged,
        packaged("contained binary/process identity inside the release-shaped package")?,
        SourceTier::Packaged,
        &[ProofClass::CuratedExpectation],
        OWNER_PACKAGED,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::HostObserved;
    row.invalidation.push(invalidate(
        InvalidationKind::HostEnvironment,
        "the contained binary/process or its host environment changed",
    )?);
    rows.push(row);

    // Packaged semantic cells.
    let mut row = base_row(
        "intelligence.packaged-semantic-cells",
        ClaimFamily::Packaged,
        packaged("packaged semantic cells exercised from the release-shaped artifact")?,
        SourceTier::Packaged,
        &[ProofClass::EvaluatedWork],
        OWNER_PACKAGED,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::HostObserved;
    row.completeness.coverage =
        bounded("selected packaged semantic cells named by #6056/#6720; not every packaged cell");
    row.invalidation.push(invalidate(
        InvalidationKind::HostEnvironment,
        "the packaged artifact or its host environment changed under the cell evidence",
    )?);
    rows.push(row);

    // Manifest-selected client/plugin/platform identity.
    let mut row = base_row(
        "intelligence.manifest-selected-client-platform-identity",
        ClaimFamily::InstalledHost,
        installed("manifest-selected client, plugin, and platform identity")?,
        SourceTier::InstalledHost,
        &[ProofClass::CuratedExpectation],
        OWNER_INSTALLED_CLIENT,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::HostObserved;
    row.invalidation.push(invalidate(
        InvalidationKind::HostEnvironment,
        "the installed host, client, plugin, or platform changed",
    )?);
    rows.push(row);

    // Actual client launches exact packaged bytes.
    let mut row = base_row(
        "intelligence.client-launches-exact-packaged-bytes",
        ClaimFamily::ActualClient,
        client("actual client launches the exact packaged bytes")?,
        SourceTier::ActualClient,
        &[ProofClass::EvaluatedWork],
        OWNER_INSTALLED_CLIENT,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::HostObserved;
    row.invalidation.push(invalidate(
        InvalidationKind::HostEnvironment,
        "the actual client or installed host environment changed",
    )?);
    rows.push(row);

    // Selected client application cells.
    let mut row = base_row(
        "intelligence.selected-client-application-cells",
        ClaimFamily::ActualClient,
        client("selected client application cells driven by the actual client")?,
        SourceTier::ActualClient,
        &[ProofClass::EvaluatedWork],
        OWNER_INSTALLED_CLIENT,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.completeness.currentness = CurrentnessRule::HostObserved;
    row.completeness.coverage = bounded(
        "selected client application cells named by #4346/#6739/#7122; not every client cell",
    );
    row.invalidation.push(invalidate(
        InvalidationKind::HostEnvironment,
        "the actual client or installed host environment changed under the cell evidence",
    )?);
    rows.push(row);

    // Client lifecycle/restart/currentness/cleanup.
    let mut row = base_row(
        "intelligence.client-lifecycle-restart-currentness-cleanup",
        ClaimFamily::ActualClient,
        client("client lifecycle, restart, currentness, and cleanup on the installed host")?,
        SourceTier::ActualClient,
        &[ProofClass::CuratedExpectation],
        OWNER_INSTALLED_CLIENT,
    )?;
    row.completeness.currentness = CurrentnessRule::HostObserved;
    row.invalidation.push(invalidate(
        InvalidationKind::HostEnvironment,
        "the client lifecycle or installed host environment changed",
    )?);
    rows.push(row);

    // Correctness-bound work envelope.
    let mut row = base_row(
        "intelligence.correctness-bound-work-envelope",
        ClaimFamily::Performance,
        execution("correctness-bound work envelope over the maintained profile targets")?,
        SourceTier::Source,
        &[ProofClass::EvaluatedWork],
        OWNER_WORK_PERF,
    )?;
    row.work = WorkRequirement::PerformanceResource(WorkScope::new(
        "correctness-bound work envelope over the maintained profile targets",
    )?);
    rows.push(row);

    // Latency/resource thresholds and policy.
    let mut row = base_row(
        "intelligence.latency-resource-thresholds-policy",
        ClaimFamily::Performance,
        execution("latency and resource thresholds and policy for the maintained profile")?,
        SourceTier::Source,
        &[ProofClass::EvaluatedWork],
        OWNER_WORK_PERF,
    )?;
    row.work = WorkRequirement::PerformanceResource(WorkScope::new(
        "measured latency/resource results against the named thresholds",
    )?);
    rows.push(row);

    // Exact target/route/nonzero test/mutation work.
    let mut row = base_row(
        "intelligence.target-route-nonzero-test-mutation-work",
        ClaimFamily::TestReachability,
        cross_file(
            "exact target, route, and non-zero test/mutation work of the maintained profile",
        )?,
        SourceTier::Source,
        &[ProofClass::EvaluatedWork],
        OWNER_TOPOLOGY,
    )?;
    row.work = WorkRequirement::Production(WorkScope::new(
        "non-zero test and mutation work on the exact maintained targets and routes",
    )?);
    rows.push(row);

    // Selected legacy replacement.
    let mut row = base_row(
        "intelligence.selected-legacy-replacement",
        ClaimFamily::LegacyExit,
        cross_file("selected legacy replacement paths of the maintained profile")?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_SEMANTIC_DEBT,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.legacy_exit = LegacyExitRequirement {
        replacement_currentness: Obligation::Required,
        old_path_absence: Obligation::NotApplicable,
        recurrence_proof: Obligation::NotApplicable,
    };
    rows.push(row);

    // Old-path absence/work/recurrence proof.
    let mut row = base_row(
        "intelligence.old-path-absence-recurrence-proof",
        ClaimFamily::LegacyExit,
        cross_file(
            "old-path absence, absence work, and recurrence proof for selected legacy replacements",
        )?,
        SourceTier::Source,
        &[ProofClass::EvaluatedWork],
        OWNER_SEMANTIC_DEBT,
    )?;
    row.ceiling = ClaimCeiling::AcceptedCompatibility;
    row.legacy_exit = LegacyExitRequirement {
        replacement_currentness: Obligation::NotApplicable,
        old_path_absence: Obligation::Required,
        recurrence_proof: Obligation::Required,
    };
    rows.push(row);

    // Allowed limitations/expected failures.
    rows.push(base_row(
        "intelligence.allowed-limitations-expected-failures",
        ClaimFamily::PublicClaim,
        cross_file(
            "catalog of allowed limitations and expected failures of the maintained profile",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_PROFILE_TRAIN,
    )?);

    // Machine/public claim ceiling.
    let mut row = base_row(
        "intelligence.machine-public-claim-ceiling",
        ClaimFamily::PublicClaim,
        cross_file(
            "maximum claim wording of compiler_maintained_code_intelligence.v1: bounded public \
             maintained-code-intelligence claim over the selected rows; no support, release, \
             or publication authorization",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_PROFILE_TRAIN,
    )?;
    row.ceiling = ClaimCeiling::BoundedPublicClaim;
    rows.push(row);

    // support/release authority=false.
    rows.push(base_row(
        "intelligence.support-release-authority-false",
        ClaimFamily::PublicClaim,
        cross_file(
            "no row of this profile confers support, release, tag, or publication authority; \
             support/release authority is false",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_PROFILE_TRAIN,
    )?);

    // #8722 full integrated publication: a later, separately owned row source,
    // explicit as a closed unsupported state — never an omission and never a
    // prerequisite to the bounded rows above.
    let mut row = base_row(
        "intelligence.integrated-publication-8722",
        ClaimFamily::PublicClaim,
        cross_file(
            "full integrated selected publication owned by #8722 (later, separately owned)",
        )?,
        SourceTier::Source,
        &[ProofClass::CuratedExpectation],
        OWNER_INTEGRATED_PUBLICATION,
    )?;
    row.disposition = RowDisposition::unsupported(
        "#8722 full integrated publication is a later separately-owned row source; it is not a \
         current accepted row of this inventory and cannot fill a missing bounded subject or \
         current observation",
    )?;
    rows.push(row);

    Ok(rows)
}

/// Checked `compiler_maintained_code_intelligence.v1`: imports
/// `compiler_bounded_execution.v1` verbatim and adds the packaged, installed,
/// actual-client, work envelope, legacy-exit, and claim-ceiling rows.
pub fn compiler_maintained_code_intelligence_v1() -> Result<CompilerProfileDefinition> {
    let lower = compiler_bounded_execution_v1()?;
    let mut rows = lower.rows.clone();
    rows.extend(maintained_rows()?);
    let mut limitations = lower.limitations.clone();
    limitations.push(AllowedLimitation::new(
        "lim.maintained-selected-cells",
        "maintained code intelligence",
        "selected provider/refactor/client cells only; full integrated publication is \
         separately owned by #8722",
        OWNER_PROFILE_TRAIN,
    )?);
    Ok(CompilerProfileDefinition {
        id: CompilerProfileId::new(MAINTAINED_ID)?,
        version: CompilerProfileVersion::new(V1)?,
        purpose: "maintained code intelligence compiler operating profile over the exact \
                  lower-profile imports, release-shaped packaged process, installed client, \
                  and selected maintained rows"
            .to_owned(),
        change_reason: "initial checked row inventory for #12330".to_owned(),
        owner: owner(OWNER_PROFILE_TRAIN)?,
        imports: vec![CompilerProfileImport::for_profile(&lower)?],
        rows,
        limitations,
    })
}

/// The four checked initial profiles in import order
/// (local → project → execution → maintained).  This is the single authority
/// #12187 consumes without transcription; no second hand-maintained copy may
/// become authoritative.
pub fn initial_profiles() -> Result<[CompilerProfileDefinition; 4]> {
    Ok([
        compiler_local_lexical_v1()?,
        compiler_static_project_v1()?,
        compiler_bounded_execution_v1()?,
        compiler_maintained_code_intelligence_v1()?,
    ])
}

#[cfg(test)]
mod tests {
    use super::{
        EXECUTION_ID, LOCAL_ID, MAINTAINED_ID, PROJECT_ID, compiler_bounded_execution_v1,
        compiler_local_lexical_v1, compiler_maintained_code_intelligence_v1,
        compiler_static_project_v1, initial_profiles,
    };
    use crate::compiler_profile_contract::{
        ClaimCeiling, ClaimFamily, CompilerProfileDefinition, CompilerProfileImport,
        CompilerProfileRow, CompilerProfileVersion, CurrentnessRule, InvalidationKind, ProofClass,
        RowDisposition, SourceTier, WorkRequirement,
    };
    use anyhow::Result;
    use std::collections::BTreeSet;

    /// Own (non-imported) row IDs of `compiler_local_lexical.v1`, exactly the
    /// 22 propositions #12330 requires for the local lexical profile.
    const LOCAL_ROWS: [&str; 22] = [
        "lexical.candidate-toolchain-identity",
        "lexical.observation-base-parse",
        "lexical.observation-base-compile",
        "lexical.observation-comp-parse",
        "lexical.observation-comp-compile",
        "lexical.observation-run-parse",
        "lexical.observation-run-compile",
        "lexical.invocation-process-validity",
        "lexical.semantic-debt-retirement-accepted",
        "lexical.parser-generation-accepted",
        "lexical.semantic-snapshot-accepted",
        "lexical.pir-lexical-contribution",
        "lexical.external-references-compiler-backed",
        "lexical.occurrence-denominator-complete",
        "lexical.rename-authorization",
        "lexical.workspace-edit-application",
        "lexical.zero-request-time-compiler-construction",
        "lexical.zero-legacy-source-scan-work",
        "lexical.lifecycle-currentness-cleanup",
        "lexical.mutation-execution-required",
        "lexical.exact-perllsp-process",
        "lexical.claim-ceiling",
    ];

    /// Own row IDs added by `compiler_static_project.v1`.
    const PROJECT_ROWS: [&str; 19] = [
        "project.import-local-profile-exact",
        "project.world-snapshot-accepted",
        "project.root-project-profile-source-generation-closure",
        "project.module-dependency-graph",
        "project.scc-schedule",
        "project.private-implementation-transition",
        "project.public-interface-transition",
        "project.reverse-dependency-invalidation-closure",
        "project.stale-publication-rejection",
        "project.multi-root-close-reopen-currentness",
        "project.cross-file-definition",
        "project.cross-file-references",
        "project.cross-file-rename-complete-or-refuse",
        "project.cross-file-edit-application",
        "project.representative-project-lifecycle",
        "project.cold-equivalence-correctness",
        "project.reuse-recompute-work",
        "project.performance-resource-envelope",
        "project.claim-ceiling",
    ];

    /// Own row IDs added by `compiler_bounded_execution.v1`.
    const EXECUTION_ROWS: [&str; 19] = [
        "execution.executable-profile-identity",
        "execution.unsupported-fact-catalog-identity",
        "execution.package-subtable-effect-denominator",
        "execution.compiler-fact-eir-lowering",
        "execution.eir-verification",
        "execution.bounded-deterministic-evaluation",
        "execution.hard-limit-resource-policy",
        "execution.curated-gold-reviewed",
        "execution.hermetic-real-perl-oracle",
        "execution.eir-gold-agreement",
        "execution.eir-oracle-agreement",
        "execution.selected-upstream-row-denominator",
        "execution.nonzero-eir-work",
        "execution.nonzero-tap-work",
        "execution.zero-legacy-scaffold-calls",
        "execution.no-project-execution-from-editor-requests",
        "execution.editor-runtime-dependency-false",
        "execution.dynamic-boundaries-explicit",
        "execution.claim-ceiling",
    ];

    /// Own row IDs added by `compiler_maintained_code_intelligence.v1`.
    const MAINTAINED_ROWS: [&str; 19] = [
        "intelligence.import-lower-profile-identities-exact",
        "intelligence.upstream-series-denominator",
        "intelligence.selected-provider-refactor-rows",
        "intelligence.release-shaped-package-identity",
        "intelligence.contained-binary-process-identity",
        "intelligence.packaged-semantic-cells",
        "intelligence.manifest-selected-client-platform-identity",
        "intelligence.client-launches-exact-packaged-bytes",
        "intelligence.selected-client-application-cells",
        "intelligence.client-lifecycle-restart-currentness-cleanup",
        "intelligence.correctness-bound-work-envelope",
        "intelligence.latency-resource-thresholds-policy",
        "intelligence.target-route-nonzero-test-mutation-work",
        "intelligence.selected-legacy-replacement",
        "intelligence.old-path-absence-recurrence-proof",
        "intelligence.allowed-limitations-expected-failures",
        "intelligence.machine-public-claim-ceiling",
        "intelligence.support-release-authority-false",
        "intelligence.integrated-publication-8722",
    ];

    /// Canonical owner references from the #12330 owner map.  Every row owner
    /// must reference at least one of these; owner strings are navigation
    /// and ownership only, never evidence.
    const CANONICAL_OWNER_REFS: [&str; 29] = [
        "#12291", "#12139", "#12140", "#12141", "#8722", "#12117", "#12118", "#12119", "#12120",
        "#12165", "#11665", "#5214", "#12109", "#12191", "#2660", "#8669", "#12156", "#12157",
        "#12079", "#4772", "#2425", "#6232", "#4770", "#2447", "#4760", "#7422", "#6720", "#4346",
        "#9311",
    ];

    /// Pinned semantic digests of the four initial profiles.  Any semantic
    /// row change (proposition, subject, evidence, work law, limitation,
    /// legacy exit, ceiling, owner, invalidation) fails this gate and
    /// requires the row/profile version transition declared by #12186.
    const PINNED_DIGESTS: [(&str, &str); 4] = [
        (LOCAL_ID, "3436949225dfe7bdff85c480fd54eff0c1fb34abe52fd01fd430fccf2e2609a0"),
        (PROJECT_ID, "0c12f57c966ff3f29fe155dbd8d246a11b64b11d45a15eb251416d4d4da378f7"),
        (EXECUTION_ID, "f496c70a64de3ab653ab1c795f3e61e036fe20aa75ed3ed74865edfab53c3b7b"),
        (MAINTAINED_ID, "c6177f0766ba4d1d12c231b51a82a402ca5a8654f4f3d57c43a560cc2d7b0203"),
    ];

    fn row_ids(profile: &CompilerProfileDefinition) -> BTreeSet<&str> {
        profile.rows.iter().map(|row| row.id.as_str()).collect()
    }

    fn assert_row_id_set(
        profile: &CompilerProfileDefinition,
        expected: &BTreeSet<&str>,
        context: &str,
    ) {
        let actual = row_ids(profile);
        let missing: Vec<&str> = expected.difference(&actual).copied().collect();
        let unexpected: Vec<&str> = actual.difference(expected).copied().collect();
        assert!(
            missing.is_empty() && unexpected.is_empty(),
            "{context}: missing {missing:?}, unexpected {unexpected:?}"
        );
    }

    // -------------------------------------------------------------------
    // Acceptance: row ID inventory, validation, closure, and pinned identity
    // -------------------------------------------------------------------

    // Acceptance: every initial profile proposition has one stable explicit
    // row ID, and the full four-profile import chain validates and closes.
    #[test]
    fn initial_rows_match_the_required_inventory_and_close_the_import_chain() -> Result<()> {
        let local = compiler_local_lexical_v1()?;
        let project = compiler_static_project_v1()?;
        let execution = compiler_bounded_execution_v1()?;
        let maintained = compiler_maintained_code_intelligence_v1()?;

        assert_eq!(local.rows.len(), 22);
        assert_eq!(project.rows.len(), 41, "22 imported local rows + 19 own rows");
        assert_eq!(execution.rows.len(), 60, "41 imported rows + 19 own rows");
        assert_eq!(maintained.rows.len(), 79, "60 imported rows + 19 own rows");

        assert_row_id_set(&local, &LOCAL_ROWS.into_iter().collect(), "local lexical inventory");
        let project_expected: BTreeSet<&str> = LOCAL_ROWS.into_iter().chain(PROJECT_ROWS).collect();
        assert_row_id_set(&project, &project_expected, "static project inventory");
        let execution_expected: BTreeSet<&str> =
            project_expected.iter().copied().chain(EXECUTION_ROWS).collect();
        assert_row_id_set(&execution, &execution_expected, "bounded execution inventory");
        let maintained_expected: BTreeSet<&str> =
            execution_expected.iter().copied().chain(MAINTAINED_ROWS).collect();
        assert_row_id_set(&maintained, &maintained_expected, "maintained inventory");

        for profile in [&local, &project, &execution, &maintained] {
            profile.validate()?;
        }
        project.verify_import_closure(&local)?;
        execution.verify_import_closure(&project)?;
        maintained.verify_import_closure(&execution)?;
        Ok(())
    }

    // Acceptance: profile identity is deterministic and semantic; the pinned
    // digests fail any semantic drift and force version movement.
    #[test]
    fn initial_profile_digests_are_pinned() -> Result<()> {
        for profile in initial_profiles()? {
            eprintln!("PIN {} {}", profile.id.as_str(), profile.semantic_fingerprint()?.as_str());
        }
        for (profile, (expected_id, expected_digest)) in
            initial_profiles()?.iter().zip(PINNED_DIGESTS.iter())
        {
            assert_eq!(profile.id.as_str(), *expected_id);
            assert_eq!(profile.version.as_str(), "v1");
            assert_eq!(
                profile.semantic_fingerprint()?.as_str(),
                *expected_digest,
                "semantic drift in {expected_id}: a semantic row change requires a row/profile \
                 version transition (#12186), not a silent digest update"
            );
        }
        Ok(())
    }

    // Acceptance: every row names an exact subject, owner, evidence family,
    // currentness/completeness/work law, limitation/exit, and claim ceiling,
    // and every owner references the canonical #12330 owner map.
    #[test]
    fn every_row_carries_the_full_field_set_and_a_canonical_owner() -> Result<()> {
        for profile in initial_profiles()? {
            profile.validate()?;
            for row in &profile.rows {
                row.validate()?;
                assert!(
                    CANONICAL_OWNER_REFS
                        .iter()
                        .chain(["#12125", "#12126", "#12127", "#12128", "#12129", "#12176"].iter())
                        .any(|reference| row.owner.owner.contains(reference)),
                    "row {:?} owner {:?} is outside the canonical #12330 owner map",
                    row.id.as_str(),
                    row.owner.owner
                );
                assert!(!row.invalidation.is_empty());
                assert!(!row.owner.wake_event.is_empty());
            }
        }
        Ok(())
    }

    // Acceptance: the inventory feeds #12187 without a second hand-maintained
    // copy — the public constructors are the single authority and return
    // validated profiles.
    #[test]
    fn initial_profiles_are_the_single_authority_for_the_manifest() -> Result<()> {
        let profiles = initial_profiles()?;
        assert_eq!(profiles.len(), 4);
        let ids: Vec<&str> = profiles.iter().map(|profile| profile.id.as_str()).collect();
        assert_eq!(ids, [LOCAL_ID, PROJECT_ID, EXECUTION_ID, MAINTAINED_ID]);
        for profile in &profiles {
            profile.validate()?;
            profile.semantic_fingerprint()?;
        }
        Ok(())
    }

    // Acceptance: no live candidate evaluation, product behavior, support or
    // release action occurs — the production half of this module constructs
    // data only.
    #[test]
    fn inventory_performs_no_evaluation_or_product_behavior() {
        let source = include_str!("compiler_profile_initial_rows.rs");
        let production = match source.split("#[cfg(test)]").next() {
            Some(production) => production,
            None => unreachable!("module has a production half"),
        };
        for forbidden in ["std::process", "Command::", "std::net", "reqwest", "octocrab"] {
            assert!(
                !production.contains(forbidden),
                "the inventory must not perform live evaluation or product behavior ({forbidden})"
            );
        }
    }

    // -------------------------------------------------------------------
    // Issue #12330 falsifiers
    // -------------------------------------------------------------------

    // Falsifier 1: all local lexical propositions collapse into one #12079
    // pass row.
    #[test]
    fn falsifier_01_local_lexical_propositions_do_not_collapse() -> Result<()> {
        let local = compiler_local_lexical_v1()?;
        assert_eq!(local.rows.len(), 22, "the local lexical propositions must stay distinct");
        let distinct_subjects: BTreeSet<String> =
            local.rows.iter().map(|row| format!("{:?}", row.subject)).collect::<BTreeSet<_>>();
        assert_eq!(
            distinct_subjects.len(),
            local.rows.len(),
            "two rows collapsed onto one proposition"
        );
        // A single collapsed row cannot reproduce the inventory identity.
        let mut collapsed = local.clone();
        collapsed.rows.truncate(1);
        assert_ne!(collapsed.semantic_fingerprint()?, local.semantic_fingerprint()?);
        Ok(())
    }

    // Falsifier 2: #8722 becomes a prerequisite to the bounded local lexical
    // observation rows.
    #[test]
    fn falsifier_02_integrated_publication_is_not_a_local_prerequisite() -> Result<()> {
        let local = compiler_local_lexical_v1()?;
        assert!(local.imports.is_empty(), "the bounded local profile imports nothing");
        for row in &local.rows {
            assert!(
                !row.owner.owner.contains("#8722"),
                "local row {:?} must not depend on #8722",
                row.id.as_str()
            );
        }
        for row_id in [
            "lexical.observation-base-parse",
            "lexical.observation-base-compile",
            "lexical.observation-comp-parse",
            "lexical.observation-comp-compile",
            "lexical.observation-run-parse",
            "lexical.observation-run-compile",
        ] {
            assert!(
                local.rows.iter().any(|row| row.id.as_str() == row_id),
                "bounded observation row {row_id} must exist without #8722"
            );
        }
        Ok(())
    }

    // Falsifier 3: #8722 redefines the #12291 series/subject/denominator or
    // fills a missing bounded field.
    #[test]
    fn falsifier_03_integrated_publication_cannot_redefine_bounded_observations() -> Result<()> {
        let local = compiler_local_lexical_v1()?;
        for row in &local.rows {
            if row.id.as_str().starts_with("lexical.observation-") {
                assert!(
                    row.owner.owner.contains("#12291"),
                    "observation row {:?} must be owned by the #12291 bounded packet",
                    row.id.as_str()
                );
                let coverage = format!("{:?}", row.completeness.coverage);
                assert!(
                    coverage.contains("#12291"),
                    "observation row {:?} must name the #12291 bounded denominator",
                    row.id.as_str()
                );
                assert!(
                    coverage.contains("cannot redefine"),
                    "observation row {:?} must forbid #8722 redefinition",
                    row.id.as_str()
                );
            }
        }
        // The only place #8722 appears in the whole chain is its own
        // explicitly unsupported row in the maintained profile.
        let maintained = compiler_maintained_code_intelligence_v1()?;
        let references: Vec<&str> = maintained
            .rows
            .iter()
            .filter(|row| row.owner.owner.contains("#8722"))
            .map(|row| row.id.as_str())
            .collect();
        assert_eq!(references, ["intelligence.integrated-publication-8722"]);
        Ok(())
    }

    // Falsifier 4: compiler-world, navigation, and refactor collapse into one
    // static-project row.
    #[test]
    fn falsifier_04_world_navigation_and_refactor_stay_distinct() -> Result<()> {
        let project = compiler_static_project_v1()?;
        for row_id in [
            "project.module-dependency-graph",
            "project.cross-file-definition",
            "project.cross-file-references",
            "project.cross-file-rename-complete-or-refuse",
            "project.cross-file-edit-application",
        ] {
            assert!(
                project.rows.iter().any(|row| row.id.as_str() == row_id),
                "row {row_id} must remain a separate proposition"
            );
        }
        let families: Vec<ClaimFamily> = [
            "project.module-dependency-graph",
            "project.cross-file-definition",
            "project.cross-file-rename-complete-or-refuse",
        ]
        .iter()
        .map(|id| match project.rows.iter().find(|row| row.id.as_str() == *id) {
            Some(row) => row.evidence.family,
            None => unreachable!("row {id} exists"),
        })
        .collect();
        assert_eq!(
            families,
            [ClaimFamily::ProjectWorld, ClaimFamily::Provider, ClaimFamily::Edit],
            "world, navigation, and refactor are independent proposition families"
        );
        Ok(())
    }

    // Falsifier 5: gold/oracle/replay stands in for EIR mechanism or work.
    #[test]
    fn falsifier_05_gold_oracle_cannot_stand_in_for_eir_mechanism() -> Result<()> {
        let execution = compiler_bounded_execution_v1()?;
        let axes_of = |id: &str| -> BTreeSet<ProofClass> {
            match execution.rows.iter().find(|row| row.id.as_str() == id) {
                Some(row) => row.evidence.proof_axes.clone(),
                None => unreachable!("row {id} exists"),
            }
        };
        assert_eq!(
            axes_of("execution.curated-gold-reviewed"),
            BTreeSet::from([ProofClass::CuratedExpectation])
        );
        assert_eq!(
            axes_of("execution.hermetic-real-perl-oracle"),
            BTreeSet::from([ProofClass::RealPerlOracle])
        );
        // Agreement rows require the EIR mechanism axis conjunctively: gold
        // or oracle alone never satisfies them.
        assert_eq!(
            axes_of("execution.eir-gold-agreement"),
            BTreeSet::from([ProofClass::EirMechanism, ProofClass::CuratedExpectation])
        );
        assert_eq!(
            axes_of("execution.eir-oracle-agreement"),
            BTreeSet::from([ProofClass::EirMechanism, ProofClass::RealPerlOracle])
        );
        // Mechanism rows are separate propositions from agreement rows.
        assert_eq!(
            axes_of("execution.eir-verification"),
            BTreeSet::from([ProofClass::EirMechanism])
        );
        // Evaluated work is its own axis and its own rows.
        assert_eq!(
            axes_of("execution.nonzero-eir-work"),
            BTreeSet::from([ProofClass::EvaluatedWork])
        );
        assert_eq!(
            axes_of("execution.nonzero-tap-work"),
            BTreeSet::from([ProofClass::EvaluatedWork])
        );
        Ok(())
    }

    // Falsifier 6: package, install, and client stages collapse.
    #[test]
    fn falsifier_06_package_install_and_client_stages_stay_distinct() -> Result<()> {
        let maintained = compiler_maintained_code_intelligence_v1()?;
        let tier_of = |id: &str| -> SourceTier {
            match maintained.rows.iter().find(|row| row.id.as_str() == id) {
                Some(row) => row.evidence.source_tier,
                None => unreachable!("row {id} exists"),
            }
        };
        assert_eq!(tier_of("intelligence.release-shaped-package-identity"), SourceTier::Packaged);
        assert_eq!(tier_of("intelligence.contained-binary-process-identity"), SourceTier::Packaged);
        assert_eq!(
            tier_of("intelligence.manifest-selected-client-platform-identity"),
            SourceTier::InstalledHost
        );
        assert_eq!(
            tier_of("intelligence.client-launches-exact-packaged-bytes"),
            SourceTier::ActualClient
        );
        assert_eq!(
            tier_of("intelligence.selected-client-application-cells"),
            SourceTier::ActualClient
        );
        Ok(())
    }

    // Falsifier 7: performance or an aggregate score replaces correctness.
    #[test]
    fn falsifier_07_performance_cannot_replace_correctness() -> Result<()> {
        for profile in initial_profiles()? {
            profile.validate()?;
            let performance_rows: Vec<&CompilerProfileRow> = profile
                .rows
                .iter()
                .filter(|row| row.evidence.family == ClaimFamily::Performance)
                .collect();
            let correctness_rows: Vec<&CompilerProfileRow> = profile
                .rows
                .iter()
                .filter(|row| matches!(row.work, WorkRequirement::Correctness))
                .collect();
            assert!(!correctness_rows.is_empty(), "correctness rows must exist");
            for row in &performance_rows {
                assert!(
                    matches!(row.work, WorkRequirement::PerformanceResource(_)),
                    "performance row {:?} must be typed as a performance/resource result, \
                     never as correctness",
                    row.id.as_str()
                );
                assert_eq!(
                    row.ceiling,
                    ClaimCeiling::ObservedEvidence,
                    "performance evidence stays observed, never an accepted compatibility \
                     substitute"
                );
            }
        }
        Ok(())
    }

    // Falsifier 8: a failed/unsupported/not-proven denominator row is
    // omitted.
    #[test]
    fn falsifier_08_denominator_and_unsupported_rows_are_explicit() -> Result<()> {
        let maintained = compiler_maintained_code_intelligence_v1()?;
        for denominator in [
            "lexical.occurrence-denominator-complete",
            "execution.package-subtable-effect-denominator",
            "execution.selected-upstream-row-denominator",
            "intelligence.upstream-series-denominator",
        ] {
            assert!(
                maintained.rows.iter().any(|row| row.id.as_str() == denominator),
                "denominator row {denominator} must survive the import chain verbatim"
            );
        }
        let unsupported: Vec<&CompilerProfileRow> = maintained
            .rows
            .iter()
            .filter(|row| matches!(row.disposition, RowDisposition::Unsupported { .. }))
            .collect();
        assert_eq!(unsupported.len(), 1, "the #8722 row is the one explicit unsupported row");
        assert_eq!(unsupported[0].id.as_str(), "intelligence.integrated-publication-8722");
        assert!(
            !maintained
                .required_unconditional_row_ids()
                .contains("intelligence.integrated-publication-8722"),
            "an unsupported row is a closed typed state, never a silently required or omitted row"
        );
        Ok(())
    }

    // Falsifier 9: optional/required/limitation semantics change without
    // version movement.
    #[test]
    fn falsifier_09_disposition_semantics_require_version_movement() -> Result<()> {
        let local = compiler_local_lexical_v1()?;
        let expected = local.semantic_fingerprint()?;
        let mut weakened = local.clone();
        weakened.rows[0].disposition = RowDisposition::Optional;
        assert_eq!(weakened.version.as_str(), "v1");
        assert_ne!(
            weakened.semantic_fingerprint()?,
            expected,
            "weakening a disposition without a version change must fail the identity gate"
        );
        let mut limited = local.clone();
        limited.limitations.clear();
        assert_ne!(
            limited.semantic_fingerprint()?,
            expected,
            "dropping a limitation without a version change must fail the identity gate"
        );
        Ok(())
    }

    // Falsifier 10: a lower profile is imported by name only.
    #[test]
    fn falsifier_10_import_by_name_only_fails_closure() -> Result<()> {
        let project = compiler_static_project_v1()?;
        let local = compiler_local_lexical_v1()?;
        project.verify_import_closure(&local)?;

        let mut name_only = project.clone();
        let import = match name_only.imports.first_mut() {
            Some(import) => import,
            None => unreachable!("project imports the local profile"),
        };
        import.digest = compiler_bounded_execution_v1()?.semantic_fingerprint()?;
        assert!(
            name_only.verify_import_closure(&local).is_err(),
            "an import with a non-matching digest must fail closure"
        );

        let mut version_only = project.clone();
        let import = match version_only.imports.first_mut() {
            Some(import) => import,
            None => unreachable!("project imports the local profile"),
        };
        import.version = CompilerProfileVersion::new("v2")?;
        assert!(
            version_only.verify_import_closure(&local).is_err(),
            "an import with a stale version must fail closure"
        );
        Ok(())
    }

    // Falsifier 11: two different propositions get one stable row ID.
    #[test]
    fn falsifier_11_one_row_id_cannot_carry_two_propositions() -> Result<()> {
        let mut local = compiler_local_lexical_v1()?;
        let duplicate = match local.rows.first() {
            Some(row) => row.clone(),
            None => unreachable!("local profile has rows"),
        };
        local.rows.push(duplicate);
        let error = match local.validate() {
            Err(error) => error,
            Ok(()) => unreachable!("a duplicate row ID must fail validation"),
        };
        assert!(
            format!("{error:#}").contains("duplicate ids"),
            "duplicate row identity must be named: {error:#}"
        );
        // Across the chain every imported row ID keeps its proposition
        // verbatim: no higher row may reuse an imported ID with different
        // content (verify_import_closure enforces verbatim preservation).
        let maintained = compiler_maintained_code_intelligence_v1()?;
        let execution = compiler_bounded_execution_v1()?;
        maintained.verify_import_closure(&execution)?;
        Ok(())
    }

    // Falsifier 12: identity changes under harmless ordering/formatting.
    #[test]
    fn falsifier_12_ordering_cannot_change_inventory_identity() -> Result<()> {
        for profile in initial_profiles()? {
            let expected = profile.semantic_fingerprint()?;
            let mut reversed = profile.clone();
            reversed.rows.reverse();
            reversed.limitations.reverse();
            assert_eq!(
                reversed.semantic_fingerprint()?,
                expected,
                "row order must not change the identity of {:?}",
                profile.id.as_str()
            );
        }
        Ok(())
    }

    // Falsifier 13: issue/PR/workflow state is used as evidence.
    #[test]
    fn falsifier_13_workflow_state_is_never_evidence() -> Result<()> {
        for profile in initial_profiles()? {
            for row in &profile.rows {
                // Evidence is only ever the closed typed dimensions; owner
                // strings carry issue references as navigation, and the
                // falsifier is those references leaking into evidence.
                assert!(!row.evidence.proof_axes.is_empty());
                for axis in &row.evidence.proof_axes {
                    assert!(
                        !axis.tag().contains("issue")
                            && !axis.tag().contains("workflow")
                            && !axis.tag().contains("github"),
                        "proof axis {:?} must not encode workflow state",
                        axis.tag()
                    );
                }
                assert!(!row.evidence.family.tag().contains("issue"));
                assert!(!row.evidence.source_tier.tag().contains("issue"));
            }
        }
        Ok(())
    }

    // Falsifier 14: an overbroad claim ceiling is assigned to a narrow
    // subject.
    #[test]
    fn falsifier_14_claim_ceilings_match_subject_breadth() -> Result<()> {
        let profiles = initial_profiles()?;
        let strongest_required = |profile: &CompilerProfileDefinition| -> ClaimCeiling {
            match profile
                .rows
                .iter()
                .filter(|row| row.disposition.is_required())
                .map(|row| row.ceiling)
                .max_by_key(|ceiling| match ceiling {
                    ClaimCeiling::ObservedEvidence => 0,
                    ClaimCeiling::AcceptedCompatibility => 1,
                    ClaimCeiling::BoundedPublicClaim => 2,
                }) {
                Some(ceiling) => ceiling,
                None => unreachable!("every profile has required rows"),
            }
        };
        assert_eq!(strongest_required(&profiles[0]), ClaimCeiling::AcceptedCompatibility);
        assert_eq!(strongest_required(&profiles[1]), ClaimCeiling::AcceptedCompatibility);
        assert_eq!(strongest_required(&profiles[2]), ClaimCeiling::AcceptedCompatibility);
        assert_eq!(strongest_required(&profiles[3]), ClaimCeiling::BoundedPublicClaim);
        for profile in &profiles {
            for row in &profile.rows {
                if row.ceiling == ClaimCeiling::BoundedPublicClaim {
                    assert_eq!(
                        row.evidence.family,
                        ClaimFamily::PublicClaim,
                        "only the maintained public-claim ceiling row may carry a bounded \
                         public claim, not a narrow subject ({:?})",
                        row.id.as_str()
                    );
                    assert_eq!(row.id.as_str(), "intelligence.machine-public-claim-ceiling");
                }
            }
        }
        // The support/release=false row claims only observed evidence: it
        // cannot be turned into support authority by its own ceiling.
        let maintained = &profiles[3];
        let row = match maintained
            .rows
            .iter()
            .find(|row| row.id.as_str() == "intelligence.support-release-authority-false")
        {
            Some(row) => row,
            None => unreachable!("the support/release=false row exists"),
        };
        assert_eq!(row.ceiling, ClaimCeiling::ObservedEvidence);
        Ok(())
    }

    // Closure: lower-profile imports preserve every exact row and limitation
    // verbatim through the whole chain.
    #[test]
    fn imports_preserve_every_row_and_limitation_verbatim() -> Result<()> {
        let local = compiler_local_lexical_v1()?;
        let maintained = compiler_maintained_code_intelligence_v1()?;
        for imported in &local.rows {
            let own = maintained.rows.iter().find(|row| row.id.as_str() == imported.id.as_str());
            match own {
                Some(own) => assert_eq!(
                    own,
                    imported,
                    "imported row {:?} must be preserved verbatim",
                    imported.id.as_str()
                ),
                None => unreachable!("imported row {:?} must survive the chain", imported.id),
            }
        }
        for limitation in &local.limitations {
            assert!(
                maintained.limitations.iter().any(|own| own == limitation),
                "imported limitation {:?} must survive the chain",
                limitation.id
            );
        }
        // The import descriptor is exact identity/version/digest, never a
        // name-only reference.
        let import = CompilerProfileImport::for_profile(&local)?;
        let project = compiler_static_project_v1()?;
        assert!(project.imports.contains(&import));
        Ok(())
    }

    // Closure: currentness rules and invalidation inputs agree — a row that
    // is current while the world model or host is unchanged must also name
    // the input that re-opens it when that basis changes.
    #[test]
    fn currentness_rules_have_matching_invalidation_inputs() -> Result<()> {
        for profile in initial_profiles()? {
            for row in &profile.rows {
                let kinds: BTreeSet<InvalidationKind> =
                    row.invalidation.iter().map(|input| input.kind).collect();
                match row.completeness.currentness {
                    CurrentnessRule::ProjectWorldCurrent => assert!(
                        kinds.contains(&InvalidationKind::WorldModel),
                        "row {:?} is project-world current but names no world-model \
                         invalidation input",
                        row.id.as_str()
                    ),
                    CurrentnessRule::HostObserved => assert!(
                        kinds.contains(&InvalidationKind::HostEnvironment),
                        "row {:?} is host-observed but names no host-environment \
                         invalidation input",
                        row.id.as_str()
                    ),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    // Closure: required applicable rows are conjunctive across the inventory.
    #[test]
    fn required_rows_are_conjunctive_across_the_inventory() -> Result<()> {
        let maintained = compiler_maintained_code_intelligence_v1()?;
        let required = maintained.required_unconditional_row_ids();
        assert_eq!(required.len(), 78, "79 rows minus the one unsupported row");
        for row_id in LOCAL_ROWS
            .iter()
            .chain(PROJECT_ROWS.iter())
            .chain(EXECUTION_ROWS.iter())
            .chain(MAINTAINED_ROWS.iter())
            .filter(|id| **id != "intelligence.integrated-publication-8722")
        {
            assert!(required.contains(row_id), "required row {row_id} must be conjunctive");
        }
        Ok(())
    }
}
