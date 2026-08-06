---
name: review-pr
description: Run Codex's cumulative substantive pull-request review, orchestrating useful adversarial lenses and publishing one evidence-backed judgment before live integration.
---

# Review PR

Run the substantive review in Codex. This skill is the operational review procedure;
do not defer the work to a shared method document, a bot summary, green CI, or the fact
that the root session read the diff.

The Codex root owns the cumulative judgment and GitHub review. Focused native
subagents may gather evidence or challenge one dimension, but they do not authorize
merge and their verdicts are not votes.

## Authoritative inputs

Read the selected PR, controlling issue and current synthesis, accepted claim and
non-goals, governing specification/ADR/policy or competent external authority,
cumulative diff, live production or operational consumers, focused proof and known
limitations, submitted reviews and inline threads, and current GitHub integration
facts.

Use the PR head to identify the candidate currently visible on GitHub. It is not a
review-validity token. Do not compute a claim digest, run review-start/review-done
receipt machinery, or post a status-only exact-head comment.

## Review orchestration

First determine whether `$finish-pr` or an earlier invocation has already produced
current joined adversarial evidence for this cumulative candidate. Reuse that evidence
when its claim, production path, authority, proof, compatibility, risk, and rollback
subjects remain current. Do not dispatch duplicate review passes merely because this
skill was entered separately.

When an applicable lens is absent, stale, contradictory, or materially changed,
invoke `$orchestrate-work` for only the missing dimensions. Normal focused assignments
include:

- `$review-tests` for proof discrimination, historical-defect controls,
  schema/validator agreement, and false-green tests;
- `$review-candidate` for cumulative implementation correctness, semantic ownership,
  production reachability, complexity, compatibility, risk, and rollback;
- a bounded read-only production-path trace from the real request, command, installer,
  workflow, or runtime consumer to the changed seam;
- a bounded external-truth pass against perldoc, protocol/platform documentation, a
  dependency API, release topology, or another competent authority;
- a focused security, persistence, packaging, migration, or support pass when the
  claim touches that boundary.

A useful Codex subagent brief names the exact PR/candidate, controlling claim,
established facts, authoritative files or sources, one read-only question, realistic
falsifiers, required evidence, uncertainty to preserve, and non-goals. Tell the child
which `$skill` to consume. Do not ask several vague agents to repeat the same review.

The construction context must not be the only detection surface supporting a
substantive merge. Independence comes from a different source, oracle, threat model,
method, environment, or attention surface—not merely a different agent name.

Join evidence rather than counting answers. Resolve contradictions against source and
proof, reject unsupported confidence, and inspect the load-bearing seams yourself
before publishing the cumulative judgment. One integrating writer repairs accepted
findings through `$address-review-comments`; read-only reviewers do not mutate the
candidate.

## Required review procedure

1. **Reconstruct the candidate and evidence map.** Establish the claim and non-goals,
   controlling authority, cumulative changed seams, live callers and consumers, proof
   and limitations, prior findings and dispositions, and current GitHub facts.
2. **Trace production reachability.** Show how a real request or operation reaches the
   changed behavior. A compiled component, public setter, adapter, or fixture is not
   system proof unless the live route consumes it.
3. **Challenge proof discrimination and evidence integrity.** Identify realistic wrong
   implementations the proof rejects. Check negative, stale, failure, recovery,
   refusal, and opposite-direction controls; independent oracles; schema/validator
   agreement; loaded or recomputed identities and hashes; generated-source binding;
   and whether hosted proof actually exercised the claimed path.
4. **Challenge external and semantic truth.** Verify user-visible, language, protocol,
   platform, dependency, and release claims against competent authority. Confirm the
   candidate extends the correct semantic owner rather than creating a second parser,
   readiness model, schema, or compatibility authority.
5. **Challenge claim honesty, complexity, risk, and rollback.** Keep the title, body,
   code, tests, docs, and generated evidence inside one acceptance-and-rollback claim.
   Do not let an intended rejection, fallback, limitation, safe refusal, or partial
   implementation conceal the exact condition the contract says must block.
6. **Classify GitHub facts separately.** Record checks, threads, draft state,
   mergeability, rulesets, queue state, and prerequisites as a snapshot. They inform
   integration but do not create a substantive review result.
7. **Publish the review.** Put file-specific material findings in inline threads and
   publish one cumulative review or useful clean conclusion.

## Substantive review results

Use one result:

```text
REVIEW_CURRENT
CHANGES_REQUIRED
NOT_PROVEN
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

- `REVIEW_CURRENT` means the reviewed claim is supported and no substantive finding
  remains. It may now enter `$verify-live-ci`.
- `CHANGES_REQUIRED` means a candidate-owned correctness, reachability, proof,
  authority, complexity, risk, or rollback defect must be repaired.
- `NOT_PROVEN` preserves missing, contradictory, stale, partial, or instrument-failed
  evidence.
- `BLOCKED_BY_PREREQUISITE` names the exact external claim or contract that must become
  trustworthy first.
- `SUPERSEDED_OR_CLOSE` preserves the durable reason the claim should not proceed.

Green checks, `mergeable: true`, zero open threads, bot approval, or author
self-certification cannot independently create `REVIEW_CURRENT`.

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

Do not submit only `LGTM`, `review complete`, reviewer identity, a head SHA, a check
summary, or a status line. A clean review is valid when it records what was examined,
what realistic wrong behavior was challenged, and what remains unproved.

## Semantic currentness

Review is cumulative and semantic:

- a later commit does not invalidate review merely because the SHA changed;
- a finding repair requires checking that finding, its proof, and the changed seam;
- a material change to claim, production route, authority, proof, compatibility, risk,
  or rollback requires review of the affected dimensions;
- formatting, editorial cleanup, generated receipt refresh, and stronger tests do not
  trigger a full review unless they change a substantive conclusion;
- conflict or combined-tree repair receives focused review of the affected interaction.

Do not restart a full deep review or duplicate a still-current review merely to show
activity.

## Routes

- `REVIEW_CURRENT` → `$verify-live-ci`
- `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` → `$address-review-comments`
- weak or non-discriminating proof → `$review-tests` or `$prepare-proof`
- candidate correctness, reachability, ownership, complexity, or rollback uncertainty
  → `$review-candidate` through `$orchestrate-work`
- `REVIEW_SCOPE_CHANGED` → review the affected dimensions; use `$prepare-issue` only
  when the claim or owner changed
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite in the invoking flow
- `SUPERSEDED_OR_CLOSE` → preserve the durable disposition through the invoking flow
- `NOT_PROVEN` / `REVIEW_NOT_PROVEN` → resolve the missing evidence, authority, or
  review instrument