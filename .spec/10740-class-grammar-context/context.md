# Context: #10740 — scope-aware class grammar context

Parser controller: #10341. Parent C0 train slot: #10687 — C0-06A.
Production successors: #10838 / #10846 / #10854 / #10864.
Shared parser successors: #11089 / #11093.
Semantic class-scope owners: #10346 / #6672.

## Problem

Perl's native `class` feature makes a small number of constructs admissible
only while the parser is inside class grammar. On the current tree the parser
carried that fact as a scalar depth counter:

```rust
// crates/perl-parser-core/src/engine/parser/mod.rs
in_class_body: usize,
```

written in exactly one place, as a manually paired increment/decrement around
the class body parse:

```rust
// crates/perl-parser-core/src/engine/parser/declarations.rs
self.in_class_body += 1;
let body = self.parse_block();
self.in_class_body -= 1;
let body = body?;
```

and read in exactly one place:

```rust
// crates/perl-parser-core/src/engine/parser/statements.rs
self.in_class_body > 0
    && self.peek_kind() == Some(TokenKind::Identifier)
    && /* ADJUST */ && /* `{` */
```

Three properties of that convention are the problem this issue owns:

1. **Correctness is a caller obligation, not a structural guarantee.** The
   decrement is placed before the `?` deliberately so an `Err` from
   `parse_block` cannot skip it. That is correct today, but it is correct by
   the author's care at one call site rather than by construction, and the
   next writer to add a `?` between the pair reintroduces a leak silently.
2. **A `usize` cannot fail safely.** An unbalanced decrement is an arithmetic
   underflow — a debug panic or a release wraparound to `usize::MAX`, which
   would admit class syntax for the remainder of the file.
3. **The state cannot express what the successors need.** A counter records
   *how deep*, not *which form*. Statement-form classes (#10864) end at the
   next `class`/`package`/statement-list boundary rather than at a matching
   brace, which a depth counter has no way to represent.

## Measured current behavior

Characterized on `origin/main` at 6727a00 before any change, by parsing each
source and reading the emitted AST. `ADJUST` is admitted as a class member
when it is emitted as `NodeKind::Method { name: "ADJUST", .. }`; otherwise it
stays an ordinary identifier expression.

| Source | ADJUST admitted |
|---|---|
| `ADJUST { }` | no — ordinary identifier + error |
| `class Foo { ADJUST { } }` | yes |
| `class Foo { }` then `ADJUST { }` | no |
| `class A { }` `ADJUST { }` `class B { }` | no |
| `class Foo { class Bar { } }` then `ADJUST { }` | no |
| `sub outer { class Foo { } }` then `ADJUST { }` | no |
| `class Foo { ] }` (recovered) then `ADJUST { }` | no |
| `class Foo { class Bar { } ADJUST { } }` | yes |
| `class Foo { if (1) { ADJUST { } } }` | yes |
| `class Foo { method m { ADJUST { } } }` | yes |
| `class Foo;` | parse error; no class node; no frame opened |

**No context leak is observable on current main.** This change is therefore an
architectural replacement that preserves behavior exactly, not a bug fix, and
it is described that way in the candidate.

## Scope ruling

- This issue owns **parser grammar admission state** and its restoration.
- It does **not** own class semantics: no class name, entity, scope,
  inheritance, MRO, role, field storage, generation, or object identity may
  enter parser state. Semantic class lifetime stays with #10346 / #6672.
- It does **not** own the shared header primitives (#11089 VERSION, #11093
  declaration attributes) or any parser output cutover (#10838 / #10846 /
  #10854).
- It does **not** admit statement-form classes; #10864 does.

## Two facts the issue text overstates

Recorded here because they change what a reviewer should expect from the diff:

1. The issue asks to prove that "`field`, `method`, and ADJUST remain admitted
   inside current class block exactly as before." Only **ADJUST** is gated on
   class context. `field` is admitted by sigil lookahead
   (`is_field_declaration_context`, `helpers.rs`) and `method` by an
   identifier lookahead (`statements.rs`), both anywhere in the file and
   neither consulting `in_class_body`. The context governs ADJUST admission
   alone. The candidate pins the ungated identity of `field`/`method` so a
   later change cannot quietly route them through the context.
2. The issue asks for "fresh/checkpoint parsing ends with the same AST and
   terminal state." There is no parser-level checkpoint that saves or restores
   parser context fields; the lexer-level `Checkpointable` machinery only
   reclassifies lookahead tokens. The incremental path
   (`incremental.rs:241`) hands the *whole* assembled token list to a fresh
   `Parser::from_tokens`, so parsing always starts outside class grammar at
   the file start and no mid-file resume exists to diverge. The candidate
   proves the fresh/incremental equivalence that is actually reachable.

## Chosen mechanism

A stack of typed frames with mark/restore, plus a closure guard that mirrors
the crate's existing `with_depth` idiom, so restoration cannot be skipped:

```rust
fn within_class_grammar<T>(
    &mut self,
    form: ClassGrammarForm,
    f: impl FnOnce(&mut Self) -> ParseResult<T>,
) -> ParseResult<T> {
    let restore = self.class_grammar.mark();
    self.class_grammar.enter(form);
    let result = f(self);
    self.class_grammar.restore(restore);
    result
}
```

`restore` truncates to the observed depth. That is strictly stronger than a
balanced decrement: it discards frames an inner production entered and failed
to leave, it is inert rather than panicking when the context is already at or
below the mark, and it restores the *enclosing* frame rather than clearing the
context — which is what keeps `class Foo { class Bar { } ADJUST { } }`
working.

`ClassGrammarForm::Statement` is represented but not entered by production
parsing, so that #10864 activates a transition on this state machine instead
of redesigning parser state.

## Non-authorities

Nothing here is a support claim, a release action, a compatibility retirement,
a NodeKind change, or a semantic promotion. Parser output is byte-identical.
