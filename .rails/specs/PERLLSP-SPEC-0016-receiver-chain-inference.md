# PERLLSP-SPEC-0016 — Bounded receiver chain inference

## Goal
Support chains like `$service->db->prepare->execute` via bounded semantic inference.

## Rules
- Add depth cap (default 4).
- Stop on dynamic method names or unknown packages.
- Collapse to fallback for unresolved or ambiguous unions.
- Persist boundary metadata (dynamic/stale/fallback transitions).

## Safety
Exact claims require the same freshness/confidence/source-backed gates at each promoted hop.
