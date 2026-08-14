# Typeglob-alias corpus slice

## Issue

[#6505](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6505)

## Scope

Measure the existing `typeglob_alias` fixture through manifest-backed parser AST expectations and the public parser E2E selector.

## Claim boundary

This covers the package, original subroutine, typeglob assignment, explicit coderef, and alias call represented in one fixture. It preserves the fixture's dynamic-boundary evidence and does not establish runtime alias resolution, glob-slot semantics across packages, or dynamic dispatch correctness.
