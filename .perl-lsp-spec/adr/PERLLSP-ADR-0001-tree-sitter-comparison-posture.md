# PERLLSP-ADR-0001: Native parser and Tree-sitter comparison posture

## Status
Accepted

## Decision
perl-lsp maintains a native parser as the production parser, while Tree-sitter parsers are compared as first-class external targets.

The project will not make claims about a current upstream parser without running the current target through the comparison harness.

## Consequences
- Historical vendored parser results remain useful but are not treated as upstream verdicts.
- Differential reporting must preserve target identity in output receipts.
