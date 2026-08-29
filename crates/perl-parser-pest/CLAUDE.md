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

The historical `ci-v2-bundle-sync` recipe and its archived comparison are not a
package-local proof requirement. If historical parity is investigated while the
retirement work in #8814 remains open, treat that check as bounded evidence of byte
equality only; it does not establish current correctness, a second design authority,
or a requirement to keep the live crate synchronized with the archive.

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

Issue #8814 records the accepted direction for this boundary: the live crate is the
canonical source for this experimental parser, while the archived `tree-sitter-perl-rs`
v2 bundle is historical evidence and compatibility debt. The repository may still
contain legacy synchronization machinery while that issue remains open, but this file
does not prescribe it as a current contract. Do not expand the old synchronized set,
copy new architecture into the archive, or treat archive equality as product
correctness.

Changes here should improve comparison reliability, evidence honesty, legacy
compatibility, or retirement readiness. Do not widen the crate into a competing
production parser or infer production reachability from package-local green tests.
