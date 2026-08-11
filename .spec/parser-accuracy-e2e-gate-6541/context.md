# parser-accuracy E2E gate repair

## Issue

[#6541](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6541)

## Scope

Repair the three AST expectations that make `parser_accuracy_e2e` fail on
current main, and run that test in the routed Rust lane so the parser-accuracy
measurement surface is actually enforced.

Two expectations in `qualified_refs` stored a literal backslash-n where a
newline was intended. One expectation in `typeglob_alias` described the `Unary`
reference node (`\&original`) while claiming the `AmperCall` child
(`&original`). All three were corrected against observed parser output rather
than by adjusting the parser.

## Claim boundary

This restores a green `parser_accuracy_e2e` and closes the gap that let two
consecutive slice PRs merge broken expectations. It does not change parser
behavior, does not add fixture coverage, and does not repair the
`operator`/`parent_operator` values in `typeglob_alias` that disagree with
observed output — those feed the metrics scorer rather than this test and are
tracked separately.
