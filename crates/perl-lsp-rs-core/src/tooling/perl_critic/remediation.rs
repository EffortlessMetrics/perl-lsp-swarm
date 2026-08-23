//! Static remediation eligibility for critic findings.
//!
//! This module owns one thing: what a finding and its attached fix *declare*
//! about remediation, derived from `FixSafety` and edit presence alone. That is
//! a property of the finding, knowable without any request, document snapshot,
//! or action registry.
//!
//! It deliberately owns no live availability. Deciding that a concrete action is
//! actually offerable right now requires exact current source identity, action
//! discovery, and edit binding, all of which are owned by the shared action and
//! currentness authorities (#4206 / #4208) and consumed by the live critic
//! integration (#7481). A critic-local approximation of that proof — document
//! generation counters, content hashes, action-id strings, edit-count equality —
//! would be a second, weaker authority for the same question, so none is
//! provided here.

use serde::{Deserialize, Serialize};

use super::native::{CriticFinding, CriticFix, FixSafety};

/// Static remediation eligibility declared by a finding and its attached fix.
///
/// The candidate variants are **not** availability. They say what an action
/// could become once the integration layer has proven current source identity,
/// action discovery, and edit binding — never that it is offerable now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriticRemediationEligibility {
    /// No remediation is attached to the finding.
    #[default]
    None,
    /// Human guidance exists, but no concrete edit is eligible.
    Manual,
    /// A suggested edit may become a preview action after live proof.
    PreviewCandidate,
    /// A safe edit may become an automatic action after live proof.
    AutomaticCandidate,
}

/// Final remediation availability, as a value vocabulary only.
///
/// This crate can name these states so downstream surfaces share one spelling.
/// It cannot *reach* `Preview` or `Automatic`: there is no mapping from
/// [`CriticRemediationEligibility`] to this type here, and no constructor that
/// takes critic-local evidence. Only the live integration layer, holding real
/// action and currentness proof, may conclude either.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriticRemediationClass {
    /// No remediation is attached to the finding.
    #[default]
    None,
    /// Guidance exists, but no current concrete action is available.
    Manual,
    /// A current concrete edit is available for preview or confirmation.
    Preview,
    /// A current safe edit is available for automatic application.
    Automatic,
}

impl FixSafety {
    /// Static remediation eligibility for this safety declaration.
    ///
    /// An empty edit set always degrades to manual guidance: a fix that carries
    /// no edit cannot be applied whatever its declared safety. Non-empty safe
    /// and suggested edits stay candidates until live discovery supplies proof.
    #[must_use]
    pub const fn remediation_eligibility(self, has_edits: bool) -> CriticRemediationEligibility {
        if !has_edits {
            return CriticRemediationEligibility::Manual;
        }

        match self {
            Self::Safe => CriticRemediationEligibility::AutomaticCandidate,
            Self::Suggested => CriticRemediationEligibility::PreviewCandidate,
            Self::ManualOnly => CriticRemediationEligibility::Manual,
        }
    }
}

impl CriticFix {
    /// Static remediation eligibility for this attached fix.
    #[must_use]
    pub fn remediation_eligibility(&self) -> CriticRemediationEligibility {
        self.safety.remediation_eligibility(!self.edits.is_empty())
    }
}

impl CriticFinding {
    /// Static remediation eligibility before any live action discovery.
    ///
    /// A finding with no fix is [`CriticRemediationEligibility::None`].
    #[must_use]
    pub fn remediation_eligibility(&self) -> CriticRemediationEligibility {
        self.fix
            .as_ref()
            .map_or(CriticRemediationEligibility::None, CriticFix::remediation_eligibility)
    }
}

#[cfg(test)]
mod tests {
    use perl_parser_core::position::{Position, Range};

    use super::{CriticRemediationClass, CriticRemediationEligibility};
    use crate::tooling::perl_critic::{
        CriticCategory, CriticFinding, CriticFindingShape, CriticFix, CriticTextEdit, FixSafety,
        Severity,
    };

    const RULE_ID: &str = "native.test.remediation";

    fn test_range() -> Range {
        Range {
            start: Position { byte: 0, line: 0, column: 0 },
            end: Position { byte: 1, line: 0, column: 1 },
        }
    }

    fn edit() -> CriticTextEdit {
        CriticTextEdit { range: test_range(), new_text: "replacement".to_string() }
    }

    fn finding(fix: Option<CriticFix>) -> CriticFinding {
        CriticFinding {
            rule_id: RULE_ID.to_string(),
            category: CriticCategory::Syntax,
            severity: Severity::Harsh,
            range: test_range(),
            message: "test finding".to_string(),
            explanation: "test explanation".to_string(),
            suppression_key: RULE_ID.to_string(),
            observed_shape: CriticFindingShape::General,
            related: Vec::new(),
            fix,
        }
    }

    fn fix(safety: FixSafety, edits: Vec<CriticTextEdit>) -> CriticFix {
        CriticFix { title: "test action".to_string(), safety, edits }
    }

    // -----------------------------------------------------------------------
    // #6970 required mapping.
    // -----------------------------------------------------------------------

    #[test]
    fn a_finding_without_a_fix_is_not_eligible() {
        assert_eq!(finding(None).remediation_eligibility(), CriticRemediationEligibility::None);
    }

    #[test]
    fn manual_only_safety_is_manual() {
        let subject = finding(Some(fix(FixSafety::ManualOnly, vec![edit()])));
        assert_eq!(
            subject.remediation_eligibility(),
            CriticRemediationEligibility::Manual,
            "ManualOnly never advertises an applicable edit"
        );
    }

    #[test]
    fn suggested_with_a_non_empty_edit_is_a_preview_candidate() {
        let subject = finding(Some(fix(FixSafety::Suggested, vec![edit()])));
        assert_eq!(
            subject.remediation_eligibility(),
            CriticRemediationEligibility::PreviewCandidate
        );
    }

    #[test]
    fn safe_with_a_non_empty_edit_is_an_automatic_candidate() {
        let subject = finding(Some(fix(FixSafety::Safe, vec![edit()])));
        assert_eq!(
            subject.remediation_eligibility(),
            CriticRemediationEligibility::AutomaticCandidate
        );
    }

    /// An empty edit set degrades regardless of declared safety — including
    /// `Safe`, which is the direction that would otherwise fail open.
    #[test]
    fn an_empty_edit_set_degrades_every_safety_to_manual() {
        for safety in [FixSafety::Safe, FixSafety::Suggested, FixSafety::ManualOnly] {
            let subject = finding(Some(fix(safety, Vec::new())));
            assert_eq!(
                subject.remediation_eligibility(),
                CriticRemediationEligibility::Manual,
                "{safety:?} with no edits must not stay a candidate"
            );
        }
    }

    // -----------------------------------------------------------------------
    // #6970 / #7650 falsifiers: static metadata cannot reach live availability.
    // -----------------------------------------------------------------------

    /// `Safe` plus a non-empty edit is the strongest thing this substrate can
    /// say, and it is still only a *candidate*. If a future change lets this
    /// module conclude `Automatic`, this assertion is the tripwire.
    #[test]
    fn safe_plus_edits_stops_at_candidate_and_is_not_final_automatic() {
        let subject = finding(Some(fix(FixSafety::Safe, vec![edit()])));
        let eligibility = subject.remediation_eligibility();

        assert_eq!(eligibility, CriticRemediationEligibility::AutomaticCandidate);
        assert_ne!(
            serde_json::to_value(eligibility).ok(),
            serde_json::to_value(CriticRemediationClass::Automatic).ok(),
            "candidate eligibility must not be interchangeable with final automatic availability"
        );
    }

    /// `Suggested` can never reach automatic in either vocabulary, however many
    /// edits it carries.
    #[test]
    fn suggested_never_reaches_automatic_in_either_vocabulary() {
        let subject = finding(Some(fix(FixSafety::Suggested, vec![edit(), edit(), edit()])));
        let eligibility = subject.remediation_eligibility();

        assert_eq!(eligibility, CriticRemediationEligibility::PreviewCandidate);
        assert_ne!(eligibility, CriticRemediationEligibility::AutomaticCandidate);
    }

    /// Eligibility is a function of safety and edit *presence* only. Two fixes
    /// whose edit sets differ entirely but agree on emptiness are
    /// indistinguishable here — which is exactly why this module must not be
    /// treated as proof that a specific edit set is current.
    #[test]
    fn eligibility_does_not_discriminate_between_different_edit_sets() {
        let one = finding(Some(fix(FixSafety::Safe, vec![edit()])));
        let other_shape = CriticTextEdit {
            range: test_range(),
            new_text: "a completely different replacement".to_string(),
        };
        let two = finding(Some(fix(FixSafety::Safe, vec![other_shape])));

        assert_eq!(one.remediation_eligibility(), two.remediation_eligibility());
        assert_eq!(one.remediation_eligibility(), CriticRemediationEligibility::AutomaticCandidate);
    }

    /// The structural ratchet for #7650: this module must not regrow a
    /// critic-local authorization protocol. Any of these names returning means a
    /// second, weaker authority for source identity, action discovery, or edit
    /// binding has reappeared alongside #4206 / #4208 / #7481.
    #[test]
    fn no_critic_local_authorization_authority_is_reintroduced() {
        let source = include_str!("remediation.rs");
        let module_body =
            source.split_once("#[cfg(test)]").map_or(source, |(module_body, _tests)| module_body);

        for forbidden in [
            "CriticSourceProof",
            "CriticCodeActionProof",
            "from_discovery",
            "observed_generation",
            "current_generation",
            "content_hash",
            "action_id",
            "fn authorizes",
            "fn remediation_class",
        ] {
            assert!(
                !module_body.contains(forbidden),
                "`{forbidden}` reintroduces critic-local live proof; \
                 source/action/edit authority belongs to #4206/#4208/#7481"
            );
        }
    }

    /// The final vocabulary may be *named* here so surfaces share one spelling,
    /// but nothing in this module maps eligibility onto it.
    #[test]
    fn the_final_class_vocabulary_is_nameable_but_unreachable_from_eligibility() {
        assert_eq!(CriticRemediationClass::default(), CriticRemediationClass::None);

        let source = include_str!("remediation.rs");
        let module_body =
            source.split_once("#[cfg(test)]").map_or(source, |(module_body, _tests)| module_body);

        assert!(
            !module_body.contains("CriticRemediationClass::Automatic")
                && !module_body.contains("CriticRemediationClass::Preview"),
            "this module must not construct a final available class from static metadata"
        );
    }
}
