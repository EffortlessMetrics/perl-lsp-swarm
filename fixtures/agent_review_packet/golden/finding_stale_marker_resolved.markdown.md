# Review finding: example-train/R_service_marker/N_service_marker_probe/FIND-marker-tree-comparison

## Finding identity

- packet: example-train/R_service_marker/N_service_marker_probe/2026-08-24T00:00:00Z @ head b31d5e9a2
- lens: semantic_correctness
- outcome: resolved_current_head (severity advisory)
- final disposition: resolved_on_current_head

## Supporting evidence

- cargo test -p xtask --bin xtask service_marker_probe --locked@b31d5e9a2 (focused_test_observation): The stale-marker falsifier initially compared marker presence instead of recorded tree; the focused test passed for a marker recorded against another tree.
- invert tree comparison mutation@b31d5e9a2 (mutation_observation): Inverting the comparison flips both focused tests red, proving the comparison is load-bearing after repair.

## Suggested action

Bind the mismatch to the recorded-tree versus observed-tree comparison and reject alternate-tree markers in the same change.

## Builder response

- accepted: Repaired the comparison and added the alternate-tree rejection case to the focused test at the same head.

## Current-head resolution

- resolution head: b31d5e9a2
- evidence: cargo test -p xtask --bin xtask service_marker_probe --locked@b31d5e9a2 (after repair) (focused_test_receipt)
- evidence: invert tree comparison mutation red@b31d5e9a2 (mutation_receipt)
