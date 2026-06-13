# Acceptance Criteria: LSP Rename Keyword Validation

## §Behavior

| Input | Condition | Expected Result |
|-------|-----------|-----------------|
| `new_name = "if"` (keyword) | Renaming `sub foo` to keyword | LSP error response with code -32602 and message mentioning "reserved keyword" |
| `new_name = "while"` (keyword) | Renaming any symbol to keyword | Same error response, no edits produced |
| `new_name = "package"` (keyword) | Renaming to control-flow keyword | Same error response |
| `new_name = "my_var"` (non-keyword) | Renaming valid identifier | Rename succeeds, edits produced as normal |
| `new_name = "$if"` (sigil + keyword) | Renaming variable to keyword-sigil combo | Error for keyword part after sigil stripping |
| `new_name = "123invalid"` (starts with digit) | Invalid syntax before keyword check | Existing validation catches it (existing behavior) |
| Empty keyword list check | Keyword list is empty or misconfigured | Keywords still enforced via canonical `perl_lexer::RENAME_KEYWORDS` |

## §Hazards

| Hazard Class | Surface | Risk | Mitigation |
|---|---|---|---|
| **LSP-1: Protocol Contract Violation** | `textDocument/rename` request/response | Reject rename with error code; may confuse clients expecting null response for unavailable renames | Use standard LSP error code `-32602` (InvalidRequest); document as validation error in error message |
| **LSP-2: State Corruption** | Workspace edit generation | Partial edits if validation happens after some edits generated | Validation in `normalize_rename_target()` happens BEFORE any edit generation; early return prevents state contamination |
| **LSP-3: Scope Expansion** | Keyword validation reach | Validation intended for subroutines but accidentally blocks variables (Perl allows `$if`, `$while`) | Current implementation validates bare name regardless of sigil; variables with sigils should pass. Document context-aware rules as future work if needed. |
| **LSP-4: Performance Regression** | Keyword check on every rename | Unexpected latency if keyword list is huge or check is slow | Binary search via `is_rename_keyword()` is O(log n); negligible overhead. List is ~25 keywords. |
| **COV-1: Test Coverage Gap** | Keyword rejection path | Only happy path tested; corner cases missed | Add test for: keyword rejection, non-keyword success, edge cases (empty, sigil mismatch). See §Test-Grid. |
| **COV-2: Integration Gap** | Workspace rename feature integration | Changes in one path but not others (e.g., prepareRename vs rename) | Validation added to `normalize_rename_target()` which is called by both `handle_prepare_rename` and `handle_rename_workspace_inner`. |

## §Contracts

**LSP Protocol** (`textDocument/rename`):
- Request: `{ textDocument: { uri }, position: { line, character }, newName: string }`
- Response (success): `{ changes: { uri: TextEdit[] } }` — one entry per file with edits
- Response (failure): JsonRpcError with code (e.g., -32602) and message
- **Change**: Rename to keyword now returns error instead of applying rename

**Parser/Semantic Invariants**:
- Keyword list from `perl_lexer::RENAME_KEYWORDS` is canonical and sorted (binary search compatible)
- `is_rename_keyword(token: &str)` is the authoritative check; used by both core provider and LSP handler
- **Change**: No change to parser or semantic analyzer; validation is LSP-layer only

**Core Provider** (`crates/perl-lsp-rs-core/src/providers/rename/validate.rs`):
- `validate_name()` already checks keywords via `is_rename_keyword()`
- `RenameOptions::default()` has `validate_new_name: true`
- **Change**: LSP handler now calls similar validation before passing to provider

## §API-Shape

**New/Modified Public API**:
- None — no new types or functions
- Changes are internal to `LspServer::normalize_rename_target()` which is private

**Sigils and Keywords**:
- Current: `$if`, `@while`, `%package` are valid variable names (sigil + keyword bare name allowed)
- After change: Validation strips sigil and checks bare name
- **Design decision**: Block keyword bare names globally; context-aware rules (allow variable-to-keyword) deferred to follow-up issue
- **Implication**: `$if` will be rejected in phase 1; can be allowed in phase 2 via SymbolKind check

**Error Code**:
- `-32602` (InvalidRequest) is standard LSP error for invalid parameters
- Clients expect error response (not null) for invalid input

**Keyword List Source**:
- Single source of truth: `perl_lexer::RENAME_KEYWORDS` (constant slice of ~25 strings, sorted)
- No duplication or divergence from lexer/parser keyword classification

## §Test-Grid

| Test Name | Input | Expected | Invariant |
|---|---|---|---|
| `test_rename_subroutine_to_keyword_fails` | Rename `sub foo` to `if` | Error response, no edits | Keyword rejected at validation layer, before edit generation |
| `test_rename_to_while_keyword` | Rename `sub bar` to `while` | Error with message `"reserved keyword"` | Different keyword still rejected |
| `test_rename_to_control_keyword` | Rename `sub process` to `package` | Error response | Control-flow keyword rejected |
| **Positive**: `test_rename_valid_identifier_succeeds` (existing) | Rename to `new_name` (non-keyword) | Edits produced, rename succeeds | No regression; valid names still work |
| **Positive**: `test_rename_array_valid` (existing) | Rename `@items` to `@values` | Success with both sigils preserved | Non-keyword renames unaffected |
| **Negative**: `test_rename_to_empty_name` (existing) | Rename to empty string | Error (invalid identifier) | Early exit before keyword check |
| **Negative**: `test_rename_to_digit_leader` (existing, `test_rename_invalid_identifier_is_rejected`) | Rename to `1abc` | Error (invalid syntax) | Non-keyword validation unaffected |
| **State-transition**: Rename same symbol twice | 1. Rename `foo` to `bar` (success) 2. Rename `bar` to `if` (error) | Step 1 succeeds, Step 2 fails cleanly | Validation does not corrupt prior state |
| **Adversarial**: Keyword in mixed case | Rename to `IF` (uppercase) | Keyword check is case-sensitive; `IF` is not in keyword list; rename allowed | Perl keywords are lowercase; uppercase identifiers are allowed |
| **Adversarial**: Keyword with trailing underscore | Rename to `if_` | Not a keyword; rename succeeds | Validation matches exact keyword, not prefix |

## §Blast-Radius

**Consumers** (code that calls the affected function):
- `handle_prepare_rename()` calls `normalize_rename_target()` indirectly (via token validation) — **LOW RISK**: prepare is read-only, no edits affected
- `handle_rename_workspace_inner()` calls `normalize_rename_target()` — **HIGH RISK**: this is the main rename path; must preserve success for valid names
- `scoped_lexical_rename_edits()` uses `RenameProvider` directly, not `normalize_rename_target()` — **NO CHANGE**: already has validation

**Downstream crates**:
- `perl-lsp-rs` tests: Rename tests in `crates/perl-lsp-rs/tests/lsp_rename_tests.rs` — must pass with new validation
- `perl-lsp-rs-core`: No changes to core provider; validation is LSP-only
- `perl-lexer`: No changes; `is_rename_keyword()` is used as-is

**Boundary changes**:
- **LSP protocol boundary**: Error response for keyword rename (client-facing change) — clients must expect error, not null
- **Core/LSP boundary**: No change; core `validate_name()` already implemented, LSP handler now calls similar logic
- **Must-not-touch boundaries**:
  - Parser AST (no change to symbol representation)
  - Semantic analyzer (no change to symbol resolution)
  - Workspace index (no change to indexing)
  - Keyword list (use canonical `perl_lexer::RENAME_KEYWORDS` only)

**Scope boundaries**:
- **In scope**: Reject rename to keywords in LSP handler
- **Out of scope**: Context-aware rules (allow variable-to-keyword); deferred to follow-up #1401-phase-2
- **Out of scope**: Keyword list maintenance (managed by `perl-lexer` crate)
- **Out of scope**: prepareRename behavior beyond basic syntax (current behavior is unchanged)
