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

The heredoc contract (#8220) is the one integrated consumer of that vocabulary. A
deterministic pre-pass in `heredoc.rs` runs before stage 1: an opener owns the physical
lines below its logical line up to its terminator, those bytes become the node's
content, and they leave the text handed to Pest so following code resumes at the line
after the terminator. `parse_heredoc_outcome` reports that contract as a
`ParseCompleteness`, and every case the pre-pass cannot own truthfully — a missing
terminator, a body over the byte budget, more openers on one line than the depth budget,
a Perl-illegal `<< MARKER` — carries a typed diagnostic instead of an empty content that
reads as a clean parse.

That completeness is heredoc-scoped. `Complete` means no opener lost or truncated a
body; it is not a whole-source accounting claim, which remains #8093's row. The budgets
mirror `perl-lexer`'s `MAX_HEREDOC_BYTES`/`MAX_HEREDOC_DEPTH`, and production heredoc
lexing remains `perl-lexer`'s.

The pre-pass removes body lines before the grammar sees them, so a false opener deletes
real source. Two invariants keep that safe, and both have drift guards:

- the scanner and the grammar must decide identically which `<<` is an opener.
  `scanner_and_grammar_agree_on_openers` parses each row with the grammar directly and
  compares, and `parse_heredoc_outcome` reports any residual disagreement instead of
  returning a clean parse, because an opener the scanner misses creates no capture and
  no per-capture defect could otherwise see it;
- non-code regions own no openers — comments, strings, quote-like operators and bare
  regex literals including runs left open across lines, POD, `format` bodies, and
  everything after `__DATA__`/`__END__`.

Expectations in `tests/heredoc_body_contract.rs` are derived from real `perl` behavior,
not from this crate's output; keep it that way when extending them. When adding a
construct the scanner must skip, add the negative control first — it is the half of the
suite that catches an over-eager scanner.

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
