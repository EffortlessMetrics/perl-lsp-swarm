# PLSP-SPEC-0027: Differential oracle contract

Status: accepted
Owner: perl-lsp maintainers
Linked proposal: [PLSP-PROP-0001](../proposals/PLSP-PROP-0001-real-perl-editor-trust.md)
Linked specs:
- [PLSP-SPEC-0015](PLSP-SPEC-0015-real-perl-editor-trust-v1-boundary.md)
- [PLSP-SPEC-0017](PLSP-SPEC-0017-fact-provenance-and-source-backing.md)
- [PLSP-SPEC-0022](PLSP-SPEC-0022-module-path-authority.md)
- [PLSP-SPEC-0023](PLSP-SPEC-0023-ambient-inputs.md)
- [PLSP-SPEC-0025](PLSP-SPEC-0025-pir-v0.md)
- [PLSP-SPEC-0026](PLSP-SPEC-0026-determinism-receipt-v1.md)
Linked ADRs:
- [PLSP-ADR-0002](../adr/PLSP-ADR-0002-confidence-before-cutover.md)
Linked plan: [Real Perl Editor Trust implementation plan](../../plans/real-perl-editor-trust/implementation-plan.md)
Status impact: compiler facts, parser status, real-workspace receipts,
provider confidence matrix, support tiers, future determinism receipts

## Current Implementation Status

Differential oracle work is currently a planning contract. Existing status rows
already treat real Perl and `perldoc` as helper/oracle seams, and
[compiler_facts.md](../project/status/compiler_facts.md) records this
contract-defined boundary while keeping broader conformance receipts planned.

This spec defines how future oracle receipts may compare Rust-native compiler
facts, parser output, module resolution, PIR lowering, or provider receipts
against explicitly bounded external oracles. It does not add oracle execution,
workspace probing, provider behavior, parser bucket movement, support-tier
promotion, release-lineage sync, or a determinism claim.

Current evidence remains in:

- [compiler fact substrate](../project/status/compiler_facts.md)
- [Real Perl Editor Trust dashboard](../project/status/real_perl_editor_trust_v1.md)
- [module resolution status](../project/status/module_resolution.md)
- [provider confidence matrix](../project/status/provider_confidence_matrix.md)
- [support tiers](../project/status/SUPPORT_TIERS.md)
- [active goal manifest](../../.perl-lsp/goals/active.toml)

Current next work is not stored here; see the routing dashboard and active goal.

## Contract

A differential oracle is an explicitly configured comparison surface. It can
help maintainers understand whether Rust-native compiler/editor facts agree
with an external Perl-facing observation, but it is not a source of truth and
is not an editor-runtime dependency.

Differential oracle receipts must:

- identify the oracle kind and authority
- identify the source snapshot, fixture, or workspace baseline being compared
- identify the Rust-native fact, parser, PIR, resolver, or provider receipt under
  comparison
- declare the oracle command or fixture input without exposing secrets
- declare module-path authority and ambient-input policy
- declare subprocess environment allow/deny behavior
- declare timeout, resource, and failure policy
- report oracle availability, skipped state, or failure state explicitly
- compare only the scoped observation named by the receipt
- preserve provenance, confidence, freshness, source-backing, fallback, and
  blocker state from the Rust-native side
- state whether provider behavior changed

Differential oracle receipts must not:

- run from ordinary editor provider requests
- run from explanation-only, workspace-trust-report, or report-only commands
- silently inherit ambient `PERL5LIB`, `PERL5OPT`, startup `@INC`, local::lib, DAP
  launch metadata, perldoc state, or user shell state
- treat oracle output as workspace source
- suppress diagnostics, authorize edits, promote generated symbols, promote
  semantic-token classes, or broaden completion/navigation from oracle agreement
  alone
- claim parser bucket movement without a fresh generated parser/corpus receipt
- imply full CPAN support, full static Perl support, or deterministic Perl
  behavior

## Oracle Classes

Every oracle receipt must name one oracle class:

| Oracle class | Examples | Authority |
| --- | --- | --- |
| ParserComparison | parse the same source with Rust parser and a bounded external parser/oracle | Comparison evidence only; no bucket movement without generated parser status. |
| CompileCheck | `perl -c` or equivalent syntax/compile check over an explicit fixture | Differential syntax evidence only; no provider behavior. |
| ModuleResolutionOracle | compare resolver output with an explicit Perl include-path command | Module-path comparison only; ambient roots must be labeled. |
| PerldocOracle | perldoc lookup/configuration availability | Documentation/helper boundary, not compiler truth. |
| RuntimeProbe | explicitly bounded tiny Perl snippet for a narrow behavior question | Test-only oracle; never editor-runtime dependency. |
| RealWorkspaceBaseline | CPAN-style project fixture/baseline with declared setup | Livability evidence with known limits, not all-CPAN proof. |
| ExternalCorpus | generated corpus or upstream fixture set | Measurement evidence only; freshness and generator provenance required. |

Unknown, unavailable, skipped, timed-out, or unclassified oracle classes must be
reported as fallback, unsupported, or unknown. They must not be silently treated
as evidence of agreement.

## Required Receipt Fields

Future implementations may choose exact Rust and JSON names, but each
differential oracle receipt must expose these semantic fields:

```text
schema_version
oracle_class
oracle_authority
surface_under_test
source_snapshot
workspace_or_fixture_identity
rust_fact_or_receipt
oracle_command_class
module_path_authority
ambient_policy
subprocess_boundary
timeout_policy
resource_policy
oracle_status
comparison_result
agreement_scope
divergence_summary
fallback
blockers
unknowns
provider_behavior_changed
support_claim_changed
proof_commands
user_message
copyable_payload
```

`oracle_status` must be one of:

```text
available
skipped
unavailable
timed_out
failed
unsupported
unknown
```

`comparison_result` must be one of:

```text
agrees
diverges
partial
not_comparable
not_run
unknown
```

`agrees` is allowed only for the exact scoped observation named by the receipt.
It must not be generalized to the provider, project, parser bucket, CPAN, or
Perl language.

## Execution Boundaries

Oracle execution is allowed only in explicit proof commands, tests, or
opt-in receipt generation. It must not be triggered by:

- live completion, hover, goto, references, diagnostics, symbols, semantic
  tokens, rename, or safe-delete requests
- provider decision explanation commands
- workspace trust report commands
- missing-module explanation commands
- preview rename or preview safe-delete commands

Oracle subprocesses must declare:

- executable identity or class
- inherited environment policy
- explicit supplied environment
- module path policy
- timeout
- working directory class
- redaction policy
- failure and skip policy

If the oracle cannot run under the declared boundary, the receipt must record
`oracle_status = unavailable`, `skipped`, `failed`, `timed_out`, or `unknown`
instead of silently weakening the comparison.

## Valid PR Shapes

Valid PRs under this spec include:

- docs PRs that define oracle classes and claim boundaries
- schema PRs that add a differential oracle receipt shape without behavior
- validator PRs that check oracle receipt fields, redaction, links, or required
  boundaries
- fixture PRs that compare one source-backed fixture against an explicit oracle
- parser/corpus PRs that consume oracle evidence and update generated status
  through the generator
- real-workspace receipt PRs that record setup, skipped, unavailable, or
  divergence states without broad support claims
- module-resolution PRs that compare a configured resolver case against an
  explicitly bounded include-path oracle

Every valid PR must say whether it changes docs, schemas, validators, fixture
receipts, parser/runtime behavior, module-resolution behavior, provider
behavior, subprocess seams, generated status, support claims, or release-lineage
sync.

## Invalid PR Shapes

Invalid PRs include:

- adding real Perl or `perldoc` as an editor-runtime dependency
- running oracles from live provider or explanation/report commands
- silently inheriting ambient environment into oracle proof
- treating oracle output as workspace source
- claiming parser bucket movement without fresh generated parser status
- claiming provider promotion from oracle agreement alone
- authorizing rename or safe delete from oracle agreement
- suppressing diagnostics from oracle agreement without provider-specific proof
- exposing raw secrets, private environment values, unnecessary raw paths, or
  launch payloads
- bundling oracle work with unrelated parser, provider, refactor, release, or
  source-sync changes

## Acceptance

A PR satisfies this spec when:

- the touched oracle class is named and scoped
- oracle authority and execution boundary are explicit
- module-path and ambient inputs are labeled
- unavailable, skipped, failed, timed-out, partial, and divergent states are
  visible
- Rust-native provenance, confidence, freshness, source-backing, fallback, and
  blocker state remain canonical
- generated parser/status changes, when present, come from the generator and
  include freshness evidence
- provider behavior and support claims remain unchanged unless a separate
  promotion row and receipt explicitly allow them
- user-facing text explains what was compared, what was not run, what diverged,
  and what cannot be claimed

## Proof Commands

Docs-only PRs for this spec may use:

```bash
rtk cargo xtask check-active-goal-manifest
rtk cargo xtask ci-hygiene check-doc-paths docs/specs
rtk cargo xtask ci-hygiene check-doc-paths docs/project/status
rtk cargo xtask check-support-claims
rtk cargo xtask check-provider-confidence-matrix
rtk git diff --check
```

Schema or validator PRs must also run focused schema/validator tests. Parser,
corpus, module-resolution, PIR, provider, subprocess, or real-workspace oracle
PRs must also run the focused tests and status generators for the changed
surface.

## Non-goals

- No oracle runner from this spec alone.
- No real Perl, `perldoc`, DAP, or application execution.
- No workspace probing.
- No provider behavior change.
- No parser bucket movement.
- No support-tier promotion.
- No release-lineage sync.
- No full CPAN, full static Perl, runtime conformance, or deterministic
  Perl-language claim.

## Claim Boundaries

This spec may claim that differential oracle receipts must identify the oracle,
execution boundary, module-path authority, ambient policy, comparison scope,
fallbacks, blockers, and divergences. It may not claim that oracle receipts
exist, that real Perl is an editor dependency, that parser buckets moved, that
providers are more correct, or that any support tier has been promoted.
