use perl_parser_core::error::{
    ParseCompletion, ParseCompletionClass, ParseCompletionValidationError, ParseOutput,
};
use perl_source_identity::ContentDigest;
use thiserror::Error;

use super::geometry_attachment::{
    SourceGeometryAttachment, SourceGeometryAttachmentState, SourceGeometrySubject,
    SourceGeometryValidationError,
};

/// Monotonic identity for one committed parser generation.
///
/// Advancement is checked: a generation must never silently stop advancing,
/// because reuse would make stale caches and tasks indistinguishable from
/// current ones. Use [`ParseGeneration::checked_next`] and fail closed before
/// committing state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParseGeneration(u64);

impl ParseGeneration {
    /// Initial generation assigned to a newly parsed source.
    pub const INITIAL: Self = Self(0);

    /// Return the raw monotonic generation value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Return the next generation, or `None` when the counter is exhausted.
    ///
    /// Exhaustion is a typed failure for the commit path, never a saturated
    /// reuse of the previous generation.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

/// Terminal parser disposition for one exact source generation.
///
/// This is the coarse projection of [`ParseCompletion`]. The snapshot stores
/// the typed completion produced by the parser operation and never classifies
/// from diagnostic count, wording, or a post-hoc diagnostic scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseTerminalDisposition {
    /// Parsing completed without recovery, budget exhaustion, or cancellation.
    Clean,
    /// Parsing produced a current partial tree through at least one recorded
    /// synthetic repair (`recovered_count > 0`).
    Recovered,
    /// Parsing could not produce an ordinary clean/recovered result.
    Catastrophic,
    /// Parsing stopped through cooperative cancellation.
    Cancelled,
    /// Parsing stopped because a parser resource budget was exhausted.
    BudgetExhausted,
}

impl ParseTerminalDisposition {
    /// Project the coarse snapshot class from a typed parser completion.
    #[must_use]
    pub const fn from_completion(completion: ParseCompletion) -> Self {
        match completion.class() {
            ParseCompletionClass::Clean => Self::Clean,
            ParseCompletionClass::Recovered => Self::Recovered,
            ParseCompletionClass::Catastrophic => Self::Catastrophic,
            ParseCompletionClass::Cancelled => Self::Cancelled,
            ParseCompletionClass::BudgetExhausted => Self::BudgetExhausted,
            _ => Self::Catastrophic,
        }
    }
}

/// Production path that created the committed snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseSnapshotStrategy {
    /// Initial or explicitly requested full fresh parse.
    Fresh,
    /// A lexer restart was attempted, followed by the authoritative full parser.
    IncrementalTokenRestartThenFullParse,
    /// The incremental path failed closed to a complete full-parser fallback.
    IncrementalFullFallback,
}

/// One generation-bound parser result.
///
/// This is the sole owned authority binding exact source identity, parser-state
/// lifetime, generation, terminal disposition, production strategy, native
/// parser output, and the source-geometry availability attachment. Construct it
/// through [`ParseSnapshot::from_output`]; fields are private so no consumer can
/// assemble an inconsistent `{source, instance, generation, output, geometry}`
/// combination directly.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ParseSnapshot {
    generation: ParseGeneration,
    content_digest: ContentDigest,
    source_len: usize,
    completion: ParseCompletion,
    strategy: ParseSnapshotStrategy,
    source_geometry: SourceGeometryAttachment,
    parse_output: ParseOutput,
}

impl ParseSnapshot {
    /// Build a snapshot for a new independent parser-state lifetime.
    ///
    /// The source identity is the canonical `source_identity.v1`
    /// [`ContentDigest`] (SHA-256, domain-separated), so a collision cannot
    /// authorize cross-source reuse. Current constructors retain an explicit
    /// geometry-unavailable attachment until the canonical same-operation
    /// producer in #13978/#13980 is available.
    #[must_use]
    pub fn from_output(
        source: &str,
        generation: ParseGeneration,
        strategy: ParseSnapshotStrategy,
        parse_output: ParseOutput,
    ) -> Self {
        Self::from_output_with_subject(source, generation, strategy, parse_output, None)
    }

    pub(super) fn from_output_for_existing_instance(
        source: &str,
        generation: ParseGeneration,
        strategy: ParseSnapshotStrategy,
        parse_output: ParseOutput,
        previous_subject: &SourceGeometrySubject,
    ) -> Self {
        Self::from_output_with_subject(
            source,
            generation,
            strategy,
            parse_output,
            Some(previous_subject),
        )
    }

    fn from_output_with_subject(
        source: &str,
        generation: ParseGeneration,
        strategy: ParseSnapshotStrategy,
        parse_output: ParseOutput,
        previous_subject: Option<&SourceGeometrySubject>,
    ) -> Self {
        let completion = parse_output.completion();
        let disposition = ParseTerminalDisposition::from_completion(completion);
        let content_digest = ContentDigest::of_bytes(source.as_bytes());
        let source_len = source.len();
        let geometry_subject = match previous_subject {
            Some(previous) => SourceGeometrySubject::next_for_same_instance(
                previous,
                generation,
                content_digest.clone(),
                source_len,
                disposition,
                strategy,
            ),
            None => SourceGeometrySubject::new(
                generation,
                content_digest.clone(),
                source_len,
                disposition,
                strategy,
            ),
        };
        let source_geometry = SourceGeometryAttachment::unavailable(geometry_subject);
        Self {
            generation,
            content_digest,
            source_len,
            completion,
            strategy,
            source_geometry,
            parse_output,
        }
    }

    /// Instrument or setup failure cannot become a parser snapshot.
    ///
    /// External timeout, missing instrument, harness panic, and receipt-write
    /// failure remain operation failure/`NOT_PROVEN`, not a parser-domain
    /// terminal class.
    pub fn from_operation_failure(
        failure: ParseOperationFailure,
    ) -> Result<Self, ParseSnapshotConstructionError> {
        match failure {
            ParseOperationFailure::InstrumentFailure => {
                Err(ParseSnapshotConstructionError::InstrumentFailure)
            }
            ParseOperationFailure::NotProven => Err(ParseSnapshotConstructionError::NotProven),
        }
    }

    #[cfg(test)]
    fn with_instrument_failure_geometry(mut self) -> Self {
        self.source_geometry = SourceGeometryAttachment::instrument_failure(
            self.source_geometry.subject().clone(),
            super::geometry_attachment::SourceGeometryInstrumentFailureReason::ProducerFailed,
        );
        self
    }

    /// Monotonic committed generation.
    #[must_use]
    pub const fn generation(&self) -> ParseGeneration {
        self.generation
    }

    /// Canonical exact-source content digest for this generation.
    #[must_use]
    pub fn content_digest(&self) -> &ContentDigest {
        &self.content_digest
    }

    /// Exact source length represented by the snapshot.
    #[must_use]
    pub const fn source_len(&self) -> usize {
        self.source_len
    }

    /// Typed parser-operation completion carried into this snapshot.
    #[must_use]
    pub const fn completion(&self) -> ParseCompletion {
        self.completion
    }

    /// Terminal parser disposition for this generation.
    #[must_use]
    pub const fn disposition(&self) -> ParseTerminalDisposition {
        ParseTerminalDisposition::from_completion(self.completion)
    }

    /// Production path that produced this generation.
    #[must_use]
    pub const fn strategy(&self) -> ParseSnapshotStrategy {
        self.strategy
    }

    /// Source-geometry availability and exact snapshot subject.
    #[must_use]
    pub const fn source_geometry(&self) -> &SourceGeometryAttachment {
        &self.source_geometry
    }

    /// Native recovery-aware parser output for this generation.
    #[must_use]
    pub const fn parse_output(&self) -> &ParseOutput {
        &self.parse_output
    }

    /// Validate that the snapshot still belongs to `source`, that its terminal
    /// disposition agrees with the native parser output, and that its geometry
    /// attachment belongs to this exact parser-state lifetime and generation.
    pub fn validate_against(&self, source: &str) -> Result<(), ParseSnapshotValidationError> {
        if self.source_len != source.len() {
            return Err(ParseSnapshotValidationError::SourceLength {
                recorded: self.source_len,
                observed: source.len(),
            });
        }

        let observed_digest = ContentDigest::of_bytes(source.as_bytes());
        if self.content_digest != observed_digest {
            return Err(ParseSnapshotValidationError::ContentDigest {
                recorded: self.content_digest.clone(),
                observed: observed_digest,
            });
        }

        let observed_completion = self.parse_output.completion();
        if self.completion != observed_completion {
            return Err(ParseSnapshotValidationError::Disposition {
                recorded: ParseTerminalDisposition::from_completion(self.completion),
                observed: ParseTerminalDisposition::from_completion(observed_completion),
            });
        }
        self.completion.validate_against_output(&self.parse_output)?;

        if matches!(
            self.source_geometry.state(),
            SourceGeometryAttachmentState::InstrumentFailure { .. }
        ) {
            return Err(ParseSnapshotValidationError::InstrumentFailure);
        }

        // Unavailable attachments compare tautologically (the attachment
        // subject was built from these same fields); the discriminating
        // payload check runs only for Partial/Complete states inside
        // validate_for. The uniform call is deliberate defense in depth.
        let expected_geometry_subject = SourceGeometrySubject::next_for_same_instance(
            self.source_geometry.subject(),
            self.generation,
            self.content_digest.clone(),
            self.source_len,
            self.disposition(),
            self.strategy,
        );
        self.source_geometry.validate_for(&expected_geometry_subject)?;

        Ok(())
    }
}

/// Parser-domain operation that failed before a snapshot could exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseOperationFailure {
    /// Geometry or other instrumentation failed independently of parser output.
    InstrumentFailure,
    /// The operation result is missing or untrustworthy.
    NotProven,
}

/// Why a parser snapshot could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ParseSnapshotConstructionError {
    /// Instrument or setup failure cannot become a parser snapshot.
    #[error("instrument or setup failure cannot produce a parser snapshot")]
    InstrumentFailure,
    /// Missing instrument, harness failure, or receipt-write failure is not a
    /// parser-domain terminal.
    #[error("operation failure remains NOT_PROVEN, not a parser snapshot")]
    NotProven,
}

/// Snapshot/source identity, terminal-class, or geometry-attachment mismatch.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ParseSnapshotValidationError {
    /// Recorded and observed source lengths differ.
    #[error("snapshot source length mismatch: recorded {recorded}, observed {observed}")]
    SourceLength {
        /// Length stored in the snapshot.
        recorded: usize,
        /// Length of the source supplied for validation.
        observed: usize,
    },
    /// Recorded and observed exact-source digests differ.
    #[error("snapshot content digest does not match the supplied source")]
    ContentDigest {
        /// Digest stored in the snapshot.
        recorded: ContentDigest,
        /// Digest calculated from the supplied source.
        observed: ContentDigest,
    },
    /// Stored terminal disposition disagrees with the native parse output.
    #[error("snapshot terminal disposition mismatch: recorded {recorded:?}, observed {observed:?}")]
    Disposition {
        /// Disposition stored in the snapshot.
        recorded: ParseTerminalDisposition,
        /// Disposition derived from the native output.
        observed: ParseTerminalDisposition,
    },
    /// Stored completion fails required/forbidden field checks.
    #[error(transparent)]
    Completion(#[from] ParseCompletionValidationError),
    /// Instrument failure leaked into a constructed snapshot.
    #[error("instrument failure cannot remain a successful parser snapshot")]
    InstrumentFailure,
    /// Source geometry is malformed or belongs to another snapshot subject.
    #[error(transparent)]
    SourceGeometry(#[from] SourceGeometryValidationError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::error::{
        ParseBudgetExhaustionDimension, ParseCancellationIdentity, ParseCatastrophicCause,
        ParseCompletionStage, ParseCoreDimension, ParseError, ParseStopCause,
    };
    use perl_parser_core::parser::Parser;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn parse(source: &str) -> ParseOutput {
        Parser::new(source).parse_with_recovery()
    }

    fn make_snapshot(source: &str, output: ParseOutput) -> ParseSnapshot {
        ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::Fresh,
            output,
        )
    }

    #[test]
    fn clean_output_has_clean_disposition() {
        let source = "my $x = 1;";
        let snapshot = make_snapshot(source, parse(source));

        assert_eq!(snapshot.completion(), ParseCompletion::CompletedClean);
        assert_eq!(snapshot.disposition(), ParseTerminalDisposition::Clean);
        assert!(snapshot.validate_against(source).is_ok());
        assert!(matches!(
            snapshot.source_geometry().state(),
            SourceGeometryAttachmentState::Unavailable { .. }
        ));
        assert_eq!(snapshot.source_geometry().subject().generation(), ParseGeneration::INITIAL);
        assert_eq!(
            snapshot.source_geometry().subject().content_digest(),
            snapshot.content_digest()
        );
        assert_eq!(snapshot.source_geometry().subject().source_len(), source.len());
    }

    #[test]
    fn independent_snapshots_with_identical_bytes_have_distinct_instances() {
        let source = "my $x = 1;";
        let first = make_snapshot(source, parse(source));
        let second = make_snapshot(source, parse(source));

        assert!(
            !first.source_geometry().subject().same_instance_as(second.source_geometry().subject())
        );
        assert_ne!(first.source_geometry().subject(), second.source_geometry().subject());
        assert_eq!(first.completion(), second.completion());
    }

    #[test]
    fn diagnostics_with_recovery_make_the_snapshot_recovered() {
        let source = "my $x = ;";
        let snapshot = make_snapshot(source, parse(source));

        assert!(matches!(snapshot.completion(), ParseCompletion::CompletedWithRecovery { .. }));
        assert_eq!(snapshot.disposition(), ParseTerminalDisposition::Recovered);
        assert_eq!(
            snapshot.source_geometry().subject().disposition(),
            ParseTerminalDisposition::Recovered
        );
    }

    #[test]
    fn advisory_only_output_is_not_recovered() {
        let source = "my $x = 1;";
        let mut advisory = parse(source);
        assert_eq!(advisory.recovered_count, 0);
        advisory
            .diagnostics
            .push(ParseError::Advisory { message: "style note".to_string(), location: 0 });
        let snapshot = make_snapshot(source, advisory);
        assert_eq!(snapshot.completion(), ParseCompletion::CompletedClean);
        assert_eq!(snapshot.disposition(), ParseTerminalDisposition::Clean);
        assert!(snapshot.validate_against(source).is_ok());
    }

    #[test]
    fn blocking_diagnostic_without_recovery_is_not_inferred_catastrophe() {
        let source = "my $x = 1;";
        let mut output = parse(source);
        assert_eq!(output.recovered_count, 0);
        output.diagnostics.push(ParseError::UnexpectedEof);

        let snapshot = make_snapshot(source, output);
        assert_eq!(snapshot.disposition(), ParseTerminalDisposition::Clean);
        assert_ne!(snapshot.disposition(), ParseTerminalDisposition::Recovered);
        assert_ne!(snapshot.disposition(), ParseTerminalDisposition::Catastrophic);
        assert!(snapshot.validate_against(source).is_ok());
    }

    #[test]
    fn typed_catastrophic_constructor_is_reachable_with_and_without_safe_partial() {
        let source = "my $x = 1;";
        let mut without_partial = parse(source);
        without_partial.set_stop_cause(Some(ParseStopCause::CatastrophicTermination));
        let without_partial = make_snapshot(source, without_partial);
        assert_eq!(
            without_partial.completion(),
            ParseCompletion::catastrophic(
                ParseCatastrophicCause::UnrecoverableParserOrLexerFailure,
                true,
            )
        );
        assert_eq!(without_partial.disposition(), ParseTerminalDisposition::Catastrophic);
        assert!(without_partial.validate_against(source).is_ok());

        let mut with_shell = parse(source);
        with_shell.ast = perl_parser_core::Node::new(
            perl_parser_core::NodeKind::Program { statements: vec![] },
            perl_parser_core::SourceLocation { start: 0, end: 0 },
        );
        with_shell.set_stop_cause(Some(ParseStopCause::CatastrophicTermination));
        let with_shell = make_snapshot(source, with_shell);
        assert_eq!(
            with_shell.completion(),
            ParseCompletion::catastrophic(
                ParseCatastrophicCause::UnrecoverableParserOrLexerFailure,
                false,
            )
        );
        assert_eq!(with_shell.disposition(), ParseTerminalDisposition::Catastrophic);
    }

    #[test]
    fn diagnostic_text_cannot_synthesize_cancellation_or_exhaustion() {
        let source = "my $x = 1;";
        let mut cancelled_diag = parse(source);
        cancelled_diag.diagnostics.push(ParseError::Cancelled);
        cancelled_diag.diagnostics.push(ParseError::RecursionLimit);
        let cancelled_diag = make_snapshot(source, cancelled_diag);
        assert_eq!(cancelled_diag.disposition(), ParseTerminalDisposition::Clean);
        assert_ne!(cancelled_diag.disposition(), ParseTerminalDisposition::Cancelled);
        assert_ne!(cancelled_diag.disposition(), ParseTerminalDisposition::BudgetExhausted);

        let mut cancelled = parse(source);
        cancelled.diagnostics.push(ParseError::syntax("looks recovered", 0));
        cancelled.set_stop_cause(Some(ParseStopCause::Cancelled));
        let cancelled = make_snapshot(source, cancelled);
        assert_eq!(
            cancelled.completion(),
            ParseCompletion::cancelled(
                ParseCompletionStage::Unspecified,
                ParseCancellationIdentity::CooperativeToken,
            )
        );
        assert_eq!(cancelled.disposition(), ParseTerminalDisposition::Cancelled);

        let mut exhausted = parse(source);
        exhausted.diagnostics.push(ParseError::Cancelled);
        exhausted.set_stop_cause(Some(ParseStopCause::NestingOrDepthBudgetExhausted {
            limit: 512,
            usage: 513,
        }));
        let exhausted = make_snapshot(source, exhausted);
        assert_eq!(exhausted.disposition(), ParseTerminalDisposition::BudgetExhausted);
        assert!(matches!(
            exhausted.completion(),
            ParseCompletion::BudgetExhausted {
                dimension: ParseBudgetExhaustionDimension::NestingOrDepth,
                limit: Some(512),
                usage: Some(513),
                ..
            }
        ));
    }

    #[test]
    fn live_cancellation_and_core_budget_use_typed_completion() {
        let flag = Arc::new(AtomicBool::new(true));
        let mut parser = Parser::new_with_cancellation("my $x = 1;", flag);
        let cancelled = parser.parse_with_recovery();
        let snapshot = make_snapshot("my $x = 1;", cancelled);
        assert_eq!(snapshot.disposition(), ParseTerminalDisposition::Cancelled);
        assert!(snapshot.validate_against("my $x = 1;").is_ok());

        let mut exhausted = parse("my $x = 1;");
        exhausted.set_stop_cause(Some(ParseStopCause::CoreBudgetExhausted {
            dimension: ParseCoreDimension::DiagnosticsEmitted,
            limit: 1,
            usage: 1,
        }));
        let exhausted = make_snapshot("my $x = 1;", exhausted);
        assert_eq!(exhausted.disposition(), ParseTerminalDisposition::BudgetExhausted);
    }

    #[test]
    fn instrument_failure_cannot_become_a_snapshot() {
        assert!(matches!(
            ParseSnapshot::from_operation_failure(ParseOperationFailure::InstrumentFailure),
            Err(ParseSnapshotConstructionError::InstrumentFailure)
        ));
        assert!(matches!(
            ParseSnapshot::from_operation_failure(ParseOperationFailure::NotProven),
            Err(ParseSnapshotConstructionError::NotProven)
        ));

        let source = "my $x = 1;";
        let leaked = make_snapshot(source, parse(source)).with_instrument_failure_geometry();
        assert_eq!(
            leaked.validate_against(source),
            Err(ParseSnapshotValidationError::InstrumentFailure)
        );
        assert_eq!(leaked.disposition(), ParseTerminalDisposition::Clean);
    }

    #[test]
    fn fresh_restart_and_fallback_share_the_terminal_constructor() {
        let source = "my $x = 1;";
        let output = parse(source);
        let fresh = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::Fresh,
            output.clone(),
        );
        let restart = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::IncrementalTokenRestartThenFullParse,
            output.clone(),
        );
        let fallback = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::IncrementalFullFallback,
            output,
        );
        assert_eq!(fresh.completion(), restart.completion());
        assert_eq!(restart.completion(), fallback.completion());
        assert_eq!(fresh.disposition(), ParseTerminalDisposition::Clean);
    }

    #[test]
    fn repeated_identical_source_is_deterministic() {
        let source = "my $x = ;";
        let first = make_snapshot(source, parse(source));
        let second = make_snapshot(source, parse(source));
        assert_eq!(first.completion(), second.completion());
        assert_eq!(first.disposition(), second.disposition());
    }

    #[test]
    fn exact_source_identity_is_load_bearing() {
        let source = "my $x = 1;";
        let snapshot = make_snapshot(source, parse(source));

        assert!(matches!(
            snapshot.validate_against("my $y = 1;"),
            Err(ParseSnapshotValidationError::ContentDigest { .. })
        ));
        assert!(matches!(
            snapshot.validate_against("my $z = 1;"),
            Err(ParseSnapshotValidationError::ContentDigest { .. })
        ));
    }

    #[test]
    fn generation_advancement_fails_closed_at_exhaustion() {
        let generation = ParseGeneration::INITIAL;
        assert_eq!(generation.checked_next().map(ParseGeneration::get), Some(1));

        let exhausted = ParseGeneration::INITIAL;
        let max = ParseGeneration(u64::MAX);
        assert!(max.checked_next().is_none());
        assert_eq!(exhausted.get(), 0);
    }
}
