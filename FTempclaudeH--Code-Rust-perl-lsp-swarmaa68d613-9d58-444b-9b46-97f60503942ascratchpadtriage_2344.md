<!-- triage-research run_id:2026-07-11-haiku-verify issue:2344 mode:research -->

## Current state

**OPEN but prerequisite closed.** Issue #2344 is open with labels `size/L`, `needs-assignee`, `ci`.

**Status of linked work:**
- Parent: #2918 (scoped merge-gate) = CLOSED on 2026-07-01
- Prerequisite for: #2915 (mutation real-gate) = CLOSED as NOT_PLANNED on 2026-07-01
- Migrated from: #2912 = CLOSED on 2026-07-01

**Implementation status on origin/main:**
- Feature NOT implemented: no `.ci/mutation/baseline/` directory exists
- Mutation testing IS active (nightly lane, non-blocking, 87% mutation score per `docs/project/QUALITY_INFRASTRUCTURE.md`)
- Baseline management system (per-crate known survivors tracking, post-merge update jobs, PR-time enforcement) is NOT present

## Claim check

✓ **CONFIRMED**: The `.ci/mutation/baseline/<crate>.json` baseline infrastructure described in the issue does not exist on origin/main (verified via `git ls-tree origin/main -- .ci`, no `mutation/baseline/` directory found).

✓ **CONFIRMED**: The current mutation testing is non-blocking (lines 91-92 of QUALITY_INFRASTRUCTURE.md: "Results are non-blocking but tracked for trend analysis").

✓ **CONFIRMED**: Prerequisite issue #2915 (mutation real-gate, which this feature unblocks) was closed as NOT_PLANNED on 2026-07-01 at 07:03:46Z.

## Triage verdict

**needs-decision**

**Reason:**
Issue #2344's stated prerequisite (#2915) was closed as NOT_PLANNED just before the last research pass (2026-07-04). The prior research comment correctly noted the closure but recommended closing #2344 as a duplicate of #2912, which is not the actual state: #2344 is the active canonical (migrated from #2912), which is now closed.

The real decision is: **Should #2344 remain open as future infrastructure, or close as won't-do following #2915's explicit NOT_PLANNED closure?**

- If mutation baseline enforcement is planned later, this remains open.
- If NOT_PLANNED means mutation enforcement is deferred indefinitely, this should close as won't-do (not as a duplicate).

The issue needs a comment from maintainers / the orchestrator clarifying the stance: keep open for later, or close as deferred. Current state (open + no assignee + prerequisite not planned) is a decision point, not a blocker state.

## Next step

Post this triage; route to orchestrator for disposition (keep-open-or-close decision). The feature is not broken (it doesn't exist), and the prerequisite closure is recent (2026-07-01), so no urgent fix required.

---

**Files checked:**
- Origin/main tree: `.ci/` structure
- `docs/project/QUALITY_INFRASTRUCTURE.md` (mutation testing doctrine)
- Issue bodies: #2344, #2915 (CLOSED NOT_PLANNED), #2918 (CLOSED), #2912 (CLOSED)
