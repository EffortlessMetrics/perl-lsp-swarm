# Upstream Snapshot Provenance (`tree-sitter-perl-c`)

This file is the auditable record for the vendored C snapshot in `c-src/`
and the vendored query sources in `queries/`.

## Current vendored snapshot

- **Upstream grammar repository:** `tree-sitter-perl/tree-sitter-perl`
  (`https://github.com/tree-sitter-perl/tree-sitter-perl`)
- **Upstream tracking ref for refreshes:** `master` (resolve to a specific commit
  at refresh time)
- **Generator version used by current `parser.c`:** `tree-sitter v0.25.9`
  (from the generated header comment in `c-src/parser.c`)
- **Import provenance in this repository:** snapshot introduced in commit
  `c57aadcba61cf295c6abc2b2a9c85cdf13de9cbb` by copying the archived C sources
  from `archive/crates/tree-sitter-perl-rs/src/` into `crates/tree-sitter-perl-c/c-src/`.

### Snapshot fingerprints (for audit/diff checks)

- `c-src/parser.c` SHA-256:
  `07b7bb23511188e97cfdbd6ac6289439f872e58504d2c51d9e24e59fae957d2a`
- `c-src/scanner.c` SHA-256:
  `01bbea22f0864679692fb0163b29304d346666f2181b1dbbe08f900c9bb219eb`
- `c-src/tsp_unicode.h` SHA-256:
  `9cbc0731f8c9bd52bd3de9644fd887f20cecea2d17634b20769db4940eadb566`
- `c-src/bsearch.h` SHA-256:
  `cb08206e89750c1fab700b89fc9876afb5cc689827e514ef49a5569c54635b61`

### Vendored query fingerprints (for audit/diff checks)

Queries are copies of the repository's root grammar snapshot
(`tree-sitter-perl/queries/*.scm`), introduced by public API work on
`crates/tree-sitter-perl-c` (2026-08). They must be refreshed together with
`c-src/` because query validity depends on the exact grammar snapshot.

Normalization contract: the vendored copies differ from the upstream bytes
only by whitespace hygiene required by this repository's binary-diff gate
(`git diff --check`) — the trailing space on one separator line in
`injections.scm` and one blank line at EOF of `highlights.scm`. Query
semantics are unaffected; every other byte matches upstream.

- `queries/injections.scm`
  - upstream-source SHA-256: `b89b4870f26325c8bc678cf970d10afe7f2bafb9c23b99fae21cbb1a8017a84f`
  - vendored (normalized) SHA-256: `027e3f0502d08ae647f4be25bb879b8a401cf3f8836cfeaafd3b4e9e88a732d6`
  — compiles cleanly against the current `c-src/` parser via
  `load_injections_query()`.
- `queries/highlights.scm`
  - upstream-source SHA-256: `db02f6b650e5df79ae764f30721c7ff6983925c39c7ca72e1738c17e76e6734d`
  - vendored (normalized) SHA-256: `2414f4fe4ccb0f9fe3a55af265888320c1fdddd8522d48678754a8ee57a08a03`
  — **known snapshot delta:** targets newer grammar surface than the frozen
  `c-src/` parser (`postfix_deref` literal-token children at row 136 and
  `slices` `hashref:`/`arrayref:` fields), so `load_highlights_query()`
  returns a typed `tree_sitter::QueryError` (kind `Structure`) until the
  next joint refresh. Do not patch the `.scm` in place; resolve through a
  full snapshot refresh.

> **Important:** This snapshot predates explicit provenance tracking in this
> crate. During the next refresh, record the exact upstream commit SHA used to
> generate/copy `parser.c` and `scanner.c` in this file.

## What is vendored vs local

### Vendored from upstream grammar snapshot

- `c-src/parser.c` (generated parser)
- `c-src/scanner.c` (external scanner)
- `c-src/bsearch.h`
- `c-src/tsp_unicode.h`
- `c-src/tree_sitter/parser.h`
- `c-src/tree_sitter/array.h`
- `c-src/tree_sitter/alloc.h`
- `queries/injections.scm` (exposed as [`INJECTIONS_QUERY`])
- `queries/highlights.scm` (exposed as [`HIGHLIGHTS_QUERY`])

### Local wrapper/maintenance code in this crate

- `src/lib.rs` (FFI declaration + Rust API)
- `build.rs` (compile/link vendored C sources)
- `tests/` (Rust-level behavior/query integration tests)
- `src/bin/` (CLI helpers for parse and benchmark smoke checks)
- `README.md`, `ROADMAP.md`, and this file

## Refresh procedure

1. **Choose upstream commit/ref and record it first**

   ```bash
   UPSTREAM_REF=<commit-or-tag>
   ```

2. **Fetch upstream grammar sources**

   ```bash
   git clone https://github.com/tree-sitter-perl/tree-sitter-perl /tmp/tree-sitter-perl
   cd /tmp/tree-sitter-perl
   git checkout "$UPSTREAM_REF"
   ```

3. **Regenerate parser with pinned CLI (if `src/parser.c` is not committed upstream)**

   ```bash
   tree-sitter --version  # record exact version in this file
   tree-sitter generate
   ```

4. **Copy vendored C snapshot files into this crate**

   ```bash
   cp src/parser.c /workspace/perl-lsp/crates/tree-sitter-perl-c/c-src/parser.c
   cp src/scanner.c /workspace/perl-lsp/crates/tree-sitter-perl-c/c-src/scanner.c
   cp src/bsearch.h /workspace/perl-lsp/crates/tree-sitter-perl-c/c-src/bsearch.h
   cp src/tsp_unicode.h /workspace/perl-lsp/crates/tree-sitter-perl-c/c-src/tsp_unicode.h
   cp src/tree_sitter/parser.h /workspace/perl-lsp/crates/tree-sitter-perl-c/c-src/tree_sitter/parser.h
   cp src/tree_sitter/array.h /workspace/perl-lsp/crates/tree-sitter-perl-c/c-src/tree_sitter/array.h
   cp src/tree_sitter/alloc.h /workspace/perl-lsp/crates/tree-sitter-perl-c/c-src/tree_sitter/alloc.h
   ```

4b. **Copy vendored query sources into this crate**

   ```bash
   mkdir -p /workspace/perl-lsp/crates/tree-sitter-perl-c/queries
   cp queries/injections.scm /workspace/perl-lsp/crates/tree-sitter-perl-c/queries/
   cp queries/highlights.scm /workspace/perl-lsp/crates/tree-sitter-perl-c/queries/
   ```

   After a joint refresh, confirm both `load_injections_query()` and
   `load_highlights_query()` compile cleanly and flip the drift tripwire test
   (`load_highlights_query_fails_closed_on_snapshot_drift`) back to the
   positive-capture assertions described in its comment.

5. **Update this file**

   - Upstream repo + exact commit/tag
   - Generator version
   - New SHA-256 fingerprints
   - Date of refresh (UTC)

## Refresh validation checklist

Run these checks from repo root after refreshing:

- [ ] **Build / compile sanity**
  - `cargo check --all-targets -p tree-sitter-perl-c`
- [ ] **Tests**
  - `cargo test -p tree-sitter-perl-c`
- [ ] **Query conformance (injections/highlights behavior)**
  - `cargo test -p tree-sitter-perl-c bdd_injections_query`
- [ ] **Benchmark sanity (non-regression smoke)**
  - `cargo run -p tree-sitter-perl-c --bin bench_parser_c --features test-utils -- tree-sitter-perl/test/corpus/statements`

If any step fails, do not ship the snapshot update without documenting the delta.
