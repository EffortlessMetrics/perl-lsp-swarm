# PR 1711-B phase 2 -- unified-traversal coverage-delta characterization

**Controlling issue:** [#1711](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1711)
(LSP-freshness reliability lane). **Related:** #3396, #4013 (1711-A measurement),
#4018 (1711-B shadow -- parity harness + this unified traversal).

This is a **shadow-only** characterization. No production behavior has
changed: `WorkspaceIndex::index_file_with_generation` is byte-for-byte
unchanged and still calls the existing dual walk
(`IndexVisitor::visit` + `extract_symbol_refs`). The unified traversal
described here (`IndexVisitor::visit_unified` /
`FileExtractionBundle::build_unified`, in
`crates/perl-workspace/src/workspace/workspace_index.rs`) is additive,
`#[allow(dead_code)]`-justified, and unused by the live path. This document
is the durable, narrative record of what the
`extraction_bundle_shadow_compare::coverage_delta_*` tests in that file
mechanically enforce -- **those tests are the source of truth**; if this
document and the tests ever disagree, the tests win and this document is
stale.

## What changed structurally

Today, `index_file_with_generation` runs the reference walk TWICE per file,
via two independently hand-written AST walkers:

- **Legacy** (`IndexVisitor::visit_node` / `::visit_children`): its
  "everything else" fallback (`visit_children`'s final `_ => {}` arm) is a
  **hand-maintained allowlist** that silently stops recursion for any
  `NodeKind` not explicitly listed in either `visit_node` or
  `visit_children`.
- **Canonical** (`extract_symbol_refs`, in `perl-symbol`): its fallback
  (`_ => node.for_each_child(...)`) delegates to `perl_ast::Node::for_each_child`,
  a **compiler-exhaustive** dispatcher (no wildcard arm -- every `NodeKind`
  variant must be handled, enforced at compile time).

The unified traversal (`IndexVisitor::visit_unified` / `walk_unified`)
replaces legacy's incomplete fallback with `Node::for_each_child` too. Every
case below was **empirically verified against current `origin/main` before
this traversal was written** (via a temporary probe harness, since replaced
by the permanent tests listed per case): the legacy `FileIndex` had **zero**
references for the construct; canonical's `FileFactShard`/`extract_symbol_refs`
**already** found it (unaffected by this change -- verified byte-for-byte
identical between the unified traversal and production's existing canonical
output, across the full Perl corpus and mojolicious/dancer2/catalyst
real-project sweep; see `assert_unified_canonical_parity` and the
`parity_over_*` tests in the same module).

## The coverage-delta cases

Each case is a minimal, checked-in, mechanically-enforced fixture. Test
function names below are in
`crates/perl-workspace/src/workspace/workspace_index.rs` ::
`extraction_bundle_shadow_compare`.

### 1. Block-form `package Foo { ... }` bodies

```perl
package Foo { sub bar { baz(); } }
```

`IndexVisitor::visit_node`'s `Package` arm only updates `current_package` --
it never recurses into `block` for references (declarations are still seen
correctly, via the separate `extract_symbol_decls` walk in
`project_symbol_declarations`). `baz()`'s call reference inside the block is
invisible to `find_references("baz")` today. Test:
`coverage_delta_package_block_form`.

### 2. Block-form `class Foo { ... }` bodies (Perl 5.38+ `use feature 'class'`)

```perl
use feature 'class';
class Foo { method bar { baz(); } }
```

Same gap as case 1: `NodeKind::Class`'s `body` is never recursed into by
`visit_node` today. Test: `coverage_delta_class_body_form`.

### 3. Typeglob aliasing

```perl
package Foo;
sub original { 1 }
*alias = \&original;
```

`NodeKind::Typeglob` has **no arm at all** in `visit_node`/`visit_children`.
Any file using typeglob aliasing (`*foo`, `*alias = ...`) has those
reference sites completely absent from the legacy `FileIndex` today,
regardless of unification. Test: `coverage_delta_typeglob_alias`.

### 4. `goto &handler` coderef targets

```perl
package Foo;
sub dispatch { goto &handler; }
```

`NodeKind::Goto` has **no arm at all** in `visit_node`/`visit_children` --
unlike backslash-`Unary` (case below), legacy has no existing fallback
behavior here either; today it is a hard, silent no-op. Test:
`coverage_delta_goto_coderef_target`.

### 5. Regex-bind expressions with a nested call

```perl
package Foo;
sub bar { return compute() =~ /x/; }
```

`NodeKind::Match`/`Substitution`/`Transliteration` are not in
`visit_node`/`visit_children`'s coverage at all. This is likely the
**highest real-world-impact** case: `=~` binds are an extremely common Perl
idiom, and the corpus/real-project sweep (see "Aggregate impact" below)
shows this construct (or similar previously-unreached shapes) recurring
across nearly every real-world file tested. Test:
`coverage_delta_regex_bind_nested_call`.

### 6. `tie` argument lists

```perl
package Foo;
sub bar { tie my %h, 'Helper', extra_arg(); }
```

`NodeKind::Tie` has no arm at all in `visit_node`/`visit_children`. Test:
`coverage_delta_tie_args`.

### 7. Indirect-object call arguments

```perl
package Foo;
sub bar { my $obj = new Foo(make_arg()); }
```

`NodeKind::IndirectCall` (`new Class @args`) has no arm at all in
`visit_node`/`visit_children` -- note this is DIFFERENT from canonical's own
documented Phase-1 exclusion (`extract_symbol_refs` doesn't emit a
`SymbolRef` for the `IndirectCall` node itself either), but canonical's
generic `for_each_child`-based fallback still recurses into its `object`/
`args` today, so nested calls like `make_arg()` are already visible to
canonical -- just not to legacy. Test: `coverage_delta_indirect_call_args`.

### 8. `Subroutine` signature default-value expressions

```perl
package Foo;
sub greet($name = default_name()) { return $name; }
```

`IndexVisitor::visit_node`'s `Subroutine` arm visits only `body`, never
`prototype`/`signature`. (`Method`'s arm already visits `signature`
correctly -- this gap is `Subroutine`-specific.) Test:
`coverage_delta_subroutine_signature_default`.

**Implementation note (parity-test-caught bug, fixed before landing):** a
signature parameter's own BOUND variable (e.g. `$name` itself) must NOT be
walked as a reference -- it is a declaration site. `extract_symbol_refs`
has explicit skip logic for `MandatoryParameter`/`SlurpyParameter`/
`OptionalParameter`/`NamedParameter` that the generic `Node::for_each_child`
fallback does not know about (it just visits every child field
uniformly). The unified traversal replicates that explicit skip; the first
draft of this change did not, and `coverage_delta_subroutine_signature_default`
caught the resulting canonical-parity divergence immediately (see git
history on this file/PR for the failing run).

### 9. Non-`Variable` assignment / increment targets

```perl
package Foo;
sub bar { my %h; $h{compute_key()} = 1; }
```

`IndexVisitor::visit_node`'s `Assignment` arm (and the `++`/`--` `Unary`
arm) only special-cases a bare `NodeKind::Variable` lhs/operand -- for
anything else (e.g. an indexed/complex assignment target), it does
**nothing**: no classification, no recursion. A nested call inside the
index expression (`compute_key()`) is invisible today. Test:
`coverage_delta_assignment_indexed_target`.

## What does NOT change (and was verified, not assumed)

- **Canonical (`FileFactShard`) output is byte-for-byte identical** between
  production's existing dual walk and the unified traversal, across every
  fixture exercised (`assert_unified_canonical_parity`, run over all 9
  targeted edge cases in the parity-harness suite, all 37
  `test_corpus/gold/**/fixture.pl` fixtures, and all 29 `.pm`/`.pl` files
  across the mojolicious/dancer2/catalyst real-project skeletons). Canonical
  already reached all nine cases above via its own complete
  `Node::for_each_child`-based fallback -- unification changes nothing on
  that side.
- **Legacy `FileIndex` never LOSES a reference under unification**
  (`assert_unified_legacy_is_superset`, same fixture sweep): every key/count
  present under the old dual walk is still present, with at least as many
  entries, under the unified walk.
- `IndexVisitor`'s explicit legacy quirks are preserved exactly where they
  already existed -- e.g. a `Foreach` loop variable still gets BOTH a
  `Write` entry (from the loop-variable classification) AND a separate
  `Read` entry (from generic recursion into the same node) today, and the
  unified traversal reproduces that exact double-entry, not a "fixed"
  single entry, since fixing it would be an unrelated, unproven behavior
  change out of scope here.
- Declaration extraction (`extract_symbol_decls`, called with different
  `Some("main")`/`None` package-context seeds per projection) is completely
  **unchanged** -- still two separate calls. Unifying declarations is a
  separable follow-up (see the #1711 feasibility comment, item 3);
  resolving it was explicitly out of scope for this phase.

## Aggregate impact (from the corpus/real-project sweep)

`assert_unified_legacy_is_superset` reports (via `--nocapture`) the
per-fixture total legacy-reference-count growth. Representative counts from
one run (informational -- not asserted as a threshold, since the point is
"never fewer", not a specific number):

| Fixture | Old total | New total | Growth |
|---|---:|---:|---:|
| `test_corpus/gold/goto_oop_method/fixture.pl` | 26 | 27 | +1 |
| `catalyst_skeleton/lib/Catalyst/Dispatcher.pm` | 136 | 139 | +3 |
| `catalyst_skeleton/lib/Catalyst/Log.pm` | 78 | 82 | +4 |
| `catalyst_skeleton/lib/Catalyst/Utils.pm` | 103 | 110 | +7 |
| `dancer2_skeleton/lib/Dancer2/Core/DSL.pm` | 114 | 116 | +2 |
| `dancer2_skeleton/lib/Dancer2/Core/Request.pm` | 114 | 124 | +10 |
| `dancer2_skeleton/lib/Dancer2/Core/Response.pm` | 74 | 75 | +1 |
| `mojolicious_skeleton/lib/Mojo/Base.pm` | 150 | 157 | +7 |
| `mojolicious_skeleton/lib/Mojo/EventEmitter.pm` | 87 | 89 | +2 |
| `mojolicious_skeleton/lib/Mojolicious/Commands.pm` | 56 | 58 | +2 |
| `mojolicious_skeleton/lib/Mojolicious/Controller.pm` | 177 | 183 | +6 |
| `mojolicious_skeleton/lib/Mojolicious/Plugins.pm` | 72 | 75 | +3 |
| `mojolicious_skeleton/lib/Mojolicious/Renderer.pm` | 69 | 75 | +6 |
| `mojolicious_skeleton/lib/Mojolicious/Routes.pm` | 202 | 209 | +7 |
| `mojolicious_skeleton/lib/Mojolicious/Sessions.pm` | 66 | 68 | +2 |
| `mojolicious_skeleton/lib/Mojolicious/Types.pm` | 48 | 49 | +1 |
| `mojolicious_skeleton/lib/Mojolicious.pm` | 168 | 170 | +2 |

16 of the 29 real-project files, and 1 of the 37 gold-corpus fixtures, show
growth -- i.e. **this is not a rare edge case**: the majority of real-world
Mojolicious/Catalyst/Dancer2 code exercises at least one of the nine
constructs above (regex-binds are the most likely dominant contributor,
given how common `=~` is). Reproduce via:

```bash
cargo test -p perl-workspace --lib extraction_bundle_shadow_compare -- --nocapture --test-threads=1
```

## What this document does NOT claim

- It does not claim the newly-surfaced references are all independently
  *useful* (some may be low-value, e.g. duplicate coverage of something
  already found via a different path) -- only that they are currently
  *absent* and canonical already treats them as real.
- It does not claim cutover is safe to ship without further review: the
  maintainer's decision to cut over is a downstream-impact question (does
  gaining these references change rename/find-references/workspace-symbol
  results in a way users would notice, and is that good or bad?) that this
  characterization surfaces evidence for but does not itself answer.
- It does not attempt to unify declaration extraction (the `Some("main")`
  vs `None` package-context seed question) -- that remains a separate,
  smaller follow-up.

## Reproduction

```bash
cargo test -p perl-workspace --lib extraction_bundle_shadow_compare -- --nocapture --test-threads=1
```

All parity assertions are hard `assert_eq!`/`assert!` (not merely printed);
the aggregate-impact table above is `eprintln!`-only, informational context
alongside the mechanically-enforced pass/fail results.
