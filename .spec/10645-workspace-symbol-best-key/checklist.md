# Checklist — #10645

- [x] Spec packet precedes production edits.
- [x] Red case proven against pre-repair seam: geometry dedup collapsed
      same-anchor rows deterministically (`left: 1, right: 2`) and bare-exact
      evidence flipped with the hash seed on `main@8aaadee46`.
- [x] One accumulation authority consuming #10794 comparator; no local tiers.
- [x] Source-backed seam aggregates all admitted keys per row before materialization.
- [x] Generated/framework seam selects best key with membership-preserving filter.
- [x] Cap applied strictly after per-row best-key selection.
- [x] Deterministic total output order independent of map iteration.
- [x] Work receipt counters exposed; unset counters remain `not_proven`.
- [x] Stable regressions WS-BEST-001..013 green.
- [x] Mutation controls M1..M15 each falsified by a named proof (see acceptance.md).
- [x] Architecture gate `cargo xtask check-workspace-symbol-best-key` wired and green
      (falsified once by injecting a banned pattern, then restored).
- [x] Focused packages: fmt clean, clippy `-D warnings` green for touched code,
      lib tests 819/819 green.
- [x] No cross-row dedup/ranking/budget scope absorbed (#10642 untouched).
- [x] Handoff recorded for #10642; next leaf noted (#10806 parallel extension).
