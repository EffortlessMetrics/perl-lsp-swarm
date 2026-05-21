# Semantic Receiver Intelligence lane (Track C)

Track C governs: `TypeFact -> ReceiverFact -> provider exact/fallback/blocked decision -> method completion -> receipts`.

## Ownership split
- Track A: parser target fairness/comparison.
- Track B: production parser edge-gap closure.
- Track C: semantic type/receiver facts and method-completion cutover.

## Current substrate
TypeFact, ShapeFact, ReceiverFact, HashShape/ObjectShape, and a narrow hash-slot exact completion pilot already exist.

## Safety rule
Completion broadens per receiver class only after exact/fallback/dynamic receipts. Dynamic, stale, ambiguous, low-confidence, and unsupported generated/no-source facts must not authorize exact behavior.

## Scope note
This lane starts with rail/spec/policy structure before behavior changes.
