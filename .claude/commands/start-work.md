---
description: Pre-mutation guard — confirm an issue is build-ready before creating a writer worktree/branch
argument-hint: "<issue#> [--mechanical] | --mechanical [<issue#>]"
user-invocable: true
---

# Start Work

Advisory pre-mutation guard. Confirms one controlling issue is actually
build-ready **before** any writer worktree, branch, or production edit gets
created, then hands off to the existing entry points — it replaces none of
them. Context: **$ARGUMENTS**

This is **not** a required check and does not fork the `builder-ready` gate.
It composes over `.claude/agents/spec-planner.md`, the `worktree-manager`
skill, and [WORKTREE_PROTOCOL.md §Fresh branch](../../docs/reference/WORKTREE_PROTOCOL.md).
See issue [#3971](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3971)
for the full model.

## Step 1: Parse arguments

Extract the optional `<issue#>` and the optional `--mechanical` flag from
$ARGUMENTS.

**Without `--mechanical`:** an issue number is required. If none is given,
stop and ask for one — this command never guesses a target.

**With `--mechanical`:** an issue number is optional. Genuinely mechanical
work (see "Which changes qualify as mechanical" in the Mechanical fast path
section below) frequently has no controlling issue at all — a typo fix or a
deterministic lockfile regeneration doesn't need one to exist first. When
`--mechanical` is present, skip **Step 2 entirely** (the issue-open check —
this applies whether or not an issue number was also given; mechanical work
has no issue-state precondition to verify) and skip straight to the
**Mechanical fast path** below, which also skips the build-readiness check
(Steps 3-4), the Step 4b advisory audit, and the Step 6b writer-admission
check. Only the safety-relevant checks still apply: collision (Step 5),
fresh `origin/main` (Step 6), and worktree integrity.

The flag must be passed explicitly; never infer it from an issue's body or
title — and never carry it over from an earlier invocation in the same
session. Each `/start-work` call re-classifies fresh from $ARGUMENTS.

## Step 2: Read the issue

Skipped entirely under `--mechanical` (see Step 1) — jump straight to the
Mechanical fast path.

```bash
gh issue view <issue#> --json title,body,labels,state,comments
```

Stop immediately if the issue is not `OPEN` — report its current state and
the PR/commit that closed it, if any.

## Step 3: Confirm `builder-ready` — the existing authoritative gate

Check `labels` for `builder-ready`. This is the **same** live signal
`lead-build.md` and `LIVE_SIGNALS_VS_LABELS.md` already treat as authoritative
— this step does not introduce a second gate, it turns the trust `lead-build`
already places in the label into an explicit pre-check.

**If `builder-ready` is absent: STOP.** Report:
```
#<issue#> is not builder-ready. Read-only investigation may continue, but no
writer worktree may be created. Next step: route through plan-review
(/plan-review-read, /plan-review-verify, /plan-review-stress,
/plan-review-improve) to reach a BUILD verdict. If this is a new concern that
hasn't been scoped yet, file or reconcile the controlling issue first — there
is no dedicated intake command for that step yet.
```

## Step 4: Confirm the latest plan revision has a matching BUILD verdict

Scan `comments` for the plan/plan-review comment-header convention already in
use on this repo (HTML comment markers, e.g. `<!-- implementation-plan:v1
revision: N ... -->` and `<!-- implementation-plan-review:v1 plan_revision: N
... -->` — see #3807 for a worked example):

1. Find the **highest** `revision` number among `implementation-plan:v1`
   comments — this is the latest plan revision.
2. Find every `implementation-plan-review:v1` comment whose `plan_revision`
   equals that number, posted **after** the plan comment it reviews.
   **Latest-wins:** if the latest plan revision has more than one review
   (e.g. an initial `REVISE`, then a later `BUILD` after the plan was
   patched in place, or vice versa), only the **chronologically last**
   review of that revision is authoritative. A stale `REVISE` followed by a
   later `BUILD` does NOT block; a stale `BUILD` followed by a later
   `REVISE`/`SPLIT` DOES block, even if an earlier review said `BUILD`.
3. Confirm that latest review's verdict is `BUILD` (not `SPLIT`, `REVISE`, or
   `REJECT`).
4. Check every comment posted **after** that review for language that
   materially invalidates it (a later `REVISE`/`SPLIT` verdict, a
   research-verifier finding that contradicts a load-bearing claim, a
   maintainer objection). A routine status update or an unrelated label
   change does not invalidate it.

Not every issue uses the formal comment-header convention — for simpler
issues, treat a plan-reviewer comment that explicitly states a build
recommendation as equivalent. Use judgment; when genuinely ambiguous, treat
it as unmet and stop.

**If no matching BUILD verdict exists, or a later comment invalidates it:
STOP.** Report:
```
#<issue#> carries builder-ready but the latest plan revision (rev N) has no
matching BUILD verdict [/ was invalidated by a later comment: <what and
why>]. Next step: re-run plan-review on the current revision before any
worktree is created.
```

## Step 4b: Advisory issue-plan audit

`xtask issue-plan audit` (`xtask/src/tasks/issue_plan.rs`, recovered in #3973)
is a report-only audit of issue-plan quality: missing builder-ready work-order
sections, `builder-ready` surviving on a closed issue, stale
`needs-plan-review`/sign-off label contradictions, and `#0000` placeholder
references. It has no dedicated single-issue flag — only `--fixture <path>`
(an offline JSON array of issues) or `--repo`/`--label` (a live, whole-set
query). Every check it runs is purely per-issue (no cross-issue
correlation), so a one-issue fixture built from `gh issue view` is a complete
target.

A failed or empty fetch must never read as a clean "no findings" — that would
be a silent false-clean (the instrument ran, but never actually saw real
issue data). Check `gh issue view`'s own exit status **and** that the
resulting fixture is a non-empty JSON array before trusting the audit's
output:

```bash
FIXTURE="$(mktemp --suffix=.json)"
ISSUE_JSON="$(gh issue view <issue#> --repo EffortlessMetrics/perl-lsp-swarm \
  --json number,title,body,state,labels)"
GH_STATUS=$?
if [ "$GH_STATUS" -eq 0 ] && [ -n "$ISSUE_JSON" ]; then
  echo "$ISSUE_JSON" | jq -s . > "$FIXTURE"
fi
if [ "$GH_STATUS" -eq 0 ] && [ -s "$FIXTURE" ] \
   && [ "$(jq 'length > 0' "$FIXTURE" 2>/dev/null)" = "true" ]; then
  cargo xtask issue-plan audit --fixture "$FIXTURE" --dry-run --format json
else
  echo "Note: could not fetch #<issue#> for the issue-plan audit (fetch" \
       "failed or empty) — skipping the advisory audit; this does not" \
       "block the handoff."
fi
rm -f "$FIXTURE"
```

This is advisory signal only — it does **not** add a second gate alongside
`builder-ready`/the BUILD verdict from Steps 3-4, and neither a non-empty
`findings` array nor a failed fetch ever stops the handoff. Surface findings
as a note, not a stop:

```
Note: issue-plan audit flags <n> finding(s) for #<issue#> (e.g.
"builder-ready but body is missing a 'non-goals' section") — consider
addressing before building; proceeding is not blocked on this.
```

Skipped under `--mechanical`, alongside Steps 3-4 — mechanical work has no
planning-readiness decision for this audit to check.

## Step 5: Collision check — is someone already writing this?

Fetch remote branches **first** — `git branch -a` only shows remote-tracking
refs that have already been fetched into this checkout. A stale checkout
would silently miss a branch another agent just pushed, defeating the
collision check:

```bash
git fetch origin "+refs/heads/impl/*:refs/remotes/origin/impl/*"
gh pr list --search "#<issue#>" --state open
git branch -a --list "impl/<issue#>-*" "*/impl/<issue#>-*"
git worktree list
python3 scripts/worktree-manager.py query
```

Use the precise `impl/<issue#>-<slug>` branch-name convention spec-planner and
`WORKTREE_PROTOCOL.md` already use — **not** a bare `*<issue#>*` substring
glob. A bare substring glob false-matches on shared digit sequences (e.g.
searching `39` also matches `392`, `3971`, `4390`), producing phantom
collisions or masking real ones. Same care applies to the PR search: after
`gh pr list --search "#<issue#>"` returns hits, confirm the match is this
issue and not a substring/false hit — e.g. cross-check
`closingIssuesReferences` or the PR title/body for the exact `#<issue#>`
token, not just "the search returned something."

Look for: an open PR referencing the issue, a local or remote branch matching
`impl/<issue#>-*`, a live worktree on such a branch, or a worktree-manager
slot already owned by another agent for this issue.

**If any of these exist: STOP.** Report what was found and its state (e.g.
"PR #NNN is open and in-build — continue there via `/builder-read-pr`, do not
open a second worktree"). This mirrors `lead-build.md`'s existing duplicate-PR
and active-builder guards; `/start-work` just runs the same check earlier,
before mutation instead of before spawning a second builder.

Under the mechanical fast path with no issue number, there is no `#<issue#>`
or `impl/<issue#>-*` to search for — adapt the searches to whatever branch
name/slug the mechanical change will use instead (e.g. `gh pr list --search
"<slug>"` and `git branch -a --list "*<slug>*"`), and still check
`git worktree list` / `worktree-manager.py query` for an existing writer on
that slug.

## Step 6: Fetch fresh `origin/main`

```bash
git fetch origin main
```

Confirm the fetch succeeded before proceeding — a stale base produces the
exact `origin/master`-base-ref class of defect this repo has hit before.

## Step 6b: Advisory writer-admission check

`xtask writer-admission` (`xtask/src/tasks/writer_admission.rs`, landed in
#4099) is a read-only admission diagnostic: it inspects canonical-base
freshness, `refs/heads/origin/*` shadow refs, a dangling/detached HEAD, the
branch↔worktree mapping, dirty/unpushed state, disk capacity, and
writer-collision (an open PR already owning the target branch), and returns
a `PASS` / `BLOCK` / `NOT_PROVEN` verdict with a per-check reason. It never
mutates git state, the filesystem, or GitHub, and always exits `0` — the
verdict lives in its output, not its exit code.

Run it for the branch/worktree about to be admitted:

```bash
cargo xtask writer-admission --base origin/main \
  --repo EffortlessMetrics/perl-lsp-swarm --json
```

If the target branch is already known (e.g. reusing an idle worktree-manager
slot instead of creating a fresh one), pass it explicitly along with the
worktree path being reused:

```bash
cargo xtask writer-admission --branch impl/<issue#>-<slug> \
  --base origin/main --worktree <path> \
  --repo EffortlessMetrics/perl-lsp-swarm --json
```

When no branch exists yet (the common case — the branch is created *by* the
Step 7 hand-off), omitting `--branch`/`--worktree` falls back to the current
checkout, which still surfaces real pre-admission risk: disk headroom,
dirty/unpushed state, shadow-ref contamination, and canonical-base drift in
the environment about to spawn the new worktree.

This is advisory signal exactly like Step 4b — it does **not** add a sixth
hard gate alongside issue-open/`builder-ready`/BUILD-verdict/collision/fresh-
`origin/main`, and neither a `BLOCK` nor a `NOT_PROVEN` verdict stops the
handoff on its own:

- **`BLOCK`**: surface every `BLOCK`-status check's reason as a strong note,
  e.g. `Note: writer-admission reports BLOCK: disk headroom 188.3G is below
  the floor 372.6G (max(FLOOR_GB=200, FLOOR_PCT=5%)), 71 worktree(s) present
  — resolve before creating the worktree.` The operator may still proceed;
  this is a strong nudge to fix the underlying condition (e.g. run
  `just clean-worktrees`) first, not a stop.
- **`NOT_PROVEN`**: surface as `Note: writer-admission couldn't verify
  <check>: <reason> — proceeding without this signal.`
- **`PASS`**: no note needed beyond recording it in the settled-packet output
  (Step 7's Output section).

Skipped under `--mechanical`, alongside Steps 2-4 and 4b — mechanical work
has no planning-readiness decision for this diagnostic to compose into;
only the plain safety checks (collision, fresh `origin/main`, worktree
integrity) still apply.

## Step 7: Hand off — delegate, don't fork

All conditions (issue open, `builder-ready`, matching BUILD verdict, no
collision, fresh `origin/main` — plus the Step 6b writer-admission verdict as
advisory signal) are the settled packet. `/start-work` does not create the
branch, worktree, or `.spec/` files itself — it hands off to the existing
entry point:

- **Pipeline work** (going through the standard build gate): spawn the
  `spec-planner` agent on the issue. It reads the `builder-ready` issue,
  creates `impl/<issue#>-<slug>` off `origin/main`, and writes
  `.spec/<issue#>-<slug>/{checklist,acceptance,context}.md` per its own todo
  list.
- **Direct/non-pipeline work** (no spec-builder workflow needed): use the
  `worktree-manager` skill's `allocate` command. **`allocate` already creates
  the git worktree itself** — it runs `git worktree add -B <branch> <path>
  <base>` for a new slot, or `git -C <path> checkout -B <branch> <base>` when
  reusing an idle slot (verified in `scripts/worktree-manager.py`). Do
  **not** follow it with a separate `git worktree add` — that would try to
  create a second worktree for the same branch at a different path and
  fail, or silently diverge from the one `allocate` already set up:
  ```bash
  python3 scripts/worktree-manager.py allocate --slot issue-<issue#> --branch impl/<issue#>-<slug> --owner <agent-id>
  ```
  The command's own stdout (`allocated slot=... path=... branch=... ref=...`)
  names the worktree path already created — `cd` into that path directly.
  [WORKTREE_PROTOCOL.md §Fresh branch (new work)](../../docs/reference/WORKTREE_PROTOCOL.md)
  documents the equivalent raw-`git worktree add` form for when the
  worktree-manager isn't in use; don't run both forms for the same slot.

Report which path was used and the resulting branch/worktree location.

## Mechanical fast path (`--mechanical`)

Reachable **only** via the explicit `--mechanical` flag — never inferred from
issue content, and never carried over from an earlier invocation. This path
is genuinely issue-free: it does not require an open issue, or any issue at
all, to exist.

### Which changes qualify as mechanical (self-classify against this list)

**No issue required — mechanical:**
- Obvious typo fixes (comments, doc prose, log/error strings).
- Formatting-only repair (whitespace, `cargo fmt`/`rustfmt`, markdown lint).
- Deterministic regeneration (lockfiles, generated fixtures, snapshot
  re-recording from an unchanged source-of-truth, corpus-manifest refresh).
- No-code lock-or-dependency refresh: a `Cargo.lock`/lockfile bump with no
  source-code changes, no known user-facing/security fix riding along, and
  no material compatibility decision — i.e. the version number moved and
  nothing else.

**Issue path required — not mechanical, even if it looks small:**
- Anything touching behavior, policy, architecture, semantics, CI authority,
  or security.
- A dependency bump that requires source-code changes to adopt, fixes a
  known user-facing or security concern, or carries a material compatibility
  decision (breaking API, MSRV bump, feature-flag change) — the issue isn't
  optional just because the diff is a version bump; it's the fix/decision
  behind the bump that needs one.
- Architecture, parser/compiler semantics, concurrency, security, public
  LSP/DAP behavior, CI authority, merge policy, and control-plane behavior
  always take the full path above, even with the flag.

When genuinely ambiguous, treat it as **not** mechanical and use the full
path — the flag is for the clear cases, not a way to skip planning on a
judgment call.

### What still runs

Only the safety-relevant checks still apply — none of them are
planning-decision gates, they're basic write-safety that has nothing to do
with whether a plan was reviewed:

- **Step 5** (collision check) — is someone already writing this change?
- **Step 6** (fresh `origin/main`) — is the base current?
- Worktree integrity generally (Step 5's `git worktree list` /
  worktree-manager query, and the branch/worktree-mapping half of what Step
  6b's tool checks, if a target branch is already known).

**Everything else is skipped**, because none of it is a safety check — it's
the planning-readiness gate this fast path exists to bypass for genuinely
mechanical work:

- **Step 2** (issue-open check) — skipped entirely; there is no issue
  precondition when `--mechanical` is set, whether or not an issue number
  was also given.
- **Steps 3-4** (`builder-ready` label / matching BUILD verdict) — skipped;
  no planning decision to clear.
- **Step 4b** (issue-plan audit) — skipped; nothing to audit without a plan.
- **Step 6b** (writer-admission) — skipped; it's advisory planning-readiness
  signal composed alongside 3-4/4b, not a safety check in its own right.

Before proceeding, record and print all five fields — do not create the
worktree until every field is filled in:

```
Scope: <exact files/change, one line>
Deterministic oracle: <what mechanically proves this is correct — a
  regeneration command, a formatter, a lockfile diff — not "looks right">
No behavior/policy change: <confirm — if this touches behavior, it is not
  mechanical; stop and use the full path instead>
No collision: <confirm Step 5 found nothing>
Rollback: <exact command to revert if this is wrong>
```

If an issue number happens to be given (e.g. to link a mechanical cleanup to
a tracking issue for visibility), it's recorded for reference only — it
carries no gating weight on this path. If no issue number is given, branch
naming falls back to a descriptive `<slug>` (there is no established
issue-free branch-naming convention codified elsewhere in this repo yet —
flagging as a follow-up rather than inventing one here); adapt Step 5's and
Step 7's `impl/<issue#>-<slug>` / `--slot issue-<issue#>` forms to that slug
directly.

Then hand off exactly as in Step 7.

## Output

On success, print the settled packet — for the full path: issue #, plan
revision, BUILD verdict comment link, collision check result, base SHA, and
the Step 6b writer-admission verdict (with any `BLOCK`/`NOT_PROVEN` reasons
noted); for the mechanical path: the five recorded fields, collision check
result, base SHA, and issue # if one was given — and the branch/worktree
handed off to. On stop, print exactly one next action — never both a stop
and a partial handoff.

## Bootstrap note

`/start-work` cannot gate its own creation — the PR that adds this file is
exempt from its own check by construction. Do not add a `/start-work` guard
step to the PR that first introduces it.
