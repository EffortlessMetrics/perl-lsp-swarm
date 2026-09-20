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
Admission revalidates nested shared wire records through their constructors.
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
