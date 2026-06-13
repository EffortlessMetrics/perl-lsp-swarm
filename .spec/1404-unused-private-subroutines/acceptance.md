# Acceptance Criteria: #1404 — Semantic: Dead code detection for unused private subroutines

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| Perl file with `sub _unused { }` and no calls to `_unused` | Emit `UnusedPrivateSubroutine` diagnostic at subroutine definition | Single-file scope; cross-file calls not tracked |
| Perl file with `sub _helper { }` and a call to `_helper()` within same file | No diagnostic | Presence of even one reference suppresses the warning |
| Perl file with `sub public_func { }` (no leading underscore) | No diagnostic | Only subroutines starting with `_` are checked |
| Perl file with `sub _helper { }` referenced via string eval or method call | Diagnostic emitted | No dynamic-boundary suppression; static scope analysis only |
| Empty file or file with no subroutine definitions | No diagnostics | Graceful no-op |
| Multiple unused private subroutines in same file | One diagnostic per unused private subroutine | Each gets its own ScopeIssue with correct line number |
| Package with multiple `sub _*` where some used and some unused | Diagnostics only for unused ones | Correctly distinguishes referenced from unreferenced |

All tests pass: `cargo test -p perl-semantic-analyzer && cargo test -p perl-lsp-rs-core`
No clippy warnings: `cargo clippy -p perl-diagnostics -p perl-semantic-analyzer -p perl-lsp-rs-core`
Formatted: `cargo xtask fmt`

## §Hazards

Hazard rows seeded from SUBSYSTEM_HAZARD_DEFAULTS.md for LSP and Semantic Analyzer subsystems.

| Hazard Class | Invariant | Surface (file:fn) | Required adversarial test | Risk |
|---|---|---|---|---|
| **Scanner literal/comment blindness** | Subroutine definition in a string literal or comment must NOT trigger unused-sub diagnostic | `scope_analyzer::mod.rs::analyze()` | Test: `sub _helper { } # comment: sub _unused` in string context — only first `_helper` counted | Low — parser already excludes comments/strings from AST |
| **Reference-space collision** | Private subroutine name collision (e.g., multiple files defining `sub _helper`) must not cause false negatives; scope is per-file | `scope_analyzer::mod.rs::analyze()` | Test: File A has `sub _helper { }` (unused); File B has `sub _helper { _helper(); }` — both analyzed independently, A flags as unused | Medium — implementation must ensure per-file isolation |
| **Bounds / definition exclusion** | Subroutine `<anon>` and subroutines without names must not crash or panic | `scope_analyzer::mod.rs::analyze()` | Test: Anonymous `sub { my $x = 1; }` does not cause panic or false positive | Low — symbol table already handles anon subs; name check filters them |
| **Test-encodes-the-bug** | Test fixture itself must have a verifiable private subroutine that is actually unused | `scope_analyzer_tests.rs::test_unused_private_subroutine` | Verify test source code contains `sub _unused { }` with zero calls; inspect fixture before running | Medium — easy to write test that accidentally calls `_unused` |
| **Protocol-safety** | Diagnostic emission must follow DiagnosticSeverity contract and range bounds | `scope.rs::scope_issues_to_diagnostics()` | Test: Emitted diagnostic has valid line/col range and severity is Warning (not Error/Info) | Low — existing diagnostic pipeline handles envelope; we just add a case |
| **Coverage/measurement integrity** | Coverage metrics must not regress if test for UnusedPrivateSubroutine is added | `scope_analyzer_tests.rs` | Verify coverage report includes new test; track covered lines | Low — test is additive, not replacing existing coverage |

**Subsystem-specific defaults consulted**: docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md — LSP Diagnostic Providers (LSP-1, LSP-2) and Semantic Analyzer (PARSER-2 adapted for semantic layer)

## §Contracts

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| **LSP Diagnostic Code Range** | `perl-diagnostics/src/codes/mod.rs` §Code Ranges (PL300-PL399: Subroutine) | New code `UnusedPrivateSubroutine` maps to PL305, fitting in the stable Subroutine range; no protocol-spec extension needed |
| **ScopeIssue Emission Pattern** | `scope_analyzer::mod.rs` §IssueKind enum | New `IssueKind::UnusedPrivateSubroutine` variant added; follows existing pattern for other issue kinds (e.g., `UnusedVariable`, `UnusedParameter`) |
| **Diagnostic Conversion Pipeline** | `scope.rs::scope_issues_to_diagnostics()` | Extended to handle new IssueKind; emits with `DiagnosticSeverity::Warning` and `DiagnosticTag::Unnecessary` (mirrors `UnusedVariable`/`UnusedParameter` behavior) |
| **Backwards compatibility** | `scope_issues_to_diagnostics()` vs `scope_issues_to_diagnostics_with_semantics()` | Both variants updated; no existing code breaks; UnusedPrivateSubroutine always emitted (no semantic suppression rules) |

N/A — N/A statements not applicable; all relevant contracts touched are well-defined in existing modules.

## §API-Shape

| Item | Kind | Signature / Range | Dup-risk (grep result) | Caller count |
|---|---|---|---|---|
| `IssueKind::UnusedPrivateSubroutine` | enum variant | `enum IssueKind { ... UnusedPrivateSubroutine, ... }` | `grep "UnusedPrivateSubroutine"` across crate returns: scope.rs (2 matches: match arms), scope_analyzer_tests.rs (1 match: test assertion) | 3 |
| `DiagnosticCode::UnusedPrivateSubroutine` | enum variant | `enum DiagnosticCode { ... UnusedPrivateSubroutine, ... }` | grep returns: mod.rs (definition), metadata.rs (as_str match), scope.rs (1 match in match arm) | 3 |
| Subroutine detection logic | internal function | `impl ScopeAnalyzer { fn detect_unused_private_subs(&self, ...) -> Vec<ScopeIssue> }` or inline | N/A — internal, no public export | internal only |

**Dup-risk analysis**: `UnusedPrivateSubroutine` and `UnusedPrivateSubroutine` are unique enum variants; no naming collisions with existing Rust code. `PL305` is a new diagnostic code in the stable range; no codebase collisions with PL301-PL304 or PL306+.

**Caller count**: All callers are internal to the change (match arms added in scope.rs and scope_analyzer_tests.rs); no external consumers breaking.

## §Test-Grid

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| Private subroutine with zero references | positive | `test_unused_private_subroutine_simple` | Detects `sub _helper { }` as unused; emits exactly one ScopeIssue |
| Private subroutine with exactly one reference | negative | `test_unused_private_subroutine_called` | No diagnostic when `_helper()` is called within same file |
| Public subroutine (no leading underscore) | negative | `test_public_subroutine_not_flagged` | `sub public_func { }` generates no unused-private diagnostic |
| Multiple unused private subroutines | positive | `test_multiple_unused_private_subs` | File with `sub _a { }` and `sub _b { }` emits two separate diagnostics |
| Private subroutine referenced before definition | negative | `test_unused_private_forward_ref` | Call to `_helper()` before its definition still suppresses diagnostic |
| Anonymous subroutine | negative | `test_anon_sub_not_flagged` | `my $fn = sub { }; $fn->()` does not generate unused-private diagnostic |
| Dunder method (double underscore) | negative | `test_dunder_method_not_flagged` | `sub __DEMOLISH { }` does not trigger diagnostic (not single-underscore pattern) |
| Empty file | negative | `test_empty_file_no_crash` | File with no code does not crash; emits zero diagnostics |
| Diagnostic code mapping | adversarial | `test_unused_private_sub_diagnostic_code` | ScopeIssue converts to Diagnostic with code `PL305` and severity `Warning` |
| Diagnostic tag application | adversarial | `test_unused_private_sub_unnecessary_tag` | Emitted diagnostic includes `DiagnosticTag::Unnecessary` (like `UnusedVariable`) |
| Line number accuracy | adversarial | `test_unused_private_sub_line_number` | Diagnostic range matches the exact line of `sub _helper` in source |
| Reference in method chain | negative | `test_unused_private_in_method_chain` | `$obj->_helper()` counts as a reference; no diagnostic |
| Reference via string eval | positive | `test_unused_private_string_eval_missed` | `eval "_helper()"` does NOT suppress diagnostic (string eval not tracked in static analysis) |

## §Blast-Radius

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| `scope.rs` diagnostics provider | perl-lsp-rs-core | direct (match arm) | Must handle new `IssueKind::UnusedPrivateSubroutine` variant | Add match cases in `scope_issues_to_diagnostics()` and `scope_issues_to_diagnostics_with_semantics()` |
| LSP diagnostic publishing | perl-lsp-rs | indirect (consumes Diagnostic from scope.rs) | Diagnostic will appear in editor once scope.rs emits it; no code changes needed | None — diagnostic pipeline is generic |
| Symbol extractor | perl-semantic-analyzer | none | Not directly consumed; only AST/symbol table used | None |
| Workspace index | perl-semantic-analyzer | none | Not directly consumed; single-file scope analysis | None |
| Perl::Critic integration | perl-lsp-rs-core | none | Quick-fix handler for Perl::Critic's `ProhibitUnusedPrivateSubroutines` already exists; native diagnostic complements but doesn't replace it | None |

**Must-not-touch boundary**:
- Parser (crates/perl-parser/, crates/perl-parser-core/, crates/perl-lexer/) — scope analysis is semantic only
- DAP (crates/perl-dap/) — diagnostics are LSP-only feature
- Configuration subsystem (crates/perl-lsp-config/) — opt-out via .perl-lsp.toml deferred to future issue
- Workspace index (crates/perl-workspace/) — cross-file reference tracking deferred
- Public API contracts (PARSER_CONTRACTS.md) — no parser changes
