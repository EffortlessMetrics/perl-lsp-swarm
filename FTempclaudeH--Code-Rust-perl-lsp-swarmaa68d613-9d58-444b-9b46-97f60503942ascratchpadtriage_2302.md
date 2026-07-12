<!-- research-verifier triage pass: 2026-07-11 -->

## Current state

**Status**: OPEN duplicate of #2875 (canonical, lower number, OPEN).

**Feature**: Consolidated `cargo xtask orchestrator-reconcile --dry-run|--apply` command bundling four hygiene checks (stale PRs, worktree accumulation, issue-PR state drift, branch cleanup).

**Implementation**: NOT IMPLEMENTED as of origin/main HEAD `ec6148cc2` (2026-07-11).

### Ground truth (verified via git + gh)

- **Consolidated command**: Zero hits for `orchestrator-reconcile` / `orchestrator_reconcile` in xtask codebase (`xtask/src/main.rs`, `xtask/src/tasks/*.rs`).
- **Partial primitives only**:
  - `cargo xtask worktree-cleanup` — `xtask/src/tasks/worktrees.rs:cleanup()` prunes stale `.claude/worktrees` entries ✓
  - `cargo xtask merge-ready reconcile-queue` — `xtask/src/tasks/queue_reconciler.rs` resolves label contradictions using live CI state ✓
- **Missing**: stale-PR idle detection (7+ days, CONFLICTING base), issue-PR drift resets (`in-build` orphans), >30-day branch cleanup, umbrella `orchestrator-reconcile` command itself.
- **Last commit touching these areas**: 2026-07-04 or earlier; zero commits on `orchestrator_reconcile` since issue filed.

### Related triage work

- **Issue #2875** (canonical): Harvest review posted 2026-06-26 (`EffortlessSteven`) confirms same state, notes size/M, flags potential shape mismatch (xtask subcommand vs. skill in `.claude/skills/` per parent #2883).

## Triage verdict

**Recommendation: Close #2302 as duplicate of #2875.**

Rationale:
1. Identical scope (same four checks, same invocation contract, same feature request).
2. #2875 is lower-numbered and canonical (OPEN).
3. Last research comment on #2302 (2026-07-04, `EffortlessSteven`) already recommends this closure.
4. Harvest review on #2875 (2026-06-26) provides deeper context: partial primitives + caveat about xtask-vs-skill shape mismatch awaiting maintainer confirmation.
5. No implementation progress in 7-day window (last comment → today).

**Next state**: CLOSE as duplicate of #2875. Consolidate future work on #2875 (which has harvest review + shape-mismatch caveat).

---

*Research pass: Haiku verifier, external-truth-gate read-only. Verified against origin/main HEAD `ec6148cc2` (2026-07-11), gh issue queries, and grep of xtask source.*
