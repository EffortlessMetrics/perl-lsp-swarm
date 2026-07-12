# PR 1711-A -- didChange re-extraction work-shape measurement receipt

**Controlling issue:** [#1711](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1711)
(LSP-freshness reliability lane). **Related:** #3396.

This is a **measurement-only** receipt. No extraction or propagation behavior
changed. The instrumentation that produced these numbers
(`crates/perl-workspace/src/workspace/workspace_index.rs`, `reindex_metrics`
module) is compiled ONLY under `#[cfg(test)]` -- verified by `cargo check -p
perl-workspace` and `cargo check -p perl-lsp-rs` (no `--tests`) producing an
identical build before and after this change.

## What was measured

The production `didChange` -> shard-update path
(`WorkspaceIndex::index_file_with_generation`, called from
`crates/perl-lsp-rs/src/runtime/text_sync.rs:294` and `:1076` via a background
task) on a fixture file with 80 `sub`s (565+ LOC, 81 symbols including the
enclosing package -- see `fixture_prefix`/`fixture_is_large_enough` in
`reindex_workshape_measurement`), across six edit classes:

| # | Edit class | anchors | entities | occurrences | edges | Extraction re-run? |
|---|---|---|---|---|---|---|
| 1 | Comment/whitespace-only (trailing, no byte-span shift) | unchanged | unchanged | unchanged | unchanged | **YES -- every extractor runs once** |
| 2 | Reference-only (new call to existing sub) | **changed** (see note) | unchanged | changed | -- | YES |
| 3 | Declaration-changing (new sub) | changed | changed | -- | -- | YES |
| 4 | Dynamic/generated fact (`eval "sub NAME {...}"`) | changed | changed | -- | -- | YES |
| 5 | Revert-to-original | == original (bit-identical) | == original | == original | == original | YES (deterministic) |
| 6 | Superseded generation (older, out-of-order) | n/a -- rejected before commit | n/a | n/a | n/a | **NO -- rejected pre-parse, no stale publish** |

**Note on class 2:** `anchors_hash` changes even though no declaration
changed. `symbol_refs_to_semantic_facts`
(`crates/perl-symbol/src/surface/facts.rs`) emits one `AnchorFact` per
REFERENCE, not only per declaration -- the `anchors` category conflates
declaration-anchors and reference-anchors. Any hypothetical category-hash-gated
skip would have to treat "anchors changed" as common (any new/removed
reference trips it), not rare. This does **not** affect the comment-only case
(class 1), where no reference is added or removed either.

## The key measurement: comment-only edit (class 1)

Despite **zero** category-hash change, the current path re-runs, once each,
every canonical extractor:

- `IndexVisitor::visit` (legacy symbol/reference walk)
- `extract_symbol_decls`
- `extract_symbol_refs`
- `extract_eval_sub_boundaries`
- `extract_generated_member_facts`
- `extract_import_specs`
- `extract_use_lib_facts`

and tears down + rebuilds the legacy symbol/search/global-reference caches in
full: 321 symbol-table entries removed and re-added (80 subs x [sub + 2
params + 1 lexical] + 1 package), 643 global-reference entries removed and
re-added -- identical counts before and after, i.e. this churn produces no net
change, only wasted work.

### Informational timing (NOT gated -- no hard-millisecond threshold anywhere in this PR)

Single-sample and 15-iteration repeated-edit distributions, `cargo test`
`dev`-profile-with-opts build, shared/debug hardware (informational only, per
the measurement-discipline directive):

| Metric | Single sample | 15-iteration min / median / max |
|---|---|---|
| Instrumented extraction total (visit + 6 extractors) | ~1.0 ms | 0.84 ms / 1.23 ms / 1.60 ms |
| Independent full re-parse of the same text | ~1.3 ms | 1.01 ms / 1.28 ms / 2.53 ms |

Extraction is **the same order of magnitude as parsing itself** on this
fixture -- roughly 50-100% of parse cost, not a negligible fraction of it.
Reference-only, declaration-changing, and dynamic-fact edits all show similar
per-edit extraction totals (~1.2-1.8 ms single-sample) since the SAME
unconditional full re-extraction runs regardless of edit class.

## Disposition: MATERIAL

Per the issue's own framing, this crosses from "bounded" to "material":

- The comment-only case (the edit class #1711 exists to ask about) still pays
  full extraction + full legacy cache churn on every edit, even though the
  category-scoped propagation machinery (`ShardCategoryHashes` /
  `plan_shard_replacement`) proves after the fact that nothing downstream
  needed to change.
- Extraction cost is comparable to parse cost on this fixture size (not
  "cheap relative to parse" -- the bounded-disposition criterion from the
  issue thread does not hold here).
- Correctness is not at risk (class 6 confirms superseded generations are
  still rejected before publish; class 5 confirms determinism across
  revert). This is purely a cost/overlap finding.

**Recommendation:** proceed toward a retirement design (a follow-up issue/PR,
NOT this one), but first prove that category-preclassification (a cheap
pre-check of which categories a diff could plausibly touch, run BEFORE the
full extractors) is itself cheaper than the extraction it would avoid --
otherwise the fix trades one full walk for two. The class-2 finding above
(reference-anchors share the anchors category) means any preclassification
design must treat "anchors changed" as common, not rare, or it will provide
little benefit beyond the comment-only case. Consolidating the legacy
`IndexVisitor` projection with the canonical extraction path (today they run
independently and duplicate a full AST walk) is a candidate first step,
since it would shrink the "extraction total" baseline being measured here
before any skip-logic is added.

## Reproduction

```bash
cargo test -p perl-workspace --lib reindex_workshape_measurement -- --nocapture --test-threads=1
```

Six edit-class tests plus a repeated-sampling timing test, all under
`crates/perl-workspace/src/workspace/workspace_index.rs` ::
`reindex_workshape_measurement`. Every category-hash assertion is a hard
`assert_eq!`/`assert_ne!` (mechanically enforced); every timing number is
`eprintln!`-only (never asserted against a threshold).
