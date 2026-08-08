---
name: review-pr
description: Run Claude Code's cumulative substantive pull-request review, orchestrating useful adversarial lenses and publishing one evidence-backed judgment before live integration.
user-invocable: false
---

# Review PR

Run the substantive review in Claude Code. This skill is the operational review
procedure; do not defer the work to shared doctrine, a bot summary, green CI, or the
fact that the main thread read the diff.

The Claude lane root owns the cumulative judgment and GitHub review. Focused subagents
may gather evidence or challenge one dimension, but they do not authorize merge and
their verdicts are not votes.

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

The lane root retains review scope and current/stale dimensions, evidence credibility
and contradictions, finding severity/disposition, whether the claim is supported or
requires repair/`NOT_PROVEN`/prerequisite/closeout, and the one cumulative submitted
review plus next route.

### Useful review contexts

First reuse current joined adversarial evidence from `finish-pr` or an earlier
invocation when claim, production path, authority, proof, compatibility, risk, and
rollback remain current. Do not dispatch duplicate review merely because this skill
was entered separately.

When a lens is absent, stale, contradictory, or materially changed, invoke
`orchestrate-work` only for missing dimensions:

- `review-tests` for proof discrimination, historical-defect controls,
  schema/validator agreement, and false-green tests;
- `review-candidate` for implementation correctness, semantic ownership, production
  reachability, complexity, compatibility, risk, and rollback;
- bounded production-path tracing from real request/command/installer/workflow/runtime
  consumer to changed seam;
- bounded external truth against perldoc, protocol/platform docs, dependency API,
  release topology, or other competent authority;
- focused security, persistence, packaging, migration, performance, or support review.

A useful child brief names the exact PR/candidate, claim, settled facts, authorities,
one read-only question, named skill, falsifiers, required evidence, uncertainty, and
non-goals. Use ordinary subagents/context forks for independent returns; use an Agent
Team only when lateral communication changes the result. Do not ask vague agents to
repeat the same review.

### Mutation owner and join

The construction context must not be the only detection surface supporting a merge.
Independence comes from changed source, oracle, threat model, method, environment, or
attention—not identity alone.

Join evidence rather than counting answers. Resolve contradictions against source and
proof, reject unsupported confidence, and inspect load-bearing seams before publishing
the cumulative judgment. One candidate writer repairs accepted findings through
`address-review-comments`; read-only reviewers do not mutate.

### Return packet

Return candidate/head and claim identity, cumulative seams/live consumers, lenses and
searched scope, authorities/falsifiers, proof/production-route conclusions, findings
with severity/evidence/disposition, contradictions, prior dispositions, limitations/
`NOT_PROVEN`, GitHub-fact snapshot, substantive result, and next route.

## Required review procedure

1. **Reconstruct the candidate and evidence map.** Establish claim/non-goals,
   controlling authority, cumulative seams, live callers/consumers, proof/limitations,
   prior findings/dispositions, and current GitHub facts.
2. **Trace production reachability.** Show how a real request or operation reaches the
   behavior. Compiled components, setters, adapters, and fixtures are not system proof
   unless the live route consumes them.
3. **Challenge proof discrimination and evidence integrity.** Identify realistic wrong
   implementations the proof rejects. Check negative, stale, failure, recovery,
   refusal, opposite-direction controls; independent oracles; schema/validator
   agreement; loaded/recomputed identities/hashes; generated-source binding; and
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
7. **Publish the review.** Put localized findings inline and publish one cumulative
   review or useful clean conclusion.

## Substantive review results

```text
REVIEW_CURRENT
CHANGES_REQUIRED
NOT_PROVEN
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

- `REVIEW_CURRENT` means the claim is supported and no substantive finding remains; it
  may enter `verify-live-ci`.
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

Keep subagent/Team topology, raw exploration, temporary experiments, duplicated clean
reports, and routine progress runtime-local. GitHub receives localized findings,
dispositions, and one cumulative review because those survive the review context.

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

- `REVIEW_CURRENT` → `verify-live-ci`
- `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` → `address-review-comments`
- weak/non-discriminating proof → `review-tests` or `prepare-proof`
- candidate correctness/reachability/ownership/complexity/rollback uncertainty →
  `review-candidate` through `orchestrate-work`
- `REVIEW_SCOPE_CHANGED` → review affected dimensions; `prepare-issue` only when claim/owner changed
- `BLOCKED_BY_PREREQUISITE` → preserve exact prerequisite
- `SUPERSEDED_OR_CLOSE` → preserve durable closeout
- `NOT_PROVEN` / `REVIEW_NOT_PROVEN` → resolve missing evidence, authority, or instrument
