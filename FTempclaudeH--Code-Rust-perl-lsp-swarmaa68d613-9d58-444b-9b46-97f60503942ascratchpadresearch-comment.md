## Current State (origin/main)

**Workflows and label writers:**
- `.github/workflows/pipeline-labels.yml` (line 1–184): owns `needs-deep-review`, `in-review`, `merge-ready`; uses `issues` + `pull_request` events (standard `GITHUB_TOKEN`); mutates labels via `actions/github-script` with no upstream status publisher
- `.github/workflows/pr-title-check.yml` (line 1–190): owns `needs-issue-link`; uses `pull_request_target` event; mutates labels via `actions/github-script` with no upstream canonical status publisher (calls `core.setFailed()` only)
- `.github/workflows/needs-label-gate.yml` (merged via #3754): evaluates `needs-*` labels and posts commit-status verdict; triggered by `pull_request` + `workflow_run` (watches both label writers); posts to resolved PR head SHA via `createCommitStatus` API

**Assertion boundary:**
The `needs-label-gate` status is **reactive and eventually-consistent** — it posts *after* a mutation completes. No explicit "pending" state exists *before* mutation; no serialization boundary guards against concurrent label-writer races.

---

## Claim Check

| Claim | Verdict | Evidence |
|-------|---------|----------|
| GITHUB_TOKEN is READ-ONLY for fork PRs | **CONFIRMED** | [GitHub Docs: Controlling permissions for GITHUB_TOKEN](https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/controlling-permissions-for-github_token) — "When a workflow is triggered from a forked repository, the GITHUB_TOKEN is set to read-only"; fork PRs cannot write commit statuses without explicit admin opt-in |
| Dependabot-triggered workflows receive read-only GITHUB_TOKEN by default | **CONFIRMED** | [GitHub Changelog 2021-02-18](https://github.blog/changelog/2021-02-18-github-actions-workflows-triggered-by-dependabot-prs-will-run-with-read-only-permissions/) + [GitHub Docs: Dependabot on GitHub Actions](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-on-actions) — workflows triggered by Dependabot run with read-only permissions for security; secrets also unavailable |
| `pull_request_target` event with base-repo context but security risk if code is checked out | **CONFIRMED** | [GitHub Security Lab: Preventing pwn requests](https://securitylab.github.com/resources/github-actions-preventing-pwn-requests/) + [Securely using pull_request_target](https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target) — `pull_request_target` grants base-repo GITHUB_TOKEN but executing untrusted PR code is a known RCE vector; safe only if PR checkout is avoided or base-only ref is used |
| Metadata-only `pull_request_target` (no code checkout) is fork-safe for status posting | **CONFIRMED** | GitHub security guidance documents confirm workflows triggered by `pull_request_target` run with base-repo write permissions *without executing PR code* — ideal for fork-safe status publishing |

---

## Scope + Non-Goals

**What the issue asks for:**
1. **Canonical status publisher** — one reusable component (composite action or reusable workflow) that all label-writing workflows invoke, not per-workflow helpers
2. **Atomic sequence:** set pending → mutate labels → fetch current live state → post final verdict
3. **Serialization boundary:** prevent races between `pipeline-labels.yml` and `pr-title-check.yml` concurrent mutations
4. **Fork/Dependabot safety:** fork/Dependabot PRs have READ-ONLY token on `pull_request` events; publisher must use `pull_request_target` event or GitHub App to write status to fork/Dependabot PR heads
5. **Crash reconciliation:** keep `workflow_run` as a backstop for workflows that exit early
6. **Adversarial tests:** concurrent add/remove, push during mutation, failure mid-sequence
7. **Methodology gate:** prevent silent coverage loss if future workflows add `needs-*` labels without invoking the publisher

**What this issue does NOT cover:**
- The HUMAN-label interval (person adds/removes label without commit SHA change) relies additionally on review-convergence + auto-merge discipline; cannot be made fully atomic with status alone per the issue's own claim boundary
- Changing which `needs-*` labels exist or their semantics
- Modifying the two existing required checks or branch-protection ruleset (that's admin work)

---

## Scope + Plan (Condensed)

**Exact surfaces to build:**
1. New composite action (`.github/actions/needs-label-gate-publisher/action.yml` or equivalent) that:
   - Accepts: `pr_number`, `labels_to_check` (array), `on_success_verdict`, `on_failure_verdict`
   - Sets pending status on the resolved PR head SHA
   - Caller does the mutation (add/remove labels)
   - Action re-fetches PR + labels (fetches live state, not cached from event payload)
   - Posts final success/failure status to the exact head SHA
   - Handles both `pull_request` and `workflow_run` trigger shapes

2. Update `pipeline-labels.yml` + `pr-title-check.yml`:
   - Before label mutations: call publisher with pending state
   - After mutations: call publisher with final verdict
   - For `pr-title-check.yml` specifically: use `pull_request_target` (already does) to ensure fork/Dependabot PRs can post status

3. Add methodology check to `.ci/policies/workflow-policy-lint`:
   - Flag any job that calls GitHub API label-write (`issues.addLabels` / `issues.removeLabel`) without invoking the canonical publisher
   - Prevents future label writers from accidentally bypassing the serialization boundary

4. Scratch-proof (test on a real PR before making the check required):
   - Label add/remove under human control
   - Concurrent bot-authored via both workflows
   - Push while mid-mutation (synchronize during pending state)
   - Fork PR (verifies `pull_request_target` path works)
   - Dependabot PR (verifies token permissions and status posting)

**Non-goals in this issue:** building reviewer-capability-read-only enforcement (M4b, separate); changing semantics of existing labels; admin-level branch-protection changes

---

## Next State + Triage Verdict

**Triage verdict:** `needs-decision`

**Rationale:**
- All external factual claims (GitHub platform behavior, fork/Dependabot token restrictions, `pull_request_target` safety) are **CONFIRMED** and accurate
- Current code state is as described: reactive gate without pre-mutation pending state
- Scope is clear and well-bounded per issue + maintainer comment
- **Blocking decision needed:** choice between implementation approaches
  - **Option A:** composite action (simpler, composable, but repeated invocation in two workflows)
  - **Option B:** reusable workflow (auto-retrigger via `workflow_run` for backstop, but more indirect)
  - **Option C:** GitHub App (most fork-safe, but requires app deployment outside this PR)
- The maintainer's 2026-07-11 comment adds hard requirements (shared serialization, adversarial tests, fork/Dependabot proof) that must be built before making `needs-label-gate` required
- This is a **specification review** that belongs with the maintainer or plan-reviewer before a builder lands code

**Ready for:** Plan/design review (Sonnet-grade) to choose implementation approach + approve the spec before building
