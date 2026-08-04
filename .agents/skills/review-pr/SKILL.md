---
name: review-pr
description: Review one pull request cumulatively through GitHub, publishing useful findings or a useful clean conclusion without exact-head or claim-hash receipt ceremony.
---

# Review PR

## Review subject

Review the current cumulative pull request against:

- the controlling issue and accepted claim;
- the changed production or operational path;
- the proof and negative controls;
- compatibility, security, packaging, migration, support, and rollback boundaries that actually apply.

The PR head identifies the code currently visible on GitHub. It is **not** a review-validity token. Do not compute a material-claim digest, post `review-start` / `review-done` receipts, or add comments that merely repeat a head SHA and hash.

## Review method

Review is directed, falsifying, and verified. Seek realistic counterexamples and check, where applicable:

- claim honesty;
- semantic and external correctness;
- proof discrimination;
- production-path reachability;
- negative, fallback, stale, and refusal behavior;
- authority, complexity, compatibility, and rollback;
- remaining uncertainty.

Use native Codex review, a read-only reviewer, an external oracle, or direct inspection when that changes the evidence surface. Identity separation alone is neither required nor sufficient.

Publish material findings through GitHub submitted reviews and inline threads. A clean review is valid; it still records what was examined, what evidence or authority was used, what realistic wrong behavior was challenged, and what remains unproved.

## Semantic currentness

Review is cumulative and semantic:

- a later commit does **not** invalidate review merely because the SHA changed;
- a finding repair requires checking the affected finding, proof, and changed seam;
- a material change to the claim, production route, authority, risk, rollback, or proof requires review of the affected dimensions;
- formatting, editorial cleanup, generated receipt refresh, or test strengthening does not trigger a full review unless it changes a substantive conclusion;
- an actual conflict resolution or integration repair receives focused review of the repaired seam.

Do not restart a full `deep` review after every push. Broaden only when the later change materially broadens the review subject.

## Useful review record

Submit a review that helps a fresh agent continue without reconstructing your judgment:

```markdown
## Review scope
- Claim, changed seams, consumers, and prior findings examined

## Evidence and falsifiers
- Commands, tests, sources, or authorities used
- Realistic wrong behavior challenged

## Findings
- Material findings with severity and evidence

<!-- Or: ## No material findings -->

## Prior finding dispositions
- fixed | refuted | superseded | follow-up, with evidence

## What this establishes
- Conclusions supported by the review

## Residual risk / not proved
- Local uncertainty and excluded surfaces

## Next action
- Repair, focused re-review, merge path, or named follow-up
```

Do not submit only `LGTM`, `review complete`, reviewer identity, a head SHA, a claim digest, or a status line. No public review receipt comment or claim hash is required.

## Routes

- `REVIEW_CURRENT` → `$verify-live-ci`
- `REVIEW_FINDINGS_OPEN` → `$address-review-comments`
- `REVIEW_SCOPE_CHANGED` → review the affected dimensions; route to `$prepare-issue` only when the claim or owner must change
- `REVIEW_NOT_PROVEN` → resolve the missing evidence, authority, or review instrument