# Acceptance: #10794 — query profile substrate and current-behavior parity

## Parity matrix

For each live path (P1 full source-backed, P2 candidate-restricted,
P3 open-document full, P4 generated/framework), membership and order must be
byte-identical to `main@ab3cece9d` behavior for:

| dimension | parity rule |
| --- | --- |
| empty query | P1/P2/P3 admit everything; P4 returns empty; per-path ordering preserved |
| whitespace-only query | identical to empty after trim, per path |
| one-char exact / prefix | admitted on every path that admits them today |
| one-char substring/subsequence | rejected wherever rejected today (loose gate) |
| two-char substring/subsequence | admitted exactly where admitted today (P4 never subsequence) |
| case handling | folded with `to_lowercase` only; distinct-case packages stay separate buckets in P1 |
| 'İ' expansion | loose eligibility measured on folded char count (WS-QP-009) |
| combining marks | no normalization: e-then-U+0301 never equals é (WS-QP-010) |
| non-match | `None`; structurally distinct from subsequence (WS-QP-011 + mutation) |
| tie-breaks | provider: tier→len→lex; index: rank→lex; generated: name/uri/range — all unchanged |

Any externally visible difference requires a separately named correction
fixture in this packet; none is planned.

## Profile-mismatch semantics

- Evidence carries the compiling profile's version + digest.
- `validate_candidate_evidence(profile, evidence)` rejects evidence whose
  digest/version differ from the request profile with a typed
  `ProfileMismatch` outcome; it never downgrades to a weaker tier and never
  treats the candidate as complete (`ProvenSuperset`-shaped exactness is
  unreachable from mismatched evidence) — WS-QP-013.
- Full and accelerated paths of one logical request receive the *same*
  compiled profile/digest — WS-QP-014.

## Profile contract checks

- One compile per logical request: repeated compilation of equal raw input is
  byte-identical (fields + digest); the handler compiles once and passes it to
  both index paths.
- Digest changes when any admitted policy proposition changes: version,
  policy id, folded bytes, threshold disposition, browse disposition
  (WS-QP-012 via versioned digest derivation).
- Profile retains: version, policy id, raw, trimmed, folded, folded char
  count, browse disposition, loose-tier eligibility.

## Stable cases

WS-QP-001…WS-QP-014 as listed in issue #10794. Implemented as Rust tests under
the owning module plus migrated-path regression tests; IDs appear in test
names. No Gherkin/spec-ledger tags are touched, so the BDD/ac-status/docs-check
chain is not triggered.

## Mutation controls (must fail)

M1 fallback conflation restored (non-match representable as subsequence tier);
M2 per-candidate independent lowercase/compile on canonical path;
M3 local matcher/threshold outside canonical owner re-created;
M4 one-character prefix-only behavior changed;
M5 'İ' expansion fixture removed;
M6 silent Unicode normalization introduced;
M7 full vs accelerated digests diverge;
M8 stale/mismatched candidate accepted as exact;
M9 membership/order change without correction fixture;
M10 LSP wire types or entity identity imported into the primitive;
M11 query policy moved back into `perl-symbol`;
M12 instrument failure reported as zero/pass.

Seeded first: M1, M8 (profile-mismatch), and WS-QP-011 red case precede any
production edit. M2/M3/M10/M11 are enforced statically by new layer rules +
forwarding-only shims; M4–M7, M9 by fixtures; M12 by receipt semantics
(missing counters are `not_proven`, never zero).

## Work counters

Bounded receipt exposes at least: profiles compiled per request, keys
examined, matches by tier, nonmatches, full-vs-accelerated profile equality,
profile-mismatch rejections, normalization limitations note. Absence of a
counter is reported as `not_proven`.

Live instrumentation status (repair review): `WorkspaceSymbolQueryWork` /
`WorkspaceSymbolQueryWorkReceipt` are owner-level types with no production
caller yet, so no live counter is asserted. What IS wired live today: the
`workspace/symbol` v2 canonical index request emits its compiled
`query_profile_version` + `query_profile_digest` on its decision trace.
`profiles_compiled_per_request == 1` is **not_proven** live: it holds
structurally on the v2 index path (one compiled instance feeds both index
tiers), but provider entry points (`search`, `search_with_candidates`) still
take `&str` and compile internally, so degraded/open-document requests may
compile more than once per logical request. Per M12 semantics this is
recorded `not_proven`, never zero.

## Repair-review addendum (post-CHANGES_REQUIRED)

- Inventory defect fixed: open-document text-fallback matcher site 7
  (`fallback/text.rs:178-201`) recorded in context.md with disposition —
  inventoried with handoff to #10645/#10642, not migrated this slice (parity:
  untrimmed/ungated contains-only admission diverges from all admitted tier
  combinations; migration requires named correction fixtures out of Q01
  scope).
- `symbol_query::compare_names_by_query` deleted instead of kept alive by its
  own tests: zero production callers after the P2/P3 evidence-sort migration,
  and its legacy-slot mapping was divergent for loose-ineligible queries
  (spec rule: removed or forwarding-only with exact exits). Ordering coverage
  remains at the canonical owner comparator tests.
- Digest field list in checklist.md corrected: browse flag and loose
  eligibility are functions of the folded bytes/version/policy id and are not
  separate digest inputs.

## Handoffs

- #10645 consumes `match_searchable_key` evidence + comparator without
  re-tokenizing; key roles already typed.
- #10642 receives evidence/profile digest through #10645.
- Q02 (#10806)/Q03 (#10827) extend the profile/policy id/evidence without a
  second owner; version bump invalidates old digests.
