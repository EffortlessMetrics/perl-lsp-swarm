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

Rebasing is ordinary integration work when it solves a concrete problem. Its
main accepted use is resolving an actual merge conflict in the candidate lane;
refresh only conflict-affected proof and review afterward. The lane owner may
also rebase when a base refresh materially simplifies active work or reduces a
concrete integration risk. There is no one-rebase limit, but do not keep a PR at
exact head or rebase after every unrelated merge merely to retrigger CI; commit
distance alone is not a finding.

Do not describe hosted CI as an "exact-head proof authority." Name the affected
hosted checks and the commit they actually evaluated. A status attaches to the
commit it ran on; that fact does not create a branch-refresh, empty-commit, or
full-CI replay rule for unaffected proof.

When the author has no local Rust toolchain, mark the PR hosted-only in
**Hosted proof** rather than burying "Local Rust proof NOT RUN" in prose.
Reviewers: missing proof (no local result and no named hosted check) is not
broken proof (a named check failed, cancelled after start, or is
instrument-failed). Hosted-only means wait on the named hosted checks; do not
file cargo fmt/clippy/test as BLOCKING solely because local proof was not run.

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

## Hosted proof
<!-- Named GitHub Actions checks that exercise this claim, and the commit they
actually evaluated. Fill this section whenever local Rust proof was not run.
Do not call hosted CI an "exact-head proof authority."

Author mark: check Hosted-only when this lane has no local Rust toolchain so
reviewers treat cargo-not-run as wait-for-hosted-log, not as a candidate defect.

Proof states (pick one per named check; missing proof is not broken proof;
cancelled and instrument-failed are not broken):
- run-and-pass: the named check passed on the named commit
- hosted-only pending: author marked hosted-only; hosted checks have not reached a terminal result
- missing: no named hosted check exists yet for this claim
- broken: the named check failed with a candidate-attributable result
- cancelled / no verdict: the named check was cancelled or never completed; wait for a current run
- instrument-failed / NOT_PROVEN: the instrument could not produce a reliable result; not a code defect
- skipped-by-policy: name why; do not report as passed
-->
- [ ] Hosted-only: no local Rust toolchain; named hosted checks are the first-class proof
- Named hosted checks:
- Commit SHA evaluated:
- Check run URL(s):
- Result per check: run-and-pass / hosted-only pending / missing / broken / cancelled / instrument-failed / skipped-by-policy / NOT_PROVEN
- Why local proof was not run (hosted-only only):

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
- [ ] `cargo fmt --all -- --check` — clean, N/A, or hosted-only (named under **Hosted proof**).
- [ ] Affected Clippy/test commands are listed under **Proof**, or hosted-only names the hosted checks that replace them.
- [ ] Hosted-only PRs check the hosted-only box and fill **Hosted proof**.
- [ ] UX-visible errors are actionable and the applicable UX/native proof is listed.
- [ ] Required generated artifacts/contracts are current.

### Reviewer proof classification
<!-- Reviewers classify proof before treating local-not-run as blocking. Missing proof is not broken proof. -->
- [ ] Proof state: run-and-pass / hosted-only / missing / broken
- [ ] "Local Rust proof NOT RUN" is not BLOCKING when **Hosted proof** names the hosted checks and the commit they evaluated.
- [ ] Missing proof (no local result and no named hosted check) is distinct from broken proof (a named check failed with a candidate-attributable result). Cancelled and instrument-failed runs are not broken.
- [ ] Hosted-only PRs wait on the named hosted checks; do not file cargo fmt/clippy/test as BLOCKING solely because the author lacked a local toolchain.

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
