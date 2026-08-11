# Unmeasured E2E fixtures and negative shape assertions

## Issues

- [#6534](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6534) — the four E2E fixtures asserting nothing
- [#6591](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6591) — expectations cannot reject an extra wrong node

## Scope

Two halves of one claim: make every `E2E_FIXTURES` entry actually measure
something, and give the manifest a way to say what must *not* be there.

`heredoc_basic`, `regex_match`, `quote_like`, and
`post_error_package_sub_recovery` are listed in `E2E_FIXTURES` but carry empty
`ast_expectations`, so the per-expectation loop never runs for them. They are
the last four such entries on main. Each gains five expectations derived from
observed parser output, with `parent_kind` and `depth` populated so the
topology enforcement from #6541 applies.

`forbidden_nodes` is added to the fixture schema because positive expectations
match with `.any(...)` and therefore cannot reject an *extra* wrong node. A
disambiguation fixture makes two claims — "a String is here" and "the braces
did not open a block" — and only the first was expressible.

## Why `line` is required on a forbidden entry

An earlier attempt at this asserted that `quote_like` must contain no `Block`
node at all. That fires on correct output: `quote_like.pl` contains
`sub quote { ... }`, whose body is a legitimate `Block`.

Every kind worth forbidding in a disambiguation fixture — `Block`,
`ExpressionStatement` — also occurs legitimately elsewhere in the same file.
"This kind must not appear anywhere" is essentially never the claim; "this kind
must not appear *here*" always is. So `line` is mandatory on `ForbiddenNode`,
the opposite of the optional refinements on `AstExpectation`, where an absent
field means unconstrained. `parent_kind` and `depth` remain optional refinements
on top.

## Claim boundary

Provably true: every `E2E_FIXTURES` entry contributes at least one assertion
that fails when the corresponding parser output changes, and three
disambiguation claims are now two-sided.

Not established: that these fixtures cover their constructs exhaustively.
`heredoc_basic` measures one single-quoted heredoc, not interpolation or
nesting; `quote_like` measures two brace-delimited forms, not the delimiter
matrix; `regex_match` measures one bound match, not the ambiguity corpus; the
recovery fixture measures one error region, not general recovery.

`heredoc_basic` deliberately gains no forbidden entry. Its claim is "the body
is not parsed as code", and no kind-based entry expresses that honestly —
lines 4-5 contain no nodes of any kind today, so any specific kind would be
trivially absent rather than discriminating. Adding one would manufacture the
appearance of proof.
