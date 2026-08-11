# Role-method corpus slice

## Issue

[#6441](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6441)

## Scope

Promote the existing `role_method` fixture from symbol-oriented evidence to executable parser AST evidence.

## Claim boundary

This measures the two package declarations, two subroutine declarations, both returns, and the local `provided()` call represented in one fixture. It does not establish role composition, method resolution, inheritance, or runtime dispatch semantics.