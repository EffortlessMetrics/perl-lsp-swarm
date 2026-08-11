# Method-call corpus slice

## Issue

[#6489](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6489)

## Scope

Measure the existing `method_call` fixture through manifest-backed parser AST expectations and the public parser E2E selector.

## Claim boundary

This covers one fixture's package, subroutine, local variable, receiver method call, package-qualified method call, and return nodes. It does not establish method resolution, inheritance, or cross-file dispatch correctness.
