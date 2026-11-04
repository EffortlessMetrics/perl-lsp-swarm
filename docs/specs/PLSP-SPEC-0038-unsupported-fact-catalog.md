# PLSP-SPEC-0038: Unsupported-fact catalog contract

Status: draft
Owner: #4770 (unsupported-fact catalog and cross-layer reason mapping)
Linked parent: [#2543](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/2543) — unsupported-fact taxonomy and executable-profile receipt contract
Linked specs:
- [PLSP-SPEC-0035](PLSP-SPEC-0035-executable-profile-and-eir.md) — executable-profile charter; profile scope for `outside_profile` classes
- [PLSP-SPEC-0025](PLSP-SPEC-0025-pir-v0.md) — PIR-A data-model contract; analysis-side vocabulary
Linked issues:
- [#4770](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4770) — owner; catalog and projections
- [#4773](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4773) — profile manifest consumer
- [#4775](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4775) — EIR boundary-ID consumer
- [#4751](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4751) / [#4753](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4753) — boundary-instance registry mechanism and population (prior art, not superseded)
- [#4736](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4736) / [#4749](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4749) — ledger consumers
- [#17510](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17510) — EIR entry gate: candidate PLSP-ADR-0008 (PR #17672, unmerged) stages this catalog at §6(2)
Status impact: unsupported-fact identity, cross-layer reason projections, profile-boundary vocabulary

## Purpose

Compiler facts, semantic boundaries, provider decisions, curated gold, the
real-Perl oracle, and future executable-profile/EIR receipts each name
unsupported behavior in their own vocabulary — and three load-bearing fields
are free-form prose today (`CompileEffect.dynamic_reason`,
`SemanticBoundaryRegistryEntry.reason` / `semantic_meaning`). This spec
defines the stable machine-readable **class** vocabulary those layers project
onto, the entry contract each class carries, and the versioning rules that
keep receipt compatibility.

This spec defines vocabulary and contract. It does **not** build the
validator, migrate any prose field, change provider behavior, or claim Perl
compatibility.

## Contract — Identity

### I1 — IDs are stable receipt compatibility

A class ID, once published in the catalog, never changes meaning. Renaming,
splitting, or merging an ID requires a schema/catalog version migration with
a recorded mapping. A class whose construct becomes supported **retires**; it
is never repurposed for another meaning.

### I2 — ID shape

IDs are lowercase `snake_case` ASCII (`[a-z0-9_]+`). IDs sort
lexicographically in canonical serialization.

### I3 — Reuse only on meaning cover

An existing vocabulary ID is reused as a class ID only when its documented
meaning covers the seed class exactly. Otherwise a new ID is minted with a
distinction note recorded in the entry. Near-misses stay distinct: provider
candidate-resolution outcomes and schema escape hatches are never class IDs.

### I4 — Profile scope is part of the ID

Classes whose limitation is profile membership (rather than fundamental
modeling) carry the `outside_profile` suffix. The suffix is a validated
pattern, not a hint: the validator enforces the shape. Profile expansion
retires classes; it never renames them.

## Contract — Entry

### E0 — Canonical artifacts

The catalog lives at exactly two paths:

- `schemas/unsupported_fact_catalog.v1.schema.json` — the JSON Schema
  (draft 2020-12, following repo schema conventions) with
  `schema_version` const `unsupported_fact_catalog.v1`.
- `policy/compiler/unsupported-facts.v1.toml` — the versioned TOML catalog
  (new `policy/compiler/` directory).

No other path carries catalog authority. Consumers cite class IDs, never
file paths.

### E1 — Required entry fields

Each catalog entry carries all of:

- `class_id`: the stable ID (I1–I4).
- `owning_layer`: the compiler layer that recognizes the class
  (`parser`, `hir`, `pir`, `semantic`, `provider`, `oracle`, `eir`).
- `trigger_rule`: the recognition rule, precise enough to implement.
- `distinctions`: what the class is not — explicit separation from
  `DynamicBoundary`, low confidence, stale evidence, fallback, and the
  nearest neighboring class ID.
- `compile_acceptance_blocks`: whether compile acceptance blocks on this class.
- `downstream_blocks`: whether downstream static facts block or require
  qualification.
- `allowed_provider_behaviors`: subset of `block`, `fallback`, `advisory`,
  `require_confirmation`.
- `curated_gold_treatment`: expected gold handling for the class.
- `oracle_result_class` / `oracle_promotion_effect`: expected oracle
  projection; `unknown` result class projects to `blocks_promotion`, never
  to agreement, support, or known limitation.
- `profile_boundary_behavior`: executable-profile boundary behavior.
- `owner_issue`: owning issue or permanent claim boundary.
- `history`: introduction version and any migration record.

### E2 — Blocking is explicit and total

`compile_acceptance_blocks` and `downstream_blocks` are booleans, never
absent. An entry that cannot decide its blocking effect is incomplete and
fails validation.

### E3 — Provider exactness cannot bypass a blocking class

No provider behavior may produce exact output for a class marked blocking.
The validator rejects any consumer mapping that does.

## Contract — Seed vocabulary

### V1 — The fourteen seed classes

| Class ID | Owning layer | Seed |
|---|---|---|
| `ambiguous_bareword` | parser | ambiguous bareword intent; NOT provider `ambiguous_identity` (candidate-resolution outcome) |
| `symbolic_ref` | hir | dynamic symbolic-reference name (reused vocabulary) |
| `autoload` | hir | dynamic method dispatch / AUTOLOAD (reused vocabulary) |
| `generated_no_source` | semantic | generated member without a supported adapter (reused vocabulary) |
| `dynamic_stash_mutation` | hir | dynamic stash/typeglob mutation; NOT `typeglob_alias` (alias, not mutation) |
| `ambient_module_resolution` | hir | ambient module resolution; NOT `dynamic_require` (require-$expr only) |
| `source_filter` | parser | source filter |
| `xs_native_dependency` | semantic | XS or native-extension dependency |
| `unsupported_parser_construct` | parser | unsupported parser construct |
| `compile_time_execution_outside_profile` | semantic | compile-time execution outside the executable profile |
| `runtime_builtin_outside_profile` | semantic | runtime builtin outside the profile |
| `regex_construct_outside_profile` | semantic | regex construct outside the profile |
| `host_capability_outside_profile` | semantic | filesystem/network/environment capability outside the profile |
| `unknown_boundary_evidence` | semantic | unknown or malformed boundary evidence; NOT the `unknown` schema escape hatch |

### V2 — Seed completeness

The seed set covers compiler, provider, oracle, and runtime boundaries. Later
classes extend the table; no seed ID is removed except by retirement (I1).

## Contract — Consumer projections

### P1 — Projections are checked and local

Each consumer maps its own vocabulary onto class IDs through a checked local
projection. Projections are one-directional (consumer field cites a class
ID); the catalog never aliases consumer vocabularies to each other. A
citation of an unknown ID fails closed.

### P2 — Prose fields cite; they are not replaced

`CompileEffect.dynamic_reason`, registry `reason` / `semantic_meaning`, and
similar prose fields may cite a class ID alongside prose. Adoption is per
consumer, additive, and separately reviewed. No migration of existing prose
is authorized by this spec.

### P3 — Blockers are not classes

Provider decision blockers name guards that fired; class IDs name unsupported
behavior encountered. A consumer mapping must preserve the distinction: the
`unknown` blocker (schema escape hatch) never satisfies a class citation,
and candidate-resolution outcomes never stand in for intent classes.

### P4 — Oracle projection defaults

Unless an entry states otherwise, a class projects to oracle result class
`dynamic_or_unsupported` with promotion effect `blocks_promotion`.
`unknown_boundary_evidence` projects to `unknown` / `blocks_promotion`:
per PLSP-SPEC-0027, `unknown` blocks promotion. Unknown is never agreement,
never a known limitation, and never support.

## Contract — Validation

### X1 — Fail-closed validator

The catalog validator (`compiler-profile unsupported --check`, slice 2)
rejects at least: unknown IDs in receipts; duplicate IDs; missing consumer
behavior; a source-lock ID masquerading as a general class; an EIR boundary
without a stable catalog ID; any entry violating E0–E3, I1–I4, or P1–P4.

### X2 — Deterministic report

`--report` output is deterministic for identical catalog input: sorted IDs,
stable counts, no host paths, no timestamps.

## Claim boundaries

- Docs: this spec plus the generated catalog artifacts are the only doc
  claims. No generated status surface moves on spec acceptance.
- Releases: no release-notes claim, no version-bump requirement, no
  compatibility claim follows from this spec.
- User-facing behavior: no provider, editor-visible, or CLI-default change.
  The slice-2 validator is a new opt-in command; no existing command
  changes behavior.

## Non-goals

- No boundary-instance registry mechanism (owned by #4751) or population
  (owned by #4753).
- No migration of existing prose reasons to class IDs.
- No validator implementation (slice 2), gate wiring (slice 4), provider
  behavior change, or compiler semantics change.
- No EIR or interpreter implementation; no broad compatibility claim.

## Acceptance mapping

- Versioned schema and catalog exist → I1–I4, E0–E1, V1–V2 (slice 1).
- Seed classes cover all four boundaries → V2 (slice 1).
- Cross-layer mappings typed and validated → P1–P4, X1 (slices 2–3).
- Unknown IDs fail closed → P1, X1 (slice 2).
- Provider exactness cannot bypass blocking → E3, X1 (slice 2).
- Ledger consumers can consume → P1–P2 (slice 3).
- EIR/profile receipts can stamp boundary IDs → V1, P1 (slice 3).
- No behavior or semantics change → Non-goals (all slices).
