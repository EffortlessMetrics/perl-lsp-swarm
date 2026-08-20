# Tree-Sitter Incremental Crossover Evidence

> **Status: historical/non-routing evidence (archived).** This document
> preserves an earlier exploratory measurement pass for a token-replay path
> associated with `tree-sitter-perl-rs`. It does not describe the current
> parser implementation, an active crossover decision, or current routing
> policy. Keep the receipts for provenance only; do not use them to enable or
> route production behavior.

The measurements below are historical evidence, not a universal performance
promise and not evidence for AST-subtree reuse.

## Historical receipt identity

The historical receipts for all three profiles were produced from commit
`085c09bb8c8b9950264d9e8322ca263228860daf` with `rustc 1.95.0
(59807616e 2026-04-14)` through the repository's now-retired command wrapper.
The direct commands below are retained as reproduction context only; they are
not a current workflow, implementation, or routing instruction, and they are
not a claim that the historical receipts were produced by these exact commands:

```text
cargo xtask tree-sitter-incremental-proof --profile pr
cargo xtask tree-sitter-incremental-proof --profile nightly
cargo xtask tree-sitter-incremental-proof --profile release
```

The machine-readable receipts are written to
`target/receipts/tree-sitter-incremental-proof-{pr,nightly,release}.json` and
are identified by the profile-specific `input_hash` recorded in each receipt.

```text
pr      fa6523905635d4a16a21f59b73f2c66e76330c742460153029a653ec346e659c
nightly 8cd53b407f0b3bf964af2d1b7f56160690d1fda22b8365cb1ebb15fccaab055b
release 3086329efa38a0a94e160590e320e34a295f47a226223727d33de2afbc7e7052
```

| Profile | Rows | Operations | Fallback | Equivalence failures | Replay p95 faster |
| --- | ---: | ---: | ---: | ---: | ---: |
| PR | 54 | 270 | 29.3% | 0 | 6 (11.1%) |
| Nightly | 72 | 1,080 | 29.7% | 0 | 14 (19.4%) |
| Release | 90 | 2,250 | 29.9% | 0 | 14 (15.6%) |

The historical measurements establish mechanical fresh-equivalence for this
matrix. They
do not establish that replay is faster in general; p95 results are from one
machine and vary between runs.

## Historical release-profile comparison

The release profile groups rows by document size. `Ratio` is the median of
`replay_p95 / fresh_p95`; values below 100% favor replay. Work columns are
averages over the rows in each band.

| Size band | Rows | Faster rows | Faster rate | Ratio | Fallback | Reprocessed bytes | Tokens reused | Tokens re-lexed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Small (<=1.2 KB) | 46 | 3 | 7% | 148% | 33% | 238 | 52 | 57 |
| Medium (1.2-10 KB) | 22 | 3 | 14% | 160% | 41% | 2,234 | 572 | 565 |
| Large (10-50 KB) | 18 | 5 | 28% | 108% | 14% | 5,183 | 2,944 | 479 |
| Very large (>50 KB) | 4 | 3 | 75% | 80% | 12% | 41,918 | 6,512 | 1,149 |

At the time of this measurement, the data suggested the following provisional
shape:

- the measured small and medium bands did not justify a replay preference;
- the measured large band was mixed and needed repeated measurements by edit
  class;
- the measured very-large band showed the clearest replay benefit, but the
  sample was only four rows;
- recovery, incomplete, quote-like, heredoc, and other context-sensitive edits
  retained substantial fallback behavior and would require fail-closed
  handling in any future experiment.

That historical data was not sufficient to hard-code a byte threshold or enable
replay globally. Any future reactivation would need to repeat each matrix cell
and report confidence or run dispersion, then add phase timing for edit
validation, checkpoint selection, re-lexing, token assembly, parser execution,
AST reconstruction, and cache rebuilding.

## AST-reuse decision

In this historical snapshot, AST-subtree reuse remained deferred. The receipts
show fresh-equivalence and benefit in the largest measured band, but they do not
measure phase costs or prove that AST reconstruction is the dominant remaining
cost. No current AST-reuse or replay-routing decision follows from these
receipts.
