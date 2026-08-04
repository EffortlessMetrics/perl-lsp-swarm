---
name: review-pr
description: Review one pull request cumulatively through GitHub, publishing useful findings or a concise clean conclusion without exact-head or claim-hash receipt ceremony.
user-invocable: false
---

# Review PR

Review the current cumulative pull request against its controlling issue, accepted claim, changed production path, proof, and applicable compatibility, security, packaging, migration, support, and rollback boundaries.

The PR head identifies the code currently visible on GitHub. It is not a review-validity token. Do not compute a material-claim digest, call `scripts/reviews/run review-start|review-done`, or post comments that merely repeat a head SHA and claim hash.

Review is directed, falsifying, and verified. Seek realistic counterexamples and check claim honesty, semantic and external correctness, proof discrimination, production reachability, negative/fallback behavior, authority, complexity, compatibility, rollback, and uncertainty where applicable. Use native Claude review, a focused read-only reviewer, an external oracle, or direct inspection when that changes the evidence surface. A clean review is valid.

Publish material findings through GitHub submitted reviews and inline threads. For a clean result, leave a concise conclusion that tells the next session what was checked and what remains unproved.

Review currentness is semantic:

- a later commit does not invalidate review merely because the SHA changed;
- a finding repair requires checking the affected finding, proof, and changed seam;
- a material change to claim, production route, authority, risk, rollback, or proof requires review of the affected dimensions;
- formatting, editorial cleanup, generated receipt refresh, or test strengthening does not trigger a full review unless it changes a substantive conclusion;
- conflict or integration repair receives focused review of the repaired seam.

Do not restart a full `deep` review after every push. Broaden only when the later change materially broadens the review subject.

## Routes

- `REVIEW_CURRENT` → `verify-live-ci`
- `REVIEW_FINDINGS_OPEN` → `address-review-comments`
- `REVIEW_SCOPE_CHANGED` → review the affected dimensions; route to `prepare-issue` only when the claim or owner must change
- `REVIEW_NOT_PROVEN` → resolve missing evidence, authority, or review instrumentation
