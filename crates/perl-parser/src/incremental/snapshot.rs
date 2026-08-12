use perl_parser_core::error::{ParseError, ParseOutput};
use thiserror::Error;

/// Monotonic identity for one committed parser generation.
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

    /// Return the next generation, saturating at `u64::MAX`.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Stable content fingerprint for the exact source bytes in a snapshot.
///
/// This is an identity/checking hash, not a cryptographic digest. The FNV-1a
/// algorithm is intentionally implemented locally so the value is stable across
/// processes and Rust toolchains without depending on `Hash` implementation
/// details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentFingerprint(u64);

impl ContentFingerprint {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    /// Fingerprint exact source bytes.
    #[must_use]
    pub fn from_source(source: &str) -> Self {
        let mut value = Self::OFFSET_BASIS;
        for byte in source.as_bytes() {
            value ^= u64::from(*byte);
            value = value.wrapping_mul(Self::PRIME);
        }
        Self(value)
    }

    /// Return the stable numeric fingerprint.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Terminal parser disposition for one exact source generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseTerminalDisposition {
    /// Parsing completed without diagnostics or recovery.
    Clean,
    /// Parsing produced a current partial tree plus diagnostics or recovery.
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
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ParseSnapshot {
    /// Monotonic committed generation.
    pub generation: ParseGeneration,
    /// Fingerprint of the exact source bytes parsed.
    pub content_fingerprint: ContentFingerprint,
    /// Exact source length represented by the snapshot.
    pub source_len: usize,
    /// Terminal parser disposition independent from diagnostic count alone.
    pub disposition: ParseTerminalDisposition,
    /// Production path that produced this generation.
    pub strategy: ParseSnapshotStrategy,
    /// Native recovery-aware parser output.
    pub parse_output: ParseOutput,
}

impl ParseSnapshot {
    /// Build a snapshot from the native parser output and exact source bytes.
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
            content_fingerprint: ContentFingerprint::from_source(source),
            source_len: source.len(),
            disposition,
            strategy,
            parse_output,
        }
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

        let observed_fingerprint = ContentFingerprint::from_source(source);
        if self.content_fingerprint != observed_fingerprint {
            return Err(ParseSnapshotValidationError::ContentFingerprint {
                recorded: self.content_fingerprint,
                observed: observed_fingerprint,
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
    /// Recorded and observed source fingerprints differ.
    #[error("snapshot content fingerprint does not match the supplied source")]
    ContentFingerprint {
        /// Fingerprint stored in the snapshot.
        recorded: ContentFingerprint,
        /// Fingerprint calculated from the supplied source.
        observed: ContentFingerprint,
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
    } else if output.recovered_count > 0 || !output.diagnostics.is_empty() {
        ParseTerminalDisposition::Recovered
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

        assert_eq!(snapshot.disposition, ParseTerminalDisposition::Clean);
        assert!(snapshot.validate_against(source).is_ok());
    }

    #[test]
    fn diagnostics_make_the_snapshot_recovered() {
        let source = "my $x = ;";
        let snapshot = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::Fresh,
            parse(source),
        );

        assert_eq!(snapshot.disposition, ParseTerminalDisposition::Recovered);
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
        assert_eq!(cancelled.disposition, ParseTerminalDisposition::Cancelled);

        let mut exhausted = parse(source);
        exhausted.terminated_early = true;
        let exhausted = ParseSnapshot::from_output(
            source,
            ParseGeneration::INITIAL,
            ParseSnapshotStrategy::Fresh,
            exhausted,
        );
        assert_eq!(exhausted.disposition, ParseTerminalDisposition::BudgetExhausted);
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
            Err(ParseSnapshotValidationError::ContentFingerprint { .. })
        ));
    }
}