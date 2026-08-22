# Context: #10690 — withdraw hard-coded missing-import edits

## Origin

Issue #10690 (train slot ICC-00B of parent programme #8277; sibling containments
#8305 organizer, #11079 PL700 removal). Replacement controllers: #790 (exact
candidate planner train) and #8948/#10738 (governed planner/action). Authority
ledger #10865; action/blocker authority #4206/#4212. Ruling until #790 lands:

```text
hard-coded function→module affinity
or PL109 presentation
!=
candidate identity
!=
import edit authorization
```

The withdrawn edits turn a hard-coded spelling (`dumper`, `encode`, `decode`,
`basename`, `dirname`, `mkpath`, `rmtree`, `slurp`, `decode_json`,
`encode_json`) into an inserted `use Module;`. Neither route establishes one
current unresolved callable subject, lexical/package ownership, exporter
visibility, effective `@INC`, ambiguity, import form, exact insertion geometry,
existing-directive extension semantics, or held-action currentness.

## Route inventory (re-verified at main@b38568b082a733dfac7b3da76eb626b77f8a9616)

Live production routes being withdrawn:

1. **Route A — enhanced global missing-import action**
   `crates/perl-lsp-rs-core/src/providers/code_actions/enhanced/import_management.rs`
   `add_missing_imports`: local undefined-looking-function scan
   (`find_undefined_functions`) → `guess_module_for_function` → insert
   `use <module>;\n` at `helpers.find_import_insert_position()`. Dispatched from
   `enhanced/mod.rs::get_global_refactorings`. No capability gate stands between
   a client request and this route.
2. **Route B — PL109 diagnostic import quick fix**
   `crates/perl-lsp-rs-core/src/providers/code_actions/diagnostic_routes.rs`
   extends the `UnquotedBareword` ("PL109") arm with
   `quick_fixes::fix_import_for_bareword_function`, which resolves the symbol at
   the diagnostic range against the same hard-coded map and inserts
   `use <module>;\n` after the last `use`/`require` line.
3. **Hard-coded affinity table**
   `crates/perl-lsp-rs-core/src/providers/import_management/mod.rs::
   guess_module_for_function` — the ten-spelling static map feeding both routes.
   No ledger row marks it fixture/history/non-authoritative, so per outcome 6 it
   is removed outright.
4. **Compatibility placeholder advertisement**
   `crates/perl-parser/src/ide/lsp_compat/code_actions.rs` pushes an enabled,
   empty-`WorkspaceEdit` action literally titled "Add missing imports".
   Verified non-route from the shipped server (no crate outside `perl-parser`
   consumes `perl_parser::ide`; zero references from perl-lsp-rs-core,
   perl-lsp-rs, perllsp), but it advertises the withdrawn claim through a public
   API surface, so the placeholder arm is removed under outcomes 3 and 7.

Verified non-routes (left untouched by this containment):

- Completion: no completion `additionalTextEdits` path consumes the affinity
  table (`rg guess_module_for_function` finds no completion consumer).
- Commands: no workspace command reaches either route; the process fixture
  additionally probes a fabricated import command and requires rejection.
- `perl-parser/src/refactor/import_optimizer.rs` "Add missing imports":
  `ImportOptimizer`/`RefactoringEngine` have zero references from the server
  crates; its `analysis.missing_imports` comes from its own import analysis,
  not the hard-coded table. Organizer territory (#10696), outside this claim.
- Feature/capability surfaces: `features.toml` QuickFix description lists
  variable/pragma/deprecated-pattern fixes only and never advertised
  missing-import insertion; capability snapshots unchanged expected.
- VS Code extension: no command, menu, or keybinding reaches these families.

## Withdrawal decision

The internal `CodeAction` type has no disabled representation and the shared
disabled/refusal seam does not exist yet (#4206/#4212). Per outcome clause 3 and
the issue's stop condition, the actions are **omitted** rather than represented
by enabled empty edits or disabled stubs; omission needs no new shared seam, so
no prerequisite filing is required. Both bypasses become unreachable in the same
PR — disabling only Route A would leave Route B live.

## Preservation obligations

- PL109 quoting fixes survive untouched: `quick_fixes::fix_bareword` keeps its
  single-quote, double-quote, and uppercase-filehandle declaration options.
- Every other diagnostic/code-action family is untouched; deletion-only
  production change (two functions + one scan helper + one table + one dispatch
  block + two compat lines).
- `collect_imports`/`sort_imports`/`find_imports_range`/`ImportManager` remain:
  they are organizer helpers owned by the #8305/#10696 lane, not affinity
  authority, and existing suites pin them.

## Restoration pointers

Restoration is not a revert. Exact unresolved-subject selection belongs to #790;
governed planner/action to #8948/#10738. Any mutation that re-couples a
hard-coded spelling, a PL109 presentation, a `container_name`, or byte-zero
insertion geometry to import-edit authority must fail the containment guards
added by this PR (`missing_import_withdrawal_containment_tests.rs` source-scan
ban plus behavioral falsifiers).
