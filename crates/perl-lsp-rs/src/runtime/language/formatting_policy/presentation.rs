//! Central formatting presentation policy (#13849).
//!
//! Maps an already-classified [`FormatOutcome`] to LSP projection, receipt,
//! optional user notification, and bounded dedup identity. Formatter semantics
//! stay in the provider outcome; this module only decides presentation.
#![expect(
    dead_code,
    reason = "policy:13849-slice1: exhaustive mapping lands before wire-handler migration"
)]

use perl_lsp_rs_core::hashing::fnv1a64;
use perl_lsp_rs_core::tooling::perltidy::native::{
    FormatDisposition, FormatEngine, FormatOutcome, FormatReasonCode,
};

use super::Surface;

/// Formatting request surface family for presentation and dedup identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FormattingPresentationSurface {
    Document,
    Range,
    Ranges,
    OnType,
}

impl From<Surface> for FormattingPresentationSurface {
    fn from(surface: Surface) -> Self {
        match surface {
            Surface::Document => Self::Document,
            Surface::Range => Self::Range,
            Surface::Ranges => Self::Ranges,
            Surface::OnType => Self::OnType,
        }
    }
}

/// Whether the caller explicitly invoked formatting versus an automatic path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormattingInvocationKind {
    Automatic,
    UserInvoked,
}

/// LSP JSON-RPC result class for one formatting outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormattingLspResultClass {
    ReturnEdits,
    ReturnEmptyEdits,
    ProtocolError(FormattingProtocolErrorKind),
}

/// Typed protocol errors that must not masquerade as successful empty edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormattingProtocolErrorKind {
    ContentModifiedStaleSource,
    ContentModifiedStaleConfiguration,
    InternalInstrumentFailure,
    InternalFormattedOutputParseError,
    InternalInvalidConfiguration,
    InternalOutcomeContract,
}

/// Receipt fields projected alongside the LSP response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FormattingReceiptPresentation {
    pub decision: &'static str,
    pub fallback: &'static str,
    pub reason_wire: &'static str,
}

/// User-visible notification channel and severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormattingUserNotificationChannel {
    ShowMessage,
    LogMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormattingUserMessageSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FormattingUserNotification {
    pub channel: FormattingUserNotificationChannel,
    pub severity: FormattingUserMessageSeverity,
    pub message: String,
}

/// Bounded dedup identity for actionable formatting warnings (#13849).
///
/// Retains only hashed fingerprints — never raw URIs, paths, or message bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FormattingWarningDedupKey {
    pub reason: FormatReasonCode,
    pub surface: FormattingPresentationSurface,
    /// Hashed logical document identity (`FormatIdentity::source_id`), not raw URI text.
    pub source_document_fingerprint: u64,
    pub source_generation: u64,
    pub source_content_fingerprint: u64,
    pub config_fingerprint: u64,
    pub actual_engine: FormatEngine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FormattingPresentationDecision {
    pub lsp_result: FormattingLspResultClass,
    pub receipt: FormattingReceiptPresentation,
    pub notification: Option<FormattingUserNotification>,
    pub dedup_key: Option<FormattingWarningDedupKey>,
    pub retry_without_source_change: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FormattingPresentationInput<'a> {
    pub surface: FormattingPresentationSurface,
    pub invocation: FormattingInvocationKind,
    pub edit_count: usize,
    pub outcome: &'a FormatOutcome,
}

/// Decide how one typed formatter outcome should be presented on the wire.
#[must_use]
pub(crate) fn decide_formatting_presentation(
    input: &FormattingPresentationInput<'_>,
) -> FormattingPresentationDecision {
    let reason = input.outcome.reason;
    let disposition = input.outcome.disposition;
    let receipt = receipt_for_reason(reason);
    let dedup_key =
        |notify: bool| notify.then(|| formatting_warning_dedup_key(input.surface, input.outcome));

    match reason {
        FormatReasonCode::Applied | FormatReasonCode::AppliedPreservingLiterals => {
            if disposition != FormatDisposition::Applied {
                return outcome_contract_violation(receipt);
            }
            if input.edit_count == 0 {
                return outcome_contract_violation(receipt);
            }
            FormattingPresentationDecision {
                lsp_result: FormattingLspResultClass::ReturnEdits,
                receipt,
                notification: None,
                dedup_key: None,
                retry_without_source_change: true,
            }
        }
        FormatReasonCode::AlreadyFormatted
        | FormatReasonCode::AlreadyFormattedPreservingLiterals => {
            if disposition != FormatDisposition::NoChange || input.edit_count != 0 {
                return outcome_contract_violation(receipt);
            }
            FormattingPresentationDecision {
                lsp_result: FormattingLspResultClass::ReturnEmptyEdits,
                receipt,
                notification: None,
                dedup_key: None,
                retry_without_source_change: true,
            }
        }
        FormatReasonCode::FormatterDisabled => {
            if disposition != FormatDisposition::Refused || input.edit_count != 0 {
                return outcome_contract_violation(receipt);
            }
            FormattingPresentationDecision {
                lsp_result: FormattingLspResultClass::ReturnEmptyEdits,
                receipt,
                notification: None,
                dedup_key: None,
                retry_without_source_change: false,
            }
        }
        FormatReasonCode::UnsupportedSyntax => {
            if disposition != FormatDisposition::Refused || input.edit_count != 0 {
                return outcome_contract_violation(receipt);
            }
            let notify = matches!(input.invocation, FormattingInvocationKind::UserInvoked);
            FormattingPresentationDecision {
                lsp_result: FormattingLspResultClass::ReturnEmptyEdits,
                receipt,
                notification: notify.then(|| {
                    user_notification(
                        input.outcome,
                        UNSUPPORTED_SYNTAX_HEADLINE,
                        FormattingUserMessageSeverity::Warning,
                    )
                }),
                dedup_key: dedup_key(notify),
                retry_without_source_change: false,
            }
        }
        FormatReasonCode::LiteralPreservationUnsupported => {
            if disposition != FormatDisposition::Refused || input.edit_count != 0 {
                return outcome_contract_violation(receipt);
            }
            let notify = matches!(input.invocation, FormattingInvocationKind::UserInvoked);
            FormattingPresentationDecision {
                lsp_result: FormattingLspResultClass::ReturnEmptyEdits,
                receipt,
                notification: notify.then(|| {
                    user_notification(
                        input.outcome,
                        LITERAL_PRESERVATION_HEADLINE,
                        FormattingUserMessageSeverity::Warning,
                    )
                }),
                dedup_key: dedup_key(notify),
                retry_without_source_change: true,
            }
        }
        FormatReasonCode::SourceParseError => {
            if disposition != FormatDisposition::Refused || input.edit_count != 0 {
                return outcome_contract_violation(receipt);
            }
            FormattingPresentationDecision {
                lsp_result: FormattingLspResultClass::ReturnEmptyEdits,
                receipt,
                notification: None,
                dedup_key: None,
                retry_without_source_change: false,
            }
        }
        FormatReasonCode::UnsafeRange => {
            if disposition != FormatDisposition::Refused || input.edit_count != 0 {
                return outcome_contract_violation(receipt);
            }
            let notify = matches!(input.invocation, FormattingInvocationKind::UserInvoked);
            FormattingPresentationDecision {
                lsp_result: FormattingLspResultClass::ReturnEmptyEdits,
                receipt,
                notification: notify.then(|| {
                    user_notification(
                        input.outcome,
                        UNSAFE_RANGE_HEADLINE,
                        FormattingUserMessageSeverity::Warning,
                    )
                }),
                dedup_key: dedup_key(notify),
                retry_without_source_change: true,
            }
        }
        FormatReasonCode::StaleSource => FormattingPresentationDecision {
            lsp_result: FormattingLspResultClass::ProtocolError(
                FormattingProtocolErrorKind::ContentModifiedStaleSource,
            ),
            receipt,
            notification: None,
            dedup_key: None,
            retry_without_source_change: true,
        },
        FormatReasonCode::InvalidConfiguration => FormattingPresentationDecision {
            lsp_result: FormattingLspResultClass::ProtocolError(
                FormattingProtocolErrorKind::InternalInvalidConfiguration,
            ),
            receipt,
            notification: None,
            dedup_key: None,
            retry_without_source_change: false,
        },
        FormatReasonCode::FormattedOutputParseError => {
            if disposition != FormatDisposition::FailedOrNotProven {
                return outcome_contract_violation(receipt);
            }
            FormattingPresentationDecision {
                lsp_result: FormattingLspResultClass::ProtocolError(
                    FormattingProtocolErrorKind::InternalFormattedOutputParseError,
                ),
                receipt,
                notification: None,
                dedup_key: None,
                retry_without_source_change: false,
            }
        }
        FormatReasonCode::InstrumentFailure => {
            if disposition != FormatDisposition::FailedOrNotProven {
                return outcome_contract_violation(receipt);
            }
            FormattingPresentationDecision {
                lsp_result: FormattingLspResultClass::ProtocolError(
                    FormattingProtocolErrorKind::InternalInstrumentFailure,
                ),
                receipt,
                notification: None,
                dedup_key: None,
                retry_without_source_change: false,
            }
        }
    }
}

const UNSUPPORTED_SYNTAX_HEADLINE: &str = "Native formatting left the source unchanged because its syntax is outside the formatter's current safe subset.";
const LITERAL_PRESERVATION_HEADLINE: &str = "Native formatting left the source unchanged because a literal or opaque region requires preservation support that is not available for this request.";
const UNSAFE_RANGE_HEADLINE: &str =
    "Formatting left the source unchanged because the requested range cannot be formatted safely.";

fn receipt_for_reason(reason: FormatReasonCode) -> FormattingReceiptPresentation {
    let reason_wire = reason_wire_name(reason);
    let (decision, fallback) = match reason {
        FormatReasonCode::Applied
        | FormatReasonCode::AppliedPreservingLiterals
        | FormatReasonCode::AlreadyFormatted
        | FormatReasonCode::AlreadyFormattedPreservingLiterals => ("acted", "none"),
        FormatReasonCode::FormattedOutputParseError | FormatReasonCode::InstrumentFailure => {
            ("blocked", "no_edit")
        }
        FormatReasonCode::StaleSource | FormatReasonCode::InvalidConfiguration => {
            ("blocked", "no_edit")
        }
        _ => ("blocked", "no_edit"),
    };
    FormattingReceiptPresentation { decision, fallback, reason_wire }
}

const fn reason_wire_name(reason: FormatReasonCode) -> &'static str {
    match reason {
        FormatReasonCode::Applied => "applied",
        FormatReasonCode::AppliedPreservingLiterals => "applied_preserving_literals",
        FormatReasonCode::AlreadyFormatted => "already_formatted",
        FormatReasonCode::AlreadyFormattedPreservingLiterals => {
            "already_formatted_preserving_literals"
        }
        FormatReasonCode::FormatterDisabled => "formatter_disabled",
        FormatReasonCode::UnsupportedSyntax => "unsupported_syntax",
        FormatReasonCode::LiteralPreservationUnsupported => "literal_preservation_unsupported",
        FormatReasonCode::SourceParseError => "source_parse_error",
        FormatReasonCode::FormattedOutputParseError => "formatted_output_parse_error",
        FormatReasonCode::UnsafeRange => "unsafe_range",
        FormatReasonCode::StaleSource => "stale_source",
        FormatReasonCode::InvalidConfiguration => "invalid_configuration",
        FormatReasonCode::InstrumentFailure => "instrument_failure",
    }
}

fn formatting_warning_dedup_key(
    surface: FormattingPresentationSurface,
    outcome: &FormatOutcome,
) -> FormattingWarningDedupKey {
    FormattingWarningDedupKey {
        reason: outcome.reason,
        surface,
        source_document_fingerprint: outcome
            .identity
            .source_id
            .as_deref()
            .map(|id| fnv1a64(id.as_bytes()))
            .unwrap_or(0),
        source_generation: outcome.identity.source_generation.unwrap_or(0),
        source_content_fingerprint: fnv1a64(outcome.identity.content_digest.as_bytes()),
        config_fingerprint: fnv1a64(outcome.identity.config_fingerprint.as_bytes()),
        actual_engine: outcome.identity.actual_engine,
    }
}

fn user_notification(
    outcome: &FormatOutcome,
    headline: &str,
    severity: FormattingUserMessageSeverity,
) -> FormattingUserNotification {
    FormattingUserNotification {
        channel: FormattingUserNotificationChannel::ShowMessage,
        severity,
        message: compose_user_message(outcome, headline),
    }
}

fn compose_user_message(outcome: &FormatOutcome, headline: &str) -> String {
    let mut message = String::from(headline);
    message.push_str(" The source was left unchanged.");
    if outcome.identity.actual_engine == FormatEngine::Native {
        message.push_str(" The native formatter handled this request.");
    } else if outcome.identity.actual_engine == FormatEngine::ExternalLegacy {
        message.push_str(" External Perl::Tidy compatibility handled this request.");
    }
    if let Some(action) = outcome.next_action.as_deref().filter(|action| !action.is_empty()) {
        message.push(' ');
        message.push_str(action);
    }
    message
}

fn outcome_contract_violation(
    _receipt: FormattingReceiptPresentation,
) -> FormattingPresentationDecision {
    FormattingPresentationDecision {
        lsp_result: FormattingLspResultClass::ProtocolError(
            FormattingProtocolErrorKind::InternalOutcomeContract,
        ),
        receipt: FormattingReceiptPresentation {
            decision: "blocked",
            fallback: "no_edit",
            reason_wire: "instrument_failure",
        },
        notification: None,
        dedup_key: None,
        retry_without_source_change: false,
    }
}

#[cfg(test)]
mod tests {
    // Fixture assertions read decision payloads via expect; module-scoped
    // exemption matches the sibling test-module convention.
    #![allow(clippy::expect_used)]

    use super::*;
    use perl_lsp_rs_core::tooling::perltidy::native::{
        FormatChangeSummary, FormatEvidenceState, FormatExecutionStrategy, FormatIdentity,
        FormatLineEndingDisposition, FormatRequestTarget, FormatSafetyEvidence, FormatterMode,
    };

    fn minimal_outcome(disposition: FormatDisposition, reason: FormatReasonCode) -> FormatOutcome {
        FormatOutcome {
            disposition,
            reason,
            identity: FormatIdentity {
                source_id: None,
                content_digest: "digest-a".to_string(),
                source_generation: Some(7),
                actual_engine: FormatEngine::Native,
                requested_mode: FormatterMode::Native,
                config_fingerprint: "cfg-1".to_string(),
            },
            target: FormatRequestTarget::Document,
            execution: FormatExecutionStrategy::NativeDocument,
            change: FormatChangeSummary {
                edit_count: 0,
                source_bytes_changed: 0,
                rendered_bytes_changed: 0,
                changed_lines: 0,
            },
            safety: FormatSafetyEvidence {
                parse_before: FormatEvidenceState::Proven,
                parse_after: FormatEvidenceState::NotApplicable,
                literal_preservation: FormatEvidenceState::NotApplicable,
                utf8: FormatEvidenceState::Proven,
                line_endings: FormatLineEndingDisposition::NotChecked,
            },
            next_action: Some("Try a smaller supported range.".to_string()),
        }
    }

    fn all_format_reason_codes() -> [FormatReasonCode; 13] {
        [
            FormatReasonCode::Applied,
            FormatReasonCode::AppliedPreservingLiterals,
            FormatReasonCode::AlreadyFormatted,
            FormatReasonCode::AlreadyFormattedPreservingLiterals,
            FormatReasonCode::FormatterDisabled,
            FormatReasonCode::UnsupportedSyntax,
            FormatReasonCode::LiteralPreservationUnsupported,
            FormatReasonCode::SourceParseError,
            FormatReasonCode::FormattedOutputParseError,
            FormatReasonCode::UnsafeRange,
            FormatReasonCode::StaleSource,
            FormatReasonCode::InvalidConfiguration,
            FormatReasonCode::InstrumentFailure,
        ]
    }

    fn disposition_for_reason(reason: FormatReasonCode) -> FormatDisposition {
        match reason {
            FormatReasonCode::Applied | FormatReasonCode::AppliedPreservingLiterals => {
                FormatDisposition::Applied
            }
            FormatReasonCode::AlreadyFormatted
            | FormatReasonCode::AlreadyFormattedPreservingLiterals => FormatDisposition::NoChange,
            FormatReasonCode::FormattedOutputParseError | FormatReasonCode::InstrumentFailure => {
                FormatDisposition::FailedOrNotProven
            }
            FormatReasonCode::StaleSource | FormatReasonCode::InvalidConfiguration => {
                FormatDisposition::Refused
            }
            _ => FormatDisposition::Refused,
        }
    }

    #[test]
    fn minimal_outcome_records_the_native_document_execution_strategy() {
        let outcome = minimal_outcome(FormatDisposition::Applied, FormatReasonCode::Applied);
        assert_eq!(outcome.execution, FormatExecutionStrategy::NativeDocument);
    }

    #[test]
    fn formatting_presentation_reason_code_registry_matches_enum() {
        assert_eq!(all_format_reason_codes().len(), 13);
    }

    #[test]
    fn formatting_presentation_maps_every_reason_code_without_panic() {
        for reason in all_format_reason_codes() {
            let mut outcome = minimal_outcome(disposition_for_reason(reason), reason);
            let edit_count = if matches!(
                reason,
                FormatReasonCode::Applied | FormatReasonCode::AppliedPreservingLiterals
            ) {
                outcome.change.edit_count = 1;
                1
            } else {
                0
            };
            let input = FormattingPresentationInput {
                surface: FormattingPresentationSurface::Document,
                invocation: FormattingInvocationKind::UserInvoked,
                edit_count,
                outcome: &outcome,
            };
            let _decision = decide_formatting_presentation(&input);
        }
    }

    #[test]
    fn formatting_presentation_supported_success_stays_silent() {
        let outcome = minimal_outcome(FormatDisposition::Applied, FormatReasonCode::Applied);
        let decision = decide_formatting_presentation(&FormattingPresentationInput {
            surface: FormattingPresentationSurface::Document,
            invocation: FormattingInvocationKind::UserInvoked,
            edit_count: 2,
            outcome: &outcome,
        });
        assert_eq!(decision.lsp_result, FormattingLspResultClass::ReturnEdits);
        assert!(decision.notification.is_none());
        assert!(decision.dedup_key.is_none());
    }

    #[test]
    fn formatting_presentation_unsupported_syntax_warns_on_user_invocation() {
        let outcome =
            minimal_outcome(FormatDisposition::Refused, FormatReasonCode::UnsupportedSyntax);
        let decision = decide_formatting_presentation(&FormattingPresentationInput {
            surface: FormattingPresentationSurface::Range,
            invocation: FormattingInvocationKind::UserInvoked,
            edit_count: 0,
            outcome: &outcome,
        });
        assert_eq!(decision.lsp_result, FormattingLspResultClass::ReturnEmptyEdits);
        assert!(decision.notification.is_some());
        assert!(decision.dedup_key.is_some());
        let message = decision.notification.expect("notification").message;
        assert!(message.contains("syntax is outside"));
        assert!(message.contains("Try a smaller supported range."));
    }

    #[test]
    fn formatting_presentation_unsupported_syntax_is_quiet_when_automatic() {
        let outcome =
            minimal_outcome(FormatDisposition::Refused, FormatReasonCode::UnsupportedSyntax);
        let decision = decide_formatting_presentation(&FormattingPresentationInput {
            surface: FormattingPresentationSurface::Document,
            invocation: FormattingInvocationKind::Automatic,
            edit_count: 0,
            outcome: &outcome,
        });
        assert!(decision.notification.is_none());
        assert!(decision.dedup_key.is_none());
    }

    #[test]
    fn formatting_presentation_failed_outcomes_never_return_empty_edits() {
        for reason in
            [FormatReasonCode::InstrumentFailure, FormatReasonCode::FormattedOutputParseError]
        {
            let outcome = minimal_outcome(FormatDisposition::FailedOrNotProven, reason);
            let decision = decide_formatting_presentation(&FormattingPresentationInput {
                surface: FormattingPresentationSurface::Document,
                invocation: FormattingInvocationKind::UserInvoked,
                edit_count: 0,
                outcome: &outcome,
            });
            assert!(matches!(decision.lsp_result, FormattingLspResultClass::ProtocolError(_)));
        }
    }

    #[test]
    fn formatting_presentation_dedup_key_varies_by_surface_and_generation() {
        let outcome =
            minimal_outcome(FormatDisposition::Refused, FormatReasonCode::UnsupportedSyntax);
        let base = formatting_warning_dedup_key(FormattingPresentationSurface::Document, &outcome);
        let other_surface =
            formatting_warning_dedup_key(FormattingPresentationSurface::Range, &outcome);
        assert_ne!(base, other_surface);

        let mut other_generation = outcome.clone();
        other_generation.identity.source_generation = Some(8);
        let changed_generation = formatting_warning_dedup_key(
            FormattingPresentationSurface::Document,
            &other_generation,
        );
        assert_ne!(base, changed_generation);
    }

    #[test]
    fn formatting_presentation_dedup_key_varies_by_source_id_with_identical_content() {
        let mut outcome_a =
            minimal_outcome(FormatDisposition::Refused, FormatReasonCode::UnsupportedSyntax);
        outcome_a.identity.source_id = Some("file:///a.pl".to_string());
        let mut outcome_b = outcome_a.clone();
        outcome_b.identity.source_id = Some("file:///b.pl".to_string());
        let key_a =
            formatting_warning_dedup_key(FormattingPresentationSurface::Document, &outcome_a);
        let key_b =
            formatting_warning_dedup_key(FormattingPresentationSurface::Document, &outcome_b);
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn formatting_presentation_outcome_contract_violation_receipt_is_blocked() {
        let outcome = minimal_outcome(FormatDisposition::Refused, FormatReasonCode::Applied);
        let decision = decide_formatting_presentation(&FormattingPresentationInput {
            surface: FormattingPresentationSurface::Document,
            invocation: FormattingInvocationKind::UserInvoked,
            edit_count: 0,
            outcome: &outcome,
        });
        assert!(matches!(
            decision.lsp_result,
            FormattingLspResultClass::ProtocolError(
                FormattingProtocolErrorKind::InternalOutcomeContract
            )
        ));
        assert_eq!(decision.receipt.decision, "blocked");
        assert_eq!(decision.receipt.fallback, "no_edit");
        assert_eq!(decision.receipt.reason_wire, "instrument_failure");
    }
}
