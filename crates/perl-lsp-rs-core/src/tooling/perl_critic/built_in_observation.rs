//! Producer-owned built-in critic overlap observations (#11918).
//!
//! A core lint emitter that owns a reviewed overlap with a native critic rule
//! declares the critic identity, reviewed shape, and critic-scale severity
//! directly at the syntax branch that observed the proposition. Nothing in
//! this type can be reconstructed later from an internal diagnostic, an LSP
//! severity, a code string, or a range: construction is only available through
//! named constructors admitted for the reviewed overlap cohort, and every
//! field is private.
//!
//! The critic-scale [`Severity`] argument is declared by the emitter itself.
//! It is deliberately not derived from any diagnostic severity scale; the
//! producer makes the declaration before the LSP-scale value could collapse
//! distinct perlcritic buckets (Stern and Harsh both project to LSP WARNING).

use super::normalized::{CriticFindingCandidate, CriticSourceIdentity};
use super::{CriticObservedIdentity, Severity};

/// One checked, producer-owned built-in critic observation (#11918).
///
/// Construction is named and checked for the admitted overlap cohort only:
///
/// | Constructor | Reviewed identity |
/// |-------------|-------------------|
/// | [`Self::literal_undef_comparison`] | PL404 literal undef comparison |
/// | [`Self::potentially_undef_comparison`] | PL404 potentially-undef comparison |
/// | [`Self::backtick_exec`] | PL601 backtick |
/// | [`Self::qx_exec`] | PL601 qx |
/// | [`Self::readpipe_exec`] | PL606 readpipe |
/// | [`Self::system_call`] | PL603 system |
/// | [`Self::exec_call`] | PL604 exec |
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltInCriticObservation {
    identity: CriticObservedIdentity<'static>,
    severity: Severity,
    range: (usize, usize),
    message: String,
    explanation: Option<String>,
}

impl BuiltInCriticObservation {
    /// Built-in PL404 comparison against an explicit literal `undef`.
    #[must_use]
    pub fn literal_undef_comparison(
        severity: Severity,
        range: (usize, usize),
        message: impl Into<String>,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_literal_undef_comparison(),
            severity,
            range,
            message.into(),
            explanation,
        )
    }

    /// Built-in PL404 comparison whose operand may be undefined through data flow.
    #[must_use]
    pub fn potentially_undef_comparison(
        severity: Severity,
        range: (usize, usize),
        message: impl Into<String>,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_potentially_undef_comparison(),
            severity,
            range,
            message.into(),
            explanation,
        )
    }

    /// Built-in PL601 backtick command execution.
    #[must_use]
    pub fn backtick_exec(
        severity: Severity,
        range: (usize, usize),
        message: impl Into<String>,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_backtick_exec(),
            severity,
            range,
            message.into(),
            explanation,
        )
    }

    /// Built-in PL601 `qx` command execution.
    #[must_use]
    pub fn qx_exec(
        severity: Severity,
        range: (usize, usize),
        message: impl Into<String>,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_qx_exec(),
            severity,
            range,
            message.into(),
            explanation,
        )
    }

    /// Built-in PL606 `readpipe` command execution.
    #[must_use]
    pub fn readpipe_exec(
        severity: Severity,
        range: (usize, usize),
        message: impl Into<String>,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_readpipe_exec(),
            severity,
            range,
            message.into(),
            explanation,
        )
    }

    /// Built-in PL603 `system` process execution.
    #[must_use]
    pub fn system_call(
        severity: Severity,
        range: (usize, usize),
        message: impl Into<String>,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_system_call(),
            severity,
            range,
            message.into(),
            explanation,
        )
    }

    /// Built-in PL604 `exec` process replacement.
    #[must_use]
    pub fn exec_call(
        severity: Severity,
        range: (usize, usize),
        message: impl Into<String>,
        explanation: Option<String>,
    ) -> Self {
        Self::new(
            CriticObservedIdentity::built_in_exec_call(),
            severity,
            range,
            message.into(),
            explanation,
        )
    }

    const fn new(
        identity: CriticObservedIdentity<'static>,
        severity: Severity,
        range: (usize, usize),
        message: String,
        explanation: Option<String>,
    ) -> Self {
        Self { identity, severity, range, message, explanation }
    }

    /// Checked exact built-in producer identity.
    #[must_use]
    pub const fn identity(&self) -> CriticObservedIdentity<'static> {
        self.identity
    }

    /// Producer-declared critic-scale severity.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Exact observed source byte span.
    #[must_use]
    pub const fn range(&self) -> (usize, usize) {
        self.range
    }

    /// User-facing message reported by the emitting lint.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Optional detailed explanation reported by the emitting lint.
    #[must_use]
    pub fn explanation(&self) -> Option<&str> {
        self.explanation.as_deref()
    }

    /// Bind this observation to one exact logical source/generation as a
    /// normalization candidate.
    ///
    /// The byte span observed at the emission branch is resolved into the
    /// exact multi-coordinate source range with the same position authority
    /// the native candidates use, so alias merging compares identical range
    /// identity. Remediation availability stays producer-owned by the
    /// ordinary core diagnostic surface; the observation itself carries none.
    #[must_use]
    pub fn into_candidate(
        self,
        source: &str,
        source_identity: CriticSourceIdentity,
    ) -> CriticFindingCandidate {
        let (start, end) = self.range;
        CriticFindingCandidate::with_fix_availability(
            self.identity,
            source_identity,
            self.severity,
            super::native::range_for_byte_span(source, start, end),
            self.message,
            self.explanation,
            false,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::BuiltInCriticObservation;
    use crate::tooling::perl_critic::{
        CriticCategory, CriticFinding, CriticFindingOrigin, CriticFindingShape,
        CriticSourceIdentity, Severity, native_finding_candidates, normalize_critic_findings,
    };

    const GENERATION: u64 = 7;
    const SOURCE_KEY: [u8; 16] = [9; 16];
    const SOURCE: &str = "system('ls');";

    fn span_at(offset: usize) -> (usize, usize) {
        let end = offset + 6;
        assert!(end <= SOURCE.len(), "test spans must stay inside SOURCE");
        (offset, end)
    }

    fn subject() -> CriticSourceIdentity {
        CriticSourceIdentity::new(SOURCE_KEY, GENERATION)
    }

    #[derive(Debug, Clone, Copy)]
    enum Member {
        LiteralUndefComparison,
        PotentiallyUndefComparison,
        Backtick,
        Qx,
        Readpipe,
        System,
        Exec,
    }

    impl Member {
        fn all() -> [Self; 7] {
            [
                Self::LiteralUndefComparison,
                Self::PotentiallyUndefComparison,
                Self::Backtick,
                Self::Qx,
                Self::Readpipe,
                Self::System,
                Self::Exec,
            ]
        }

        fn declared_severity(self) -> Severity {
            match self {
                Self::LiteralUndefComparison | Self::PotentiallyUndefComparison => Severity::Stern,
                Self::Backtick | Self::Qx | Self::Readpipe | Self::System | Self::Exec => {
                    Severity::Harsh
                }
            }
        }

        fn expected_code(self) -> &'static str {
            match self {
                Self::LiteralUndefComparison | Self::PotentiallyUndefComparison => "PL404",
                Self::Backtick | Self::Qx => "PL601",
                Self::Readpipe => "PL606",
                Self::System => "PL603",
                Self::Exec => "PL604",
            }
        }

        fn expected_shape(self) -> CriticFindingShape {
            match self {
                Self::LiteralUndefComparison => CriticFindingShape::LiteralUndefComparison,
                Self::PotentiallyUndefComparison => CriticFindingShape::PotentiallyUndefComparison,
                Self::Backtick => CriticFindingShape::Backtick,
                Self::Qx => CriticFindingShape::Qx,
                Self::Readpipe => CriticFindingShape::Readpipe,
                Self::System => CriticFindingShape::SystemCall,
                Self::Exec => CriticFindingShape::ExecCall,
            }
        }

        fn observation(self) -> BuiltInCriticObservation {
            let (severity, range) = (self.declared_severity(), span_at(0));
            match self {
                Self::LiteralUndefComparison => BuiltInCriticObservation::literal_undef_comparison(
                    severity,
                    range,
                    "message",
                    Some("explanation".to_string()),
                ),
                Self::PotentiallyUndefComparison => {
                    BuiltInCriticObservation::potentially_undef_comparison(
                        severity, range, "message", None,
                    )
                }
                Self::Backtick => {
                    BuiltInCriticObservation::backtick_exec(severity, range, "message", None)
                }
                Self::Qx => BuiltInCriticObservation::qx_exec(severity, range, "message", None),
                Self::Readpipe => {
                    BuiltInCriticObservation::readpipe_exec(severity, range, "message", None)
                }
                Self::System => {
                    BuiltInCriticObservation::system_call(severity, range, "message", None)
                }
                Self::Exec => BuiltInCriticObservation::exec_call(severity, range, "message", None),
            }
        }
    }

    fn native_finding(rule_id: &str, shape: CriticFindingShape) -> CriticFinding {
        // Mirrors exactly what `range_for_byte_span(SOURCE, 0, 6)` resolves
        // to so alias merging sees identical range identity.
        let range = perl_parser_core::position::Range {
            start: perl_parser_core::position::Position { byte: 0, line: 0, column: 0 },
            end: perl_parser_core::position::Position { byte: 6, line: 0, column: 6 },
        };
        CriticFinding {
            rule_id: rule_id.to_string(),
            category: CriticCategory::Security,
            severity: Severity::Harsh,
            range,
            message: "native finding".to_string(),
            explanation: "native explanation".to_string(),
            suppression_key: rule_id.to_string(),
            observed_shape: shape,
            related: Vec::new(),
            fix: None,
        }
    }

    fn native_system_candidate(
        source: CriticSourceIdentity,
    ) -> super::super::normalized::CriticFindingCandidate {
        native_finding_candidates(
            [native_finding("native.security.system_exec", CriticFindingShape::SystemCall)],
            source,
        )
        .0
        .remove(0)
    }

    #[test]
    fn every_named_constructor_pins_the_exact_reviewed_identity() {
        for member in Member::all() {
            let observation = member.observation();
            assert_eq!(observation.identity().origin(), CriticFindingOrigin::BuiltInDiagnostic);
            assert_eq!(observation.identity().code(), member.expected_code());
            assert_eq!(observation.identity().shape(), member.expected_shape());
            assert_eq!(observation.severity(), member.declared_severity());
            assert_eq!(observation.range(), span_at(0));
        }
    }

    #[test]
    fn observations_resolve_through_the_checked_identity_registry() {
        for member in Member::all() {
            let rows =
                normalize_critic_findings([member.observation().into_candidate(SOURCE, subject())]);
            assert_eq!(rows.len(), 1);
            assert!(
                rows[0].canonical_id().is_some(),
                "{} must resolve to a registered canonical finding",
                member.expected_code()
            );
        }
    }

    #[test]
    fn observation_candidates_carry_no_remediation_claim() {
        let candidate = Member::System.observation().into_candidate(SOURCE, subject());
        let rows = normalize_critic_findings([candidate]);
        assert!(!rows[0].has_available_fix());
    }

    #[test]
    fn wrong_shape_observations_never_merge() {
        let literal =
            Member::LiteralUndefComparison.observation().into_candidate(SOURCE, subject());
        let potential =
            Member::PotentiallyUndefComparison.observation().into_candidate(SOURCE, subject());
        assert_eq!(
            normalize_critic_findings([literal, potential]).len(),
            2,
            "literal versus potentially-undef must stay separate canonical findings"
        );

        let backtick = Member::Backtick.observation().into_candidate(SOURCE, subject());
        let qx = Member::Qx.observation().into_candidate(SOURCE, subject());
        assert_eq!(
            normalize_critic_findings([backtick, qx]).len(),
            2,
            "backtick versus qx must stay separate canonical findings"
        );
    }

    #[test]
    fn readpipe_system_and_exec_remain_separate_canonical_findings() {
        let rows = normalize_critic_findings([
            Member::Readpipe.observation().into_candidate(SOURCE, subject()),
            Member::System.observation().into_candidate(SOURCE, subject()),
            Member::Exec.observation().into_candidate(SOURCE, subject()),
        ]);
        assert_eq!(rows.len(), 3);
        let mut codes: Vec<_> = rows.iter().map(|row| row.public_code().to_string()).collect();
        codes.sort();
        assert_eq!(codes, ["PL603", "PL604", "PL606"]);
    }

    #[test]
    fn exact_system_document_yields_one_row_with_both_contributors() {
        let native_candidates = vec![native_system_candidate(subject())];
        let builtin_candidates = [Member::System.observation().into_candidate(SOURCE, subject())];

        let rows =
            normalize_critic_findings(native_candidates.into_iter().chain(builtin_candidates));

        assert_eq!(rows.len(), 1, "the reviewed alias pair must be one logical row");
        let row = &rows[0];
        assert_eq!(row.canonical_id(), Some("critic.security.system_call"));
        assert_eq!(row.public_code(), "PL603");
        assert_eq!(row.contributors().len(), 2);
        assert!(
            row.contributors().iter().any(|contributor| contributor.identity().origin()
                == CriticFindingOrigin::BuiltInDiagnostic)
        );
        assert!(row.contributors().iter().any(
            |contributor| contributor.identity().origin() == CriticFindingOrigin::NativeCritic
        ));
        assert!(!row.has_severity_conflict(), "matched declarations invent no conflict");
    }

    #[test]
    fn different_generation_or_document_never_merges_the_alias_pair() {
        let next_generation = CriticSourceIdentity::new(SOURCE_KEY, GENERATION + 1);
        for mismatched_source in [next_generation, CriticSourceIdentity::new([8; 16], GENERATION)] {
            let builtin = Member::System.observation().into_candidate(SOURCE, subject());
            let native = native_system_candidate(mismatched_source);
            let rows = normalize_critic_findings(std::iter::once(builtin).chain([native]));
            assert_eq!(rows.len(), 2, "different source/generation cannot merge");
        }
    }

    #[test]
    fn candidate_arrival_permutation_is_byte_equivalent() {
        let native_side = vec![native_system_candidate(subject())];
        let builtin_side = vec![Member::System.observation().into_candidate(SOURCE, subject())];

        let forward =
            normalize_critic_findings(native_side.clone().into_iter().chain(builtin_side.clone()));
        let mut reversed_native = native_side;
        reversed_native.reverse();
        let backward = normalize_critic_findings(reversed_native.into_iter().chain(builtin_side));
        assert_eq!(forward, backward);
    }
}
