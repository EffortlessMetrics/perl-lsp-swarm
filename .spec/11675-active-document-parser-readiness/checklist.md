# Checklist: #11675

- [x] Inventory: old `active-document-ready` emission sites (didOpen index task,
      didChange index task), pending-parse accounting (`notify_parse_complete`,
      untouched), consumers (#4048/#7383-class UX tests).
- [x] State machine + required-effect profile v1 (push diagnostics publication,
      document symbols; pull-client applicability at install).
- [x] Install points: didOpen insert, didChange generation bump (sync) and
      async enqueue; acceptance marks in didOpen, sync fallback, and worker
      callback; guarded/failed terminals; supersession; close removal.
- [x] Effect attach hooks in both accepted-ticket sinks.
- [x] Red-first order witness (fails on main emission ordering), stale-attach,
      guard supersession, pull-profile, close-removal proofs; fmt/clippy
      `-p perl-lsp-rs` all-targets clean.
- [ ] PR body review map; publish; review; live CI; squash merge; closeout.
