# PERLLSP-SPEC-0001: Parser target registry

## Scope
Define the parser-target registry contract for differential runs.

## Required target labels
- `ts-vendored-c`
- `ts-upstream-c`
- `pest-legacy`
- `v3-native`

## Registry requirements
- Each target must have a stable label and parser implementation binding.
- Differential outputs must record enabled targets per run.
- Default runs may exclude optional targets, but `all` must enumerate every available target.

## Fairness rule
Comparisons across targets are only valid when:
- input corpus is identical,
- timeout and resource limits are identical,
- output schema version is identical.

## Upstream proof rule
No project doc may assert that the current upstream Tree-sitter parser passes or fails a gap without a receipt generated from the current registered upstream target.
