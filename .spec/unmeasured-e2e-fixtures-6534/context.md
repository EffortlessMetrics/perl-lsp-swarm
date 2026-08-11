# Unmeasured E2E fixtures slice

## Issue

[#6534](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6534)

## Scope

Give manifest-backed AST expectations to the four fixtures already selected by
`E2E_FIXTURES` but carrying an empty `ast_expectations` list: `heredoc_basic`,
`regex_match`, `quote_like`, and `post_error_package_sub_recovery`.

Expectations are derived from observed parser output, not from an idealized
shape, so the fixture record stays honest about what the parser produces today.

## Claim boundary

This closes the gap between "listed in the E2E selector" and "measured by the
E2E selector" for these four fixtures. It covers the heredoc body binding,
the bound match against a regex literal, the `q{}`/`qq{}` quote-like pair, and
the package/subroutine region the parser recovers after a deliberate syntax
error.

It does not change parser behavior, does not add fixture sources, and does not
establish heredoc interpolation semantics, regex engine behavior, quote-like
delimiter exhaustiveness, or a general error-recovery guarantee beyond the one
recovery region this fixture contains.
