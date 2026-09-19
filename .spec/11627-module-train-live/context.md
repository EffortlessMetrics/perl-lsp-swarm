# Context: #11627 — read-only module live frontier (`module_train_live.v1`)

## Problem

The stable train topology (`module_train.v1`, #11625) and the offline current-tree
projection (`module-train status|next`, #11626) answer "what may proceed on this
exact tree". Nothing answers "what is happening on each node **right now**": which
branches/worktrees/PRs claim each node, whether a candidate is canonical or
duplicate, whether its head/review/proof/checks are current, and which single safe
action each writer/conflict surface should take next. Every controller re-derives
that join by hand from prose and feelings about CI colour.

## Why this approach

Join — do not clone — the two existing authorities to one bounded, immutable,
read-only observation, then classify pure/offline/deterministic from it:

* #11625 owns topology (nodes, roles, typed edges, writer/conflict keys).
* #11626 owns offline current-tree state (`LoadedManifest::node_statuses()`,
  the digest-pinned projection). This slice adds one additive public accessor;
  it changes no C02 semantics.
* This slice owns live candidate/stack/worktree/check/review observation and the
  safe-action projection. It performs **no external mutation**: no assignment,
  lease, scheduling, branch/worktree/issue/PR creation, comment, review, repair,
  push, merge, close, release, publication, or support promotion.

Commands (`module-train live …`):

```bash
cargo xtask module-train live refresh --output <snapshot.json>          # network reads only here
cargo xtask module-train live refresh --from-fixture <raw.json> --output <snapshot.json>
cargo xtask module-train live check   --snapshot <snapshot.json>        # offline
cargo xtask module-train live next    --snapshot <snapshot.json>        # offline
cargo xtask module-train live explain <node> --snapshot <snapshot.json> # offline
```

Normalization, validation, action classification, frontier selection and
explanation are pure/offline/deterministic from the immutable snapshot.

## Snapshot identity

`module_train_live.v1` binds, under a semantic digest (SHA-256 over the canonical
walk already used by the train tooling; `observed_at` stays **outside** the
digest):

* repository identity, default branch, observed main SHA (+ observation source);
* the pinned `module_train.v1` manifest digest and the C02 projection summary;
* per-instrument state: `git_local`, `git_remote`, `github_prs` — each
  `ok | failed | rate_limited | permission_denied | truncated | unavailable`
  with detail; instrument failure is **never** absence, `not_proven` or
  `instrument_failed` by law;
* bounded git facts: HEAD, branch, dirty paths (capped), local branches with
  upstream tracking, worktrees (path/head/branch/dirty/locked), remote refs;
* bounded GitHub PR records (bound or module-associated only; bodies are
  consumed for binding, then dropped): state, draft, base/head refs + OIDs,
  mergeable, mergeCommit, review decision, latest reviews (login/state/time),
  check-rollup summary capped per state bucket with truncation flag;
* per node: C02 state + typed reasons, observed candidate states, associated
  local/remote surfaces with dirty/unpushed/unique disposition, exactly one
  recommended action with reason codes and limitations.

## Candidate binding law

A branch/PR owns a module node only through an explicit machine-checkable
identity block in the PR body, equivalent to:

```text
Module train: #11625
Module node: #<implementation issue>
Parent/controller: #<controller issue>
```

(leading markdown bullets/bold tolerated), plus agreement with the manifest:
the node exists, the controller matches the node's chain controller, the base
matches the declared relation (`main` while `stack_relation` is `none`), and the
node is an implementation-capable role. Title similarity, branch names, touched
files, author, model, labels, issue assignment, age, update time and CI colour
are recorded as diagnostics, never ownership. PRs carrying module-train trailers
that fail agreement are recorded `misbound`; local/remote branches are
name-associated diagnostics only — an associated unbound surface yields a bounded
ownership decision (`RECONCILE`), never a silent `START`.

## Action model (exactly one per node / writer-conflict surface)

`START | RESUME | REPAIR | RESTACK | REVIEW | WAIT | MERGE_READY_RECOMMENDATION |
SUPERSEDE_RECOMMENDED | RECONCILE | RETURN_TO_ISSUE | BLOCKED | NOT_PROVEN | STOP`

Load-bearing laws encoded and tested:

* one viable candidate is resumed/repaired/restacked before any duplicate START;
* two candidates for one node are `RECONCILE`, ranked by nothing (byte-identical
  snapshot under candidate order permutation);
* controllers/fan-in/gates/claims bound as implementation are `STOP`
  (`controller_selected_as_implementation`);
* a C02-blocked node never STARTs merely because no PR exists;
* instrument failure (permission/rate-limit/truncation/timeout/malformed) is
  `NOT_PROVEN`/`instrument_failed`, never "no candidate" and never pass;
* behavior receipts are **not observable** — an unconditional typed blocker
  that keeps `MERGE_READY_RECOMMENDATION` unreachable from live observation
  (the classifier branch exists and is exercised by synthetic-fact unit tests);
* review threads **are** observable since #14237, through one gated read-only
  GraphQL document, and fail closed: a truncated or unobserved thread page, a
  head that moved between the list and the review read, or any GraphQL
  instrument failure leaves resolution unprovable rather than resolved;
* review-head **currency** remains unobservable, and deliberately so. #14237
  observes whether each opinionated review was submitted against the head
  commit, but reports it only as a diagnostic
  (`reviewed_commit_differs_from_head`). `REVIEW_CURRENTNESS.md` ("Review is
  semantic, not exact-head") and `AGENTS.md` ("head SHA change alone -> no
  review invalidation") forbid treating a head SHA as a review-validity token;
  materiality of later commits is a judgment this observation cannot make, so
  `head_moved_after_review` is never raised from a commit delta;
* merged PRs are `merged_current_tree` only when their merge commit is an
  ancestor of the locally observed HEAD (read-only `git merge-base
  --is-ancestor`); otherwise `merged_candidate_pending_current_tree_probe`;
* main movement alone never invalidates an action (no rebase/restack churn
  without `CONFLICTING`/stale-base facts);
* a hard-dependency node with a nonterminal bound candidate forces dependents to
  `WAIT` (`hard_dep_candidate_nonterminal`), which is what keeps fan-in nodes
  (L09G / P11F class) from starting while children are in flight; fan-in/claim
  starts additionally require child behavior receipts, which are unobservable
  here → `NOT_PROVEN`;
* dirty/unpushed/unique local work is never disposable (`RECONCILE`, never
  silent START/discard).

## Read-only enforcement

All adapter subprocesses route through one choke point with a fixed allowlist:
`git rev-parse | status | for-each-ref | worktree list | ls-remote |
merge-base --is-ancestor` and `gh pr list | gh pr view` (JSON only). The
allowlist itself is asserted read-only by tests (no push/merge/close/create/
write verbs anywhere); no other `Command` construction exists in the module.
Network reads happen only in `refresh` (network mode); `--from-fixture` and all
other subcommands are fully offline.

## Honest residual boundaries of this slice (never guessed)

* Behavior-receipt/profile observation: absent, because #11619 (P11A) has not
  landed and this tree has no `module-process` task or
  `module_resolution_composition.v1` schema → `MERGE_READY_RECOMMENDATION`
  remains unreachable live; the typed blocker is named.
* Review-thread observation and review-head binding: closed by #14237. Currency
  is computed from `latestOpinionatedReviews` (the reviews that carry a
  decision), so an advisory comment left on an older commit does not report a
  current approval as stale.
* Per-node semantic implementation probes (C02 residual, inherited): nodes whose
  implementation landed without binding trailers (e.g. C02's own merged PR)
  remain honestly classified from C02 data (`ready` + unbound-surface
  diagnostics), never "landed".
* Explicit stack parsing (`explicit_stack_member`), supersession projection,
  `conflict_key_collision` beyond manifest uniqueness, issue-state observation
  (deliberately excluded: closure/labels are never authority), `RETURN_TO_ISSUE`
  reachability, `#11106` shared-authority extraction: vocabulary reserved,
  fail-closed, documented.
* #11626's own `explain` static packet and `graph` residuals stay with #11626;
  `live explain` composes the manifest's static node facts with the live
  addendum today and extends when C02's packet lands.

## Authority and ownership

* Topology: #11625. Offline current-tree projection: #11626 (consumed through
  its public seam, unmodified semantically). Live observation + action model:
  this issue. Behavior/claim truth: #8479 / P11 / #7460 — never manufactured
  here. Candidate/writer/review mechanics: #4177 / #3957 / #3982 / #8042 /
  #3693 / #6060 / #3985 / #3989 / #6193 / #10168 as current #11106 children.
* No active-goal pointer, agent registry, lease table, queue, polling daemon,
  mutable frontier store, or model/provider roster is created. The snapshot is
  an immutable artifact produced on demand; GitHub and Git own all live state.

## Privacy and bounds

Raw API payloads are never stored: bodies are scanned for the identity block and
dropped; PR records keep bounded fields only; check names are capped per state
bucket (50) with a truncation flag; dirty paths are capped (50). Lists are
fetched with explicit limits, and truncation degrades precisely rather than
globally: an OPEN-window truncation marks absence of a viable candidate
unprovable (every node gates to `NOT_PROVEN`), while a MERGED-window
truncation — permanent at this repository's merge velocity for any bounded
window — degrades only merged-candidate facts to a recorded per-node
limitation (`merged_window_truncated_merged_facts_not_proven`) without gating
viability. No truncated list is ever treated as complete.

## Adoption, rollback, stop

* **Adoption:** run the four commands; `next`/`explain` are the consumer
  surfaces for controllers and (#11114-class) packet consumers.
* **Rollback:** revert this PR; C02 and C01 artifacts are untouched.
* **Stop conditions:** instrument failure at refresh (snapshot records the
  failure and downstream classification returns `NOT_PROVEN`); manifest digest
  drift (inherited fail-closed from the C02 loader); snapshot digest mismatch or
  stored-action inconsistency at `check`.

## Links

Controlling issue: #11627 (C03). Umbrella: #11869. Programme: #8133 / #4240.
Topology: #11625 (merged #12043). Offline projection: #11626 (merged #12055).
Dogfood consumer: #11114.
