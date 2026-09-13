# CLAUDE.md (perl-semantic-facts)

## Role

Neutral semantic-fact vocabulary for Perl analysis layers: strongly-typed
IDs and serializable fact records shared between parser-derived semantics,
semantic-analyzer synthesis, and workspace indexing. A types-only crate: it
defines vocabulary and validates it, and performs no analysis.

`lib.rs` holds the shared vocabulary, but this is no longer a single-file
crate — `src/` also carries several contract modules of its own, listed below
only where this guide describes them.

## Owns

- ID newtypes: `FileId`, `ScopeId`, `EntityId`, `AnchorId`, `OccurrenceId`,
  `EdgeId`, `DiagnosticId` (all `u64`-backed via the `id_newtype!` macro).
- Kind enums: `EntityKind` (Package, Class, Role, Subroutine, ...),
  `OccurrenceKind` (Definition, Reference, Call, Import, ...), `EdgeKind`
  (Defines, References, Inherits, ComposesRole, ...).
- Confidence/provenance vocabulary: `Provenance` (how a fact was derived --
  `ExactAst` through `LiteralRequireImport`), `Confidence` (High/Medium/Low).
- Fact records: `AnchorFact`, `EntityFact`, `OccurrenceFact`, `EdgeFact`,
  `DiagnosticFact`.
- Export/import modeling: `ExportSet`, `ExportTag`, `ImportSpec`,
  `ImportKind`, `ImportSymbols`, `UseLibFact`.
- Visibility resolution: `VisibleSymbol`, `VisibleSymbolContext`.
- `structural_access/` -- the ordered structural access-hop contract (#13619):
  `StructuralAccessChain`, `StructuralAccessHop` and their vocabulary, for
  recording `$config->{groups}{staff}[0]` hop by hop. Validated constructors,
  private fields, deterministic fingerprints. No producer consumes it yet.

## Does not own

Explicitly, per the crate's own doc comment: does not parse Perl, does not
implement LSP providers, and does not own workspace storage backends. This
is a types-only vocabulary crate.

## Neighbors

- Upstream: `serde` (only dependency).
- Downstream: `perl-lsp-rs`, `perl-parser-core`, `perl-semantic-analyzer`,
  `perl-symbol`, `perl-workspace`.

## Read first

- `src/lib.rs` -- the shared vocabulary; read the top doc comment first for
  the "does not" boundary, then the ID newtypes and kind enums before the
  fact record structs (records reference the IDs/kinds).
- `src/structural_access/mod.rs` -- read its module doc before touching any
  file in that directory. It states the ownership fence, the
  spelling-is-evidence rule, and the transport trust boundary, all of which
  its laws depend on.

## Focused validation

`cargo test -p perl-semantic-facts`. `tests/prop_json_roundtrip.rs`
(property-based) guards serde round-tripping for every fact type --
required since these records cross process/cache boundaries as JSON.
`tests/bdd_semantic_facts.rs` covers behavioral scenarios.

For `structural_access/`: `cargo test -p perl-semantic-facts --lib
structural_access` runs the in-module falsifiers, and
`tests/structural_access_roundtrip.rs` drives the same public API from
outside the crate. Each falsifier is named for the wrong implementation it
rejects, so a failure names the law that broke.

## Review hotspots

- Any new fact-record field needs the round-trip property test to still
  pass; a field that can't round-trip through `serde_json` breaks every
  consumer that persists these facts.
- `Provenance` variants encode a specific derivation method (e.g.
  `LiteralRequireImport` is documented as more precise than `ExactAst` for
  a narrow case) -- read each variant's doc comment before adding a new one
  or assuming a general reliability ordering across all of them.
- `structural_access/` fingerprints: every component folds through its own
  labelled, length-prefixed field. Joining components into one string
  reintroduces cross-field collisions that were shipped and fixed once
  already. Borrowed enums fold through explicit stable tags, never `Debug`
  text, so a variant rename cannot move a persisted digest.
- `structural_access/` validation laws are cross-field and several are
  deliberately *narrow*. A shape or status that cannot distinguish two cases
  must not decide between them -- `ValueShape::Scalar` covers `undef`, which
  Perl autovivifies, so it constrains no operator. Four laws in the original
  PR were too strict for exactly this reason; check the "asserts too little"
  question before adding or tightening one.
- `structural_access/` blank-identity checks split on source token versus
  runtime value. Spelling text, sigils and variable names are *written*, so
  they reject any whitespace-only string -- `my $  ;` is a syntax error. A
  package in `ValueShape::Object`/`PackageName` is a runtime string reachable
  through `bless $ref, $name`, and `bless {}, "  "` is a real class with
  working dispatch, so only the empty string is rejected there. Verify the
  interpreter before changing either side.

## Claim boundary

Describes the fact vocabulary as authored (types and their doc comments),
plus the `structural_access/` contract as authored. Does not assert which
analysis stage actually populates which fact kind at runtime -- that logic
lives in the consuming crates listed above, and no producer emits a
structural access chain yet.

This guide documents `lib.rs` and `structural_access/`. The crate's other
modules under `src/` are not described here; read them directly rather than
assuming this file covers them.
