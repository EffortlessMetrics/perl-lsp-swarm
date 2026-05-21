# PERLLSP-SPEC-0006: Parser boundedness budgets

## Contract

Every parser timeout/hang risk must have a budget.

## Policy file

```text
policy/parser-boundedness-budgets.toml
```

## Shape

```toml
schema_version = "1.0"
owner = "parser-edge-gap-closure"
status = "advisory"

[defaults]
max_fixture_bytes = 1048576
iterations = 10
build_profile = "release"
```

## Required behavior

For P0/P1 timeout/hang risks, tests must prove at least:

```text
no crash
no unbounded loop
bounded parse result
bounded error or partial AST is acceptable where correctness is impossible
```

## Required receipt

```json
{
  "schema_version": 1,
  "scenario": "deep_nesting_stack_overflow",
  "fixture": "...",
  "parser": "v3-native",
  "bytes": 1234,
  "duration_ms": 42,
  "verdict": "bounded-error",
  "crashed": false,
  "hung": false,
  "budget_verdict": "pass"
}
```
