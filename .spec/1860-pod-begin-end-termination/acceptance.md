# Acceptance Criteria: #1860 — fix(lexer): =begin...=end POD blocks incorrectly terminated at =cut instead of =end FORMAT

## §Behavior

The Perl POD specification defines three distinct block termination rules. This fix ensures the lexer correctly implements all three.

| Input / Condition | Expected Result | Notes |
|---|---|---|
| `=begin html\n<b>bold</b>\n=end html\nmy $x = 1;` | Lexer emits tokens for `my $x = 1;` after the =end html directive | Code following =end html is lexed normally, not consumed as POD. |
| `my $before = 1;\n=begin html\n<b>bold</b>\n=end html\nmy $x = 1;` | Two `my` tokens emitted: one for `$before` and one for `$x` | =begin...=end block is correctly skipped; code before and after is emitted. |
| `=for html <i>italic</i>\n\nmy $y = 2;` | Lexer emits tokens for `my $y = 2;` after the blank line | =for terminates at blank line, not at =cut. |
| `=for html <i>italic</i>\n=for text plain\n\nmy $y = 2;` | Next POD directive (=for text) terminates the previous =for block | =for can also terminate at the next POD directive. |
| `my $x = 1;\n=pod\ncontent\n=cut\nmy $y = 2;` | Two `my` tokens emitted | =pod...=cut behavior is unchanged (regression check). |
| `=begin html\nno matching end\n[EOF]` | Lexer consumes to EOF without panicking | If =end html never appears, consume remainder (current behavior preserved). |
| `=begin html\n=end html\n=end html\nmy $x = 1;` | First =end html terminates the block; second =end html is emitted as code | Only the first matching =end FORMAT terminates the block. |
| `=begin html\n=end TEXT\nmy $x = 1;` | Lexer continues searching for =end html (=end TEXT does not match) | =end must have matching FORMAT token, not a different format. |
| `my $str = "=begin html"; =for test\n\nmy $x = 1;` | Entire source is tokenized; =begin in string does not start POD block | String literals mask POD directives (comment-blindness hazard). |

**All tests pass:** `cargo test -p perl-lexer`
**No clippy warnings:** `cargo clippy -p perl-lexer`
**Formatted:** `cargo xtask fmt`

## §Hazards

Seeded from `docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md` — Parser subsystem.

| Class | Invariant | Surface (file:fn) | Required adversarial test |
|---|---|---|---|
| **PARSER-1: Literal/comment/raw-string blindness** | Every byte- or char-level scanner (including POD block scanners) must skip characters inside string literals (`"..."`, `'...'`), heredoc bodies, comment regions (`#...`), and `q{}`/`qq{}`/`qw{}`/`qr{}` quote-like operators. A scanner that is correct on bare source is insufficient. | `crates/perl-lexer/src/lib.rs:next_token()` POD detection branch (lines 680–724) | `test_begin_end_inside_string_literal`: Supply input where `=begin` or `=for` appears exclusively inside a double-quoted string literal. Assert the scanner treats the string as bare text, not as a POD directive starter. Supply a second input with the directive both inside a literal and outside (e.g., `my $str = "=for test"; =for real\n\n`); assert only the outside directive is acted upon. |
| **Test-encodes-the-bug** | Error-recovery and boundary test cases must not snapshot AST variants or token streams that the current implementation cannot actually produce. Snapshotting an unreachable variant as "expected" encodes a latent lie. | `crates/perl-lexer/tests/pod_skipping_tests.rs:pod_directive_types_are_all_skipped()` | `pod_directive_types_are_all_skipped` currently uses `=cut` to terminate ALL directives (including =begin), masking the bug that =begin should terminate at =end. This test MUST be updated to use correct terminators for each directive type BEFORE the fix is implemented. Verify that updating the test alone (without implementation changes) causes the test to fail, proving the test encodes the current buggy behavior. Then implement the fix and verify the test passes. |

## §Contracts

The following Perl POD specifications define the expected behavior:

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| POD block terminators | `perldoc perlpod` § POD Directives — =begin/=end, =for, =pod | This fix implements the three distinct termination rules per Perl POD spec: (1) =begin FORMAT...=end FORMAT (terminated by matching =end FORMAT), (2) =for FORMAT (terminated by next blank line or next POD directive), (3) =pod, =head*, =over, =item, =back, =encoding (terminated by =cut). The lexer now correctly follows the spec instead of treating all directives uniformly. |

**Contract verification:** Research-verifier (score in issue #1860) confirmed that Perl 5 perldoc perlpod defines these three rules. The current lexer violates the contract by searching for =cut universally.

## §API-Shape

**N/A — No new public API surface.** This is a bug fix to the internal `next_token()` method's POD-detection branch. The method signature and public behavior are unchanged; the lexer now correctly terminates POD blocks according to the Perl spec instead of prematurely consuming code after valid terminators.

| Item | Kind | Signature / Change | Dup-risk | Caller count |
|---|---|---|---|---|
| `Lexer::next_token()` | method | No signature change; internal logic refactored | N/A | Called by all lexer consumers (parser, LSP, DAP, incremental-parsing); changes are transparent. |

## §Test-Grid

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| =begin html...=end html block terminates at =end, not =cut | positive | `test_begin_end_pod_blocks_terminate_correctly` (sub-scenario 1) | Basic =begin/=end termination works correctly. |
| =for block terminates at blank line | positive | `test_begin_end_pod_blocks_terminate_correctly` (sub-scenario 2) | =for block termination at blank line is correct. |
| =pod...=cut behavior unchanged | positive | `test_begin_end_pod_blocks_terminate_correctly` (sub-scenario 3) | Regression: existing =pod/=cut behavior preserved. |
| No matching =end FORMAT (consumes to EOF) | negative | `test_begin_end_pod_blocks_terminate_correctly` (implicit in step 6) or separate test | Graceful EOF handling when =end FORMAT never arrives. |
| =end with wrong FORMAT token does not terminate | negative | Test (e.g., `test_begin_end_format_mismatch`) | Only matching =end FORMAT terminates the block. |
| =begin/=for inside string literal are not POD directives | adversarial | `test_begin_end_inside_string_literal` | PARSER-1 hazard: comment/literal blindness. Scanner does not incorrectly parse directives inside string literals. |
| =for directive with comment after FORMAT token | adversarial | `test_for_with_comment_after_format` | FORMAT token is correctly extracted; comment does not interfere with blank-line termination. |
| Nested =begin blocks (=begin inside =begin without matching =end) | adversarial | `test_nested_begin_blocks_first_match_wins` | Only the first matching =end FORMAT terminates; nested directives do not confuse matching logic. |
| Multiple =end directives (second one is code, not POD) | adversarial | `test_multiple_end_directives` | After first =end FORMAT terminates the block, subsequent =end directives are treated as code tokens. |
| =pod/=head/=over/=item/=back/=encoding still use =cut | positive | Updated `pod_directive_types_are_all_skipped` | Directives other than =begin/=for continue to use =cut as terminator (no regression). |
| Mixed directive types in same file | integration | `test_mixed_pod_directives` (or expand pod_directive_types_are_all_skipped) | Multiple directive types in one file correctly use their respective terminators. |

## §Blast-Radius

The change is isolated to the lexer's POD scanning logic. No public API changes; all consumers (parser, LSP, DAP) use the lexer transparently via `next_token()`.

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| `perl-parser` | `crates/perl-parser/` | Lexer is a library dependency | Transparent; parser receives corrected token stream from lexer | None — lexer change is internal |
| `perl-lsp-rs` | `crates/perl-lsp-rs/` | Lexer is a library dependency | Transparent; LSP server receives corrected token stream | None — lexer change is internal |
| `perl-dap` | `crates/perl-dap/` | Lexer is a library dependency | Transparent; DAP server receives corrected token stream | None — lexer change is internal |
| `perl-incremental-parsing` | `crates/perl-incremental-parsing/` | Lexer is a library dependency | Transparent; incremental parser receives corrected token stream | None — lexer change is internal |
| Test suite (`perl-lexer/tests/`) | `crates/perl-lexer/` | Internal tests | Must update test expectations for POD termination | Update `pod_directive_types_are_all_skipped` to use correct terminators; add new tests |

**Must-not-touch boundary:**
- Parser AST (`crates/perl-ast/`) — no changes
- LSP protocol handlers (`crates/perl-lsp-rs/`) — no changes
- DAP protocol handlers (`crates/perl-dap/`) — no changes
- Other crates — lexer change is isolated and transparent

**No consumer code refactoring required:** All existing code that uses the lexer will automatically benefit from the fix (code after valid POD terminators will now be correctly tokenized instead of being silently consumed).

## §Coverage Considerations

**Inline test helpers:** No inline `#[cfg(test)]` blocks are added to `crates/perl-lexer/src/lib.rs`. All tests are in `crates/perl-lexer/tests/pod_skipping_tests.rs`, avoiding the ripr-seam tension (COV-4). If any test helpers are needed in the production source, they will be relocated to the tests directory per best practice.

**Integration test coverage:** New tests in `pod_skipping_tests.rs` exercise the refactored POD logic at the integration level (full lexer input to token stream). Adversarial tests (`test_begin_end_inside_string_literal`) validate the PARSER-1 hazard at the same level.
