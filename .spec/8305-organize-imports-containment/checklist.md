# Checklist: #8305 — organize-imports containment

Mutations that would restore broad replacement or duplicate advertisement —
each must fail the named guard:

| Mutation | Guard that must fail |
| --- | --- |
| Re-add `organize_imports` (or any `collect_imports → sort_imports → find_imports_range` pipeline) to a production `src/` path | `no_production_route_references_the_withdrawn_organizer` source-scan test |
| Re-wire the enhanced provider to emit a `SourceOrganizeImports` action | `enhanced_provider_never_offers_organize_imports*` behavioral tests |
| Return a broad first-to-last import-line replacement from any action | executable-statement-between-imports negative fixtures (provider + exact process) |
| Re-advertise `source.organizeImports` in capabilities | `source_organize_imports_is_withdrawn_from_every_profile`, snapshot pins, process-fixture initialize assertion |
| Re-add extension command/menu/keybinding/quick-pick entries | extension jest suites asserting absence + package.json contribution scan |
| Resolve-shaped bypass injecting an organizer edit | resolve handler quickfix-only pin |

## Route inventory at implementation time

See context.md. Live routes: enhanced provider global action, refactors.rs
composition, runtime kind mapping, capability flag + advertisement, snapshots,
VS Code command/menu/keybinding/quick-pick. Non-routes: perl-parser lsp_compat,
perl-refactoring ImportOptimizer, executeCommand (none), sibling lanes #10690 /
#11079 / #11158.

## Verification commands

```bash
cargo fmt -p perl-lsp-rs-core -p perl-lsp-rs -- --check
cargo clippy -p perl-lsp-rs-core -p perl-lsp-rs --all-targets --locked -- -D warnings
cargo test -p perl-lsp-rs-core --all-targets --locked code_action
cargo test -p perl-lsp-rs --all-targets --locked code_action
cargo test -p perl-lsp-rs-core -p perl-lsp-rs --all-targets --locked organize
cargo test -p perl-lsp-rs-core --test organize_imports_containment_tests --locked
cargo test -p perllsp --test lsp_organize_imports_containment_process --locked
cargo xtask check-test-wiring
cargo xtask check-support-claims
git diff --check
```

`cargo xtask check-architecture` does not exist in the current xtask CLI; the
route/architecture guard is the in-tree source-scan test above. VS Code
extension jest suites run locally when node_modules can be installed;
otherwise CI (`ux-regression-gate.yml` extension-jest job) owns them.
