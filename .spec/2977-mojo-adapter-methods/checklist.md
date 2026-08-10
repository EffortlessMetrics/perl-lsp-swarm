# Implementation checklist

- [x] Issue #2977 claimed and narrowed to a review-forward first slice.
- [x] Current completion and semantic-analyzer seams inspected.
- [x] Mojo::Pg and Mojo::mysql upstream method references recorded.
- [x] Add separate adapter method catalogs at the existing completion seam.
- [x] Gate catalog selection on explicit imported-module evidence.
- [x] Add completion tests for both adapters and prefix filtering.
- [x] Add negative tests for missing imports and unknown adapter names.
- [x] Confirm DBI and generic method completion remains unchanged.
- [x] Decide whether hover/signature can reuse the catalog without scope growth;
  defer it because this slice is static completion only.
- [x] Run focused tests, formatting, relevant clippy/policy checks, and
  cargo-allow review. Focused tests/formatting/Clippy pass; cargo-allow's
  changed-file diff timed out and its spec-system doctor reports the repo's
  pre-existing missing artifact-ledger/profile setup.
- [x] Review the diff against this spec and simplify before PR.
- [x] Post proof and claim boundary to issue #2977.

## Explicit follow-ups

- Result-object methods (`Mojo::Pg::Results`, `Mojo::mysql::Results`).
- Promise/callback return-flow inference and issue #2587 boundary.
- Pub/sub callback signatures and migration DSL semantics.
