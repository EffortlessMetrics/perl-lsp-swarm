# Checklist: #11301

- [x] Record the landed #11298 candidate/commit seam and accepted base.
- [x] Inventory direct source callers, including `index_files_batch`.
- [x] Add initial single-file and batch names.
- [x] Add a non-zero owner-supplied `NonZeroU32` live generation guard with URI identity.
- [x] Add typed live outcomes and deterministic unit proof.
- [x] Migrate only established initial fixture/profile callers.
- [x] Ledger deferred compatibility callers with successors and removal conditions.
- [x] Add and run the source-backed caller/compatibility validator twice.
- [ ] Migrate didOpen/didSave (#11305) in its own claim; explicitly deferred from #11301.
- [ ] Decide watcher, close/reload, file-operation, stable-read, root/configuration,
  provider, and complete-snapshot semantics in their owning claims.
