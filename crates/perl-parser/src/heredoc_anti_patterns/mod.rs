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
//! Known residual, unchanged by this decision: the *detector patterns* treat a
//! left-shift `<<` as a heredoc marker. That imprecision is pre-existing on
//! single lines and is a property of the `<<` token test, not of the newline
//! horizon. The body mask below does not share it — see the term-position rule.
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
//! actually found. A genuinely unterminated declaration then costs nothing
//! instead of blinding the rest of the file, and the same property makes the
//! mask degrade rather than misfire on any delimiter spelling or line ending it
//! fails to recognise.
//!
//! The fail-safe is a backstop, not the primary guard against reading a left
//! shift as a heredoc; it only helps when the operand never reappears as a line.
//! Perl itself distinguishes the two by **position**, not by delimiter spelling,
//! and the mask now applies that rule: `<<` after a complete term (a number, a
//! variable, a closing bracket, a string) is the shift operator, while `<<`
//! after an operator, separator, opener, or bareword function name starts a
//! heredoc. Confirmed against `perl -c` 5.38, which compiles `1<<FOO`,
//! `$y<<FOO` and `f()<<FOO` with no `FOO` line present but rejects `print
//! <<FOO` and `my $t = <<FOO` with "Can't find string terminator". Errors here
//! are asymmetric by design: declining to mask a real heredoc restores the
//! pre-mask status quo, whereas masking a shift blanks live code.
//!
//! "Complete term" is not decidable from the token alone, which is why the rule
//! carries three explicit admissions rather than one predicate. A list operator
//! puts what follows back into argument position, so `print $fh <<EOF` and
//! `print {$fh} <<EOF` are heredocs even though a variable and a `}` precede
//! the `<<`, and `CORE::print <<EOF` is one even though a qualified name does —
//! while `$y << FOO`, `$h{k} << FOO` and `CORE::time << FOO` remain shifts.
//! Every row there is a `perl -c` result, not a reading of the grammar.
//!
//! Two residuals are accepted there, both losing coverage rather than blanking
//! code. A filehandle block longer than `FILEHANDLE_BLOCK_BUDGET` is not matched
//! back to its opening brace, so its heredoc is not admitted: the backward scan
//! runs at every `<<` preceded by `}` and is quadratic without a bound, and
//! precomputing whole-file brace matches would add an `O(n)` pass and `O(n)`
//! memory to every call to serve a construct an order of magnitude larger than
//! any observed filehandle expression. That precomputation is the fix if a real
//! case appears.
//!
//! The unqualified bareword stays irreducible: Perl consults the symbol table,
//! reading `somefunc<<FOO` as a shift when no such sub is declared and
//! `Foo::bar<<FOO` as a heredoc when `Foo::bar` is defined. A mask that cannot
//! see declarations cannot reproduce that, so barewords are admitted and the
//! fail-safe absorbs the rest.
//!
//! The mask's own traversal is monotone and indexed rather than line-walking.
//! Walking lines and searching forward per declaration is quadratic on input
//! this detector must survive — 4000 unterminated `print <<A;` lines cost ~53 ms
//! that way against ~5 ms indexed, with the gap widening — and the `(?{<<`
//! scaling input cannot catch it, since that input has no newline and no
//! delimiter, so the mask returns immediately. `antip_body_mask_work_scales_linearly`
//! covers the mask separately for that reason.
//!
//! `EvalHeredocDetector` consumes the same body mask for its origin check. It
//! scans raw source, so without it an `eval '...<<...'` appearing as heredoc
//! *text* reported PL805 for an eval that never executes.

mod detectors;
mod model;
mod utils;

pub use detectors::AntiPatternDetector;
pub use model::{AntiPattern, Diagnostic, Location, Severity};
