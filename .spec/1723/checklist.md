# Issue #1723: implementation checklist

- [x] `CallableDeclarationKind` (subroutine, method).
- [x] `CallableExitKind` — explicit value, explicit bare, implicit value,
      implicit void, implicit unknown.
- [x] `CallableExitBoundary` — conditional, loop, exception, dynamic,
      recovered, unsupported-fallthrough, traversal-budget.
- [x] `CallableExitCompleteness::{Complete, Partial}`; any boundary forces
      `Partial`.
- [x] Deterministic node/depth budgets (`max_nodes: 8_192`, `max_depth: 256`)
      with budget-widening instead of truncation.
- [x] `CallableExitSummary` with callable/body ranges, ordered exits,
      nested-callable exclusion, unreachable-tail count, boundaries,
      completeness, and work count (`visited_nodes`).
- [x] Boundary table covers the live `NodeKind::kind_name()` vocabulary:
      `If`/`Unless`/`Ternary`/`Given`/`When`/`Default`/`StatementModifier` →
      conditional; `While`/`Until`/`For`/`Foreach`/`CStyleFor`/`Continue` →
      loop; `Try`/`Catch`/`Finally`/`Eval` → exception; `Goto` → dynamic;
      `Error` → recovered.
- [x] Fixtures: straight-line implicit value, top-level return with
      unreachable tail, nested-callable exclusion, conditional partial,
      ternary partial, given/when partial, empty-body implicit void,
      budget widening (see `.spec/1723/acceptance.md`).
- [x] Deterministic exit ordering and dedup independent of traversal order.

## Deliberately out of scope (stop rules)

- No return-type inference or unification.
- No `CallableResultFact` production (owned by #10309 / #10904 consumers).
- No general CFG/fixpoint solver, call graph, callsite binding, or
  materialization (#10154 and later train rows).
- No provider output changes.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy -p perl-semantic-analyzer --all-targets --locked -- -D warnings
cargo test -p perl-semantic-analyzer --all-targets --locked callable_exit
cargo test -p perl-semantic-analyzer --all-targets --locked
git diff --check
```
