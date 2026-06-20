# Context: #1654 — Fix scope-analyzer: state variables not distinguished from my

## Problem

The scope analyzer treats `state` variable declarations identically to `my` declarations. This misses critical semantic differences:

1. **State initialization happens only once per function** (persistence), not each call
2. **State redeclaration should be an error**, whereas `my` allows redeclaration
3. **State scope is block-scoped like `my`** (NOT function-wide as initially misunderstood)
4. **Unused state variables should be reported** (unlike `our`)

Current behavior misclassifies:
- `state $x = 0; state $x = 1;` — should error, currently accepted
- `state $counter = 0;` — loses persistence semantics tracking
- State redeclaration checking is not enforced

## Why this approach

The plan-reviewer and research verifier confirmed:

1. **Parser already distinguishes state**: tree-sitter-perl/grammar.js:697 produces `declarator="state"`
2. **AST carries the declarator string**: perl-ast/src/ast.rs line 1597 stores declarator, allowing distinction
3. **Scope analyzer uses binary `is_our` check**: declarations.rs line 28 has no state handling
4. **Adding `is_state` flag is minimal**: No parser/AST changes needed, just internal scope_analyzer logic

## Alternatives rejected

- **Alt 1: Treat state redeclaration like our (silent accept)**
  - **Rejected**: Perl 5 spec forbids redeclaring state. Silent accept misleads users.

- **Alt 2: Create separate NodeKind::StateDeclaration**
  - **Rejected**: Parser already produces VariableDeclaration with declarator="state". New NodeKind ripples to parser and all tests.

- **Alt 3: Add new IssueKind::StateRedeclaration**
  - **Rejected**: VariableRedeclaration is already the right category and already implemented.

## Prior art / duplicates

**Search conducted**: Searched codebase for existing `is_state` tracking in scope analyzer.

**Finding**: No existing implementation. Variable struct only has `is_our: bool`. New location (Variable::is_state field) is canonical.

**Related**: Builtins documentation (builtins.rs:767) already correctly documents state semantics, confirming alignment with existing knowledge.

## Research verification corrections

The research-verifier identified two FALSE claims:

1. **FALSE: "state scope is function-wide, not block-scoped like my"**
   - **Actual**: Perl documentation states "state follows the same scoping rules as my" (block-scoped)
   - **Impact**: None. Scope_analyzer already uses block-scoped tracking. No changes needed to scope-handling logic.

2. **FALSE: "Variable struct has `is_state: bool` field (mod.rs line 108)"**
   - **Actual**: Variable struct has `is_our: bool`, not `is_state`
   - **Impact**: Confirmed — adding `is_state: bool` is the correct fix.

## Links

- **Issue**: #1654
- **Related issues**: #1659 (state scope — corrected), #1657 (local() dynamic scope), #1661 (our redeclaration validation), #1664 (state without initializer warnings)
- **Perl semantics**: perldoc perlsub — State Variables section
- **Crate docs**: CLAUDE.md in perl-semantic-analyzer

## Known unknowns

None — research verification confirmed all key facts.
