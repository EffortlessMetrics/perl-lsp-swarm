# Context: #1860 — fix(lexer): =begin...=end POD blocks incorrectly terminated at =cut instead of =end FORMAT

## Problem

The Perl lexer incorrectly uses `=cut` as the universal terminator for all POD directives. Per the Perl POD specification (perldoc perlpod), `=begin FORMAT` blocks must be terminated by `=end FORMAT` with a matching format token, and `=for FORMAT` blocks terminate at the next blank line or next POD directive — not at `=cut`.

This causes the lexer to silently consume valid Perl code that follows correct POD block terminators. For example:

```perl
my $before = 1;
=begin html
<b>bold</b>
=end html
my $x = 1;  # This line is incorrectly consumed as part of the POD block
```

The lexer searches for `=cut` even after seeing `=end html`, consuming `my $x = 1;` as part of the POD block instead of lexing it as Perl code tokens. If the file has no `=cut` at all, the lexer consumes everything to EOF.

**User impact:** Perl source files with POD blocks using the spec-correct terminators (`=end FORMAT` or blank-line-separated `=for` blocks) are silently mis-tokenized, leading to LSP features (hover, completion, goto-definition) failing on code after the POD block.

**Scope:** This is a medium-complexity lexer fix. The logic is localized to the POD detection branch (lines 680–724 in `crates/perl-lexer/src/lib.rs`), and test updates are confined to `crates/perl-lexer/tests/pod_skipping_tests.rs`. No consumer API changes are required.

## Why this approach

**Root cause:** The lexer implements a single POD termination strategy for all directives, treating =begin/=for identically to =pod/=head1/etc.

**Solution:** Distinguish between three termination rules:
1. **=begin FORMAT...=end FORMAT** → Search for the matching =end FORMAT (capturing the FORMAT token from the =begin line).
2. **=for FORMAT** → Search for the next blank line or next POD directive (=pod, =begin, =head*, etc.).
3. **=pod, =head*, =over, =item, =back, =encoding** → Search for =cut (preserve existing behavior).

This three-way branch is the minimal fix that satisfies the Perl POD spec without over-generalizing. Each path handles its own termination rule and returns the correct position to resume tokenization.

**Design decisions:**
- **Helper functions:** Two new internal helper functions (`skip_until_end_format` and `skip_until_blank_or_pod_directive`) encapsulate the two new termination rules. They can be implemented as inline closures or static helper fns; the choice is a code-style preference. (The current codebase uses inline closures for similar POD logic.)
- **FORMAT token capture:** Extract the FORMAT token immediately after detecting =begin or =for. The token is a single word (whitespace-delimited), per POD spec. For simplicity, do not handle Perl comments in the FORMAT token region in this fix (they are rare and out of scope).
- **EOF handling:** If the terminator is never found before EOF, consume to EOF (preserving current behavior for robustness).
- **Whitespace handling:** Allow arbitrary whitespace between =end and the FORMAT token, per POD spec.
- **No API changes:** The `next_token()` method signature and public behavior are identical. This is a transparent bug fix.

## Alternatives rejected

1. **Option A: Heuristic-based approach** — "Assume =begin blocks are short; terminate at the next blank line instead of =end FORMAT." **Rejected:** This breaks the POD spec and would incorrectly terminate blocks that span multiple paragraphs. The POD spec's explicit =end FORMAT is the correct rule.

2. **Option B: Single unified scanner for all POD blocks** — "Implement a generic POD-block parser that handles all three rules with a single state machine." **Rejected:** Over-engineering. The three rules are semantically distinct and better handled by three separate code paths. The simpler, explicit approach is more maintainable.

3. **Option C: Relegate =begin/=for to a separate lexer mode** — "Switch to a different lexer mode when =begin is detected, then back to normal mode at =end." **Rejected:** The lexer does not use lexer modes; all POD handling is in a single branch. Introducing a mode system would require architectural changes outside the scope of this fix.

## Prior art / duplicates

**Related but different:** Issue #1627 (POD indentation handling in LSP) and PR #1511 (POD formatting) touch POD, but address LSP-level handling, not lexer tokenization. This fix is orthogonal — it ensures the lexer correctly identifies POD block boundaries before LSP-level formatting is applied.

**No duplicate:** A sweep of the codebase confirms no existing implementation of the three-rule POD termination logic. The fix is new to the lexer.

**Perl spec authority:** perldoc perlpod (official Perl documentation, available via `perldoc perlpod` in any Perl installation) defines the three termination rules. This fix brings the lexer into conformance with the official spec.

## Links

- **Issue:** #1860
- **Ratification (plan-reviewed):** [https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1860#issuecomment-4757250014](commented by EffortlessSteven)
- **Research verification:** Issue #1860 marked `research-reviewed`; Perl POD spec claims verified by research-verifier
- **Perl POD spec:** [perldoc perlpod](https://perldoc.perl.org/perlpod) — official Perl documentation for POD directives and termination rules
- **Lexer source:** `crates/perl-lexer/src/lib.rs` lines 680–724 (POD detection branch)
- **Tests:** `crates/perl-lexer/tests/pod_skipping_tests.rs` (existing POD tests; to be updated and expanded)
- **Hazard class:** PARSER-1 (Literal/comment/raw-string blindness) from `docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md` — POD block scanners must skip delimiters inside string literals
- **Related incidents:**
  - [docs/learnings/2026-06-coverage-gate-measurement.md](Motivation for PARSER-1 hazard row: scanner-blindness to string literals caused coverage measurement errors)
  - Conceptual parallel: [docs/concepts/hazard-class-invariants.md](Class 4: Scanner literal/comment blindness)

## Implementation notes for builder

1. **Order of operations:** Implement helper functions first (Step 2–4 of the checklist), then refactor the POD detection branch to use them (Step 2), then update tests (Steps 5–7).

2. **Line number sensitivity:** PR #1873 (malformed hex/binary/octal) is currently open and touches `crates/perl-lexer/src/lib.rs`. If it merges before this build, line numbers may shift. Use `grep "=begin"` to locate the POD detection branch and verify line numbers in the checklist.

3. **Test-encodes-the-bug:** The existing test `pod_directive_types_are_all_skipped` currently masks the bug by using =cut for all directives. Update this test to use correct terminators BEFORE implementing the fix. Verify that the updated test fails with the current lexer, then implement the fix and verify the test passes. This proves the test was encoding the buggy behavior.

4. **Adversarial test for PARSER-1:** Add a test that supplies =begin or =for inside a string literal and asserts it is not treated as a POD directive. This validates the hazard row for PARSER-1 (literal/comment/raw-string blindness).

5. **No breaking changes:** Code relying on the buggy behavior (consuming everything after valid POD terminators) will see a change in token stream. That is the intended fix. No breaking changes to the public API.

## Verification checklist for builder

Before submitting the PR:
- [ ] All three POD termination rules are implemented correctly per perldoc perlpod
- [ ] `pod_directive_types_are_all_skipped` uses correct terminators for each directive
- [ ] New test `test_begin_end_pod_blocks_terminate_correctly` covers all three sub-scenarios
- [ ] Adversarial test for PARSER-1 (literal blindness) is present
- [ ] No inline `#[cfg(test)]` blocks added to production source (tests in `tests/` directory)
- [ ] `cargo test -p perl-lexer` passes
- [ ] `cargo test --workspace --lib` passes (no regressions in consumers)
- [ ] `cargo xtask fmt` passes
- [ ] `cargo clippy -p perl-lexer` passes
