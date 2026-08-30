# Checklist: #11674

- [x] Inventory: `reindex_document_symbols` / `clear_document_symbols` call sites
      (didOpen clean+failed, 3 didOpen guards, 3 didChange guards,
      post-parse worker path, didClose eviction).
- [x] Sink operation `commit_document_symbols(identity, disposition)` with
      validate -> compare/record -> atomic mutate; ledger is the #6729 anchor.
- [x] Migrate all parser-triggered sites; keep didClose eviction lifecycle-owned
      and documented as the raw exception.
- [x] Extraction-to-commit test hook (`document_symbols_before_commit_hook`).
- [x] Retarget sink outcomes onto #11672's `ParseEffectCommitOutcomeV1`
      (contract landed after the sink); claim-local enum removed, mapping
      recorded in context.md.
- [x] Focused proof: sink falsifiers + handler preservation flows; fmt/clippy
      `-p perl-lsp-rs`; targeted filters (`document_symbol`, `text_sync`,
      `workspace` symbol suites) under `RUST_TEST_THREADS=2`.
- [ ] PR body review map; publish; review; live CI; squash merge; closeout.
