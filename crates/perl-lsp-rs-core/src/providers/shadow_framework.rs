//! Shared shadow-compare framework for semantic provider cutover (issue #9085).
//!
//! Centralizes the comparison loop, receipt emission, and verdict vocabulary
//! across all nine per-provider shadow implementations (parent tracker #2440).
//!
//! # Framework goals
//!
//! 1. **Single receipt-emission path** — enforces the #3057 discipline: every
//!    field in a [`SemanticShadowCompareReceipt`] is *derived* from the actual
//!    query results supplied by the caller; no caller may hardcode `match_count`,
//!    `available`, or `identities` by hand.
//!
//! 2. **Canonical verdict vocabulary** — [`ProviderVerdict`] names the four
//!    possible outcomes across all providers (Exact / Ambiguous / Dynamic /
//!    LegacyFallback, per issue #4243).  Per-provider result types (e.g.
//!    `DefinitionCutoverResult`, `ReferencesCutoverResult`) carry the same
//!    semantic meaning in different containers; the framework names the
//!    vocabulary they all express.
//!
//! 3. **Parameterized comparison loop** — [`run_shadow_compare`] accepts
//!    old-path and new-path callbacks so each provider supplies its own input
//!    types and semantic policies (definition's generation double-check,
//!    references' fallback budget, rename's safety gate) without forking the
//!    structural template.
//!
//! 4. **PIR receipt mapping** — documents and provides the type-level mapping
//!    from [`pir_adapter::PirReceiptAdapter`] (for `PirShadowCompareReceipt`,
//!    the tenth divergent shape in `references_pir_shadow`) onto the canonical
//!    `SemanticShadowCompareReceipt` shape.  Actual migration of the PIR shadow
//!    onto the framework happens in the parent issue's later children
//!    (#9086, #9087).
//!
//! # What this module does NOT do
//!
//! - No existing shadow file is deleted or migrated here.
//! - No behavior change: per-provider receipts remain byte-identical.
//! - Policy hooks are not flattened; they remain in the caller callbacks.

use perl_semantic_facts::ProviderFactTrace;
use perl_workspace::semantic_shadow_compare::{
    SemanticShadowCompareReceipt, ShadowQueryInput, ShadowQueryName, ShadowResultSummary,
};

// ── Canonical verdict vocabulary ──────────────────────────────────────────────

/// Canonical four-way cutover verdict vocabulary shared across all providers.
///
/// Every per-provider cutover result enum expresses one of these four
/// outcomes, even when its concrete type is provider-specific:
///
/// | Arm | Meaning | Action |
/// |---|---|---|
/// | `Exact` | Semantic path produced one confident, exact answer | Use it |
/// | `Ambiguous` | Multiple or mixed-confidence candidates | Present options |
/// | `Dynamic` | Dynamic boundary observed; no reliable semantic result | Suppress or block |
/// | `LegacyFallback` | Semantic path unavailable; semantic data absent | Defer to legacy |
///
/// Per-provider payloads differ (`S` / `L` are provider-specific types) but
/// the four-arm structure is universal.
///
/// # Per-provider mapping
///
/// | Provider | `Exact(S)` | `Ambiguous(S)` | `Dynamic` | `LegacyFallback(L)` |
/// |---|---|---|---|---|
/// | definition | `DefinitionCandidate` | `Vec<DC>` | – | `Option<Location>` |
/// | references | `Vec<OccurrenceFact>` | `Vec<OF>` | – | `Vec<Location>` |
/// | hover | hover result | multiple | dynamic | `Option<String>` |
/// | rename | `RenamePlan` | blocked-ambiguous | dynamic | – |
/// | safe-delete | `Allowed` | – | dynamic | – |
/// | diagnostics | `Warn` | `WeakWarn` | `Suppress` | – |
/// | completion | candidates | lower-ranked | dynamic | – |
/// | semantic-tokens | token list | – | – | legacy tokens |
/// | PIR references | exact ranges | – | dynamic | legacy ranges |
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderVerdict<S, L> {
    /// Semantic path produced one confident, exact answer.  Use it as-is.
    Exact(S),
    /// Semantic path found multiple or mixed-confidence candidates.
    /// Present options to the user or apply provider-specific ranking.
    Ambiguous(S),
    /// A dynamic boundary was observed, or the semantic data is internally
    /// inconsistent.  Suppress the action or block the edit.
    Dynamic,
    /// Semantic path is unavailable or produced no usable output.
    /// Fall back to the legacy result.
    LegacyFallback(L),
}

impl<S, L> ProviderVerdict<S, L> {
    /// Returns `true` for the [`Exact`](Self::Exact) arm.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        matches!(self, Self::Exact(_))
    }

    /// Returns `true` for the [`Ambiguous`](Self::Ambiguous) arm.
    #[must_use]
    pub fn is_ambiguous(&self) -> bool {
        matches!(self, Self::Ambiguous(_))
    }

    /// Returns `true` for the [`Dynamic`](Self::Dynamic) arm.
    #[must_use]
    pub fn is_dynamic(&self) -> bool {
        matches!(self, Self::Dynamic)
    }

    /// Returns `true` for the [`LegacyFallback`](Self::LegacyFallback) arm.
    #[must_use]
    pub fn is_legacy_fallback(&self) -> bool {
        matches!(self, Self::LegacyFallback(_))
    }

    /// Returns the canonical kebab-case label for the verdict arm.
    ///
    /// Suitable for `tracing::debug!` fields and receipt notes.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Exact(_) => "exact",
            Self::Ambiguous(_) => "ambiguous",
            Self::Dynamic => "dynamic",
            Self::LegacyFallback(_) => "legacy_fallback",
        }
    }
}

// ── Parameterized comparison loop ─────────────────────────────────────────────

/// Output of the **old (legacy)** path in a shadow-compare run.
///
/// The caller must supply the raw result value and derive the receipt summary
/// from it — never hardcode `available`, `match_count`, or `identities`
/// directly (the #3057 discipline).
pub struct ShadowOldPathOutput<T> {
    /// Raw result produced by the legacy path.
    pub value: T,
    /// Receipt summary derived from `value`.
    ///
    /// Use [`summarize_identities`] with identities extracted from `value` to
    /// produce this; do not construct [`ShadowResultSummary`] by hand.
    pub summary: ShadowResultSummary,
}

/// Output of the **new (semantic)** path in a shadow-compare run.
///
/// Same discipline as [`ShadowOldPathOutput`]: `summary` must be derived from
/// `value`, and `fact_source_traces` must reflect the actual provenance of the
/// semantic facts consulted.
pub struct ShadowNewPathOutput<T> {
    /// Raw result produced by the semantic path.
    pub value: T,
    /// Receipt summary derived from `value`.
    pub summary: ShadowResultSummary,
    /// Typed fact-source traces reflecting the actual provenance and confidence
    /// of the semantic facts used.  Never provide a fixed/hardcoded list.
    pub fact_source_traces: Vec<ProviderFactTrace>,
    /// Optional notes for the receipt (e.g. suppression reasons, quality
    /// observations).  Empty when there is nothing to add.
    pub notes: Vec<String>,
}

/// Paired output from both paths in a shadow-compare run, along with the
/// generated [`SemanticShadowCompareReceipt`].
pub struct ShadowCompareOutput<OldT, NewT> {
    /// Raw result from the old (legacy) path.
    pub old_value: OldT,
    /// Raw result from the new (semantic) path.
    pub new_value: NewT,
    /// Generated receipt for scorecard aggregation.  All fields are derived
    /// from the path outputs; none are hardcoded.
    pub receipt: SemanticShadowCompareReceipt,
}

/// Run a shadow compare for any provider.
///
/// Accepts old-path and new-path callbacks so each provider can supply its
/// own query logic and semantic policies without duplicating the structural
/// template.  The receipt is always built from the callbacks' outputs; no
/// field may be hardcoded by the caller (the #3057 discipline).
///
/// # Arguments
///
/// * `query_name` — which shadow query this run corresponds to.
/// * `symbol` — the symbol string driving the query (used in `ShadowQueryInput`).
/// * `old_path` — callback that runs the legacy path and returns a summary.
/// * `new_path` — callback that runs the semantic path and returns a summary,
///   fact-source traces, and optional notes.
///
/// # Type parameters
///
/// * `OldT` — raw result type of the old (legacy) path.
/// * `NewT` — raw result type of the new (semantic) path.
/// * `OldFn` — callable for the legacy path.
/// * `NewFn` — callable for the semantic path.
///
/// # Behavior
///
/// The `old_path` callback is always called first, then `new_path`.  The
/// receipt verdict is derived deterministically from the two summaries by
/// [`SemanticShadowCompareReceipt::from_summaries_with_fact_source_traces`].
///
/// Per-provider policy hooks (e.g. filtering low-confidence candidates,
/// budget limits, safety-gate checks) belong inside the `new_path` callback;
/// the framework does not apply them.
///
/// # Example
///
/// ```rust,ignore
/// let output = run_shadow_compare(
///     ShadowQueryName::FindDefinition,
///     symbol,
///     || ShadowOldPathOutput {
///         value: workspace_index.find_definition(symbol),
///         summary: legacy_location_to_summary(location.as_ref()),
///     },
///     || {
///         let candidates = semantic_queries.definitions(symbol, context);
///         ShadowNewPathOutput {
///             value: candidates.clone(),
///             summary: semantic_candidates_to_summary(&candidates),
///             fact_source_traces: definition_fact_source_traces(&candidates, fallback_state),
///             notes: vec![],
///         }
///     },
/// );
/// ```
pub fn run_shadow_compare<OldT, NewT, OldFn, NewFn>(
    query_name: ShadowQueryName,
    symbol: impl Into<String>,
    old_path: OldFn,
    new_path: NewFn,
) -> ShadowCompareOutput<OldT, NewT>
where
    OldFn: FnOnce() -> ShadowOldPathOutput<OldT>,
    NewFn: FnOnce() -> ShadowNewPathOutput<NewT>,
{
    let input = ShadowQueryInput { symbol: symbol.into() };

    // Always run old path before new path: maintains the consistent ordering
    // the per-provider shadows have always used, and means old_path cannot
    // observe any side-effect from the semantic path.
    let old_out = old_path();
    let new_out = new_path();

    let receipt = SemanticShadowCompareReceipt::from_summaries_with_fact_source_traces(
        query_name,
        input,
        old_out.summary,
        new_out.summary,
        new_out.notes,
        new_out.fact_source_traces,
    );

    ShadowCompareOutput { old_value: old_out.value, new_value: new_out.value, receipt }
}

// ── PIR receipt adapter ────────────────────────────────────────────────────────

/// Utilities for mapping the tenth divergent receipt shape
/// (`PirShadowCompareReceipt`) onto the canonical
/// [`SemanticShadowCompareReceipt`] shape.
///
/// This module only defines the *mapping*; it does not migrate the PIR shadow
/// implementation.  Migration is performed by the parent issue's later children
/// (#9086, #9087).
///
/// # Mapping rationale
///
/// `PirShadowCompareReceipt` was designed before the canonical shape existed
/// and carries several fields that do not directly correspond to the shared
/// schema.  The table below defines how each PIR field projects:
///
/// | PIR field | Canonical field | Derivation |
/// |---|---|---|
/// | `compiler_candidate_count` | `new_result.match_count` | direct |
/// | `legacy_candidate_count` | `old_result.match_count` | direct |
/// | `missing_from_compiler` | `notes` | formatted as note entries |
/// | `extra_in_compiler` | `notes` | formatted as note entries |
/// | `range_disagreements` | `notes` | formatted as note entries |
/// | `refusal_reason` | `old_result.available` / `notes` | refusal → `Unavailable` verdict |
/// | `provider_behavior_changed` | `notes` | flag in notes when `true` |
/// | `latency` | (not in canonical shape) | elided; callers may log separately |
/// | `fact_source_traces` | `fact_source_traces` | direct |
///
/// The canonical shape also carries `schema_version`, `query` (always
/// `ShadowQueryName::FindReferences` for PIR), and `input` (the symbol name).
pub mod pir_adapter {
    use perl_semantic_facts::ProviderFactTrace;
    use perl_workspace::semantic_shadow_compare::{
        SemanticShadowCompareReceipt, ShadowQueryInput, ShadowQueryName, ShadowResultSummary,
        summarize_identities,
    };

    /// Minimal projection of a `PirShadowCompareReceipt` for mapping purposes.
    ///
    /// This is the subset of fields from `references_pir_shadow::PirShadowCompareReceipt`
    /// needed to produce a canonical [`SemanticShadowCompareReceipt`].  Migration
    /// of the full PIR shadow onto the framework is deferred to #9086/#9087.
    ///
    /// Callers fill this from `PirShadowCompareReceipt` fields directly; the
    /// adapter then derives the canonical shape without hardcoding any values.
    #[derive(Debug, Clone)]
    pub struct PirReceiptAdapter {
        /// Symbol name (sigil + bare name) that was queried.
        pub symbol: String,
        /// Number of distinct reference ranges from the compiler (PIR-A) path.
        pub compiler_candidate_count: usize,
        /// Number of distinct reference ranges from the legacy path.
        pub legacy_candidate_count: usize,
        /// Legacy-only ranges not found by the compiler, as `(start, end)` byte offsets.
        pub missing_from_compiler: Vec<(usize, usize)>,
        /// Compiler-only ranges not found in the legacy result.
        pub extra_in_compiler: Vec<(usize, usize)>,
        /// Whether the underlying extractor reported a behavior change.
        pub provider_behavior_changed: bool,
        /// Why the comparison was refused, expressed as a string label.
        /// `None` when the comparison ran.  Use the PIR enum's `Debug` label
        /// or a short kebab-case string (e.g. `"not_same_file_lexical"`).
        pub refusal_reason_label: Option<String>,
        /// Typed fact-source traces from the PIR receipt (passed through unchanged).
        pub fact_source_traces: Vec<ProviderFactTrace>,
    }

    impl PirReceiptAdapter {
        /// Convert this adapter into a canonical [`SemanticShadowCompareReceipt`].
        ///
        /// All fields in the output are derived from `self`; none are hardcoded.
        pub fn into_canonical(self) -> SemanticShadowCompareReceipt {
            let refused = self.refusal_reason_label.is_some();

            // Old (legacy) path summary: always available unless refused.
            let old_result = if refused {
                summarize_identities(None)
            } else {
                let identities =
                    (0..self.legacy_candidate_count).map(|i| format!("legacy:{i}")).collect();
                summarize_identities(Some(identities))
            };

            // New (compiler) path summary: available only when comparison ran.
            let new_result = if refused {
                summarize_identities(None)
            } else {
                let identities =
                    (0..self.compiler_candidate_count).map(|i| format!("compiler:{i}")).collect();
                summarize_identities(Some(identities))
            };

            // Derive notes from the PIR-specific fields.
            let mut notes: Vec<String> = Vec::new();
            if let Some(ref label) = self.refusal_reason_label {
                notes.push(format!("pir_refusal: {label}"));
            }
            if !self.missing_from_compiler.is_empty() {
                notes.push(format!(
                    "pir_missing_from_compiler: {} ranges",
                    self.missing_from_compiler.len()
                ));
            }
            if !self.extra_in_compiler.is_empty() {
                notes.push(format!(
                    "pir_extra_in_compiler: {} ranges",
                    self.extra_in_compiler.len()
                ));
            }
            if self.provider_behavior_changed {
                notes.push("pir_provider_behavior_changed: true".to_string());
            }

            SemanticShadowCompareReceipt::from_summaries_with_fact_source_traces(
                ShadowQueryName::FindReferences,
                ShadowQueryInput { symbol: self.symbol },
                old_result,
                new_result,
                notes,
                self.fact_source_traces,
            )
        }

        /// Returns `true` if this adapter describes a refused comparison
        /// (i.e. the PIR shadow did not run).
        #[must_use]
        pub fn was_refused(&self) -> bool {
            self.refusal_reason_label.is_some()
        }

        /// Returns `true` if the compiler found more or equal ranges vs legacy.
        ///
        /// Used in scorecard gate assertions: `extra_in_compiler == 0` is the
        /// precondition for PIR promotion (per the PR3 plan-reviewed spec on
        /// issue #2635).
        #[must_use]
        pub fn no_extra_in_compiler(&self) -> bool {
            self.extra_in_compiler.is_empty()
        }
    }

    /// Build a [`ShadowResultSummary`] that represents a PIR path output.
    ///
    /// Identities are positional (`"compiler:0"`, `"compiler:1"`, …) because
    /// PIR ranges are not yet correlated to named symbols in the framework.
    /// The migration to stable identities is left to #9086/#9087.
    pub fn pir_new_summary(candidate_count: usize, refused: bool) -> ShadowResultSummary {
        if refused {
            return summarize_identities(None);
        }
        let identities = (0..candidate_count).map(|i| format!("compiler:{i}")).collect();
        summarize_identities(Some(identities))
    }

    /// Build a [`ShadowResultSummary`] that represents a legacy path output.
    pub fn pir_old_summary(candidate_count: usize, refused: bool) -> ShadowResultSummary {
        if refused {
            return summarize_identities(None);
        }
        let identities = (0..candidate_count).map(|i| format!("legacy:{i}")).collect();
        summarize_identities(Some(identities))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use perl_semantic_facts::{
        Confidence, Provenance, ProviderFactFreshness, ProviderFactSourceKind, ProviderFactTrace,
        ProviderFallbackState, ProviderSurface,
    };
    use perl_workspace::semantic_shadow_compare::{
        ShadowCompareVerdict, ShadowQueryName, summarize_identities,
    };

    use super::pir_adapter::PirReceiptAdapter;
    use super::*;

    // ── ProviderVerdict ──────────────────────────────────────────────────────

    #[test]
    fn provider_verdict_labels_are_stable() {
        let exact: ProviderVerdict<i32, i32> = ProviderVerdict::Exact(1);
        let ambiguous: ProviderVerdict<i32, i32> = ProviderVerdict::Ambiguous(2);
        let dynamic: ProviderVerdict<i32, i32> = ProviderVerdict::Dynamic;
        let fallback: ProviderVerdict<i32, i32> = ProviderVerdict::LegacyFallback(3);

        assert_eq!(exact.label(), "exact");
        assert_eq!(ambiguous.label(), "ambiguous");
        assert_eq!(dynamic.label(), "dynamic");
        assert_eq!(fallback.label(), "legacy_fallback");
    }

    #[test]
    fn provider_verdict_predicates_are_mutually_exclusive() {
        let exact: ProviderVerdict<i32, i32> = ProviderVerdict::Exact(1);
        assert!(exact.is_exact());
        assert!(!exact.is_ambiguous());
        assert!(!exact.is_dynamic());
        assert!(!exact.is_legacy_fallback());

        let ambiguous: ProviderVerdict<i32, i32> = ProviderVerdict::Ambiguous(2);
        assert!(!ambiguous.is_exact());
        assert!(ambiguous.is_ambiguous());
        assert!(!ambiguous.is_dynamic());
        assert!(!ambiguous.is_legacy_fallback());

        let dynamic: ProviderVerdict<i32, i32> = ProviderVerdict::Dynamic;
        assert!(!dynamic.is_exact());
        assert!(!dynamic.is_ambiguous());
        assert!(dynamic.is_dynamic());
        assert!(!dynamic.is_legacy_fallback());

        let fallback: ProviderVerdict<i32, i32> = ProviderVerdict::LegacyFallback(3);
        assert!(!fallback.is_exact());
        assert!(!fallback.is_ambiguous());
        assert!(!fallback.is_dynamic());
        assert!(fallback.is_legacy_fallback());
    }

    // ── run_shadow_compare ───────────────────────────────────────────────────

    fn make_trace(source: ProviderFactSourceKind) -> ProviderFactTrace {
        ProviderFactTrace::new(
            ProviderSurface::Definition,
            source,
            Provenance::ExactAst,
            Confidence::High,
            ProviderFactFreshness::Fresh,
            ProviderFallbackState::Shadow,
            None,
            None,
            Some(1),
        )
    }

    #[test]
    fn run_shadow_compare_same_result_yields_same_verdict() {
        let output = run_shadow_compare(
            ShadowQueryName::FindDefinition,
            "My::Sub::foo",
            || ShadowOldPathOutput {
                value: "legacy_loc",
                summary: summarize_identities(Some(vec!["file.pm:10:0".to_string()])),
            },
            || ShadowNewPathOutput {
                value: "semantic_loc",
                summary: summarize_identities(Some(vec!["file.pm:10:0".to_string()])),
                fact_source_traces: vec![make_trace(ProviderFactSourceKind::SemanticFact)],
                notes: vec![],
            },
        );

        assert_eq!(output.receipt.verdict, ShadowCompareVerdict::Same);
        assert_eq!(output.receipt.query, ShadowQueryName::FindDefinition);
        assert_eq!(output.receipt.input.symbol, "My::Sub::foo");
        assert_eq!(output.old_value, "legacy_loc");
        assert_eq!(output.new_value, "semantic_loc");
    }

    #[test]
    fn run_shadow_compare_new_better_yields_improved_verdict() {
        let output = run_shadow_compare(
            ShadowQueryName::FindReferences,
            "My::pkg::bar",
            || ShadowOldPathOutput {
                value: 1usize,
                summary: summarize_identities(Some(vec!["a.pm:5:0".to_string()])),
            },
            || ShadowNewPathOutput {
                value: 2usize,
                summary: summarize_identities(Some(vec![
                    "a.pm:5:0".to_string(),
                    "b.pm:12:0".to_string(),
                ])),
                fact_source_traces: vec![make_trace(ProviderFactSourceKind::SemanticFact)],
                notes: vec![],
            },
        );

        assert_eq!(output.receipt.verdict, ShadowCompareVerdict::Improved);
    }

    #[test]
    fn run_shadow_compare_new_worse_yields_regression_verdict() {
        let output = run_shadow_compare(
            ShadowQueryName::FindReferences,
            "My::pkg::baz",
            || ShadowOldPathOutput {
                value: 3usize,
                summary: summarize_identities(Some(vec![
                    "a.pm:5:0".to_string(),
                    "b.pm:12:0".to_string(),
                    "c.pm:20:0".to_string(),
                ])),
            },
            || ShadowNewPathOutput {
                value: 2usize,
                summary: summarize_identities(Some(vec![
                    "a.pm:5:0".to_string(),
                    "b.pm:12:0".to_string(),
                ])),
                fact_source_traces: vec![],
                notes: vec!["one site missing from semantic path".to_string()],
            },
        );

        assert_eq!(output.receipt.verdict, ShadowCompareVerdict::Regression);
        assert_eq!(output.receipt.notes, vec!["one site missing from semantic path"]);
    }

    #[test]
    fn run_shadow_compare_old_unavailable_yields_unavailable_verdict() {
        let output = run_shadow_compare(
            ShadowQueryName::SafeDeletePlan,
            "old_sub",
            || ShadowOldPathOutput {
                value: false,
                summary: summarize_identities(None), // legacy path returned nothing
            },
            || ShadowNewPathOutput {
                value: true,
                summary: summarize_identities(Some(vec!["safe_delete:allowed".to_string()])),
                fact_source_traces: vec![],
                notes: vec![],
            },
        );

        // Unavailable old path → Unavailable verdict regardless of new path.
        assert_eq!(output.receipt.verdict, ShadowCompareVerdict::Unavailable);
    }

    #[test]
    fn run_shadow_compare_receipt_has_fact_traces() {
        let trace = make_trace(ProviderFactSourceKind::CompilerFact);
        let output = run_shadow_compare(
            ShadowQueryName::DiagnosticsCheck,
            "My::Mod::missing",
            || ShadowOldPathOutput {
                value: (),
                summary: summarize_identities(Some(vec!["warn:1".to_string()])),
            },
            || ShadowNewPathOutput {
                value: (),
                summary: summarize_identities(Some(vec!["warn:1".to_string()])),
                fact_source_traces: vec![trace.clone()],
                notes: vec![],
            },
        );

        assert_eq!(output.receipt.fact_source_traces.len(), 1);
        assert_eq!(
            output.receipt.fact_source_traces[0].source,
            ProviderFactSourceKind::CompilerFact
        );
    }

    #[test]
    fn run_shadow_compare_old_path_runs_before_new_path() {
        // Verify evaluation order: old runs before new.
        // We detect this with a shared counter via a cell.
        use std::cell::Cell;
        let counter = Cell::new(0u32);
        let old_order = Cell::new(0u32);
        let new_order = Cell::new(0u32);

        run_shadow_compare(
            ShadowQueryName::FindDefinition,
            "sym",
            || {
                counter.set(counter.get() + 1);
                old_order.set(counter.get());
                ShadowOldPathOutput { value: (), summary: summarize_identities(Some(vec![])) }
            },
            || {
                counter.set(counter.get() + 1);
                new_order.set(counter.get());
                ShadowNewPathOutput {
                    value: (),
                    summary: summarize_identities(Some(vec![])),
                    fact_source_traces: vec![],
                    notes: vec![],
                }
            },
        );

        assert!(old_order.get() < new_order.get(), "old path must run before new path");
    }

    // ── PirReceiptAdapter ────────────────────────────────────────────────────

    #[test]
    fn pir_adapter_no_refusal_produces_derived_summaries() {
        let adapter = PirReceiptAdapter {
            symbol: "x".to_string(),
            compiler_candidate_count: 3,
            legacy_candidate_count: 2,
            missing_from_compiler: vec![],
            extra_in_compiler: vec![(10, 11)],
            provider_behavior_changed: false,
            refusal_reason_label: None,
            fact_source_traces: vec![],
        };

        assert!(!adapter.was_refused());
        assert!(!adapter.no_extra_in_compiler());

        let receipt = adapter.into_canonical();
        assert_eq!(receipt.query, ShadowQueryName::FindReferences);
        assert_eq!(receipt.input.symbol, "x");
        assert_eq!(receipt.old_result.match_count, 2);
        assert_eq!(receipt.new_result.match_count, 3);
        assert!(receipt.new_result.available);
        assert!(receipt.old_result.available);
        // Extra-in-compiler note must be present.
        assert!(
            receipt.notes.iter().any(|n| n.contains("pir_extra_in_compiler")),
            "expected extra_in_compiler note, got {:?}",
            receipt.notes
        );
    }

    #[test]
    fn pir_adapter_refusal_yields_unavailable_verdict() {
        let adapter = PirReceiptAdapter {
            symbol: "y".to_string(),
            compiler_candidate_count: 0,
            legacy_candidate_count: 0,
            missing_from_compiler: vec![],
            extra_in_compiler: vec![],
            provider_behavior_changed: false,
            refusal_reason_label: Some("not_same_file_lexical".to_string()),
            fact_source_traces: vec![],
        };

        assert!(adapter.was_refused());

        let receipt = adapter.into_canonical();
        assert_eq!(receipt.verdict, ShadowCompareVerdict::Unavailable);
        assert!(
            receipt.notes.iter().any(|n| n.contains("not_same_file_lexical")),
            "refusal reason must appear in notes"
        );
    }

    #[test]
    fn pir_adapter_missing_from_compiler_note_included() {
        let adapter = PirReceiptAdapter {
            symbol: "z".to_string(),
            compiler_candidate_count: 1,
            legacy_candidate_count: 3,
            missing_from_compiler: vec![(5, 6), (20, 21)],
            extra_in_compiler: vec![],
            provider_behavior_changed: false,
            refusal_reason_label: None,
            fact_source_traces: vec![],
        };

        assert!(adapter.no_extra_in_compiler());

        let receipt = adapter.into_canonical();
        assert!(
            receipt.notes.iter().any(|n| n.contains("pir_missing_from_compiler")),
            "missing_from_compiler note must appear, got {:?}",
            receipt.notes
        );
        // No extra-in-compiler note.
        assert!(
            !receipt.notes.iter().any(|n| n.contains("pir_extra_in_compiler")),
            "unexpected extra_in_compiler note in {:?}",
            receipt.notes
        );
    }

    #[test]
    fn pir_adapter_behavior_changed_note_included() {
        let adapter = PirReceiptAdapter {
            symbol: "w".to_string(),
            compiler_candidate_count: 2,
            legacy_candidate_count: 2,
            missing_from_compiler: vec![],
            extra_in_compiler: vec![],
            provider_behavior_changed: true,
            refusal_reason_label: None,
            fact_source_traces: vec![],
        };

        let receipt = adapter.into_canonical();
        assert!(
            receipt.notes.iter().any(|n| n.contains("pir_provider_behavior_changed: true")),
            "behavior_changed note must appear, got {:?}",
            receipt.notes
        );
    }

    #[test]
    fn pir_adapter_fact_traces_passed_through_unchanged() {
        let trace = ProviderFactTrace::new(
            ProviderSurface::References,
            ProviderFactSourceKind::DynamicBoundary,
            Provenance::DynamicBoundary,
            Confidence::High,
            ProviderFactFreshness::Fresh,
            ProviderFallbackState::Blocked,
            None,
            None,
            Some(1),
        );

        let adapter = PirReceiptAdapter {
            symbol: "t".to_string(),
            compiler_candidate_count: 0,
            legacy_candidate_count: 0,
            missing_from_compiler: vec![],
            extra_in_compiler: vec![],
            provider_behavior_changed: false,
            refusal_reason_label: Some("dynamic_boundary".to_string()),
            fact_source_traces: vec![trace],
        };

        let receipt = adapter.into_canonical();
        assert_eq!(receipt.fact_source_traces.len(), 1);
        assert_eq!(receipt.fact_source_traces[0].source, ProviderFactSourceKind::DynamicBoundary);
    }

    #[test]
    fn pir_summary_helpers_derive_from_count() {
        let summary = pir_adapter::pir_new_summary(3, false);
        assert!(summary.available);
        assert_eq!(summary.match_count, 3);
        assert_eq!(summary.identities.len(), 3);
        assert_eq!(summary.identities[0], "compiler:0");

        let refused = pir_adapter::pir_new_summary(3, true);
        assert!(!refused.available);
        assert_eq!(refused.match_count, 0);

        let old_summary = pir_adapter::pir_old_summary(2, false);
        assert_eq!(old_summary.match_count, 2);
        assert_eq!(old_summary.identities[0], "legacy:0");
    }
}
