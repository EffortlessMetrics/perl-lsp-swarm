//! Compiled resource budget for AI backend responses.
//!
//! Destination validation, DNS pinning, redirect refusal, and credential
//! binding all decide *whether* a request may leave the process. None of them
//! bound what the remote endpoint may then send back. An approved but hostile
//! or malfunctioning endpoint can answer with a delimiter-free byte stream, a
//! single enormous event, an unending event sequence, or an unbounded run of
//! tiny deltas — each of which grows an allocation before any candidate
//! validation runs.
//!
//! This module owns the limit policy for that response path. Every limit is a
//! compiled constant with an explicit default and an explicit hard maximum:
//!
//! ```text
//! HTTP body bytes
//! → bounded line/frame reader   (ResponseBytes, LineBytes)
//! → bounded SSE event parser    (EventBytes, EventCount)
//! → bounded typed delta         (DeltaBytes)
//! → bounded cumulative output   (CompletionBytes, CompletionChars, CompletionLines)
//! → StreamChunk
//! ```
//!
//! [`AiResponseBudget::with_limit`] is the single door for narrowing a limit.
//! It validates against the compiled hard maxima, so a future user-owned
//! backend profile can select values without becoming able to loosen policy.

use std::fmt;

/// Which limit a budget decision names.
///
/// The identity is deliberately coarse: it tells an operator which boundary
/// was crossed without carrying any response, prompt, or completion content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BudgetKind {
    /// Total bytes consumed from the HTTP response body.
    ResponseBytes,
    /// Bytes in one physical response line, before any delimiter is seen.
    LineBytes,
    /// Bytes accumulated into the data payload of one SSE event.
    EventBytes,
    /// Number of SSE events yielded for one response.
    EventCount,
    /// Bytes in one decoded completion delta.
    DeltaBytes,
    /// Bytes accumulated into the cumulative completion.
    CompletionBytes,
    /// Unicode scalar values accumulated into the cumulative completion.
    CompletionChars,
    /// Lines spanned by the cumulative completion.
    CompletionLines,
}

impl BudgetKind {
    /// Stable machine-readable identity for logs and typed errors.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResponseBytes => "response_bytes",
            Self::LineBytes => "line_bytes",
            Self::EventBytes => "event_bytes",
            Self::EventCount => "event_count",
            Self::DeltaBytes => "delta_bytes",
            Self::CompletionBytes => "completion_bytes",
            Self::CompletionChars => "completion_chars",
            Self::CompletionLines => "completion_lines",
        }
    }
}

impl fmt::Display for BudgetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A crossed limit, carrying only limit identity and bounded numeric metadata.
///
/// `observed_at_least` is a lower bound rather than a measurement: enforcement
/// refuses *before* the offending bytes are accumulated, so the true size of
/// what the peer intended to send is unknown and deliberately not discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetViolation {
    /// The limit that was crossed.
    pub kind: BudgetKind,
    /// The configured value of that limit.
    pub limit: u64,
    /// A lower bound on what the response would have required.
    pub observed_at_least: u64,
}

impl fmt::Display for BudgetViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AI response budget exceeded: {} limit {}, observed at least {}",
            self.kind, self.limit, self.observed_at_least
        )
    }
}

impl std::error::Error for BudgetViolation {}

/// Rejection of a proposed budget value, raised before any request is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetError {
    /// A limit of zero admits nothing at all and is never a usable policy.
    NotPositive {
        /// The limit that was set to zero.
        kind: BudgetKind,
    },
    /// A limit above the compiled hard maximum for its kind.
    AboveHardMaximum {
        /// The limit that was set too high.
        kind: BudgetKind,
        /// The value that was asked for.
        requested: u64,
        /// The compiled ceiling for that limit.
        hard_maximum: u64,
    },
    /// A limit that cannot be satisfied under a related limit: a completion
    /// must admit at least one maximal delta, or the delta limit is
    /// unreachable policy.
    ///
    /// There is deliberately no matching event-versus-line rule. A data
    /// payload reaches its event through a line that also carries a `data:`
    /// prefix, so requiring `event_bytes >= line_bytes` would make an event
    /// that exactly fills its own budget unrepresentable.
    Incoherent {
        /// The limit that is too small for its companion.
        kind: BudgetKind,
        /// The value that was asked for.
        requested: u64,
        /// The smallest value that keeps the pair satisfiable.
        at_least: u64,
    },
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPositive { kind } => {
                write!(f, "AI response budget {kind} must be greater than zero")
            }
            Self::AboveHardMaximum { kind, requested, hard_maximum } => write!(
                f,
                "AI response budget {kind} of {requested} exceeds the compiled maximum \
                 {hard_maximum}"
            ),
            Self::Incoherent { kind, requested, at_least } => write!(
                f,
                "AI response budget {kind} of {requested} is below the {at_least} required by a \
                 related limit"
            ),
        }
    }
}

impl std::error::Error for BudgetError {}

/// Compiled default limits.
///
/// An inline Perl completion is a few hundred bytes. These values sit orders
/// of magnitude above any plausible legitimate candidate so that no real
/// completion is refused, while still bounding a hostile response to memory a
/// language server can absorb.
const DEFAULT_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const DEFAULT_LINE_BYTES: u64 = 256 * 1024;
const DEFAULT_EVENT_BYTES: u64 = 512 * 1024;
const DEFAULT_EVENT_COUNT: u64 = 20_000;
const DEFAULT_DELTA_BYTES: u64 = 64 * 1024;
const DEFAULT_COMPLETION_BYTES: u64 = 1024 * 1024;
const DEFAULT_COMPLETION_CHARS: u64 = 256 * 1024;
const DEFAULT_COMPLETION_LINES: u64 = 4_096;

/// Compiled hard maxima.
///
/// No caller — including a future user-owned backend profile — may select a
/// limit above these. They are the point past which a single inline-completion
/// response stops being a completion and starts being a denial-of-service.
const MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LINE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EVENT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_EVENT_COUNT: u64 = 200_000;
const MAX_DELTA_BYTES: u64 = 1024 * 1024;
const MAX_COMPLETION_BYTES: u64 = 16 * 1024 * 1024;
const MAX_COMPLETION_CHARS: u64 = 4 * 1024 * 1024;
const MAX_COMPLETION_LINES: u64 = 65_536;

/// One request-scoped budget governing the whole AI response path.
///
/// Fields are private so that every value in a live budget has passed
/// [`AiResponseBudget::with_limit`]; there is no struct-literal route that
/// skips validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiResponseBudget {
    response_bytes: u64,
    line_bytes: u64,
    event_bytes: u64,
    event_count: u64,
    delta_bytes: u64,
    completion_bytes: u64,
    completion_chars: u64,
    completion_lines: u64,
}

impl Default for AiResponseBudget {
    fn default() -> Self {
        Self::compiled_default()
    }
}

impl AiResponseBudget {
    /// The compiled default budget applied to every AI response.
    #[must_use]
    pub const fn compiled_default() -> Self {
        Self {
            response_bytes: DEFAULT_RESPONSE_BYTES,
            line_bytes: DEFAULT_LINE_BYTES,
            event_bytes: DEFAULT_EVENT_BYTES,
            event_count: DEFAULT_EVENT_COUNT,
            delta_bytes: DEFAULT_DELTA_BYTES,
            completion_bytes: DEFAULT_COMPLETION_BYTES,
            completion_chars: DEFAULT_COMPLETION_CHARS,
            completion_lines: DEFAULT_COMPLETION_LINES,
        }
    }

    /// The compiled ceiling for a limit. No budget may exceed it.
    #[must_use]
    pub const fn hard_maximum(kind: BudgetKind) -> u64 {
        match kind {
            BudgetKind::ResponseBytes => MAX_RESPONSE_BYTES,
            BudgetKind::LineBytes => MAX_LINE_BYTES,
            BudgetKind::EventBytes => MAX_EVENT_BYTES,
            BudgetKind::EventCount => MAX_EVENT_COUNT,
            BudgetKind::DeltaBytes => MAX_DELTA_BYTES,
            BudgetKind::CompletionBytes => MAX_COMPLETION_BYTES,
            BudgetKind::CompletionChars => MAX_COMPLETION_CHARS,
            BudgetKind::CompletionLines => MAX_COMPLETION_LINES,
        }
    }

    /// The current value of one limit.
    #[must_use]
    pub const fn limit(&self, kind: BudgetKind) -> u64 {
        match kind {
            BudgetKind::ResponseBytes => self.response_bytes,
            BudgetKind::LineBytes => self.line_bytes,
            BudgetKind::EventBytes => self.event_bytes,
            BudgetKind::EventCount => self.event_count,
            BudgetKind::DeltaBytes => self.delta_bytes,
            BudgetKind::CompletionBytes => self.completion_bytes,
            BudgetKind::CompletionChars => self.completion_chars,
            BudgetKind::CompletionLines => self.completion_lines,
        }
    }

    /// Return this budget with one limit replaced.
    ///
    /// # Errors
    ///
    /// Returns [`BudgetError`] when `value` is zero, above the compiled hard
    /// maximum for `kind`, or leaves a related pair unsatisfiable. The
    /// receiver is consumed and no partially-applied budget escapes.
    pub fn with_limit(mut self, kind: BudgetKind, value: u64) -> Result<Self, BudgetError> {
        if value == 0 {
            return Err(BudgetError::NotPositive { kind });
        }
        let hard_maximum = Self::hard_maximum(kind);
        if value > hard_maximum {
            return Err(BudgetError::AboveHardMaximum { kind, requested: value, hard_maximum });
        }

        match kind {
            BudgetKind::ResponseBytes => self.response_bytes = value,
            BudgetKind::LineBytes => self.line_bytes = value,
            BudgetKind::EventBytes => self.event_bytes = value,
            BudgetKind::EventCount => self.event_count = value,
            BudgetKind::DeltaBytes => self.delta_bytes = value,
            BudgetKind::CompletionBytes => self.completion_bytes = value,
            BudgetKind::CompletionChars => self.completion_chars = value,
            BudgetKind::CompletionLines => self.completion_lines = value,
        }

        self.check_coherent()?;
        Ok(self)
    }

    /// A completion must admit one maximal delta; otherwise the delta limit
    /// describes something the completion limit would always reject first.
    fn check_coherent(self) -> Result<(), BudgetError> {
        if self.completion_bytes < self.delta_bytes {
            return Err(BudgetError::Incoherent {
                kind: BudgetKind::CompletionBytes,
                requested: self.completion_bytes,
                at_least: self.delta_bytes,
            });
        }
        Ok(())
    }

    /// Refuse `observed` when it crosses `kind`'s limit.
    ///
    /// Callers check *before* accumulating, so `observed` is the size the
    /// response would reach, not a size already allocated.
    pub(crate) const fn check(
        &self,
        kind: BudgetKind,
        observed: u64,
    ) -> Result<(), BudgetViolation> {
        let limit = self.limit(kind);
        if observed > limit {
            return Err(BudgetViolation { kind, limit, observed_at_least: observed });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AiResponseBudget, BudgetError, BudgetKind, BudgetViolation};

    const EVERY_KIND: [BudgetKind; 8] = [
        BudgetKind::ResponseBytes,
        BudgetKind::LineBytes,
        BudgetKind::EventBytes,
        BudgetKind::EventCount,
        BudgetKind::DeltaBytes,
        BudgetKind::CompletionBytes,
        BudgetKind::CompletionChars,
        BudgetKind::CompletionLines,
    ];

    #[test]
    fn every_default_is_positive_and_within_its_compiled_maximum() {
        let budget = AiResponseBudget::compiled_default();
        for kind in EVERY_KIND {
            let value = budget.limit(kind);
            assert!(value > 0, "{kind} default must admit at least one unit");
            assert!(
                value <= AiResponseBudget::hard_maximum(kind),
                "{kind} default {value} exceeds its compiled maximum"
            );
        }
    }

    #[test]
    fn the_default_budget_is_internally_coherent() {
        let budget = AiResponseBudget::compiled_default();
        assert!(budget.limit(BudgetKind::CompletionBytes) >= budget.limit(BudgetKind::DeltaBytes));
    }

    #[test]
    fn a_value_above_the_compiled_maximum_is_rejected_for_every_kind() {
        for kind in EVERY_KIND {
            let hard_maximum = AiResponseBudget::hard_maximum(kind);
            let result = AiResponseBudget::compiled_default().with_limit(kind, hard_maximum + 1);
            assert_eq!(
                result,
                Err(BudgetError::AboveHardMaximum {
                    kind,
                    requested: hard_maximum + 1,
                    hard_maximum,
                }),
                "{kind} must refuse a value above its compiled maximum"
            );
        }
    }

    #[test]
    fn a_value_exactly_at_the_compiled_maximum_is_accepted() {
        // Negative control for the rejection above: the boundary itself is a
        // legal selection, so the check is `>` and not `>=`.
        for kind in EVERY_KIND {
            let hard_maximum = AiResponseBudget::hard_maximum(kind);
            let widened = AiResponseBudget::compiled_default()
                // Raise the one companion limit first so coherence, not the
                // ceiling, is the only thing under test here.
                .with_limit(
                    BudgetKind::CompletionBytes,
                    AiResponseBudget::hard_maximum(BudgetKind::CompletionBytes),
                )
                .and_then(|budget| budget.with_limit(kind, hard_maximum));
            assert!(widened.is_ok(), "{kind} must accept its own compiled maximum: {widened:?}");
        }
    }

    #[test]
    fn a_zero_limit_is_rejected_for_every_kind() {
        for kind in EVERY_KIND {
            assert_eq!(
                AiResponseBudget::compiled_default().with_limit(kind, 0),
                Err(BudgetError::NotPositive { kind }),
                "{kind} must refuse a zero limit"
            );
        }
    }

    #[test]
    fn an_event_limit_below_the_line_limit_is_allowed() {
        // A data payload rides a line that also carries its `data:` prefix,
        // so an event budget under the line budget is a legal narrowing —
        // not a contradiction.
        let budget = AiResponseBudget::compiled_default();
        let line_bytes = budget.limit(BudgetKind::LineBytes);
        let narrowed = budget.with_limit(BudgetKind::EventBytes, line_bytes - 1);
        assert!(narrowed.is_ok(), "a small event budget must be selectable: {narrowed:?}");
    }

    #[test]
    fn a_completion_smaller_than_one_maximal_delta_is_rejected() {
        let budget = AiResponseBudget::compiled_default();
        let delta_bytes = budget.limit(BudgetKind::DeltaBytes);
        assert_eq!(
            budget.with_limit(BudgetKind::CompletionBytes, delta_bytes - 1),
            Err(BudgetError::Incoherent {
                kind: BudgetKind::CompletionBytes,
                requested: delta_bytes - 1,
                at_least: delta_bytes,
            })
        );
    }

    #[test]
    fn a_narrowed_limit_is_the_value_actually_read_back() {
        let budget = AiResponseBudget::compiled_default()
            .with_limit(BudgetKind::LineBytes, 4_096)
            .and_then(|b| b.with_limit(BudgetKind::EventCount, 7));
        let Ok(budget) = budget else {
            unreachable!("narrowing within the compiled maxima must succeed");
        };
        assert_eq!(budget.limit(BudgetKind::LineBytes), 4_096);
        assert_eq!(budget.limit(BudgetKind::EventCount), 7);
        // Untouched limits keep their compiled defaults.
        assert_eq!(
            budget.limit(BudgetKind::ResponseBytes),
            AiResponseBudget::compiled_default().limit(BudgetKind::ResponseBytes)
        );
    }

    #[test]
    fn check_admits_the_limit_and_refuses_one_unit_past_it() {
        let budget = AiResponseBudget::compiled_default();
        let limit = budget.limit(BudgetKind::DeltaBytes);
        assert_eq!(budget.check(BudgetKind::DeltaBytes, limit), Ok(()));
        assert_eq!(
            budget.check(BudgetKind::DeltaBytes, limit + 1),
            Err(BudgetViolation {
                kind: BudgetKind::DeltaBytes,
                limit,
                observed_at_least: limit + 1,
            })
        );
    }

    #[test]
    fn a_violation_renders_only_bounded_numeric_metadata() {
        let violation = BudgetViolation {
            kind: BudgetKind::CompletionBytes,
            limit: 1024,
            observed_at_least: 1025,
        };
        assert_eq!(
            violation.to_string(),
            "AI response budget exceeded: completion_bytes limit 1024, observed at least 1025"
        );
    }
}
