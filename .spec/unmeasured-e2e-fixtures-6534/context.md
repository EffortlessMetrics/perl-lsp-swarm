# Unmeasured E2E fixtures slice

## Issue

[#6534](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6534)

## Scope

Give manifest-backed AST expectations to the four fixtures selected by
`E2E_FIXTURES` that still carry an empty `ast_expectations` list on current
main: `heredoc_basic`, `regex_match`, `quote_like`, and
`post_error_package_sub_recovery`.

Expectations are derived from observed parser output, not from an idealized
shape, so the fixture record stays honest about what the parser produces today.
With `parent_kind` and `depth` enforced as of #6541, each expectation also
constrains tree topology rather than kind and line alone.

## Claim boundary

This closes the gap between "listed in the E2E selector" and "measured by the
E2E selector" for these four fixtures — the last four such entries on main. It
covers the heredoc body binding and the subroutine after the terminator, the
bound match against a regex literal, the `q{}`/`qq{}` quote-like pair, and the
package and subroutine the parser recovers after a deliberate syntax error.

It does not change parser behavior, does not add fixture sources, and does not
establish heredoc interpolation semantics, regex engine behavior, quote-like
delimiter exhaustiveness, or a general error-recovery guarantee beyond the one
recovery region this fixture contains.
