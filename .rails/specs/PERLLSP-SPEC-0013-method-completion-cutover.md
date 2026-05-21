# PERLLSP-SPEC-0013 — Method completion cutover by receiver class

## Problem
Receiver fact kinds outnumber promoted exact completion classes.

## Initial promotion targets
- `HashRefSlot`
- `ArrayIndex`

## Promotion gate
Promote only when all are true:
- exact fallback state,
- high confidence,
- fresh fact,
- no dynamic boundary,
- source range present,
- package resolvable.

## Labels
Completion details must include receiver-class evidence labels for exact claims.

## Safety
Dynamic, stale, ambiguous, low-confidence, and unsupported generated/no-source facts remain fallback/blocked.
