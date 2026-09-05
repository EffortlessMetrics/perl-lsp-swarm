# Checklist: #12823 checkpointed corpus warm lane

Base pin: `origin/main@d2f6f9bde46e880200af34658d9d727d69f195cc` (2026-08-26).
Composition sibling: PR #12827 (nightly trust-jobs repair) — disjoint file set;
this claim implements its own escalated residual.

Red-first receipts-of-record (2026-08-26):

- Runs API `post-merge-corpus-ratchet.yml`, event=schedule, five most recent all
  failure: 32946491767 (2026-08-26T08:11Z), 32825363442, 32705081617,
  32627235945, 32561271632.
- Kill signature per issue #12823: job 98108195798 start 08:11:36Z, SIGTERM
  08:36:05Z (~24m29s), exit 143, "Batch 9/40 stalls → internal 300s timeout →
  individual retry → SIGTERM ~80s later"; cache miss `cpan-corpus-Linux-...`
  with save gated on install success.
- In-tree mechanism: legacy save gate literal
  `if: steps.cpan-install.outcome == 'success'`; install skip gate literal
  `if: steps.cpan-corpus-cache-restore.outputs.cache-hit != 'true'`.

Planned surface:

- [x] `.github/workflows/post-merge-corpus-ratchet.yml`: split full lane into
      schedule-gated `corpus-warm-full` (unconditional budgeted install, marker
      output, canonical + rolling saves) and completion-gated
      `corpus-ratchet-full` carrying the byte-preserved sweep/ratchet/enforce/
      scope/artifact chain; `open-ratchet-pr` unchanged
- [x] `xtask/src/tasks/cpan_corpus.rs`: additive `time_budget` config +
      deadline-aware batch planning helpers, machine-readable completion marker,
      unit tests for planner/marker/budget guards
- [x] `xtask/src/main.rs`: `--time-budget-minutes` clap arg on
      `cpan-corpus install`, wired additively (no behavior change when absent)
- [x] `xtask/tests/corpus_ratchet_checkpoint_policy.rs`: structural policy pins
      CRW-001..CRW-010 incl. mutation controls
- [x] Package-scoped proof: fmt, clippy `-D warnings`, tests for xtask only

Proof commands:

```bash
cargo fmt -p xtask -- --check
cargo clippy -p xtask --all-targets --locked -- -D warnings
cargo test -p xtask --all-targets --locked
```

Open residuals (owned by issue #12823 follow-up, not silently dropped):

- [ ] Runner-diagnostics investigation of external SIGTERM provenance (design
      no longer depends on it)
- [ ] Cache-retention/prune economics across repo-wide 10 GB limit if rolling
      checkpoints ever displace other lanes
