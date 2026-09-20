# Issue #1723: bounded callable exit contributors

## Problem statement

A later callable-result producer needs to know which source expressions may
contribute to a callable's result and whether the current syntax pass has a
complete exit denominator. Collecting every `return` node and immediately
unifying inferred types is unsafe as canonical evidence when nested callables,
unreachable tails, conditionals, loops, exceptions, `goto`, parser recovery,
or incomplete traversal are involved.

## Outcome

`CallableExitSummary` in
`crates/perl-semantic-analyzer/src/analysis/callable_exit.rs` publishes a
bounded, syntax-level inventory of one callable's explicit returns and its
implicit fallthrough exit, with typed boundaries for everything the pass
cannot prove. This module does not infer or unify result types, does not
build a CFG, and does not produce `CallableResultFact`.

## Admitted complete syntax profile

`CallableExitCompleteness::Complete` is admitted only when no boundary was
recorded:

- straight-line callable bodies;
- a direct top-level unconditional `return` (later statements counted as
  `unreachable_tail_count`);
- a final straight-line expression, assignment, or initialized declaration
  as `ImplicitValue`;
- an empty body as `ImplicitVoid`.

Nested `sub`/`method` declarations are excluded from the parent's exits and
counted in `nested_callable_count`.

## Boundary table

Boundaries are keyed by `NodeKind::kind_name()`:

| Boundary | Node kinds |
| --- | --- |
| `ConditionalControl` | `If`, `Unless`, `Ternary`, `Given`, `When`, `Default`, `StatementModifier` |
| `LoopControl` | `While`, `Until`, `For`, `Foreach`, `CStyleFor`, `Continue` |
| `ExceptionControl` | `Try`, `Catch`, `Finally`, `Eval` |
| `DynamicControl` | `Goto` |
| `RecoveredSyntax` | `Error` |
| `UnsupportedFallthrough` | non-block body, or a final statement shape without an exposed value range |
| `TraversalBudget` | node/depth budget exceeded |

Any recorded boundary yields `CallableExitCompleteness::Partial`; the exit
inventory is retained, not dropped.

## Source identity

Every exit carries `statement_range`, an optional `value_range`, and the
`control_depth` beneath the callable body where it was observed. Exits are
sorted by `(statement_range.start, statement_range.end, kind, control_depth)`
and deduplicated, so summary identity is independent of traversal and input
order.

## Work budgets

`CallableExitBudget::default()` = `max_nodes: 8_192`, `max_depth: 256`.
Budget exhaustion records `TraversalBudget` and appends an `ImplicitUnknown`
exit; the summary never truncates to `Complete`.

## Stop rules

When exact completeness requires canonical control-flow/dominance proof
beyond this issue, stop and update the owning CFG issue rather than adding
another local control graph. No return-type inference, no callable-result
fact production, no general CFG/fixpoint solver, no call graph, no callsite
binding/materialization, no provider output.
