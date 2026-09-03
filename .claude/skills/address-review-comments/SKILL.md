---
name: address-review-comments
description: Verify and disposition every substantive human, bot, and CI finding through the repository's sanctioned GitHub review helper.
user-invocable: false
---

# Address review comments

Read the current PR head, controlling issue, governing contract, proof, submitted
reviews, inline threads, and relevant CI findings.

For each substantive finding choose one supported class:

```text
fixed                    (terminal: resolves the thread)
refuted                  (terminal)
superseded               (terminal)
post-merge-follow-up     (terminal: requires --issue plus an argument that the
                          current claim is already satisfied)
current-blocker          (non-terminal: thread stays open)
blocked-by-prerequisite  (non-terminal: requires --issue)
not-proven               (non-terminal)
```

The legacy class `follow-up` is rejected by the helper because it does not say
whether the current claim is already satisfied or still blocked; choose
`post-merge-follow-up` or a keep-open class explicitly.

## Orchestration affordances

### Root decisions

The main Claude thread retains whether a finding is valid, stale, refuted, superseded,
or a bounded follow-up; whether resolving it belongs in the current candidate; whether
it changes the candidate, claim, owner, proof, risk, or rollback boundary; the accepted
repair; and which proof/review dimensions become stale.

### Useful subagent work

Use focused subagents, context forks, or an Agent Team only where useful for:

- complete thread/review/finding inventory;
- source and external-authority verification;
- high-output CI log/artifact classification;
- reproduction and production-path tracing;
- proof/oracle challenge;
- detecting a test, ratchet, support claim, or policy weakening disguised as a fix.

Children return finding identity, direct/contradictory evidence, searched scope,
classification, suggested disposition, uncertainty, and affected dimensions. They do
not resolve threads or authorize repair.

### Mutation owner and join

One candidate writer integrates accepted repairs. The main Claude thread joins duplicate
or conflicting findings, verifies dispositions against current evidence, and
decides which findings are repaired, refuted, superseded, followed up, blocked, or not
proven.

The join is complete only when every substantive finding has one supported
visible disposition, every promoted failure class is closed across its bounded governed
surface, candidate and claim mutation are recorded, accepted mutations have current
affected proof, and any material claim/owner change has returned to the proper earlier
route.

### Repair-wave boundary

Before the writer mutates, pin one review-wave observation basis: the current candidate
head, complete thread and submitted-review inventory, active requested reviewers, and
any current-head review check or run. Treat a review as usefully in flight only when
GitHub exposes a durable current-subject signal: a native requested reviewer or team
whose judgment is deliberately required, or a current-head review check/run with a
named completion or terminal event. A prior bot comment, stale review, reaction, quota
warning, or expectation that a reviewer usually runs is not liveness. Let a useful
in-flight review finish before the wave is published unless a material defect requires
immediate repair; in that case explicitly supersede the old review subject. A quota-
limited, unavailable, or failed reviewer is missing evidence, not a clean conclusion
and not a reason to wait indefinitely.

Judge finding validity and current-candidate admission separately. A confirmed finding
belongs in this candidate when leaving it unresolved would make the candidate
materially false, misleading, unsafe, under-proven, incompatible with its accepted
contract, or outside its stated risk/rollback boundary, and the repair remains inside
the same acceptance-and-rollback proposition. A repair that creates a separately
reversible proposition, consumer, authority, proof system, release horizon, or rollback
seam is a bounded follow-up or prerequisite rather than current-candidate expansion.
Route a missing governed owner or unsettled premise to issue/proof preparation,
`BLOCKED`, or `NOT_PROVEN`. When the proposed repair would mainly enlarge a one-use or
non-gated proof instrument, first narrow, simplify, delete, or split that instrument
rather than making its private implementation the candidate's dominant surface.

Do not treat comments as independent patch instructions. Only when two or more findings
share the same underlying mechanism, or one repair exposes another instance of that
mechanism, promote them to one failure class. Name the governed surface, enumerate
current instances inside that bounded surface, and repair the owning abstraction, rule,
or section rather than only the commented line.

A promoted class repair is complete only when the writer:

- fixes or dispositions every known instance in the bounded surface;
- adds or updates one class-level falsifier that would catch the repeated mechanism;
- rereads the complete governing semantic unit, not only the edited row, field, or line;
- updates dependent claims, summaries, tables, and proof descriptions that derive from
  the changed rule;
- runs affected proof and identifies any review dimensions made stale.

One writer integrates the full accepted wave and publishes it once. Do not push each
verified finding separately while useful review or CI is still evaluating the previous
head. If the wave introduces a new checker, registry, parser, abstraction, or substantial
proof surface, run `simplify-candidate` before final challenge. Reconcile the PR body to
the candidate's current claim, proof, limitations, and remaining work; GitHub history
already retains superseded intermediate conclusions.

### Return packet

Return candidate/head identity, review-wave observation basis, complete substantive
finding set, validity and current-candidate disposition, promoted failure classes and
bounded surfaces, evidence, commits/issues used by the helper, `candidate_changed`,
`claim_changed`, `stale_review_dimensions`, affected proof/review results, unresolved
contradictions, limitations, the earliest still-missing judgment, and typed result.

## Reply quality

The inline reply is a concise engineering decision record, not a completion signal.

Before replying, decide separately:

- whether the comment identifies a real failure on the current candidate;
- whether the reviewer's proposed repair, scope, or layer is correct;
- which invariant, owner, or architectural boundary governs the seam.

Do not blindly agree. A comment can be right about the failure and wrong about the
repair. Preserve the valid concern, reject the wrong mechanism, and repair the owning
seam.

Do not reflexively defend the candidate. Prior intent, familiarity, green checks, or
"works as designed" do not answer the concern. Refute only from current source,
governing authority, or discriminating evidence.

State the narrowest supported conclusion. Preserve mixed findings: fully valid, partly
valid, stale, superseded, and incorrect are different judgments. For `fixed`, explain
the failure and why the selected repair belongs at that boundary. For `refuted`,
identify the false premise or existing mechanism. For `superseded`, name the changed
candidate or premise. For `follow-up`, explain why the current claim is complete and
the residual is genuinely separate.

A bare `fixed`, `done`, `addressed`, generic thanks, labels-only reply, or paraphrase of
the diff is inadequate.

Use one short paragraph between the required lines:

```text
Disposition: <class>

<judgment, architectural reason, and what changed or why no change is warranted>

Evidence: <specific current source, proof, commit, authority, or linked issue>
```

The reply must answer the comment in its inline context. Mention files, symbols, tests,
or contracts only when they carry the reasoning; do not paste logs or narrate agent
activity.

## Procedure

1. Enumerate the PR's review threads with `scripts/reviews/threads <pr> [owner/repo] [--unresolved-only] [--json]`. This is the sanctioned read-only enumerator and the source of the `<threadId>` that step 9 passes to `disposition --thread`; `scripts/reviews/state` returns an aggregate classification with no per-thread identity and cannot supply it. Do not hand-roll a `reviewThreads` GraphQL query.
2. Pin the review-wave observation basis. Treat a review as in flight only when a native requested reviewer/team or a current-head review check/run supplies a durable current-subject signal and named terminal event. Let that useful review finish, or explicitly supersede its subject when an immediate material repair is required. Record unavailable, rate-limited, failed, stale, or merely expected reviews as missing evidence rather than liveness.
3. Verify each finding against current source and authority; do not patch comments literally. Decide separately whether it is valid and whether resolving it belongs in this candidate.
   A finding the currentness contract already answers is refuted, not complied with.
   Base staleness, "behind by N", head-SHA movement, and a demand to rebuild on current
   main for a conflict-free candidate are all answered by
   [`REVIEW_CURRENTNESS.md`](../../../docs/agents/REVIEW_CURRENTNESS.md) —
   this repository squash-merges, so unrelated base movement changes nothing. Reply
   `Disposition: refuted` citing the contract. Rebasing to satisfy such a request costs
   a full re-proof cycle, invites a fresh set of stale-head objections against the new
   head, and can silently absorb concurrent edits made to the branch meanwhile.

   A failing check is narrower, and the same contract's check-attribution rules decide
   it. Refuting one takes evidence, not arithmetic on SHAs:

   - a run that was **cancelled** by a newer push reached no verdict and is not
     evidence in either direction — let the run at the current head answer;
   - a run that **genuinely failed** stays a finding even though its SHA is
     superseded. Later commits that did not touch the failing seam do not refute it: a
     documentation-only push leaves a test failure exactly as true as it was.
     Revalidate that seam at the current head before dispositioning;
   - a failure blamed on the base needs the same gate, the same failure signature, and
     that signature observed at this PR's **merge base** — not at current main, and
     not a locally approximated command. Short of that it is `NOT_PROVEN`, not
     somebody else's.
4. Admit a confirmed finding when leaving it unresolved would make the current candidate materially false, misleading, unsafe, under-proven, incompatible with its accepted contract, or outside its stated risk/rollback boundary, and the repair stays inside the same acceptance-and-rollback proposition. Route separately reversible work, missing authority, or an unsettled premise to its proper follow-up, prerequisite, blocker, or `NOT_PROVEN` state.
5. Only for repeated instances of one mechanism, join findings into a failure class and enumerate that class across its bounded governed surface. An isolated finding remains isolated.
6. Batch the admitted source/proof repairs and every promoted class repair through one integrating writer; do not publish per-comment pushes and do not create an empty repair commit when no candidate bytes need to change.
7. Record `candidate_changed`, `claim_changed`, and `stale_review_dimensions`. `candidate_changed` means a semantic candidate change — source behavior, proof obligations, or claim-bearing contracts — not a byte-only update: formatting, generated-receipt refreshes, comment-only edits, and additive test strengthening do not set it true. When the candidate or claim changed semantically or review dimensions became stale, run affected focused proof and reread the complete governing semantic unit plus dependent claims. Run a class-level falsifier only when a failure class was promoted. When `candidate_changed=false`, `claim_changed=false`, and `stale_review_dimensions` is empty, preserve current proof/review conclusions and do not manufacture another challenge cycle.
8. If the repair introduced substantial proof machinery, run `simplify-candidate`; then update the PR synthesis to the current candidate rather than appending a repair diary.
9. Write the canonical human reply under the **Reply quality** contract: keep the `Disposition: <class>` and `Evidence: <claim-bounded evidence summary>` lines and put the concise reasoned judgment between them. Pass that complete text through `--reply` to `scripts/reviews/disposition` with the PR, thread ID, lowercase class, and class-specific evidence (`--commit`, `--argument`, `--superseded-by`, or `--issue`).
10. Let the helper append the `<!-- disposition:v1 ... -->` marker to that supplied reply and post it. Terminal classes then resolve the thread; non-terminal classes keep it open, and reopen a falsely-resolved thread before posting evidence.
11. Re-run the enumerator to confirm no substantive thread or unclosed promoted failure class remains without a non-terminal disposition.
12. If source/proof changed, invalidate and refresh only the affected review dimensions. If the wave changed no candidate, claim, or review dimension, preserve the existing review and continue at the earliest genuinely missing judgment.

Do not use raw thread-resolution APIs, resolve performatively, or use pr-responded or
reviewer-persona labels as evidence.

## GitHub boundary

Localized findings and dispositions belong in their inline threads. Cross-cutting
finding classes, material premise changes, or bounded follow-up decisions may update the
PR/issue synthesis. Preserve the human-readable `Disposition:` / `Evidence:` reply and
helper marker before resolution.

Keep subagent/Team topology, raw logs, duplicate findings, temporary reproduction output,
retries, and routine progress runtime-local. Do not post one agent summary per finding
or resolve merely to make the thread count green.

## Routes

- `FINDINGS_REPAIRED_OR_DISPOSITIONED` with `candidate_changed=true`, `claim_changed=true`, or non-empty `stale_review_dimensions` → affected proof, then `final-challenge`
- `FINDINGS_REPAIRED_OR_DISPOSITIONED` with `candidate_changed=false`, `claim_changed=false`, and empty `stale_review_dimensions` → preserve current proof/review and continue at the earliest still-missing judgment
- `MUTABLE_FINDINGS_OPEN` → complete one joined repair wave through the current writer
- `MATERIAL_PREMISE_CHANGED` → `prepare-issue`
- `PROOF_WEAKENED` → `prepare-proof`
- `FOLLOW_UP_ACCEPTED` → create/link the bounded follow-up and continue within the current claim
- `DISPOSITION_INSTRUMENT_FAILURE` / `BLOCKED` / `NOT_PROVEN` → preserve the unresolved finding or missing evidence
