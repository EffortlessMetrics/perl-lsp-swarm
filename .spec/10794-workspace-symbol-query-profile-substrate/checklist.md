# Checklist: #10794 — query profile substrate

## Order of work

- [x] Reconciliation: substrate absent at `main@ab3cece9d`; call-site inventory
      recorded in context.md; no colliding PR/branch.
- [x] Spec packet (this directory) before production edits.
- [x] Red-first: WS-QP-011 (non-match → None), WS-QP-013 (profile mismatch
      cannot prove exactness), M1 mutation control (fallback/no-match
      conflation) written against the new owner before migration edits.
- [ ] New owner module `perl-workspace::workspace_symbol_query`:
      profile compile, typed match API, evidence comparator, legacy order
      projections, work receipt, digest derivation.
- [ ] Migrate P1/P4 index paths + handler compile-once.
- [ ] Migrate P2/P3 provider paths via forwarding shims in `symbol_query`
      (numeric fallback tier removed).
- [ ] Remove/forward duplicate helpers reached by migrated paths:
      - symbol_query::matches_query / compare_names_by_query / match_tier /
        is_subsequence → forwarding to new owner;
      - workspace_index inline matcher + matches_query_text + local
        is_subsequence → consume profile/evidence;
      - MIN_LOOSE_MATCH_QUERY_CHARS re-exports stay as deprecated-forwarding
        until #9268 (public perl-symbol API removal is out of scope).
- [ ] Architecture recurrence rules added to `cargo xtask layer-check`:
      perl-symbol ↛ lsp-types, perl-symbol ↛ perl-workspace,
      perl-workspace ↛ perl-lsp-*.
- [ ] Verification commands run and recorded.

## Profile version/digest

```text
WORKSPACE_SYMBOL_QUERY_PROFILE_VERSION = 1
policy id  = ws-symbol-query/exact-prefix-substring-subsequence.v1
digest     = FNV-1a64 over (version, policy id, folded bytes, browse flag,
             loose eligibility); stable across processes
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
