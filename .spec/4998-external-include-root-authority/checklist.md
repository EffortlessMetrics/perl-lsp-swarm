# #4998 - External include-root authority checklist

Proof executed on candidate branch `fix/4998-includepaths-client-channel`
(worktree `plsw-lane-4998`, base `origin/main@16fef8db1`).

## Production changes

- [x] `ExternalIncludePathAuthority` + `UnauthorizedExternalIncludePathSource`
      replace `apply_external_include_paths: bool`
      (`crates/perl-lsp-rs-core/src/config/mod.rs`)
- [x] Default context fails closed as unclassified-untrusted
- [x] Untrusted arrivals rejected with actionable channel-naming reason;
      accepted values never cleared (`RejectedClientIncludePathReason::ExternalUnauthorized`)
- [x] Unscoped slot classified `GenericUnscopedConfiguration`; folder slot
      `FolderConfiguration`; init options `InitializationOptions`
      (`crates/perl-lsp-rs/src/runtime/workspace/configuration_response.rs`)
- [x] didChangeConfiguration sites classified
      `DidChangeConfiguration`
      (`crates/perl-lsp-rs/src/runtime/workspace.rs`)
- [x] Recurrence gate pinning dependency-detected roots to relative contained
      literals (`crates/perl-lsp-rs-core/src/config/dependency_detection.rs`)
- [x] Product claim aligned (vscode-extension/package.json)

## Proof log

- [x] `cargo fmt -p perl-lsp-rs-core -- --check` — clean
- [x] `cargo fmt -p perl-lsp-rs -- --check` — clean
- [x] `cargo clippy -p perl-lsp-rs-core -p perl-lsp-rs --locked -- -D warnings -A missing_docs`
      (CI gate shape, justfile:239) — clean
- [x] all-targets clippy capture: zero diagnostics in touched files; remaining
      errors are pre-existing main-baseline test-file lints outside CI's gate
- [x] `cargo test -p perl-lsp-rs-core --lib --profile agent --locked` — 3250 passed / 0 failed
- [x] `cargo test -p perl-lsp-rs --lib --profile agent --locked` — 1601-1602 passed;
      rotating failures confined to five known load-flaky timing tests
      (doctor probes, text-sync churn, indexing readiness); each passes in
      isolation and none touches this claim's seams
- [x] `cargo build -p perllsp --profile agent --locked`
- [x] `cargo test -p perl-lsp-ux-tests --profile agent --locked --test ux_scenario_14_inc_conformance`
      — 14 passed / 0 failed including the flipped zero-visibility fixture

## Residual claims (registered, not closed here)

- Trusted user/operator adapter implementation: #10817 observation train
- Full typed provenance for every include-root writer (runtime_derived class):
  #10813/#10807
- Actual-client (Coc/Neovim/Emacs) host proofs consume this server policy:
  #10741 lane
