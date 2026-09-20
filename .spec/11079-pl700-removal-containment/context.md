# Context: #11079 — withdraw the PL700 prose-driven whole-line removal edit

## Origin

Issue #11079 (train slot ICC-00C of parent programme #8277; sibling containments
#8305/#10690 organizer and #11158 completion imports). Replacement controllers:
#1719 (exact unused explicit imported binding + token pruning) and #8322
(exact complete module-load assessment + removal). Ruling until those land:

```text
PL700 code/message/range/line shape
!=
explicit-symbol removal authority
!=
complete module-load removal authority
```

The withdrawn edit derives its title from quoted diagnostic prose
(`Module 'Foo' appears to be unused`), requires only that the diagnostic line
begin with `use `, and deletes the complete line. It can therefore delete a
module load that carries compile/import/registration effects, destroy trailing
comments or grouping, retarget its presentation to a module named in prose but
not present on the diagnosed line, and expand a sub-line range to whole-line
geometry. None of these decisions owns semantic evidence.

## Route inventory (re-verified at main@16fef8db19c3aa0a82530ba39b5eb7625e581de1)

Live production routes being withdrawn:

1. `crates/perl-lsp-rs-core/src/providers/code_actions/diagnostic_routes.rs`
   — match arm `DiagnosticCode::UnusedImport` ("PL700") →
   `quick_fixes::fix_unused_import`. This is the only production dispatch of
   the diagnostic family into an edit.
2. `crates/perl-lsp-rs-core/src/providers/code_actions/quick_fixes.rs`
   — `fix_unused_import` itself: prose-derived action title plus
   whole-diagnostic-line deletion edit.
3. Live reachability: `crates/perl-lsp-rs/src/runtime/language/code_actions.rs`
   builds every `textDocument/codeAction` response through
   `CodeActionsProvider::get_code_actions`, which funnels diagnostics into
   `diagnostic_routes::quick_fixes_for_diagnostics`. No capability gate or
   feature flag stands between a client request and this route.

Verified non-routes (left untouched by this containment):

- V2 provider (`perl-lsp/src/features/code_actions_provider/`) — dispatch has
  no PL700/`UnusedImport` arm.
- Native critic registry (`perl-lsp-rs/src/perl_critic/`) — no unused-import
  rule emits a Safe automatic fix.
- Legacy `BuiltInAnalyzer` (`perl-lsp-rs-core/src/tooling/perl_critic/`) — no
  unused-import violation produces a quick fix.
- Enhanced provider (`code_actions/enhanced/import_management.rs`) —
  organize-imports family (#8305 lane), not PL700-driven.
- `perl-parser` compatibility provider (`ide/lsp_compat/code_actions.rs`) —
  quick-fix dispatch handles only syntax/style/security codes; its generic
  "Remove unused imports" placeholder is an empty-edit organize-imports surface
  owned by #8305/#10690, not keyed on PL700.
- Diagnostic producer (`providers/diagnostics/lints/unused_imports.rs`) —
  emits the PL700 hint only; unchanged here so the diagnostic remains a clearly
  approximate, non-fixable advisory pending its owning diagnostic issue.
- VS Code extension — no command, menu, or keybinding reaches this family.

## Withdrawal decision

The internal `CodeAction` type has no disabled representation and the shared
disabled/refusal seam does not exist yet (#4206/#4212). Per the issue's outcome
clause 3, the action is **omitted** rather than represented as a disabled
placeholder. A truthful greyed-out "unavailable" presentation returns only when
the shared seam lands; until then omission is the honest state. The unsafe edit
is unreachable either way.

## Restoration pointers

Restoration is not a revert. Exact explicit-symbol removal belongs to the
#1719 train; exact complete module-load removal belongs to the #8322 train;
future live cutovers #10728/#10758. Any mutation that re-couples PL700 code,
message text, or line geometry to import-edit authority must fail the
containment guards added by this PR.
