<!--
PR title convention: end with a real issue ref, e.g.
  fix(crate): description (#NNNN)

Replace NNNN with the tracking issue number this PR addresses.
The validate-title CI check enforces this format — placeholder refs
like (#0000) or (#9999) will fail CI.

A PR owns one coherent acceptance-and-rollback claim. Complete the sections
that apply to that claim. Use "N/A" where a repository-specific proof surface
genuinely does not apply; do not fabricate evidence.

Reviewers, before asking for a rebuild: this repository squash-merges. A
conflict-free candidate whose base is behind main needs no rebase, branch
update, empty commit, CI replay, or review refresh. "Behind by N" and "targets
<sha> while live main is <sha>" are not findings. Only a real conflict or an
actual combined-tree failure changes that.

At final merge handoff, the lane owner may choose one late rebase when the
candidate is many commits behind and refreshing once is cheaper or safer. That
is an optional one-time decision, not a rule to keep every PR at zero distance.

Do not describe hosted CI as an "exact-head proof authority." Name the affected
hosted checks and the commit they actually evaluated. A status attaches to the
commit it ran on; that fact does not create a branch-refresh, empty-commit, or
full-CI replay rule for unaffected proof.

Attribute a failing check before treating it as a candidate defect, and note that
this cuts both ways. A cancelled run reached no verdict; a run that genuinely
failed stays a finding until the seam it names actually changed, so a later
unrelated push does not clear it. Blaming the base takes the same gate and the
same failure signature observed at this PR's merge base, not at current main.

The branch keeps one writer until the claim lands. Request changes; do not push
to someone else's candidate.

See docs/agents/REVIEW_CURRENTNESS.md.
-->

## Claim
<!-- One sentence: what becomes true if this candidate is accepted? -->

## Controlling issue
<!-- Fixes/Closes/Relates to #NNN. Link any umbrella separately. -->

## Governing contract
<!-- Spec, ADR, policy, accepted issue plan, invariant, or N/A for a bounded repair. -->

## Changed production path
<!-- Trace the real user/protocol/runtime route to the changed behavior. Use N/A only when there is no production-path subject. -->

## Proof
<!-- Exact focused commands, tests, fixtures, external oracle, and observed results. Distinguish pass/fail/not-run/NOT_PROVEN. -->

## Test hardening
<!-- What realistic wrong implementation was challenged? What negative, stale, failure, recovery, or opposite-direction control was added or already existed? -->

## Simplification
<!-- What duplicate authority, scaffolding, overbroad API, repeated validation, or dead compatibility was removed? "Already minimal" is valid. -->

## Deviations
<!-- Material differences from the issue plan/contract and why. Link corrected issue/spec state where applicable. -->

## Claim Boundary
<!-- Keep the conclusion inside the proof boundary. What becomes provably true, and what is explicitly out of scope? -->

## Non-goals
<!-- Explicit non-goals, unrun proof, unsupported cases, and remaining work. -->

## Risk and rollback
<!-- Failure modes, compatibility/support effects, and how this coherent claim is reverted or disabled. -->

## Review index
<!-- Point reviewers to the issue synthesis, governing contract, key production seams, proof, generated artifacts, and high-risk files. -->

---

## Repository classification

### Lane
<!-- Retained for current CI/review routing. Pick one. See docs/swarm/review-rules.md. -->
- [ ] trust
- [ ] substrate
- [ ] reliability

### Behavior
- [ ] no behavior change
- [ ] preview only
- [ ] scoped pilot
- [ ] live behavior change

### Risk surfaces
- [ ] edit-producing
- [ ] provider behavior
- [ ] subprocess
- [ ] path/module resolution
- [ ] public API
- [ ] parser/lexer core

### Promotion discipline
<!-- Required for trust-lane PRs. Write N/A for substrate or reliability PRs. -->
- Surface:
- Fact class:
- Promotion rule:
- Fallback rule:
- Blocker rule:
- Receipt:

## Verification

- [ ] Lane and applicable risk surfaces are declared above.
- [ ] Trust-lane PRs name promotion, fallback, blocker, and receipt boundaries.
- [ ] I ran the cheapest discriminating proof first.
- [ ] Focused and affected proof covers the candidate's changed semantic subjects; unaffected completed proof remains usable.
- [ ] `cargo fmt --all -- --check` — clean or N/A.
- [ ] Affected Clippy/test commands are listed under **Proof**.
- [ ] UX-visible errors are actionable and the applicable UX/native proof is listed.
- [ ] Required generated artifacts/contracts are current.

## Quality-gate effect
<!-- Complete when this touches proof-gated code, receipts, coverage/RIPR policy, CI, or test evidence. Otherwise N/A. -->
- new RIPR gaps:
- total RIPR+ gaps:
- patch coverage:
- project coverage:
- receipt freshness:
- exception status:
- local verify command:
- receipt command:

## Retained state
<!-- Complete when this changes a long-lived map, cache, queue, background task, session holder, or subprocess lifecycle. Otherwise N/A. -->
- Owner / key / bound:
- Cleanup or invalidation event:
- Key normalization:
- Close versus delete/folder-removal behavior:
- Protection against stale background repopulation:
- Regression proof / receipt / debug counter:

## CI cost / verification note
<!-- See docs/ci/cost-and-verification-policy.md and docs/ci/lem-budgeting.md. -->
- [ ] Broad CI was requested only when the claim/risk requires it.
- [ ] Any high-cost CI label is explained.
- [ ] New CI work names the failure mode it catches and estimated LEM.

## Remaining work
<!-- Linked residual claims, follow-ups, or N/A. -->
