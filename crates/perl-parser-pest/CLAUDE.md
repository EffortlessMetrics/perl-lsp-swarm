# perl-parser-pest

Experimental Pest-based Perl parser used as a comparison instrument, legacy
compatibility reference, and benchmark substrate. It is not the production parser,
an LSP fallback, default-gate authority, or evidence of Tree-sitter compatibility.

## Authority and scope

The repository-root `CLAUDE.md` and applicable `AGENTS.md` own routes, orchestration,
review, proof currentness, and result vocabulary. Current source, manifests, tests,
fixtures, and sync recipes own the exact API, dependency, module, and synchronized-file
inventory. This file narrows those contracts to the crate's role, claim limits, change
hazards, and proof routes.

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

# Required while the archived v2 compatibility copy remains synchronized.
just ci-v2-bundle-sync
```

Run the bundle-sync recipe only when its current recipe says the changed source is in
the synchronized set. Passing it proves equality under that transitional contract; it
does not make the archive a second design authority.

## Instrument contract

The current implementation has three useful stages:

1. Pest parses `grammar.pest` into parser pairs.
2. AST construction projects those pairs into crate-local `AstNode` values, with Pratt
   parsing for operator precedence.
3. `SexpFormatter` serializes that AST into a crate-local comparison projection.

The S-expression is not a Tree-sitter syntax tree, ABI, node-schema, source-range, or
semantic-parity contract. Do not label matching text shapes as Tree-sitter
compatibility. Benchmark and corpus evidence must identify this provider/package and
the grammar/projection revision needed to reproduce the observation.

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

The live crate is the design authority. The archived `tree-sitter-perl-rs` v2 bundle is
transitional compatibility debt. While `just ci-v2-bundle-sync` remains part of the
repository contract, changes to its synchronized set must satisfy it, but do not expand
that set, copy new architecture into the archive, or treat archive equality as product
correctness.

Changes here should improve comparison reliability, evidence honesty, legacy
compatibility, or retirement readiness. Do not widen the crate into a competing
production parser or infer production reachability from package-local green tests.
