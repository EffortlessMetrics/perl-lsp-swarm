# PERLLSP-PROP-0001: Parser differential and Tree-sitter fairness

## Problem
Historical results from our vendored Tree-sitter snapshot are valuable, but they are not sufficient to make claims about current upstream parser behavior.

## Proposal
Establish parser differential as a first-class lane with explicit target labels and proof receipts. Initial target set:

- `ts-vendored-c`
- `ts-upstream-c`
- `pest-legacy`
- `v3-native`

## Proof requirements
Any claim about upstream Tree-sitter parser quality must be backed by current harness receipts from the same fixture set and same run configuration as other targets.

## Out of scope
This proposal does not replace the production parser. It defines fairness and evidence rules for comparison.
