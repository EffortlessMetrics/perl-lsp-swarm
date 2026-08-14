# Incremental cold-parse AST oracle

## Issue

[#1382](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1382)

## Scope

Strengthen the existing ASCII-safe incremental edit property with a fresh-parser AST oracle after every edit.

## Claim boundary

This checks incremental AST S-expression equality for the existing generated edit domain. UTF-8 boundary, heredoc, and regex-specific edit domains remain separate coverage.
