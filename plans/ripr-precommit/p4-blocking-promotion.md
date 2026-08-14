# P4 — promote exact staged RIPR to blocking commit policy

Issue: #9116  
Parent: #9112  
Depends on: #9115 / PR #9123  
Branch: `agent/ripr-staged-blocking`

## End goal

Promote the already-proven staged RIPR evaluator from shadow evidence to authoritative commit policy. This PR must not redesign the evaluator: P2 owns the exact subject and P3 owns its falsifiers/cache/performance proof. P4 should be a small policy transition that maps complete normalized evidence to `PASS`/`BLOCKED` and all required-but-unproven states to `NOT_PROVEN`, through the existing `cargo xtask precommit` authority.

## Start condition

Do not implement the blocking switch until #9115 records an explicit promotion-ready disposition covering:

- same-subject staged/committed parity;
- unstaged/index mutation falsifiers;
- delete/rename/test-only/config/suppression cases;
- exact cache identity;
- warm p95 / hard-ceiling acceptance.

If that evidence is absent, keep this PR draft and blocked.

## Required decision semantics

```text
PASS / NOT_APPLICABLE
  complete trusted analysis finds no blocking new actionable gap,
  or no RIPR-relevant staged subject exists

BLOCKED
  complete trusted analysis finds >=1 unsuppressed actionable new gap
  in the exact frozen staged candidate

NOT_PROVEN
  the required claim cannot be established because the tool, version,
  subject, sandbox, output, timeout, completeness, or policy state is invalid,
  unavailable, partial, or otherwise untrustworthy
```

Once this PR lands, required RIPR `NOT_PROVEN` is non-zero. A warning followed by a successful commit is not an allowed representation.

## Codex implementation order

1. Consume the existing P2/P3 result type rather than parsing RIPR again.
2. Add the smallest gate-policy mapping needed to produce the three postures above.
3. Route it through the canonical commit check selected by `cargo xtask precommit`; do not add a dedicated RIPR hook or shell launcher.
4. Keep applicability cheap: docs-only/unrelated staged trees must not spawn RIPR.
5. Preserve `mode=draft` plus unchanged-test semantics and the exact P3 cache contract.
6. Ensure missing/wrong-version/broken producer, malformed output, timeout, and any producer-declared incomplete/ineligible state become `NOT_PROVEN`.
7. Produce coach-style feedback using the existing guidance vocabulary and narrow rerun mechanism.
8. Update precommit/RIPR docs to say staged RIPR is now authoritative while the dedicated CI lane remains temporarily in overlap through P5.

## Coach packet

For `BLOCKED`, include:

```text
result class / check id
why it ran
candidate tree identity
affected staged paths and canonical gap ids
why the result matters
bounded repair route
narrow rerun
what pre-push/CI still prove
```

For `NOT_PROVEN`, distinguish setup/instrument failure from a product finding and name the explicit repair/setup route. Never install automatically.

## Mandatory controls

- weak-oracle staged gap blocks;
- adding the focused staged test allows the exact candidate to pass when evidence supports it;
- unstaged fix alone does not pass;
- missing binary fails as `NOT_PROVEN`;
- wrong version fails as `NOT_PROVEN`;
- timeout fails as `NOT_PROVEN`;
- malformed output fails as `NOT_PROVEN`;
- docs-only staged change is a no-process positive no-op;
- valid suppression applies only under the exact staged policy identity;
- deleted-side 0.10 false target remains contained;
- test-harness-only 0.10 false target remains contained by current narrow policy;
- `git commit --no-verify` remains documented as bypass, not proof.

## Guardrails

- Keep `.github/workflows/ripr.yml` present and required in this PR.
- No live branch-protection/ruleset mutation here.
- No 0.11 bump.
- No removal of `HeadLineExtents` or existing producer workarounds.
- No compile/Clippy/test expansion of commit tier.
- No second hook authority.

## Acceptance before merge

- canonical precommit blocks real staged actionable new gaps;
- all required producer/input/tool failures fail as `NOT_PROVEN`;
- docs-only/unrelated commits remain cheap;
- cached decisions preserve exact identity and posture;
- hook generation/installation still routes through one repository authority;
- current required RIPR CI still runs independently after this PR.

## Suggested review map

Review only the policy transition and operator feedback relative to the proven P3 evaluator. A large rewrite of sandbox, normalization, or cache logic in P4 is a scope warning and should normally move back to its owning predecessor.
