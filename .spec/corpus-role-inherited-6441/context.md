# Role and inherited-method corpus slices

## Issue

[#6441](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6441)

## Scope

Promote the existing `role_method` and `inherited_method` fixtures from symbol-oriented evidence to executable parser AST evidence.

## Claim boundary

The slices measure package and subroutine declarations, returns, and the represented local or qualified calls. They do not establish role composition, inheritance resolution, method lookup, or runtime dispatch semantics.

This branch is based on the current `main` tree and carries both adjacent slices so the public selector and manifest remain coherent.
