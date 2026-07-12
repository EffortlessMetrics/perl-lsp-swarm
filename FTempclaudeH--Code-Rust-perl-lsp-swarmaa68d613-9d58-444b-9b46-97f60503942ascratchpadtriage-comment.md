## Triage Research — Fact-Check Pass (2026-07-11, entry-independent verification)

### Current State
This is a **program-state board** tracking control-plane enablement (epic #3612). Recent updates posted 2026-07-11T01:40:59Z and 2026-07-11T06:43:47Z are thorough. Verification against live `origin/main` + GitHub API:

### Claim Check — Milestone Merge Status

**M3 deterministic selector (#3726)**: 
- Claim: "MERGED 2026-07-11 (`dc260f325`)"
- Verified: ✅ CONFIRMED — `gh pr view 3726` shows MERGED at commit `dc260f325` on 2026-07-11T06:37:51Z

**M5 first increment (#3798/#3749)**:
- Claim: "MERGED (`645d1c6b5`, #3798/#3749)"
- Verified: ⚠️ PARTIALLY CORRECT — #3798 merged (CONFIRMED), BUT #3749 is an **OPEN ISSUE** (not merged PR). PR #3798 fixes issue #3749; the claim's grammar conflates them. Citation: `gh issue view 3749` → state=OPEN, title="tooling(worktree-manager): slot re-allocation branches off stale local main..."

**M4 supporting PRs (#3827, #3815, #3808)**:
- Claims: All merged on specified dates
- Verified: ✅ ALL CONFIRMED — #3827 merged 2026-07-11T11:00:39Z, #3815 merged 2026-07-11T09:09:26Z, #3808 merged 2026-07-11T08:23:55Z

### Claim Check — Branch Protection Protocol

**Claim**: "the `main` ruleset (16664791) has NO required status checks" (M4a, line about Admin target)
- Verified: ❌ REFUTED — GitHub API returns 2 required status checks: (1) "Perl LSP Rust Small Result" (app_id 15368), (2) "ripr+ New Gap Gate" (app_id 15368). Corroborated by `.ci/policies/required-checks.toml` (both marked required=true, enforcement="github-branch-protection").
- **Contradiction noted**: Same M4a paragraph then says "preserving the 2 existing checks + strictness" — the ruleset DOES have 2 checks.

### Scope & Plan

The issue is durable/rank-2 authority (per its own note). Existing owner updates:
- ✅ M3 selector trust verified (entry-independence dry-run, 2026-07-11T01:40:59Z)
- ✅ M5 worktree isolation first increment merged (2026-07-11T06:43:47Z)  
- ✅ M4b publication boundary reframed + core live-proven (#3808, #3815, #3827 all merged)

**§6 in-flight (VOLATILE)**: As warned, §6 is stale:
- #3738 listed as "landing on CI" but now MERGED (2026-07-11)
- #3726 listed as "held" but now MERGED (2026-07-11T06:37:51Z, after #3701 merged)

This is expected volatility in a live tracking board. Update §6 by verifying against `gh pr list --state open` as advised.

### Next-State Triage

**Verdict**: `builder-ready` for #3751 (M4 capability enforcement packet) — most external facts verified; #3798/#3749 reference needs rewording; branch-protection claim needs correction. This board's role is internal state tracking (not a spec needing external-truth gate review per #3118), so minor factual drift is acceptable if kept current via periodic syncs.

**Recommendation**: Update issue body to clarify M5 claim as "M5 first increment MERGED via #3798 (fixing #3749)" and correct the M4a branch-protection claim to reflect the actual 2 required checks now live. Both are already-merged realities, just description cleanup.
