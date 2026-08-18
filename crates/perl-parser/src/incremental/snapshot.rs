use perl_parser_core::error::{ParseError, ParseOutput};
use perl_source_identity::ContentDigest;
use thiserror::Error;

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
/// The classification consumes maintained parser-output fields (`recovered_count`,
/// `terminated_early`) and typed diagnostic variants; it never treats a bare
/// non-empty diagnostic vector as recovery, because [`ParseError::Advisory`]
/// diagnostics do not invalidate the AST. #8036 owns the final exact stop-cause
/// vocabulary this classification will consume once it lands.
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
/// This is the sole owned authority binding exact source identity, generation,
/// terminal disposition, production strategy, and the native parser output.
/// Construct it through [`ParseSnapshot::from_output`]; fields are private so
/// no consumer can assemble an inconsistent `{source, generation, output}`
/// combination directly.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ParseSnapshot {
    generation: ParseGeneration,
    content_digest: ContentDigest,
    source_len: usize,
    disposition: ParseTerminalDisposition,
    strategy: ParseSnapshotStrategy,
    parse_output: ParseOutput,
}

impl ParseSnapshot {
    /// Build a snapshot from the native parser output and exact source bytes.
    ///
    /// The source identity is the canonical `source_identity.v1`
    /// [`ContentDigest`] (SHA-256, domain-separated), so a collision cannot
    /// authorize cross-source reuse.
    #[must_use]
    pub fn from_output(
        source: &str,
        generation: ParseGeneration,
        strategy: ParseSnapshotStrategy,
        parse_output: ParseOutput,
    ) -> Self {
        let disposition = classify_output(&parse_output);
        Self {
            generation,
            content_digest: ContentDigest::of_bytes(source.as_bytes()),
            source_len: source.len(),
            disposition,
            strategy,
            parse_output,
        }
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

    /// Terminal parser disposition for this generation.
    #[must_use]
    pub const fn disposition(&self) -> ParseTerminalDisposition {
        self.disposition
    }

    /// Production path that produced this generation.
    #[must_use]
    pub const fn strategy(&self) -> ParseSnapshotStrategy {
        self.strategy
    }

    /// Native recovery-aware parser output for this generation.
    #[must_use]
    pub const fn parse_output(&self) -> &ParseOutput {
        &self.parse_output
    }

    /// Validate that the snapshot still belongs to `source` and that its
    /// terminal disposition agrees with the native parser output.
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

        let observed_disposition = classify_output(&self.parse_output);
        if self.disposition != observed_disposition {
            return Err(ParseSnapshotValidationError::Disposition {
                recorded: self.disposition,
                observed: observed_disposition,
            });
        }

        Ok(())
    }
}

/// Snapshot/source identity or terminal-class mismatch.
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
}

fn classify_output(output: &ParseOutput) -> ParseTerminalDisposition {
    if output.diagnostics.iter().any(|error| matches!(error, ParseError::Cancelled)) {
        ParseTerminalDisposition::Cancelled
    } else if output.terminated_early
        || output.diagnostics.iter().any(|error| {
            matches!(error, ParseError::RecursionLimit | ParseError::NestingTooDeep { .. })
        })
    {
        ParseTerminalDisposition::BudgetExhausted
    } else if output.recovered_count > 0 {
        ParseTerminalDisposition::Recovered
    } else if output.diagnostics.iter().any(ParseError::blocks_clean_parse) {
        ParseTerminalDisposition::Catastrophic
    } else {
        ParseTerminalDisposition::Clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::parser::Parser;

    fn parse(source: &str) -> ParseOutput {
        Parser::new(source).parse_with_recovery()
    }

    #[test]
    fn clean_output_has_clean_disposition() {
        let source = "my $x = 1;";
        let snapshot = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::Fresh,
            parse(source),
        );

        assert_eq!(snapshot.disposition(), ParseTerminalDisposition::Clean);
        assert!(snapshot.validate_against(source).is_ok());
    }

    #[test]
    fn diagnostics_with_recovery_make_the_snapshot_recovered() {
        let source = "my $x = ;";
        let snapshot = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::Fresh,
            parse(source),
        );

        assert_eq!(snapshot.disposition(), ParseTerminalDisposition::Recovered);
    }

    #[test]
    fn advisory_only_output_is_not_recovered() {
        // ParseError::Advisory explicitly does not invalidate the AST, so an
        // advisory-only valid parse must not be classified as Recovered; that
        // would wrongly disable downstream features gating on recovered output.
        let source = "my $x = 1;";
        let mut advisory = parse(source);
        assert_eq!(advisory.recovered_count, 0);
        advisory
            .diagnostics
            .push(ParseError::Advisory { message: "style note".to_string(), location: 0 });
        let snapshot = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::Fresh,
            advisory,
        );
        assert_eq!(snapshot.disposition(), ParseTerminalDisposition::Clean);
        assert!(snapshot.validate_against(source).is_ok());
    }

    #[test]
    fn blocking_diagnostic_without_recovery_is_catastrophic() {
        let source = "my $x = 1;";
        let mut output = parse(source);
        assert_eq!(output.recovered_count, 0);
        output.diagnostics.push(ParseError::UnexpectedEof);

        let snapshot = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::Fresh,
            output,
        );

        assert_eq!(snapshot.disposition(), ParseTerminalDisposition::Catastrophic);
        assert!(snapshot.validate_against(source).is_ok());
    }

    #[test]
    fn cancellation_and_budget_exhaustion_are_not_recovery() {
        let source = "my $x = 1;";
        let mut cancelled = parse(source);
        cancelled.diagnostics.push(ParseError::Cancelled);
        let cancelled = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::Fresh,
            cancelled,
        );
        assert_eq!(cancelled.disposition(), ParseTerminalDisposition::Cancelled);

        let mut exhausted = parse(source);
        exhausted.terminated_early = true;
        let exhausted = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::Fresh,
            exhausted,
        );
        assert_eq!(exhausted.disposition(), ParseTerminalDisposition::BudgetExhausted);
    }

    #[test]
    fn exact_source_identity_is_load_bearing() {
        let source = "my $x = 1;";
        let snapshot = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::Fresh,
            parse(source),
        );

        assert!(matches!(
            snapshot.validate_against("my $y = 1;"),
            Err(ParseSnapshotValidationError::ContentDigest { .. })
        ));
        // Same length, different bytes: a length-only check would accept this.
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
