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
//! 3. every fixture is then **deserialized into the typed model** and run
//!    through [`validate_receipt`], which enforces the cross-field rules a
//!    JSON Schema cannot express — duplicate stage names, and reconciling the
//!    declared required-stage denominator against the rows actually present.
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

/// Fixtures validated by `run()`, relative to the xtask crate directory.
///
/// The uninstrumented fixture is not decoration: it is the only receipt that
/// exercises the contract's headline claim, so the check would stop proving
/// "missing is not zero" if it were dropped.
const FIXTURES: &[&str] = &[
    "fixtures/compiler_performance_receipt.v1.json",
    "fixtures/compiler_performance_receipt.v1.uninstrumented.json",
];

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
        if raw.is_empty() {
            return Err(serde::de::Error::custom("must not be empty"));
        }
        Ok(Self(raw))
    }
}

/// A git object name: exactly forty lowercase hex digits.
///
/// `subject.tree` is load-bearing identity — consumers key caches and receipt
/// dedup on it — so `"HEAD"`, a short SHA, or an upper-case spelling must fail
/// to decode rather than reach a cache key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeSha(String);

impl TreeSha {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for TreeSha {
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
    pub candidate: RequiredText,
    pub tree: TreeSha,
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

    for fixture in FIXTURES {
        let path = root.join("xtask").join(fixture);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let receipt = validate_fixture(&validator, fixture, &text)?;
        // Name what was actually checked, so the command's output is evidence
        // rather than an unfalsifiable "passed".
        println!(
            "  {fixture}: receipt {} over tree {} conforms",
            receipt.receipt_id.as_str(),
            receipt.subject.tree.as_str()
        );
    }

    Ok(CheckStats {
        required_fields: REQUIRED.len(),
        stages: STAGES.len(),
        fixtures: FIXTURES.len(),
    })
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

    let receipt: CompilerPerformanceReceipt = serde_json::from_value(value)
        .with_context(|| format!("{label}: fixture does not satisfy the typed contract"))?;
    validate_receipt(&receipt).with_context(|| format!("{label}: semantic violation"))?;
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

    // The conditional halves carry the "not zero" rule, so they are pinned by
    // the same exact-set check as the vocabularies.
    for (path, expected) in [
        (&["$defs", "work", "then", "required"][..], WORK_COUNTERS),
        (&["$defs", "correctness", "then", "required"][..], CORRECTNESS_COUNTERS),
    ] {
        require_set(schema, path, expected, &mut errors);
    }
    require_set(
        schema,
        &["$defs", "provider", "required"],
        &["status", "correctness", "timing"],
        &mut errors,
    );
    require_conditional_required(schema, &["$defs", "provider"], PROVIDER_COUNTERS, &mut errors);
    require_conditional_required(schema, &["$defs", "cache"], CACHE_COUNTERS, &mut errors);

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
    let actual = values.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>();
    let wanted = expected.iter().copied().collect::<BTreeSet<_>>();
    for missing in wanted.difference(&actual) {
        errors.push(format!("{} missing {missing:?}", path.join(".")));
    }
    for extra in actual.difference(&wanted) {
        errors.push(format!("{} contains unsupported {extra:?}", path.join(".")));
    }
}

/// Assert that one `allOf` branch of `def` requires exactly `expected` under
/// its `then`, which is how the measured-only counter rule is spelled for the
/// definitions that carry more than one conditional.
fn require_conditional_required(
    schema: &Value,
    def: &[&str],
    expected: &[&str],
    errors: &mut Vec<String>,
) {
    let label = def.join(".");
    let Some(branches) =
        lookup(schema, def).and_then(|value| value.get("allOf")).and_then(Value::as_array)
    else {
        errors.push(format!("{label}.allOf must be an array of conditionals"));
        return;
    };
    let wanted = expected.iter().copied().collect::<BTreeSet<_>>();
    let found = branches.iter().any(|branch| {
        lookup(branch, &["then", "required"]).and_then(Value::as_array).is_some_and(|values| {
            values.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>() == wanted
        })
    });
    if !found {
        errors.push(format!("{label}.allOf must require exactly {expected:?} when measured"));
    }
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

    fn measured() -> Value {
        serde_json::from_str(MEASURED).expect("fixture must be valid JSON")
    }

    fn uninstrumented() -> Value {
        serde_json::from_str(UNINSTRUMENTED).expect("fixture must be valid JSON")
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
        let receipt: CompilerPerformanceReceipt =
            serde_json::from_value(value).expect("the shape still deserializes");
        let rejected = validate_receipt(&receipt);
        assert!(rejected.is_err(), "the typed boundary must reject zero-from-missing");
        let rendered = format!("{:?}", rejected.err());
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
        let receipt: CompilerPerformanceReceipt =
            serde_json::from_value(value).expect("the shape still deserializes");
        let rejected = validate_receipt(&receipt);
        assert!(rejected.is_err(), "cache presence is not a hit at the typed boundary either");
        assert!(
            format!("{:?}", rejected.err()).contains("cache presence is not a hit"),
            "the failure must name the rule it enforces"
        );
    }

    /// Same reasoning for the provider latency/correctness pairing.
    #[test]
    fn typed_model_alone_rejects_measured_latency_without_correctness() {
        let mut value = measured();
        value["provider"]["correctness"] = json!({"status": "not_proven"});
        let receipt: CompilerPerformanceReceipt =
            serde_json::from_value(value).expect("the shape still deserializes");
        let rejected = validate_receipt(&receipt);
        assert!(rejected.is_err(), "a measured latency claim needs measured correctness");
        assert!(
            format!("{:?}", rejected.err())
                .contains("measured latency claim cannot omit measured correctness"),
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

    // -- load-bearing identity ----------------------------------------------------

    #[test]
    fn rejects_an_unpinned_artifact_schema_identity() {
        let mut receipt = measured();
        receipt["subject"]["identities"]["source"]["schema"] = json!("source.legacy");
        assert!(check(&receipt).is_err(), "an identity slot accepts only its own v1 constant");
    }

    #[test]
    fn rejects_a_tree_that_is_not_an_object_name() {
        for spelling in ["HEAD", "1234", "F530D2F1221C516484A3654F01A3AD694AB56969"] {
            let mut receipt = measured();
            receipt["subject"]["tree"] = json!(spelling);
            assert!(check(&receipt).is_err(), "{spelling:?} must not decode as a tree identity");
        }
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

    #[test]
    fn rejects_a_relaxed_measured_only_counter_rule() {
        let mut schema = schema_value();
        schema["$defs"]["work"]["then"]["required"] = json!(["units"]);
        assert!(
            validate_schema_document(&schema).is_err(),
            "the measured-only counter rule is pinned"
        );
    }

    #[test]
    fn rejects_a_dropped_cache_outcome_vocabulary() {
        let mut schema = schema_value();
        schema["$defs"]["cache"]["properties"]["outcome"]["enum"] = json!(["hit", "miss"]);
        assert!(validate_schema_document(&schema).is_err());
    }
}
