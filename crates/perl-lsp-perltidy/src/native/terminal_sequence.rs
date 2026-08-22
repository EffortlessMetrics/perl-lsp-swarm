//! Pure terminal-sequence final-newline policy (#8048 shift-left seam).
//!
//! Everything in this module operates on exact bytes/strings and complete LF,
//! CRLF, and supported bare-CR sequences. Nothing here produces or consumes LSP
//! positions, encodings, or edit plans.
//!
//! Authority boundary: this module has **no production caller** yet. Current
//! `NativeFormatter` newline handling (`FinalNewline`) and the LSP projection
//! keep their existing behavior until the byte-native train lands:
//!
//! ```text
//! #10237 byte-native source/target/edit-plan contracts
//! → #10239 native formatter API/caller cutover (integration owner)
//! → #10242 canonical generation-owned LSP projection
//! ```
//!
//! Per the #8048 ruling, publishing this pure policy early is sanctioned
//! provided it stays out of production paths, preserves sequences atomically,
//! distinguishes insert-only/trim-only/both/neither, and computes evidence
//! after the final bytes exist. Wiring it into [`crate::native`] formatters or
//! any LSP projection before #10239 is out of bounds for current claims.

/// One complete line-ending sequence, treated atomically.
///
/// CRLF is never split into separate CR and LF bytes by this module, and a
/// bare CR is a supported sequence rather than residue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminalSequence {
    /// Line feed (`\n`).
    Lf,
    /// Carriage return followed by line feed (`\r\n`), one sequence.
    CrLf,
    /// Bare carriage return (`\r`).
    Cr,
}

impl TerminalSequence {
    /// The exact bytes of this sequence.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }

    /// Length of this sequence in bytes.
    #[must_use]
    pub const fn len_bytes(self) -> usize {
        match self {
            Self::Lf | Self::Cr => 1,
            Self::CrLf => 2,
        }
    }
}

/// The complete run of line-ending sequences at the very end of a document.
///
/// An empty run means the document does not end with a line-ending sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRun {
    sequences: Vec<TerminalSequence>,
}

impl TerminalRun {
    /// The atomic sequences of the run, in document order.
    #[must_use]
    pub fn sequences(&self) -> &[TerminalSequence] {
        &self.sequences
    }

    /// Number of complete sequences in the run.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sequences.len()
    }

    /// Whether the document has no terminal line-ending sequence.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sequences.is_empty()
    }

    /// The last (deepest) sequence of the run, which defines the document's
    /// terminal convention when present.
    #[must_use]
    pub fn final_sequence(&self) -> Option<TerminalSequence> {
        self.sequences.last().copied()
    }
}

/// Scan the end of `source` and classify every trailing byte as part of one
/// complete sequence.
///
/// The scan is bounded to the terminal run and treats CRLF atomically, so
/// `"a\r\n\n"` yields `[CrLf, Lf]` rather than splitting the pair or losing
/// the final empty logical line the way a line iterator does.
#[must_use]
pub fn trailing_run(source: &str) -> TerminalRun {
    let bytes = source.as_bytes();
    let mut sequences = Vec::new();
    let mut index = bytes.len();

    while index > 0 {
        // CRLF is recognized before lone CR/LF so the pair is never split.
        if index >= 2 && bytes[index - 2] == b'\r' && bytes[index - 1] == b'\n' {
            sequences.push(TerminalSequence::CrLf);
            index -= 2;
        } else if bytes[index - 1] == b'\n' {
            sequences.push(TerminalSequence::Lf);
            index -= 1;
        } else if bytes[index - 1] == b'\r' {
            sequences.push(TerminalSequence::Cr);
            index -= 1;
        } else {
            break;
        }
    }

    sequences.reverse();
    TerminalRun { sequences }
}

/// The line-ending convention established by the last sequence anywhere in the
/// source, or `None` when the document contains no line endings at all.
#[must_use]
pub fn source_convention(source: &str) -> Option<TerminalSequence> {
    let mut chars = source.chars().peekable();
    let mut convention = None;

    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    let _ = chars.next();
                    convention = Some(TerminalSequence::CrLf);
                } else {
                    convention = Some(TerminalSequence::Cr);
                }
            }
            '\n' => convention = Some(TerminalSequence::Lf),
            _ => {}
        }
    }

    convention
}

/// The two independent LSP final-newline booleans, kept independent.
///
/// Unlike the single [`crate::FinalNewline`] setting, neither boolean may
/// collapse the other's semantics through hidden precedence:
///
/// - `insert_final_newline` adds one complete sequence only when none exists;
/// - `trim_final_newlines` retains exactly one final sequence and removes only
///   the excess sequences before it;
/// - both true produce exactly one accepted final sequence;
/// - neither true preserves the terminal bytes exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinalNewlinePolicy {
    /// Insert a final line-ending sequence only when none exists.
    pub insert_final_newline: bool,
    /// Remove excess final sequences while retaining exactly one.
    pub trim_final_newlines: bool,
}

/// What a policy application did to the terminal bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalChange {
    /// Output bytes equal input bytes; no insertion or removal happened.
    Unchanged,
    /// Exactly one sequence was appended to an unterminated document.
    Inserted,
    /// Excess sequences were removed while retaining the final one.
    Trimmed,
    /// Input had no terminal sequence and none was requested.
    LeftUnterminated,
}

/// Evidence over complete terminal sequences, computed **after** the final
/// bytes exist.
///
/// Every field is derived from the exact predecessor bytes and the exact final
/// bytes, never from an intermediate value, so the evidence cannot describe a
/// state the returned bytes do not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalNewlineEvidence {
    /// Trailing run of the exact predecessor bytes.
    pub predecessor: TerminalRun,
    /// Trailing run of the exact final bytes.
    pub final_run: TerminalRun,
    /// Sequence appended by the policy, if any.
    pub inserted: Option<TerminalSequence>,
    /// Number of complete excess sequences removed, if any.
    pub removed_count: usize,
    /// Classification of what happened to the terminal bytes.
    pub change: TerminalChange,
}

impl TerminalNewlineEvidence {
    /// Whether the evidence describes an output identical to its input.
    #[must_use]
    pub fn is_no_change(&self) -> bool {
        self.change == TerminalChange::Unchanged || self.change == TerminalChange::LeftUnterminated
    }

    /// Exact bounded comparison between the recorded runs and the recorded
    /// actions.
    ///
    /// Detects sequence conversion, partial CRLF splitting, and terminal
    /// insertion/removal mismatches: the final run must be exactly the
    /// predecessor run minus `removed_count` leading sequences plus the
    /// inserted sequence. Any other relationship — including a run that could
    /// only arise from stripping CR/LF bytes individually — is inconsistent
    /// and must be treated as failed/not-proven by consumers, never as an
    /// applied result.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        let action_shape_is_consistent = match (self.inserted, self.removed_count, self.change) {
            (Some(_), 0, TerminalChange::Inserted) => self.predecessor.is_empty(),
            (None, removed, TerminalChange::Trimmed) => {
                removed > 0 && removed < self.predecessor.len()
            }
            (None, 0, TerminalChange::Unchanged) => !self.predecessor.is_empty(),
            (None, 0, TerminalChange::LeftUnterminated) => self.predecessor.is_empty(),
            _ => false,
        };
        if !action_shape_is_consistent {
            return false;
        }

        if self.predecessor.len() < self.removed_count {
            return false;
        }

        let retained = &self.predecessor.sequences()[self.removed_count..];
        let mut reconstructed = retained.to_vec();
        if let Some(inserted) = self.inserted {
            reconstructed.push(inserted);
        }

        reconstructed == self.final_run.sequences()
    }
}

/// The result of applying [`FinalNewlinePolicy`] to exact source bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyOutcome {
    /// Final projected bytes.
    pub bytes: String,
    /// Evidence bound to those final bytes.
    pub evidence: TerminalNewlineEvidence,
}

/// Apply the independent insert/trim policy to `source`.
///
/// `explicit_convention`, when given, selects the proven convention for a
/// requested insertion instead of deriving it from the source. It never
/// converts sequences that already exist: trimming retains the document's own
/// final sequence, and insertion into an already-terminated document is a
/// no-op regardless of convention.
#[must_use]
pub fn apply_terminal_sequence_policy(
    source: &str,
    policy: FinalNewlinePolicy,
    explicit_convention: Option<TerminalSequence>,
) -> PolicyOutcome {
    let predecessor = trailing_run(source);

    let mut removed_count = 0_usize;
    let mut bytes = String::with_capacity(source.len() + TerminalSequence::Lf.len_bytes());
    bytes.push_str(source);

    if policy.trim_final_newlines && predecessor.len() > 1 {
        removed_count = predecessor.len() - 1;
        // Cut back to the exact content boundary, then retain exactly one
        // complete sequence: the document's own final one. Sequences are
        // removed whole, never split.
        let total_trailing: usize =
            predecessor.sequences().iter().map(|sequence| sequence.len_bytes()).sum();
        bytes.truncate(source.len() - total_trailing);
        bytes.push_str(predecessor.final_sequence().map_or("", TerminalSequence::as_str));
    }

    let mut inserted = None;
    if policy.insert_final_newline && trailing_run(&bytes).is_empty() {
        let sequence = explicit_convention
            .or_else(|| source_convention(source))
            .unwrap_or(TerminalSequence::Lf);
        bytes.push_str(sequence.as_str());
        inserted = Some(sequence);
    }

    let change = if inserted.is_some() {
        TerminalChange::Inserted
    } else if removed_count > 0 {
        TerminalChange::Trimmed
    } else if predecessor.is_empty() {
        TerminalChange::LeftUnterminated
    } else {
        TerminalChange::Unchanged
    };

    let evidence = TerminalNewlineEvidence {
        final_run: trailing_run(&bytes),
        predecessor,
        inserted,
        removed_count,
        change,
    };

    PolicyOutcome { bytes, evidence }
}

#[cfg(test)]
mod tests {
    use super::{
        FinalNewlinePolicy, TerminalChange, TerminalSequence, apply_terminal_sequence_policy,
    };

    #[test]
    fn trim_policy_and_evidence_fields_follow_the_real_terminal_seam() {
        let outcome = apply_terminal_sequence_policy(
            "x\n\r\n\r",
            FinalNewlinePolicy { insert_final_newline: false, trim_final_newlines: true },
            None,
        );

        assert_eq!(outcome.bytes, "x\r");
        assert_eq!(
            outcome.evidence.predecessor.sequences(),
            &[TerminalSequence::Lf, TerminalSequence::CrLf, TerminalSequence::Cr]
        );
        assert_eq!(outcome.evidence.final_run.sequences(), &[TerminalSequence::Cr]);
        assert_eq!(outcome.evidence.removed_count, 2);
        assert_eq!(outcome.evidence.change, TerminalChange::Trimmed);
        assert!(outcome.evidence.is_consistent());
    }

    #[test]
    fn insertion_evidence_fields_follow_the_final_returned_bytes() {
        let outcome = apply_terminal_sequence_policy(
            "x",
            FinalNewlinePolicy { insert_final_newline: true, trim_final_newlines: false },
            Some(TerminalSequence::CrLf),
        );

        assert!(outcome.evidence.predecessor.is_empty());
        assert_eq!(outcome.evidence.final_run.sequences(), &[TerminalSequence::CrLf]);
        assert_eq!(outcome.evidence.inserted, Some(TerminalSequence::CrLf));
        assert_eq!(outcome.evidence.change, TerminalChange::Inserted);
        assert!(outcome.evidence.is_consistent());
    }
}
