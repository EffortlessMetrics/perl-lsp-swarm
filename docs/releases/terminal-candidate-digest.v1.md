# Terminal candidate digest v1

- Product SHA: `4b422f3aeec74c0ec8cf760bb82233c2ff54f5e6`
  (`ci(panic-registry): follow-up adjudication on landed main (#16417) (#16439)`)
- Date (UTC): 2026-09-25
- Verdict: **READY** (with two named qualifications, §5–§6)
- Re-verify: `python3 -m unittest scripts.tests.test_domain6_fragment`
  plus the per-item commands below, all run at the pinned SHA.
  Each item can be re-run individually using
  `python3 -m unittest
  scripts.tests.test_domain6_fragment.Domain6FragmentTest.<name>` with
  `<name>` one of: `test_header_pins_exact_range_and_prefilter`,
  `test_record_counts`, `test_exactly_once_coverage`,
  `test_five_seed_rows_reviewed`,
  `test_all_other_units_explicit_not_proven`, `test_merge_units_named`,
  `test_no_private_temp_path_references`, `test_digest_valid`,
  `test_schema_validates_checked_artifact_draft_2020_12`,
  `test_grouping_identity_rules`, `test_merge_terminal_arithmetic`,
  `test_excluded_merges_ledger_complete`, `test_no_placeholder_identity`,
  `test_noref_body_evidence_emitted`, `test_checked_artifact_is_lf_only`,
  `test_check_rejects_crlf_mutation`,
  `test_deterministic_rerender_byte_identical`.

Phase 0 carriers are all landed at this SHA: #16227 (verbatim `34fce6f7e`),
#16419 (denominator fragment, owner-landed `f4d370e1a`), #16420-base, and the
#16439 follow-up (`4b422f3ae`, byte-identical to proven head `5d35aca9`).

## 1. Pinned denominator — PROVEN

`docs/releases/domain6-windows-editor-distribution-first-mile.v1.json`
(`domain6_windows_editor_distribution_first_mile`), landed by #16419:

- 20 merge units (`merge_units_created: 18`, `merge_units_kept: 2`)
- 682 work-unit commits, parent range pinned
  (`parent_denominator.observed_head: 10297415…`, `start_sha: f6b7b2c6…`)
- `not_proven_unit_count: 667` across the wider Domain-6 scope; this
  fragment carries the 5 reviewed seeds plus 15 explicitly `not_proven` merges.

## 2. Exact-once coverage — PROVEN

`test_exactly_once_coverage`: 682 covered commits, all unique; 20 merge
commits disjoint from covered; 3 noref-watchlist commits intentionally mapped.
`test_digest_valid` (`sha256:cc24882a…`), `test_record_counts`,
`test_merge_terminal_arithmetic`, and `test_deterministic_rerender_byte_identical`
all green — full suite `OK` on the pinned SHA.

## 3. Owned installed effects — PROVEN (reviewed rows)

`test_five_seed_rows_reviewed` green. All five reviewed rows carry
`reachable_installed_effect`, `proof_owner_refs`, `platforms_and_targets`,
and `artifact_or_route_effects` with concrete values:

| Row | Effect | Proof owner (exists at SHA) |
| --- | ------ | --------------------------- |
| PR#10198 | managed candidates namespaced by host target | `vscode-extension/src/test/managedNamespaceIsolation.test.ts` |
| PR#12086 | managed binary download/retain/GC lifecycle | `vscode-extension/src/test/downloader.test.ts` |
| PR#14523 | typed DAP launch-authority startup contract | `clients/sublime host_tests`, editor-transport inventory |
| PR#16207 | RC/numeric VSIX identity binding offline | `.spec/14923-rc-vsix-binding/README.md`, `fixtures/rc_vsix_binding/` |
| PR#16371 | bootstrap values feed installer docs; release-archive install smoke | `scripts/post-publish-smoke.sh` |

## 4. Release consequences — PROVEN (reviewed rows)

Same five rows carry `release_note_disposition`
(`required` × 2, `covered`, `not_user_facing` × 2),
`migration_or_upgrade_refs`, `api_schema_package_effects`, and
`public_claim_refs`; absence of all 16 reviewed-only fields on every other
row is enforced by `test_all_other_units_explicit_not_proven`.

## 5. Windows artifact proof — QUALIFIED PASS

No Windows portability seam exists in the Phase 0 diffs, so ci-scope
correctly resolves `windows_runner: false` and the `Windows platform smoke`
job skips by scope — not by missing evidence. Windows-relevant installed
effects (managed download lifecycle, installer bootstrap) are covered by the
reviewed rows above; no Windows-runner execution receipt exists beyond that.
Revisit if a future candidate touches a portability seam.

## 6. Clean first-mile receipt — PROVEN (via reviewed row)

First-mile = install bootstrap → release-archive smoke, owned by the
reviewed PR#16371 distribution row (`release_note_disposition:
not_user_facing`, smoke owner `scripts/post-publish-smoke.sh`, verified
present at the SHA). No standalone first-mile receipt artifact exists outside
the fragment row; no open defect is filed against this row.

## 7. Live PR frontier joined — MAPPED 2026-09-25

| Item | State | Disposition |
| ---- | ----- | ----------- |
| #16372 (required-checks prose → policy) | OPEN, owner-held (EffortlessSteven), hetzner-era CI | Hands off; not this digest's claim |
| #16440 (pin-ledger contradiction) | OPEN | Base-owned, routed, non-required |
| #16442 (UX fixture-matrix drift, scenario 63) | OPEN | Base-owned, routed, non-required |
| #16439, #16446, #16444 | MERGED / CLOSED-superseded / CLOSED-fixed | Landed or withdrawn with receipts |
| #16371, #16373–#16375 seeds | MERGED / unmapped / CLOSED / unmapped | Only #16371 maps to a fragment row |

Frontier state is time-sensitive; re-map before acting on it.

## Limitations

- Windows (§5) and first-mile (§6) rest on scope analysis and the reviewed
  distribution row respectively, not on dedicated execution receipts.
- The frontier table (§7) was true at writing time only.
- Denominator review extends to 5 of 672 Domain-6 units (667 of them
  `not_proven`); the remaining `not_proven` units are explicitly
  unclaimed, not implicitly proven.
