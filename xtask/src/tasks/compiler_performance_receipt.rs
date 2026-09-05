//! Validate the transport-neutral compiler performance receipt contract.
//!
//! Three surfaces have to agree before this check passes, because each one
//! catches a class the others cannot (#9311):
//!
//! 1. the **schema document** carries the exact closed vocabularies — a later
//!    edit that quietly drops `not_proven` from an evidence enum is a contract
//!    change, not a formatting change;
//! 2. every **fixture** is evaluated against that schema with a real JSON
//!    Schema engine, so the conditional rules (counters only when measured,
//!    no contradictory stage states, no cache hit without validated
//!    currentness) are actually applied rather than merely written down;
//! 3. every fixture is then **deserialized into the typed model**, which runs
//!    [`validate_receipt`] inside decoding and so enforces the cross-field
//!    rules a JSON Schema cannot express — duplicate stage names, and
//!    reconciling the declared required-stage denominator against the rows
//!    actually present. Decoding is the enforcement point, not a step a
//!    consumer has to remember.
//!
//! The property the whole receipt exists to protect is that *missing
//! instrumentation is not zero*. It is enforced twice: the schema forbids a
//! counter whose paired status is not `measured`, and the typed model rejects
//! the same shape after deserialization, so an unobserved stage cannot reach a
//! consumer as a number that reads like fact.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::Path;

const SCHEMA_PATH: &str = "schemas/compiler_performance_receipt.v1.schema.json";
const SCHEMA_ID: &str =
    "https://effortlessmetrics.dev/perl-lsp/schemas/compiler_performance_receipt.v1.schema.json";
const VERSION: &str = "compiler_performance_receipt.v1";

/// Directory holding the committed receipt fixtures, relative to the repo root.
const FIXTURE_DIR: &str = "xtask/fixtures";

/// Filename prefix that marks a file in [`FIXTURE_DIR`] as a v1 receipt.
///
/// Fixtures are discovered rather than hand-registered: a constant list is a
/// hole, because a newly committed receipt would silently escape validation
/// unless its author also remembered to edit that list.
const FIXTURE_PREFIX: &str = "compiler_performance_receipt.v1";

const REQUIRED: &[&str] = &[
    "schema_version",
    "receipt_id",
    "subject",
    "workload",
    "cache",
    "stages",
    "provider",
    "limitations",
];
const STAGES: &[&str] = &[
    "upstream",
    "lex_parse",
    "compile_unit_extraction",
    "hir",
    "pir",
    "effects",
    "module_graph",
    "world",
    "interface_invalidation",
    "fact_projection",
    "provider_request",
    "serialization",
];
const EVIDENCE_STATUS: &[&str] =
    &["measured", "not_proven", "not_applicable", "failed", "cancelled"];
const APPLICABILITY: &[&str] = &["applicable", "not_applicable", "required_missing"];
const STAGE_RESULT: &[&str] = &["pass", "failed", "cancelled", "timeout", "not_proven"];
const INSTRUMENTATION: &[&str] = &["complete", "partial", "missing", "failed"];
const WORKLOAD_CLASS: &[&str] =
    &["focused_fixture", "selected_upstream", "representative_edit", "provider_vertical"];
const CACHE_MODE: &[&str] = &["cold", "warm", "reused", "bypassed", "unknown"];
const CACHE_OUTCOME: &[&str] = &["hit", "miss", "stale", "bypass", "not_proven"];
const CACHE_CURRENTNESS: &[&str] = &["validated", "unvalidated", "stale", "unknown"];

const WORK_COUNTERS: &[&str] = &["units", "objects", "bytes", "reused", "recomputed"];
const CORRECTNESS_COUNTERS: &[&str] =
    &["false_exact", "stale_exact", "unsafe_edit", "unexplained_empty"];
const PROVIDER_COUNTERS: &[&str] = &["requests", "exact", "partial", "fallback", "refusal"];
const CACHE_COUNTERS: &[&str] = &["loaded", "copied", "reused", "recomputed"];

// ---------------------------------------------------------------------------
// Closed vocabularies
// ---------------------------------------------------------------------------

/// Whether the paired observation was actually taken.
///
/// Modelled as a closed enum rather than a `String` so an unknown state is a
/// deserialization failure at the typed boundary, not a value that flows on to
/// a consumer which has no arm for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Measured,
    NotProven,
    NotApplicable,
    Failed,
    Cancelled,
}

impl EvidenceStatus {
    fn is_measured(self) -> bool {
        matches!(self, Self::Measured)
    }
}

/// The twelve compiler stages named by the governing contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StageName {
    Upstream,
    LexParse,
    CompileUnitExtraction,
    Hir,
    Pir,
    Effects,
    ModuleGraph,
    World,
    InterfaceInvalidation,
    FactProjection,
    ProviderRequest,
    Serialization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    Applicable,
    NotApplicable,
    RequiredMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StageResult {
    Pass,
    Failed,
    Cancelled,
    Timeout,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Instrumentation {
    Complete,
    Partial,
    Missing,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadClass {
    FocusedFixture,
    SelectedUpstream,
    RepresentativeEdit,
    ProviderVertical,
}

/// How the run was configured. A setup label, never evidence of an outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    Cold,
    Warm,
    Reused,
    Bypassed,
    Unknown,
}

/// The validated cache result, distinct from [`CacheMode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheOutcome {
    Hit,
    Miss,
    Stale,
    Bypass,
    NotProven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheCurrentness {
    Validated,
    Unvalidated,
    Stale,
    Unknown,
}

// ---------------------------------------------------------------------------
// Validated scalars
// ---------------------------------------------------------------------------

/// A string the schema constrains with `minLength: 1`.
///
/// Modelled as a newtype so the emptiness rule is enforced where the value is
/// decoded, not only where a JSON Schema consumer happens to look.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequiredText(String);

impl RequiredText {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for RequiredText {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        // Whitespace-only is not a value: a receipt_id of " " identifies nothing,
        // and `minLength: 1` alone would accept it.
        if raw.trim().is_empty() {
            return Err(serde::de::Error::custom("must not be empty or whitespace-only"));
        }
        Ok(Self(raw))
    }
}

/// A git object name: exactly forty lowercase hex digits.
///
/// `subject.candidate` and `subject.tree` are both load-bearing identity —
/// consumers key caches, receipt dedup, and attribution on them — so `"HEAD"`,
/// a branch name, a short SHA, or an upper-case spelling must fail to decode
/// rather than reach a cache key or attribute a measurement to nothing exact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GitObjectName(String);

impl GitObjectName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for GitObjectName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let well_formed = raw.len() == 40
            && raw.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !well_formed {
            return Err(serde::de::Error::custom(format!(
                "must be 40 lowercase hex digits, got {raw:?}"
            )));
        }
        Ok(Self(raw))
    }
}

/// Generate the closed `schema` vocabulary for one identity slot.
///
/// Each slot accepts exactly its own v1 constant, so a receipt claiming
/// `source.legacy` is rejected at the typed boundary and not only by a JSON
/// Schema consumer that may never run.
macro_rules! pinned_schema {
    ($name:ident, $value:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
        pub enum $name {
            #[serde(rename = $value)]
            V1,
        }
    };
}

pinned_schema!(SourceSchema, "source.v1");
pinned_schema!(ParserSchema, "parser.v1");
pinned_schema!(HirSchema, "hir.v1");
pinned_schema!(PirSchema, "pir.v1");
pinned_schema!(EffectsSchema, "effects.v1");
pinned_schema!(WorldSchema, "world.v1");
pinned_schema!(InterfacesSchema, "interfaces.v1");

// ---------------------------------------------------------------------------
// Typed receipt
// ---------------------------------------------------------------------------

/// A receipt that has passed every rule in [`validate_receipt`].
///
/// Deserialization is fail-closed: the cross-field rules run *inside* decoding
/// via `try_from`, so `serde_json::from_str::<CompilerPerformanceReceipt>` can
/// only produce a receipt that already satisfies them. A consumer cannot hold
/// an invalid value of this type by forgetting a second call — which it
/// otherwise would, since nothing in the type system compels one.
#[derive(Debug, Deserialize, Serialize)]
#[serde(try_from = "ReceiptFields")]
pub struct CompilerPerformanceReceipt {
    pub schema_version: SchemaVersion,
    pub receipt_id: RequiredText,
    pub subject: Subject,
    pub workload: Workload,
    pub cache: Cache,
    pub stages: Vec<Stage>,
    pub provider: Provider,
    pub limitations: Vec<RequiredText>,
}

/// The wire shape, before the cross-field rules are applied.
///
/// Private on purpose: it exists only so `try_from` has something to decode
/// into, and holding one is exactly the unchecked state the public type is
/// meant to make unrepresentable.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptFields {
    schema_version: SchemaVersion,
    receipt_id: RequiredText,
    subject: Subject,
    workload: Workload,
    cache: Cache,
    stages: Vec<Stage>,
    provider: Provider,
    limitations: Vec<RequiredText>,
}

impl TryFrom<ReceiptFields> for CompilerPerformanceReceipt {
    type Error = String;

    fn try_from(raw: ReceiptFields) -> std::result::Result<Self, Self::Error> {
        let receipt = Self {
            schema_version: raw.schema_version,
            receipt_id: raw.receipt_id,
            subject: raw.subject,
            workload: raw.workload,
            cache: raw.cache,
            stages: raw.stages,
            provider: raw.provider,
            limitations: raw.limitations,
        };
        validate_receipt(&receipt).map_err(|error| format!("{error:#}"))?;
        Ok(receipt)
    }
}

/// The receipt version constant. An unknown version fails to decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum SchemaVersion {
    #[serde(rename = "compiler_performance_receipt.v1")]
    V1,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub repository: RequiredText,
    pub candidate: GitObjectName,
    pub tree: GitObjectName,
    pub dirty_tree: bool,
    pub toolchain: RequiredText,
    pub runner: RequiredText,
    pub identities: Identities,
}

/// The seven identity slots, as a fixed struct rather than an open map: a
/// missing family is a decode failure and an extra family is rejected by
/// `deny_unknown_fields`.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Identities {
    pub source: Identity<SourceSchema>,
    pub parser: Identity<ParserSchema>,
    pub hir: Identity<HirSchema>,
    pub pir: Identity<PirSchema>,
    pub effects: Identity<EffectsSchema>,
    pub world: Identity<WorldSchema>,
    pub interfaces: Identity<InterfacesSchema>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Identity<S> {
    pub schema: S,
    pub profile: RequiredText,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workload {
    pub id: RequiredText,
    pub class: WorkloadClass,
    pub profile: RequiredText,
    pub fixture: RequiredText,
    pub series: RequiredText,
    pub cache_mode: CacheMode,
    pub required_stages: Vec<StageName>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cache {
    pub status: EvidenceStatus,
    pub outcome: CacheOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<RequiredText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currentness: Option<CacheCurrentness>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copied: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recomputed: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub name: StageName,
    pub applicability: Applicability,
    pub result: StageResult,
    pub work: Work,
    pub timing: Timing,
    pub instrumentation: Instrumentation,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub status: EvidenceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recomputed: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    pub status: EvidenceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_ns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_ns: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub status: EvidenceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<u64>,
    pub correctness: Correctness,
    pub timing: Timing,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Correctness {
    pub status: EvidenceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub false_exact: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_exact: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsafe_edit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unexplained_empty: Option<u64>,
}

// ---------------------------------------------------------------------------
// Semantic validation
// ---------------------------------------------------------------------------

/// Counters exist exactly where their paired status is `measured`.
///
/// Both directions matter. Missing counters under `measured` would publish a
/// measurement with nothing measured; present counters under any other status
/// are the zero-from-missing failure this receipt exists to prevent.
fn check_counters(
    label: &str,
    status: EvidenceStatus,
    counters: &[(&str, bool)],
    errors: &mut Vec<String>,
) {
    for (name, present) in counters {
        match (status.is_measured(), present) {
            (true, false) => errors.push(format!("{label}: measured evidence must carry {name}")),
            (false, true) => errors.push(format!(
                "{label}: {name} is present but the evidence status is {status:?}; missing instrumentation is not zero"
            )),
            _ => {}
        }
    }
}

fn check_work(label: &str, work: &Work, errors: &mut Vec<String>) {
    check_counters(
        label,
        work.status,
        &[
            ("units", work.units.is_some()),
            ("objects", work.objects.is_some()),
            ("bytes", work.bytes.is_some()),
            ("reused", work.reused.is_some()),
            ("recomputed", work.recomputed.is_some()),
        ],
        errors,
    );
}

fn check_timing(label: &str, timing: &Timing, errors: &mut Vec<String>) {
    if timing.status.is_measured() {
        if timing.wall_ns.is_none() {
            errors.push(format!("{label}: measured timing must carry wall_ns"));
        }
    } else {
        if timing.wall_ns.is_some() {
            errors.push(format!("{label}: wall_ns is present but timing is not measured"));
        }
        if timing.cpu_ns.is_some() {
            errors.push(format!("{label}: cpu_ns is present but timing is not measured"));
        }
    }
}

/// Cross-field rules a JSON Schema cannot express, plus a second enforcement
/// surface for the ones it can.
///
/// The schema and this function are deliberately redundant on the counter
/// rules: a receipt reaches a Rust consumer through `serde`, which never
/// consults the schema, so a rule enforced only in JSON would not protect the
/// typed path at all.
pub fn validate_receipt(receipt: &CompilerPerformanceReceipt) -> Result<()> {
    let mut errors = Vec::new();

    // Collection invariants the schema also states. They are repeated here
    // because a Rust consumer decodes through `serde`, which never consults the
    // schema: a receipt with no stage rows, or no declared denominator, proves
    // nothing and must not reach a consumer as a valid receipt.
    if receipt.stages.is_empty() {
        errors.push("stages: a receipt with no stage rows proves nothing".to_owned());
    }
    if receipt.workload.required_stages.is_empty() {
        errors.push(
            "workload.required_stages: the denominator this receipt is judged against cannot be empty"
                .to_owned(),
        );
    }
    let mut declared: HashSet<StageName> = HashSet::new();
    for required in &receipt.workload.required_stages {
        if !declared.insert(*required) {
            errors.push(format!("workload.required_stages: {required:?} is declared twice"));
        }
    }

    // Stage identity: a name may appear once. `uniqueItems` cannot catch this,
    // because two rows sharing a name but differing in any counter are
    // distinct JSON objects.
    let mut seen: HashSet<StageName> = HashSet::new();
    for stage in &receipt.stages {
        if !seen.insert(stage.name) {
            errors.push(format!("stages: duplicate row for {:?}", stage.name));
        }
    }

    // The declared denominator is reconciled against the rows actually
    // present, so a producer cannot make a required stage vanish by simply
    // omitting it.
    for required in &receipt.workload.required_stages {
        if !seen.contains(required) {
            errors.push(format!(
                "workload.required_stages names {required:?}, but no stage row reports it — an absent required stage must be carried as a required_missing row"
            ));
        }
    }

    for stage in &receipt.stages {
        let label = format!("stages[{:?}]", stage.name);
        check_work(&format!("{label}.work"), &stage.work, &mut errors);
        check_timing(&format!("{label}.timing"), &stage.timing, &mut errors);

        // A row that is not applicable, or required but unobserved, cannot
        // also present as successful measured evidence.
        if stage.applicability != Applicability::Applicable {
            if stage.result != StageResult::NotProven {
                errors.push(format!(
                    "{label}: applicability {:?} cannot carry result {:?}",
                    stage.applicability, stage.result
                ));
            }
            if stage.work.status.is_measured() {
                errors.push(format!("{label}: a non-applicable stage cannot report measured work"));
            }
            if stage.timing.status.is_measured() {
                errors
                    .push(format!("{label}: a non-applicable stage cannot report measured timing"));
            }
            if stage.instrumentation == Instrumentation::Complete {
                errors.push(format!(
                    "{label}: a non-applicable stage cannot report complete instrumentation"
                ));
            }
        }

        if stage.applicability == Applicability::RequiredMissing
            && !receipt.workload.required_stages.contains(&stage.name)
        {
            errors.push(format!(
                "{label}: marked required_missing but is absent from workload.required_stages"
            ));
        }

        if stage.instrumentation == Instrumentation::Complete
            && !(stage.work.status.is_measured() && stage.timing.status.is_measured())
        {
            errors.push(format!(
                "{label}: complete instrumentation requires measured work and measured timing"
            ));
        }

        // The mirror rule. Instrumentation that is missing cannot have produced
        // a measurement, so a row claiming both is describing two different
        // runs. `result` is deliberately *not* constrained this way: whether a
        // stage succeeded is observable without counting its work, so `pass`
        // with no measurement is honest as long as instrumentation says so.
        if stage.instrumentation == Instrumentation::Missing
            && (stage.work.status.is_measured() || stage.timing.status.is_measured())
        {
            errors.push(format!(
                "{label}: missing instrumentation cannot report measured work or timing"
            ));
        }
    }

    // Cache presence is not a hit.
    check_counters(
        "cache",
        receipt.cache.status,
        &[
            ("loaded", receipt.cache.loaded.is_some()),
            ("copied", receipt.cache.copied.is_some()),
            ("reused", receipt.cache.reused.is_some()),
            ("recomputed", receipt.cache.recomputed.is_some()),
        ],
        &mut errors,
    );
    if receipt.cache.outcome == CacheOutcome::Hit {
        if !receipt.cache.status.is_measured() {
            errors.push("cache: a hit requires measured evidence".to_owned());
        }
        if receipt.cache.key.is_none() {
            errors.push("cache: a hit requires a cache key".to_owned());
        }
        if receipt.cache.currentness != Some(CacheCurrentness::Validated) {
            errors.push(
                "cache: a hit requires validated currentness — cache presence is not a hit"
                    .to_owned(),
            );
        }
    } else if receipt.cache.reused.is_some_and(|reused| reused > 0) {
        // A loaded or copied artifact is not automatically work avoided: only a
        // validated hit may claim reuse.
        errors.push(format!(
            "cache: outcome {:?} cannot claim reused work; loaded and copied artefacts are counted separately",
            receipt.cache.outcome
        ));
    }
    if !receipt.cache.status.is_measured() && receipt.cache.outcome != CacheOutcome::NotProven {
        errors.push(format!(
            "cache: outcome {:?} claims more than the evidence status {:?} supports",
            receipt.cache.outcome, receipt.cache.status
        ));
    }

    // Provider utility, and the correctness evidence a latency claim may not omit.
    check_counters(
        "provider",
        receipt.provider.status,
        &[
            ("requests", receipt.provider.requests.is_some()),
            ("exact", receipt.provider.exact.is_some()),
            ("partial", receipt.provider.partial.is_some()),
            ("fallback", receipt.provider.fallback.is_some()),
            ("refusal", receipt.provider.refusal.is_some()),
        ],
        &mut errors,
    );
    check_timing("provider.timing", &receipt.provider.timing, &mut errors);
    check_counters(
        "provider.correctness",
        receipt.provider.correctness.status,
        &[
            ("false_exact", receipt.provider.correctness.false_exact.is_some()),
            ("stale_exact", receipt.provider.correctness.stale_exact.is_some()),
            ("unsafe_edit", receipt.provider.correctness.unsafe_edit.is_some()),
            ("unexplained_empty", receipt.provider.correctness.unexplained_empty.is_some()),
        ],
        &mut errors,
    );
    if receipt.provider.timing.status.is_measured()
        && !receipt.provider.correctness.status.is_measured()
    {
        errors.push(
            "provider: a measured latency claim cannot omit measured correctness counters"
                .to_owned(),
        );
    }

    if errors.is_empty() {
        return Ok(());
    }
    bail!("compiler performance receipt violations: {}", errors.join("; "));
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run() -> Result<()> {
    let root = project_root()?;
    let stats = validate(&root)?;
    println!(
        "compiler performance receipt check passed: {} required fields, {} stages, {} fixtures conform to the schema and the typed contract",
        stats.required_fields, stats.stages, stats.fixtures
    );
    Ok(())
}

pub struct CheckStats {
    pub required_fields: usize,
    pub stages: usize,
    pub fixtures: usize,
}

fn validate(root: &Path) -> Result<CheckStats> {
    let schema = load_schema(root)?;
    validate_schema_document(&schema)?;

    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| color_eyre::eyre::eyre!("{SCHEMA_PATH}: invalid schema: {error}"))?;

    let fixtures = discover_fixtures(root)?;
    if fixtures.is_empty() {
        bail!(
            "{FIXTURE_DIR} holds no {FIXTURE_PREFIX}* fixture; the contract would be checked against nothing"
        );
    }
    for path in &fixtures {
        let label = path
            .file_name()
            .map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let receipt = validate_fixture(&validator, &label, &text)?;
        // Name what was actually checked, so the command's output is evidence
        // rather than an unfalsifiable "passed".
        println!(
            "  {label}: receipt {} over tree {} conforms",
            receipt.receipt_id.as_str(),
            receipt.subject.tree.as_str()
        );
    }

    Ok(CheckStats {
        required_fields: REQUIRED.len(),
        stages: STAGES.len(),
        fixtures: fixtures.len(),
    })
}

/// Every committed v1 receipt fixture, in a stable order.
///
/// Discovery rather than a hand-maintained list: a fixture that nobody
/// registered is a fixture nobody validates.
fn discover_fixtures(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let dir = root.join(FIXTURE_DIR);
    let mut found = Vec::new();
    collect_fixtures(&dir, &mut found)?;
    found.sort();
    Ok(found)
}

/// Walk one directory tree, collecting v1 receipt fixtures.
///
/// Recursive for the same reason discovery replaced a constant list: scanning
/// only direct children would leave a fixture one directory deeper silently
/// unchecked, which is the hole the list had.
fn collect_fixtures(dir: &Path, found: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let path = entry.with_context(|| format!("failed to read {}", dir.display()))?.path();
        if path.is_dir() {
            collect_fixtures(&path, found)?;
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with(FIXTURE_PREFIX) && name.ends_with(".json") {
            found.push(path);
        }
    }
    Ok(())
}

fn load_schema(root: &Path) -> Result<Value> {
    let path = root.join(SCHEMA_PATH);
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

/// Run one receipt through the schema, then the typed model, then the
/// cross-field rules — the three surfaces the module doc describes.
fn validate_fixture(
    validator: &jsonschema::Validator,
    label: &str,
    text: &str,
) -> Result<CompilerPerformanceReceipt> {
    let value: Value = serde_json::from_str(text)
        .with_context(|| format!("{label}: fixture is not valid JSON"))?;

    let violations: Vec<String> =
        validator.iter_errors(&value).map(|error| error.to_string()).collect();
    if !violations.is_empty() {
        bail!("{label}: schema violations: {}", violations.join("; "));
    }

    // Decoding applies the typed vocabulary *and* the cross-field rules: the
    // public receipt type can only be constructed through `validate_receipt`.
    let receipt: CompilerPerformanceReceipt = serde_json::from_value(value)
        .with_context(|| format!("{label}: fixture does not satisfy the typed contract"))?;
    Ok(receipt)
}

/// Pin every proof-sensitive vocabulary in the schema document.
///
/// Without this, an edit that removed `not_proven` from an evidence enum, or a
/// stage from the stage list, would still leave the check green while silently
/// narrowing the contract the receipt advertises.
fn validate_schema_document(schema: &Value) -> Result<()> {
    let mut errors = Vec::new();
    require_string(schema, &["$id"], SCHEMA_ID, &mut errors);
    require_string(schema, &["properties", "schema_version", "const"], VERSION, &mut errors);
    require_set(schema, &["required"], REQUIRED, &mut errors);
    // The counter bound keeps the two surfaces from disagreeing: without it the
    // schema would accept an integer the typed model cannot hold.
    if lookup(schema, &["$defs", "count", "maximum"]).and_then(Value::as_u64) != Some(u64::MAX) {
        errors.push("$defs.count.maximum must bound counters to u64::MAX".to_owned());
    }
    // Both exact identities share one pattern; losing it would let a receipt
    // attribute measurements to a branch name or a symbolic ref.
    if lookup(schema, &["$defs", "git_object_name", "pattern"]).and_then(Value::as_str)
        != Some("^[0-9a-f]{40}$")
    {
        errors.push("$defs.git_object_name.pattern must pin exact 40-hex object names".to_owned());
    }

    for (path, expected) in [
        (&["$defs", "stage_name", "enum"][..], STAGES),
        (&["$defs", "evidence_status", "enum"][..], EVIDENCE_STATUS),
        (&["$defs", "stage", "properties", "applicability", "enum"][..], APPLICABILITY),
        (&["$defs", "stage", "properties", "result", "enum"][..], STAGE_RESULT),
        (&["$defs", "stage", "properties", "instrumentation", "enum"][..], INSTRUMENTATION),
        (&["$defs", "workload", "properties", "class", "enum"][..], WORKLOAD_CLASS),
        (&["$defs", "workload", "properties", "cache_mode", "enum"][..], CACHE_MODE),
        (&["$defs", "cache", "properties", "outcome", "enum"][..], CACHE_OUTCOME),
        (&["$defs", "cache", "properties", "currentness", "enum"][..], CACHE_CURRENTNESS),
    ] {
        require_set(schema, path, expected, &mut errors);
    }

    require_set(
        schema,
        &["$defs", "provider", "required"],
        &["status", "correctness", "timing"],
        &mut errors,
    );

    // The measured-only counter rule is where the contract actually lives, and
    // it has two halves. Pinning only `then` would let the `else` prohibition be
    // deleted silently: the committed fixtures would still pass, because they
    // never carry a counter under a non-measured status, while the schema had
    // quietly started permitting exactly the numbers this receipt exists to
    // forbid. Both halves are checked for every definition that carries one.
    for (def, expected) in [
        (&["$defs", "work"][..], WORK_COUNTERS),
        (&["$defs", "correctness"][..], CORRECTNESS_COUNTERS),
        (&["$defs", "provider"][..], PROVIDER_COUNTERS),
        (&["$defs", "cache"][..], CACHE_COUNTERS),
    ] {
        require_measured_only_rule(schema, def, expected, &mut errors);
    }

    if !errors.is_empty() {
        bail!("compiler performance receipt schema violations: {}", errors.join("; "));
    }
    Ok(())
}

fn lookup<'a>(mut value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    for part in path {
        value = value.get(*part)?;
    }
    Some(value)
}

fn require_string(schema: &Value, path: &[&str], expected: &str, errors: &mut Vec<String>) {
    if lookup(schema, path).and_then(Value::as_str) != Some(expected) {
        errors.push(format!("{} must be {expected:?}", path.join(".")));
    }
}

fn require_set(schema: &Value, path: &[&str], expected: &[&str], errors: &mut Vec<String>) {
    let Some(values) = lookup(schema, path).and_then(Value::as_array) else {
        errors.push(format!("{} must be a string array", path.join(".")));
        return;
    };
    // A JSON Schema enum may hold any type. Silently dropping non-strings would
    // let `["measured", 7]` pass an exact-set check that is supposed to pin the
    // vocabulary, so a non-string member is reported rather than skipped.
    for value in values.iter().filter(|value| value.as_str().is_none()) {
        errors.push(format!("{} contains a non-string member {value}", path.join(".")));
    }
    let actual = values.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>();
    let wanted = expected.iter().copied().collect::<BTreeSet<_>>();
    for missing in wanted.difference(&actual) {
        errors.push(format!("{} missing {missing:?}", path.join(".")));
    }
    for extra in actual.difference(&wanted) {
        errors.push(format!("{} contains unsupported {extra:?}", path.join(".")));
    }
}

/// Assert that a definition carries a complete measured-only counter rule:
/// some conditional branch requires exactly `expected` when the status is
/// `measured`, **and** that same branch forbids every one of them otherwise.
///
/// A definition may spell the rule directly (`if`/`then`/`else` on the def) or
/// as one entry in `allOf` when it carries more than one conditional; both
/// shapes are accepted, and the two halves must live on the same branch.
fn require_measured_only_rule(
    schema: &Value,
    def: &[&str],
    expected: &[&str],
    errors: &mut Vec<String>,
) {
    let label = def.join(".");
    let Some(node) = lookup(schema, def) else {
        errors.push(format!("{label} is missing"));
        return;
    };

    let mut branches: Vec<&Value> = vec![node];
    if let Some(all_of) = node.get("allOf").and_then(Value::as_array) {
        branches.extend(all_of.iter());
    }

    let wanted = expected.iter().copied().collect::<BTreeSet<_>>();
    let matching = branches.iter().find(|branch| {
        lookup(branch, &["then", "required"]).and_then(Value::as_array).is_some_and(|values| {
            values.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>() == wanted
        })
    });

    let Some(branch) = matching else {
        errors.push(format!("{label} must require exactly {expected:?} when measured"));
        return;
    };

    // The `if` has to actually select `measured`, or the two halves are pinned
    // to the wrong condition.
    if lookup(branch, &["if", "properties", "status", "const"]).and_then(Value::as_str)
        != Some("measured")
    {
        errors.push(format!("{label} must condition its counter rule on status == \"measured\""));
    }

    match forbidden_counters(branch) {
        Some(forbidden) if forbidden == wanted => {}
        Some(forbidden) => errors.push(format!(
            "{label} must forbid exactly {expected:?} when not measured, found {forbidden:?}"
        )),
        None => errors.push(format!(
            "{label} must forbid every counter when not measured; the else prohibition is missing or malformed"
        )),
    }
}

/// The counter names an `else` branch forbids, via `not.anyOf[{required:[name]}]`.
fn forbidden_counters(branch: &Value) -> Option<BTreeSet<&str>> {
    let clauses = lookup(branch, &["else", "not", "anyOf"])?.as_array()?;
    let mut names = BTreeSet::new();
    for clause in clauses {
        let required = clause.get("required")?.as_array()?;
        let [only] = required.as_slice() else {
            return None;
        };
        names.insert(only.as_str()?);
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MEASURED: &str = include_str!("../../fixtures/compiler_performance_receipt.v1.json");
    const UNINSTRUMENTED: &str =
        include_str!("../../fixtures/compiler_performance_receipt.v1.uninstrumented.json");

    fn schema_value() -> Value {
        serde_json::from_str(include_str!(
            "../../../schemas/compiler_performance_receipt.v1.schema.json"
        ))
        .expect("the committed schema must be valid JSON")
    }

    fn validator() -> jsonschema::Validator {
        jsonschema::validator_for(&schema_value()).expect("the committed schema must compile")
    }

    /// Run one candidate receipt through the same three surfaces `run()` uses.
    fn check(value: &Value) -> Result<()> {
        validate_fixture(&validator(), "candidate", &value.to_string()).map(|_| ())
    }

    /// Evaluate a candidate against the JSON Schema **only**.
    ///
    /// The schema is the transport-neutral surface: a consumer in another
    /// language has nothing else. A rule that only `validate_receipt` enforces
    /// is not part of that contract, so rules the schema is supposed to carry
    /// get a control that cannot be satisfied by the Rust validator.
    fn schema_violations(value: &Value) -> Vec<String> {
        validator().iter_errors(value).map(|error| error.to_string()).collect()
    }

    /// Decode a candidate through the typed boundary *only* and return the
    /// rejection message.
    ///
    /// Deserialization is the enforcement point, so a control for a typed rule
    /// asserts that decoding fails — no separate `validate_receipt` call, which
    /// is exactly the call a real consumer would forget.
    fn decode_error(value: Value) -> String {
        match serde_json::from_value::<CompilerPerformanceReceipt>(value) {
            Ok(_) => panic!("the typed boundary accepted a receipt it must reject"),
            Err(error) => error.to_string(),
        }
    }

    fn measured() -> Value {
        serde_json::from_str(MEASURED).expect("fixture must be valid JSON")
    }

    fn uninstrumented() -> Value {
        serde_json::from_str(UNINSTRUMENTED).expect("fixture must be valid JSON")
    }

    /// Discovery must reach a fixture that is not a direct child.
    ///
    /// Review asked whether the convention forbids nested receipts. It does not
    /// say either way, so the safe reading is that a nested fixture must be
    /// validated rather than silently skipped.
    #[test]
    fn discovery_reaches_nested_fixtures_and_ignores_unrelated_files() {
        let root = tempfile::tempdir().expect("temp dir");
        let fixtures = root.path().join(FIXTURE_DIR);
        let nested = fixtures.join("nested");
        fs::create_dir_all(&nested).expect("create nested fixture dir");
        let top = fixtures.join(format!("{FIXTURE_PREFIX}.json"));
        let deep = nested.join(format!("{FIXTURE_PREFIX}.deep.json"));
        fs::write(&top, MEASURED).expect("write top-level fixture");
        fs::write(&deep, MEASURED).expect("write nested fixture");
        fs::write(fixtures.join("unrelated.json"), "{}").expect("write unrelated file");

        let found = discover_fixtures(root.path()).expect("discovery must succeed");
        assert_eq!(found, vec![top, deep], "nested fixtures are discovered; others are not");
    }

    // -- the committed artifacts ------------------------------------------------

    #[test]
    fn schema_document_pins_every_vocabulary() {
        validate_schema_document(&schema_value()).expect("the committed schema must self-check");
    }

    #[test]
    fn both_fixtures_conform_to_schema_typed_model_and_semantics() {
        check(&measured()).expect("the measured fixture must conform");
        check(&uninstrumented()).expect("the uninstrumented fixture must conform");
    }

    /// The uninstrumented fixture is the one that carries the contract's
    /// headline claim, so its shape is asserted rather than assumed.
    #[test]
    fn uninstrumented_fixture_publishes_no_counters_at_all() {
        let receipt: CompilerPerformanceReceipt =
            serde_json::from_str(UNINSTRUMENTED).expect("must decode");
        let lex_parse = receipt
            .stages
            .iter()
            .find(|stage| stage.name == StageName::LexParse)
            .expect("fixture declares lex_parse");
        assert_eq!(lex_parse.work.status, EvidenceStatus::NotProven);
        assert_eq!(lex_parse.work.units, None, "an unobserved stage must publish no unit count");
        assert_eq!(lex_parse.work.bytes, None);
        assert_eq!(receipt.provider.correctness.false_exact, None);
        assert_eq!(receipt.cache.outcome, CacheOutcome::NotProven);
        assert_eq!(receipt.receipt_id.as_str(), "fixture-compiler-performance-uninstrumented-001");
        assert_eq!(receipt.subject.tree.as_str(), "f530d2f1221c516484a3654f01a3ad694ab56969");
        assert!(
            receipt.stages.iter().any(|stage| {
                stage.applicability == Applicability::RequiredMissing
                    && stage.name == StageName::ModuleGraph
            }),
            "a required but unobserved stage must stay explicit"
        );
    }

    // -- missing instrumentation is not zero ------------------------------------

    /// The single most important control: an unobserved stage may not report
    /// zero work. Without this the receipt's whole reason to exist is unproven.
    #[test]
    fn rejects_zero_counters_under_unproven_work() {
        let mut receipt = uninstrumented();
        receipt["stages"][0]["work"] = json!({
            "status": "not_proven",
            "units": 0, "objects": 0, "bytes": 0, "reused": 0, "recomputed": 0
        });
        assert!(check(&receipt).is_err(), "not_proven work must not carry numeric zeros");
    }

    #[test]
    fn rejects_measured_work_without_counters() {
        let mut receipt = uninstrumented();
        receipt["stages"][0]["work"] = json!({"status": "measured"});
        assert!(check(&receipt).is_err(), "measured work must carry its counters");
    }

    #[test]
    fn rejects_measured_timing_without_a_value() {
        let mut receipt = measured();
        receipt["stages"][1]["timing"] = json!({"status": "measured"});
        assert!(check(&receipt).is_err(), "measured timing must carry wall_ns");
    }

    #[test]
    fn rejects_zero_correctness_counters_under_unproven_provider() {
        let mut receipt = uninstrumented();
        receipt["provider"]["correctness"] = json!({
            "status": "not_proven",
            "false_exact": 0, "stale_exact": 0, "unsafe_edit": 0, "unexplained_empty": 0
        });
        assert!(
            check(&receipt).is_err(),
            "uninstrumented correctness must not publish four zeros that read as 'no failures'"
        );
    }

    #[test]
    fn rejects_measured_provider_latency_without_correctness_evidence() {
        let mut receipt = measured();
        receipt["provider"]["correctness"] = json!({"status": "not_proven"});
        assert!(
            check(&receipt).is_err(),
            "a measured latency claim may not omit correctness counters"
        );
    }

    /// The typed path must reject the same shape on its own.
    ///
    /// A Rust consumer reaches a receipt through `serde`, which never consults
    /// the JSON Schema. If this rule lived only in the schema, every typed
    /// consumer would still read an unobserved stage as zero work, so the
    /// control deliberately skips the schema and exercises deserialization
    /// plus [`validate_receipt`] alone.
    #[test]
    fn typed_model_alone_rejects_zero_counters_under_unproven_work() {
        let mut value = uninstrumented();
        value["stages"][0]["work"] = json!({
            "status": "not_proven",
            "units": 0, "objects": 0, "bytes": 0, "reused": 0, "recomputed": 0
        });
        let rendered = decode_error(value);
        assert!(
            rendered.contains("missing instrumentation is not zero"),
            "the failure must name the rule it enforces, got {rendered}"
        );
    }

    /// The typed boundary independently refuses an unvalidated cache hit.
    ///
    /// Mutation testing showed this rule was invisible to the suite while every
    /// control went through the schema first: disabling the typed check alone
    /// left all tests green. A rule with no failing test is not enforced, so
    /// this control skips the schema deliberately.
    #[test]
    fn typed_model_alone_rejects_a_cache_hit_without_validated_currentness() {
        let mut value = measured();
        value["cache"]["currentness"] = json!("unvalidated");
        assert!(
            decode_error(value).contains("cache presence is not a hit"),
            "the failure must name the rule it enforces"
        );
    }

    /// Same reasoning for the provider latency/correctness pairing.
    #[test]
    fn typed_model_alone_rejects_measured_latency_without_correctness() {
        let mut value = measured();
        value["provider"]["correctness"] = json!({"status": "not_proven"});
        assert!(
            decode_error(value).contains("measured latency claim cannot omit measured correctness"),
            "the failure must name the rule it enforces"
        );
    }

    /// The typed boundary is also the second surface for the identity pin.
    #[test]
    fn typed_model_alone_rejects_an_unpinned_identity() {
        let mut value = measured();
        value["subject"]["identities"]["source"]["schema"] = json!("source.legacy");
        let decoded: std::result::Result<CompilerPerformanceReceipt, _> =
            serde_json::from_value(value);
        assert!(decoded.is_err(), "source.legacy must not decode at the typed boundary");
    }

    // -- contradictory stage states ---------------------------------------------

    #[test]
    fn rejects_not_applicable_stage_reporting_pass() {
        let mut receipt = measured();
        receipt["stages"][0]["result"] = json!("pass");
        assert!(check(&receipt).is_err(), "a not_applicable stage cannot pass");
    }

    #[test]
    fn rejects_required_missing_stage_reporting_measured_work() {
        let mut receipt = uninstrumented();
        receipt["stages"][2]["work"] = json!({"status": "measured", "units": 1, "objects": 1, "bytes": 1, "reused": 0, "recomputed": 1});
        assert!(check(&receipt).is_err(), "a required_missing stage cannot report measured work");
    }

    #[test]
    fn rejects_complete_instrumentation_over_unproven_work() {
        let mut receipt = uninstrumented();
        receipt["stages"][0]["instrumentation"] = json!("complete");
        assert!(
            check(&receipt).is_err(),
            "complete instrumentation requires measured work and timing"
        );
    }

    // -- required-stage denominator and stage identity ---------------------------

    #[test]
    fn rejects_a_required_stage_with_no_row() {
        let mut receipt = uninstrumented();
        // Drop the required_missing row while leaving module_graph in the
        // declared denominator: the absence must stay visible.
        let stages = receipt["stages"].as_array().map(|rows| rows[..2].to_vec());
        receipt["stages"] = json!(stages.expect("fixture stages are an array"));
        assert!(check(&receipt).is_err(), "an omitted required stage must be rejected");
    }

    #[test]
    fn rejects_duplicate_stage_rows() {
        let mut receipt = uninstrumented();
        let duplicate = receipt["stages"][0].clone();
        let mut rows = receipt["stages"].as_array().cloned().unwrap_or_default();
        rows.push(duplicate);
        receipt["stages"] = json!(rows);
        assert!(check(&receipt).is_err(), "two rows for one stage cannot both be authoritative");
    }

    #[test]
    fn rejects_required_missing_row_outside_the_denominator() {
        let mut receipt = uninstrumented();
        receipt["workload"]["required_stages"] = json!(["lex_parse", "hir"]);
        assert!(
            check(&receipt).is_err(),
            "a required_missing row must name a stage the denominator requires"
        );
    }

    #[test]
    fn rejects_an_empty_stage_list() {
        let mut receipt = uninstrumented();
        receipt["stages"] = json!([]);
        assert!(check(&receipt).is_err(), "a receipt with no stage rows proves nothing");
    }

    // -- cache presence is not a hit ---------------------------------------------

    #[test]
    fn rejects_cache_hit_without_validated_currentness() {
        let mut receipt = measured();
        receipt["cache"]["currentness"] = json!("unvalidated");
        assert!(check(&receipt).is_err(), "cache presence is not a hit");
    }

    #[test]
    fn rejects_cache_hit_without_a_key() {
        let mut receipt = measured();
        receipt["cache"]
            .as_object_mut()
            .map(|cache| cache.remove("key"))
            .expect("cache is an object");
        assert!(check(&receipt).is_err(), "a hit must identify the artefact it matched");
    }

    #[test]
    fn rejects_unproven_cache_claiming_an_outcome() {
        let mut receipt = uninstrumented();
        receipt["cache"]["outcome"] = json!("hit");
        assert!(check(&receipt).is_err(), "an unmeasured cache observation cannot claim a hit");
    }

    /// A copied or loaded artifact is not automatically work avoided.
    #[test]
    fn rejects_reused_work_claimed_without_a_hit() {
        let mut receipt = measured();
        receipt["cache"]["outcome"] = json!("miss");
        receipt["cache"]["currentness"] = json!("unvalidated");
        assert!(
            check(&receipt).is_err(),
            "only a validated hit may claim reuse; loading and copying are counted separately"
        );
    }

    /// The schema, not only the Rust validator, must refuse reuse on a miss.
    ///
    /// Review found this enforced on the typed path alone, which made the
    /// transport-neutral claim false: a non-Rust consumer validating against
    /// the committed schema would have accepted avoided-work on a documented
    /// cache miss.
    #[test]
    fn schema_alone_rejects_reused_work_on_a_cache_miss() {
        let mut receipt = measured();
        receipt["cache"]["outcome"] = json!("miss");
        receipt["cache"]["currentness"] = json!("unvalidated");
        assert!(
            !schema_violations(&receipt).is_empty(),
            "the schema itself must reject positive reuse without a hit"
        );
        assert!(check(&receipt).is_err(), "and so must the full check");
    }

    /// Zero reuse on a miss is honest and must stay valid, so the rule above
    /// cannot have been implemented by forbidding the field outright.
    #[test]
    fn a_measured_cache_miss_with_no_reuse_remains_valid() {
        let mut receipt = measured();
        receipt["cache"]["outcome"] = json!("miss");
        receipt["cache"]["currentness"] = json!("unvalidated");
        receipt["cache"]["reused"] = json!(0);
        assert_eq!(
            schema_violations(&receipt),
            Vec::<String>::new(),
            "a miss that claims no reuse is an honest observation"
        );
        check(&receipt).expect("a measured miss with zero reuse must be accepted");
    }

    // -- instrumentation cannot contradict evidence --------------------------------

    #[test]
    fn rejects_missing_instrumentation_reporting_measured_evidence() {
        let mut receipt = uninstrumented();
        receipt["stages"][0]["work"] = json!({
            "status": "measured", "units": 1, "objects": 1, "bytes": 1, "reused": 0, "recomputed": 1
        });
        receipt["stages"][0]["timing"] = json!({"status": "measured", "wall_ns": 1});
        assert!(
            !schema_violations(&receipt).is_empty(),
            "the schema must reject measured evidence under missing instrumentation"
        );
        assert!(check(&receipt).is_err());
    }

    #[test]
    fn typed_model_alone_rejects_missing_instrumentation_over_measured_evidence() {
        let mut value = uninstrumented();
        value["stages"][0]["work"] = json!({
            "status": "measured", "units": 1, "objects": 1, "bytes": 1, "reused": 0, "recomputed": 1
        });
        value["stages"][0]["timing"] = json!({"status": "measured", "wall_ns": 1});
        assert!(
            decode_error(value).contains("missing instrumentation cannot report measured"),
            "the failure must name the rule it enforces"
        );
    }

    /// `result` is deliberately independent of measurement: a stage can be
    /// known to have succeeded without its work being counted. This pins that
    /// decision so a later reader does not mistake it for an oversight.
    #[test]
    fn an_uninstrumented_stage_may_still_report_pass() {
        let mut receipt = uninstrumented();
        receipt["stages"][0]["result"] = json!("pass");
        check(&receipt).expect("success and measurement are separate dimensions");
    }

    // -- typed collection invariants -------------------------------------------------

    /// Mutation testing caught this one passing for the wrong reason: with an
    /// empty `stages` list, the required-stage reconciliation fires first, so
    /// disabling the empty-list rule left the test green. It now asserts the
    /// specific message, which is the only thing that makes it a control for
    /// *this* rule.
    #[test]
    fn typed_model_alone_rejects_an_empty_stage_list() {
        let mut value = uninstrumented();
        value["stages"] = json!([]);
        value["workload"]["required_stages"] = json!(["lex_parse"]);
        assert!(
            decode_error(value).contains("no stage rows proves nothing"),
            "the empty-list rule itself must fire, not only the denominator reconciliation"
        );
    }

    #[test]
    fn typed_model_alone_rejects_an_empty_required_stage_denominator() {
        let mut value = uninstrumented();
        value["workload"]["required_stages"] = json!([]);
        assert!(decode_error(value).contains("cannot be empty"), "the denominator cannot be empty");
    }

    #[test]
    fn typed_model_alone_rejects_a_duplicated_required_stage() {
        let mut value = uninstrumented();
        value["workload"]["required_stages"] = json!(["lex_parse", "hir", "module_graph", "hir"]);
        assert!(
            decode_error(value).contains("declared twice"),
            "a denominator cannot count a stage twice"
        );
    }

    // -- load-bearing identity ----------------------------------------------------

    #[test]
    fn rejects_an_unpinned_artifact_schema_identity() {
        let mut receipt = measured();
        receipt["subject"]["identities"]["source"]["schema"] = json!("source.legacy");
        assert!(check(&receipt).is_err(), "an identity slot accepts only its own v1 constant");
    }

    /// The schema must carry the identity pin on its own.
    ///
    /// `rejects_an_unpinned_artifact_schema_identity` goes through the typed
    /// model too, whose one-variant enum rejects `source.legacy` regardless of
    /// the schema — so deleting the schema-side `const` left every test green.
    /// A transport-neutral consumer has only the schema, so the pin needs a
    /// control that sees only the schema.
    #[test]
    fn schema_alone_rejects_an_unpinned_artifact_schema_identity() {
        let mut receipt = measured();
        receipt["subject"]["identities"]["source"]["schema"] = json!("source.legacy");
        assert!(
            !schema_violations(&receipt).is_empty(),
            "the schema itself must pin each identity slot to its v1 constant"
        );
    }

    /// Same reasoning for the timing rule: `check_timing` on the typed path
    /// masked a deleted schema conditional.
    #[test]
    fn schema_alone_rejects_measured_timing_without_a_value() {
        let mut receipt = measured();
        receipt["stages"][1]["timing"] = json!({"status": "measured"});
        assert!(
            !schema_violations(&receipt).is_empty(),
            "the schema itself must require wall_ns under measured timing"
        );

        let mut smuggled = measured();
        smuggled["stages"][1]["timing"] = json!({"status": "not_proven", "wall_ns": 0});
        assert!(
            !schema_violations(&smuggled).is_empty(),
            "and must forbid a value under a non-measured timing"
        );
    }

    #[test]
    fn rejects_a_tree_that_is_not_an_object_name() {
        for spelling in ["HEAD", "1234", "F530D2F1221C516484A3654F01A3AD694AB56969"] {
            let mut receipt = measured();
            receipt["subject"]["tree"] = json!(spelling);
            assert!(check(&receipt).is_err(), "{spelling:?} must not decode as a tree identity");
        }
    }

    /// `subject.candidate` is load-bearing identity too, not free text.
    ///
    /// Review found it typed as `RequiredText`, so `HEAD`, a branch name, or
    /// arbitrary text decoded fine and a receipt could attribute its
    /// measurements to nothing exact. It is now the same pinned git object name
    /// as `tree`, on both surfaces.
    #[test]
    fn rejects_a_candidate_that_is_not_an_object_name() {
        for spelling in ["HEAD", "main", "a05f482", "A05F4820AB9385B261A2993EA9687CF9D7BEEDFC"] {
            let mut receipt = measured();
            receipt["subject"]["candidate"] = json!(spelling);
            assert!(
                !schema_violations(&receipt).is_empty(),
                "{spelling:?}: the schema must reject an inexact candidate identity"
            );
            assert!(check(&receipt).is_err(), "{spelling:?} must not decode either");
        }
    }

    /// Both exact identities need a control that cannot be satisfied by the
    /// schema.
    ///
    /// Mutation testing caught this: reverting `candidate` to free text failed
    /// no test, because the control above reaches the rule through the schema's
    /// pattern first. The typed boundary needs its own falsifier, exactly as
    /// the cache and counter rules did.
    #[test]
    fn typed_model_alone_rejects_inexact_git_identities() {
        for field in ["candidate", "tree"] {
            for spelling in
                ["HEAD", "main", "a05f482", "A05F4820AB9385B261A2993EA9687CF9D7BEEDFC", ""]
            {
                let mut value = measured();
                value["subject"][field] = json!(spelling);
                let rendered = decode_error(value);
                assert!(
                    rendered.contains("40 lowercase hex digits"),
                    "{field} = {spelling:?} must fail to decode as an object name, got {rendered}"
                );
            }
        }
    }

    #[test]
    fn rejects_an_unpinned_git_object_pattern() {
        let mut schema = schema_value();
        schema["$defs"]["git_object_name"]["pattern"] = json!("^.+$");
        assert!(
            validate_schema_document(&schema).is_err(),
            "relaxing the object-name pattern is a contract change"
        );
    }

    #[test]
    fn rejects_a_missing_identity_family() {
        let mut receipt = measured();
        receipt["subject"]["identities"]
            .as_object_mut()
            .map(|identities| identities.remove("world"))
            .expect("identities is an object");
        assert!(check(&receipt).is_err(), "all seven identity families are load-bearing");
    }

    #[test]
    fn rejects_an_unknown_schema_version() {
        let mut receipt = measured();
        receipt["schema_version"] = json!("compiler_performance_receipt.v2");
        assert!(check(&receipt).is_err(), "an unknown receipt version must be rejected");
    }

    #[test]
    fn rejects_an_empty_required_string() {
        let mut receipt = measured();
        receipt["receipt_id"] = json!("");
        assert!(check(&receipt).is_err(), "minLength: 1 is part of the contract");
    }

    #[test]
    fn rejects_a_whitespace_only_required_string() {
        let mut receipt = measured();
        receipt["receipt_id"] = json!("   ");
        assert!(
            !schema_violations(&receipt).is_empty(),
            "whitespace-only is not a value the schema accepts"
        );
        assert!(check(&receipt).is_err(), "nor one the typed boundary accepts");
    }

    // -- determinism ---------------------------------------------------------------

    /// Key order in the source document must not change the decoded receipt,
    /// and re-serializing must be byte-stable.
    #[test]
    fn serialization_is_deterministic_and_order_independent() {
        let receipt: CompilerPerformanceReceipt =
            serde_json::from_str(MEASURED).expect("fixture must decode");
        let first = serde_json::to_string(&receipt).expect("must serialize");
        let second = serde_json::to_string(&receipt).expect("must serialize");
        assert_eq!(first, second, "serialization must be byte-stable");

        // `serde_json::Value`'s map is ordered by insertion, so round-tripping
        // through a re-parsed document gives a differently ordered source with
        // identical meaning.
        let reordered: Value = serde_json::from_str(&first).expect("must reparse");
        let round_tripped: CompilerPerformanceReceipt =
            serde_json::from_value(reordered).expect("must decode");
        assert_eq!(
            serde_json::to_string(&round_tripped).expect("must serialize"),
            first,
            "decoding must not depend on key order"
        );
    }

    // -- the schema document itself --------------------------------------------------

    #[test]
    fn rejects_a_stage_dropped_from_the_vocabulary() {
        let mut schema = schema_value();
        schema["$defs"]["stage_name"]["enum"] = json!(["upstream", "lex_parse"]);
        assert!(validate_schema_document(&schema).is_err());
    }

    #[test]
    fn rejects_not_proven_dropped_from_the_evidence_vocabulary() {
        let mut schema = schema_value();
        schema["$defs"]["evidence_status"]["enum"] =
            json!(["measured", "not_applicable", "failed", "cancelled"]);
        assert!(
            validate_schema_document(&schema).is_err(),
            "silently narrowing the evidence vocabulary is a contract change"
        );
    }

    /// Review found the pin guarding only the `then` half: deleting an `else`
    /// prohibition left the check green, and both committed fixtures still
    /// passed, because neither carries a counter under a non-measured status.
    /// The schema would have started permitting exactly the numbers this
    /// receipt exists to forbid, silently.
    #[test]
    fn rejects_a_deleted_counter_prohibition() {
        for def in ["work", "correctness"] {
            let mut schema = schema_value();
            schema["$defs"][def]
                .as_object_mut()
                .map(|node| node.remove("else"))
                .expect("the definition is an object");
            assert!(
                validate_schema_document(&schema).is_err(),
                "{def}: deleting the else prohibition must fail the pin"
            );
        }
    }

    #[test]
    fn rejects_a_weakened_counter_prohibition() {
        let mut schema = schema_value();
        // Forbid only one of the five counters when not measured.
        schema["$defs"]["work"]["else"] = json!({"not": {"anyOf": [{"required": ["units"]}]}});
        assert!(
            validate_schema_document(&schema).is_err(),
            "forbidding a subset is not the measured-only rule"
        );
    }

    #[test]
    fn rejects_a_counter_prohibition_deleted_from_a_multi_conditional_definition() {
        for def in ["provider", "cache"] {
            let mut schema = schema_value();
            let branches = schema["$defs"][def]["allOf"].as_array().cloned().unwrap_or_default();
            let stripped: Vec<Value> = branches
                .into_iter()
                .map(|mut branch| {
                    if let Some(object) = branch.as_object_mut() {
                        if object.contains_key("then") && object.contains_key("else") {
                            object.remove("else");
                        }
                    }
                    branch
                })
                .collect();
            schema["$defs"][def]["allOf"] = json!(stripped);
            assert!(
                validate_schema_document(&schema).is_err(),
                "{def}: the else prohibition must be pinned inside allOf too"
            );
        }
    }

    /// The rule must be tied to `measured`, not to whatever the `if` happens to
    /// name, or both halves could be pinned to the wrong condition.
    #[test]
    fn rejects_a_counter_rule_conditioned_on_the_wrong_status() {
        let mut schema = schema_value();
        schema["$defs"]["work"]["if"]["properties"]["status"]["const"] = json!("not_proven");
        assert!(validate_schema_document(&schema).is_err());
    }

    #[test]
    fn rejects_a_relaxed_measured_only_counter_rule() {
        let mut schema = schema_value();
        schema["$defs"]["work"]["then"]["required"] = json!(["units"]);
        assert!(
            validate_schema_document(&schema).is_err(),
            "the measured-only counter rule is pinned"
        );
    }

    /// A JSON Schema enum may hold any type, so an exact-set check that skipped
    /// non-strings could be widened without the pin noticing.
    #[test]
    fn rejects_a_non_string_member_smuggled_into_a_vocabulary() {
        let mut schema = schema_value();
        schema["$defs"]["evidence_status"]["enum"] =
            json!(["measured", "not_proven", "not_applicable", "failed", "cancelled", 7]);
        assert!(validate_schema_document(&schema).is_err());
    }

    #[test]
    fn rejects_an_unbounded_counter_type() {
        let mut schema = schema_value();
        schema["$defs"]["count"]
            .as_object_mut()
            .map(|count| count.remove("maximum"))
            .expect("count is an object");
        assert!(
            validate_schema_document(&schema).is_err(),
            "an unbounded counter lets the schema accept what the typed model cannot hold"
        );
    }

    #[test]
    fn rejects_a_dropped_cache_outcome_vocabulary() {
        let mut schema = schema_value();
        schema["$defs"]["cache"]["properties"]["outcome"]["enum"] = json!(["hit", "miss"]);
        assert!(validate_schema_document(&schema).is_err());
    }
}
