# Checklist: #11673

- [x] Inventory push entry points: `publish_diagnostics`, `publish_syntax_only_diagnostics`,
      `publish_parse_errors_fast`, didOpen/didChange guarded no-parse notifies,
      debounced fire (`publish_fn` → `publish_diagnostics`), close-time clear (out of scope, lifecycle-owned).
- [x] Red-first falsifiers: fast-path ABA (close/reopen between snapshot and enqueue),
      full-path ABA (value-only check passes on stale instance), late-N-after-N+1 rejection.
- [x] Sink operation `commit_push_diagnostics(identity, payload, disposition)` with
      sink-lock-local validate → compare/record → enqueue.
- [x] Migrate fast/full/syntax-only/guard paths; keep outer `commit_parse_effect_if_current`
      as cheap coalescing only.
- [x] Typed outcome vocabulary shaped for #11672 retargeting.
- [ ] Focused proof: `cargo fmt`, clippy `-p perl-lsp-rs`, targeted diagnostics tests
      (`RUST_TEST_THREADS=2`).
- [ ] PR body review map; publish; review; live CI; squash merge; closeout.
