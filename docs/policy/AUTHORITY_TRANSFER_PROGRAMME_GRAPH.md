# Authority-Transfer Stable Programme Graph

> Status: accepted contract (AT01, #11697) under programme #11696.
> Machine DAG: `.ci/authority-transfer-programme/graph.v1.json`
> Schema: `authority_transfer_programme_graph.v1`

This document names the canonical surfaces for the stable authority-transfer
programme graph: the versioned manifest for the semantic-close and
configuration-convergence train (#10168 controller rail and the configuration
authority rail), plus the AT01–AT08 control-plane leaves.

## What the graph owns

Accepted programme topology only:

- stable node IDs independent of issue titles, paths, or ordering;
- one bounded proposition per node with explicit non-claims;
- typed semantic edges (hard, evidence, optional, parallel-after, fan-in);
- authority inputs/outputs with exactly one canonical owner per output;
- exclusive/shared semantic conflict keys — never file globs alone;
- operation/evidence profile IDs from the graph registry;
- claim ceilings, registered first falsifiers, durable artifacts with owners;
- predecessor identity (declared stable node IDs, resolved fail-closed) plus
  exit condition for retirement nodes;
- downstream handoffs and the terminal relation a leaf may use.

Controllers (`PGC` #11696, `SMC` #10168, `CFG` #6738), fan-in rows
(`FI1` #7066, `AT07` #11703), and live enforcement (`LE1` #10416) are
non-buildable governance roles and never enter a builder frontier. The optional
live observer (`AT06` #11702) is marked `observation_optional` and can never be
a hard dependency of offline work.

## What the graph must never contain

No current issue state, PR or branch identity, main SHAs, proof verdicts,
readiness, assignees/leases, completion estimates, or model routing. Those are
observations owned by #11698 (exact-tree probes) and #11699 (offline
frontiers), keyed to these stable node IDs. The invariant:
current-tree movement changes projections, never graph bytes.

Domain semantics stay with their canonical issues; the graph references them by
issue number and stores only addressable dependencies, authorities, and claim
boundaries. Any node field outside the schema fails closed as
`EMBEDDED_DOMAIN_POLICY`; reserved current-state keys fail as `STATE_LEAKAGE`.

## Canonical validator

```text
xtask/src/bin/authority-transfer-graph.rs
```

Deterministic commands (exit 0 pass / 2 typed rejection / 3 instrument failure):

```bash
cargo run -p xtask --bin authority-transfer-graph -- check
cargo run -p xtask --bin authority-transfer-graph -- graph
cargo run -p xtask --bin authority-transfer-graph -- explain <node-id>
cargo run -p xtask --bin authority-transfer-graph -- normalized-manifest
```

`check` validates the stable manifest, proves every shift-left fixture still
rejects for exactly its pinned reason, proves the positive control accepts, and
regenerates the committed projection byte-for-byte.

## CI invocation

```bash
scripts/check-authority-transfer-graph.sh
```

The script runs rustfmt over the validator, the validator test target, and the
`check` command above. The `.github/workflows/authority-transfer-graph.yml`
lane invokes it on every PR or push touching the manifest, fixtures,
validator, script, or the workflow itself.

## Fixtures

`.ci/authority-transfer-programme/fixtures/*.json` are deliberately invalid mini
graphs added before (and kept ahead of) validator behavior. Each envelope pins
one expected rejection code; `valid-mini-graph.json` is the positive control.
The committed generated projection lives at
`.ci/authority-transfer-programme/generated/normalized-graph.v1.json` and must
stay byte-identical under repeated normalization.

## Handoff

```text
#11697 stable graph (this contract)
→ #11698 projects exact current-tree observations keyed by stable node IDs
→ #11699 derives independent build/review/closeout eligibility
```
