# Implementation Checklist: #10740 — scope-aware class grammar context

## Change order

Parser-state-only change inside `crates/perl-parser-core`, plus this spec
bundle. No parser output, AST, NodeKind, diagnostic, or public API change.

### Step 1 — Characterize current behavior before touching production code

Parse each boundary case on pristine `origin/main` and record whether ADJUST is
admitted. Establishes that no leak is observable today, so the change is a
preserving refactor and the tests are parity tests rather than a bug fix.
Result table: `context.md`.

- [x] Top level, after class close, between classes, after nested classes,
      after a class in a sub, after a recovered malformed body
- [x] Inside class body, after an inner class closes, inside nested block /
      method / sub / anonymous sub
- [x] `class Foo;` statement form

### Step 2 — Add the falsifiers first

**File:** `crates/perl-parser-core/tests/class_grammar_context.rs` (new file,
per the package `CLAUDE.md` one-test-file-per-change rule).

Assert on the emitted AST, not on parser internals, so the tests are valid
against both the counter and its replacement.

- [x] Five negative controls for context leaking past a class body
- [x] Positive control for enclosing-frame restoration after an inner class
- [x] Statement-form-not-admitted boundary controls
- [x] `field` / `method` ungated identity controls
- [x] Fresh vs incremental equivalence

### Step 3 — Introduce the context type

**File:** `crates/perl-parser-core/src/engine/parser/class_grammar.rs` (new).

- [x] `ClassGrammarForm { Block, Statement }`
- [x] `ClassGrammarMark(usize)` — an observed depth
- [x] `ClassGrammarContext { frames: Vec<ClassGrammarForm> }` with
      `admits_class_members`, `current_form`, `mark`, `enter`, `restore`
- [x] `restore` truncates to the mark: cannot underflow, cannot leak a frame,
      restores the enclosing frame rather than clearing
- [x] Module docs state the grammar-admission-not-semantics boundary
- [x] 7 unit tests for the transition semantics

`Statement` and `current_form` carry a narrow
`#[allow(dead_code, reason = "...")]` naming #10864, matching the three
existing `allow(dead_code)` sites in this crate. They are the seam the issue
requires be explicit so #10864 does not redesign parser state.

### Step 4 — Add the structured guard

**File:** `crates/perl-parser-core/src/engine/parser/helpers.rs`

- [x] `within_class_grammar(form, f)` placed beside `with_depth`, mirroring
      its closure form and its rationale (no `Drop` guard aliasing
      `&mut Parser`)

### Step 5 — Migrate the field and its single writer/reader

- [x] `mod.rs`: `in_class_body: usize` → `class_grammar: ClassGrammarContext`
- [x] `mod.rs`: constructor initializes `ClassGrammarContext::default()`
- [x] `declarations.rs`: paired `+= 1` / `-= 1` → one
      `within_class_grammar(ClassGrammarForm::Block, Self::parse_block)?`
- [x] `statements.rs`: `self.in_class_body > 0` →
      `self.class_grammar.admits_class_members()`
- [x] `grep in_class_body` returns zero occurrences

### Step 6 — Prove the tests discriminate

- [x] Mutant A (`restore` no-op): 6 of 14 fail
- [x] Mutant B (always admit): 7 of 14 fail
- [x] Mutant C (`restore` clears all frames): 1 of 14 fails, and it is the
      test written for exactly that property
- [x] Source restored after each mutant

### Step 7 — Repair the oracle after independent review

Independent adversarial review of the candidate found that
`admitted_adjust_blocks` matched on the node name alone, and that
`method ADJUST { }` is a legal ordinary method declaration reaching
`parse_method` — emitting the same `Method { name: "ADJUST" }` shape without
ever consulting the class grammar context. The oracle could not tell the two
apart, and its doc comment claimed it could.

- [x] Oracle now also requires `name_span.is_none()`
      (`parse_adjust_block` emits `None`; `parse_method` always records the
      name token's span)
- [x] Doc comment corrected to state the real distinction
- [x] `an_ordinary_method_named_adjust_is_not_an_admitted_block` added, holding
      the distinction in both directions (ordinary method not counted inside or
      outside a class; a genuine block still counted)
- [x] Mutants A/B/C re-run against the stricter oracle: 6 / 7 / 1 failures,
      unchanged discrimination

### Step 8 — Verify

- [x] `cargo fmt -p perl-parser-core -- --check`
- [x] `cargo clippy -p perl-parser-core --lib --locked -- -D warnings`
- [x] `cargo clippy -p perl-parser-core --test class_grammar_context --locked -- -D warnings`
- [x] `cargo test -p perl-parser-core --all-targets --locked` — 368 targets
      ok, 0 failed
- [x] `cargo check --workspace --all-targets --locked`

## Pre-existing failure, not candidate-owned

`cargo clippy -p perl-parser-core --all-targets -- -D warnings` fails to
compile the `hir_data_section_shell` test target with 12 `expect_used` /
`panic` errors. Reproduced identically on a detached worktree of pristine
`origin/main` at 6727a00 with no candidate changes, and that file is not in
this diff. Main-owned; not repaired here and not absorbed into this claim.

## Writer key

`parser/class-grammar-context`. No concurrent declaration-parser,
class-context, attribute-parser, or statement-list-context writer.

## Stop conditions honored

No new class/field/ADJUST node emission, no statement-class admission, no
sibling reparenting, no semantic class state, no VERSION/attribute
centralization, and no broadening into unrelated parser context.
