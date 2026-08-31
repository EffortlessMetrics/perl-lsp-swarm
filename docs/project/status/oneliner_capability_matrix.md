# Perl Command-Line Analysis Capability Matrix

Status: generated
Owner: perl-lsp maintainers
Generator: `cargo xtask oneliner-capability-matrix`
Check: `cargo xtask oneliner-capability-matrix --check`
Evidence source: [`crates/perl-parser-core/tests/command_line_oneliners.rs`](../../../crates/perl-parser-core/tests/command_line_oneliners.rs)

"One-liner support" is not one capability. This matrix separates it into eight layers so that accepting a program body is never reported as understanding a command line. Each row carries its own evidence: a row claiming `supported` or `partial` must cite fixtures that are live in the conformance corpus, and the generator fails instead of rendering a claim that has none.

Scope of the check, exactly: it proves each citation is real and reachable — present in the corpus as running code, not commented out, quoted, `#[ignore]`d, or disabled by `cfg`, and declaring every switch the row claims. It does not run the fixtures. Execution is `cargo test -p perl-parser-core --test command_line_oneliners`, which every earned row names.

Support vocabulary:

- `supported`: earned for this layer, with cited fixture evidence and an invocable command.
- `partial`: earned in part; the row names the exact missing layer rather than a generic caveat.
- `unsupported`: not earned. A boundary control may still prove where the behavior stops.
- `not_applicable`: outside this lane, owned elsewhere.

Evidence is parser-side only. A fixture from the parser corpus can earn layer 1 and nothing above it, so parser-only proof cannot be promoted into an end-to-end support claim.

## Layer summary

| # | Layer | Earned rows | Total rows |
| --- | --- | --- | --- |
| 1 | Parser-body acceptance | 5 | 15 |
| 2 | Structured-argv decoding | 0 | 1 |
| 3 | Source composition and provenance | 0 | 1 |
| 4 | Implicit runtime/loop context | 0 | 1 |
| 5 | Compile-time feature/module/include context | 0 | 1 |
| 6 | Diagnostics and editor operations | 0 | 1 |
| 7 | Shell-specific extraction adapters | 0 | 3 |
| 8 | Trusted differential-oracle coverage | 0 | 1 |

## 1. Parser-body acceptance

The parser accepts the source body Perl would hand it for this form. This is a syntax claim about the body text only: it does not decode the switch, synthesize the implicit loop, or supply interpreter setup.

| Subject | Status | Missing layer | Evidence | Boundary control | Invocable | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| -e | `supported` | — | `e_print_literal`; `e_grep_diamond_input`; `e_map_diamond_input`; `e_explicit_diamond_loop`; `e_sort_diamond_with_for_modifier`; `e_parenthesized_split_slice`; `e_printf_special_variables`; `positive_idioms_have_typed_ast_hir_and_source_range_proof` | `negative_controls_keep_context_errors_and_boundaries_visible` | `cargo test -p perl-parser-core --test command_line_oneliners` | Single-fragment program bodies parse cleanly with typed AST/HIR and source ranges. The `-e` switch itself is not decoded; that is layer 2. |
| -n | `supported` | — | `ne_implicit_topic_match`; `ne_skip_blank_lines`; `ne_end_phase_counter`; `ne_argv_and_input_line_number`; `ne_begin_phase_input_record_separator`; `ne_capture_group` | `negative_controls_keep_context_errors_and_boundaries_visible` | `cargo test -p perl-parser-core --test command_line_oneliners` | Bodies that rely on `$_`, `next`, `$ARGV`, `$.`, and phase blocks parse. The implicit read loop is not synthesized; that is layer 4. |
| -p | `supported` | — | `pe_implicit_topic_substitution`; `pe_implicit_topic_transliteration`; `pe_trim_whitespace`; `positive_idioms_have_typed_ast_hir_and_source_range_proof` | `negative_controls_keep_context_errors_and_boundaries_visible` | `cargo test -p perl-parser-core --test command_line_oneliners` | Substitution and transliteration bodies lower to typed HIR. The implicit print-back loop is not synthesized; that is layer 4. |
| -a | `supported` | — | `lane_first_autosplit_field`; `lane_join_autosplit_fields` | — | `cargo test -p perl-parser-core --test command_line_oneliners` | Both cited bodies read `@F`, the variable autosplit populates, so the evidence is specific to this switch rather than incidental. Autosplit inserts no source text; whether `@F` is actually populated is layer 4. |
| -l | `partial` | switch-specific evidence: no cited body contains anything `-l` changes, so acceptance here is incidental to the `-lane` bundle rather than proof about record separators | `lane_first_autosplit_field`; `lane_join_autosplit_fields` | — | `cargo test -p perl-parser-core --test command_line_oneliners` | The cited bodies arrive through the `-lane` bundle and parse, but unlike `-a` and its `@F`, nothing in them is specific to record-separator handling. Promoting this row needs a body whose parse depends on `-l`. |
| -E | `unsupported` | — | — | — | — | No corpus case supplies an `-E` body, so feature-enabled constructs such as `say` carry no command-line parser-body evidence here. |
| -F | `unsupported` | — | — | — | — | No corpus case declares `-F`. The autosplit pattern lives in argv, not in the body, so evidence must come from layer 2. |
| -0 / -g | `unsupported` | — | — | — | — | No corpus case declares `-0` or `-g`. A body that sets `$/` directly is ordinary Perl and does not evidence the switch. |
| -I | `unsupported` | — | — | — | — | Include roots are argv values, not body syntax. No corpus case declares `-I`. |
| -M | `unsupported` | — | — | — | — | Import requests are argv values, not body syntax. No corpus case declares `-M`. |
| -m | `unsupported` | — | — | — | — | No-import module requests are argv values, not body syntax. No corpus case declares `-m`. |
| repeated source fragments | `unsupported` | — | — | `negative_controls_keep_context_errors_and_boundaries_visible` | — | Repeated `-e` fragments join with newlines. The corpus asserts single-line inputs and keeps a multiline negative control, so multi-fragment composition is an excluded boundary rather than an untested gap. |
| -- | `unsupported` | — | — | `negative_controls_keep_context_errors_and_boundaries_visible` | — | The argv terminator never reaches the body. The option-contamination control shows raw switch text parsing as an ordinary unary expression rather than being recognized. |
| explicit script file | `not_applicable` | — | — | — | — | A named script is ordinary file parsing owned by the general parser corpus and generated parser status, not by the command-line lane. |
| stdin program | `unsupported` | — | — | — | — | Reading a program from stdin (`perl -`) has no corpus case and no ingestion path in this lane. |

## 2. Structured-argv decoding

A command line is decoded into typed arguments: switch clusters, switch-attached values, repeated source fragments, the `--` terminator, and residual operands.

| Subject | Status | Missing layer | Evidence | Boundary control | Invocable | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| whole layer | `unsupported` | — | — | `negative_controls_keep_context_errors_and_boundaries_visible` | — | No switch decoder exists in the workspace. The option-contamination control proves the boundary directly: `-ne print;` parses as a unary expression on `ne`, which is what a parser without argv decoding must do. |

## 3. Source composition and provenance

Decoded fragments are composed into one analyzable source unit whose offsets map back to original command coordinates.

| Subject | Status | Missing layer | Evidence | Boundary control | Invocable | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| whole layer | `unsupported` | — | — | `negative_controls_keep_context_errors_and_boundaries_visible` | — | Nothing composes fragments or maps offsets back to command coordinates. Corpus ranges are offsets into a single body string, which is not command provenance. |

## 4. Implicit runtime/loop context

Switch-implied runtime structure (the `-n`/`-p` read loop, `-a` autosplit, `-l` record handling) is modeled as semantic context rather than left to the reader.

| Subject | Status | Missing layer | Evidence | Boundary control | Invocable | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| whole layer | `unsupported` | — | — | `negative_controls_keep_context_errors_and_boundaries_visible` | — | The implicit `-n`/`-p` loop is never synthesized. The explicit-loop control asserts a written loop stays a real `Foreach` with a typed `LoopShell`, so an implicit wrapper cannot be mistaken for one. |

## 5. Compile-time feature/module/include context

Switch-implied compile-time context — `-M`/`-m` imports, `-I` include roots, and `-E` feature enablement — informs name resolution and diagnostics.

| Subject | Status | Missing layer | Evidence | Boundary control | Invocable | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| whole layer | `unsupported` | — | — | — | — | `-M`, `-m`, `-I`, and `-E` supply no name-resolution or feature context. Body `BEGIN` blocks parse, but a parsed phase block is not switch-derived compile-time context. |

## 6. Diagnostics and editor operations

Diagnostics and each LSP/editor operation answer against command-line source at original command coordinates.

| Subject | Status | Missing layer | Evidence | Boundary control | Invocable | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| whole layer | `unsupported` | — | — | — | — | No LSP or editor operation accepts a command line as a document. Diagnostics, hover, completion, and navigation over command-line source are all unearned. |

## 7. Shell-specific extraction adapters

A host shell's quoting and tokenization rules are decoded before argv decoding. Each shell is a separate adapter; none is implied by core structured-argv support.

| Subject | Status | Missing layer | Evidence | Boundary control | Invocable | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| POSIX shell | `unsupported` | — | — | — | — | No adapter decodes POSIX shell quoting. Layer 2 remaining unsupported means there is nothing for an adapter to feed. |
| PowerShell | `unsupported` | — | — | — | — | PowerShell quoting differs from POSIX and needs its own adapter and evidence. It is never implied by POSIX or by structured-argv support. |
| cmd.exe | `unsupported` | — | — | — | — | cmd.exe quoting differs from both POSIX and PowerShell and needs its own adapter and evidence. |

## 8. Trusted differential-oracle coverage

A trusted oracle compares this toolchain's understanding against real `perl` behavior for the same command line.

| Subject | Status | Missing layer | Evidence | Boundary control | Invocable | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| whole layer | `unsupported` | — | — | — | — | No command line is executed against real `perl` and compared. The corpus is a parser-side contract and makes no behavioral agreement claim. |

## Corpus fixtures

20 fixtures are available as evidence: 18 switch-bundle cases and 2 typed-proof targets.

| Fixture | Declared switches |
| --- | --- |
| `e_explicit_diamond_loop` | `-e` |
| `e_grep_diamond_input` | `-e` |
| `e_map_diamond_input` | `-e` |
| `e_parenthesized_split_slice` | `-e` |
| `e_print_literal` | `-e` |
| `e_printf_special_variables` | `-e` |
| `e_sort_diamond_with_for_modifier` | `-e` |
| `lane_first_autosplit_field` | `-lane` |
| `lane_join_autosplit_fields` | `-lane` |
| `ne_argv_and_input_line_number` | `-ne` |
| `ne_begin_phase_input_record_separator` | `-ne` |
| `ne_capture_group` | `-ne` |
| `ne_end_phase_counter` | `-ne` |
| `ne_implicit_topic_match` | `-ne` |
| `ne_skip_blank_lines` | `-ne` |
| `pe_implicit_topic_substitution` | `-pe` |
| `pe_implicit_topic_transliteration` | `-pe` |
| `pe_trim_whitespace` | `-pe` |
| `negative_controls_keep_context_errors_and_boundaries_visible` | typed proof |
| `positive_idioms_have_typed_ast_hir_and_source_range_proof` | typed proof |
