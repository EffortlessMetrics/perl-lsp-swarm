# Acceptance Criteria: #1854 — add recursion depth guard to parse_unary

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| Normal unary operator: `!$x` | Parses successfully | Single-level unary |
| Nested unary operators: `!!!$x` | Parses successfully | Moderate nesting (~5 levels) |
| Moderate nesting: `!!!!!!!!!!$x` | Parses successfully | Within limits (~10 levels) |
| Deep nesting: 130+ unary operators | Returns ParseError::NestingTooDeep | Exceeds MAX_RECURSION_DEPTH (128) |
| Unary minus: `-$x` | Parses successfully | Different operator token |
| Unary plus: `+$x` | Parses successfully | Different operator token |
| Bitwise NOT: `~$x` | Parses successfully | Different operator token |
| Typeglob dereference: `*$x` | Parses successfully | Typeglob handling in parse_unary |
| Pre-increment: `++$x` | Parses successfully | Pre-increment operator |
| Pre-decrement: `--$x` | Parses successfully | Pre-decrement operator |
| Nested with different operators: `!-~+$x` | Parses successfully | Mix of operator types |
| Very deep nesting (200 levels) | Returns ParseError::NestingTooDeep | Well above threshold |

All tests pass: `cargo test -p perl-parser-core`
No clippy warnings: `cargo clippy -p perl-parser-core`
Formatted: `cargo xtask fmt`

## §Hazards

| Class | Invariant | Surface (file:fn) | Required adversarial test |
|---|---|---|---|
| **Bounds/overflow (PARSER-1)** | Recursion depth guards prevent stack overflow on deeply nested unary expressions | `crates/perl-parser-core/src/engine/parser/expressions/unary.rs:parse_unary` | `test_parse_unary_exceeds_max_depth`: construct expression with 200+ nested unary operators and verify NestingTooDeep error |
| **Performance regression (PARSER-2)** | Recursion guard check (inline, hot path) must not degrade parsing performance on normal code | `crates/perl-parser-core/src/engine/parser/expressions/unary.rs:parse_unary` | `test_parse_unary_normal_performance`: parse file with normal unary nesting (5-10 levels) and verify no measurable regression |
| **Guard re-entry (PARSER-3)** | Internal recursive calls must use parse_unary_inner to avoid double-counting depth on each recursive step | `crates/perl-parser-core/src/engine/parser/expressions/unary.rs:parse_unary_inner` | `test_parse_unary_guard_not_double_counted`: parse expression with 64 nested unary operators and verify it succeeds (would fail if guard re-entered on each step) |
| **External caller compatibility (PARSER-4)** | External callers (postfix.rs, precedence.rs) continue to use parse_unary and benefit from guard without code change | `crates/perl-parser-core/src/engine/parser/expressions/postfix.rs:line 100`, `crates/perl-parser-core/src/engine/parser/expressions/precedence.rs:lines 821, 1020` | `test_postfix_unary_depth_guarded`: parse method call with deeply nested prefix on object reference and verify guard protects it |
| **Error message clarity (PARSER-5)** | ParseError::NestingTooDeep must include depth and max_depth for diagnostics | `crates/perl-parser-core/src/engine/parser/helpers.rs:check_recursion` | `test_parse_unary_error_message`: verify error includes "NestingTooDeep { depth: X, max_depth: Y }" format |
| **Test-encodes-the-bug (PARSER-6)** | Guard must catch pathological input (adversarial nesting) before it causes stack overflow | `crates/perl-parser-core/tests/parse_unary_recursion_guard.rs` | `test_parse_unary_adversarial_nesting`: generate pathological nested unary expression and verify graceful error, not panic/segfault |

**Subsystem-specific defaults consulted**: docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md — PARSER (PARSER-1 through PARSER-4 selected; PARSER-5 and PARSER-6 are cross-subsystem error-handling and adversarial coverage)

## §Contracts

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| **Recursion depth protection** | PARSER_CONTRACTS.md (if exists) or docs/concepts/ | `parse_unary` now enforces the same recursion depth limit (MAX_RECURSION_DEPTH = 128) that other parser entry points (`parse_primary`, `parse_statement`, etc.) enforce. Extends the depth-guard contract to cover unary expression parsing, closing a gap where deeply nested unary operators could bypass protection. |
| **Parser error contract** | Core ParseError enum definition | `parse_unary` now returns `ParseError::NestingTooDeep` (already defined) on depth violation, consistent with other parser functions. No new error type required. |
| **Token stream invariant** | Token consumption must be atomic or recoverable | On NestingTooDeep error, parse_unary propagates immediately without consuming extra tokens, preserving caller's ability to recover and re-sync. |

## §API-Shape

| Item | Kind | Signature / Range | Dup-risk (grep result) | Caller count |
|---|---|---|---|---|
| `parse_unary` | private method | `fn parse_unary(&mut self) -> ParseResult<Node>` (wrapper, no change to signature) | 0 — internal to Parser impl block | 3 external calls (postfix.rs:100, precedence.rs:821, precedence.rs:1020) + 8 internal calls (now to parse_unary_inner) |
| `parse_unary_inner` | private method (new) | `fn parse_unary_inner(&mut self) -> ParseResult<Node>` (renamed from parse_unary body) | 0 — implementation detail, only called from parse_unary wrapper and internal recursion | N/A (new) |

N/A (public) — Neither function is public API. Both are private methods of the Parser struct, used only internally by the parser engine.

## §Test-Grid

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| Normal single-level unary | positive | `test_parse_unary_single_operator` | Single operator parses without error |
| Nested unary (5 levels) | positive | `test_parse_unary_moderate_nesting` | Moderate nesting within limit parses successfully |
| Nested unary (60 levels) | positive | `test_parse_unary_high_nesting_within_limit` | High nesting near limit parses successfully |
| Empty/missing operand | negative | `test_parse_unary_missing_operand` | Graceful error on missing operand (not NestingTooDeep) |
| Unary at EOF | negative | `test_parse_unary_trailing_operator` | Standalone operator handled (implicitly creates undef operand) |
| Depth limit exceeded (130 levels) | negative | `test_parse_unary_depth_exceeded` | Bounds/overflow — NestingTooDeep error returned |
| Depth limit exceeded (200 levels) | negative | `test_parse_unary_depth_far_exceeded` | Bounds/overflow — NestingTooDeep error returned with correct depth count |
| Guard not double-counted (64 levels) | adversarial | `test_parse_unary_guard_not_re_entered` | Performance/correctness — verify internal recursion does not re-trigger guard per step |
| Postfix operator after unary | positive | `test_parse_unary_with_postfix_chain` | External caller path — unary followed by method call/subscript parses correctly |
| Mixed operator types (deep) | positive | `test_parse_unary_mixed_operators_nested` | Different operators (!,-,~,+,++,--) can nest normally |
| Pathological alternating nesting | adversarial | `test_parse_unary_adversarial_alternating` | Guard catches adversarial input (e.g., `!-!-!-...$x` with 150 alternations) |
| Error message format | positive | `test_parse_unary_error_includes_depth` | Error message includes depth and max_depth fields |

## §Blast-Radius

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| `parse_postfix` | perl-parser-core | direct call (parse_postfix calls parse_unary) | None — parse_unary signature unchanged, guard is transparent | None |
| `parse_binary` / precedence chain | perl-parser-core | direct call (precedence.rs lines 821, 1020 call parse_unary) | None — guard is transparent to callers | None |
| `parse_statement` | perl-parser-core | indirect (statement parsing uses expression parsing) | None — no call-site changes | None |
| Parser public interface | perl-parser-core | parse() entry point | None — guard improves safety, no API change | None |
| LSP server | perl-lsp-rs | uses parser library | Positive — improved robustness against pathological Perl; no code change needed | None |
| DAP server | perl-dap | uses parser library | Positive — improved robustness; no code change needed | None |

Must-not-touch boundary:
- LSP/DAP protocol handlers — this is a parser-internal change
- Public API of perl-parser-core — no public functions modified
- Test corpus or fixture files — no changes to test inputs

One-line summary: Adding a recursion guard to parse_unary (a private method) closes a safety gap without changing any public API, caller code, or downstream dependencies. The change is purely defensive and backward-compatible.
