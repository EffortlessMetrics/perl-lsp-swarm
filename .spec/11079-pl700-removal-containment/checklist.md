# Checklist: #11079 — PL700 removal containment

Mutations that would restore prose-driven whole-line import deletion — each
must fail the named guard:

| Mutation | Guard that must fail |
| --- | --- |
| Re-add the `DiagnosticCode::UnusedImport` arm to `diagnostic_routes.rs` | `no_production_route_references_the_withdrawn_pl700_edit` (file-scoped UnusedImport ban) + every behavioral falsifier |
| Re-introduce `fix_unused_import` (or a renamed clone) in any production `src/` path | source-scan global symbol ban |
| Derive an action title from PL700 diagnostic prose | message-retarget falsifier (`use A;` + prose naming `B`) |
| Delete a line that begins with `use ` but carries an explicit import list | use-with-args whole-line falsifier |
| Destroy trailing comments on the diagnosed line | registration-comment falsifier |
| Expand a sub-line range inside a multiline directive to line deletion | multiline-range falsifier |
| Stand in refusal with an enabled empty edit / no-op / disabled stub carrying data | no-action-at-all assertions (provider + process) |
| Bypass via client-supplied context diagnostics, filtered request, or forged resolve | exact-process stdio fixture negative cases |
| Restore the edit under a compatibility provider or command surface | route inventory re-check + process fixture |

## Verification commands

```bash
cargo fmt -p perl-lsp-rs-core -p perl-lsp-rs -p perllsp -- --check
cargo clippy -p perl-lsp-rs-core -p perl-lsp-rs -p perllsp --all-targets --locked -- -D warnings
cargo test -p perl-lsp-rs-core --all-targets --locked code_action
cargo test -p perl-lsp-rs --all-targets --locked code_action
cargo test -p perl-lsp-rs-core --test pl700_withdrawal_containment_tests --locked
cargo test -p perl-lsp-rs-core --test quick_fix_new_codes_bdd --locked
cargo test -p perllsp --test lsp_pl700_withdrawal_process --locked
cargo xtask check-test-wiring
cargo xtask check-support-claims
git diff --check
```

`cargo xtask check-architecture` does not exist on the current xtask CLI
(same finding as the #8305 packet); the route/architecture guard is the
in-tree source-scan test above.

`cargo xtask check-test-wiring` / `check-support-claims` could not run at
this branch point: `xtask` itself fails to compile on Windows/stable 1.95.0
(`windows_by_handle` unstable — `volume_serial_number()`/`file_index()` in
`tasks/generate_semantic_snapshot.rs:242`, E0658). Pre-existing condition,
unrelated to this diff; repository-wide gate ownership remains with hosted CI.

## Advertisement verification

Searched current head for surfaces advertising automatic unused-import removal:
features.toml QuickFix description and LSP_FEATURES_OVERVIEW list only
variables/strict/warnings/deprecated-pattern fixes; no capability snapshot,
provider contract, status table, VS Code contribution, or book page claims a
PL700 fix. No advertisement edits are required; this file records that check.
The PL700 mention in docs/development/FRESHNESS_RAIL.md is historical CI
narrative about GoneModule/PL701, not fix advertisement.

## Shift-left discipline

The behavioral falsifiers and the source-scan guard were added first and proven
failing against unmodified main before the production deletion landed. The
exact-process fixture is part of the shift-left set because the defect is
production routing, not provider-unit behavior alone.
