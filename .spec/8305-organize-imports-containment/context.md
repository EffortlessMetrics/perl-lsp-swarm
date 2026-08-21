# Context: #8305 — withdraw the legacy organize-imports edit

## Origin

Issue #8305 (train slot ICC-00A of parent programme #8277). Ruling: until #8319
admits and implements a bounded source-preserving cohort (future live cutover
#10696),

```text
line adjacency / prefix / module category / alphabetical order
!=
semantic transformation authority
```

The legacy organizer collects lines beginning with selected `use` prefixes,
bucket-sorts them alphabetically, finds the first and last retained import-looking
lines, and replaces the complete interval. It has no proof that complete
directives commute, no package/scope/phase/`@INC` barrier model, no comment or
trivia ownership, and no guarantee that bytes between the first and last
import-looking lines are unrelated to the replacement. Any executable statement
between two import-looking lines is destroyed by the replacement.

## Route inventory (re-verified on this branch at main@331397cf9)

Live production routes:

1. `crates/perl-lsp-rs-core/src/providers/code_actions/enhanced/import_management.rs`
   `organize_imports` — the destructive implementation (`collect_imports` →
   `sort_imports` → `find_imports_range` → whole-interval replacement edit).
2. `crates/perl-lsp-rs-core/src/providers/code_actions/enhanced/mod.rs`
   `get_global_refactorings` — global call site that attaches the action to every
   enhanced code-action response.
3. `crates/perl-lsp-rs-core/src/providers/code_actions/refactors.rs`
   `get_refactoring_actions` — second composition path into the same enhanced
   provider (same choke point as 1–2).
4. `crates/perl-lsp-rs/src/runtime/language/code_actions.rs` — both the original-
   and enhanced-provider response builders map
   `InternalCodeActionKind::SourceOrganizeImports` to `"source.organizeImports"`
   with an inline workspace edit; `codeAction/resolve` only fills quickfix pragma
   edits and cannot reach the organizer.
5. Capability advertisement: `crates/perl-lsp-rs-core/src/features/flags.rs`
   `BuildFlags::{production,ga_lock}.source_organize_imports = true`;
   `crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs`
   `code_action_kinds` pushes `source.organizeImports` when the flag is set.
6. Capability snapshots: `crates/perl-lsp-rs/tests/snapshots/*.json`,
   `lsp_cap_snap__*.snap`, `lsp_code_actions_snap__code_actions_unfiltered.snap`.
7. VS Code extension: `perl-lsp.organizeImports` command (package.json commands,
   editor/context menu, `shift+alt+o` keybinding), status menu item
   (`showStatusMenuCommand`), refactoring quick-pick item, all delegating to
   `editor.action.organizeImports`, which sends a filtered
   `source.organizeImports` request to the server.

Verified non-routes (left untouched):

- `perl-refactoring::ImportOptimizer` / `perl-parser` `ide::lsp_compat`
  `CodeActionProvider::create_sort_imports_action` — public library helpers in
  the composition crate, not wired into `perllsp`/`perl-lsp-rs`; they are not the
  live authority and not the replacement owner (#8319 owns replacement design).
- No `workspace/executeCommand` route invokes the organizer.
- `add_missing_imports` (quickfix) belongs to sibling lane #10690; PL700 removal
  belongs to #11079; completion auto-import belongs to #11158. None enter this PR.

## Withdrawal decision

- Delete `organize_imports` and its call site: no client can receive the
  line-oriented edit from any direct, filtered, resolve-shaped, command,
  original-, enhanced-, parser-compat, or extension route.
- Remove `BuildFlags::source_organize_imports` and the gated advertisement:
  advertisement cannot disagree with runtime availability because no code path
  can advertise the kind. Restoration (#10696) must re-introduce advertisement
  together with a proven cohort.
- The action contract here cannot truthfully represent a disabled state (no
  shared disabled/refusal seam for source actions exists on this surface), so the
  action is omitted entirely rather than replaced by an enabled empty/no-op
  stand-in, per the issue's stop-condition guidance.
- Extension contributions for the withdrawn flow are removed (command, menu,
  keybinding, quick-pick items) so no enabled no-op remains user-visible.

## Preserved unrelated behavior

Quick fixes (including add-missing-imports), refactor.extract/rewrite, source
actions (`source.fixAll`, `source.modernize`), commands, capability rows for
other kinds, and all other extension commands remain behaviorally unchanged.

## Replacement / restoration pointers

#8319 (replacement controller), #10696 (proven-cohort live cutover),
#10865 (authority ledger). #3080/PR #3150 remain exposure history only.
