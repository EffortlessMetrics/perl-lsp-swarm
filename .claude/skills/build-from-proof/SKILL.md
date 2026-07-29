---
name: build-from-proof
description: Implement the current reviewed plan and proof through one candidate writer without widening the claim or creating duplicate authority.
user-invocable: false
---

# Build from proof

Confirm the issue, proof, current candidate branch/worktree and writer, semantic owner, consumers, and whether an equivalent current PR already implements the same claim. Do not inspect sibling lanes or touched-file overlap as a routine ownership check.

Implement through existing owners and production seams. Make reasonable documented decisions and proceed.

## Routes

- `IMPLEMENTATION_COMPLETE` → `improve-test-suite`
- `PROOF_INADEQUATE` → `prepare-proof`
- `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → stop for the same-candidate mechanical hazard
- `NOT_PROVEN` → preserve the missing authority or instrument
