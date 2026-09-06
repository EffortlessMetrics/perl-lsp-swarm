//! Anti-pattern detection for heredoc edge cases.
//!
//! This module provides detection and analysis of problematic Perl patterns
//! that make static parsing difficult or impossible, particularly around heredocs.
//!
//! The [`crate::heredoc_anti_patterns::AntiPatternDetector`] scans Perl source
//! for seven categories of heredoc-related anti-patterns and produces
//! [`crate::heredoc_anti_patterns::Diagnostic`]s describing each finding, with
//! severity, explanation, suggested fix, and documentation references.
//!
//! # Scan-bound decision (#3597, supersedes the #3568 tradeoff)
//!
//! Two detector patterns — regex code block and eval string — describe
//! constructs that *must* span newlines in real Perl: the heredoc body has to
//! reach its terminator. #3568 excluded `\n` from their character classes to
//! bound scan work, which silently dropped every multi-line occurrence — that
//! is, essentially every true positive. #3597 asked whether that coverage loss
//! actually buys anything.
//!
//! The dynamic-delimiter pattern deliberately keeps its newline horizon. It
//! describes no construct that needs to cross a line, so widening it would only
//! admit false positives on multi-line left shifts such as `1 << ${\nfoo}`.
//!
//! Measured through [`AntiPatternDetector::detect_all`] (see
//! `tests/heredoc_antip_redos_guardrail.rs`, which owns the executable form of
//! these numbers), three candidate shapes on adversarial input:
//!
//! | shape | newline-excluded | unbounded | `{0,2000}`-bounded |
//! |---|---|---|---|
//! | 40 KB dense unclosed `(?{`/`<<` | 88 µs | 106 µs | 582 000 µs |
//! | 72 KB many single-line matches | 235 µs | 239 µs | 3 396 µs |
//! | 23 KB multi-line closing blocks | 11 µs, **0 found** | 51 µs, 1000 found | 1 243 µs, 1000 found |
//!
//! Every shape scales linearly with input size, so the `captures_iter`
//! `O(m·n²)` caveat does not bind for these patterns: `regex` is a
//! finite-automaton engine and makes one left-to-right pass. Catastrophic
//! backtracking is not reachable, so the original ReDoS premise was overstated.
//!
//! The bounded-quantifier alternative proposed in the closed #3542/#3546/#3575
//! cluster is the *worst* of the three: `{0,N}` unrolls into N automaton states,
//! inflating `m` in `O(m·n)` by three to four orders of magnitude, and it still
//! truncates detection past its horizon. It is rejected on measurement.
//!
//! Those two patterns therefore use unbounded negated classes. They remain
//! bounded in practice because each class excludes its own terminator (`}`,
//! `'`, `"`), so a scan cannot run past the construct it is matching.
//!
//! Removing the horizon exposed a latent defect in `EvalHeredocDetector`: alone
//! among the detectors it scans raw source, because masking would blank the
//! contents of the very quoted string it must look inside. An `eval '` fragment
//! in a comment could therefore seed a match. It now checks each match origin
//! against the masked view, which also fixes the pre-existing single-line form
//! of that false positive.
//!
//! Known residual, unchanged by this decision: these patterns treat a left-shift
//! `<<` as a heredoc marker. That imprecision is pre-existing on single lines
//! and is a property of the `<<` token test, not of the newline horizon.
//!
//! # Heredoc body mask (#14352)
//!
//! `RegexCodeBlockHeredoc` counts braces to find the end of a `(?{ ... })`
//! block, and `mask_non_code_regions` does not blank heredoc bodies. A brace in
//! heredoc *text* therefore skewed the depth count, so a real diagnostic
//! vanished from valid code — and, because the scan stops at an unmatched outer
//! block to stay linear, from every later block in the file too. The detector
//! now blanks heredoc bodies before counting.
//!
//! Blanking is itself the mechanism that hides constructs, so the mask is
//! deliberately fail-safe: it blanks only bodies whose terminator line was
//! actually found. A `<<WORD` that is not a heredoc — a left shift such as
//! `1 << FOO`, or a genuinely unterminated declaration — then costs nothing
//! instead of blinding the rest of the file. The same property makes the mask
//! degrade rather than misfire on any delimiter spelling or line ending it
//! fails to recognise.

mod detectors;
mod model;
mod utils;

pub use detectors::AntiPatternDetector;
pub use model::{AntiPattern, Diagnostic, Location, Severity};
