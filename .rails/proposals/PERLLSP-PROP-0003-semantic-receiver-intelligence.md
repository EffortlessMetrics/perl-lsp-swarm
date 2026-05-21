# PERLLSP-PROP-0003 — Semantic Receiver Intelligence (Track C)

## Summary
Track C turns existing type-engine substrate into safe, source-backed, evidence-labeled method completion for Perl object receivers.

## Rail boundaries
- **Track A owns parser target fairness and comparison framing.**
- **Track B owns production parser edge-gap closure and parser fix delivery.**
- **Track C owns type facts, receiver facts, and method completion cutover policy.**

## Existing substrate (already present)
- `TypeFact` with erased type compatibility and richer semantic metadata.
- `ShapeFact` family including hash, array, and object shapes.
- `ReceiverFact` extraction primitives used by completion classification.
- `HashShape` and `ObjectShape` semantic shapes.
- Narrow high-confidence source-backed hash-slot exact completion pilot.

## Problem
Semantic facts exist, but user-visible completion behavior is only partially cut over. Track C formalizes how and when receiver classes are promoted from fallback/shadow into exact completion.

## Decision
Track C uses `.rails/` as durable source of truth and introduces:
- receiver-class specs,
- exact/fallback/dynamic promotion gates,
- lane tracking and receipts-driven progression.

## Safety posture
Completion broadens **only by receiver class** and only after exact/fallback/dynamic receipts prove behavior.

The following must **not** authorize exact behavior:
- dynamic receivers,
- stale facts,
- ambiguous unions,
- low-confidence or unsupported medium-confidence facts,
- generated/no-source facts without supported promotion path.

## Non-goals in C1
- No runtime/code behavior changes.
- No parser target or parser gap work (Tracks A/B).
- No latency rail changes.

## Expected outcome
A durable Track C rail that governs semantic receiver intelligence from:

`TypeFact -> ReceiverFact -> exact/fallback/blocked provider decision -> method completion -> receipts`.
