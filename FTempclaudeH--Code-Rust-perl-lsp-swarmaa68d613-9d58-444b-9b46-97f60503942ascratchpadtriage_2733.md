<!-- research-verify-triage run_id:2026-07-11-haiku-verify issue:2733 mode:read-only-research -->

## Current state

**Issue is OPEN and blocked on stale corpus baseline.** The issue cites CPAN parser errors from `.ci/cpan-corpus-baseline.json` at commit `fb9484be` (2026-04-09). That baseline **has not been regenerated since the issue was created (2026-06-25)** and is now 2.5+ months behind HEAD. The system-Perl baseline (`.ci/parser-corpus-baseline.json`, updated 2026-05-18 at commit `f201b498c`) shows zero error buckets and is current.

**Evidence:**
- CPAN baseline timestamp: 2026-04-09T21:41:13Z (line 4 of `.ci/cpan-corpus-baseline.json`)
- CPAN baseline error buckets confirmed present (lines 26, 33, 40):
  - `"unclosed_angle": 2` 
  - `"unexpected_arrow_expr": 2`
  - `"unexpected_semicolon_expr": 2`
- System-Perl baseline timestamp: 2026-05-18T20:28:21Z (line 4 of `.ci/parser-corpus-baseline.json`)
- System-Perl `first_error_buckets: {}` (line 24) — zero errors across 7047/7095 files
- Git log: 534 commits to parser crates since 2026-04-09 (`git log --oneline --since="2026-04-09" -- crates/perl-parser/ crates/perl-parser-core/ crates/perl-lexer/`)
- Status doc: `.ci/cpan-corpus-baseline.json` marked "baseline `2026-04-09`" and "insufficient_data" for recovery-only, ERROR-node, catastrophic counts (line 11 of `docs/project/status/parser.md`)

## Claim check

| Claim | Source | Status | Notes |
|-------|--------|--------|-------|
| 4 CPAN corpus files trigger parser errors (unclosed_angle: 2, unexpected_arrow_expr: 2, unexpected_semicolon_expr: 2) | Issue body + `.ci/cpan-corpus-baseline.json` | **CONFIRMED** (in stale baseline only) | Baseline from 2026-04-09; unknown if still accurate after 534+ parser commits. Ratchet enforces only increases, so count may have decreased undetected. |
| System-Perl baseline shows zero errors | `.ci/parser-corpus-baseline.json` (2026-05-18) | **CONFIRMED** | `first_error_buckets: {}`, `files_with_errors: 0`. System-Perl side of these errors is resolved. |
| "The diagnosis and root-cause hypotheses are plausible and well-structured" | Issue body + Comment #1 | **CONFIRMED** | Comment #1 (2026-06-26) verified the acceptance criteria as sound; weakness is staleness, not plan design. |
| "Recommend refresh CPAN baseline before proceeding" | Comment #1 (2026-06-26) | **CONFIRMED** | Sound recommendation; no follow-up action taken (no fresh sweep run since). |

## Scope & blocking issue

**Root issue:** CPAN corpus baseline is 2.5+ months stale (2026-04-09 → now 2026-07-11). Parser has had 534 commits that could have:
- Fixed these error buckets (most likely — prior comment mentions parser-recovery work, e.g., #1698, #3435)
- Moved errors to different buckets
- Created new errors

**To unblock:** Either:
1. **Run a fresh CPAN corpus sweep** (`just cpan-corpus-sweep` or `cargo xtask cpan-corpus sweep`) to confirm current error status, OR
2. **Batch-refresh all CPAN-baseline-dependent issues** (#2708, #2712, #2714, #2718, #2721, #2724, #2727, #2730, #2733, #2735) in one meta-issue (mentioned in Comment #1 as preferred approach), OR
3. **Close as completed if CPAN errors are within system-Perl's resolved set** (likely but unproven without fresh sweep)

## Next-state triage

**Verdict: `needs-decision` + blocker**

Reason: The issue is correctly framed and the plan is sound (per Comment #1 verification). However, the **blocking precondition (fresh CPAN corpus sweep) remains unmet 16 days after discovery.** The system-Perl side of these errors appears resolved, but CPAN status is unknown.

**Recommended path forward (one of):**
- **Path A (fastest):** Orchestrator runs a single CPAN re-baseline pass; triage all ~10 related corpus-bucket issues against refreshed numbers in one pass. This is the recommended approach from Comment #1.
- **Path B (isolated but slower):** A builder picks up this issue, runs `just cpan-corpus-sweep` locally, confirms/refutes current error count, then either closes as completed or proceeds with fixes.
- **Path C (not recommended):** Close as completed based on system-Perl resolution alone, accepting that CPAN status remains unaudited.

**Not actionable as-is for a builder** because the cited error count (2026-04-09) is stale by design (ratchet enforces only increases, so builders cannot tell if count decreased); running `just cpan-corpus-sweep` locally becomes a mandatory first step, duplicating the analysis Comment #1 already recommended.
