//! Production semantic boundary for native critic diagnostics (#7475).
//!
//! This module is the sole production composition entrypoint from native
//! producer outputs into the normalized logical finding set. Producers hand
//! checked, producer-owned observations in; policy (severity threshold,
//! include/exclude, scoped suppression) applies exactly once after canonical
//! alias merging; output ordering stays deterministic.
//!
//! External Perl::Critic process results are structurally excluded: no input
//! of this module can be constructed from a subprocess violation without
//! passing through the registry-checked identity constructors.

use super::normalized::{
    CriticFindingCandidate, CriticSourceIdentity, NormalizedCriticFinding,
    normalize_critic_findings,
};
use super::{
    CriticFindingOrigin, CriticFindingShape, CriticObservedIdentity,
    native::{CriticFinding, CriticSuppressionMap, NativeCriticRegistry},
};
use crate::providers::Diagnostic as InternalDiagnostic;
use perl_parser_core::position::{Position, Range};

/// A native finding whose `(rule_id, observed_shape)` pair has no registered
/// producer disposition. Normalization fails closed instead of guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedNativeFindingIdentity {
    rule_id: String,
    shape: CriticFindingShape,
}

impl UnresolvedNativeFindingIdentity {
    /// Rule ID whose emitted shape is not registered.
    #[must_use]
    pub fn rule_id(&self) -> &str {
        &self.rule_id
    }

    /// Emitted shape that has no reviewed disposition.
    #[must_use]
    pub const fn shape(&self) -> CriticFindingShape {
        self.shape
    }
}

/// Build an opaque per-process logical source identity for one document
/// generation.
///
/// The source key derives from the URI, so equal generations from different
/// documents cannot merge. Every candidate of one normalization call shares
/// it; output ordering never depends on hash values.
#[must_use]
pub fn critic_source_identity_for_uri(uri: &str, generation: u32) -> CriticSourceIdentity {
    let uri_hash = super::hash_content(uri);
    let (high, low) = (uri_hash.to_be_bytes(), (!uri_hash).to_be_bytes());
    let mut source_key = [0u8; 16];
    source_key[..8].copy_from_slice(&high);
    source_key[8..].copy_from_slice(&low);
    CriticSourceIdentity::new(source_key, u64::from(generation))
}

/// Post-merge native critic policy applied once per normalized logical finding.
///
/// Every field is an explicit value supplied by the caller; this type never
/// reads mutable server configuration or selects an engine.
#[derive(Debug)]
pub struct NativeCriticPolicy<'a> {
    severity_threshold: u8,
    include: &'a [String],
    exclude: &'a [String],
    suppressions: &'a CriticSuppressionMap,
}

impl<'a> NativeCriticPolicy<'a> {
    /// Build one explicit policy application for one semantic subject.
    #[must_use]
    pub const fn new(
        severity_threshold: u8,
        include: &'a [String],
        exclude: &'a [String],
        suppressions: &'a CriticSuppressionMap,
    ) -> Self {
        Self { severity_threshold, include, exclude, suppressions }
    }
}

/// Resolve one emitted native finding's checked identity.
///
/// The `(rule_id, observed_shape)` pair must appear in the producer-owned
/// disposition table owned by [`NativeCriticRegistry`]; unregistered pairs
/// fail closed. General-shape rules additionally pass the shared identity
/// authority's reviewed-shape guard.
fn resolve_native_identity(
    finding: &CriticFinding,
) -> Result<CriticObservedIdentity<'static>, UnresolvedNativeFindingIdentity> {
    let rule_id = finding.rule_id.as_str();
    let shape = finding.observed_shape;

    let unresolved = || UnresolvedNativeFindingIdentity { rule_id: rule_id.to_string(), shape };

    // The disposition table's rule IDs are `'static`, so the resolved identity
    // can outlive this call for the general-shape constructor.
    let registered_rule_id = NativeCriticRegistry::identity_dispositions()
        .iter()
        .find(|disposition| disposition.rule_id() == rule_id && disposition.shape() == shape)
        .map(|disposition| disposition.rule_id());

    let Some(registered_rule_id) = registered_rule_id else {
        return Err(unresolved());
    };

    match (rule_id, shape) {
        (_, CriticFindingShape::General) => {
            CriticObservedIdentity::general(CriticFindingOrigin::NativeCritic, registered_rule_id)
                .map_err(|_| unresolved())
        }
        ("native.common.undef_comparison", CriticFindingShape::LiteralUndefComparison) => {
            Ok(CriticObservedIdentity::native_literal_undef_comparison())
        }
        ("native.security.backtick_exec", CriticFindingShape::Backtick) => {
            Ok(CriticObservedIdentity::native_backtick_exec())
        }
        ("native.security.qx_readpipe", CriticFindingShape::Qx) => {
            Ok(CriticObservedIdentity::native_qx_exec())
        }
        ("native.security.qx_readpipe", CriticFindingShape::Readpipe) => {
            Ok(CriticObservedIdentity::native_readpipe_exec())
        }
        ("native.security.system_exec", CriticFindingShape::SystemCall) => {
            Ok(CriticObservedIdentity::native_system_call())
        }
        ("native.security.system_exec", CriticFindingShape::ExecCall) => {
            Ok(CriticObservedIdentity::native_exec_call())
        }
        _ => Err(unresolved()),
    }
}

/// Convert raw native findings into normalization candidates bound to one
/// exact logical source subject.
///
/// Findings whose emission did not declare a registered producer-owned
/// identity are rejected, not guessed: they are returned separately so the
/// caller can account for them while well-formed findings still normalize.
/// One malformed rule therefore cannot silently blank a whole diagnostic set.
pub fn native_finding_candidates(
    findings: impl IntoIterator<Item = CriticFinding>,
    source_identity: CriticSourceIdentity,
) -> (Vec<CriticFindingCandidate>, Vec<UnresolvedNativeFindingIdentity>) {
    let mut candidates = Vec::new();
    let mut unresolved = Vec::new();
    for finding in findings {
        match resolve_native_identity(&finding) {
            Ok(identity) => candidates.push(CriticFindingCandidate::with_fix_availability(
                identity,
                source_identity,
                finding.severity,
                finding.range,
                finding.message,
                Some(finding.explanation),
                finding.fix.is_some(),
            )),
            Err(failure) => unresolved.push(failure),
        }
    }
    (candidates, unresolved)
}

/// Convert raw native findings into candidates for one production subject,
/// accounting for every rejection.
///
/// This is the production entrypoint: both diagnostic call sites take their
/// candidates from here, so a finding rejected for a missing producer
/// disposition is always logged and counted rather than silently dropped.
pub fn native_finding_candidates_with_accounting(
    subject: &str,
    findings: impl IntoIterator<Item = CriticFinding>,
    source_identity: CriticSourceIdentity,
) -> Vec<CriticFindingCandidate> {
    let (candidates, unresolved) = native_finding_candidates(findings, source_identity);
    account_unresolved_native_identities(subject, &unresolved);
    candidates
}

/// Account for native findings rejected at one production subject because
/// their emitted `(rule_id, observed_shape)` pair has no registered producer
/// disposition.
///
/// Production callers must route every rejection through this accounting
/// instead of discarding the list: a future undeclared emission shape must
/// surface as a diagnosable condition, never silently vanish from the product
/// normalized set. Returns the number of rejected findings so callers can
/// assert or aggregate it.
pub fn account_unresolved_native_identities(
    subject: &str,
    unresolved: &[UnresolvedNativeFindingIdentity],
) -> usize {
    for failure in unresolved {
        tracing::warn!(
            subject = %subject,
            rule_id = %failure.rule_id(),
            shape = ?failure.shape(),
            "native critic finding rejected: emitted shape has no registered producer disposition (#7475)"
        );
    }
    unresolved.len()
}

/// Normalize candidates and apply native policy exactly once post-merge.
///
/// Order: canonical alias merge, then severity threshold, then
/// include/exclude over approved aliases, then scoped suppression.
/// Deterministic output order is owned entirely by
/// [`normalize_critic_findings`]. Filtering here, on merged rows,
/// is what makes "exclude/suppress one spelling" unable to leave a registered
/// sibling spelling behind. Rows whose contributors all declared core-scale
/// severities bypass the perlcritic threshold, which never governed them
/// (#11918).
#[must_use]
pub fn normalize_with_native_policy(
    candidates: impl IntoIterator<Item = CriticFindingCandidate>,
    policy: &NativeCriticPolicy<'_>,
) -> Vec<NormalizedCriticFinding> {
    normalize_critic_findings(candidates)
        .into_iter()
        .filter(|finding| finding.severity().passes_perlcritic_threshold(policy.severity_threshold))
        .filter(|finding| include_exclude_admits(finding, policy.include, policy.exclude))
        .filter(|finding| !policy.suppressions.suppresses_normalized(finding))
        .collect()
}

/// Convert identity-carrying built-in lint diagnostics into normalization
/// candidates bound to one exact logical source subject (#11918).
///
/// Built-in lint producers declare their checked built-in identity and their
/// own core-diagnostic-scale severity at emission. Only diagnostics carrying
/// such a declaration become candidates; every other diagnostic passes through
/// unchanged so unrelated providers are unaffected.
///
/// Byte ranges are re-expressed as parser positions with the same line/column
/// derivation the native rules use (`position_for_byte_offset` semantics), so
/// a core emission and its native counterpart share an exact merge range.
pub fn builtin_emission_candidates(
    diagnostics: impl IntoIterator<Item = InternalDiagnostic>,
    doc_text: &str,
    source_identity: CriticSourceIdentity,
) -> (Vec<CriticFindingCandidate>, Vec<InternalDiagnostic>) {
    let mut candidates = Vec::new();
    let mut passthrough = Vec::new();
    for diagnostic in diagnostics {
        let Some(identity) = diagnostic.observed_identity.clone() else {
            passthrough.push(diagnostic);
            continue;
        };
        let (start, end) = diagnostic.range;
        candidates.push(CriticFindingCandidate::for_core_diagnostic(
            identity.observed(),
            source_identity,
            diagnostic.severity,
            Range {
                start: position_for_byte_offset(doc_text, start),
                end: position_for_byte_offset(doc_text, end),
            },
            diagnostic.message,
            None,
        ));
    }
    (candidates, passthrough)
}

/// Derive a parser position from a byte offset with native-rule parity.
///
/// Mirrors `native::position_for_byte_offset`: lines count newline bytes and
/// columns count chars since the last newline. The identical derivation is
/// what lets reviewed alias pairs merge on their complete range.
fn position_for_byte_offset(content: &str, offset: usize) -> Position {
    let offset = offset.min(content.len());
    let prefix = &content[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = prefix.rfind('\n').map_or(0, |idx| idx + 1);
    let column = content[line_start..offset].chars().count();

    Position {
        byte: offset,
        line: u32::try_from(line).unwrap_or(u32::MAX),
        column: u32::try_from(column).unwrap_or(u32::MAX),
    }
}

/// Alias-aware include/exclude admission for one normalized logical row.
///
/// Include entries admit any row carrying an approved alias with that code;
/// exclude entries reject any row carrying one.
fn include_exclude_admits(
    finding: &NormalizedCriticFinding,
    include: &[String],
    exclude: &[String],
) -> bool {
    let admits = |policy: &String| {
        policy.as_str() == finding.public_code()
            || finding.approved_aliases().iter().any(|alias| alias.code() == policy.as_str())
    };

    !exclude.iter().any(admits) && (include.is_empty() || include.iter().any(admits))
}

#[cfg(test)]
mod tests {
    use perl_parser_core::position::{Position, Range};

    use super::{
        NativeCriticPolicy, critic_source_identity_for_uri, native_finding_candidates,
        normalize_with_native_policy,
    };
    use crate::providers::Diagnostic as InternalDiagnostic;
    use crate::tooling::perl_critic::{
        CriticFinding, CriticFindingCandidate, CriticFindingOrigin, CriticFindingShape,
        CriticObservedIdentity, CriticSourceIdentity, CriticSuppressionMap, Severity,
        normalize_critic_findings,
    };

    const GENERATION: u64 = 7;
    const SOURCE_KEY: [u8; 16] = [9; 16];

    fn range_at(line: u32) -> Range {
        let line_byte = u32::from(line) * 10;
        Range {
            start: Position { byte: usize::try_from(line_byte).unwrap_or(0), line, column: 0 },
            end: Position { byte: usize::try_from(line_byte + 4).unwrap_or(4), line, column: 4 },
        }
    }

    fn native_finding(
        rule_id: &str,
        shape: CriticFindingShape,
        severity: Severity,
    ) -> CriticFinding {
        CriticFinding {
            rule_id: rule_id.to_string(),
            category: super::super::native::CriticCategory::Security,
            severity,
            range: range_at(1),
            message: "native finding".to_string(),
            explanation: "native explanation".to_string(),
            suppression_key: rule_id.to_string(),
            observed_shape: shape,
            related: Vec::new(),
            fix: None,
        }
    }

    fn subject() -> CriticSourceIdentity {
        CriticSourceIdentity::new(SOURCE_KEY, GENERATION)
    }

    fn qx_native_candidates() -> Vec<CriticFindingCandidate> {
        let (candidates, unresolved) = native_finding_candidates(
            [native_finding(
                "native.security.qx_readpipe",
                CriticFindingShape::Qx,
                Severity::Harsh,
            )],
            subject(),
        );
        assert!(unresolved.is_empty());
        candidates
    }

    fn pl601_qx_candidate() -> CriticFindingCandidate {
        CriticFindingCandidate::new(
            CriticObservedIdentity::built_in_qx_exec(),
            subject(),
            Severity::Stern,
            range_at(1),
            "built-in finding",
            Some("built-in explanation".to_string()),
        )
    }

    #[test]
    fn general_rules_resolve_through_producer_dispositions() {
        let (candidates, unresolved) = native_finding_candidates(
            [native_finding(
                "native.testing.require_use_strict",
                CriticFindingShape::General,
                Severity::Harsh,
            )],
            subject(),
        );

        assert!(unresolved.is_empty());
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].identity().origin(), CriticFindingOrigin::NativeCritic);
        assert_eq!(candidates[0].identity().code(), "native.testing.require_use_strict");
    }

    #[test]
    fn combined_rules_resolve_through_reviewed_named_identities() {
        for (rule_id, shape, alias_code) in [
            ("native.security.qx_readpipe", CriticFindingShape::Qx, "PL601"),
            ("native.security.qx_readpipe", CriticFindingShape::Readpipe, "PL606"),
            ("native.security.system_exec", CriticFindingShape::SystemCall, "PL603"),
            ("native.security.system_exec", CriticFindingShape::ExecCall, "PL604"),
        ] {
            let (candidates, unresolved) = native_finding_candidates(
                [native_finding(rule_id, shape, Severity::Stern)],
                subject(),
            );
            assert!(unresolved.is_empty(), "{rule_id}/{shape:?} must resolve");
            assert_eq!(candidates[0].identity().code(), rule_id);
            assert_eq!(candidates[0].identity().shape(), shape);

            let rows = normalize_critic_findings(candidates);
            assert_eq!(rows.len(), 1);
            assert!(rows[0].canonical_id().is_some());
            assert!(
                rows[0].approved_aliases().iter().any(|alias| alias.code() == alias_code),
                "{rule_id}/{shape:?} must carry the reviewed {alias_code} alias"
            );
        }
    }

    #[test]
    fn unregistered_rule_or_shape_is_rejected_not_guessed() {
        let (candidates, unresolved) = native_finding_candidates(
            [
                native_finding(
                    "native.does.not_exist",
                    CriticFindingShape::General,
                    Severity::Harsh,
                ),
                native_finding(
                    "native.common.undef_comparison",
                    CriticFindingShape::PotentiallyUndefComparison,
                    Severity::Harsh,
                ),
            ],
            subject(),
        );

        assert!(candidates.is_empty(), "nothing may be guessed into a candidate");
        assert_eq!(unresolved.len(), 2);
        assert_eq!(unresolved[0].rule_id(), "native.does.not_exist");
        assert_eq!(unresolved[0].shape(), CriticFindingShape::General);
        assert_eq!(unresolved[1].shape(), CriticFindingShape::PotentiallyUndefComparison);
    }

    #[test]
    fn undeclared_emission_shapes_surface_through_accounting_not_vanish() {
        // An emission whose shape the producer never declared must reach
        // production accounting as a countable, attributable condition; a
        // future rule regression may not silently shrink the diagnostic set.
        let (_, unresolved) = native_finding_candidates(
            [
                native_finding(
                    "native.security.system_exec",
                    CriticFindingShape::General,
                    Severity::Stern,
                ),
                native_finding(
                    "native.security.qx_readpipe",
                    CriticFindingShape::Qx,
                    Severity::Harsh,
                ),
            ],
            subject(),
        );

        let accounted =
            super::account_unresolved_native_identities("file:///subject.pm", &unresolved);
        assert_eq!(accounted, 1, "exactly the undeclared rejection is accounted");
        assert_eq!(accounted, unresolved.len());
    }

    #[test]
    fn production_entrypoint_logs_each_rejected_emission_shape() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct SharedBuf(Arc<Mutex<Vec<u8>>>);

        impl SharedBuf {
            fn snapshot(&self) -> String {
                let bytes = self.0.lock().map(|guard| guard.clone()).unwrap_or_default();
                String::from_utf8(bytes).unwrap_or_default()
            }
        }

        impl std::io::Write for SharedBuf {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                if let Ok(mut guard) = self.0.lock() {
                    guard.extend_from_slice(buf);
                }
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let buffer = SharedBuf::default();
        let writer_buffer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || writer_buffer.clone())
            .finish();

        let declared =
            native_finding("native.security.qx_readpipe", CriticFindingShape::Qx, Severity::Harsh);
        let undeclared = native_finding(
            "native.security.system_exec",
            CriticFindingShape::General,
            Severity::Stern,
        );

        tracing::subscriber::with_default(subscriber, || {
            let candidates = super::native_finding_candidates_with_accounting(
                "file:///pin-test.pm",
                [declared, undeclared],
                subject(),
            );

            assert_eq!(candidates.len(), 1, "only the declared shape becomes a candidate");
            assert_eq!(candidates[0].identity().code(), "native.security.qx_readpipe");
        });

        let captured = buffer.snapshot();
        assert!(
            captured.contains("no registered producer disposition"),
            "the accounting warn must fire for the undeclared shape; got: {captured:?}"
        );
        assert!(
            captured.contains("native.security.system_exec"),
            "the warn must attribute the rejected rule; got: {captured:?}"
        );
    }

    #[test]
    fn reviewed_core_native_alias_becomes_one_logical_row_with_all_contributors() {
        let normalized = normalize_critic_findings(
            qx_native_candidates().into_iter().chain([pl601_qx_candidate()]),
        );

        assert_eq!(normalized.len(), 1, "registered aliases must merge");
        let row = &normalized[0];
        assert_eq!(row.contributors().len(), 2);
        assert!(row.canonical_id().is_some());
        assert!(row.has_severity_conflict(), "contributor severities disagree");
        assert_eq!(
            row.severity().perlcritic_severity(),
            Some(Severity::Stern),
            "most severe contributor wins"
        );
        assert!(!row.has_available_fix());
        assert!(row.approved_aliases().iter().any(|alias| alias.code() == "PL601"));
    }

    #[test]
    fn coincident_unregistered_findings_remain_distinct_rows() {
        let (strict, _) = native_finding_candidates(
            [native_finding(
                "native.testing.require_use_strict",
                CriticFindingShape::General,
                Severity::Harsh,
            )],
            subject(),
        );
        let (warnings, _) = native_finding_candidates(
            [native_finding(
                "native.testing.require_use_warnings",
                CriticFindingShape::General,
                Severity::Harsh,
            )],
            subject(),
        );

        let rows = normalize_critic_findings(strict.into_iter().chain(warnings));
        assert_eq!(rows.len(), 2, "range/message coincidence must not invent identity");
    }

    #[test]
    fn equal_generations_across_documents_never_merge() {
        let other_document = CriticSourceIdentity::new([8; 16], GENERATION);
        let same_doc = qx_native_candidates();
        let (other_doc, _) = native_finding_candidates(
            [native_finding(
                "native.security.qx_readpipe",
                CriticFindingShape::Qx,
                Severity::Harsh,
            )],
            other_document,
        );

        let normalized = normalize_critic_findings(same_doc.into_iter().chain(other_doc));
        assert_eq!(normalized.len(), 2, "different source keys cannot merge");
    }

    #[test]
    fn policy_applies_once_after_merge_and_suppression_hits_the_logical_row() {
        // The compatibility selector `PL603` must reach the merged logical row;
        // producer-side raw filtering could never honor it.
        let suppressions = CriticSuppressionMap::from_source("## no critic PL603\nsystem('ls');\n");
        let include: Vec<String> = Vec::new();
        let exclude: Vec<String> = Vec::new();

        let (system_rows, _) = native_finding_candidates(
            [native_finding(
                "native.security.system_exec",
                CriticFindingShape::SystemCall,
                Severity::Stern,
            )],
            subject(),
        );
        let policy = NativeCriticPolicy::new(1, &include, &exclude, &suppressions);
        assert!(
            normalize_with_native_policy(system_rows, &policy).is_empty(),
            "compatibility suppression must suppress the canonical logical row"
        );

        assert_eq!(
            normalize_with_native_policy(qx_native_candidates(), &policy).len(),
            1,
            "unrelated rules are not suppressed"
        );
    }

    #[test]
    fn exclusion_by_any_approved_spelling_removes_the_logical_row() {
        let include: Vec<String> = Vec::new();
        let suppressions = CriticSuppressionMap::from_source("");
        let exclude_native_rule = vec!["native.security.qx_readpipe".to_string()];
        let exclude_compat_alias = vec!["PL601".to_string()];

        let by_native_rule =
            NativeCriticPolicy::new(1, &include, &exclude_native_rule, &suppressions);
        assert!(normalize_with_native_policy(qx_native_candidates(), &by_native_rule).is_empty());

        let by_compat_alias =
            NativeCriticPolicy::new(1, &include, &exclude_compat_alias, &suppressions);
        assert!(
            normalize_with_native_policy(
                qx_native_candidates().into_iter().chain([pl601_qx_candidate()]),
                &by_compat_alias,
            )
            .is_empty(),
            "excluding one spelling must remove the whole alias set"
        );
    }

    #[test]
    fn candidate_order_cannot_change_normalized_output() {
        let native_side = qx_native_candidates();
        let forward = normalize_critic_findings(
            native_side.clone().into_iter().chain([pl601_qx_candidate()]),
        );
        let backward =
            normalize_critic_findings(std::iter::once(pl601_qx_candidate()).chain(native_side));

        assert_eq!(forward, backward);
    }

    #[test]
    fn source_identity_binds_uri_and_generation_opaquely() {
        let identity = critic_source_identity_for_uri("file:///a.pm", 3);
        assert_ne!(identity.source_key(), [0u8; 16]);
        assert_eq!(identity.generation(), 3);
        assert_ne!(
            identity.source_key(),
            critic_source_identity_for_uri("file:///b.pm", 3).source_key()
        );
    }

    // --- #11918: reviewed core/native alias rows merge before projection ---

    use perl_diagnostics::codes::DiagnosticSeverity as CoreSeverity;

    /// A built-in system() emission exactly as the core lint declares it.
    fn pl603_core_diagnostic() -> InternalDiagnostic {
        InternalDiagnostic {
            range: (0, 14),
            severity: CoreSeverity::Warning,
            code: Some("PL603".to_string()),
            message: "system() executes a shell command. Ensure input is sanitized.".to_string(),
            related_information: Vec::new(),
            tags: Vec::new(),
            suggestion: None,
            fixable: false,
            observed_identity: Some(
                crate::tooling::perl_critic::CriticObservedIdentity::built_in_system_call().into(),
            ),
        }
    }

    fn native_system_candidates() -> Vec<CriticFindingCandidate> {
        let (candidates, unresolved) = native_finding_candidates(
            [native_finding(
                "native.security.system_exec",
                CriticFindingShape::SystemCall,
                Severity::Stern,
            )],
            subject(),
        );
        assert!(unresolved.is_empty());
        candidates
    }

    /// A native system() finding on the same span the core diagnostic covers.
    ///
    /// `native_finding` above pins its range on line 1; the #11918 merge tests
    /// need both producers to observe one identical span.
    fn native_system_candidates_on_core_span() -> Vec<CriticFindingCandidate> {
        let mut finding = native_finding(
            "native.security.system_exec",
            CriticFindingShape::SystemCall,
            Severity::Stern,
        );
        finding.range = Range {
            start: Position { byte: 0, line: 0, column: 0 },
            end: Position { byte: 14, line: 0, column: 14 },
        };
        let (candidates, unresolved) = native_finding_candidates([finding], subject());
        assert!(unresolved.is_empty());
        candidates
    }

    fn open_policy() -> NativeCriticPolicy<'static> {
        let include: Vec<String> = Vec::new();
        let exclude: Vec<String> = Vec::new();
        let suppressions: &'static CriticSuppressionMap =
            Box::leak(Box::new(CriticSuppressionMap::from_source("")));
        NativeCriticPolicy::new(1, include.leak(), exclude.leak(), suppressions)
    }

    #[test]
    fn system_document_yields_one_logical_row_with_both_contributor_identities() {
        let doc_text = "system(\"ls -la\");";
        let (core_candidates, passthrough) =
            super::builtin_emission_candidates([pl603_core_diagnostic()], doc_text, subject());
        assert!(passthrough.is_empty(), "the declared emission must become a candidate");

        let policy = open_policy();
        let rows = normalize_with_native_policy(
            native_system_candidates_on_core_span().into_iter().chain(core_candidates),
            &policy,
        );

        assert_eq!(rows.len(), 1, "reviewed aliases must merge into one logical row");
        let row = &rows[0];
        let identities: Vec<_> =
            row.contributors().iter().map(|contributor| contributor.identity()).collect();
        assert!(
            identities.iter().any(|identity| {
                identity.origin() == CriticFindingOrigin::BuiltInDiagnostic
                    && identity.code() == "PL603"
            }),
            "the PL603 contributor identity must be retained: {identities:?}"
        );
        assert!(
            identities.iter().any(|identity| {
                identity.origin() == CriticFindingOrigin::NativeCritic
                    && identity.code() == "native.security.system_exec"
            }),
            "the native contributor identity must be retained: {identities:?}"
        );
        assert!(!row.has_severity_conflict(), "cross-scale claims are not a conflict fact");
        assert_eq!(row.public_code(), "PL603", "built-in presentation wins");
    }

    #[test]
    fn suppression_by_compat_spelling_removes_the_merged_row_across_producers() {
        let doc_text = "## no critic PL603\nsystem(\"ls -la\");";
        let (core_candidates, _) =
            super::builtin_emission_candidates([pl603_core_diagnostic()], doc_text, subject());
        let suppressions = CriticSuppressionMap::from_source(doc_text);
        let include: Vec<String> = Vec::new();
        let exclude: Vec<String> = Vec::new();
        let policy = NativeCriticPolicy::new(1, &include, &exclude, &suppressions);

        let rows = normalize_with_native_policy(
            native_system_candidates_on_core_span().into_iter().chain(core_candidates),
            &policy,
        );
        assert!(rows.is_empty(), "one approved spelling must remove the whole logical row");
    }

    #[test]
    fn exclusion_of_any_approved_spelling_removes_the_whole_merged_row() {
        let doc_text = "system(\"ls -la\");";
        let (core_candidates, _) =
            super::builtin_emission_candidates([pl603_core_diagnostic()], doc_text, subject());
        let include: Vec<String> = Vec::new();
        let suppressions = CriticSuppressionMap::from_source("");

        for spelling in ["PL603", "native.security.system_exec"] {
            let exclude = vec![spelling.to_string()];
            let policy = NativeCriticPolicy::new(1, &include, &exclude, &suppressions);
            let rows = normalize_with_native_policy(
                native_system_candidates_on_core_span().into_iter().chain(core_candidates.clone()),
                &policy,
            );
            assert!(rows.is_empty(), "excluding {spelling} must remove the whole row");
        }
    }

    #[test]
    fn undeclared_builtin_diagnostics_pass_through_without_inventing_identity() {
        let mut undeclared = pl603_core_diagnostic();
        undeclared.observed_identity = None;
        undeclared.code = Some("PL602".to_string());

        let (candidates, passthrough) =
            super::builtin_emission_candidates([undeclared], "$SIG{__WARN__} = sub {}", subject());

        assert!(candidates.is_empty(), "coincidence must not invent an identity");
        assert_eq!(passthrough.len(), 1);
        assert!(passthrough[0].observed_identity.is_none());
    }

    #[test]
    fn builtin_emission_ranges_share_native_position_derivation_for_merging() {
        // The merge key compares complete ranges. The built-in converter derives
        // line/column from byte offsets with the same rule as native rules, so
        // an emission and its native counterpart on the same span are eligible.
        let doc_text = "system(\"ls -la\");";
        let (mut core_candidates, _) =
            super::builtin_emission_candidates([pl603_core_diagnostic()], doc_text, subject());
        let native_candidates = native_system_candidates_on_core_span();

        let merged = normalize_critic_findings(
            core_candidates.drain(..).chain(native_candidates).collect::<Vec<_>>(),
        );
        assert_eq!(merged.len(), 1, "same-span alias emissions must merge");
        assert_eq!(merged[0].contributors().len(), 2);
    }
}
