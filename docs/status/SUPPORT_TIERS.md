# Support tiers

This file maps repository claims to proof commands.

## Tier definitions

| Tier | Meaning |
|---|---|
| Stable | Claim is enforced by required CI proof. |
| Stabilizing | Expected to work, still converging on full enforcement. |
| Experimental | Available without broad support claim. |
| Advisory | Informational; does not block merges. |
| Not supported | Explicitly out of scope. |

## Claim map

| Surface | Tier | Claim | Proof command | Notes |
|---|---|---|---|---|
| Source-of-truth scaffolding | Stabilizing | Artifact taxonomy and templates exist. | `git diff --check` | Validator wiring pending. |
| Doc artifact linkage | Advisory | Artifact ledger links IDs and paths. | `cargo xtask check-doc-artifacts` | Command planned, not yet implemented. |
| Active goal linkage | Advisory | Goal manifest references plan/spec/proposal IDs. | `cargo xtask check-goals` | Command planned, not yet implemented. |
