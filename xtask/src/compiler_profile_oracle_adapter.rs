//! Concrete `oracle_receipt.v1` observation adapter for maintained compiler
//! operating profiles (#12302, train row COMP-PROFILE-E05A, parent #12202).
//!
//! This module normalizes one canonical differential real-Perl oracle receipt
//! (`schemas/oracle_receipt.v1.schema.json`) into exactly one landed #12188
//! [`CompilerProfileObservationV1`] envelope, and registers the one
//! [`ObservationAdapterDescriptor`] that owns the `oracle_receipt` family.
//!
//! The source receipt remains canonical.  The observation is an evaluation
//! input: it preserves or narrows source claims and can never create curated
//! gold independence, EIR mechanism, provider behavior, product runtime, or
//! operating-profile verdicts.
//!
//! Deliberately absent (issue non-goals): no oracle runner, no curated-gold
//! implementation, no EIR/compiler/provider/client execution, no evidence-set
//! assembly or evaluator, no status/support/release/publication action, and no
//! change to `schemas/oracle_receipt.v1.schema.json` itself.
//!
//! # Closure laws expressed here
//!
//! - the production schema is the structural authority and is actually
//!   applied ([`validate_receipt_value`] compiles and runs the schema through
//!   `jsonschema`), so a structurally invalid receipt can never be adapted;
//! - the adapter's closed vocabularies are checked against the production
//!   schema's `$defs` enums on every adaptation ([`ensure_vocabulary_current`]),
//!   so a schema that gains, loses, or renames a member fails closed instead
//!   of silently dropping into an older adapter reading;
//! - one comparison class, fixture, source snapshot, extractor, Perl oracle,
//!   module-path authority, and producer/schema identity bind exact,
//!   non-transferable subject dimensions; equal fact names or outputs from
//!   another producer can never satisfy the selected subject;
//! - Rust and oracle fact sets are never merged: they are counted, classified,
//!   and reported independently;
//! - every typed comparison result and promotion effect stays distinct; only
//!   an all-`oracle_agrees` receipt with a completed instrument, a declared
//!   oracle subject, bounded module/ambient authority, and non-empty fact
//!   evidence reaches [`ObservationDisposition::Pass`];
//! - `supports_promotion` is source metadata: the strongest ceiling this
//!   adapter may reach is [`ClaimCeiling::AcceptedCompatibility`], which is
//!   not support, release, or publication authorization;
//! - dynamic, stale, unsupported, ambient, generated, fallback, and
//!   low-confidence states remain visible through the independent currentness,
//!   completeness, and limitation axes and constrain the ceiling;
//! - redaction is load-bearing: a false or leaking redaction flag is an
//!   instrument/privacy failure ([`TerminalState::InstrumentFailed`]), never a
//!   semantic pass or fail;
//! - normalized output is bounded and private-safe: no source bodies, raw
//!   paths, raw environment values, launch payloads, full messages, or raw
//!   logs cross the boundary — only hashes, path classes, bounded ids, typed
//!   results, and the source receipt digest;
//! - non-semantic input ordering cannot change normalized bytes: the receipt
//!   digest and the observation identity are computed over canonical text with
//!   every order-insensitive collection sorted.

use crate::compiler_profile_contract::{
    ClaimCeiling, ClaimFamily, InvalidationInput, InvalidationKind, ProofClass,
};
use crate::compiler_profile_observation::{
    AdapterId, AdapterIdentity, AdapterLossiness, AdapterVersion, CandidateSubjectIdentity,
    CanonicalReceiptReference, CompilerProfileObservationV1, CompletenessDisposition,
    CurrentnessDisposition, InstrumentAndTerminalState, InvalidationEvidence,
    LimitationDisposition, ObservationAdapterDescriptor, ObservationAdapterRegistry,
    ObservationClass, ObservationDigest, ObservationDisposition, ObservedClaimCeiling,
    ProducerAndSchemaIdentity, ReceiptFamily, ReceiptId, SchemaVersion, SubjectDimension,
    SubjectDimensionKind, TerminalState, WorkDisposition,
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::OnceLock;

/// The production `oracle_receipt.v1` schema document, embedded so the adapter
/// carries its structural authority without depending on a repository root.
const SCHEMA_TEXT: &str = include_str!("../../schemas/oracle_receipt.v1.schema.json");

/// The one source receipt family this adapter owns.
pub const SOURCE_FAMILY: &str = "oracle_receipt";

/// Stable identity of this adapter.
pub const ADAPTER_ID: &str = "adapter.oracle-receipt-v1";

/// Stable version of this adapter.
pub const ADAPTER_VERSION: &str = "v1";

/// Exact source schema tag this adapter accepts.
pub const SOURCE_SCHEMA_TAG: &str = "oracle_receipt.v1";

/// Numeric source schema version corresponding to [`SOURCE_SCHEMA_TAG`].
pub const SOURCE_SCHEMA_VERSION: u32 = 1;

/// Producer identity of the `oracle_receipt` family.
const PRODUCER: &str = "oracle-receipt-differential";

/// Instrument identity carried by every observation this adapter emits.
const INSTRUMENT: &str = "oracle-receipt-differential";

/// Source authority/owner of the accepted receipt family.
const SOURCE_AUTHORITY: &str = "compiler-operating-profile evidence train E05A";

// ---------------------------------------------------------------------------
// Closed source vocabulary
// ---------------------------------------------------------------------------

macro_rules! closed_vocabulary {
    (
        $(#[$meta:meta])*
        $name:ident, $schema_def:expr, {
            $($variant:ident => $tag:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
        pub enum $name {
            $(
                #[serde(rename = $tag)]
                #[doc = concat!("Source vocabulary member `", $tag, "`.")]
                $variant,
            )+
        }

        impl $name {
            /// The `$defs` name this vocabulary mirrors in the production schema.
            pub const SCHEMA_DEF: &'static str = $schema_def;

            /// Stable source tag, identical to the schema enum member.
            pub fn tag(self) -> &'static str {
                match self {
                    $(Self::$variant => $tag),+
                }
            }

            /// Every member's source tag, for comparison against the schema.
            pub fn schema_tags() -> ::std::collections::BTreeSet<&'static str> {
                ::std::collections::BTreeSet::from([$($tag),+])
            }
        }
    };
}

closed_vocabulary! {
    /// Closed comparison class.  One class can never satisfy another; a future
    /// class requires a source schema transition and an adapter update.
    ComparisonClass, "comparison_class", {
        PackageSubTable => "PackageSubTable",
        ImportExport => "ImportExport",
        IsaComposition => "IsaComposition",
        ConstantPrototype => "ConstantPrototype",
        FrameworkGeneratedMember => "FrameworkGeneratedMember",
        CompileEffect => "CompileEffect",
    }
}

closed_vocabulary! {
    /// Closed typed comparison result class.
    ResultClass, "result_class", {
        OracleAgrees => "oracle_agrees",
        CompilerMissing => "compiler_missing",
        CompilerExtra => "compiler_extra",
        RangeMismatch => "range_mismatch",
        ProvenanceMismatch => "provenance_mismatch",
        ConfidenceOrFreshnessMismatch => "confidence_or_freshness_mismatch",
        DynamicOrUnsupported => "dynamic_or_unsupported",
        OracleAmbientUnbounded => "oracle_ambient_unbounded",
        StaleOrPartial => "stale_or_partial",
        Unknown => "unknown",
    }
}

closed_vocabulary! {
    /// Closed promotion effect.  `supports_promotion` is source metadata and
    /// authorizes nothing on its own.
    PromotionEffect, "promotion_effect", {
        SupportsPromotion => "supports_promotion",
        BlocksPromotion => "blocks_promotion",
        KnownLimitation => "known_limitation",
        Unknown => "unknown",
    }
}

closed_vocabulary! {
    /// Closed fact provenance.
    FactProvenance, "fact_provenance", {
        ExplicitSource => "ExplicitSource",
        SourceBackedGenerated => "SourceBackedGenerated",
        GeneratedNoSource => "GeneratedNoSource",
        DynamicBoundary => "DynamicBoundary",
        AmbientInput => "AmbientInput",
        Unknown => "Unknown",
    }
}

closed_vocabulary! {
    /// Closed fact confidence.
    Confidence, "confidence", {
        High => "high",
        Medium => "medium",
        Low => "low",
    }
}

closed_vocabulary! {
    /// Closed fact freshness.
    Freshness, "freshness", {
        Fresh => "fresh",
        Stale => "stale",
        Unknown => "unknown",
        NotApplicable => "not_applicable",
    }
}

closed_vocabulary! {
    /// Closed fallback state.
    FallbackState, "fallback_state", {
        None => "none",
        LegacyProvider => "legacy_provider",
        NoResult => "no_result",
        NoEdit => "no_edit",
        RequireConfirmation => "require_confirmation",
        RefreshWorkspaceFacts => "refresh_workspace_facts",
        ShadowReceiptOnly => "shadow_receipt_only",
    }
}

/// Closed source-range path class.  A redacted private fixture stays redacted
/// and can never be upgraded to a public one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathClass {
    /// A public test fixture whose bounded source identity may be named.
    PublicTestFixture,
    /// A redacted private fixture; only its content hash and class may cross.
    RedactedPrivateFixture,
}

impl PathClass {
    /// Stable source tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::PublicTestFixture => "public_test_fixture",
            Self::RedactedPrivateFixture => "redacted_private_fixture",
        }
    }
}

/// Closed Perl interpreter identity.  Declared fixture Perl and system Perl
/// are different subjects; `unknown` satisfies no exact oracle row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpreter {
    /// The declared fixture interpreter.
    DeclaredFixturePerl,
    /// An ambient system interpreter.
    SystemPerl,
    /// An unknown interpreter.
    Unknown,
}

impl Interpreter {
    /// Stable source tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::DeclaredFixturePerl => "declared_fixture_perl",
            Self::SystemPerl => "system_perl",
            Self::Unknown => "unknown",
        }
    }
}

/// Closed oracle invocation mode.  A shadow test command stays shadow/test
/// evidence and can never become product runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationMode {
    /// The declared fixture command.
    DeclaredFixtureCommand,
    /// A shadow test command.
    ShadowTestCommand,
    /// An unknown invocation.
    Unknown,
}

impl InvocationMode {
    /// Stable source tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::DeclaredFixtureCommand => "declared_fixture_command",
            Self::ShadowTestCommand => "shadow_test_command",
            Self::Unknown => "unknown",
        }
    }
}

/// Closed module-path authority.  Ambient-reported authority cannot satisfy a
/// hermetic exact row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleAuthority {
    /// A declared fixture root.
    DeclaredFixtureRoot,
    /// Declared module roots.
    DeclaredModuleRoots,
    /// Ambient roots reported by the environment.
    AmbientReported,
}

impl ModuleAuthority {
    /// Stable source tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::DeclaredFixtureRoot => "declared_fixture_root",
            Self::DeclaredModuleRoots => "declared_module_roots",
            Self::AmbientReported => "ambient_reported",
        }
    }
}

/// Closed ambient-input authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmbientAuthority {
    /// Reported only; not a declared input.
    ReportedOnly,
    /// An explicitly declared input.
    DeclaredInput,
    /// An unbounded ambient input.
    Unbounded,
}

impl AmbientAuthority {
    /// Stable source tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::ReportedOnly => "reported_only",
            Self::DeclaredInput => "declared_input",
            Self::Unbounded => "unbounded",
        }
    }
}

/// Closed denied-environment key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum DeniedEnvironmentKey {
    /// `PERL5LIB`.
    #[serde(rename = "PERL5LIB")]
    Perl5Lib,
    /// `PERL5OPT`.
    #[serde(rename = "PERL5OPT")]
    Perl5Opt,
    /// `local::lib`.
    #[serde(rename = "local::lib")]
    LocalLib,
}

impl DeniedEnvironmentKey {
    /// The closed set of startup inputs a hermetic oracle run must account
    /// for.  Every key here is either denied or explicitly declared; silence
    /// about one is not hermeticity.
    pub const ALL: [Self; 3] = [Self::Perl5Lib, Self::Perl5Opt, Self::LocalLib];

    /// Stable source tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Perl5Lib => "PERL5LIB",
            Self::Perl5Opt => "PERL5OPT",
            Self::LocalLib => "local::lib",
        }
    }
}

// ---------------------------------------------------------------------------
// Typed source receipt
// ---------------------------------------------------------------------------

/// One exact source range.  A null range stays null and is never
/// reconstructed from names, order, or other facts.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRange {
    /// Path class of the range.
    pub path_class: PathClass,
    /// Zero-based start line.
    pub start_line: u32,
    /// Zero-based start character.
    pub start_character: u32,
    /// Zero-based end line.
    pub end_line: u32,
    /// Zero-based end character.
    pub end_character: u32,
}

/// Bounded source snapshot identity.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSnapshot {
    /// Path class of the snapshot.
    pub path_class: PathClass,
    /// Bounded fixture source identity.
    pub fixture_source: String,
    /// Content hash of the snapshot.
    pub content_hash: String,
}

/// Rust extractor observation; producer identity is load-bearing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustExtractor {
    /// Extractor name.
    pub name: String,
    /// Extractor version.
    pub version: String,
    /// Extractor fact model.
    pub fact_model: String,
}

/// Real-Perl oracle observation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerlOracle {
    /// Interpreter identity.
    pub interpreter: Interpreter,
    /// Interpreter version.
    pub version: String,
    /// Invocation mode.
    pub invocation_mode: InvocationMode,
}

/// Module-path authority and declared roots.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulePathAuthority {
    /// Declared authority.
    pub authority: ModuleAuthority,
    /// Declared module roots.
    pub declared_roots: Vec<String>,
    /// Whether ambient roots were reported.
    pub ambient_roots_reported: bool,
}

/// Environment declaration; values are never carried, only bounded keys.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentDeclaration {
    /// Environment keys denied by the hermetic boundary.
    pub denied: Vec<DeniedEnvironmentKey>,
    /// Environment keys declared present.
    pub declared: Vec<String>,
    /// Whether environment values were redacted.
    pub redacted_values: bool,
}

/// One ambient input and its authority.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbientInput {
    /// Bounded ambient input kind.
    pub kind: String,
    /// Declared authority of the input.
    pub authority: AmbientAuthority,
}

/// One framework-generated input.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedInput {
    /// Generating framework.
    pub framework: String,
    /// Provenance of the generated input.
    pub provenance: FactProvenance,
    /// Source range, or null.
    pub source_range: Option<SourceRange>,
}

/// One dynamic boundary or unsupported effect.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryEntry {
    /// Bounded boundary kind.
    pub kind: String,
    /// Source range, or null.
    pub source_range: Option<SourceRange>,
}

/// One stale fact and its freshness.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaleFact {
    /// Fact identity.
    pub fact_id: String,
    /// Declared freshness.
    pub freshness: Freshness,
}

/// One normalized fact.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedFact {
    /// Fact identity.
    pub fact_id: String,
    /// Fact name.
    pub name: String,
    /// Fact provenance.
    pub provenance: FactProvenance,
    /// Fact confidence.
    pub confidence: Confidence,
    /// Fact freshness.
    pub freshness: Freshness,
    /// Fallback state.
    pub fallback: FallbackState,
    /// Source range, or null.
    pub source_range: Option<SourceRange>,
}

/// Rust and oracle fact sets, kept independent and never merged.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedFacts {
    /// Facts observed by the Rust extractor.
    pub rust: Vec<NormalizedFact>,
    /// Facts observed by the real-Perl oracle.
    pub oracle: Vec<NormalizedFact>,
}

/// One typed comparison result.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonResult {
    /// Typed result class.
    pub result_class: ResultClass,
    /// Fact identity the comparison ranges over.
    pub fact_id: String,
    /// Declared promotion effect.
    pub promotion_effect: PromotionEffect,
    /// Explanatory message; never parsed for semantics.
    pub message: String,
}

/// Redaction flags; all three are load-bearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Redaction {
    /// Whether private paths were redacted.
    pub private_paths_redacted: bool,
    /// Whether environment values were redacted.
    pub environment_values_redacted: bool,
    /// Whether raw launch payloads were redacted.
    pub raw_launch_payloads_redacted: bool,
}

/// A typed `oracle_receipt.v1` document.  Unknown fields and unknown
/// vocabulary members fail closed at deserialization.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleReceiptV1 {
    /// Source schema tag; must equal [`SOURCE_SCHEMA_TAG`].
    pub schema_version: String,
    /// Exact receipt identity.
    pub receipt_id: String,
    /// Exact comparison class.
    pub comparison_class: ComparisonClass,
    /// Exact fixture identity.
    pub fixture_id: String,
    /// Bounded source snapshot identity.
    pub source_snapshot: SourceSnapshot,
    /// Rust extractor observation.
    pub rust_extractor: RustExtractor,
    /// Real-Perl oracle observation.
    pub perl_oracle: PerlOracle,
    /// Module-path authority.
    pub module_path_authority: ModulePathAuthority,
    /// Environment declaration.
    pub environment: EnvironmentDeclaration,
    /// Ambient inputs.
    pub ambient_inputs: Vec<AmbientInput>,
    /// Framework-generated inputs.
    pub generated_inputs: Vec<GeneratedInput>,
    /// Dynamic boundaries.
    pub dynamic_boundaries: Vec<BoundaryEntry>,
    /// Stale facts.
    pub stale_facts: Vec<StaleFact>,
    /// Unsupported effects.
    pub unsupported_effects: Vec<BoundaryEntry>,
    /// Independent Rust and oracle fact sets.
    pub normalized_facts: NormalizedFacts,
    /// Typed comparisons (non-empty).
    pub comparisons: Vec<ComparisonResult>,
    /// Observed provider-behavior flag; never permission or proof.
    pub provider_behavior_changed: bool,
    /// Structural constant: oracle execution is test-only.
    pub editor_runtime_dependency: bool,
    /// Redaction flags.
    pub redaction: Redaction,
    /// Declared claim boundary.
    pub claim_boundary: String,
}

// ---------------------------------------------------------------------------
// Production schema authority
// ---------------------------------------------------------------------------

fn schema_value() -> Result<Value> {
    serde_json::from_str(SCHEMA_TEXT).context("embedded oracle_receipt.v1 schema must parse")
}

/// The schema document, its compiled validator, and the vocabulary-drift
/// verdict are all constant for a given build: the schema is embedded at
/// compile time and the adapter's vocabularies are closed enums. Parsing,
/// compiling, and re-deriving them per receipt would put that constant work on
/// the hot path of `adapt_receipts` and the E07 evidence-set lane, so each is
/// computed once. Errors are stored as text because neither `anyhow::Error`
/// nor a compiled validator is cloneable.
fn schema_authority() -> &'static Result<jsonschema::Validator, String> {
    static AUTHORITY: OnceLock<Result<jsonschema::Validator, String>> = OnceLock::new();
    AUTHORITY.get_or_init(|| {
        let schema = schema_value().map_err(|error| format!("{error:#}"))?;
        verify_vocabulary(&schema).map_err(|error| format!("{error:#}"))?;
        jsonschema::validator_for(&schema)
            .map_err(|error| format!("oracle_receipt.v1 schema is invalid: {error}"))
    })
}

fn receipt_validator() -> Result<&'static jsonschema::Validator> {
    match schema_authority() {
        Ok(validator) => Ok(validator),
        Err(error) => bail!("{error}"),
    }
}

/// Assert that this adapter's closed vocabularies still equal the production
/// schema's `$defs` enums.  A schema that gains, loses, or renames a member
/// fails closed here rather than being silently read through an older adapter
/// vocabulary.
pub fn ensure_vocabulary_current() -> Result<()> {
    match schema_authority() {
        Ok(_) => Ok(()),
        Err(error) => bail!("{error}"),
    }
}

fn verify_vocabulary(schema: &Value) -> Result<()> {
    ensure_enum_matches(schema, ComparisonClass::SCHEMA_DEF, ComparisonClass::schema_tags())?;
    ensure_enum_matches(schema, ResultClass::SCHEMA_DEF, ResultClass::schema_tags())?;
    ensure_enum_matches(schema, PromotionEffect::SCHEMA_DEF, PromotionEffect::schema_tags())?;
    ensure_enum_matches(schema, FactProvenance::SCHEMA_DEF, FactProvenance::schema_tags())?;
    ensure_enum_matches(schema, Confidence::SCHEMA_DEF, Confidence::schema_tags())?;
    ensure_enum_matches(schema, Freshness::SCHEMA_DEF, Freshness::schema_tags())?;
    ensure_enum_matches(schema, FallbackState::SCHEMA_DEF, FallbackState::schema_tags())?;
    ensure_nested_enum_matches(
        schema,
        &["$defs", "perl_oracle", "properties", "interpreter", "enum"],
        ["declared_fixture_perl", "system_perl", "unknown"],
    )?;
    ensure_nested_enum_matches(
        schema,
        &["$defs", "perl_oracle", "properties", "invocation_mode", "enum"],
        ["declared_fixture_command", "shadow_test_command", "unknown"],
    )?;
    ensure_nested_enum_matches(
        schema,
        &["$defs", "module_path_authority", "properties", "authority", "enum"],
        ["declared_fixture_root", "declared_module_roots", "ambient_reported"],
    )?;
    ensure_nested_enum_matches(
        schema,
        &["$defs", "source_range", "properties", "path_class", "enum"],
        ["public_test_fixture", "redacted_private_fixture"],
    )?;
    ensure_nested_enum_matches(
        schema,
        &["$defs", "environment", "properties", "denied", "items", "enum"],
        ["PERL5LIB", "PERL5OPT", "local::lib"],
    )?;
    Ok(())
}

fn ensure_enum_matches(schema: &Value, def: &str, expected: BTreeSet<&'static str>) -> Result<()> {
    ensure_nested_enum_matches(schema, &["$defs", def, "enum"], expected)
}

fn ensure_nested_enum_matches<I>(schema: &Value, path: &[&str], expected: I) -> Result<()>
where
    I: IntoIterator<Item = &'static str>,
{
    let expected: BTreeSet<&str> = expected.into_iter().collect();
    let mut node = schema;
    for segment in path {
        node = match node.get(*segment) {
            Some(next) => next,
            None => bail!(
                "oracle_receipt.v1 schema is missing {}; the adapter fails closed",
                path.join(".")
            ),
        };
    }
    let Some(members) = node.as_array() else {
        bail!("oracle_receipt.v1 schema {} must be an array", path.join("."));
    };
    let actual: BTreeSet<&str> = members.iter().filter_map(Value::as_str).collect();
    if actual.len() != members.len() {
        bail!("oracle_receipt.v1 schema {} must contain only strings", path.join("."));
    }
    if actual != expected {
        let missing: Vec<&str> = expected.difference(&actual).copied().collect();
        let extra: Vec<&str> = actual.difference(&expected).copied().collect();
        bail!(
            "adapter {ADAPTER_ID} vocabulary has drifted from oracle_receipt.v1 schema {}: \
             adapter-only {missing:?}, schema-only {extra:?}; a changed source vocabulary \
             requires an adapter update and fails closed",
            path.join(".")
        );
    }
    Ok(())
}

/// Validate one receipt document with the production schema and decode it into
/// the typed closed vocabulary.
///
/// The schema is the structural authority for this receipt family, so it is
/// actually compiled and applied: parsing the document as JSON alone would let
/// a structurally invalid receipt reach adaptation.
pub fn validate_receipt_value(value: &Value) -> Result<OracleReceiptV1> {
    ensure_vocabulary_current()?;

    // An unknown or future source schema fails closed before any structural
    // reading, so a mis-tagged document can never be read as this family.
    match value.get("schema_version").and_then(Value::as_str) {
        Some(tag) if tag == SOURCE_SCHEMA_TAG => {}
        Some(tag) => bail!(
            "adapter {ADAPTER_ID} accepts source schema {SOURCE_SCHEMA_TAG:?} only; \
             {tag:?} is unknown or future and fails closed"
        ),
        None => bail!("oracle receipt is missing the required schema_version tag"),
    }

    let violations: Vec<String> =
        receipt_validator()?.iter_errors(value).map(|error| error.to_string()).collect();
    if !violations.is_empty() {
        bail!(
            "oracle receipt fails the production oracle_receipt.v1 schema with {} violation(s): {}",
            violations.len(),
            violations.join("; ")
        );
    }

    let receipt: OracleReceiptV1 = serde_json::from_value(value.clone())
        .context("oracle receipt does not decode into the closed oracle_receipt.v1 vocabulary")?;
    ensure_adapter_invariants(&receipt)?;
    Ok(receipt)
}

/// The invariants the adapter owns in its own right, independently of the
/// schema that currently also encodes them.
///
/// The controlling issue makes the adapter — not only the schema — responsible
/// for rejecting an editor-runtime receipt, and the adapter's own `Pass` rule
/// reads the typed comparison set, so it must never run on an empty one. Both
/// checks are unreachable through a document that satisfies the current
/// schema; they exist so a future schema relaxation cannot silently widen what
/// this adapter accepts.
pub fn ensure_adapter_invariants(receipt: &OracleReceiptV1) -> Result<()> {
    if receipt.editor_runtime_dependency {
        bail!(
            "oracle receipt {:?} declares an editor runtime dependency; oracle execution is \
             test-only and the adapter rejects the receipt",
            receipt.receipt_id
        );
    }
    if receipt.comparisons.is_empty() {
        bail!(
            "oracle receipt {:?} carries no typed comparisons, so no agreement can be observed",
            receipt.receipt_id
        );
    }
    // A fact identity names one observation per side. The schema does not
    // require uniqueness, but every downstream judgment — side-aware
    // comparison coverage above all — reads facts by identity, and a repeated
    // identity would silently collapse two distinct observations into one.
    ensure_unique_fact_ids("rust", &receipt.normalized_facts.rust)?;
    ensure_unique_fact_ids("oracle", &receipt.normalized_facts.oracle)?;
    Ok(())
}

fn ensure_unique_fact_ids(side: &str, facts: &[NormalizedFact]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for fact in facts {
        if !seen.insert(fact.fact_id.as_str()) {
            bail!(
                "the {side} fact set repeats identity {:?}; one identity names one observation \
                 per side",
                fact.fact_id
            );
        }
    }
    Ok(())
}

/// Validate and decode one receipt document from JSON text.
pub fn validate_receipt_json(text: &str) -> Result<OracleReceiptV1> {
    let value: Value =
        serde_json::from_str(text).context("oracle receipt is not well-formed JSON")?;
    validate_receipt_value(&value)
}

// ---------------------------------------------------------------------------
// Canonical receipt digest
// ---------------------------------------------------------------------------

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn range_text(range: Option<&SourceRange>) -> String {
    match range {
        // A null range stays null: it is never reconstructed from names, order,
        // or neighbouring facts.
        None => "null".to_owned(),
        Some(range) => format!(
            "{}:{}:{}:{}:{}",
            range.path_class.tag(),
            range.start_line,
            range.start_character,
            range.end_line,
            range.end_character
        ),
    }
}

fn fact_text(fact: &NormalizedFact) -> String {
    format!(
        "id={:?} name={:?} provenance={} confidence={} freshness={} fallback={} range={}",
        fact.fact_id,
        fact.name,
        fact.provenance.tag(),
        fact.confidence.tag(),
        fact.freshness.tag(),
        fact.fallback.tag(),
        range_text(fact.source_range.as_ref())
    )
}

fn sorted<T, F>(items: &[T], render: F) -> Vec<String>
where
    F: Fn(&T) -> String,
{
    let mut rendered: Vec<String> = items.iter().map(render).collect();
    rendered.sort();
    rendered
}

/// Deterministic canonical text of one receipt.  Every order-insensitive
/// collection is sorted, so non-semantic input ordering cannot change the
/// receipt digest or the observation identity derived from it.
pub fn canonical_receipt_text(receipt: &OracleReceiptV1) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{SOURCE_SCHEMA_TAG}");
    let _ = writeln!(out, "receipt_id={:?}", receipt.receipt_id);
    let _ = writeln!(out, "comparison_class={}", receipt.comparison_class.tag());
    let _ = writeln!(out, "fixture_id={:?}", receipt.fixture_id);
    let _ = writeln!(
        out,
        "source_snapshot path_class={} fixture_source={:?} content_hash={:?}",
        receipt.source_snapshot.path_class.tag(),
        receipt.source_snapshot.fixture_source,
        receipt.source_snapshot.content_hash
    );
    let _ = writeln!(
        out,
        "rust_extractor name={:?} version={:?} fact_model={:?}",
        receipt.rust_extractor.name,
        receipt.rust_extractor.version,
        receipt.rust_extractor.fact_model
    );
    let _ = writeln!(
        out,
        "perl_oracle interpreter={} version={:?} invocation_mode={}",
        receipt.perl_oracle.interpreter.tag(),
        receipt.perl_oracle.version,
        receipt.perl_oracle.invocation_mode.tag()
    );
    let mut roots = receipt.module_path_authority.declared_roots.clone();
    roots.sort();
    let _ = writeln!(
        out,
        "module_path_authority authority={} ambient_roots_reported={} roots={roots:?}",
        receipt.module_path_authority.authority.tag(),
        receipt.module_path_authority.ambient_roots_reported
    );
    let mut denied: Vec<&str> = receipt.environment.denied.iter().map(|key| key.tag()).collect();
    denied.sort_unstable();
    let mut declared = receipt.environment.declared.clone();
    declared.sort();
    let _ = writeln!(
        out,
        "environment denied={denied:?} declared={declared:?} redacted_values={}",
        receipt.environment.redacted_values
    );
    for line in sorted(&receipt.ambient_inputs, |input| {
        format!("kind={:?} authority={}", input.kind, input.authority.tag())
    }) {
        let _ = writeln!(out, "ambient_input {line}");
    }
    for line in sorted(&receipt.generated_inputs, |input| {
        format!(
            "framework={:?} provenance={} range={}",
            input.framework,
            input.provenance.tag(),
            range_text(input.source_range.as_ref())
        )
    }) {
        let _ = writeln!(out, "generated_input {line}");
    }
    for line in sorted(&receipt.dynamic_boundaries, |entry| {
        format!("kind={:?} range={}", entry.kind, range_text(entry.source_range.as_ref()))
    }) {
        let _ = writeln!(out, "dynamic_boundary {line}");
    }
    for line in sorted(&receipt.stale_facts, |fact| {
        format!("fact_id={:?} freshness={}", fact.fact_id, fact.freshness.tag())
    }) {
        let _ = writeln!(out, "stale_fact {line}");
    }
    for line in sorted(&receipt.unsupported_effects, |entry| {
        format!("kind={:?} range={}", entry.kind, range_text(entry.source_range.as_ref()))
    }) {
        let _ = writeln!(out, "unsupported_effect {line}");
    }
    // The Rust and oracle fact sets are canonicalized under separate labels;
    // they are never merged into one undifferentiated set.
    for line in sorted(&receipt.normalized_facts.rust, fact_text) {
        let _ = writeln!(out, "rust_fact {line}");
    }
    for line in sorted(&receipt.normalized_facts.oracle, fact_text) {
        let _ = writeln!(out, "oracle_fact {line}");
    }
    for line in sorted(&receipt.comparisons, |comparison| {
        format!(
            "result_class={} fact_id={:?} promotion_effect={} message={:?}",
            comparison.result_class.tag(),
            comparison.fact_id,
            comparison.promotion_effect.tag(),
            comparison.message
        )
    }) {
        let _ = writeln!(out, "comparison {line}");
    }
    let _ = writeln!(out, "provider_behavior_changed={}", receipt.provider_behavior_changed);
    let _ = writeln!(out, "editor_runtime_dependency={}", receipt.editor_runtime_dependency);
    let _ = writeln!(
        out,
        "redaction private_paths={} environment_values={} raw_launch_payloads={}",
        receipt.redaction.private_paths_redacted,
        receipt.redaction.environment_values_redacted,
        receipt.redaction.raw_launch_payloads_redacted
    );
    let _ = writeln!(out, "claim_boundary={:?}", receipt.claim_boundary);
    out
}

/// Deterministic digest of one receipt's canonical text.
pub fn receipt_digest(receipt: &OracleReceiptV1) -> Result<ObservationDigest> {
    ObservationDigest::from_hex(&sha256_hex(canonical_receipt_text(receipt).as_bytes()))
        .context("sha256 hex output must satisfy the digest invariant")
}

// ---------------------------------------------------------------------------
// Adapter descriptor and registry
// ---------------------------------------------------------------------------

/// The one observation class this adapter may emit: parser/compiler-internal
/// fact production proven on the real-Perl oracle axis.  It is never provider,
/// edit, execution, packaged, installed-host, actual-client, or EIR evidence.
pub fn emitted_class() -> ObservationClass {
    ObservationClass {
        family: ClaimFamily::ParserInternal,
        proof_class: ProofClass::RealPerlOracle,
    }
}

/// The static descriptor registering this adapter for `oracle_receipt` v1.
pub fn oracle_receipt_adapter() -> Result<ObservationAdapterDescriptor> {
    let descriptor = ObservationAdapterDescriptor {
        id: AdapterId::new(ADAPTER_ID)?,
        version: AdapterVersion::new(ADAPTER_VERSION)?,
        source_family: ReceiptFamily::new(SOURCE_FAMILY)?,
        schema_min: SchemaVersion::new(SOURCE_SCHEMA_VERSION),
        schema_max: SchemaVersion::new(SOURCE_SCHEMA_VERSION),
        source_authority: SOURCE_AUTHORITY.to_owned(),
        emitted_classes: BTreeSet::from([emitted_class()]),
        provable_dimensions: BTreeSet::from([
            SubjectDimensionKind::FixtureSeries,
            SubjectDimensionKind::Toolchain,
            SubjectDimensionKind::CompilerPolicy,
            SubjectDimensionKind::ProducerConfiguration,
        ]),
        preserved_fields: BTreeSet::from([
            "ambient_inputs.authority".to_owned(),
            "claim_boundary".to_owned(),
            "comparison_class".to_owned(),
            "comparisons.promotion_effect".to_owned(),
            "comparisons.result_class".to_owned(),
            "dynamic_boundaries".to_owned(),
            "editor_runtime_dependency".to_owned(),
            "environment.declared".to_owned(),
            "environment.denied".to_owned(),
            "fixture_id".to_owned(),
            "generated_inputs.provenance".to_owned(),
            "module_path_authority".to_owned(),
            "normalized_facts.oracle".to_owned(),
            "normalized_facts.rust".to_owned(),
            "perl_oracle".to_owned(),
            "provider_behavior_changed".to_owned(),
            "receipt_id".to_owned(),
            "redaction".to_owned(),
            "rust_extractor".to_owned(),
            "source_snapshot.content_hash".to_owned(),
            "source_snapshot.path_class".to_owned(),
            "stale_facts".to_owned(),
            "unsupported_effects".to_owned(),
        ]),
        lossiness: AdapterLossiness::lossy(
            "comparison message text, declared module root values (digest only), and exact \
             source-range coordinates are dropped; typed result classes, promotion effects, \
             provenance, confidence, freshness, fallback, path classes, and boundary counts \
             are preserved",
        )?,
        // A differential oracle receipt is bounded fixture evidence: at its
        // strongest it supports an accepted compatibility state for its exact
        // subject, never a bounded public claim and never support, release, or
        // publication authorization.
        source_claim_ceiling: ClaimCeiling::AcceptedCompatibility,
        observation_claim_ceiling: ObservedClaimCeiling::new(ClaimCeiling::AcceptedCompatibility),
        required_currentness_inputs: BTreeSet::from([
            InvalidationKind::Source,
            InvalidationKind::Dependency,
            InvalidationKind::Oracle,
            InvalidationKind::HostEnvironment,
        ]),
        unsupported_source_versions: BTreeSet::new(),
        supersedes: None,
    };
    descriptor.validate()?;
    Ok(descriptor)
}

/// A registry owning exactly this adapter.  Evidence-set assembly (E07)
/// composes the sibling receipt-family registries; this constructor exists so
/// every observation the adapter emits can be validated against the registry
/// that declares it.
pub fn oracle_receipt_registry() -> Result<ObservationAdapterRegistry> {
    ObservationAdapterRegistry::from_descriptors(vec![oracle_receipt_adapter()?])
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Independent observations the adapter derives from one receipt before it
/// selects the envelope's closed dispositions.  Each list stays separate so a
/// blocking contradiction can never be averaged away by a passing comparison,
/// and a limitation can never be silently promoted into support.
#[derive(Debug, Default)]
struct Findings {
    instrument: Vec<String>,
    blocking: Vec<String>,
    unproven: Vec<String>,
    stale: Vec<String>,
    incomplete: Vec<String>,
    limitations: Vec<String>,
}

impl Findings {
    fn join(reasons: &[String]) -> String {
        reasons.join("; ")
    }
}

fn count_by<T, F: Fn(&T) -> bool>(items: &[T], predicate: F) -> usize {
    items.iter().filter(|item| predicate(item)).count()
}

/// The distinct fact identities one side observed.  Identity uniqueness inside
/// a side is an adapter invariant, so this set is never lossy.
fn side_fact_ids(facts: &[NormalizedFact]) -> BTreeSet<&str> {
    facts.iter().map(|fact| fact.fact_id.as_str()).collect()
}

/// Which side each result class needs evidence from, and why a receipt that
/// does not carry it cannot be taken at its word.
///
/// The two fact sets are independent: `oracle_agrees` asserts that both the
/// Rust extractor and the real-Perl oracle observed the same fact, so a
/// one-sided receipt claims differential agreement it never gathered.
/// `compiler_missing` and `compiler_extra` are directional by definition, and
/// the three mismatch classes compare two observations that must both exist.
fn evidence_incoherence(
    result_class: ResultClass,
    in_rust: bool,
    in_oracle: bool,
) -> Option<&'static str> {
    match result_class {
        ResultClass::OracleAgrees
        | ResultClass::RangeMismatch
        | ResultClass::ProvenanceMismatch
        | ResultClass::ConfidenceOrFreshnessMismatch => match (in_rust, in_oracle) {
            (true, true) => None,
            (false, true) => Some("names no Rust fact to compare"),
            (true, false) => Some("names no oracle fact to compare"),
            (false, false) => Some("names neither a Rust nor an oracle fact"),
        },
        ResultClass::CompilerMissing => match (in_rust, in_oracle) {
            (false, true) => None,
            (true, _) => Some("names a fact the Rust set does carry"),
            (false, false) => Some("names no oracle fact that could be missing"),
        },
        ResultClass::CompilerExtra => match (in_rust, in_oracle) {
            (true, false) => None,
            (_, true) => Some("names a fact the oracle set does carry"),
            (false, false) => Some("names no Rust fact that could be extra"),
        },
        // These classes already land in the unproven bucket on their own; they
        // report a boundary rather than a two-sided observation.
        ResultClass::DynamicOrUnsupported
        | ResultClass::OracleAmbientUnbounded
        | ResultClass::StaleOrPartial
        | ResultClass::Unknown => None,
    }
}

fn boundary_provenance(provenance: FactProvenance) -> bool {
    matches!(
        provenance,
        FactProvenance::GeneratedNoSource
            | FactProvenance::DynamicBoundary
            | FactProvenance::AmbientInput
            | FactProvenance::Unknown
    )
}

fn collect_findings(receipt: &OracleReceiptV1) -> Findings {
    let mut findings = Findings::default();

    // Redaction and hermeticity are instrument/privacy properties, not
    // semantic pass/fail: a false or leaking flag fails the instrument.
    if !receipt.redaction.private_paths_redacted {
        findings.instrument.push("private paths are not redacted".to_owned());
    }
    if !receipt.redaction.environment_values_redacted {
        findings.instrument.push("environment values are not redacted".to_owned());
    }
    if !receipt.redaction.raw_launch_payloads_redacted {
        findings.instrument.push("raw launch payloads are not redacted".to_owned());
    }
    if !receipt.environment.redacted_values {
        findings.instrument.push("the environment declaration is not value-redacted".to_owned());
    }
    let denied: BTreeSet<&str> = receipt.environment.denied.iter().map(|key| key.tag()).collect();
    let leaked: Vec<&str> = receipt
        .environment
        .declared
        .iter()
        .map(String::as_str)
        .filter(|key| denied.contains(key))
        .collect();
    if !leaked.is_empty() {
        findings
            .instrument
            .push(format!("{} denied environment key(s) are declared present", leaked.len()));
    }

    // Each closed startup input must be positively accounted for. Silence is
    // not hermeticity: a receipt that neither denies nor declares `PERL5LIB`
    // has said nothing about it, and an absent entry can never read as
    // support.  Declared-and-denied is a self-contradicting instrument;
    // declared-without-denial is an admitted ambient input.
    let declared: BTreeSet<&str> =
        receipt.environment.declared.iter().map(String::as_str).collect();
    let mut leaked = 0_usize;
    let mut undenied = 0_usize;
    let mut unaccounted = 0_usize;
    for key in DeniedEnvironmentKey::ALL {
        match (denied.contains(key.tag()), declared.contains(key.tag())) {
            (true, true) => leaked += 1,
            (false, true) => undenied += 1,
            (false, false) => unaccounted += 1,
            (true, false) => {}
        }
    }
    if leaked > 0 {
        findings
            .instrument
            .push(format!("{leaked} denied startup input(s) are also declared present"));
    }
    if undenied > 0 {
        findings
            .unproven
            .push(format!("{undenied} closed startup input(s) are declared but not denied"));
    }
    if unaccounted > 0 {
        findings
            .unproven
            .push(format!("{unaccounted} closed startup input(s) are neither denied nor declared"));
    }

    // Comparison results are read against the side that actually observed each
    // fact.  The two fact sets are independent, so a result class that claims
    // evidence from a side which named nothing cannot be taken at its word.
    let rust_ids = side_fact_ids(&receipt.normalized_facts.rust);
    let oracle_ids = side_fact_ids(&receipt.normalized_facts.oracle);

    // Typed comparison results stay distinct; one passing comparison can never
    // erase another selected mismatch or boundary.
    for comparison in &receipt.comparisons {
        if let Some(incoherence) = evidence_incoherence(
            comparison.result_class,
            rust_ids.contains(comparison.fact_id.as_str()),
            oracle_ids.contains(comparison.fact_id.as_str()),
        ) {
            let detail = format!("a {} comparison {incoherence}", comparison.result_class.tag());
            // Both axes, deliberately. On the product axis an incoherent row
            // proves nothing — but a declared mismatch outranks `not_proven`
            // there, and softening a mismatch would erase exactly what the
            // receipt reported. Recording the same finding as incompleteness
            // keeps the incoherence visible in the envelope whichever product
            // disposition wins.
            findings.unproven.push(detail.clone());
            findings.incomplete.push(detail);
        }
        // A receipt carries one comparison per named fact, so an aggregated
        // reason string is the only handle a consumer has on which fact caused
        // the verdict. Every reason names its fact.
        let subject = format!(
            "the comparison over fact {:?} in class {}",
            comparison.fact_id,
            comparison.result_class.tag()
        );
        match comparison.promotion_effect {
            PromotionEffect::BlocksPromotion => {
                findings.blocking.push(format!("{subject} blocks promotion"))
            }
            PromotionEffect::Unknown => {
                findings.unproven.push(format!("{subject} carries an unknown promotion effect"))
            }
            PromotionEffect::KnownLimitation => {
                findings.limitations.push(format!("{subject} carries a known limitation"))
            }
            PromotionEffect::SupportsPromotion => {}
        }
        match comparison.result_class {
            ResultClass::OracleAgrees => {}
            ResultClass::CompilerMissing
            | ResultClass::CompilerExtra
            | ResultClass::RangeMismatch
            | ResultClass::ProvenanceMismatch
            | ResultClass::ConfidenceOrFreshnessMismatch => {
                findings.blocking.push(format!("{subject} contradicts the compiler facts"))
            }
            ResultClass::DynamicOrUnsupported
            | ResultClass::OracleAmbientUnbounded
            | ResultClass::StaleOrPartial
            | ResultClass::Unknown => {
                findings.unproven.push(format!("{subject} reaches no exact result"))
            }
        }
    }

    // The oracle subject must be exact: unknown interpreter or invocation
    // satisfies no exact row, and system Perl or a shadow command remains a
    // different, visibly bounded subject.
    if receipt.perl_oracle.interpreter == Interpreter::Unknown {
        findings.unproven.push("the Perl interpreter identity is unknown".to_owned());
    } else if receipt.perl_oracle.interpreter == Interpreter::SystemPerl {
        findings.limitations.push(
            "the oracle ran on ambient system Perl, not the declared fixture Perl".to_owned(),
        );
    }
    if receipt.perl_oracle.invocation_mode == InvocationMode::Unknown {
        findings.unproven.push("the oracle invocation mode is unknown".to_owned());
    } else if receipt.perl_oracle.invocation_mode == InvocationMode::ShadowTestCommand {
        findings
            .limitations
            .push("the oracle ran a shadow test command, which stays shadow evidence".to_owned());
    }

    // Ambient module roots and unbounded ambient inputs cannot satisfy a
    // hermetic exact row.
    if receipt.module_path_authority.authority == ModuleAuthority::AmbientReported {
        findings.unproven.push("module-path authority is ambient-reported".to_owned());
    }
    if receipt.module_path_authority.ambient_roots_reported {
        findings.unproven.push("ambient module roots were reported".to_owned());
    }
    let unbounded =
        count_by(&receipt.ambient_inputs, |input| input.authority == AmbientAuthority::Unbounded);
    if unbounded > 0 {
        findings.unproven.push(format!("{unbounded} ambient input(s) are unbounded"));
    }
    let reported_only = count_by(&receipt.ambient_inputs, |input| {
        input.authority == AmbientAuthority::ReportedOnly
    });
    if reported_only > 0 {
        findings.limitations.push(format!("{reported_only} ambient input(s) are reported only"));
    }

    // Freshness is an independent axis: stale and unknown facts stay visible
    // and can never read as fresh exact support.
    if !receipt.stale_facts.is_empty() {
        findings.stale.push(format!("{} declared stale fact(s)", receipt.stale_facts.len()));
    }
    let facts = || receipt.normalized_facts.rust.iter().chain(&receipt.normalized_facts.oracle);
    let stale_facts = facts().filter(|fact| fact.freshness == Freshness::Stale).count();
    if stale_facts > 0 {
        findings.stale.push(format!("{stale_facts} normalized fact(s) are stale"));
    }
    let unknown_freshness = facts().filter(|fact| fact.freshness == Freshness::Unknown).count();
    if unknown_freshness > 0 {
        findings
            .unproven
            .push(format!("{unknown_freshness} normalized fact(s) have unknown freshness"));
    }

    // Fallback and confidence stay visible: neither can be called exact
    // compiler support.
    let fallbacks = facts().filter(|fact| fact.fallback != FallbackState::None).count();
    if fallbacks > 0 {
        findings.limitations.push(format!("{fallbacks} normalized fact(s) carry a fallback state"));
    }
    let low_confidence = facts().filter(|fact| fact.confidence != Confidence::High).count();
    if low_confidence > 0 {
        findings
            .limitations
            .push(format!("{low_confidence} normalized fact(s) are below high confidence"));
    }

    // Completeness is measured against the receipt's own denominator: every
    // comparison ranges over a named fact, and every named fact is compared.
    if !receipt.dynamic_boundaries.is_empty() {
        findings
            .incomplete
            .push(format!("{} dynamic boundary/boundaries", receipt.dynamic_boundaries.len()));
    }
    // The two boundary arrays share a shape but not a meaning, so they take
    // deliberately different axes.  A dynamic boundary is an incompleteness of
    // this run: the oracle could not reach that construct here, and a later
    // run with more information can. An unsupported effect is a durable
    // statement about what this comparison class can never speak to, so it is
    // both an incompleteness now and a standing limitation on the claim.
    if !receipt.unsupported_effects.is_empty() {
        let unsupported = format!("{} unsupported effect(s)", receipt.unsupported_effects.len());
        findings.incomplete.push(unsupported.clone());
        findings.limitations.push(unsupported);
    }
    let named: BTreeSet<&str> = rust_ids.union(&oracle_ids).copied().collect();
    let compared: BTreeSet<&str> =
        receipt.comparisons.iter().map(|comparison| comparison.fact_id.as_str()).collect();
    let uncovered = compared.difference(&named).count();
    if uncovered > 0 {
        findings.incomplete.push(format!("{uncovered} comparison(s) range over an unnamed fact"));
    }
    let uncompared = named.difference(&compared).count();
    if uncompared > 0 {
        findings.incomplete.push(format!("{uncompared} named fact(s) are not compared"));
    }
    let generated_boundaries =
        count_by(&receipt.generated_inputs, |input| boundary_provenance(input.provenance));
    if generated_boundaries > 0 {
        findings.incomplete.push(format!(
            "{generated_boundaries} generated input(s) have no explicit source provenance"
        ));
    }
    let fact_boundaries = facts().filter(|fact| boundary_provenance(fact.provenance)).count();
    if fact_boundaries > 0 {
        findings.incomplete.push(format!(
            "{fact_boundaries} normalized fact(s) have no explicit source provenance"
        ));
    }

    // An observed provider-behavior change is source metadata this
    // parser-internal observation cannot evaluate; it stays a visible bound.
    if receipt.provider_behavior_changed {
        findings.limitations.push(
            "the receipt reports changed provider behavior, which a parser-internal oracle \
             observation cannot evaluate"
                .to_owned(),
        );
    }

    findings
}

/// Normalize one validated receipt into the landed observation envelope.
fn normalize(receipt: &OracleReceiptV1) -> Result<CompilerProfileObservationV1> {
    let findings = collect_findings(receipt);
    let rust_facts = receipt.normalized_facts.rust.len();
    let oracle_facts = receipt.normalized_facts.oracle.len();

    let terminal = if findings.instrument.is_empty() {
        TerminalState::Completed
    } else {
        TerminalState::instrument_failed(&Findings::join(&findings.instrument))?
    };

    // Zero fact evidence is a distinct typed state; it can never be typed pass
    // or not-applicable.
    let work = if rust_facts == 0 && oracle_facts == 0 {
        WorkDisposition::ZeroWork
    } else {
        WorkDisposition::completed(&format!(
            "{} typed comparison(s) over {rust_facts} Rust and {oracle_facts} oracle fact(s) in \
             comparison class {}",
            receipt.comparisons.len(),
            receipt.comparison_class.tag()
        ))?
    };

    let disposition =
        if !findings.instrument.is_empty() || matches!(work, WorkDisposition::ZeroWork) {
            ObservationDisposition::NotProven
        } else if !findings.blocking.is_empty() {
            ObservationDisposition::Failed
        } else if !findings.unproven.is_empty() {
            ObservationDisposition::NotProven
        } else {
            ObservationDisposition::Pass
        };

    let currentness = if !findings.stale.is_empty() {
        CurrentnessDisposition::Stale
    } else if !findings.instrument.is_empty() || !findings.unproven.is_empty() {
        CurrentnessDisposition::NotProven
    } else {
        CurrentnessDisposition::Current
    };

    let completeness = if !findings.instrument.is_empty() {
        CompletenessDisposition::NotProven
    } else if findings.incomplete.is_empty() {
        CompletenessDisposition::Complete
    } else {
        CompletenessDisposition::partial(&Findings::join(&findings.incomplete))?
    };

    let limitation = if findings.limitations.is_empty() {
        LimitationDisposition::None
    } else {
        LimitationDisposition::accepted_debt(
            &format!(
                "oracle differential receipt in comparison class {}",
                receipt.comparison_class.tag()
            ),
            &Findings::join(&findings.limitations),
        )?
    };

    // The strongest reachable ceiling requires every independent axis to be
    // clean at once; anything else stays internal observed evidence.
    let ceiling = if disposition == ObservationDisposition::Pass
        && currentness == CurrentnessDisposition::Current
        && completeness == CompletenessDisposition::Complete
        && limitation == LimitationDisposition::None
        && terminal == TerminalState::Completed
    {
        ObservedClaimCeiling::new(ClaimCeiling::AcceptedCompatibility)
    } else {
        ObservedClaimCeiling::new(ClaimCeiling::ObservedEvidence)
    };

    let observation = CompilerProfileObservationV1 {
        receipt: CanonicalReceiptReference {
            id: ReceiptId::new(&receipt.receipt_id)?,
            digest: receipt_digest(receipt)?,
            producer: ProducerAndSchemaIdentity::new(
                PRODUCER,
                ReceiptFamily::new(SOURCE_FAMILY)?,
                SchemaVersion::new(SOURCE_SCHEMA_VERSION),
            )?,
        },
        subject: subject_identity(receipt)?,
        class: emitted_class(),
        disposition,
        currentness,
        completeness,
        work,
        limitation,
        ceiling,
        invalidation: InvalidationEvidence::new(vec![
            InvalidationInput::new(
                InvalidationKind::Source,
                "the receipt's source snapshot content hash or path class changed",
            )?,
            InvalidationInput::new(
                InvalidationKind::Dependency,
                "the Rust extractor name, version, fact model, or declared module roots changed",
            )?,
            InvalidationInput::new(
                InvalidationKind::Oracle,
                "the declared Perl interpreter, version, or invocation mode changed",
            )?,
            InvalidationInput::new(
                InvalidationKind::HostEnvironment,
                "the declared or denied environment authority changed",
            )?,
        ])?,
        instrument: InstrumentAndTerminalState::new(INSTRUMENT, terminal)?,
        adapter: AdapterIdentity {
            id: AdapterId::new(ADAPTER_ID)?,
            version: AdapterVersion::new(ADAPTER_VERSION)?,
        },
    };
    observation.validate()?;
    Ok(observation)
}

/// Bind the subject dimensions this adapter can prove.  Every bound dimension
/// is exact and non-transferable: another comparison class, fixture, source
/// snapshot, extractor, Perl oracle, or module-path authority produces a
/// different subject, and the four dimensions the receipt cannot speak to stay
/// explicitly not proven.
fn subject_identity(receipt: &OracleReceiptV1) -> Result<CandidateSubjectIdentity> {
    let mut subject = CandidateSubjectIdentity::not_proven();

    // A redacted private fixture never exposes its bounded source identity; a
    // public test fixture may name it.  The class travels with the identity so
    // a private subject can never be read as a public one.
    let fixture_source = match receipt.source_snapshot.path_class {
        PathClass::PublicTestFixture => {
            format!(" source={}", receipt.source_snapshot.fixture_source)
        }
        PathClass::RedactedPrivateFixture => String::new(),
    };
    subject.bind(
        SubjectDimensionKind::FixtureSeries,
        SubjectDimension::proven(&format!(
            "fixture={} class={} path_class={} content_hash={}{fixture_source}",
            receipt.fixture_id,
            receipt.comparison_class.tag(),
            receipt.source_snapshot.path_class.tag(),
            receipt.source_snapshot.content_hash,
        ))?,
    );

    subject.bind(
        SubjectDimensionKind::Toolchain,
        SubjectDimension::proven(&format!(
            "rust_extractor={}@{} perl={}@{} invocation={}",
            receipt.rust_extractor.name,
            receipt.rust_extractor.version,
            receipt.perl_oracle.interpreter.tag(),
            receipt.perl_oracle.version,
            receipt.perl_oracle.invocation_mode.tag(),
        ))?,
    );

    // Declared module roots are host paths, so only their count and a stable
    // digest cross the boundary; the digest keeps the roots load-bearing
    // without leaking them.  It is carried whole: a truncated prefix would be
    // a hint rather than the exact, non-transferable identity this dimension
    // claims, and truncation buys no privacy a full digest does not already
    // give.
    let mut roots = receipt.module_path_authority.declared_roots.clone();
    roots.sort();
    let roots_digest = sha256_hex(roots.join("\u{1f}").as_bytes());
    subject.bind(
        SubjectDimensionKind::CompilerPolicy,
        SubjectDimension::proven(&format!(
            "fact_model={} module_path_authority={} declared_roots={} roots_digest={} \
             ambient_roots_reported={}",
            receipt.rust_extractor.fact_model,
            receipt.module_path_authority.authority.tag(),
            roots.len(),
            roots_digest,
            receipt.module_path_authority.ambient_roots_reported,
        ))?,
    );

    let mut denied: Vec<&str> = receipt.environment.denied.iter().map(|key| key.tag()).collect();
    denied.sort_unstable();
    let mut declared = receipt.environment.declared.clone();
    declared.sort();
    subject.bind(
        SubjectDimensionKind::ProducerConfiguration,
        SubjectDimension::proven(&format!(
            "source_schema={SOURCE_SCHEMA_TAG} adapter={ADAPTER_ID}@{ADAPTER_VERSION} \
             denied_env=[{}] declared_env=[{}]",
            denied.join(","),
            declared.join(","),
        ))?,
    );

    Ok(subject)
}

/// Adapt one `oracle_receipt.v1` document into a normalized observation.
///
/// The receipt is validated with the production schema, decoded into the
/// closed source vocabulary, normalized into the landed envelope, and finally
/// checked against the registry that declares this adapter, so an observation
/// this adapter could not legally emit never escapes.
pub fn adapt_receipt_value(value: &Value) -> Result<CompilerProfileObservationV1> {
    let receipt = validate_receipt_value(value)?;
    let observation = normalize(&receipt)?;
    oracle_receipt_registry()?
        .validate_observation(&observation)
        .context("the adapted observation must satisfy the registry that declares this adapter")?;
    Ok(observation)
}

/// Adapt one `oracle_receipt.v1` document from JSON text.
pub fn adapt_receipt_json(text: &str) -> Result<CompilerProfileObservationV1> {
    let value: Value =
        serde_json::from_str(text).context("oracle receipt is not well-formed JSON")?;
    adapt_receipt_value(&value)
}

/// Adapt many receipts, keyed by receipt id.  Duplicate receipt ids fail
/// closed: one receipt identity owns exactly one observation.
pub fn adapt_receipts(values: &[Value]) -> Result<BTreeMap<String, CompilerProfileObservationV1>> {
    let mut observations = BTreeMap::new();
    for value in values {
        let observation = adapt_receipt_value(value)?;
        let id = observation.receipt.id.as_str().to_owned();
        if observations.insert(id.clone(), observation).is_some() {
            bail!("receipt id {id:?} appears twice; one receipt identity owns one observation");
        }
    }
    Ok(observations)
}

/// Bounded receipt fixtures used by this crate's proof and by the sibling
/// evidence-set lanes.  They are synthetic public test fixtures: no private
/// path, environment value, launch payload, or source body appears in them.
pub mod fixtures {
    use serde_json::{Value, json};

    /// A fully agreeing, hermetic, complete receipt.
    pub fn agreeing_receipt() -> Value {
        json!({
            "schema_version": "oracle_receipt.v1",
            "receipt_id": "oracle-receipt-0001",
            "comparison_class": "IsaComposition",
            "fixture_id": "isa-composition-basic",
            "source_snapshot": {
                "path_class": "public_test_fixture",
                "fixture_source": "differential_oracle/isa_composition_basic.pl",
                "content_hash": "sha256:2f1c9a"
            },
            "rust_extractor": {
                "name": "perl-semantic-facts",
                "version": "0.8.3",
                "fact_model": "package-sub-table.v1"
            },
            "perl_oracle": {
                "interpreter": "declared_fixture_perl",
                "version": "v5.38.0",
                "invocation_mode": "declared_fixture_command"
            },
            "module_path_authority": {
                "authority": "declared_fixture_root",
                "declared_roots": ["fixtures/differential_oracle/lib"],
                "ambient_roots_reported": false
            },
            "environment": {
                "denied": ["PERL5LIB", "PERL5OPT", "local::lib"],
                "declared": ["PATH"],
                "redacted_values": true
            },
            "ambient_inputs": [],
            "generated_inputs": [],
            "dynamic_boundaries": [],
            "stale_facts": [],
            "unsupported_effects": [],
            "normalized_facts": {
                "rust": [fact("fact-isa-1", "Child::ISA"), fact("fact-isa-2", "Child::new")],
                "oracle": [fact("fact-isa-1", "Child::ISA"), fact("fact-isa-2", "Child::new")]
            },
            "comparisons": [
                comparison("oracle_agrees", "fact-isa-1", "supports_promotion"),
                comparison("oracle_agrees", "fact-isa-2", "supports_promotion")
            ],
            "provider_behavior_changed": false,
            "editor_runtime_dependency": false,
            "redaction": {
                "private_paths_redacted": true,
                "environment_values_redacted": true,
                "raw_launch_payloads_redacted": true
            },
            "claim_boundary": "one fixture, one comparison class, test-only oracle evidence"
        })
    }

    /// One fresh, high-confidence, explicit-source normalized fact.
    pub fn fact(fact_id: &str, name: &str) -> Value {
        json!({
            "fact_id": fact_id,
            "name": name,
            "provenance": "ExplicitSource",
            "confidence": "high",
            "freshness": "fresh",
            "fallback": "none",
            "source_range": {
                "path_class": "public_test_fixture",
                "start_line": 3,
                "start_character": 0,
                "end_line": 3,
                "end_character": 24
            }
        })
    }

    /// One typed comparison row.
    pub fn comparison(result_class: &str, fact_id: &str, promotion_effect: &str) -> Value {
        json!({
            "result_class": result_class,
            "fact_id": fact_id,
            "promotion_effect": promotion_effect,
            "message": "bounded explanatory text that is never parsed for semantics"
        })
    }
}
