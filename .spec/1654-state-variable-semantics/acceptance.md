# Acceptance Criteria: #1654 — Fix scope-analyzer: state variables not distinguished from my

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| Single `state $x = 0` declaration in function scope | Variable tracked as state, marked initialized, no error | Baseline case |
| Two `state $x` declarations in same function/block scope | First succeeds; second triggers `VariableRedeclaration` error | Core fix: state cannot be redeclared |
| `state $x` in outer block, `state $x` in nested block | Both succeed, tracked as separate variables | State follows block-scoping |
| `my $x; my $x;` | Redeclaration error (unchanged behavior, baseline) | my behavior unchanged |
| `our $x; our $x;` | Silent accept (unchanged behavior, baseline) | our behavior unchanged |
| `use vars qw($x); state $x = 0;` | State declaration shadows the package global, no error | Normal shadowing rules apply |

All tests pass: `cargo test -p perl-semantic-analyzer`
No clippy warnings: `cargo clippy -p perl-semantic-analyzer`
Formatted: `cargo xtask fmt`

## §Hazards

| Class | Invariant | Surface (file:fn) | Required adversarial test |
|---|---|---|---|
| ID/ref-space collision | N/A — no numeric ID allocation | N/A | N/A |
| Bounds/overflow | Variable sigil indexing does not panic on malformed input | `mod.rs::Scope::declare_variable_parts`, `mod.rs::sigil_to_index` | Test with empty sigil, non-standard sigil, extremely long variable name |
| Protocol-safety | Change is internal; does not affect LSP/DAP protocol | `declarations.rs::handle_variable_declaration` | N/A — not protocol-facing |
| Scanner literal/comment blindness | N/A — scope analyzer is AST-based | N/A | N/A |
| Test-encodes-the-bug | Must write adversarial test for state redeclaration | `scope_and_symbol_tests.rs::scope_state_redeclaration_error` | Test must fail with current code, pass with fix |
| Coverage/measurement integrity | New test cases must cover state-specific behavior | `scope_and_symbol_tests.rs` | Run cargo tarpaulin; verify >90% branch coverage for is_state paths |

## §Contracts

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| Variable declaration semantic distinction | perldoc perlsub — State Variables | Implementation distinguishes state from my: cannot be redeclared in same scope, follows block-scoping |
| Scope and variable tracking invariant | docs/reference/PARSER_CONTRACTS.md — Variable Declaration Scope | Scope analyzer tracks all declared variables with correct scope boundaries per declarator type |
| Issue-kind classification | IssueKind enum in mod.rs | State redeclaration uses existing IssueKind::VariableRedeclaration |

## §API-Shape

| Item | Kind | Signature / Range | Dup-risk (grep result) | Caller count |
|---|---|---|---|---|
| `Variable::is_state` | struct field | `is_state: bool` | grep: 0 matches — new field | 1 initial + 1 read in Variable construction |
| `declare_variable_parts(is_state)` | method parameter | `is_state: bool` parameter | grep: 2 callers in declare_variable_parts_in_context | 2 total |
| `declare_variable_parts_in_context(is_state)` | method parameter | `is_state: bool` parameter | grep: 3 existing call sites | 3 callers |

## §Test-Grid

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| State variable single declaration | positive | `scope_state_variable_extracted` (exists) | State variables recognized |
| State variable redeclaration in same scope | negative/adversarial | `scope_state_redeclaration_error` (new) | VariableRedeclaration issue raised |
| State in outer and inner blocks | positive | `scope_state_in_nested_blocks_allowed` (new) | Separate scope tracking confirmed |
| State variable with initializer | positive | `scope_state_initialization_tracking` (new) | is_initialized tracked correctly |
| My variable still rejects redeclaration | negative/regression | (extend existing) | my behavior unchanged |
| Our variable still accepts redeclaration | positive/regression | (extend existing) | our behavior unchanged |

## §Blast-Radius

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| `scope_issues()` test helper | perl-semantic-analyzer (tests) | Direct call to ScopeAnalyzer::analyze | None — return type unchanged | No update required |
| Symbol extraction downstream | perl-workspace, perl-lsp-rs | Dependency on perl-semantic-analyzer | None — does not depend on Variable struct internals | No update required |
| Dead code detector | perl-semantic-analyzer | May eventually use is_state flag | Deferred to follow-up issue | No update required |

Must-not-touch boundary:
- Parser (tree-sitter-perl/grammar.js) — already correctly produces declarator="state"
- AST (perl-ast/src/ast.rs) — already stores declarator string
- Other semantic analyzer modules, DAP, LSP, pragma tracker
