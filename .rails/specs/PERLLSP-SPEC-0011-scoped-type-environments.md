# PERLLSP-SPEC-0011 — Scoped type environments

## Problem
Provider decisions need lexical-scope facts at cursor position, not broad accumulated state.

## Proposal
Add scoped snapshots and APIs:
- `environment_at(position)`
- `fact_at_position(name, position)`

## Rules
- Inner scope shadows outer scope.
- Stale scope snapshots cannot authorize exact behavior.
- Missing/stale facts preserve fallback.

## Track boundary
Track C owns scope-aware semantic gating; parser concerns remain Tracks A/B.
