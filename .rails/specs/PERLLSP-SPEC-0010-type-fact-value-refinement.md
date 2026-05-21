# PERLLSP-SPEC-0010 — Type-fact value refinement

## Problem
`$services{$name}` remains dynamic when `$name` is statically known.

## Proposal
Add literal value facts to `TypeFact`:
- `StringLiteral(String)`
- `IntegerLiteral(i64)`
- `BooleanLiteral(bool)`
- `StringSet(BTreeSet<String>)`

## Exactness rule
Exact receiver completion allowed when the key resolves to a static literal that maps to a single source-backed, fresh, high-confidence receiver package.

## Fallback rule
Fallback/blocked for unknown, runtime-computed, ambiguous, or stale keys.

## Track boundary
No parser fairness/gap closure work; Track C semantics only.
