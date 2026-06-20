# Implementation Checklist: Recognize local() as Dynamic Scope Declaration

## Overview

The scope analyzer does not currently recognize `local` expressions as dynamic scope declarations. This causes:
1. No error detection when `local` is applied to lexical variables (which is a Perl error)
2. Missing validation for proper use of `local` with package variables
3. No distinction between dynamic and lexical scope in error reporting

This checklist implements detection and validation of `local` expressions in the scope analyzer.

## Compilation Order

The implementation must follow this sequence to ensure compilation at each step:

### Step 1: Add is_lexical field to Variable struct
**File**: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
**Line**: ~108 (Variable struct definition)

**Change**: Add `is_lexical: bool` field to Variable struct
```rust
#[derive(Debug)]
struct Variable {
    declaration_offset: usize,
    is_used: RefCell<bool>,
    is_our: bool,
    is_lexical: bool,  // NEW FIELD
    is_initialized: RefCell<bool>,
}
```

**Reason**: Must add field before it can be set or read in any code paths. This ensures all Variable construction sites can initialize it.

**Verify**: `cargo build -p perl-semantic-analyzer 2>&1 | grep -E "error|warning" | head -20`

---

### Step 2: Update Variable construction in declare_variable_parts
**File**: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
**Line**: ~213-221 (Variable construction inside declare_variable_parts)

**Change**: Set `is_lexical` field when constructing Variable

Before:
```rust
inner.insert(
    name.to_string(),
    Rc::new(Variable {
        declaration_offset: offset,
        is_used: RefCell::new(is_our),
        is_our,
        is_initialized: RefCell::new(is_initialized),
    }),
);
```

After:
```rust
// Determine if this is a lexical variable (my declaration)
// is_lexical is set based on the declarator context
// Note: declare_variable_parts is called from handle_variable_declaration
// which passes is_our = (declarator == "our")
// We need is_lexical = (declarator == "my")
//
// For now, we use a heuristic: lexical if not 'our' and not 'local'
// This assumes declare_variable_parts is only called for my/our/local declarations
let is_lexical = !is_our;  // Simplified: if not 'our', then 'my'

inner.insert(
    name.to_string(),
    Rc::new(Variable {
        declaration_offset: offset,
        is_used: RefCell::new(is_our),
        is_our,
        is_lexical,
        is_initialized: RefCell::new(is_initialized),
    }),
);
```

**Alternative approach**: Pass `is_lexical` as a parameter to declare_variable_parts. This is cleaner but requires signature change. For now, use the heuristic.

**Verify**: `cargo build -p perl-semantic-analyzer 2>&1 | grep -E "error"` (should have no errors)

---

### Step 3: Add LocalOnLexical variant to IssueKind enum
**File**: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
**Line**: ~69-90 (IssueKind enum definition)

**Change**: Add new variant to enum

Before:
```rust
pub enum IssueKind {
    VariableShadowing,
    UnusedVariable,
    UndeclaredVariable,
    VariableRedeclaration,
    DuplicateParameter,
    ParameterShadowsGlobal,
    UnusedParameter,
    UnquotedBareword,
    UninitializedVariable,
    CaptureVarWithoutRegexMatch,
}
```

After:
```rust
pub enum IssueKind {
    VariableShadowing,
    UnusedVariable,
    UndeclaredVariable,
    VariableRedeclaration,
    DuplicateParameter,
    ParameterShadowsGlobal,
    UnusedParameter,
    UnquotedBareword,
    UninitializedVariable,
    CaptureVarWithoutRegexMatch,
    /// A variable declared via `my` was used with `local` — Perl error.
    LocalOnLexical,
}
```

**Verify**: `cargo build -p perl-semantic-analyzer 2>&1 | grep -E "error"` (should have no errors; possible compiler message about unused variant handled in next step)

---

### Step 4: Implement handle_localization in calls_and_exprs
**File**: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/calls_and_exprs.rs`
**Line**: After handle_unary function (around line 102)

**Add new function**:
```rust
/// Handle `NodeKind::Unary` with op "local" or "dynamically".
///
/// Validates that localization is applied to variables that can be localized.
/// `local` can only be applied to package variables (our) or undeclared names.
/// Applying `local` to a lexical variable (my) is a Perl error.
pub(super) fn handle_localization<'a>(
    analyzer: &ScopeAnalyzer,
    node: &'a Node,
    operand: &'a Node,
    scope: &Rc<Scope>,
    ancestors: &mut Vec<&'a Node>,
    issues: &mut Vec<ScopeIssue>,
    context: &AnalysisContext<'a>,
) {
    // Extract variable from operand if it's a simple Variable node
    if let crate::ast::NodeKind::Variable { sigil, name } = &operand.kind {
        // Check if variable exists in scope
        if let Some(var) = scope.find_variable_parts(sigil, name) {
            // If variable is lexical (my-scoped), this is an error
            if var.is_lexical {
                let line = context.get_line(operand.location.start);
                let full_name = format!("{}{}", sigil, name);
                issues.push(ScopeIssue {
                    kind: IssueKind::LocalOnLexical,
                    variable_name: full_name,
                    line,
                    range: (operand.location.start, operand.location.end),
                    description: format!(
                        "Cannot localize lexical variable '{}{}'; local works only with package variables",
                        sigil, name
                    ),
                });
            } else {
                // Variable exists and is not lexical (e.g., our, or package-scoped)
                // Mark it as used in this dynamic scope
                let _ = scope.use_variable_parts(sigil, name);
            }
        } else {
            // Variable does not exist in scope: implicitly a package variable
            // This is valid — local on undeclared names is allowed
            // Mark it as used
            let _ = scope.use_variable_parts(sigil, name);
        }
    }
    
    // Always recurse into operand for nested expressions (e.g., local $hash{$key})
    ancestors.push(node);
    analyzer.analyze_node(operand, scope, ancestors, issues, context);
    ancestors.pop();
}
```

**Verify**: `cargo build -p perl-semantic-analyzer 2>&1 | grep -E "error"` (should have no errors)

---

### Step 5: Update Unary handler in mod.rs to use handle_localization
**File**: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
**Line**: ~717 (Unary handler in analyze_node)

**Change**: Check op and route to appropriate handler

Before:
```rust
NodeKind::Unary { op: _, operand } => {
    calls_and_exprs::handle_unary(
        self, node, operand, scope, ancestors, issues, context,
    );
}
```

After:
```rust
NodeKind::Unary { op, operand } => {
    if op == "local" || op == "dynamically" {
        calls_and_exprs::handle_localization(
            self, node, operand, scope, ancestors, issues, context,
        );
    } else {
        calls_and_exprs::handle_unary(
            self, node, operand, scope, ancestors, issues, context,
        );
    }
}
```

**Verify**: `cargo build -p perl-semantic-analyzer 2>&1 | grep -E "error"` (should have no errors)

---

### Step 6: Add Scope::find_variable_parts helper method (if needed)
**File**: `crates/perl-semantic-analyzer/src/analysis/scope_analyzer/mod.rs`
**Line**: Check if this method exists (search for `fn find_variable_parts`)

**Check**: Does `Scope::find_variable_parts` exist? If not, add it:

```rust
/// Find a Variable in this scope or parent scopes, returning the Variable itself.
/// Returns None if variable is not declared.
fn find_variable_parts(&self, sigil: &str, name: &str) -> Option<Rc<Variable>> {
    let idx = sigil_to_index(sigil);
    let mut current_scope = self;

    loop {
        {
            let vars = current_scope.variables.borrow();
            if let Some(map) = &vars[idx] {
                if let Some(var) = map.get(name) {
                    return Some(var.clone());
                }
            }
        }
        if let Some(ref parent) = current_scope.parent {
            current_scope = parent;
        } else {
            return None;
        }
    }
}
```

If method already exists, skip this step.

**Verify**: `cargo build -p perl-semantic-analyzer 2>&1 | grep -E "error"` (check if method already exists)

---

### Step 7: Add test cases for local validation
**File**: `crates/perl-semantic-analyzer/tests/scope_and_symbol_tests.rs`
**Location**: After existing local tests (around line 710)

**Add tests**:

```rust
// 3c. local validation — error cases and edge cases

#[test]
fn scope_analyzer_local_on_lexical_error() -> Result<(), Box<dyn std::error::Error>> {
    // local on a lexical variable should error
    let code = r#"
{
    my $x = 1;
    local $x = 2;  // Error: can't localize a lexical
}
"#;
    let issues = scope_issues(code);
    let found = issues.iter().any(|i| {
        i.kind == IssueKind::LocalOnLexical && i.variable_name == "$x"
    });
    assert!(found, "should detect local on lexical variable; issues: {:?}", issues);
    Ok(())
}

#[test]
fn scope_analyzer_local_on_package_var_allowed() -> Result<(), Box<dyn std::error::Error>> {
    // local on a package variable should be OK
    let code = r#"
our $count = 0;
{
    local $count = 10;  // OK: temporarily shadows package var
}
"#;
    let issues = scope_issues(code);
    let errors = issues.iter().filter(|i| i.kind == IssueKind::LocalOnLexical).collect::<Vec<_>>();
    assert!(errors.is_empty(), "should not error on local of package var; issues: {:?}", issues);
    Ok(())
}

#[test]
fn scope_analyzer_local_undeclared_package_var_ok() -> Result<(), Box<dyn std::error::Error>> {
    // local of undeclared (implicit package) variable should be OK
    let code = r#"
{
    local $implicit_var = 5;  // OK: implicit package variable
}
"#;
    let issues = scope_issues(code);
    let errors = issues.iter().filter(|i| i.kind == IssueKind::LocalOnLexical).collect::<Vec<_>>();
    assert!(errors.is_empty(), "should not error on local of undeclared (package) var; issues: {:?}", issues);
    Ok(())
}

#[test]
fn scope_analyzer_local_nested_lexical_error() -> Result<(), Box<dyn std::error::Error>> {
    // nested block: outer is lexical, inner tries to localize
    let code = r#"
{
    my $x = 1;
    {
        local $x = 2;  // Error: can't localize outer lexical
    }
}
"#;
    let issues = scope_issues(code);
    let found = issues.iter().any(|i| {
        i.kind == IssueKind::LocalOnLexical && i.variable_name == "$x"
    });
    assert!(found, "should detect local on lexical in nested block; issues: {:?}", issues);
    Ok(())
}

#[test]
fn scope_analyzer_dynamically_treated_like_local() -> Result<(), Box<dyn std::error::Error>> {
    // dynamically (Perl 5.36+) should be treated the same as local
    let code = r#"
{
    my $count = 0;
    dynamically $count = 10;  // Error: can't dynamically-localize a lexical
}
"#;
    let issues = scope_issues(code);
    let found = issues.iter().any(|i| {
        i.kind == IssueKind::LocalOnLexical && i.variable_name == "$count"
    });
    assert!(found, "should detect dynamically on lexical; issues: {:?}", issues);
    Ok(())
}

#[test]
fn scope_analyzer_local_on_array_lexical_error() -> Result<(), Box<dyn std::error::Error>> {
    // local on array variable declared as my
    let code = r#"
{
    my @arr = (1, 2, 3);
    local @arr = ();  // Error: can't localize a lexical array
}
"#;
    let issues = scope_issues(code);
    let found = issues.iter().any(|i| {
        i.kind == IssueKind::LocalOnLexical && i.variable_name == "@arr"
    });
    assert!(found, "should detect local on lexical array; issues: {:?}", issues);
    Ok(())
}
```

**Verify**: `cargo test -p perl-semantic-analyzer scope_analyzer_local_on_lexical_error 2>&1`

---

### Step 8: Run full test suite
**Command**: `cargo test -p perl-semantic-analyzer --lib 2>&1 | tail -50`

**Expected**: All tests pass. Look for:
- New tests passing (green)
- Existing local-related tests still passing
- No regressions in scope_and_symbol_tests.rs

**Verify**: `cargo test -p perl-semantic-analyzer --lib -- --nocapture 2>&1 | grep -E "test result|FAILED"`

---

### Step 9: Run clippy and fmt
**Commands**:
```bash
cargo clippy -p perl-semantic-analyzer --lib 2>&1 | grep -E "error|warning" | head -20
cargo fmt -p perl-semantic-analyzer 2>&1 | head -10
```

**Expected**: No errors, only informational warnings if any.

**Verify**: Both commands complete without errors.

---

## Summary Table

| Step | File | Line | Change Type | Status |
|------|------|------|-------------|--------|
| 1 | `mod.rs` | ~108 | Add field to struct | Must compile after step 1 |
| 2 | `mod.rs` | ~213-221 | Update Variable construction | Must compile after step 2 |
| 3 | `mod.rs` | ~69-90 | Add enum variant | Must compile after step 3 |
| 4 | `calls_and_exprs.rs` | ~102+ | New function | Must compile after step 4 |
| 5 | `mod.rs` | ~717 | Update Unary handler | Must compile after step 5 |
| 6 | `mod.rs` | TBD | Add helper method if needed | Optional |
| 7 | `scope_and_symbol_tests.rs` | ~710+ | Add test cases | Tests must pass after step 7 |
| 8 | N/A | N/A | Full test suite | All tests must pass |
| 9 | N/A | N/A | Lint & format | No errors |

---

## Key Notes

1. **Scope::find_variable_parts()**
   - Check if this method exists before adding in Step 6
   - If it doesn't exist, must add it for handle_localization to check is_lexical flag
   - Method should return `Option<Rc<Variable>>` to access the Variable's is_lexical field

2. **is_lexical heuristic (Step 2)**
   - Current heuristic: `is_lexical = !is_our`
   - This works because declare_variable_parts is called from handle_variable_declaration
   - handle_variable_declaration knows the declarator (my/our/local)
   - More robust approach: pass declarator to declare_variable_parts, but requires signature change

3. **Builtin special variables (Step 4)**
   - handle_localization checks Variable existence before accessing is_lexical
   - Builtin special vars are NOT registered in scope (declarations.rs line 37)
   - So handle_localization will treat them as undeclared/package-scoped (valid)
   - This matches existing behavior and doesn't introduce regressions

4. **Testing**
   - Red TDD builder will add failing tests
   - These steps add actual test implementations (not red tests)
   - Ensure tests match the acceptance criteria from acceptance.md

5. **Error messages**
   - LocalOnLexical description should clarify that local only works with package variables
   - Message: "Cannot localize lexical variable '$x'; local works only with package variables"
