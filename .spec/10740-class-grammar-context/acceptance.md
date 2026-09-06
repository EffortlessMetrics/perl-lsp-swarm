# Acceptance: #10740 — scope-aware class grammar context

## Claim

The parser's class-body admission state is one scope-aware, structurally
restored grammar context instead of a scalar counter, with block-form parser
output, tokens, diagnostics, and recovery byte-identical to `origin/main`, and
with statement-form representation available to #10864 but not admitted.

## Acceptance criteria

| # | Criterion | Evidence |
|---|---|---|
| A1 | One parser-owned scope-aware grammar context replaces the scalar convention | `class_grammar.rs`; `in_class_body` has zero remaining occurrences |
| A2 | Block-form AST / token / diagnostic / recovery behavior is source-equivalent | 368 green `perl-parser-core` test targets, including the pre-existing class, field, method, ADJUST, and incremental suites |
| A3 | Restoration is structured across normal, nested, recovery, early-return, and EOF paths | `within_class_grammar` guard; leak falsifiers below |
| A4 | Near-neighbour syntax outside class grammar stays ordinary | `adjust_outside_any_class_stays_ordinary_syntax` and four sibling negative controls |
| A5 | Statement-mode seam is explicit and unit-tested without production admission | `ClassGrammarForm::Statement`; `nested_frames_restore_the_exact_enclosing_form`; `statement_form_classes_are_still_not_admitted` |
| A6 | Successors can consume the state without redesign | `enter` / `mark` / `restore` / `current_form` are form-agnostic — proven at unit level only; see the residual note below |
| A7 | No NodeKind, source payload, VERSION/attribute, semantic, provider, or support change | diff touches parser state only; no `perl-ast` change |

## Discriminating proof

Proof lives in two layers.

**Mechanism unit tests** — `src/engine/parser/class_grammar.rs`, 7 tests.
Cover fresh state, entry, mark/restore, exact enclosing-form restoration,
discarding frames an inner production left behind, and that restoring a stale
mark cannot underflow.

**Behavioral acceptance** — `tests/class_grammar_context.rs`, 15 tests.
Asserts on the emitted AST rather than the mechanism, so the tests survive the
replacement and would equally have run against the counter.

Positive controls: ADJUST admitted inside a class body; two sibling classes
each admit their own members; enclosing class frame restored after an inner
class closes; nested block and method bodies do not clear the enclosing
context.

Negative controls (the falsifiers the scalar convention never had): ADJUST
outside any class, after a class body closes, between two classes, after
nested class bodies close, after a class nested in a sub, and after a
recovered malformed class body — all must stay ordinary syntax.

Boundary controls: `class Foo;` is still a parse error, emits no class node,
and opens no frame for the statements that follow.

Ungated controls: `field` and `method` keep their identity outside class
grammar.

Equivalence control: an edit inside a class body reparses to exactly the fresh
parse — same s-expression and same diagnostics.

Oracle control: `an_ordinary_method_named_adjust_is_not_an_admitted_block`.
`ADJUST` is an ordinary identifier to the lexer, so `method ADJUST { }` is a
legal method declaration that reaches `parse_method` without consulting the
class grammar context — and emits the same `Method { name: "ADJUST" }` node
kind an admitted block emits. The oracle therefore also requires
`name_span.is_none()`, which `parse_adjust_block` sets and `parse_method`
never does. Without that check every negative control in the file would be
satisfiable by a construct unrelated to class grammar. Found by independent
adversarial review of this candidate, not by construction.

## Mutation evidence

The acceptance tests were run against three deliberately broken
implementations to confirm they discriminate rather than merely pass:

| Mutant | Injected defect | Result |
|---|---|---|
| A | `restore()` is a no-op — frames leak | 6 of 15 fail |
| B | `admits_class_members()` always true | 7 of 15 fail |
| C | `restore()` clears all frames instead of restoring the observed depth | 1 of 15 fails — `leaving_an_inner_class_body_restores_the_enclosing_class_context` |

Mutant C is the reason that test exists: it is the only case that separates
"clear the context on exit" from "restore the exact enclosing frame", and it
is the property a depth counter provided implicitly.

## Verification run

```bash
cargo fmt -p perl-parser-core -- --check          # clean
cargo clippy -p perl-parser-core --lib --locked -- -D warnings        # clean
cargo clippy -p perl-parser-core --test class_grammar_context --locked -- -D warnings   # clean
cargo test -p perl-parser-core --all-targets --locked  # 368 targets ok, 0 failed
cargo check --workspace --all-targets --locked
```

## Residual: form discrimination is not yet parser-reachable

Raised by independent review of this candidate and recorded rather than
papered over.

`within_class_grammar` has exactly one production call site, and it always
passes `ClassGrammarForm::Block`. `ClassGrammarForm::Statement` is therefore
constructed only by this module's own unit tests. A consequence: an
implementation that discarded the `form` argument entirely and kept a plain
depth counter — `current_form()` always answering `Block` — would pass all 22
tests in this candidate, because nothing reachable through `Parser` exercises
form discrimination yet.

That does not affect any correctness claim here; every behavioral claim in
this PR is about admission and restoration, which the suite does discriminate
(see the mutation table above). It does bound A6: "successors can consume the
state without redesign" rests on the unit tests of `ClassGrammarContext` in
isolation, not on anything the parser reaches. The first parser-reachable
proof of form discrimination arrives with #10864, which is the issue that
activates statement form; whoever takes it should expect to add that coverage
rather than assume this suite already carries it.

Manufacturing a production path for `Statement` here to close the gap would
mean admitting `class Foo;`, which is exactly this issue's stated non-goal.

## Known limitation carried forward, not introduced

While inside a class body the parser admits `ADJUST` within nested ordinary
blocks, method bodies, sub bodies, and anonymous subs — for example
`class Foo { method m { ADJUST { } } }`. That is current `origin/main`
behavior and #10740 preserves it exactly. Narrowing admission to the class
body's own statement list changes parser output and is a semantic-scope
decision owned by #10346 / #6672, outside this issue's non-goals. The
behavior is pinned by `a_nested_ordinary_block_does_not_clear_the_enclosing_class_context`
so a later narrowing is a deliberate, visible change.

## Out of scope

Statement-form admission (#10864), shared VERSION/attribute primitives
(#11089 / #11093), canonical class/field/ADJUST output cutovers
(#10838 / #10846 / #10854), legacy retirement (#10882 / #10884 / #10890), and
all semantic class ownership.
