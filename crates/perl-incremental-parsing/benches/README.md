# Compatibility benchmark quarantine

The benchmark target in this directory is retained temporarily while unique evidence is classified and migrated out of `perl-incremental-parsing`.

It is **not** the production benchmark authority. The target mixes historical cache, checkpoint, token-retention, full-parse, and analysis mechanisms whose metrics do not all mean parser work avoided.

The machine-readable disposition lives in [`../behavior_disposition.json`](../behavior_disposition.json). Current work is split deliberately:

- #7072 defines strategy and actual-work receipts;
- #7045 proves output equivalence and metric truth;
- #7099 owns cold, warm-full, restart, synchronization, patch, fallback, analysis, and oracle benchmark regimes;
- #7081 removes unsupported current reuse and latency claims.

Until those contracts land, running this target proves only that the historical comparison code executes:

```bash
cargo bench -p perl-incremental-parsing
```

Do not report its `reuse`, `efficiency`, cache-hit, or elapsed-time fields as shipping incremental-parser performance without classifying the exact path and parser invocations under #7072.
