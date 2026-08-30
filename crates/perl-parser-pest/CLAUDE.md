# perl-parser-pest

Experimental Pest-based Perl parser used as a comparison instrument, legacy
compatibility reference, and benchmark substrate. It is not the production parser,
an LSP fallback, default-gate authority, or evidence of Tree-sitter compatibility.

## Authority and scope

The checked-in repository-root `CLAUDE.md` and `AGENTS.md`, as classified by
`docs/agents/AUTHORITY_STATUS.md` and `docs/agents/authority_status.toml`, are the
current repository authority for routes, orchestration, review, proof currentness, and
result vocabulary. Current source, manifests, tests, fixtures, and applicable recipes
own the exact API, dependency, module, and file inventory. This file narrows those
contracts to the crate's role, claim limits, change hazards, and proof routes; it does
not establish a competing repository contract.

Keep this file durable. Update it when the instrument contract, evidence identity,
compatibility boundary, or proof route changes. Do not mirror workspace versions,
dependency lists, exhaustive type/module inventories, or temporary migration state.

## Proof routes

```bash
cargo build -p perl-parser-pest
cargo test -p perl-parser-pest
cargo test -p perl-parser-pest --test fixture_manifest
cargo clippy -p perl-parser-pest
cargo doc -p perl-parser-pest --open
```

The legacy `ci-v2-bundle-sync` recipe and its archived comparison remain active
repository machinery while the retirement work in #8814 remains open, but they are not
a package-local proof requirement. If that active check is investigated or run, treat it
as bounded evidence of byte equality only; it does not establish current correctness, a
second design authority, or a requirement to keep the live crate synchronized with the
archive.

## Instrument contract

The current implementation has three useful stages:

1. Pest parses `grammar.pest` into parser pairs.
2. AST construction projects those pairs into crate-local `AstNode` values, with Pratt
   parsing for operator precedence.
3. `SexpFormatter` serializes that AST into a crate-local comparison projection.

The live crate API documentation describes this output as a Tree-sitter-compatible
S-expression. That is a bounded serialized-format claim, not a promise that this crate
produces a Tree-sitter syntax tree, ABI, node schema, source ranges, or semantic parity.
Do not promote matching text shapes into any broader compatibility claim. The larger
compatibility decision and its evidence belong to #9214; benchmark and corpus evidence
must identify this provider/package and the grammar/projection revision needed to
reproduce the observation.

`parse()` remains the operative parser API. Typed outcome, attempt, diagnostic, failure,
and source-range types are substrate until a current production path and discriminating
tests prove integration. Do not turn type presence or re-export into an integration
claim, and do not claim complete source spans where the AST does not carry them.

## Fixture evidence

Package-local fixture identity lives under `tests/fixtures/`; the reusable runner lives
under `tests/support/` and is exercised by
`cargo test -p perl-parser-pest --test fixture_manifest`.

Fixture rows record current parse observations. They do not declare the parser correct,
define a language-support matrix, or replace targeted tests. Load and select through a
caller-supplied package root such as `CARGO_MANIFEST_DIR`, not the workspace root.
Duplicate IDs, path escape, missing sources, empty selection, and parser panics must
fail closed as instrument errors rather than being counted as parser results.

## Compatibility boundary

Issue #8814 records the accepted direction for this boundary: the live crate is the
canonical source for this experimental parser, while the archived `tree-sitter-perl-rs`
v2 bundle is historical evidence and compatibility debt. The `ci-v2-bundle-sync` route
and its archive comparison remain active transitional machinery until #8814 lands, but
their presence does not make the archive current authority. Do not expand the old
synchronized set, copy new architecture into the archive, or treat archive equality as
product correctness.

Changes here should improve comparison reliability, evidence honesty, legacy
compatibility, or retirement readiness. Do not widen the crate into a competing
production parser or infer production reachability from package-local green tests.

## Self-description hazard

This package describes itself. Its manifest carries literal identity, MSRV, and
dependency versions instead of `*.workspace = true`, and no dependency or
dev-dependency is path-only (#8771). Two edits would silently undo that:

- reintroducing `workspace = true` for any key other than `[lints]`, and
- adding a path dependency, including a shared test helper.

`[lints] workspace = true` is the one deliberate exception and must stay. The required
`cargo xtask check-lint-policy` gate enforces it on every workspace member with no
exemption mechanism, so the lint half of #8771's standalone contract cannot land while
this crate is a member; removing the marker to "finish" the decoupling turns that gate
red. Whether the invariant grows an extraction exemption or the lint decoupling moves to
the extraction PR is a separate decision.

`tests/standalone_package.rs` fails closed on all of it: it asserts `[lints]` is the
*only* inherited key so the exception cannot spread, that the marker is still present,
and that no path dependency, falsely-external `repository`/`homepage`, or unpackaged
load-bearing asset has appeared.

Assertion-boundary helpers are package-local in `tests/support/assert.rs`; the unit
tests inside `src/pure_rust_parser.rs` carry their own copy so that file stays
byte-identical to its archived twin while `ci-v2-bundle-sync` remains active machinery.
`examples/parse_basic.rs` is the compiled public example, because `[lib] doctest = false`
leaves the README snippet unchecked.

`repository` and `homepage` name the current lineage; the external repository does not
exist yet, and the pending owner is recorded under `[package.metadata.extraction]`
rather than as a future URL.
