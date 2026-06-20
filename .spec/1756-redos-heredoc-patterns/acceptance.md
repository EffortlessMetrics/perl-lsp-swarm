# Acceptance Criteria: #1756 — Fix ReDoS vulnerabilities in heredoc anti-pattern regex patterns

## §Behavior

Tabular summary of what the change does. One row per distinct behavior.

| Input / Condition | Expected Result | Notes |
|---|---|---|
| `<<${` followed by 1000 `a` characters (no closing `}`) | Regex completes in <10ms | Prevents catastrophic backtracking DoS |
| `<<$varname` (valid dynamic delimiter) | Still detected and reported | Fix maintains valid anti-pattern detection |
| `(?{aaaa...aaaa<<` without closing `}` (1000 chars) | Regex completes in <10ms | REGEX_HEREDOC_PATTERN bounded |
| Valid `(?{...<<...})` on single line | Still detected | Valid patterns continue to work |
| `eval 'aaaa...aaaa<<` without closing quote (1000 chars) | Regex completes in <10ms | EVAL_HEREDOC_PATTERN bounded |
| Valid `eval '...<<...'` on single line | Still detected | Valid patterns continue to work |
| `@EXPORT = qw(aaaa...aaaa` without closing `)` (1000 chars) | Regex completes in <10ms | @EXPORT qw pattern bounded |
| Valid `@EXPORT = qw(Foo Bar)` | Still detected | Valid export lists continue to work |
| Multiline anti-pattern (rare; spans `\n`) | Not detected | Acceptable tradeoff: line-boundary anchoring prevents DoS but misses rare multiline cases |
| Normal Perl source (1000+ lines) | No performance regression | Detector remains performant on realistic code |

All tests pass: `cargo test -p perl-parser && cargo test -p perl-lsp-rs`
No clippy warnings: `cargo clippy -p perl-parser && cargo clippy -p perl-lsp-rs`
Formatted: `cargo xtask fmt`

## §Hazards

One row per applicable hazard class. Copied verbatim from SUBSYSTEM_HAZARD_DEFAULTS.md for the parser subsystem.

| Class | Invariant | Surface (specific file/fn this change touches) | Required adversarial test |
|---|---|---|---|
| PARSER-1: Literal/comment blindness | The anti-pattern detector must not be fooled by regex delimiters appearing inside strings or comments. The bounding to `\n` preserves the existing `mask_non_code_regions()` call, which handles string/comment masking before pattern matching. | `crates/perl-parser/src/heredoc_anti_patterns/detectors.rs`: `DynamicDelimiterDetector::detect()`, `RegexHeredocDetector::detect()`, `EvalHeredocDetector::detect()` | `test_antip_heredoc_delimiter_in_string()` — supply `"<<${" ` inside a double-quoted string; assert no false positive |
| PARSER-2: Delimiter pairing | The bounded character classes no longer overflow on unmatched closing delimiters. The fix prevents O(n²) catastrophic backtracking by anchoring to newline boundaries. | `crates/perl-parser/src/heredoc_anti_patterns/detectors.rs` line 231, 337, 387; `crates/perl-lsp-rs/src/runtime/language/moniker.rs` line 277 | `test_antip_no_redos_dynamic_unclosed_brace()` — supply `<<${` + 5KB unclosed; assert completes <10ms |
| PARSER-3: Grammar-ambiguity positive + negative oracles | N/A — anti-pattern detection is heuristic-based, not grammar-sensitive. No ambiguous Perl construct interpretation required. | N/A | N/A |
| PARSER-4: Recovery honesty | N/A — anti-pattern detection does not produce recovery nodes or modify AST. It is a pure diagnostic pass. | N/A | N/A |
| PARSER-5: New NodeKind variant — audit non-exhaustive consumers | N/A — no new NodeKind variant added; this is a regex fix only. | N/A | N/A |
| Cross-system: Test-encodes-the-bug (Class 5) | Red-TDD tests must verify the pathological input actually completes in time before treating it as passing. Do not snapshot "completed in time" from a single run. | `crates/perl-parser/tests/` or `#[cfg(test)]` in `detectors.rs` | `test_antip_redos_guardrail()` — measure regex completion time and assert <10ms for all four patterns with pathological input |

## §Contracts

Which contracts from PARSER_CONTRACTS.md this change touches or must satisfy.

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| N/A — no parser contracts for diagnostics | PARSER_CONTRACTS.md (diagnostics are not a contract surface) | The anti-pattern detector is a lint/diagnostic pass, not a parsing contract. No LSP protocol contract or parser behavioral invariant is affected. The fix maintains the same diagnostic output for valid cases and only changes behavior on pathological (DoS) input. |

## §API-Shape

New public types, functions, enum variants, or ID-spaces introduced by this change.

N/A — this change has no new public API surface. The regex pattern definitions are static and internal to the detector implementations.

## §Test-Grid

Enumeration of test cases covering axes of variation.

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| 5KB unclosed brace input to DYNAMIC_DELIMITER_PATTERN | adversarial (ReDoS) | `test_antip_no_redos_dynamic_5kb_unclosed` | Performance guardrail: must complete <10ms |
| 5KB unclosed brace input to REGEX_HEREDOC_PATTERN | adversarial (ReDoS) | `test_antip_no_redos_regex_5kb_unclosed` | Performance guardrail: must complete <10ms |
| 5KB unclosed quote input to EVAL_HEREDOC_PATTERN | adversarial (ReDoS) | `test_antip_no_redos_eval_5kb_unclosed` | Performance guardrail: must complete <10ms |
| 5KB unclosed delimiter input to EXPORT_QW_RE | adversarial (ReDoS) | `test_antip_no_redos_export_5kb_unclosed` | Performance guardrail: must complete <10ms |
| Valid `<<${var}` on single line | positive | `test_antip_dynamic_delimiter_valid` | Still detects valid patterns |
| Valid `(?{...<<...})` on single line | positive | `test_antip_regex_heredoc_valid` | Still detects valid patterns |
| Valid `eval '...<<...'` on single line | positive | `test_antip_eval_heredoc_valid` | Still detects valid patterns |
| Valid `@EXPORT = qw(...)` | positive | `test_antip_export_qw_valid` | Still detects valid patterns |
| Heredoc delimiter inside double-quoted string | negative (string blindness) | `test_antip_delimiter_in_string` | Masked by `mask_non_code_regions()`; no false positive |
| Normal 1000-line Perl file | negative (performance) | `test_antip_normal_file_performance` | Detector completes <100ms on realistic code |
| Multiline pattern spanning `\n` (rare) | edge case | `test_antip_multiline_pattern_not_matched` | Acceptable loss: multiline anti-patterns are not detected (tradeoff for safety) |

## §Blast-Radius

Subsystems and crates that consume this change's output. Confirm each is unaffected or list the required update.

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| `detect_heredoc_antipatterns()` | `perl-lsp-rs-core` | calls `AntiPatternDetector::new().detect_all()` | Signature unchanged; diagnostics output for valid patterns unchanged; only behavior on pathological input changes | None — existing tests should pass |
| `diagnostics` LSP provider | `perl-lsp-rs` | reads diagnostics from heredoc_antipatterns provider | Diagnostic output for normal code unchanged; no new diagnostics introduced | None — existing LSP diagnostics tests pass |
| `moniker` symbol export lookup | `perl-lsp-rs` | calls `is_symbol_exported()` with new bounded regex | Performance improved; output for normal @EXPORT lists unchanged | None — existing moniker tests pass |
| Parser test suite | `perl-parser` | existing heredoc anti-pattern tests | Tests for valid patterns should still pass; pathological-input tests can be added | None — no existing snapshot changes required |

Must-not-touch boundary: 
- `crates/perl-ast/` — no AST changes
- `crates/perl-lsp/` — no LSP protocol changes
- `crates/perl-dap/` — no DAP protocol changes
- `crates/perl-parser-core/` — no parser core logic changes
- `docs/` — no contract revisions needed (diagnostics are not a contract surface)

## §Coverage-Map

N/A — no coverage tooling changes; standard diagnostic code paths.

## Summary

This fix eliminates ReDoS DoS vectors by bounding four unbounded regex patterns to line boundaries (`\n`). The fix:
1. **Eliminates catastrophic backtracking:** O(n²) behavior on unclosed delimiters → O(n) linear scan
2. **Preserves valid detection:** All well-formed anti-patterns continue to be detected
3. **Accepts rare accuracy loss:** Multiline anti-patterns are no longer detected (extremely rare in practice; acceptable tradeoff for security)
4. **Maintains interface stability:** No public API changes; no protocol changes
5. **Requires five red-TDD tests** measuring completion time on pathological 5KB inputs to verify guardrail
