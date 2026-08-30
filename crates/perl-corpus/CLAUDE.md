# perl-corpus

Test-evidence infrastructure for Perl parser, LSP, and DAP work. This crate owns
corpus-root authority, typed loading, fixture generation, and distribution boundaries;
it does not make every repository corpus asset part of the published crate.

## Authority and scope

The checked-in repository-root `CLAUDE.md` and `AGENTS.md`, as classified by
`docs/agents/AUTHORITY_STATUS.md` and `docs/agents/authority_status.toml`, are the
current repository authority for routes, orchestration, review, proof currentness, and
result vocabulary. Current source, manifests, tests, and generated contracts own the
exact API, dependency, module, and asset inventory. This file narrows those contracts
to crate-local semantic hazards and proof routes; it does not establish a competing
repository contract.

Keep this file durable. Update it when an ownership boundary, semantic invariant,
failure mode, or proof route changes. Do not mirror workspace versions, dependency
lists, exhaustive module tables, or transient migration state here.

## Proof routes

```bash
cargo build -p perl-corpus
cargo test -p perl-corpus
cargo test -p perl-corpus --features ci-fast
cargo test -p perl-corpus --test root_path_authority
cargo test -p perl-corpus --test corpus_asset_path
cargo test -p perl-corpus --test distribution_contract
cargo clippy -p perl-corpus --all-targets -- -D warnings -A missing_docs
cargo package -p perl-corpus --allow-dirty --list
cargo run -p perl-corpus -- --help

# Generation is self-contained; evidence must retain the seed.
cargo run -p perl-corpus -- gen program --count 10 --seed 42
```

Select the smallest proof that discriminates the changed contract, then run the
applicable repository-level route from the root contract. A generated example without
its seed is exploratory output, not reproducible evidence.

## Root authority

`CorpusRoot` and `CorpusPaths` serve different contracts.

- `CorpusRoot::resolve_authoritative(explicit)` selects explicit input, then
  `PERL_CORPUS_ROOT`, then returns `AuthoritativeRootRequired`.
- Invalid explicit input fails immediately. It never falls through to a valid
  environment value or workspace discovery.
- Strict roots must be absolute, directories, and free of symbolic-link or Windows
  reparse-point components.
- A strict root retains a shared open `same_file::Handle`. The canonical path is
  diagnostic context; clones share the retained directory identity and do not reopen
  the path.
- `CorpusRoot::require_repository_layout()` proves only the `test_corpus/` and
  `crates/perl-corpus/fuzz/` directory chains. It does not recurse, select extensions,
  inspect leaves, or redefine `CorpusTopology`.
- `CorpusPaths::discover()` and `CorpusPaths::from_root()` remain unchecked
  compatibility APIs. Their raw mutable paths are never authority.
- `CorpusPaths::try_from_root`, `try_discover`, and `resolve_authoritative` return
  immutable `ResolvedCorpusPaths`; `into_paths()` is an explicit authority downgrade.
- `ResolvedCorpusPaths` must not implement `Deref`, `AsRef<CorpusPaths>`,
  `Borrow<CorpusPaths>`, or any other implicit conversion into `CorpusPaths`. The
  downgrade is written down at the call site as `as_paths()` or `into_paths()`.
  `tests/root_path_authority.rs` holds this boundary with `assert_does_not_implement!`,
  which breaks that test target's build if such an impl reappears. Keep the enforcement
  there, not only in a doctest: the gates run `cargo test --locked --tests` and never
  `cargo test --doc`.
- Component-by-component selected-member opening must consume the retained root
  capability. Do not add another root-opening path.
- The published package ships APIs and deliberately included crate assets. Repository
  corpus data remains an external root.

## Portable member identity

- `CorpusAssetPath` alone proves one canonical root-relative component sequence. It
  does not prove topology membership, existence, containment, opening, or bytes.
- `/` is the sole portable serialization separator. A literal backslash is data;
  durable parsing must never route through the host `Path` parser.
- Host paths enter through actual host components. Host materialization pushes
  validated components individually and must round-trip injectively or fail with
  `unsupported_on_host`.
- `CorpusAsset::portable_path()` validates the v1 duplicated identity fields and
  layer prefix. `CorpusTopology::member_path()` adds exact topology membership.
- Keep topology schema v1 serialized strings byte-compatible and deterministic. Do
  not add a second component-array encoding.
- #7693 must consume `CorpusTopology::member_path()` plus the retained `CorpusRoot`;
  it must not reconstruct portable identity or reopen the root by pathname.

## Typed loading authority

`load_plain_perl_source` and `load_sectioned_corpus_document` are deliberately
different contracts.

- Loader selection comes from topology or the consumer. Never infer sectioned format
  from `.txt` alone.
- The selected leaf is opened with a platform-reviewed no-follow contract, metadata is
  read from that opened handle, and bytes are read from the same handle.
- Symbolic-link/reparse leaves, non-regular files, invalid UTF-8, and platforms without
  a reviewed no-follow contract fail explicitly.
- Plain loading preserves the exact UTF-8 source, including BOM and newline
  representation. It does not interpret delimiter-looking Perl text.
- Sectioned loading preserves exact source separately from its newline-normalized
  parser view.
- Every section delimiter candidate must have a non-empty title and closing delimiter.
  The declared header count and parsed section count must agree exactly.
- Duplicate effective IDs fail the document.
- `SectionCaseId { asset_id, section_id }` is the stable identity. The legacy
  `Section.id` fallback remains leaf-derived compatibility data and may collide across
  parent assets.
- Intermediate-component containment is separate topology/path-authority work. Do not
  overstate direct loader containment.

## Evidence and distribution boundaries

- Topology, fixture, and generated registries identify observed evidence. They do not
  by themselves declare parser correctness, language support, or API stability.
- Required selected assets and directories fail closed on absence, symbolic
  link/reparse point, non-regular type, unreadable state, or escape under their owning
  layer.
- Legacy discovery and section APIs are compatibility surfaces, not evidence
  authority. Do not route new load-bearing work through them when a strict typed path
  exists.
- Packaging the complete repository corpus, or making a consumer distribution
  self-contained, requires a separate explicit and reviewed contract.
- The `gen` module is written as `r#gen` in Rust source.
