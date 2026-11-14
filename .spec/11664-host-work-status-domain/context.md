# Context — typed host work status domain (#11664)

Parent/controller: #11606 · Local-integrity authority: #3957 · Read-only worktree
provider: #10256 · Writer-admission facts: #11617 · Executor facts: #11650/#11653/
#11659/#11661.

## Claim

WIP-01 train position: one pure typed host-status domain plus exhaustive provider
adapters over currently landed typed observations. No live observation, no mutation,
no cleanup planning/apply, no scheduler.

## Current vocabulary inventory (refreshed on origin/main @ 0884f3729, 2026-08-23)

| Existing surface | Closed vocabulary | Mapped into |
| --- | --- | --- |
| `xtask/src/worktree_cleanup/model.rs` (#10256/#10263) | `ObservationState{OBSERVED,NOT_APPLICABLE,NOT_PROVEN}`; `WorktreeClassification{KEEP,CACHE_ONLY,SALVAGE,REVIEW,NOT_PROVEN}`; `PrMatch{NONE,MATCH}`; `WorktreeActionKind{REMOVE_REGISTERED_WORKTREE,PRUNE_ADMINISTRATIVE_RECORD}`; `RepositorySubject`; `PlanSummary` counts | mutation rows (`worktree_plan` adapter), logical rows (`PrMatch`), storage disposition rows |
| `xtask/src/tasks/writer_admission.rs` (#3957 W1/W2, #11617 family) | `PASS/BLOCK/NOT_PROVEN` verdict; per-check results incl. `writer-collision`, `disk-capacity`, `dirty-unpushed`, `branch-worktree-mapping`; guidance resume/reuse; `branch_pr_status open/none/unknown`; `FLOOR_GB`/`FLOOR_PCT` floor convention | logical + mutation + storage rows (`admission_report` adapter) |
| `scripts/swarm-doctor --json` | worktree inventory / dirty / disk / divergence shape | covered via the two typed adapters above (doctor prose is not a provider; residue noted below) |
| `xtask/src/actual_host_receipt.rs` | `actual_host_receipt.v1`, `RegistrationState` | subject/provider identity vocabulary only |
| `ci_doctor` / `devex_doctor` | health booleans per toolchain/doc surface | out of scope (tool health ≠ host work); named residue R1 |
| `sync_divergence`, `pre_push_plan`, `queue_health`/`queue_snapshot` | sync/queue shapes | queue shape maps to compute QUEUED concept only as vocabulary, no adapter owner landed yet; residue R2 |

Residues (explicitly named): **R1** doctor toolchain/doc health stays owned by its
existing tasks; it is not a host-work dimension. **R2** executor state/capacity/process
typed providers (#11650/#11653/#11659/#11661) are unlanded; this PR defines the typed
input structs their future owners must produce and marks those providers
`ProviderCoverage::Missing` so absence stays visible.

## Schemas

Four independent dimensions (logical, mutation, compute, storage), each a typed
observation bound to an exact `HostWorkSubject`. One closed lifecycle
(`ACTIVE|QUEUED|STOPPING|REMOTE_IN_FLIGHT|ORPHAN_CANDIDATE|AMBIGUOUS|TERMINAL`), one
closed reason vocabulary, aggregate observation set
(`HEALTHY|SATURATED|LOW_DISK|COLLISION|SALVAGE_REQUIRED|AMBIGUOUS|NOT_PROVEN`),
cleanup-readiness fields separate from authorization. See `acceptance.md` for the truth
table and `checklist.md` for falsifier IDs.
