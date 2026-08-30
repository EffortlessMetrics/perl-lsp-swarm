# Context: #12823 — corpus ratchet cache cycle (SIGTERM vs cold install)

## Problem

`Post-Merge Corpus Ratchet` (`post-merge-corpus-ratchet.yml`) has failed every
scheduled run since inception: 99/99 red, oldest recorded 2026-05-20. The full
lane's `Save CPAN corpus cache (full)` step is gated on
`steps.cpan-install.outcome == 'success'`, and the install never reaches
`success`: an external platform SIGTERM lands at ~24m29s of job wall-clock
(observed 2026-08-10 job 93397289624, 2026-08-25 job 97732030193,
2026-08-26 run 32946491767 / job 98108195798 — always at/just after Batch 10 of
40), well under the configured `timeout-minutes: 120`. Because the corpus
checkpoint is only saved after one complete pass, a killed install banks
nothing; the next day restores nothing (`cpan-corpus-Linux-...` miss) and
repeats the identical cold loop. The cycle is self-sustaining by gating, not by
any parser defect.

Forward progress is already mechanically supported in-tree:
`cargo xtask cpan-corpus install` runs incremental mode against a populated
install dir (cpanm skips installed modules per batch; see
`xtask/src/tasks/cpan_corpus.rs`, `is_install_populated` + incremental branch),
so persisted partial state converges across runs. Only two things are missing:

1. persistence of partial state (the save gate requires one perfect pass);
2. ending the install cleanly below the observed ~24.5 min preemption envelope
   so any single scheduled run keeps control of its own termination.

## Governing evidence (2026-08-26, origin/main@d2f6f9bde46e880200af34658d9d727d69f195cc)

- Live receipts: five most recent scheduled runs all `failure`
  (32946491767, 32825363442, 32705081617, 32627235945, 32561271632).
- Kill signature re-confirmed from issue #12823: exit code 143 + "The runner has
  received a shutdown signal", Batch 9/40 stall → internal 300 s timeout →
  individual retries → SIGTERM. Not the job timeout (120 min configured at both
  relevant head SHAs); no concurrency-group collision (unique prefix); other
  long jobs the same day ran >34 min uninterrupted.
- House precedent for scheduled warm lanes: ripr.yml `seed-cache` job (#12688)
  — daily schedule-gated job builds artifacts under shared keys so other lanes
  restore warm.
- Cache policy (`docs/ci/cache-policy.md`): saves belong to main/schedule
  events, PRs restore-only. The corpus lanes are already schedule/dispatch-main
  gated, consistent with this design.

## Approach chosen (budget-aware checkpointed warm lane)

Ranked against the four candidate approaches from the lane brief, using only
in-tree evidence:

1. **Chosen — budget-aware phase gating + unconditional checkpoint
   persistence.** Splits today's monolithic full job into a schedule-gated warm
   job (`corpus-warm-full`) that always runs the install under an explicit wall
   clock budget (default 12 min < envelope with setup+save headroom), exits
   cleanly with a machine-readable completion marker, and saves whatever
   consistent state exists (canonical key on completion, rolling checkpoint key
   otherwise). A separate `corpus-ratchet-full` job runs today's sweep/ratchet/
   enforce chain verbatim, but only when the marker reports a complete corpus;
   otherwise it skips neutrally. Convergence is monotone: each scheduled run
   advances the cpanm frontier (incremental resume) until a full pass fits in
   budget, after which the canonical key is saved and the gate chain behaves
   exactly as before.
   Matches issue desired directions 1 (persistent per-batch progress), 2
   (bounded chunks below the preemption envelope), and partially 3 (the full
   1000-dist target is preserved but no longer requires one perfect pass).
2. Seeded-cache pre-warming from the existing scheduled seed job — rejected as
   primary: #12688's `seed-cache` warms only ripr rust-cache entries; no CPAN
   corpus seed artifact exists anywhere in-tree, so any seeding still needs this
   same checkpoint mechanism first. The chosen design is itself the seed job.
3. Parallel install split — rejected: cpanm invocations share one local-lib and
   `.cpanm` home (concurrent configure/build races); the river-sorted list does
   not partition dependency trees across shards; added nondeterminism buys
   little on a network-bound workload.
4. Self-hosted-lane demotion of the heaviest leg — rejected: no self-hosted
   runner infrastructure exists in-tree (all workflows pin ubuntu-24.04);
   executing third-party Perl distribution build scripts on self-hosted infra
   contradicts this workflow's read-only/no-write-authority design.

## Claim boundary

This claim repairs CI economics and the cache cycle only. It changes no parser,
baseline threshold, manifest semantics, or gate assertion: on a complete corpus
the ratchet/enforce steps execute byte-for-byte the same commands. While
converging, the scheduled workflow honestly reports "warm job made bounded
progress; ratchet skipped because corpus incomplete" instead of red-noise caused
by external preemption. Workflow conclusion during convergence is driven by the
warm job, which fails only if its own contract (run within budget, bank or keep
state consistently, report truthfully) breaks.

## Composition with repair PR #12827

PR #12827 (nightly trust-jobs repair) touches fuzz targets, benches, symbol
panic fixes, justfile public-api-check, kwalitee doc regen — zero file overlap
with this claim (workflow yml, xtask CLI/install wiring, new policy test file).
Its non-goal line names this issue (#12823) as escalated; nothing here alters
its proof surfaces.

## Rollback

Single-revert candidate: restore the workflow to the monolithic full job (git
revert of the workflow hunk) and drop the xtask flag (additive, unused by any
other caller). Rolling checkpoint keys simply stop being written; existing
canonical `cpan-corpus-Linux-*` keys remain valid and continue restoring. No
tracked-file regeneration is involved.
