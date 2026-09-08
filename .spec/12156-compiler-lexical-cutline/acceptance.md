# Acceptance: #12156 — compiler lexical cut-line manifest

Maps the issue's acceptance bullets to the landed artifacts and their checks.

- [x] **One exact manifest defines the first compiler lexical request/edit
  class and every adjacent exclusion.**
  `manifest.json` (`compiler_lexical_cutline_cases.v1`): 18 admitted rows, 15
  excluded rows, 10 lifecycle rows. Validator enforces the full required
  positive and exclusion coverage tag sets (`REQUIRED_POSITIVE_COVERAGE`,
  `REQUIRED_EXCLUSION_COVERAGE`); deleting any class fails validation.

- [x] **Exact anchors, roles, nonempty, exact empty, shadowing, sigils,
  lifecycle, authorization, preparation correlation, rename plan, projection,
  application, and requery are non-vacuously covered.**
  Every admitted row carries a hand-authored compiler-owned declaration anchor
  plus exact reference locations with roles; geometry is machine-verified
  against pinned fixture bytes. Rename rows carry authorization, plan,
  projection, and applied ID sets with enforced set identity plus a
  machine-reproduced postcondition.

- [x] **Standard LSP prepare/rename shapes remain unextended and no
  client-carried plan token is assumed.**
  `protocol_lifecycle` pins the exact 3.18 shapes and
  `client_carried_continuation_token: forbidden`; the validator rejects
  unknown fields on the protocol block (falsifier: a `preparation_token`
  field fails).

- [x] **No-prepare, matching-prepare, stale-prior-prepare/fresh-success, and
  stale-prior-prepare/current-refusal rows are distinct.**
  LX-RN-001, LX-RN-002/LX-LC-002, LX-LC-003, LX-LC-004; the validator
  requires every preparation scenario to have at least one row.

- [x] **Expected anchors/references/occurrences/authorization/plans/wire
  edits/applied edits are independent of candidate and legacy output.**
  All expectations are hand-authored literals; fixtures are SHA-256 pinned;
  anchor text, UTF-16 geometry, and postconditions are machine-reproduced
  from the fixtures, never captured from any implementation.

- [x] **Work assertions cover anchor production, contribution
  construction/sharing, provider reconstruction, result union, duplicate
  semantic/text work, rename reconstruction, stale-plan reuse, exact
  projection, and fallback/heuristic work.**
  WI-01..WI-18; zero/false/identity claims must name an instrument; the final
  #4306 old-work zero stays `pending` and pending is not zero.

- [x] **Legacy-assisted anchors, request-time rebuilds, provider-private
  rename plans, subset/superset edits, fictitious protocol continuations,
  false stale refusal, and false zeroes are independently caught.**
  37 controlled mutations with enforced bidirectional row ownership; 33
  validator tests prove each falsifier class fails for the intended reason
  (forged anchors, UTF-16 drift, ID subset/superset, continuation tokens,
  old-plan reuse, prior-preparation authorization, false zeros, noncanonical
  bytes, and more).

## Proof commands

```bash
cargo xtask compiler-lexical-cutline validate
cargo xtask compiler-lexical-cutline list
cargo xtask compiler-lexical-cutline explain LX-RN-001
cargo test -p xtask --locked --test compiler_lexical_cutline
```

## Claim boundary

No product behavior: the diff touches only `.spec/`, `schemas/`, and the
`xtask` validator/CLI/tests. Zero `src/` logic edits outside the
validator/manifest surface.
