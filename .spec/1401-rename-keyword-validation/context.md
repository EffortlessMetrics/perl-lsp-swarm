# Context: Rename Keyword Validation

## Problem Statement

The LSP rename provider currently allows renaming symbols to reserved Perl keywords (e.g., `if`, `while`, `sub`, `package`). This violates Perl syntax rules and can silently break user code.

**Impact**: Users can accidentally create invalid code like `sub if { ... }` or `package while { ... }`.

**Root cause**: The validation layer exists in `crates/perl-lsp-rs-core/src/providers/rename/validate.rs` but is not called by the main LSP rename handler (`crates/perl-lsp-rs/src/runtime/language/rename.rs`).

## Verification

**Current state** (verified on branch `working-1216` from commit 913a2fe14):
- `crates/perl-lsp-rs-core/src/providers/rename/validate.rs` line 59: `is_rename_keyword(name)` check exists in `validate_name()`
- `crates/perl-lsp-rs/src/runtime/language/rename.rs` line 740: `normalize_rename_target()` validates basic syntax (identifier chars, sigil matching) but does NOT call `validate_name()` or `is_rename_keyword()`
- `crates/perl-lsp-rs/src/runtime/language/rename.rs` line 322: `scoped_lexical_rename_edits()` uses `RenameProvider` which does call validation, but only for lexical (my/state) renames
- `crates/perl-lsp-rs/tests/lsp_rename_tests.rs`: No test for keyword rejection in rename

**Keyword list** (from `crates/perl-lexer/src/keywords/mod.rs`):
```
RENAME_KEYWORDS = ["and", "else", "elsif", "eq", "for", "foreach", "if", "last", 
                   "local", "my", "ne", "next", "not", "or", "our", "package", 
                   "redo", "require", "return", "state", "sub", "unless", "until", 
                   "use", "while"]
```
This list is canonical and already tested in `perl-lexer`.

## Design Decisions

### 1. **Where to add validation**
**Chosen**: `normalize_rename_target()` in `crates/perl-lsp-rs/src/runtime/language/rename.rs` (lines 524-585)

**Rationale**:
- This is the single validation bottleneck for all rename paths
- Called by both `handle_prepare_rename()` and `handle_rename_workspace_inner()`
- Early validation prevents edits from being generated
- Consistent with existing error handling pattern (returns `JsonRpcError`)

**Alternatives considered**:
- Add to `RenameProvider::rename()`: Requires plumbing LSP error type into core provider (bad separation of concerns)
- Add to `scoped_lexical_rename_edits()` only: Misses workspace rename path which is the primary path
- Add to `handle_rename_workspace_inner()` directly: Less reusable, duplicates logic

### 2. **Error code and message**
**Chosen**: LSP error code `-32602` (InvalidParams) with message `"Cannot rename to reserved keyword '{name}'"`

**Rationale**:
- `-32602` is standard LSP error for invalid request parameters
- Message is user-friendly and specific
- Clients expect this error code for validation failures

**Alternative**: Return empty workspace edit (no edits). **Rejected**: Clients may not treat empty edit as failure; error response is clearer.

### 3. **Scope: Context-aware rules**
**Chosen**: Phase 1 rejects ALL keyword renames (subroutines AND variables)

**Rationale**:
- Perl allows `$if`, `$while`, etc. as variable names (sigil + keyword) — this is valid syntax
- Phase 1 focuses on the critical safety issue: subroutine/package rename to keywords (always invalid)
- Phase 2 can refine with SymbolKind-aware rules (allow keywords for variables)
- Current code path doesn't track SymbolKind at validation layer; requires plumbing from semantic analyzer

**Limitation**: Variable renames like `my $var = 1; rename $var to $if` will be incorrectly rejected in Phase 1
- **Mitigation**: Document as Phase 2; open follow-up issue if user complaints arise
- **Precedent**: Perl itself allows `$if` but warns with `Possible unintended interpolation` — reject-now-refine-later is acceptable

## Prior Art and Research

**Perl semantics**:
- Keywords are reserved only in bareword/unquoted contexts
- `$if` is valid (variable with name "if")
- `&if` is valid (subroutine with name "if" with explicit sigil — rare)
- `if` (bare) is always reserved
- `sub if { }` is a SYNTAX ERROR at compile time

**LSP precedent**:
- TypeScript/JavaScript LSP: Rejects rename to `function`, `class`, `const`, etc.
- Python LSP: Rejects rename to `def`, `class`, `import`, etc.
- Approach: Validate new name against language's keyword list before generating edits

**Perl LSP ecosystem**:
- Perl Critic warning: `ProhibitReservedWords` — discourage but allow variable naming with keywords
- Grep LSP: No rename support (out of scope)
- Padre IDE: Rename module lacks keyword validation (issue/bug)

## Alternatives Rejected

### Alternative 1: Keyword validation in prepare-rename only
**Approach**: Reject keyword in `handle_prepare_rename()`, allowing workspace rename to bypass
**Rejected**: Inconsistent; client might see "prepare says no, but rename succeeds"

### Alternative 2: Configuration flag to disable keyword validation
**Approach**: Add `strictRenameKeywordValidation: bool` to client config
**Rejected**: Over-engineering; Perl syntax is fixed, no reason to bypass validation

### Alternative 3: Allow keywords for variables, reject for subroutines
**Approach**: Use SymbolKind from semantic analysis to enable context-aware rules
**Rejected**: Too much plumbing for Phase 1; defer to Phase 2

## Testing Strategy

**Positive tests** (existing):
- Valid identifier rename (`new_name = "valid_name"`)
- Sigil preservation (`rename @items to @values`)
- Cross-file workspace rename

**Negative tests** (new):
- Keyword rejection (`new_name = "if"` → error)
- Multiple keywords (`while`, `package`, `sub`)
- Sigil + keyword edge case (`$if` → validates bare name "if", rejects)

**Adversarial tests**:
- Case sensitivity (`IF` is not a keyword → allowed)
- Keyword + suffix (`if_` is not a keyword → allowed)
- Empty or whitespace names (caught by existing validation first)

**Regression tests**:
- All existing rename tests must pass
- No changes to prepare-rename behavior (read-only)
- Scoped lexical rename must still work

## Implementation Notes

**Import statement**:
```rust
use perl_lexer::is_rename_keyword;
```
Already available in `perl-lexer` crate (public function).

**Code placement**:
```rust
// In normalize_rename_target(), after is_valid_identifier() check:
if is_rename_keyword(bare_name) {
    return Err(JsonRpcError {
        code: -32602,
        message: format!("Cannot rename to reserved keyword '{}'", bare_name),
        data: None,
    });
}
```

**Testing code**:
- Use existing `LspHarness` test harness from `lsp_rename_tests.rs`
- Open document with subroutine
- Call `textDocument/rename` with keyword as `newName`
- Assert error response (not success with empty edits)

## Follow-up Work

1. **Phase 2**: Context-aware validation (allow variable-to-keyword, reject subroutine-to-keyword)
   - Requires `SymbolKind` propagation through `normalize_rename_target()`
   - Opens issue: #1401-phase-2-context-aware-keyword-validation

2. **Documentation**: Update LSP feature docs to note keyword validation
   - File: `docs/reference/LSP_IMPLEMENTATION_GUIDE.md`
   - Note: Rename now validates against `perl_lexer::RENAME_KEYWORDS`

3. **Related issues**:
   - #8551: Parser keyword classification (ensures RENAME_KEYWORDS is exhaustive)
   - #1401 (this issue): Rename keyword validation

## Open Questions

**Q: Should we validate against all Perl keywords or only control-flow keywords?**
A: Use canonical `RENAME_KEYWORDS` list from `perl-lexer` crate. This is the authoritative source already tested. Control-flow subset can be reviewed separately if stricter rules are desired.

**Q: What if user has legitimate reason to name a subroutine with a keyword?**
A: In Perl, this is a compile-time syntax error. LSP is right to reject it at edit time. If user genuinely needs a keyword, they can use `&keyword` with explicit sigil (rare edge case for Phase 2).

**Q: Will this break existing workflows?**
A: Unknown without user telemetry. Risk is low because users who rename to keywords are already getting broken code. The LSP change makes the failure explicit at edit time instead of at runtime.
