# PERLLSP-SPEC-0022 — Semantic-Token Capability Contract

## Goal
Align advertised semantic-token capabilities with actually implemented behavior.

## Decision options
1. Implement `textDocument/semanticTokens/full/delta` correctly with resultId/delta cache.
2. De-advertise delta support until full contract exists.

## Track D-first posture
Prefer the smaller safe fix first: de-advertise delta until real delta support is implemented.
