# Real Perl Editor Trust v1 Dashboard

> Human-owned. This dashboard summarizes the current Real Perl Editor Trust
> evidence boundary. It does not select work, generate metrics, broaden live
> provider behavior, or replace provider-specific proof surfaces.

Last reviewed: 2026-05-24.

This page answers:

> Which editor surfaces have enough compiler-fact proof to be trusted live,
> which surfaces are still shadowed, and what proof is required next?

Use this page as an evidence index. Use current GitHub issues/PRs and the linked
status docs to reconstruct the selected concern and its next action.

## Source Stack

| Need | Source |
| --- | --- |
| Normative v1 boundary and promotion discipline | [PLSP-SPEC-0015](../../specs/PLSP-SPEC-0015-real-perl-editor-trust-v1-boundary.md) |
| Provider decision receipt schema and explanation payload contract | [PLSP-SPEC-0016](../../specs/PLSP-SPEC-0016-provider-decision-receipt-v1.md), [provider_decision.v1.schema.json](../../../schemas/provider_decision.v1.schema.json) |
| Shared fact provenance and source-backing semantics | [PLSP-SPEC-0017](../../specs/PLSP-SPEC-0017-fact-provenance-and-source-backing.md) |
| Shared edit authorization states for rename and safe delete | [PLSP-SPEC-0018](../../specs/PLSP-SPEC-0018-edit-authorization-contract.md) |
| Semantic token class promotion rules | [PLSP-SPEC-0019](../../specs/PLSP-SPEC-0019-semantic-token-class-promotion-contract.md), [semantic-token-classes.toml](../../../policy/semantic-token-classes.toml) |
| Workspace symbol generated-label rules | [PLSP-SPEC-0020](../../specs/PLSP-SPEC-0020-workspace-symbol-generated-label-contract.md), [workspace-symbol-classes.toml](../../../policy/workspace-symbol-classes.toml) |
| Module path authority and ambient-input boundaries | [PLSP-SPEC-0022](../../specs/PLSP-SPEC-0022-module-path-authority.md), [PLSP-SPEC-0023](../../specs/PLSP-SPEC-0023-ambient-inputs.md) |
| Determinism receipt v1 planning boundary | [PLSP-SPEC-0026](../../specs/PLSP-SPEC-0026-determinism-receipt-v1.md) |
| Differential real-Perl oracle planning boundary | [PLSP-SPEC-0027](../../specs/PLSP-SPEC-0027-differential-real-perl-oracle.md), [oracle_fixture_manifest.v1.schema.json](../../../schemas/oracle_fixture_manifest.v1.schema.json), [oracle fixture manifest](../../../crates/perl-corpus/fixtures/differential_oracle/manifest.json), [oracle_receipt.v1.schema.json](../../../schemas/oracle_receipt.v1.schema.json) |
| User-facing support claims and known limitations | [SUPPORT_TIERS.md](SUPPORT_TIERS.md) |
| Provider fact source, confidence, freshness, fallback, and next proof | [provider_confidence_matrix.md](provider_confidence_matrix.md) |
| Provider promotion, fallback, blocker, and defer decisions by fact class | [provider_promotion_ledger.md](provider_promotion_ledger.md), [provider-promotion-ledger.toml](../../../policy/provider-promotion-ledger.toml) |
| Provider live/shadow state and cutover rules | [provider_cutover.md](provider_cutover.md) |
| Compiler-backed provider receipts | [semantic_shadow_compare.md](semantic_shadow_compare.md), [semantic_scorecard.md](semantic_scorecard.md) |
| UX/provider capability context | [ux_capability_dashboard.md](ux_capability_dashboard.md) |
| Real-workspace baseline anchors | [2026-05-13 Mojolicious baseline](../../forensics/2026-05-13-real-workspace-baseline-mojolicious.md), [2026-05-14 Dancer2 baseline](../../forensics/2026-05-14-real-workspace-baseline-dancer2.md), [2026-05-19 Catalyst baseline](../../forensics/2026-05-19-real-workspace-baseline-catalyst.md) |
| Historical accepted plans | [Real Perl Editor Trust plan](../../../plans/real-perl-editor-trust/implementation-plan.md), [Editor Trust UX closeout plan](../../../plans/editor-trust-ux-closeout/implementation-plan.md) |
| Live work and remaining claims | Current GitHub umbrella/leaf issues, PRs, reviews, checks, and merge closeouts |

## Provider Trust Loop

The v1 editor loop is:

```text
completion suggests it
hover explains it
definition jumps to it
references finds its uses
diagnostics trusts it
rename / safe-delete know whether it is safe
symbols and tokens expose project shape without noise
explain-provider-decision exposes the receipt boundary
```

The loop is only trusted where each answer can identify its fact source,
confidence, freshness, source-backed range, fallback state, and dynamic-boundary
blocker when relevant.

## Release-Candidate Boundary

Real Perl Editor Trust v1 is now a release-candidate boundary for the current
editor trust surface. It is not a broad provider cutover or a stable/GA product
claim. The boundary freezes the current support posture so future work can use
the promotion ledger instead of widening surfaces by default.

### Live With Fallback

These surfaces may use compiler facts only inside their scoped promotion rules,
and each keeps fallback behavior for unsupported or uncertain cases:

- Completion: partial live visible-symbol support for high-confidence imported
  and exported facts.
- Hover: partial live provenance-backed compiler, framework-adapter, and
  dynamic-boundary explanations.
- Definition, type definition, and references: partial live exact/imported
  source-backed slices with type-definition explanation receipts for the
  existing direct package/class safe subset.
- Diagnostics: partial live suppressions and conservative explanations for
  selected high-confidence semantic evidence.
- Document symbols: partial live source-backed parser-syntax symbols.
- Workspace symbols: partial live source-backed ready-index symbols plus the
  bounded generated-label pilot.
- Semantic tokens: existing parser/HIR output plus narrow source-backed
  compiler-token trace slices that emit no new token output.
- Rename: same-file lexical rename plus the narrow package-local pilot.
- Safe delete: exact unreferenced source-backed subroutine pilot with current
  source, workspace reference, workspace identity, and rollback guards.

### Preview Or Explanation Only

These surfaces are user-facing trust UX, not broader edit authorization:

- `perl.previewPackageRename` exposes planned edits, blockers, fallback state,
  and rollback/no-edit proof without authorizing broad package rename.
- `perl.previewSafeDelete` exposes allowed/blocked no-edit plans when live delete
  proof is incomplete.
- `perl.explainProviderDecision`, diagnostic explanations, missing-module lookup,
  and workspace trust report expose copyable receipts and setup boundaries.
- Workspace trust report setup hints are advisory and derived from existing
  state only; they do not probe Perl, run perldoc, start DAP, or change
  subprocess behavior.

### Blocked Or Deferred

These cases remain blocked, fallback-only, receipt-only, or deferred until a
new row in the promotion ledger names the fact class, promotion rule, fallback
rule, blocker rule, and receipt:

- Broad generated/no-source workspace-symbol promotion.
- Broad compiler-backed semantic-token output.
- Broad package/compiler-backed rename.
- Generated, imported/exported, no-source, dynamic, stale, ambiguous, or
  referenced safe-delete requests.
- Diagnostic correctness claims beyond the scoped explanation and selected
  suppression receipts.
- DAP `includePaths` behavior cutover beyond the current report/config metadata
  boundary.

New compiler facts are substrate only until a provider row promotes one fact
class through the ledger. Receiver-expression facts now have one narrow
source-backed completion pilot; every other receiver class remains substrate,
shadowed, fallback-only, or blocked according to its current evidence.
