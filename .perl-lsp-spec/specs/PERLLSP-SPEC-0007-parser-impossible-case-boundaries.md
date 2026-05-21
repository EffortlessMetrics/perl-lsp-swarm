# PERLLSP-SPEC-0007: Parser impossible-case boundaries

## Contract

Some Perl constructs cannot be parsed perfectly without executing Perl or external runtime effects. The parser must not silently claim full correctness for those cases.

## Covered classes

```text
source filters
BEGIN-time source/symbol mutation
runtime prototypes
regex code execution blocks (?{ ... }) and (??{ ... })
dynamic Unicode property semantics
indirect object forms requiring symbol knowledge
dynamic hash-vs-block disambiguation requiring semantic/runtime facts
```

## Required statuses

These gaps should usually be one of:

```text
bounded-degradation
accepted-impossible
semantic-deferred
runtime-only
```

## Required fixture style

For impossible cases, fixtures assert:

```text
parser terminates
parser does not panic
parser preserves recoverable surrounding structure
parser marks or contains opaque/dynamic boundary where available
parser docs state non-goal
```
