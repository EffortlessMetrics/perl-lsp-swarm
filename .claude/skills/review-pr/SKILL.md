---
name: review-pr
description: Formally review one fixed current PR candidate and material claim through GitHub after mutable repair converges. A clean no-finding result is valid.
user-invocable: false
---

# Review PR

Resolve the full candidate head and compute the normalized material claim/review-index digest with `scripts/reviews/claim-digest --pr <n> [--repo owner/repo]`.

Hold ordinary candidate and material-claim mutation during judgment. Before review, run:

```text
scripts/reviews/run review-start --pr <n> --kind <standard|deep> --reviewer <id> --head <sha> --claim-digest <sha256>
```

Capture the returned `comment_id=<id>` with the fixed review subject. Failure to establish the digest, running receipt, or its comment ID is `REVIEW_NOT_PROVEN`.

Review the issue, claim, contract, proof, cumulative diff, production path, current material PR claim sections, hardening, simplification, and applicable lenses. The judgment is directed at the applicable quality questions, falsifying of realistic wrong behavior, and verified through current evidence or competent authority. Publish material findings through GitHub review and threads. A clean review is valid.

After the submitted review is durable, run:

```text
scripts/reviews/run review-done --pr <n> --kind <standard|deep> --reviewer <id> --head <sha> --claim-digest <sha256> --comment-id <review-start-id>
```

`review-done` reloads and validates that exact running comment before updating it in place. Do not append a separate manual marker or rediscover the receipt by marker text. A lost/moved comment ID or failed validation/update is `REVIEW_NOT_PROVEN` until durable state is reconciled.

If the material claim changed during review, complete or retire the old subject honestly and return `CLAIM_REVIEW_STALE`; `finish-pr` reruns the bounded final challenge before starting formal review against the new subject.

Do not mutate during formal judgment. Any later head or material claim/review-index change requires a fresh formal review, with supporting proof/lenses rerun proportionately.

## Routes

- `REVIEW_CURRENT` → `verify-live-ci`
- `REVIEW_FINDINGS_OPEN` → `address-review-comments`
- `CLAIM_REVIEW_STALE` → `final-challenge`, then `review-pr`
- `REVIEW_NOT_PROVEN` → resolve candidate identity, claim identity, evidence, or review-receipt failure
