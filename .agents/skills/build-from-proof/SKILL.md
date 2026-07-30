---
name: build-from-proof
description: Explicit atomic skill for implementing the current reviewed plan and proof through one candidate writer without widening the accepted claim or creating duplicate authority.
---

# Build from proof

Implement the smallest coherent candidate satisfying the current plan and proof.

Before mutation, confirm:

- controlling issue and claim;
- current proof and limitations;
- current candidate branch/worktree identity and one-writer ownership;
- current semantic owner and intended consumers;
- no equivalent current PR already implements the same claim.

Do not scan sibling worktrees, touched-file overlap, nearby symbols, or unrelated PRs as a routine ownership check.

During implementation:

- use existing owners and public seams;
- avoid duplicate validators, registries, caches, and state machines;
- keep production wiring within the claim;
- preserve explicit unsupported or dynamic boundaries;
- make reasonable documented implementation decisions and proceed.

## Routes

- `IMPLEMENTATION_COMPLETE` → `$improve-test-suite`
- `PROOF_INADEQUATE` → `$prepare-proof`
- `MATERIAL_PREMISE_CHANGED` → `$prepare-issue`
- `WRITER_COLLISION` / `UNSAFE_WORKTREE` → stop and resolve the same-candidate mechanical hazard
- `NOT_PROVEN` → preserve the missing authority or instrument
