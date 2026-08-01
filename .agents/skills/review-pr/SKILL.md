---
name: review-pr
description: Explicit formal review skill for one fixed current PR candidate and material claim after ordinary mutable repair has converged, using GitHub's review interface and allowing a clean no-finding result.
---

# Review PR

## Fixed review-subject boundary

Resolve and record:

```text
full PR head SHA
+ normalized material PR claim/review index digest
```

Compute the digest with:

```text
scripts/reviews/claim-digest --pr <n> [--repo owner/repo]
```

Hold ordinary candidate and material-claim mutation during the judgment.

Before starting the potentially long-running review, post a review-subject-bound in-flight receipt:

```text
scripts/reviews/run review-start --pr <n> --kind <standard|deep> --reviewer <id> --head <full-head-sha> --claim-digest <sha256>
```

Capture the returned `comment_id=<id>` with the review subject. The comment ID is part of the running-review handle; do not rediscover it by marker text.

If the digest, receipt, or returned comment ID cannot be established, return `REVIEW_NOT_PROVEN`; do not leave the formal review invisibly in flight.

Review:

- controlling issue and accepted claim;
- governing contract and proof obligations;
- cumulative candidate and changed production path;
- current `Claim`, establishment/non-goal, risk/rollback, and substantive review-index sections;
- test hardening and simplification;
- applicable vision, authority, claim, external-truth, security, compatibility, and packaging lenses;
- what the evidence establishes and does not establish.

Use Codex native review or a read-only reviewer as an instrument where useful, then publish material conclusions through GitHub's submitted review and inline threads.

A review with no material findings is valid.

After the submitted review is durable, complete the same review subject:

```text
scripts/reviews/run review-done --pr <n> --kind <standard|deep> --reviewer <id> --head <full-head-sha> --claim-digest <sha256> --comment-id <review-start-id>
```

`review-done` reloads and validates that exact running comment before updating it in place. Do not append a second manual marker or select another comment with copied marker text. If the comment ID is lost, the running receipt moved, or validation/update fails, return `REVIEW_NOT_PROVEN` until the durable in-flight state is reconciled.

If the material claim changed during review, complete or retire the old subject honestly and return `CLAIM_REVIEW_STALE`; `finish-pr` reruns the bounded final challenge before starting formal review against the new subject.

## Review record

```text
Reviewed candidate: <full head SHA>
Reviewed claim digest: <sha256>
Review receipt comment: <GitHub issue-comment ID>
Reviewed claim summary
Review lenses used
REVIEW_CURRENT | REVIEW_FINDINGS_OPEN | REVIEW_NOT_PROVEN
Material findings with evidence
What the review establishes
What remains unproved
```

Do not mutate during the formal judgment. Any later head or material claim/review-index change requires a fresh formal review; supporting proof and specialist lenses rerun proportionately to changed semantic subjects.

## Routes

- `REVIEW_CURRENT` → `$verify-live-ci`
- `REVIEW_FINDINGS_OPEN` → `$address-review-comments`
- `CLAIM_REVIEW_STALE` → `$final-challenge`, then `$review-pr`
- `REVIEW_NOT_PROVEN` → resolve candidate identity, claim identity, missing evidence, or review-receipt instrument failure
