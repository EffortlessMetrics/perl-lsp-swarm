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

use super::native::CriticRelatedInformation;
use super::normalized::{
    CriticFindingCandidate, CriticPolicyRetention, CriticSourceIdentity, NormalizedCriticFinding,
    normalize_critic_findings,
};
use super::{
    CriticFindingOrigin, CriticFindingShape, CriticObservedIdentity, Severity,
    native::{CriticFinding, CriticSuppressionMap, NativeCriticRegistry, range_for_byte_span},
};

/// One producer-owned overlap observation declared by a core lint emitter
/// while it still owns the proposition (#11918).
///
/// The checked critic identity, reviewed shape, and critic-scale severity are
/// producer declarations made at the syntax branch that observed the finding.
/// They are never reconstructed from the finished diagnostic, its LSP
/// severity, code string, message, or range: the core producer states the
/// critic-scale fact before that information can collapse.
///
/// Construction is named for the admitted reviewed overlap cohort only. The
/// ordinary core diagnostic keeps its existing severity and behavior; this
/// observation is the independent critic-scale declaration that lets the row
/// merge with its native alias in [`normalize_critic_findings`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltInCriticObservation {
    identity: CriticObservedIdentity<'static>,
    severity: Severity,
    byte_range: (usize, usize),
    message: String,
    explanation: Option<String>,
    /// Producer-owned user-visible remediation of the ordinary twin row
    /// (#12004): the exact suggestion text the ordinary diagnostic rendered.
    suggestion: Option<String>,
    /// Producer-owned related information of the ordinary twin row, as
    /// byte spans over the same source the emitter observed.
    related_information: Vec<((usize, usize), String)>,
}

impl BuiltInCriticObservation {
    /// Built-in PL404 comparison against an explicit literal `undef`.
    #[must_use]
    pub fn pl404_literal_undef_comparison(
        severity: Severity,
        byte_range: (usize, usize),
        message: String,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_literal_undef_comparison(),
            severity,
            byte_range,
            message,
            explanation,
        )
    }

    /// Built-in PL404 comparison whose operand may be undefined through data
    /// flow. This shape has no native alias and stays a distinct finding.
    #[must_use]
    pub fn pl404_potentially_undef_comparison(
        severity: Severity,
        byte_range: (usize, usize),
        message: String,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_potentially_undef_comparison(),
            severity,
            byte_range,
            message,
            explanation,
        )
    }

    /// Built-in PL601 backtick command execution.
    #[must_use]
    pub fn pl601_backtick(
        severity: Severity,
        byte_range: (usize, usize),
        message: String,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_backtick_exec(),
            severity,
            byte_range,
            message,
            explanation,
        )
    }

    /// Built-in PL601 `qx` command execution.
    #[must_use]
    pub fn pl601_qx(
        severity: Severity,
        byte_range: (usize, usize),
        message: String,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_qx_exec(),
            severity,
            byte_range,
            message,
            explanation,
        )
    }

    /// Built-in PL606 `readpipe` command execution.
    #[must_use]
    pub fn pl606_readpipe(
        severity: Severity,
        byte_range: (usize, usize),
        message: String,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_readpipe_exec(),
            severity,
            byte_range,
            message,
            explanation,
        )
    }

    /// Built-in PL603 `system` process execution.
    #[must_use]
    pub fn pl603_system(
        severity: Severity,
        byte_range: (usize, usize),
        message: String,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_system_call(),
            severity,
            byte_range,
            message,
            explanation,
        )
    }

    /// Built-in PL604 `exec` process replacement.
    #[must_use]
    pub fn pl604_exec(
        severity: Severity,
        byte_range: (usize, usize),
        message: String,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_exec_call(),
            severity,
            byte_range,
            message,
            explanation,
        )
    }

    fn new(
        identity: CriticObservedIdentity<'static>,
        severity: Severity,
        byte_range: (usize, usize),
        message: impl Into<String>,
        explanation: Option<String>,
    ) -> Self {
        Self {
            identity,
            severity,
            byte_range,
            message: message.into(),
            explanation,
            suggestion: None,
            related_information: Vec::new(),
        }
    }

    /// Checked producer-declared critic identity.
    #[must_use]
    pub const fn identity(&self) -> CriticObservedIdentity<'static> {
        self.identity
    }

    /// Critic-scale severity declared by the core producer.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Exact producer-observed byte range.
    #[must_use]
    pub const fn byte_range(&self) -> (usize, usize) {
        self.byte_range
    }

    /// Producer-owned message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Producer-owned detailed explanation.
    #[must_use]
    pub fn explanation(&self) -> Option<&str> {
        self.explanation.as_deref()
    }

    /// Carry the ordinary twin row's exact suggestion text (#12004).
    ///
    /// This is a producer declaration of content that already exists on the
    /// ordinary diagnostic; it is never composed or reworded here.
    #[must_use]
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Carry one producer-owned related-information entry as a byte span over
    /// the observed source (#12004).
    #[must_use]
    pub fn with_related_information(
        mut self,
        byte_range: (usize, usize),
        message: impl Into<String>,
    ) -> Self {
        self.related_information.push((byte_range, message.into()));
        self
    }

    /// Producer-owned suggestion text declared for the ordinary twin row.
    #[must_use]
    pub fn suggestion(&self) -> Option<&str> {
        self.suggestion.as_deref()
    }

    /// Producer-owned related information declared for the ordinary twin row.
    #[must_use]
    pub fn related_information(&self) -> &[((usize, usize), String)] {
        &self.related_information
    }

    fn into_candidate(
        self,
        source: &str,
        source_identity: CriticSourceIdentity,
    ) -> CriticFindingCandidate {
        let range = range_for_byte_span(source, self.byte_range.0, self.byte_range.1);
        let related_information = self
            .related_information
            .iter()
            .map(|(span, message)| CriticRelatedInformation {
                range: range_for_byte_span(source, span.0, span.1),
                message: message.clone(),
            })
            .collect();
        CriticFindingCandidate::new(
            self.identity,
            source_identity,
            self.severity,
            range,
            self.message,
            self.explanation,
        )
        .with_remediation(self.suggestion, related_information)
    }
}

/// Convert producer-declared built-in overlap observations into normalization
/// candidates bound to one exact logical source subject (#11918).
///
/// Only the line/column coordinates are completed here, from the same source
/// text and byte span the emitter observed — the same coordinate math the
/// native path applies to its byte spans. Identity, shape, and severity are
/// already producer declarations and are never derived here.
#[must_use]
pub fn built_in_observation_candidates(
    observations: impl IntoIterator<Item = BuiltInCriticObservation>,
    source: &str,
    source_identity: CriticSourceIdentity,
) -> Vec<CriticFindingCandidate> {
    observations
        .into_iter()
        .map(|observation| observation.into_candidate(source, source_identity))
        .collect()
}

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
/// Order: canonical alias merge, then critic-owned severity threshold and
/// include/exclude retention, then scoped suppression. The severity and
/// include/exclude filters are Critic-producer policy: they strip the Critic
/// contribution from a merged row while an independently emitted built-in core
/// proposition survives ([`NormalizedCriticFinding::retained_under_critic_policy`],
/// #13798), and they still remove critic-owned rows wholesale - as does an
/// exclude naming the built-in code itself, which revokes the core
/// proposition and keeps "exclude one spelling removes the whole alias set"
/// true. Deterministic output order is owned entirely by
/// [`normalize_critic_findings`]. Filtering merged rows here is what makes
/// "exclude/suppress one spelling" unable to leave a registered sibling
/// spelling behind.
#[must_use]
pub fn normalize_with_native_policy(
    candidates: impl IntoIterator<Item = CriticFindingCandidate>,
    policy: &NativeCriticPolicy<'_>,
) -> Vec<NormalizedCriticFinding> {
    normalize_critic_findings(candidates)
        .into_iter()
        .filter_map(|finding| {
            let retention = critic_policy_retention(&finding, policy);
            finding.retained_under_critic_policy(retention)
        })
        .filter(|finding| !policy.suppressions.suppresses_normalized(finding))
        .collect()
}

/// Evaluate critic-owned severity and include/exclude policy for one merged
/// row (#13798).
///
/// An admitted row keeps every contributor. A rejected row strips its Critic
/// contributors while an independently owned built-in core proposition
/// survives, unless the exclude set names the built-in code itself - the one
/// spelling that revokes the core row.
fn critic_policy_retention(
    finding: &NormalizedCriticFinding,
    policy: &NativeCriticPolicy<'_>,
) -> CriticPolicyRetention {
    let severity_admitted =
        severity_passes_threshold(finding.severity(), policy.severity_threshold);
    if severity_admitted && include_exclude_admits(finding, policy.include, policy.exclude) {
        return CriticPolicyRetention::Admitted;
    }

    let core_named_by_exclude = policy.exclude.iter().any(|excluded| {
        finding.contributors().iter().any(|contributor| {
            contributor.identity().origin() == CriticFindingOrigin::BuiltInDiagnostic
                && contributor.identity().code() == excluded.as_str()
        })
    });
    if core_named_by_exclude {
        CriticPolicyRetention::RemoveRow
    } else {
        CriticPolicyRetention::StripCritic
    }
}

/// Whether one normalized row survives the configured severity threshold.
///
/// Severities are perlcritic threshold values (1-5, higher is stricter); a
/// row survives when its merged severity meets or exceeds the threshold.
fn severity_passes_threshold(severity: Severity, threshold: u8) -> bool {
    severity as u8 >= threshold
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
        NativeCriticPolicy, critic_policy_retention, critic_source_identity_for_uri,
        native_finding_candidates, normalize_with_native_policy,
    };
    use crate::tooling::perl_critic::{
        CriticFinding, CriticFindingCandidate, CriticFindingOrigin, CriticFindingShape,
        CriticObservedIdentity, CriticPolicyRetention, CriticSourceIdentity, CriticSuppressionMap,
        Severity, normalize_critic_findings,
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

    // --- producer-owned built-in overlap observations (#11918) ---

    const OVERLAP_SOURCE: &str = "use strict;\nuse warnings;\nsystem('ls');\n";

    /// A native finding whose range is derived from the same source bytes a
    /// built-in emitter observed, so both producers agree on the exact range.
    fn native_finding_at_source_bytes(
        rule_id: &str,
        shape: CriticFindingShape,
        severity: Severity,
        byte_range: (usize, usize),
    ) -> CriticFinding {
        CriticFinding {
            rule_id: rule_id.to_string(),
            category: super::super::native::CriticCategory::Security,
            severity,
            range: super::range_for_byte_span(OVERLAP_SOURCE, byte_range.0, byte_range.1),
            message: "native finding".to_string(),
            explanation: "native explanation".to_string(),
            suppression_key: rule_id.to_string(),
            observed_shape: shape,
            related: Vec::new(),
            fix: None,
        }
    }

    /// The `system('ls')` byte span inside [`OVERLAP_SOURCE`].
    fn system_call_bytes() -> (usize, usize) {
        let start = OVERLAP_SOURCE.find("system('ls')").unwrap_or(0);
        (start, start + "system('ls')".len())
    }

    fn merged_rows_for_one_system_call() -> Vec<super::NormalizedCriticFinding> {
        let bytes = system_call_bytes();
        let (native_candidates, unresolved) = native_finding_candidates(
            [native_finding_at_source_bytes(
                "native.security.system_exec",
                CriticFindingShape::SystemCall,
                Severity::Harsh,
                bytes,
            )],
            subject(),
        );
        assert!(unresolved.is_empty());

        let built_in = super::built_in_observation_candidates(
            [super::BuiltInCriticObservation::pl603_system(
                Severity::Harsh,
                bytes,
                "system() executes a shell command. Ensure input is sanitized.".to_string(),
                None,
            )],
            OVERLAP_SOURCE,
            subject(),
        );

        normalize_critic_findings(native_candidates.into_iter().chain(built_in))
    }

    /// #13304: the published diagnostic and the code action that resolves it
    /// must render one composition. `message()` stays the normalized problem
    /// statement; `user_visible_message()` is the complete text, so a surface
    /// that renders the bare message drops the producer's remediation and
    /// stops matching the row a client already received.
    #[test]
    fn merged_row_user_visible_message_carries_the_producer_remediation() {
        let bytes = system_call_bytes();
        let (native_candidates, unresolved) = native_finding_candidates(
            [native_finding_at_source_bytes(
                "native.security.system_exec",
                CriticFindingShape::SystemCall,
                Severity::Harsh,
                bytes,
            )],
            subject(),
        );
        assert!(unresolved.is_empty());

        let suggestion = "Pass system() a list so no shell is involved";
        let built_in = super::built_in_observation_candidates(
            [super::BuiltInCriticObservation::pl603_system(
                Severity::Harsh,
                bytes,
                "system() executes a shell command. Ensure input is sanitized.".to_string(),
                None,
            )
            .with_suggestion(suggestion)],
            OVERLAP_SOURCE,
            subject(),
        );

        let rows = normalize_critic_findings(native_candidates.into_iter().chain(built_in));
        assert_eq!(rows.len(), 1, "the reviewed alias pair must merge into one row");
        let Some(row) = rows.first() else { return };

        assert_eq!(row.remediation_suggestion(), Some(suggestion));
        assert!(
            !row.message().contains("Suggestion:"),
            "the normalized problem statement must not already embed the remediation"
        );
        assert_eq!(
            row.user_visible_message(),
            format!("{}\nSuggestion: {suggestion}", row.message()),
            "the complete user-visible text must append the producer's remediation"
        );
    }

    #[test]
    fn built_in_overlap_observations_merge_with_every_reviewed_native_alias() {
        let bytes = system_call_bytes();
        for (rule_id, shape, observe, canonical_id) in [
            (
                "native.security.system_exec",
                CriticFindingShape::SystemCall,
                super::BuiltInCriticObservation::pl603_system
                    as fn(
                        Severity,
                        (usize, usize),
                        String,
                        Option<String>,
                    ) -> super::BuiltInCriticObservation,
                "critic.security.system_call",
            ),
            (
                "native.security.system_exec",
                CriticFindingShape::ExecCall,
                super::BuiltInCriticObservation::pl604_exec
                    as fn(
                        Severity,
                        (usize, usize),
                        String,
                        Option<String>,
                    ) -> super::BuiltInCriticObservation,
                "critic.security.exec_call",
            ),
            (
                "native.security.qx_readpipe",
                CriticFindingShape::Qx,
                super::BuiltInCriticObservation::pl601_qx
                    as fn(
                        Severity,
                        (usize, usize),
                        String,
                        Option<String>,
                    ) -> super::BuiltInCriticObservation,
                "critic.security.qx_exec",
            ),
            (
                "native.security.qx_readpipe",
                CriticFindingShape::Readpipe,
                super::BuiltInCriticObservation::pl606_readpipe
                    as fn(
                        Severity,
                        (usize, usize),
                        String,
                        Option<String>,
                    ) -> super::BuiltInCriticObservation,
                "critic.security.readpipe_exec",
            ),
            (
                "native.security.backtick_exec",
                CriticFindingShape::Backtick,
                super::BuiltInCriticObservation::pl601_backtick
                    as fn(
                        Severity,
                        (usize, usize),
                        String,
                        Option<String>,
                    ) -> super::BuiltInCriticObservation,
                "critic.security.backtick_exec",
            ),
            (
                "native.common.undef_comparison",
                CriticFindingShape::LiteralUndefComparison,
                super::BuiltInCriticObservation::pl404_literal_undef_comparison
                    as fn(
                        Severity,
                        (usize, usize),
                        String,
                        Option<String>,
                    ) -> super::BuiltInCriticObservation,
                "critic.common.undef_comparison",
            ),
        ] {
            let (native_candidates, unresolved) = native_finding_candidates(
                [native_finding_at_source_bytes(rule_id, shape, Severity::Harsh, bytes)],
                subject(),
            );
            assert!(unresolved.is_empty(), "{rule_id}/{shape:?} must resolve");

            let built_in = super::built_in_observation_candidates(
                [observe(Severity::Harsh, bytes, "built-in finding".to_string(), None)],
                OVERLAP_SOURCE,
                subject(),
            );

            let rows = normalize_critic_findings(native_candidates.into_iter().chain(built_in));
            assert_eq!(rows.len(), 1, "{rule_id}/{shape:?} must merge into one row");
            let row = &rows[0];
            assert_eq!(row.canonical_id(), Some(canonical_id));
            assert_eq!(row.contributors().len(), 2, "both producer identities retained");
            assert!(
                row.contributors().iter().any(|contributor| {
                    contributor.identity().origin() == CriticFindingOrigin::BuiltInDiagnostic
                }),
                "built-in contributor identity retained"
            );
            assert!(
                row.contributors().iter().any(|contributor| {
                    contributor.identity().origin() == CriticFindingOrigin::NativeCritic
                }),
                "native contributor identity retained"
            );
        }
    }

    #[test]
    fn producer_declared_security_severity_survives_without_invented_conflict() {
        // Discriminates against a reverse mapping from the LSP `WARNING`
        // severity: mapping Warning -> Stern would flip the merged severity to
        // Stern AND raise a conflict flag the producers never declared.
        let rows = merged_rows_for_one_system_call();
        let row = &rows[0];
        assert!(!row.has_severity_conflict(), "both producers declared Harsh");
        assert_eq!(row.severity(), Severity::Harsh, "declared severity wins, not a mapped one");
    }

    #[test]
    fn pl404_literal_declares_stern_matching_its_native_alias() {
        let bytes = system_call_bytes();
        let (native_candidates, _) = native_finding_candidates(
            [native_finding_at_source_bytes(
                "native.common.undef_comparison",
                CriticFindingShape::LiteralUndefComparison,
                Severity::Stern,
                bytes,
            )],
            subject(),
        );
        let built_in = super::built_in_observation_candidates(
            [super::BuiltInCriticObservation::pl404_literal_undef_comparison(
                Severity::Stern,
                bytes,
                "undef comparison".to_string(),
                None,
            )],
            OVERLAP_SOURCE,
            subject(),
        );

        let rows = normalize_critic_findings(native_candidates.into_iter().chain(built_in));
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].has_severity_conflict());
        assert_eq!(rows[0].severity(), Severity::Stern);
    }

    #[test]
    fn wrong_shape_cannot_merge_backtick_against_native_qx() {
        let bytes = system_call_bytes();
        let (native_qx, _) = native_finding_candidates(
            [native_finding_at_source_bytes(
                "native.security.qx_readpipe",
                CriticFindingShape::Qx,
                Severity::Harsh,
                bytes,
            )],
            subject(),
        );
        let built_in_backtick = super::built_in_observation_candidates(
            [super::BuiltInCriticObservation::pl601_backtick(
                Severity::Harsh,
                bytes,
                "built-in backtick finding".to_string(),
                None,
            )],
            OVERLAP_SOURCE,
            subject(),
        );

        let rows = normalize_critic_findings(native_qx.into_iter().chain(built_in_backtick));
        assert_eq!(
            rows.len(),
            2,
            "an emitter that declared the wrong reviewed shape must not merge"
        );
    }

    #[test]
    fn pl404_potentially_undef_shape_never_merges_with_the_literal_native_alias() {
        let bytes = system_call_bytes();
        let (native_literal, _) = native_finding_candidates(
            [native_finding_at_source_bytes(
                "native.common.undef_comparison",
                CriticFindingShape::LiteralUndefComparison,
                Severity::Stern,
                bytes,
            )],
            subject(),
        );
        let built_in_potential = super::built_in_observation_candidates(
            [super::BuiltInCriticObservation::pl404_potentially_undef_comparison(
                Severity::Stern,
                bytes,
                "maybe-undef comparison".to_string(),
                None,
            )],
            OVERLAP_SOURCE,
            subject(),
        );

        let rows = normalize_critic_findings(native_literal.into_iter().chain(built_in_potential));
        assert_eq!(rows.len(), 2, "data-flow shape is a deliberately distinct finding");
    }

    #[test]
    fn readpipe_system_and_exec_retain_separate_canonical_findings() {
        let bytes = system_call_bytes();
        let mut candidates = Vec::new();
        for (rule_id, shape) in [
            ("native.security.qx_readpipe", CriticFindingShape::Readpipe),
            ("native.security.system_exec", CriticFindingShape::SystemCall),
            ("native.security.system_exec", CriticFindingShape::ExecCall),
        ] {
            let (native, _) = native_finding_candidates(
                [native_finding_at_source_bytes(rule_id, shape, Severity::Harsh, bytes)],
                subject(),
            );
            candidates.extend(native);
        }

        let rows = normalize_critic_findings(candidates);
        assert_eq!(rows.len(), 3, "same range and severity must not merge distinct identities");
        let canonical_ids: Vec<&str> = rows.iter().filter_map(|row| row.canonical_id()).collect();
        for expected in [
            "critic.security.readpipe_exec",
            "critic.security.system_call",
            "critic.security.exec_call",
        ] {
            assert!(canonical_ids.contains(&expected), "missing {expected} in {canonical_ids:?}");
        }
    }

    #[test]
    fn built_in_only_and_native_only_rows_remain_exactly_one_row_each() {
        let rows = merged_rows_for_one_system_call();
        assert_eq!(rows.len(), 1);

        let built_in_only = super::built_in_observation_candidates(
            [super::BuiltInCriticObservation::pl603_system(
                Severity::Harsh,
                system_call_bytes(),
                "built-in only".to_string(),
                None,
            )],
            OVERLAP_SOURCE,
            subject(),
        );
        let rows = normalize_critic_findings(built_in_only);
        assert_eq!(rows.len(), 1, "core-only row stays one valid row");
        assert_eq!(rows[0].contributors().len(), 1);

        let (native_only, _) = native_finding_candidates(
            [native_finding_at_source_bytes(
                "native.security.system_exec",
                CriticFindingShape::SystemCall,
                Severity::Harsh,
                system_call_bytes(),
            )],
            subject(),
        );
        let rows = normalize_critic_findings(native_only);
        assert_eq!(rows.len(), 1, "native-only row stays one valid row");
        assert_eq!(rows[0].contributors().len(), 1);
    }

    #[test]
    fn overlap_candidate_permutation_is_byte_equivalent() {
        let bytes = system_call_bytes();
        let (native, _) = native_finding_candidates(
            [native_finding_at_source_bytes(
                "native.security.system_exec",
                CriticFindingShape::SystemCall,
                Severity::Harsh,
                bytes,
            )],
            subject(),
        );
        let built_in = super::built_in_observation_candidates(
            [super::BuiltInCriticObservation::pl603_system(
                Severity::Harsh,
                bytes,
                "built-in finding".to_string(),
                None,
            )],
            OVERLAP_SOURCE,
            subject(),
        );

        let forward = normalize_critic_findings(native.clone().into_iter().chain(built_in.clone()));
        let backward = normalize_critic_findings(built_in.into_iter().chain(native));
        assert_eq!(forward, backward);
    }

    #[test]
    fn different_generation_or_source_never_merges_overlap_candidates() {
        let bytes = system_call_bytes();
        let built_in = super::built_in_observation_candidates(
            [super::BuiltInCriticObservation::pl603_system(
                Severity::Harsh,
                bytes,
                "built-in finding".to_string(),
                None,
            )],
            OVERLAP_SOURCE,
            subject(),
        );
        let (native, _) = native_finding_candidates(
            [native_finding_at_source_bytes(
                "native.security.system_exec",
                CriticFindingShape::SystemCall,
                Severity::Harsh,
                bytes,
            )],
            CriticSourceIdentity::new(SOURCE_KEY, GENERATION + 1),
        );

        let rows = normalize_critic_findings(built_in.into_iter().chain(native));
        assert_eq!(rows.len(), 2, "stale generations must not merge");
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
        assert_eq!(row.severity(), Severity::Stern, "most severe contributor wins");
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
    fn core_code_exclude_selects_remove_row_with_a_critic_only_control() -> Result<(), String> {
        let include = Vec::new();
        let suppressions = CriticSuppressionMap::from_source("");

        let overlap = merged_rows_for_one_system_call();
        let overlap_row = overlap.first().ok_or_else(|| {
            "the reviewed core/native overlap fixture must normalize to one row".to_string()
        })?;
        let exclude_core = vec!["PL603".to_string()];
        let overlap_policy = NativeCriticPolicy::new(1, &include, &exclude_core, &suppressions);
        if critic_policy_retention(overlap_row, &overlap_policy) != CriticPolicyRetention::RemoveRow
        {
            return Err(
                "excluding the independently owned core code must explicitly revoke the whole row"
                    .to_string(),
            );
        }

        let critic_only = normalize_critic_findings(qx_native_candidates());
        let critic_only_row = critic_only
            .first()
            .ok_or_else(|| "the critic-only control must normalize to one row".to_string())?;
        let exclude_compat = vec!["PL601".to_string()];
        let critic_only_policy =
            NativeCriticPolicy::new(1, &include, &exclude_compat, &suppressions);
        if critic_policy_retention(critic_only_row, &critic_only_policy)
            != CriticPolicyRetention::StripCritic
        {
            return Err(
                "without a core contributor, an alias exclusion strips Critic rather than claiming core revocation"
                    .to_string(),
            );
        }
        if !normalize_with_native_policy(qx_native_candidates(), &critic_only_policy).is_empty() {
            return Err(
                "the critic-only row must still be removed after its sole contribution is stripped"
                    .to_string(),
            );
        }
        Ok(())
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
}
