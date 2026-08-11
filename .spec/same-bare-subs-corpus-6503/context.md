# Same-bare-subs corpus slice

## Issue

[#6503](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6503)

## Scope

Measure the existing `same_bare_subs` fixture through manifest-backed parser AST expectations and the public parser E2E selector.

## Claim boundary

This covers three package declarations, three subroutine declarations, two qualified function calls, and return nodes in one multi-package fixture. It does not establish cross-file package resolution, dispatch semantics, or general call-graph correctness.
