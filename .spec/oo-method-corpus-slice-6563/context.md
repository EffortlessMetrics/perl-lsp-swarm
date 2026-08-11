# Moo attribute corpus slice

## Issue

[#6563](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6563)

## Scope

Promote the two still-dormant fixtures from the OO/method group —
`generated_accessor` and `heuristic_generated_member` — into measured parser
E2E coverage through manifest-backed AST expectations.

The issue named six fixtures. Four of them gained expectations and E2E
selection directly on `main` while this candidate was in flight:
`medium_method_call`, `role_method`, `inherited_method`, and `method_decl`.
Their prepared expectations were dropped rather than merged over the landed
ones, so this slice neither duplicates nor contests work already on main.

Expectations are derived from observed parser output. `Use` and `HashLiteral`
join the metrics scored-node set because these fixtures are the first to assert
them.

## Claim boundary

This measures the Moo `has` attribute declaration together with its options
list, and — in `generated_accessor` — both the `->new()` constructor call and
the `->name` generated-accessor call site.

It does not establish accessor generation, attribute semantics, method
resolution, or any runtime dispatch behavior — only the AST the parser produces
for these source shapes. The fixtures' symbol-level expectations, including the
`GeneratedMember` entity with `FrameworkSynthesis` provenance, remain
unexercised by this test.

Expectations on `Block`, `ExpressionStatement`, `Variable`, and `Identifier`
are deliberately omitted; asserting them would require adding those kinds to
the metrics scored set, where they are structural noise. Nesting is pinned
through the `parent_kind` and `depth` fields instead.

## Known parser observations not asserted here

Selecting expectations surfaced parser behaviors that look wrong. None is
asserted, so no expectation here launders them into expected behavior. They are
reported in [#6565](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6565):

- `ExpressionStatement` spans are empty for statements whose expression is a
  bare literal (every trailing `1;`), while the child carries the real span.
- `VariableDeclaration` for `our @ISA = qw(...)` spans only `our @ISA`, so its
  `ArrayLiteral` child lies outside the parent span.
- `qw(a b)` emits one `String` per element, but every element's span is the
  whole `qw(...)` literal.
- Under a fat comma, the outer bareword becomes `Identifier` while an inner
  bareword becomes `String`.
