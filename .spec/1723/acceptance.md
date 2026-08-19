# Issue #1723: acceptance fixtures

Fixtures live in `crates/perl-semantic-analyzer/src/analysis/callable_exit.rs`
(`mod tests`). Fixture IDs are the test names.

## Exact positives (admitted complete profile)

- `straight_line_implicit_value_is_complete` — final straight-line expression
  is one `ImplicitValue` exit, `Complete`, no boundaries.
- `top_level_return_makes_later_statements_unreachable` — one `ExplicitValue`
  exit, `unreachable_tail_count == 1`, `Complete`.
- `empty_body_has_complete_implicit_void_exit` — one `ImplicitVoid` exit,
  `Complete`.

## Boundaries (partial coverage)

- `conditional_returns_are_retained_but_partial` — `if` returns retained as
  exits, `ConditionalControl` boundary, `Partial`.
- `ternary_return_is_partial_conditional_control` — `$c ? return 1 : 2`
  records `ConditionalControl`, `Partial`; never `Complete`.
- `given_when_returns_are_partial_conditional_control` — `given`/`when`/
  `default` returns retained (two `ExplicitValue`), `ConditionalControl`,
  `Partial`.
- `traversal_budget_widens_instead_of_truncating_to_complete` — budget 1/1
  records `TraversalBudget`, appends `ImplicitUnknown`, `Partial`.

## Nested/unreachable rules

- `nested_callable_returns_do_not_leak` — inner `sub` returns never enter the
  outer summary; `nested_callable_count == 1`.

## Mutation expectations

A mutation fails CI if it:

1. treats a nested callable's returns as the outer callable's exits;
2. counts statements after a top-level unconditional return as reachable;
3. calls structured control (including ternary or given/when) complete
   without canonical CFG proof;
4. maps bare return to value/void/undef policy in this module;
5. infers or unifies return types in this module;
6. publishes `CallableResultFact` directly;
7. drops budget/recovery/unsupported boundaries and reports complete;
8. lets traversal/input order change summary identity;
9. joins callables by name/range alone;
10. changes any provider output.

## Verification commands

```bash
cargo fmt --all -- --check
cargo clippy -p perl-semantic-analyzer --all-targets --locked -- -D warnings
cargo test -p perl-semantic-analyzer --all-targets --locked callable_exit
cargo test -p perl-semantic-analyzer --all-targets --locked
git diff --check
```
