# Acceptance Criteria: #1394 Parser Return/LoopControl Precedence Standardization

---

## §Behavior

**Input → Expected Result**

| Input | Condition | Expected AST | Invariant |
|---|---|---|---|
| `return $x or die;` | At statement level | Binary OR node with Return child on left | Word operators don't bind inside return value |
| `return or die;` | No return value | Binary OR node with Return child on left | Empty return stops at word operator |
| `return $x and $y;` | Word AND operator | Binary AND node with Return child on left | Consistent with `or` behavior |
| `return $x if $cond;` | Statement modifier | StatementModifierIf wrapping Return | Modifiers still apply after value ends |
| `$x = return 1;` | Assignment context (expression-level) | Assignment with Return value child | Expression-level return unchanged |
| `$x = return $y or die;` | Assignment + word operator | Assignment containing Return; `or die` is outside assignment | Modifiers apply to assignment, not return |
| `return $x, $y;` | Comma operator | Return consuming `$x, $y` as value | Comma has higher precedence than return |
| `next or die;` | Loop control at statement level | Binary OR with LoopControl on left | Loop control already correct (no fix needed) |
| `last LABEL or die;` | Loop control with label | Binary OR with labeled LoopControl on left | Label parsing unaffected |

---

## §Hazards

| Class | Surface | Invariant | Rationale |
|---|---|---|---|
| PARSER-1 | `parse_return()` in statements.rs | Return value uses `parse_assignment()`, not `parse_expression()` | Stop at word-operator boundaries to prevent incorrect precedence |
| PARSER-2 | Word operators (or/and/xor) in expression context | Operator precedence table: return/loop-control > word operators | Statement-level return must respect Perl op precedence |
| PARSER-3 | Statement modifiers after return | Modifiers apply to return node, not its value | `return $x if $cond` must be Statement Modifier wrapping Return |
| TEST-1 | Regression on expression-level return | `parse_return_expr()` unchanged; `control_flow_expr_tests.rs` passes | Assignment/ternary/short-circuit contexts must not regress |
| TEST-2 | Loop control edge cases | `parse_loop_control()` unchanged; `loop_control_tests.rs` passes | Labeled loop control must continue to work with word operators |
| SCOPE-1 | No changes to NodeKind or public API | Return node structure identical before/after | Binary compatibility maintained |

---

## §Contracts

**PARSER_CONTRACTS.md References:**

- **Contract 4 — NodeKind Classification:** `NodeKind::Return` and `NodeKind::LoopControl` must remain classified as Expression nodes (no change)
- **Contract 2 — Indirect-Object Ambiguity:** Return as autoquoted hash key before `=>` (e.g., `return => 1`) is unchanged; `is_keyword_before_fat_arrow()` guards prevent incorrect dispatch

**LSP/DAP Protocol:**

- No LSP or DAP protocol changes; semantic tokens, hover, definition lookup all work with existing AST structure
- Return statements are callable locus points; no change to call-site classification

**Operator Precedence Reference:**

From `perlop` (official Perl operator precedence table):
```
or xor and        # Very low precedence
||= and = xor =   # Low precedence
||                # Lower precedence
&&                # Logical and
|                 # Bitwise or
^                 # Bitwise xor
&                 # Bitwise and
return            # HIGH precedence (return value stops here)
last next redo    # HIGH precedence (same level as return)
```

**Fix:** Make statement-level return respect the precedence table by using `parse_assignment()` (which stops before word operators).

---

## §API-Shape

**Changes to Public Surface:**

- `NodeKind::Return { value: Option<Box<Node>> }` — **UNCHANGED**
- `NodeKind::LoopControl { op: String, label: Option<String> }` — **UNCHANGED**
- Parser public methods:
  - `Parser::parse()` — **UNCHANGED**
  - `Parser::new()` — **UNCHANGED**

**Internal Changes (NOT public):**

- `Parser::parse_return()` — now calls `parse_assignment()` instead of `parse_expression()` for value parsing
- `Parser::parse_return_expr()` — **UNCHANGED**

**Dup-Risk Grep Targets:**

1. **Callers of `parse_return()`:**
   ```bash
   grep -n "parse_return()" crates/perl-parser-core/src/engine/parser/statements.rs
   ```
   Result: Line 423 (inside statement dispatch) — expected, no other callers

2. **Callers of `parse_return_expr()`:**
   ```bash
   grep -n "parse_return_expr()" crates/perl-parser-core/src/engine/parser/
   ```
   Result: primary.rs line 1272 (inside expression primary), precedence.rs line 130 (inside assignment level) — expected, both in expression path

3. **NodeKind::Return pattern matches:**
   ```bash
   grep -rn "NodeKind::Return" crates/ --include="*.rs" | grep -v test | wc -l
   ```
   Count: ~15 matches (IDE providers, semantic analysis, code generation) — all treat Return as an expression node, no change needed

**No ID-space or resource changes.**

---

## §Test-Grid

| Test Layer | Test Scenario | Test Name | Invariant | Status |
|---|---|---|---|---|
| **Unit: Statement-level return** | Return + `or` operator | `test_return_with_word_or_at_statement_level` | `(binary_or (return $x) (call die))` | Red (Step 2) |
| | Return + `and` operator | `test_return_with_word_and_at_statement_level` | `(binary_and (return $x) (call die))` | Red (Step 2) |
| | Return without value + `or` | `test_return_without_value_with_word_or` | `(binary_or (return) (call die))` | Red (Step 2) |
| | Return value boundary | `test_return_value_does_not_consume_or_separator` | Two return nodes, OR between | Red (Step 2) |
| **Unit: Statement-level return modifiers** | Return + statement modifier | `test_return_with_statement_modifier` | `(statement_modifier_if ... (return $x) ...)` | Existing, passes |
| **Unit: Expression-level return (regression)** | Assignment + return | Existing: `test_return_in_assignment` | `(assignment (return $x))` | Existing, must pass |
| | Ternary + return | Existing: `test_return_in_ternary_*` (5 tests) | Return as branch operand | Existing, must pass |
| | Short-circuit + return | Existing: `test_short_circuit_*_return` (4 tests) | Return as binary operand | Existing, must pass |
| **Unit: Loop control (no regression)** | Next/last/redo at statement level | Existing: `test_next_last_redo_simple` | `(loop_control ...)` | Existing, must pass |
| | Loop control + word operators | Existing: statement-level handling | `(binary_or (loop_control) ...)` | Existing, already correct |
| **Integration: Parser corpus** | CPAN module corpus | Existing: `cpan_pattern_tests.rs` | No parse errors on valid code | Existing, must pass |
| | Real-world precedence case | Manual: `return $x or die` in a subroutine | Deparser/semantics correct | Manual verification |

**Adversarial Test Cases:**

| Input | Adversary Challenge | Expected Parse | Test Name |
|---|---|---|---|
| `return (1 or 2);` | Explicit parentheses override | Return value is `(or 1 2)` | `test_return_explicit_parens_override` |
| `return 1 or 2, 3;` | Comma higher precedence than or | Return takes `(1 or 2, 3)` or just `1`? | Pending: verify comma precedence |
| `return undef or die;` | Idiomatic error check | `(binary_or (return undef) (call die))` | `test_return_undef_or_die` |

---

## §Blast-Radius

**Consumers of Return/LoopControl nodes:**

1. **IDE providers** (`crates/perl-lsp-*/src/`):
   - Semantic tokens (`semantic_tokens.rs`) — uses NodeKind classification, unchanged
   - Hover provider (`hover.rs`) — walks Return nodes, unchanged
   - Definition provider (`goto_definition.rs`) — classifies Return as expression, unchanged
   - **Impact:** NONE (no logic change, AST structure identical)

2. **Semantic analysis** (`crates/perl-semantic-analyzer/src/`):
   - `analysis/index.rs` — indexes subroutines; Return is leaf node
   - `analysis/scope.rs` — scope entry/exit; Return marks function exit
   - **Impact:** NONE (return value parsing doesn't affect scope semantics)

3. **DAP** (`crates/perl-dap/src/`):
   - Stack frame walker — Return marks function exit
   - Variable evaluation — Return not normally evaluated at debugger level
   - **Impact:** NONE (stack semantics unchanged)

4. **Parser itself** (`crates/perl-parser-core/src/`):
   - `statements.rs` — parse_word_or_expr applies after Return (line 424) — ALREADY CORRECT
   - `expressions/precedence.rs` — parse_assignment handles word operators — UNCHANGED
   - `expressions/primary.rs` — parse_return_expr called from assignment level — UNCHANGED
   - **Impact:** NONE (all call sites already compatible)

5. **Test suite** (`crates/perl-parser-core/tests/`):
   - Existing `control_flow_expr_tests.rs` — all expression-level return tests — must pass
   - Existing `loop_control_tests.rs` — loop control handling — must pass
   - New test file `control_flow_return_precedence_1394.rs` — new statement-level tests

**Boundary protection:**

- Parser is a leaf crate; changes don't propagate upstream
- NodeKind unchanged; binary compatibility preserved
- No public method signature changes
- Downstream code (semantic analyzer, LSP) uses existing AST introspection APIs

**Known cross-boundary dependencies:** NONE that will regress.

---

## §Observations

**Scope Notes:**

1. **Out of Scope:** Loop control keywords (next/last/redo) already work correctly at statement level because they only parse at expression level via `parse_loop_control()` in the primary parser, which is then wrapped by `parse_word_or_expr()` at statement level (line 325-326 in statements.rs). This is already correct and requires no changes.

2. **Incomplete Audit:** The issue mentions ternary operators and low-precedence operators. Ternary is already correct (tested). Comma operator has higher precedence than return (correct). The main gap was statement-level return consuming word operators into the return value.

3. **Perl Compatibility:** Perl 5 itself warns with `Possible precedence issue with control flow operator (return)` when it sees `return $x or die` because it parses it as `(return $x) or die` but the behavior is surprising enough to warn. This fix aligns our parser with Perl's own interpretation.

4. **Future Enhancement:** The issue mentions auditing precedence against the official Perl op-precedence table. This fix is one piece of that audit. A future follow-up could audit other statement-level constructs (die, warn, etc.) for the same issue.

---

**Version**: 1
**Subsystem**: Parser (perl-parser-core)
**Type**: Precedence Bug Fix
**Risk Level**: LOW (single function, test-covered, no API changes)
