# Parser edge-gap closure lane

Track A covers parser-target comparison fairness and current behavior snapshots.
Track B covers production parser edge-gap closure and boundedness.

Known gaps are sourced from `docs/issues/corpus/gaps/README.md` and parser docs.
Each gap must be represented with a ledger row, fixture (or accepted-impossible rationale), proof command, and claim boundary.

Timeout/hang risks must have boundedness budgets.
Impossible Perl cases are bounded/degraded, not silently claimed correct.

This lane defines process and artifacts only; it does not change parser behavior.
