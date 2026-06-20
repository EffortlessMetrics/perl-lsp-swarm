# Context: use overload Subroutine References

Issue: #1667 — Extract use overload subroutine references for find-references tracking

---

## Problem Statement

When a subroutine reference appears in a `use overload` declaration, it is not tracked as a symbol reference. This breaks:
- **find-all-references**: Overload declarations are invisible in reference queries
- **rename refactoring**: Renaming a subroutine used in overload does not update the overload declaration
- **unused symbol detection**: Subroutines only referenced in overload may be flagged as unused

### Root Cause

The Perl parser (`perl-parser-core/src/engine/parser/declarations.rs`) stores `use` statement arguments as `Vec<String>`, not as AST nodes. The symbol extractor (`perl-semantic-analyzer/src/analysis/symbol.rs`) has handlers for specific pragmas like `constant`, `Class::Tiny`, `EV`, but not for `overload`.

### Evidence

**Example code**:
```perl
package Vector;
use overload '+' => \&add;
sub add { my ($self, $other) = @_; ... }
```

**Current behavior**:
- find-references('add') returns:
  - Line 3: subroutine definition
  - (no other references — overload location missing)

**Expected behavior**:
- find-references('add') returns:
  - Line 2: use overload declaration (reference via `\&add`)
  - Line 3: subroutine definition

---

## Decision: Option B (Special-Case Overload)

### Why Option B over Option A

**Option A (AST Restructuring)**:
- Change `NodeKind::Use { args: Vec<String>, ... }` to `args: Vec<Node>`
- Update parser to collect AST nodes instead of stringifying them
- Fixes this issue for ALL pragmas (parent, base, overload, etc.)
- Impact: M-L (5-10 files, parser changes, broader refactoring)
- Benefit: Correct architectural approach, enables future pragma handling

**Option B (Special-Case Overload)**:
- Add `module == "overload"` check in symbol extractor
- Parse string arguments with regex to find `\&SUB` patterns
- Create SymbolReference entries for each found ref
- Impact: XS (1 file, ~100-150 LOC, no parser changes)
- Benefit: Ships this iteration, pragmatic, isolated

**Rationale for Option B**:
1. **Urgency**: Overload is the immediate blocker (#1667); parent/base can be a follow-up
2. **Risk**: Option A requires AST changes that could cascade; Option B is isolated
3. **Value**: 80% of the use case (overload is widely used in CPAN; parent/base less common)
4. **Simplicity**: Regex parsing is straightforward; Option A refactoring is complex
5. **Future-proof**: Option A can still be done later if needed; Option B doesn't block it

---

## Alternatives Considered

### Alternative 1: Ignore for Now
- **Rationale**: Static analysis has limits; source filters and macros also hide symbols
- **Cost**: continue accepting find-references blind spots, higher support load
- **Rejected**: Valid use case, pragmatic fix available, low risk

### Alternative 2: Document the Limitation
- **Rationale**: Add note to LSP capability matrix or release notes
- **Cost**: Users see incomplete references, no solution
- **Rejected**: Fix is available; documentation doesn't solve the problem

### Alternative 3: Warn at Parse Time
- **Rationale**: Emit diagnostic "overload subroutine references not tracked"
- **Cost**: Too noisy if adoption is wide; diagnostic fatigue
- **Rejected**: Fixes the symptom, not the root cause

---

## Related Issues

- **#1647**: Incomplete semantic extractions for other pragmas (goto-definition, diagnostics)
- **#1611**: Parent epic for E6 Navigation theme (this issue is a dependency)
- **#1686**: Epic ordering constraint (depends on #1611)

---

## Implementation Notes

### How the Symbol Extractor Works

1. Parser emits `NodeKind::Use { module: "overload", args: ["+", "\\&add", ...], ... }`
2. `SymbolExtractor::visit_node()` dispatches to `NodeKind::Use` handler (line 826)
3. Current handler calls `synthesize_*_symbols()` for specific modules:
   - `constant` → `synthesize_use_constant_symbols`
   - `Class::Tiny` → `synthesize_class_tiny_use_attrs`
   - `EV` → `synthesize_ev_framework_symbol`
4. New handler (Option B):
   - `overload` → `synthesize_overload_references` (NEW)
   - Parses args as operator => reference pairs
   - Extracts `\&SUB` patterns
   - Creates SymbolReference entries in the symbol table

### Reference Format in Args

The parser stores overload args as consecutive strings:
```
use overload '+' => \&add, '-' => \&subtract;
↓
args: ["+", "\\&add", "-", "\\&subtract"]
```

The `\` is preserved in the string representation (backslash is not an escape character in the string value itself; it's a Perl sigil).

### Regex Pattern

Extract subroutine name after `\&`:
```rust
if let Some(sub_name) = ref_str.strip_prefix("\\&") {
    // sub_name is "add", "subtract", or "Math::add" (qualified)
}
```

This pattern is safe because:
- Overload operators only accept references (never bare names like in `use parent`)
- Subroutine references are always `\&SYMBOL`
- String refs like `'stringify'` start with quote, will be skipped

### Edge Cases Handled

1. **Single operator**: `use overload '""' => \&stringify;` → extract "stringify"
2. **Multiple operators**: Iterate `args.chunks(2)` to process all pairs
3. **String refs**: `use overload '""' => 'stringify';` → skip (not `\&` prefix)
4. **Inline subs**: `use overload '+' => sub { ... };` → skip (args[1] won't start with `\&`)
5. **Qualified names**: `use overload '+' => \&Math::add;` → extract "Math::add" (qualified)

---

## Testing Strategy

### Unit Tests (perl-semantic-analyzer)

File: `crates/perl-semantic-analyzer/tests/use_overload_subroutine_refs_test.rs`

```rust
#[test]
fn test_use_overload_operator_subroutine_reference_extraction() {
    let source = r#"
        package Vector;
        use overload '+' => \&add, '-' => \&subtract;
        sub add { ... }
        sub subtract { ... }
    "#;
    
    let mut parser = Parser::new(source);
    let ast = parser.parse().unwrap();
    let table = SymbolExtractor::new().extract(&ast);
    
    // Verify SymbolReferences created for both add and subtract
    assert!(table.references.iter().any(|r| r.name == "add"));
    assert!(table.references.iter().any(|r| r.name == "subtract"));
}
```

### LSP Integration Tests (perl-lsp-rs)

File: `crates/perl-lsp-rs/tests/lsp_bdd_workflows.rs` (or new file)

**Scenario 1: find-references includes overload location**
```gherkin
Given a workspace with file "Vector.pm":
  package Vector;
  use overload '+' => \&add;
  sub add { my ($self, $other) = @_; $self }

When I request textDocument/references for "add" at line 3, column 6
Then the response includes:
  - Location at line 2 (overload declaration)
  - Location at line 3 (subroutine definition)
```

**Scenario 2: rename updates overload declaration**
```gherkin
Given a workspace with file "Vector.pm" (as above)

When I request textDocument/rename for "add" to "add_vectors" at line 3, column 6
Then the response includes WorkspaceEdit with:
  - Change at line 2: use overload '+' => \&add_vectors;
  - Change at line 3: sub add_vectors { ... }

When applied, find-references('add_vectors') includes both locations
```

---

## Verification Checklist

- [ ] Unit test passes: symbol extractor creates SymbolReferences for overload refs
- [ ] Unit test for edge cases (string refs, inline subs, qualified names)
- [ ] LSP test: find-references includes overload location
- [ ] LSP test: rename updates overload declaration
- [ ] Existing tests pass: `cargo test --workspace --lib`
- [ ] No regressions: `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs -- --test-threads=2`
- [ ] Code quality: `cargo fmt --all && cargo clippy --workspace`

---

## Future Work

### Option A Refactoring (Deferred)

If broader pragma support is needed (parent, base, etc.), implement Option A:
1. Change AST: `args: Vec<String>` → `args: Vec<Node>`
2. Update parser to collect AST nodes instead of strings
3. Update symbol extractor to walk node trees (more robust than regex)
4. Removes fragility: pattern changes won't break extraction

**Trigger for Option A**:
- Multiple pragmas need symbol extraction (parent, base, Moose attributes)
- Parser stability issues (args stringification changes)
- Broader Perl semantic coverage becomes priority

### Parent/Base Support (Blocked)

`use parent` and `use base` have similar issues (references in pragma args invisible to symbol extractor). Option B doesn't address these. They can be handled with the same approach (special-casing) or deferred to Option A refactoring.

---

## Links

**Issue**: #1667
**Epic**: #1686 (E6 Navigation theme dependency)
**Related**: #1647 (other incomplete semantic extractions), #1611 (dependency)
**Parser contract**: `docs/reference/PARSER_CONTRACTS.md` (use statement structure)
**Symbol extractor**: `crates/perl-semantic-analyzer/src/analysis/symbol.rs` (lines 826-843)
**Parser code**: `crates/perl-parser-core/src/engine/parser/declarations.rs` (lines 710-850, use parsing)
