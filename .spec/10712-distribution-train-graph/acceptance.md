# Acceptance: #10712 checked distribution train graph

From the controlling issue, each item bound to its mechanical check:

- One versioned checked DAG owns #10703 implementation dependency semantics —
  `distribution_train.v1` (this bundle), loaded fail-closed by
  `xtask/src/tasks/distribution_train.rs`.
- Stable architecture contains no mutable GitHub/agent work-state fields —
  `deny_unknown_fields` + pinned canonical digest.
- Hard, evidence, optional, route-mode, and product-unit dependencies are mechanically
  distinct — three closed dependency arrays plus `product_units`/`limitations`.
- Controllers, contracts, implementations, adapters, proof nodes, claims, and release
  handoffs are distinct — closed `kind` vocabulary; controllers/release handoffs carry
  `do_not_build` and `NO_SPEC_DELTA`.
- Standalone mode-specific predecessor DAG is encoded rather than flattened — #8367
  depends only on #10243; #9090 is a conditional join before #8359; #8372 is
  conditional release-critical.
- Managed chain encoded from current owners — #10073+#9925 → #9020 → #10079 → #10082 →
  #10083 → #7859 with #9847/#7880 evidence.
- Binstall/Action/public-route/satellite relationships are represented without copied
  authority — #8338/#9022 adapters consume #6067; satellites stay
  controller/handoff nodes that no hard edge may target.
- `check`, `graph`, `explain`, and current-tree-only `next` are deterministic —
  `cargo xtask distribution-train check|graph|explain|next`, second-run clean.
- `explain` is sufficient for a coding agent to understand the one-PR boundary without
  re-mining issue comments.
- Spec disposition integrates with #3983 (`distribution-spec check|explain|index`,
  #11164); writer admission remains #3982.
- Current-tree implementation state remains distinct from installed/public evidence —
  status vocabulary has no pass/installed semantics.
- No product behavior, branch, PR, assignment, release, or external state changes —
  this bundle is data plus offline xtask projections only.
