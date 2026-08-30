# Checklist — #9378 full-sync UTF-16 initialize/session contract

- [x] Two-writer check: no open PR references #9378 (2026-08-29).
- [x] Premise verified on current main lineage (a83ad9a027): split
      authorities, no-common fallback test, no session contract.
- [x] #8129 gate: `full_document_utf16` selected for implementation.
- [x] Model: closed offer classification, selection reason, immutable
      accepted session contract, bounded evidence (session_contract.rs).
- [x] Transaction: classify before mutation; typed -32602 failure for
      no-common/malformed; atomic acceptance after response verification.
- [x] Projection: response positionEncoding/textDocumentSync built from the
      accepted contract; divergence is a typed failure.
- [x] Competing authorities removed: `ClientCapabilities.position_encoding`
      deleted; local `sync_kind` and hard-pinned string deleted.
- [x] Consumers re-pointed: pull-diagnostics projection, parity test helper.
- [x] Ledger: `positionEncodingPin` row + `positionEncodingUtf16Pin` compat
      row updated; `docs/specs/lsp-final-surface-inventory.json` regenerated
      via the crate's sanctioned regeneration entry point.
- [x] Falsifiers LSP-FS16-001..012 encoded as tests; red-then-green recorded
      (focused runtime gate observed RED with the old fallback wired through
      the new pipeline, GREEN after the fail-closed flip).
- [x] Package proof green: perl-lsp-rs lib 1842 / perl-lsp-rs-core lib 3715
      / lifecycle integration + census + ownership map green; per-package
      fmt clean; clippy: perl-lsp-rs --all-targets clean, perl-lsp-rs-core
      findings confined to pre-existing untouched files (host baseline).
- [ ] PR opened citing (#9378), closes #9378 on merge if acceptance is met.

Known-unrelated observations (documented for review):

- `runtime::text_sync::tests::test_diagnostics_churn_drains_retained_state_after_close_delete`
  is load-flaky under parallel suite runs; reproduced identically on clean
  main a83ad9a027 in an isolated proof worktree (67 passed / 1 failed both
  sides). Not touched by this claim.
- On this Windows host, `tests/lsp_3_17_lifecycle_tests.rs`
  `test_shutdown_exit_3_17` (main's code, untouched) terminates its own test
  process early, truncating that binary's full run; the test passes in CI
  environments and the targeted suites around it are green here.
