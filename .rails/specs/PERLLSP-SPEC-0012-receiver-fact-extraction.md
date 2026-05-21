# PERLLSP-SPEC-0012 — Receiver-fact extraction from real AST

## Problem
Synthetic receiver probe reparsing drops file context and complicates chain analysis.

## Proposal
Extract receiver facts from the real AST and cursor position via `receiver_fact_at_position(...)`.

## Rules
- Use current document AST and source.
- Preserve existing exact/fallback safety posture.
- Do not claim new receiver promotions in this cutover step.

## Track boundary
Track C extraction and classification only.
