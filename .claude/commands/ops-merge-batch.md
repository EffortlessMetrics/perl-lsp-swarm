---
description: Ops step 2 — squash-merge up to 3 reviewed PRs using expected-head authorization
user-invocable: false
---

# Ops Merge Batch

Merge up to three candidates from `/ops-check-queue`. The batch limit is an
operational throttle, not a reason to update every branch after `main` moves.

Canonical authorities:

- PR disposition: `docs/specs/PLSP-SPEC-0006-pr-queue-disposition.md`
- authority map: `docs/reference/CONTROL_PLANE_AUTHORITY.md`
- review convergence: `scripts/ci/check-pr-review-convergence`
- required checks and merge permission: live GitHub policy

## Steps

### 1. Compare candidate surfaces and order only real dependencies

Before admitting more than one PR to a batch, capture each candidate's complete
changed-file set:

```bash
for pr in <candidate-pr-numbers>; do
  gh api "repos/:owner/:repo/pulls/$pr/files" --paginate \
    --jq '.[].filename' | sort -u >"/tmp/pr-$pr-files.txt"
done

# Run for every pair in the proposed batch.
comm -12 "/tmp/pr-<a>-files.txt" "/tmp/pr-<b>-files.txt"
```

No output means file-disjoint, but semantic/public-API/generated-authority
interaction must still be considered. Any shared file requires an explicit
overlap decision before both PRs enter the batch:

- `sequence-both`: both deltas are valuable and compatible; name the dependency
  or tested integration order;
- `pick-one`/`superseded-with-evidence`: compare the full semantic/test delta and
  preserve unique value before closing anything;
- `review-semantic-interaction`: the order or combined behavior is not yet
  proven, so do not batch them;
- `independent-same-file`: rare; require a reviewed explanation and applicable
  combined-tree proof, then merge one and recollect readiness/integration state
  for the second.

Deterministic order comes from an explicit stack/dependency, authority ownership,
generated-file order, or proved integration sequence. Do **not** choose a winner
or order solely because a PR is older, smaller, newer, or further behind.

Respect generated files, registries, schemas, workflows, release authorities,
and public API collisions even when line-level Git conflict detection is clean.

A prior merge moving `main` does not invalidate an unchanged PR-head review. It
may invalidate a separate integration receipt when that receipt was bound to the
old base. Recollect that second PR's live readiness and applicable integration
proof after the first merge; do not automatically mutate its head.

### 2. Capture one live readiness packet per PR

Immediately before preparing a merge, query the PR head and GitHub's combined
status rollup together:

```bash
PR_NUMBER=<decimal-pr-number>
[[ "$PR_NUMBER" =~ ^[0-9]+$ ]] || { echo "invalid PR number" >&2; exit 2; }
PR_STATE=$(gh pr view "$PR_NUMBER" \
  --json isDraft,mergeable,mergeStateStatus,labels,headRefOid,baseRefName,baseRefOid,reviewRequests,reviewDecision,statusCheckRollup,title)
EXPECTED_HEAD=$(printf '%s' "$PR_STATE" | jq -r '.headRefOid')
printf '%s' "$PR_STATE" | jq -r '
  .statusCheckRollup[] |
  {
    kind: (.__typename // "unknown"),
    name: (.name // .context),
    state: (.conclusion // .state // .status),
    started_at: .startedAt,
    completed_at: .completedAt,
    details_url: (.detailsUrl // .targetUrl)
  }'
scripts/ci/check-pr-review-convergence "$PR_NUMBER"
```

`statusCheckRollup` includes both CheckRun and commit StatusContext entries for
the current PR head. Querying `check-runs` alone can omit required or advisory
contexts published through the commit-status API. Fetch a focused underlying
run/status only when duplicate or terminal evidence needs classification.

> **Connector alternative:** fetch the PR, its combined current-head status
> rollup (or both check-run and commit-status contexts), review threads, and
> requested reviews through the canonical connector map. Keep the packet bound
> to the captured full head SHA.

Required at merge time:

- PR is not draft;
- GitHub reports no actual textual conflict;
- required checks discovered from live policy succeed in the combined rollup on
  the exact head;
- review convergence succeeds for the exact head;
- no active repair request contradicts readiness;
- Changie/release-note disposition and other live policy inputs are satisfied;
- applicable integration proof is current when the risk/policy trigger requires
  it;
- the PR head remains unchanged after evidence collection.

Labels are navigation/projected state, not proof. Advisory checks remain visible
but do not become required merely because they are present or red.

`UNKNOWN` is `NOT_PROVEN`. `UNSTABLE` must be decomposed into required,
advisory, review, policy, or platform state. `DIRTY`/`CONFLICTING` is an actual
conflict to inspect, not an automatic rebase instruction.

### 3. Run the repository pre-merge guard

```bash
just pre-merge-check "$PR_NUMBER"
# or: bash scripts/pre-merge-check.sh "$PR_NUMBER"
```

Treat its result according to the current repository contract. A local tool or
instrument failure is `NOT_PROVEN`; it is not permission to bypass protected
GitHub evidence.

### 4. Prepare a reviewed squash message safely

Do not paste a PR title or body into shell source. Both are contributor-controlled
text and may contain quotes, substitutions, backticks, or newlines.

Create the reviewed summary as data in a temporary file and reject the template
placeholder or an empty/whitespace-only message:

```bash
SUMMARY_FILE=$(mktemp)
PAYLOAD_FILE=$(mktemp)
trap 'rm -f "$SUMMARY_FILE" "$PAYLOAD_FILE"' EXIT

SUMMARY_PLACEHOLDER='<one to three reviewed sentences explaining what changed and why>'
cat >"$SUMMARY_FILE" <<'SUMMARY'
<one to three reviewed sentences explaining what changed and why>
SUMMARY

if grep -Fqx "$SUMMARY_PLACEHOLDER" "$SUMMARY_FILE" ||
   ! grep -q '[^[:space:]]' "$SUMMARY_FILE"; then
  echo "BLOCKED: replace the squash-message placeholder with a reviewed summary" >&2
  exit 2
fi

PR_TITLE=$(printf '%s' "$PR_STATE" | jq -r '.title')
COMMIT_TITLE="$PR_TITLE (#$PR_NUMBER)"

jq -n \
  --arg merge_method "squash" \
  --arg sha "$EXPECTED_HEAD" \
  --arg commit_title "$COMMIT_TITLE" \
  --rawfile commit_message "$SUMMARY_FILE" \
  '{
    merge_method: $merge_method,
    sha: $sha,
    commit_title: $commit_title,
    commit_message: $commit_message
  }' >"$PAYLOAD_FILE"
```

`jq --arg` and `--rawfile` encode untrusted text as JSON data; no PR-controlled
text is evaluated by the shell.

### 5. Recollect the complete readiness packet immediately before merge

Do not reuse Step 2 or Step 3 results after message preparation or other
operator work. Immediately before the irreversible call:

1. query a fresh `PR_STATE` with the same complete field set;
2. require the head still equals `EXPECTED_HEAD`;
3. rerun the full live-policy and combined-rollup procedure from
   `.claude/agents/green-ci.md` and require `GREEN` for that head;
4. rerun `scripts/ci/check-pr-review-convergence "$PR_NUMBER"`;
5. rerun `just pre-merge-check "$PR_NUMBER"`;
6. re-evaluate draft, mergeability, routing labels, Changie/policy, and applicable
   integration evidence from the fresh packet;
7. rebuild `PAYLOAD_FILE` from the reviewed message and unchanged head;
8. re-read `headRefOid` once more immediately before the API call.

```bash
FINAL_STATE=$(gh pr view "$PR_NUMBER" \
  --json isDraft,mergeable,mergeStateStatus,labels,headRefOid,baseRefName,baseRefOid,reviewRequests,reviewDecision,statusCheckRollup,title)
FINAL_HEAD=$(printf '%s' "$FINAL_STATE" | jq -r '.headRefOid')
test "$FINAL_HEAD" = "$EXPECTED_HEAD" || {
  echo "RETURN TO REVIEW: head moved from $EXPECTED_HEAD to $FINAL_HEAD" >&2
  exit 1
}

# Run the complete green-ci live-policy/rollup procedure here and require GREEN.
scripts/ci/check-pr-review-convergence "$PR_NUMBER"
just pre-merge-check "$PR_NUMBER"

# Rebuild the payload after final readiness succeeds.
PR_TITLE=$(printf '%s' "$FINAL_STATE" | jq -r '.title')
COMMIT_TITLE="$PR_TITLE (#$PR_NUMBER)"
jq -n \
  --arg merge_method "squash" \
  --arg sha "$EXPECTED_HEAD" \
  --arg commit_title "$COMMIT_TITLE" \
  --rawfile commit_message "$SUMMARY_FILE" \
  '{merge_method:$merge_method,sha:$sha,commit_title:$commit_title,commit_message:$commit_message}' \
  >"$PAYLOAD_FILE"

CURRENT_HEAD=$(gh pr view "$PR_NUMBER" --json headRefOid --jq .headRefOid)
test "$CURRENT_HEAD" = "$EXPECTED_HEAD" || {
  echo "RETURN TO REVIEW: head moved from $EXPECTED_HEAD to $CURRENT_HEAD" >&2
  exit 1
}
```

If any fresh input is pending, failed, stale, contradictory, or `NOT_PROVEN`, do
not merge. GitHub's endpoint still re-enforces live protections, but that is a
last line of defense, not a substitute for recollecting repository readiness.

### 6. Squash-merge only the expected head

```bash
gh api -X PUT "repos/:owner/:repo/pulls/$PR_NUMBER/merge" \
  --input "$PAYLOAD_FILE"
```

The REST merge endpoint with `merge_method: squash` and `sha: EXPECTED_HEAD`
uses GitHub's ordinary protected merge path. Required checks, review rules,
conversation-resolution rules, and rulesets still apply; the SHA field adds a
compare-and-swap guard and does not bypass protection. Use no admin/bypass
credential or exception path. A 405/409/422 policy or SHA rejection is
`BLOCKED`/`RETURN TO REVIEW`, not permission to retry with weaker protection.

> **Connector alternative:** inspect the selected merge tool's actual schema.
> Use `expected_head_sha` only when that field is explicitly supported by the
> connector, or use the raw REST endpoint's `sha` field. If the selected tool
> exposes neither an expected-head field nor the raw protected endpoint, return
> `NOT_PROVEN`; do not invent an argument or merge without compare-and-swap.

If the head moved, the merge must fail closed and return to current-head review.
Never silently merge the replacement head.

Do not use `--admin`, protection bypass, or naked force operations.

### 7. Verify and reconcile

After a successful merge:

1. verify the PR reports merged and capture the merge SHA;
2. fetch/inspect current `main` and verify the expected result landed;
3. hand exact head/merge identity to the reconciliation path;
4. update the controlling issue/spec/proof/Changie state accurately;
5. remove only labels, branches, and worktrees whose ownership and cleanup
   safety are proven;
6. preserve salvage or ambiguous work.

A squash merge means the feature branch's commit ancestry is not the mainline
history. Do not use commit ancestry alone to decide that a worktree or unique
branch delta is safe to delete.

### 8. Handle non-ready results precisely

- `CONFLICTING` → inspect mechanical versus semantic conflict; select a reviewed
  resolution strategy.
- `UNKNOWN_NOT_PROVEN` → retry boundedly or report missing state.
- required check pending → `WAIT` for that named input.
- required check failed → classify product/test/instrument/policy failure.
- current-head proof missing/stale → request same-head rerun/dispatch; do not
  mutate the branch merely to trigger CI.
- head moved → `RETURN TO REVIEW`.
- draft → remain in review.
- active `needs-*` → route the named repair.
- advisory failure only → record it and apply the declared advisory policy.
- integration basis stale → refresh integration proof; do not automatically
  change the PR head.

### 9. Verify main health at bounded checkpoints

After a high-risk parser/lexer/public-API merge, or after the batch, verify the
latest `main` run for the merged SHA. Halt when `main` has a real required
failure. Distinguish an instrument/capacity failure from a product regression
before changing source.

## Output

```text
Merged: #NNN @ <expected-head> -> <merge-sha>
Skipped: #NNN (result class, exact blocker, next action)
Main verification: <sha> <result>
Reconciliation: complete / partial / salvage-required / not-proven
```
