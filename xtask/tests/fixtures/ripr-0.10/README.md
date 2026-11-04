# Real `ripr` producer output — 0.9.0 / 0.10.0 differential

`hosted-16619-static-limitations.json` and
`hosted-16619-counted-raw-gaps.json` are field-preserving projections of
`ripr/review/comments.json` and `ripr/pr/raw-check.json` from hosted run
36419991913, artifact 10974153790
(SHA-256 `f833083add52b3cd37c40819c8c7f30101f341d78895a3506be8a41a7a5972f6`).
The guidance projection retains the receipt identity, all three inline
`call_presence` cards, and all seven `summary_only` cards. The raw projection
retains the three counted `no_static_path` findings. Fields unused by this
audit are omitted; retained values are unchanged. The
evaluated merge head is `cb695fb0bc2b7c74a311bbbed95cf7adae3b3222`, from
PR head `7280bb349f6986b04661e3d46bdc062fd0d4e210`. Each call expression
is unchanged in PR #16619's base-to-head diff, but its owner function changed.
The three cards say `gap_state: static_limitation` and `why_not_actionable`
while still carrying generic `suggested_test` text. RIPR 0.10.0's
`review_comments.rs` emits this combination, so the suggestion is not a
contradictory actionable disposition.

The raw count is **not** these three static cards. Its probes identify changed
output literals at `workspace_trust_report.rs:66` (two) and `:126` (one).
The seventh `summary_only` card is actionable but concerns an unchanged
`doctor.rs:257` seam and is absent from that counted basis. A gate must join
individual counted findings to guidance before subtracting dispositions;
matching head/base and subtracting by count alone does not establish that join.

These files are **captured producer output**, not hand-authored JSON. #9113 requires
the 0.10 migration to be proved against what the tool actually emits, because the
repository has a recorded failure mode where a version bump changed JSON fields and
the consumer silently stopped matching them
(`docs/learnings/2026-06-ripr-output-schema-break.md`).

## Provenance

Both files were produced by running the published releases over the **same** diff, so
any difference between them is a producer change and nothing else:

```bash
# Subject: a two-function crate with two tests, one of which only smoke-calls the
# changed function. The captured diff narrows `severity_label`'s threshold from
# `level > 2` to `level >= 2`.
ripr check --root . --diff <diff> --format json
```

The invocation mirrors the production one in
`xtask/src/tasks/ripr_evidence.rs::run_ripr_check` (`check --root … --diff … --format
json`), and `[analysis] mode = "draft"` comes from the repository `ripr.toml`.

| File | Producer |
|---|---|
| `weakly-exposed-check-0.9.json` | `ripr 0.9.0` — the release this repository ran before #9113 |
| `weakly-exposed-check.json` | `ripr 0.10.0` — the reviewed release after #9113 |

A deliberately small subject keeps the captures reviewable: a diff against this
repository produces ~13 MB of `related_tests` / `assertion_texts` payload for the same
one-line change. The only transformation applied is pretty-printing with sorted keys;
no field was added, removed, retargeted, or renamed. Nothing in either file references
a host path — `root` is `.` and probe paths are repository-relative.

## Measured schema delta (0.9.0 → 0.10.0)

This is the explicit disposition #9113 asks for:

| Surface | Change |
|---|---|
| `schema_version` | **`0.1` → `0.2`** — the only declared break signal |
| top-level keys | unchanged (`base`, `findings`, `mode`, `root`, `schema_version`, `summary`, `tool`) |
| `summary` fields | **identical** — nothing added, nothing removed |
| `findings[]` keys | **additive only**: `assertion_texts`, `related_tests_total` |
| `classification` values | unchanged; `weakly_exposed` still spelled `weakly_exposed` |

Every field the repository consumer reads — the `summary.weakly_exposed` /
`summary.reachable_unrevealed` / `summary.no_static_path` counts, the `findings[]`
array, and each finding's `classification` and `probe.file` / `probe.line` — survives
0.10 unchanged. **No parser broadening was required for the bump**, which is why
#9113 says to harden the parser only where real 0.10 output demands it.

The `weakly_gripped → reachable_unrevealed` alias in `ripr_evidence.rs` is retained:
0.10 does not emit `weakly_gripped`, but removing the alias would be an unrelated
behavior change to suppression canonicalization, and this PR is a version/schema
migration.

## What these fixtures prove

`weakly-exposed-check.json` carries exactly one **actionable** finding
(`classification: "weakly_exposed"`, matching `summary.weakly_exposed == 1`), so it
discriminates the count the required `ripr+ New Gap Gate` acts on rather than only the
`exposed` findings that never gate. The 0.9 capture is kept beside it so the
differential stays checkable after the tool versions are gone from any given machine.
