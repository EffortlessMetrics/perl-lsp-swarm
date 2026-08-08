---
name: review-pr
description: Run Codex's cumulative substantive pull-request review, orchestrating useful adversarial lenses and publishing one evidence-backed judgment before live integration.
---

# Review PR

Run the substantive review in Codex. This skill is the operational review procedure;
do not defer the work to a shared method document, a bot summary, green CI, or the fact
that the root session read the diff.

The Codex lane root owns the cumulative judgment and GitHub review. Focused native
workers may gather evidence or challenge one dimension, but they do not authorize
merge and their verdicts are not votes.

## Authoritative inputs

Read the selected PR, controlling issue/current synthesis, accepted claim/non-goals,
governing specification/ADR/policy or competent external authority, cumulative diff,
live production or operational consumers, focused proof/limitations, submitted reviews
and inline threads, and current GitHub integration facts.

Use the PR head to identify the candidate currently visible on GitHub. It is not a
review-validity token. Do not compute a claim digest, run review-start/review-done
receipt machinery, or post a status-only exact-head comment.

## Review orchestration

### Lane-root decisions

The lane root retains:

- review scope and which dimensions are materially current or stale;
- which worker evidence is credible, duplicated, contradictory, or incomplete;
- finding severity and candidate-owned versus prerequisite/follow-up disposition;
- whether the claim is supported, changes are required, evidence is not proven, a
  prerequisite blocks, or the claim is superseded;
- the one cumulative submitted review and next route.

### Useful read-only work

First determine whether `$finish-pr` or an earlier invocation already produced current
joined adversarial evidence. Reuse it when claim, production path, authority, proof,
compatibility, risk, and rollback remain current. Do not dispatch duplicate review
passes merely because this skill was entered separately.

When an applicable lens is absent, stale, contradictory, or materially changed, invoke
`$orchestrate-work` only for missing dimensions:

- `$review-tests` for proof discrimination, historical-defect controls,
  schema/validator agreement, and false-green tests;
- `$review-candidate` for cumulative implementation correctness, semantic ownership,
  production reachability, complexity, compatibility, risk, and rollback;
- bounded production-path tracing from real request/command/installer/workflow/runtime
  consumer to changed seam;
- bounded external truth against perldoc, protocol/platform documentation, dependency
  API, release topology, or another competent authority;
- focused security, persistence, packaging, migration, performance, or support review.

A useful worker brief names the exact PR/candidate, controlling claim, established
facts, authorities, one read-only question, named `$skill`, realistic falsifiers,
required evidence, uncertainty, and non-goals. Do not ask vague workers to repeat the
same review.

### Mutation owner and join

The construction context must not be the only detection surface supporting a
substantive merge. Independence comes from changed source, oracle, threat model, method,
environment, or attention—not merely identity.

Join evidence rather than counting answers. Resolve contradictions against source and
proof, reject unsupported confidence, and inspect load-bearing seams before publishing
the cumulative judgment. One candidate writer repairs accepted findings through
`$address-review-comments`; read-only reviewers do not mutate.

### Return packet

Return candidate/head and claim identity, cumulative seams and live consumers, lenses
and searched scope, authorities/falsifiers, proof/production-route conclusions,
findings with severity/evidence/disposition, contradictions, prior dispositions,
limitations/`NOT_PROVEN`, GitHub-fact snapshot, substantive review result, and next
route.

## Required review procedure

1. **Reconstruct the candidate and evidence map.** Establish claim/non-goals,
   controlling authority, cumulative seams, live callers/consumers, proof/limitations,
   prior findings/dispositions, and current GitHub facts.
2. **Trace production reachability.** Show how a real request or operation reaches the
   changed behavior. Compiled components, setters, adapters, and fixtures are not system
   proof unless the live route consumes them.
3. **Challenge proof discrimination and evidence integrity.** Identify realistic wrong
   implementations the proof rejects. Check negative, stale, failure, recovery,
   refusal, opposite-direction controls; independent oracles; schema/validator
   agreement; loaded/recomputed identities and hashes; generated-source binding; and
   whether hosted proof exercised the claim.
4. **Challenge external and semantic truth.** Verify user-visible, language, protocol,
   platform, dependency, and release claims against competent authority. Confirm the
   correct semantic owner rather than creating a second parser/readiness/schema/
   compatibility authority.
5. **Challenge claim honesty, complexity, risk, and rollback.** Keep title, body, code,
   tests, docs, and generated evidence inside one acceptance-and-rollback claim. Do not
   let rejection, fallback, limitation, safe refusal, or partial implementation conceal
   a condition the contract says must block.
6. **Classify GitHub facts separately.** Record checks, threads, draft state,
   mergeability, rulesets, queue state, and prerequisites as a snapshot. They inform
   integration but do not create substantive review.
7. **Publish the review.** Post file/line-anchored material findings and the cumulative
   conclusion as one submitted review with
   `scripts/reviews/inline --pr <n> --body <summary> [--findings <file>]`, which takes
   findings as a JSON array of objects carrying a repo-relative path, a line in the
   diff, an optional multi-line range start, and a body — on stdin or from a file.
   Submit as `COMMENT`; this repository does not submit `APPROVE`. The
   post is atomic — one unaddressable location aborts the whole review rather than
   silently dropping a finding — so correct the location and resubmit rather than
   falling back to a single top-level comment, which loses file/line anchoring and
   cannot be dispositioned per finding. `scripts/reviews/inline` never resolves
   anything; resolution stays in `scripts/reviews/disposition`.

   **The integrating reviewer posts.** A skill run that only answers a bounded review
   question returns file/line-anchored findings as evidence and does not write to
   GitHub itself. One cumulative judgment is published per review pass, not one per
   lens.

## Substantive review results

```text
REVIEW_CURRENT
CHANGES_REQUIRED
NOT_PROVEN
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

- `REVIEW_CURRENT` means the reviewed claim is supported and no substantive finding
  remains; it may enter `$verify-live-ci`.
- `CHANGES_REQUIRED` means a candidate-owned correctness, reachability, proof,
  authority, complexity, risk, or rollback defect requires repair.
- `NOT_PROVEN` preserves missing, contradictory, stale, partial, or instrument-failed
  evidence.
- `BLOCKED_BY_PREREQUISITE` names the exact external claim/contract required first.
- `SUPERSEDED_OR_CLOSE` preserves why the claim should not proceed.

Green checks, `mergeable: true`, zero threads, bot approval, or author self-
certification cannot create `REVIEW_CURRENT`.

## Useful GitHub review record

```markdown
## Review scope
- Claim, cumulative seams, live consumers, prior findings, and applicable risk reviewed

## Evidence and falsifiers
- Commands, tests, fixtures, sources, or authorities used
- Realistic wrong behavior challenged

## Findings
- Material findings with severity, affected claim, and evidence

<!-- Or: ## No material findings -->

## Prior finding dispositions
- fixed | refuted | superseded | follow-up, with evidence

## What this establishes
- Conclusions supported by the review

## Residual risk / not proved
- Local uncertainty, excluded surfaces, and instrument limitations

## Current GitHub facts
- Checks, threads, draft/ready state, mergeability, and prerequisites as a snapshot

## Substantive review result
- REVIEW_CURRENT | CHANGES_REQUIRED | NOT_PROVEN | BLOCKED_BY_PREREQUISITE |
  SUPERSEDED_OR_CLOSE

## Next action
- Repair, focused re-review, live integration evaluation, closeout, or named follow-up
```

Do not submit only `LGTM`, `review complete`, reviewer identity, head SHA, check
summary, or status line. A clean review is valid when it records what was examined,
what wrong behavior was challenged, and what remains unproved.

Keep worker topology, raw exploration, temporary experiments, duplicated clean reports,
and routine progress runtime-local. GitHub receives localized findings, dispositions,
and one cumulative review because those remain useful after the review context ends.

## Semantic currentness

- later commit alone does not invalidate review;
- finding repair requires checking that finding, proof, and changed seam;
- material claim, production-route, authority, proof, compatibility, risk, or rollback
  change requires affected review;
- formatting, editorial cleanup, generated receipt refresh, and stronger tests do not
  trigger full review unless a conclusion changes;
- conflict/combined-tree repair receives focused review of the affected interaction.

Do not restart a full deep review or duplicate a still-current review merely to show
activity.

## Routes

- `REVIEW_CURRENT` → `$verify-live-ci`
- `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` → `$address-review-comments`
- weak/non-discriminating proof → `$review-tests` or `$prepare-proof`
- candidate correctness/reachability/ownership/complexity/rollback uncertainty →
  `$review-candidate` through `$orchestrate-work`
- `REVIEW_SCOPE_CHANGED` → review affected dimensions; `$prepare-issue` only when claim/owner changed
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite
- `SUPERSEDED_OR_CLOSE` → preserve durable closeout
- `NOT_PROVEN` / `REVIEW_NOT_PROVEN` → resolve missing evidence, authority, or instrument
