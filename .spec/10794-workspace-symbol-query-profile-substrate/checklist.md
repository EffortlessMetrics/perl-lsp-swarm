# Checklist: #10794 — query profile substrate

## Order of work

- [x] Reconciliation: substrate absent at `main@ab3cece9d`; call-site inventory
      recorded in context.md; no colliding PR/branch.
- [x] Spec packet (this directory) before production edits.
- [x] Red-first: WS-QP-011 (non-match → None), WS-QP-013 (profile mismatch
      cannot prove exactness), M1 mutation control (fallback/no-match
      conflation) written against the new owner before migration edits.
- [x] New owner module `perl-workspace::workspace_symbol_query`:
      profile compile, typed match API, evidence comparator, legacy order
      projections, work receipt, digest derivation.
- [x] Migrate P1/P4 index paths + handler compile-once.
- [x] Migrate P2/P3 provider paths via forwarding shims in `symbol_query`
      (numeric fallback tier removed).
- [x] Remove/forward duplicate helpers reached by migrated paths:
      - symbol_query::matches_query → forwarding to new owner;
        compare_names_by_query → **deleted** in repair review (zero
        production callers after evidence-sort migration; legacy-slot mapping
        divergent for loose-ineligible queries); match_tier / is_subsequence
        removed with the migration;
      - workspace_index inline matcher + matches_query_text + local
        is_subsequence → consume profile/evidence;
      - MIN_LOOSE_MATCH_QUERY_CHARS re-exports stay as deprecated-forwarding
        until #9268 (public perl-symbol API removal is out of scope).
      - open-document text-fallback matcher (`fallback/text.rs`) inventoried,
        NOT migrated: divergent untrimmed/ungated contains-only admission;
        handoff #10645/#10642 (context.md site 7).
- [x] Architecture recurrence rules added to `cargo xtask layer-check`:
      perl-symbol ↛ lsp-types, perl-symbol ↛ perl-workspace,
      perl-workspace ↛ perl-lsp-*.
- [x] Verification commands run and recorded.

### Verification record (repair head)

| command | result |
| --- | --- |
| `cargo fmt -p perl-workspace -p perl-lsp-rs -p perl-lsp-rs-core -- --check` | pass |
| `cargo test -p perl-lsp-rs-core --all-targets --locked symbol_query` | green — 13 passed (7 lib, 5 g1a edge cases, 1 providers_module_shape), 0 failed |
| `cargo test -p perl-workspace --all-targets --locked workspace_symbol` | green — 22 passed (18 lib, 4 integration), 0 failed |
| `cargo test -p perl-lsp-rs --all-targets --locked workspace_symbol` | green — 273 suites ok incl. 22 lib tests, 0 failed |
| `cargo clippy -p perl-lsp-rs-core --lib`, `-p perl-workspace --all-targets`, `-p perl-lsp-rs --lib` (`--locked -- -D warnings`) | pass on every surface this claim edits |
| `cargo xtask layer-check` | pass (incl. new recurrence rules) |
| `cargo xtask semantic-scorecard --check` | pass |

Pre-existing-environment observations (not introduced by this branch; recorded
per the honesty rule rather than silently skipped):

- `cargo clippy ... --all-targets -D warnings` on the two lsp crates fails in
  test-only files whose content is byte-identical to `origin/main`
  (`items_after_test_module`, `expect()` in `tests/common/*`,
  `references_pir_burn_in.rs`, `detect_dead_code_mid_surrogate_position.rs`;
  last touched by #11905/#2641 on main). Claim-surface clippy (above) passes.
- `cargo xtask check-test-wiring` reports 58 unwired test files across many
  crates; none intersects this branch's changed paths (no module was added,
  removed, or re-declared here). Pre-existing main-wide condition.
- Repo-wide `cargo fmt --all -- --check` aborts with Windows OS error 206
  (command line too long) in this worktree; per-package spellings pass.

## Profile version/digest

```text
WORKSPACE_SYMBOL_QUERY_PROFILE_VERSION = 1
policy id  = ws-symbol-query/exact-prefix-substring-subsequence.v1
digest     = FNV-1a64 over (version bytes, policy id bytes, folded query
             bytes); stable across processes. Browse flag and loose-tier
             eligibility are pure functions of those inputs (empty fold /
             folded char count) and are not separate digest inputs; any
             change to them implies a change in the digested bytes.
```

## Explicit non-normalizations (unchanged by this PR)

NFC/NFKC, accent folding, transliteration, locale collation, grapheme
segmentation, package canonicalization, sigil/qualified/boundary/acronym/
multiword behavior.

## Handoffs

- #10645: per-row best-key aggregation consumes this evidence/comparator.
- #10642: final composer consumes #10645 output.
- Q02 #10806 / Q03 #10827: extend profile/policy-id/version in place.
- #9268: retire `MIN_LOOSE_MATCH_QUERY_CHARS` from public perl-symbol API once
  upper callers are live.
- #9147/#9888: orphan parser-local provider deletion (untouched here).

## Notes for reviewers

- `cargo xtask check-architecture` named in the issue no longer exists on
  main; current recurrence surface is `layer-check` (+ `check-test-wiring`),
  which this PR extends and runs.
- Cross-path ordering divergence (provider tier-len-lex vs index rank-lex) is
  pre-existing on main and preserved per path with fixtures; unification is
  #10645/#10642 scope.
