# perl-pragma

Pragma state tracking for Perl source analysis.

## Overview

`perl-pragma` walks a `perl-ast` AST and builds a range-indexed pragma map so callers can query the effective lexical pragma state at any byte offset in a file.

The tracker models far more than a strict/warnings-only surface. It currently tracks:

- strict categories (`vars`, `subs`, `refs`)
- warnings (global plus selectively disabled categories)
- `utf8`
- `encoding`
- `locale` (including optional scope argument)
- feature flags from explicit `use feature`/`no feature` and version bundles (`use vX.Y` / `use 5.xxx`)
- lexical `use builtin` imports

Lexical scope restoration is handled for ordinary blocks and other block-like forms, including eval blocks, package block form, and phase blocks.

## Public API

- **`PerlVersion`** -- parsed major/minor Perl version used for version pragma semantics.
- **`PragmaState`** -- effective lexical state snapshot, including strict/warnings, utf8/encoding/locale, feature flags, and builtin imports.
- **`PragmaTracker`** -- walks an AST via `build()` to produce a sorted `Vec<(Range<usize>, PragmaState)>`, and offers `state_for_offset()` to query it.
- **Version/feature helpers** -- `parse_perl_version`, `version_implies_strict`, `version_implies_warnings`, and `features_enabled_by_version`.

## Compile-environment schema

`compile_environment` provides the versioned canonical state and transition
schema for [#8520](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8520).
It accepts constructed schema inputs; the existing AST tracker does not produce
these states yet. Drafts pass through `admit` or bounded `read` before obtaining a
`CompileEnvironmentState`, `CompileEnvironmentTransition`, or `TransitionBundle`.

Each facet retains exact, conditional, limited, unsupported, stale, unavailable,
or resource/instrument status. Warning, language-profile, effect, and boundary
ports reference qualified external authority; absent ports never imply empty
policies. Closed `PortRole` values enforce warning/profile/effect/boundary
compatibility at every destination. Admission checks role and subject bindings;
it does not authenticate the opaque contract payload or implement its semantics.
`VersionDeclaration::Absent` is distinct from an unavailable version.
Strict categories are independent; signatures cannot enable full strict.

Source-file and semantic document-instance identities remain distinct. A binding
explicitly declares a common accepted opaque source-generation cursor domain,
while retaining the complete source, parser, compiler, and profile identities.
Admission requires a nonempty common generation label even when every facet is
non-exact; facet uncertainty cannot invent a binding. It revalidates nested shared
wire records through their constructors and rejects ignored nested wire fields.
Deltas use external variant keys (`{"strict_vars": {...}}`); the unpublished
adjacent `facet`/`value` form is rejected because buffering could hide ignored fields.
A transition's aggregate affected classes include every delta and boundary class.
Bundle order is the strict numeric `(byte_anchor, context_ordinal)` tuple; context
digests identify contexts but never break ties or impose lexicographic order.
Canonical serialization sorts set collections, preserves transition order, and
hashes domain-tagged bytes with the shared SHA-256 `ContentDigest` implementation.

Operational bounds are 65,536 transitions per bundle, 256 provenance/boundary
references per record, 1,024 custom feature/warning names, 256 bytes per name,
4,096 builtin imports or delta entries, 1 MiB per snapshot, 16 MiB bundle wire
input, and JSON depth 32. Reads include whitespace in their cap and stop after
at most cap+1 bytes; output serialization is bounded before appending bytes.
Caller-owned drafts already occupy caller memory; admission cannot retroactively
bound that allocation. Exceeding limits returns a typed failure without truncation.

`project_strict` is a fallible, one-way projection of only the three strict bits
into an existing legacy state. It clears the legacy signatures-to-strict bridge
while preserving the independent signatures feature and every unrelated field.
There is no reverse constructor or whole-state legacy reconstruction.

No directive application, warning catalog, version table, timeline, provider
publication, or migration of live consumers is implemented here.

## Compile-effect boundaries

`BoundaryRecord` is the admitted full record for
[#8561](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8561).
`BoundaryDraft` is untrusted input. The detailed lowercase `BoundaryReason` token
is separate from the domain-separated semantic instance digest. Source anchor,
checked byte-range ordering, complete `Binding`, scope, package, lifecycle phase,
source kind, effect identity, qualification, impacts and state references define
that instance. The schema has no source bytes: producers still verify range length
and UTF-8 boundaries against their actual snapshot. Source-suffix offsets have
the same producer-owned validation requirement; logical anchor digests do not
encode byte coordinates.

All eight dispositions remain distinct: exact-applied, exact-no-effect,
conditional, limited, unsupported, unavailable, stale, and resource/instrument
failure. Exact-no-effect requires available exact effect authority and an empty
applied set; it does not mean an unavailable effect was empty. Possible and applied
impacts are separate. Strict categories, named facts, source suffixes, lexical or
package scopes, finite declared fact sets, pending category/module closures, and
explicitly unbounded extent are separate selectors. Narrow and unbounded selectors
cannot coexist. The model validates declarations; it does not discover a narrower
extent or interpret an effect.

Category catalogs and module/SCC graphs have no accepted persistent identity in
this layer yet. Their selectors retain typed pending authority and cannot support
either exact disposition. No catalog/graph role or invented authority is added.
Coarse `SemanticReasonCode` and `BoundaryKind` bridges are optional compatible
classifications; they do not replace the detailed reason, guess phase, relabel
missing evidence unsupported, or accept registry debt.

The impact vocabulary and existing `SemanticFactFamily` contribution vocabulary
have these compatible descriptions. This is a documentation bridge, not an
identity conversion, currentness guarantee, complete equivalence, or debt ruling:

| Impact class | Compatible contribution family |
| --- | --- |
| `ImportVisibility` | `ImportFact` |
| `ExportVisibility` | `ExportFact` |
| `Prototypes` | `PrototypeFact` |
| `Features` | `FeatureFact` |
| `Inheritance` | `ClassInheritanceFact` |
| `GeneratedMembers` | `GeneratedMemberFact` |

No exact family mapping is asserted for `Strict`, `Warnings`, `Versions`,
`Builtins`, `Encoding`, `Locale`, `Profile`, `LexicalBindings`, `PackageBindings`,
`Constants`, `Methods`, `ModuleGraph`, `IncludeRoots`, `Types`, `CallResolution`,
`ProviderExactness`, `EditAuthorization`, or `AllFacts`. An affected state family
need not identify the contribution mechanism that produced it.
The previous `Boundary` linkage is renamed `BoundaryReference`. It now carries the
record's typed `ContentDigest`, complete source/parser/profile/compiler binding,
disposition, pending-authority declaration, affected classes and qualified port.
Construct it from `BoundaryRecord::reference()`. Existing struct-literal users must
supply these fields; old incomplete wire references are rejected. State/transition
admission checks complete subject equality, qualification and digest/port agreement.
This validates declarations, not the authenticity of a remotely supplied record.

Explanations, limitation prose, owner/replacement references, trigger excerpts and
facet explanation text do not enter boundary semantic identity. The canonical v1
projection is an alphabetically keyed JSON object, framed with the literal
`compile_effect_boundary.v1` domain and hashed with shared `ContentDigest`.
Changing metadata preserves both the record ID and its state-reference identity.
Set insertion order is canonicalized; boundary-bundle source order is retained.

Bounds are 4,096 records per bundle, 256 total possible/applied selectors,
256 possible states or provenance references, 256 bytes per identifier/owner,
4 KiB per explanation/trigger/limitation and at most 256 limitation entries,
1 MiB complete record payload, 16 MiB bundle input, and JSON depth 32.
Private digest framing does not consume the public payload allowance. Input caps
include whitespace; output serialization refuses before appending excess bytes.
`SchemaError::Limited` is an operational refusal that creates **no record**.
Callers must retain a resource/instrument failure; they must not turn it into an
admitted ordinary `BoundaryDisposition::Limited`, an exact default, or truncated
output. An admitted resource/instrument record remains representable separately.

`CompileEffectSourceKind` now lives in `perl-semantic-facts`; its existing
`perl_parser_core::hir::CompileEffectSourceKind` path re-exports the same type.
The fourteen variants retain their meanings and non-exhaustive Rust matching
contract. Explicit snake-case wire tokens reject unknown future kinds.

This is schema and reference validation only. There is no propagation, execution,
provider migration, fallback/edit authorization, timeline construction, module/SCC
invalidation, or debt acceptance.
## Benchmarks

Run the Criterion benchmark suite for build-heavy and query-heavy pragma workloads:

```bash
cargo bench -p perl-pragma
```

Benchmarks include stable names that can be diffed over time:

- `build_small_file`
- `build_large_file`
- `query_random_offsets`
- `query_monotonic_offsets`
- `final_state_lookup`
- `version_compat_walk_style`
- `scope_analyzer_walk_style`

Criterion reports per-benchmark timing statistics (including time per iteration and total sample estimates), which can be compared between runs.

## Workspace Role

Uses `perl-ast` for legacy extraction and the lower-layer `perl-source-identity`
and `perl-semantic-facts` vocabulary plus serde for the canonical schema.
`perl-parser-core` remains a consumer, never a normal dependency.

## License

MIT OR Apache-2.0

## Supplied warning catalogs

Four exact builtin catalogs and immutable supplied-policy queries are documented in
[the warning catalog contract](../../docs/reference/WARNING_CATALOG_POLICY.md).
This adds no source directive interpreter or legacy consumer migration.
