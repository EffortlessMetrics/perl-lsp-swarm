# Acceptance Specification: use overload Subroutine References

Issue: #1667 — Extract use overload subroutine references for find-references tracking

**Decision**: Implement Option B (special-case overload in symbol.rs, pragmatic isolated fix)

---

## §Behavior

**Input**: Perl source with `use overload` pragma containing subroutine references via `\&SUB`

**Processing**: Symbol extractor parses overload args, identifies `\&SUB` patterns, creates SymbolReference entries

**Output**: find-references, rename, and unused-symbol detection now correctly handle overload operator targets

| Input | Condition | Expected Result | Test Name |
|-------|-----------|-----------------|-----------|
| `use overload '+' => \&add;` | Single operator with subroutine ref | SymbolReference('add') added to symbol table | `test_single_overload_operator_ref` |
| `use overload '+' => \&add, '-' => \&sub;` | Multiple operators with refs | SymbolReferences('add', 'sub') added | `test_multiple_overload_operators` |
| `use overload '""' => 'stringify';` | Operator with string ref (not \&) | No SymbolReference created (correct skip) | `test_overload_string_ref_skip` |
| `use overload '+' => sub { ... };` | Operator with inline code ref | No SymbolReference created (correct skip) | `test_overload_inline_code_ref_skip` |
| `use overload '+' => \&Math::add;` | Operator with qualified subroutine | SymbolReference('Math::add') added | `test_overload_qualified_sub_ref` |
| find-references('add') in file with overload | Overload declared, sub defined, called | References include overload location + definition + usage | `test_find_references_includes_overload` |
| rename('add' → 'add_impl') in file with overload | Overload declared, sub used | Overload declaration updated to `\&add_impl` | `test_rename_overload_subroutine` |
| find-references('add') before/after rename | Rename refactoring completes | No stale references, correct new references | `test_rename_updates_references` |
| Unused symbol detection for 'add' in overload | Sub declared, only used in overload | Symbol marked as referenced (not unused) | `test_overload_prevents_unused_detection` |

---

## §Hazards

**Subsystem**: perl-semantic-analyzer (Tier 4, symbol extraction layer)

**Hazard class**: SEM-1 (Incomplete Symbol Reference Extraction)

| ID | Class | Surface | Severity | Invariant | Mitigation |
|---|---|---|---|---|---|
| SEM-1 | Incomplete symbol reference extraction | `symbol.rs:synthesize_overload_references` | M | All `\&SUB` patterns in `use overload` args must be extracted as SymbolReferences | Unit test `test_use_overload_operator_subroutine_reference_extraction` verifies extraction; E2E test `test_find_references_includes_overload` verifies LSP-layer integration |
| SEM-2 | Parser arg stringification drift | `declarations.rs` + `symbol.rs` | L | If parser changes how Use args are formatted, regex pattern `strip_prefix("\\&")` may fail silently | Document pattern in comment; add regression test for exact string format; consider Option A refactoring if format instability observed |
| SEM-3 | Reference scope loss | `symbol.rs` SymbolTable | M | SymbolReferences created with correct ScopeId (package scope, not nested block) | Verify SymbolReference.scope_id matches package context in test |
| SEM-4 | Qualified name handling | `symbol.rs` reference extraction | M | Qualified names like `\&Math::add` must be preserved (not collapsed to bare name) | Unit test with qualified refs; verify LSP rename correctly updates both bare and qualified forms |
| SEM-5 | Non-subroutine operands | argument parsing | L | String refs like `'stringify'` and inline subs `sub { ... }` must be skipped (not created as SymbolReferences) | Unit tests for each case; pattern must check for `\&` prefix strictly |
| SEM-6 | LSP-layer integration | `references.rs`, `rename.rs` | M | Symbol references created by extractor must be found and included in find-references and rename operations | E2E LSP test `test_find_references_includes_overload` and `test_rename_overload_subroutine` verify integration |

---

## §Contracts

**Parser contract** (`crates/perl-parser-core/src/engine/parser/declarations.rs`):

- `NodeKind::Use { module: "overload", args: Vec<String>, ... }` — Arguments are stored as strings (established behavior)
- Args are presented in order: `["+", "\\&add", "<<", "\\&lshift", ...]` — fat arrows consumed, pairs normalized
- `\` prefix is preserved in the string (e.g., `"\\&add"` not `"&add"`)

**Symbol extractor contract** (`crates/perl-semantic-analyzer/src/analysis/symbol.rs`):

- `NodeKind::Use` handler must call `synthesize_*_references()` for module-specific symbol extraction
- Pattern: Check `if module == "<name>"` then call handler with args and location
- Handler creates `SymbolReference` entries in `SymbolTable`
- Existing handlers: `synthesize_use_constant_symbols`, `synthesize_class_tiny_use_attrs`, `synthesize_ev_framework_symbol`
- New handler: `synthesize_overload_references` follows same pattern

**SymbolReference contract** (shared):

- `SymbolReference { name, qualified_name, kind: SymbolKind::Subroutine, location, scope_id, ... }`
- `location` must point to the reference site (in the Use args)
- `scope_id` must be the package-level scope where use statement appears
- `qualified_name` should expand bare names with current package context (inherited from parser context)

**LSP find-references contract** (`crates/perl-lsp-rs/src/runtime/language/references.rs`):

- References collected from workspace index include both declarations and uses
- SymbolReferences from symbol extractor must be included in results
- Established behavior: Query runs on index built from symbol table

---

## §API-Shape

**New public function**:
```rust
fn synthesize_overload_references(&mut self, args: &[String], location: SourceLocation)
```

**Location**: `crates/perl-semantic-analyzer/src/analysis/symbol.rs`, `impl SymbolExtractor`

**Visibility**: Private method (not exported, called only from `visit_node`)

**No new types or public API surface** — reuses existing `SymbolReference`, `SymbolTable`, `SymbolKind::Subroutine`

**No ID-space changes** — symbol names and scope IDs follow existing conventions

**Dup-risk grep** (verify no existing handlers named similar):
```bash
grep -n "synthesize_.*_references\|synthesize_.*_symbols" crates/perl-semantic-analyzer/src/analysis/symbol.rs
```

Current matches:
- `synthesize_use_constant_symbols` (line ~850) — constants, not overloads
- `synthesize_class_tiny_use_attrs` (line ~860) — class attributes, not overloads
- `synthesize_ev_framework_symbol` (line ~830) — EV framework, not overloads

**No collision risk**: New method name `synthesize_overload_references` is distinct.

**Caller count** (methods that will call the new function):
- `visit_node` — NodeKind::Use handler (1 caller)

---

## §Test-Grid

**Test pyramid**: Unit tests in semantic-analyzer, E2E LSP tests, no integration tests (integration covered by E2E)

| Test | Category | Rows | Invariant Verified |
|------|----------|------|-------------------|
| `test_use_overload_operator_subroutine_reference_extraction` | Unit / positive | Single ref, multiple refs, qualified names | SymbolReference created for each `\&SUB` pattern |
| `test_overload_string_ref_skip` | Unit / negative | String refs like `'stringify'` | Non-`\&` refs skipped (no false positives) |
| `test_overload_inline_code_ref_skip` | Unit / negative | Inline subs `sub { ... }` | Anonymous subs skipped (not extracted as refs) |
| `test_overload_operator_ordering` | Unit / boundary | Operators in different orders | Ref extraction not sensitive to operator sequence |
| `test_find_references_includes_overload` | E2E / positive | Single file, multi-file workspace | find-references LSP call returns overload location |
| `test_rename_overload_subroutine` | E2E / positive | Rename 'add' to 'add_impl' | Overload args updated in rename response |
| `test_rename_updates_all_references` | E2E / state-transition | Pre-rename refs, post-rename refs | No stale references, all updated correctly |
| `test_overload_prevents_unused_detection` | Integration / state-transition | Unused symbol detector + overload | Sub in overload not flagged as unused |

**Red TDD sequence**:
1. All tests start failing (symbol extractor does not handle overload yet)
2. Step 2 implementation makes unit tests green
3. Step 4-5 implementations make LSP tests green
4. Step 6 verification ensures no regressions

---

## §Blast-Radius

**Direct consumers** (crates that import from perl-semantic-analyzer):

- `perl-lsp-rs` — uses `SymbolExtractor` to build workspace index, then queries for find-references and rename
- `perl-workspace` — may consume symbol table for dead code detection
- LSP providers — `references.rs`, `rename.rs` consume workspace index built from symbols

**Downstream impact**:

- find-references handler — will return more results (correct behavior, not regression)
- rename handler — will update more locations (correct behavior, not regression)
- unused symbol detection — fewer false positives (correct behavior, not regression)

**Must NOT touch** (boundaries):

- Parser boundary — no changes to `NodeKind::Use`, args stringification, or Use statement parsing
- AST boundary — no structural changes to `perl-ast`
- Type system — no new symbol kinds, changes to `SymbolKind`, or scope types
- LSP protocol — no new requests, no changes to find-references or rename contract

**Scope**: Confined to `perl-semantic-analyzer/src/analysis/symbol.rs` — extraction logic only

**Isolated change**: Only method called from existing NodeKind::Use handler (line 826)

---

## §Coverage-Map

Not applicable — this is a semantic extraction fix, not a coverage or CI change. Existing test infrastructure (unit test patterns and E2E LSP test framework) sufficient.

---

## Notes

**Option B rationale**:
- Pragmatic: Ships quickly, isolated, low risk
- Focused: Solves overload immediately without broader AST refactoring
- Testable: Symbol extraction layer tested independently, LSP integration verified

**Option A (deferred)**:
- `args: Vec<Node>` AST restructuring would be a follow-up if broader pragma support is needed
- Would fix same issue for `use parent`, `use base`, and other pragmas
- Larger scope (5-10 files), requires parser changes, higher risk of regressions
- Document as potential future improvement in context.md

**Known limitation**:
- Pattern `strip_prefix("\\&")` is fragile if parser changes string formatting
- Mitigation: Add test with exact string format; consider Option A if format instability occurs
- This is acceptable for v0.16.0 iteration; monitor for parser changes
