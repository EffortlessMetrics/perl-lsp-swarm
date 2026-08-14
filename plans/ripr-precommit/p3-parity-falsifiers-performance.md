# P3 — staged RIPR parity, falsifiers, cache identity, and latency

Issue: #9115  
Parent: #9112  
Depends on: #9114 / PR #9122  
Branch: `agent/ripr-staged-proof`

## End goal

Earn the evidence required to promote the P2 shadow evaluator into a blocking commit control. This PR remains non-blocking: it expands the adversarial fixture corpus, proves same-subject staged-versus-committed normalization parity, defines exact local result-cache identity, and measures cold/warm latency against #3786's commit-tier budget. If the proof is not good enough, this PR must say so and leave P4 blocked rather than weakening RIPR semantics.

## Codex implementation order

1. Read the complete P2 implementation before adding test helpers; use the same production path the hook uses rather than creating a test-only staged evaluator.
2. Build a deterministic Git fixture harness capable of staging partial changes, preserving conflicting unstaged bytes, committing an equivalent candidate, mutating the live index after `TREE_OID` capture, and exercising unborn HEAD.
3. Define normalized parity in terms of canonical actionable-gap identities and repository decision state. Ignore incidental temp paths/timestamps only when they are explicitly outside the semantic contract.
4. Add the full falsifier matrix below with one expected result class per case.
5. Add or specialize the existing commit-tier cache so identical staged subjects can reuse complete trusted RIPR evidence. Cache full evidence/decision, not a boolean.
6. Make the cache key explicit and test each invalidation component independently.
7. Instrument phase timing without turning volatile host timings into tracked universal claims.
8. Run repeated cold/warm measurements over representative one-file, production+test, medium crate, and large-but-bounded changes.
9. Record an evidence-backed P4 go/no-go disposition in the issue/PR. Do not make P3 itself blocking.

## Mandatory falsifier matrix

```text
01 staged bad production + unstaged source fix
02 staged bad production + unstaged test fix
03 staged production + staged focused test
04 staged test-only change
05 deleted expression/function tail
06 whole-file delete
07 rename without content change
08 rename with content change
09 staged ripr.toml change
10 staged suppression-policy change
11 worktree config differs from staged config
12 live index changes after TREE_OID capture
13 worktree mutates during producer run
14 malformed/empty RIPR JSON
15 RIPR non-zero exit
16 RIPR timeout
17 missing RIPR binary
18 wrong RIPR version
19 identical staged tree rerun/cache hit
20 unborn HEAD / first commit
21 Windows-style path with spaces/backslashes
22 existing deleted-side #3212 reproducer
23 existing test-harness #3213 reproducer
```

Each case records expected subject identity, expected posture, expected canonical gaps, and whether an equivalent committed subject must agree.

## Cache key

At minimum:

```text
base identity
candidate TREE_OID
expected RIPR version
observed RIPR version / executable identity as required
staged-RIPR adapter/decision schema version
effective ripr.toml digest
suppression-policy digest
RIPR analysis-affecting environment identity
```

Never cache a timeout, malformed output, tool failure, partial/ineligible result, or unresolved subject as a clean success.

## Performance phases

Measure separately:

```text
sandbox/materialization
diff generation
producer startup
producer analysis
normalization/suppression
cache hit
total
```

Promotion targets remain:

```text
warm median < 5s
warm p95    < 15s
hard ceiling 30s
```

Do not switch to `instant` or `--no-unchanged-tests` to meet these numbers. `draft` plus unchanged tests is the semantic claim being evaluated.

## Likely files

```text
xtask staged/commit-check fixture helpers
staged RIPR evaluator tests
ripr_evidence normalization tests
commit-tier cache implementation/tests if existing cache needs specialization
benchmark/measurement helper only if repo policy already supports local measurements
this plan file
```

Avoid creating a new required remote benchmark workflow.

## Acceptance before merge

- same TREE_OID staged and equivalent committed normalized decisions agree;
- unstaged repairs and later index changes cannot affect the bound result;
- deletes/renames/config/suppression/test-only cases have explicit dispositions;
- cache identity is exact and fail-closed;
- complete identical results are reusable;
- cold/warm latency is measured with the accepted methodology;
- no semantic weakening was used for speed;
- the issue carries an explicit P4 promotion recommendation.

## Suggested review map

Review the fixture's ability to falsify subject leakage first, parity normalization second, cache identity third, and measurements last. A green test that cannot actually distinguish staged bytes from worktree bytes is not evidence.
