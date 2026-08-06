---
name: review-pr
description: Review one pull request cumulatively through GitHub, publishing useful findings or a useful clean conclusion and explicit merge posture without exact-head or claim-hash receipt ceremony.
---

# Review PR

Apply [`docs/agents/PR_REVIEW_STANDARD.md`](../../../docs/agents/PR_REVIEW_STANDARD.md)
to the selected pull request.

## Review subject

Review the current cumulative PR against:

- the controlling issue, accepted claim, and explicit non-goals;
- the governing contract and competent external authority where applicable;
- the changed production or operational path and its live callers/consumers;
- the proof, negative controls, fixtures, generated artifacts, and limitations;
- prior findings and evidence-backed dispositions;
- compatibility, security, persistence, packaging, migration, support, release, and
  rollback boundaries that actually apply;
- live GitHub review, thread, check, draft, mergeability, ruleset, and prerequisite
  facts.

The PR head identifies the code currently visible on GitHub. It is **not** a
review-validity token. Do not compute a material-claim digest, post `review-start` /
`review-done` receipts, or add comments that merely repeat a head SHA and hash.

## Required review procedure

1. Reconstruct the candidate and evidence map: claim/non-goals, authority, cumulative
   changed seams, live consumers, proof/limitations, prior finding dispositions, and
   current GitHub facts.
2. Trace production reachability. A compiled component, public setter, adapter, or
   fixture is not system proof unless the real request, packaging, installer, release,
   or runtime path consumes it.
3. Falsify proof and evidence integrity. Check realistic wrong implementations,
   negative/stale/failure controls, schema-validator agreement, derived rather than
   self-attested evidence, authoritative artifact identity, and whether hosted proof
   actually exercised the claimed path.
4. Verify external and semantic truth and the correct owner. Reject duplicate
   authority, private competing schemas, unreachable scaffold, and compatibility
   residue without a bounded purpose.
5. Challenge claim honesty, complexity, risk, and rollback. Keep conclusions inside
   the evidence boundary and prevent rejection, fallback, limitation, or partial
   implementation from hiding a condition the contract says must block.
6. Classify live checks, threads, draft state, mergeability, and prerequisites without
   treating any of them as substantive review.
7. Publish material findings through GitHub reviews and inline threads, or publish a
   useful clean conclusion. Record one merge posture:

```text
READY_FOR_INTEGRATION
CHANGES_REQUIRED
NOT_PROVEN
IN_FLIGHT
BLOCKED_BY_PREREQUISITE
SUPERSEDED_OR_CLOSE
```

Green CI, `mergeable: true`, zero open threads, or a clean bot review do not
independently imply `READY_FOR_INTEGRATION`.

Use native Codex review, a read-only reviewer, an external oracle, or direct inspection
when that changes the evidence surface. Identity separation alone is neither required
nor sufficient. A clean review is valid.

## Useful review record

Submit a review that helps a fresh agent continue:

```markdown
## Review scope
- Claim, changed seams, live consumers, prior findings, and applicable risk reviewed

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

## Merge posture
- READY_FOR_INTEGRATION | CHANGES_REQUIRED | NOT_PROVEN | IN_FLIGHT |
  BLOCKED_BY_PREREQUISITE | SUPERSEDED_OR_CLOSE

## Next action
- Repair, focused re-review, current integration proof, merge path, or named follow-up
```

Do not submit only `LGTM`, `review complete`, reviewer identity, a head SHA, a claim
digest, a check summary, or a status line. Do not duplicate a still-current review
merely to make activity visible.

## Related PR synthesis

When this PR belongs to a bounded related set, review it individually first. The goal
root may then summarize each PR's candidate identity, hosted/current proof,
substantive result, merge posture, explicit prerequisite, and correct repair/merge
order.

Check parent/child schema, identity, authority, status, limitation propagation, and
artifact-set contracts. A fan-in must load and validate child evidence; it cannot
become ready from copied summaries while child contracts remain unproved. The
synthesis is not batch approval and does not replace each PR's submitted review.

## Semantic currentness

Review is cumulative and semantic:

- a later commit does **not** invalidate review merely because the SHA changed;
- a finding repair requires checking the affected finding, proof, and changed seam;
- a material change to the claim, production route, authority, risk, rollback, or
  proof requires review of the affected dimensions;
- formatting, editorial cleanup, generated receipt refresh, or test strengthening
  does not trigger a full review unless it changes a substantive conclusion;
- an actual conflict resolution or integration repair receives focused review of the
  repaired seam.

Do not restart a full `deep` review after every push.

## Routes

- `READY_FOR_INTEGRATION` / `REVIEW_CURRENT` → `$verify-live-ci`
- `CHANGES_REQUIRED` / `REVIEW_FINDINGS_OPEN` → `$address-review-comments`
- `REVIEW_SCOPE_CHANGED` → review the affected dimensions; route to `$prepare-issue`
  only when the claim or owner must change
- `IN_FLIGHT` → return the named GitHub-owned transition to the invoking flow
- `BLOCKED_BY_PREREQUISITE` → preserve the exact prerequisite and return to the
  invoking flow
- `SUPERSEDED_OR_CLOSE` → preserve the durable disposition through the invoking flow
- `NOT_PROVEN` / `REVIEW_NOT_PROVEN` → resolve the missing evidence, authority, or
  review instrument
