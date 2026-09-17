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
cargo test -p perl-corpus --test distribution_contract
cargo test -p perl-corpus --test gold_repository_contract
cargo clippy -p perl-corpus --all-targets -- -D warnings -A missing_docs
cargo package -p perl-corpus --allow-dirty --list
cargo run -p perl-corpus -- --help

# Generation is self-contained; evidence must retain the seed.
cargo run -p perl-corpus -- gen program --count 10 --seed 42
```

Select the smallest proof that discriminates the changed contract, then run the
applicable repository-level route from the root contract. A generated example without
its seed is exploratory output, not reproducible evidence.

## Programme train

The stable authority train for this crate's programme (#8826 / #6696) is checked data at
`.spec/10980-perl-corpus-stable-dag/train.manifest.json` (`perl_corpus_train.v1`). It
owns which result depends on which, which authority moves, which writers conflict, and
which legacy path exits; it never owns current implementation state or readiness.

```bash
cargo xtask perl-corpus-train check
cargo xtask perl-corpus-train graph --check
cargo xtask perl-corpus-train explain-static <node>
```

Before starting a leaf in this crate, read its node packet; a leaf never depends on an
umbrella, and a shared exclusive conflict key without a dependency path is a rejection.

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

### Gold member byte fidelity

`test_corpus/gold` assertions are `(line, character)` positions, so member bytes are
part of the contract, not an implementation detail.

- `byte_fidelity::ByteFidelity::classify` reads raw bytes only. It never decodes,
  normalizes, or replaces; invalid UTF-8 is reported with the offset of the first
  undecodable byte. Newline classification reuses `loading`'s `NewlineStyle` and its
  detector rather than restating them: `\r` and `\n` cannot occur inside a multi-byte
  UTF-8 sequence, so that detector is valid on raw bytes and needs no second
  implementation for the pre-decode case.
- `tests/gold_repository_contract.rs` classifies every gold member and requires the
  default class: LF terminators, a final newline, no BOM, valid UTF-8. That check runs
  before any decoded (`String`) view, so an undecodable member is named by path and
  offset instead of surfacing as an anonymous decode error downstream.
- A member that intentionally carries other bytes needs both halves or it is rejected:
  an entry in `BYTE_EXACT_DEVIATIONS` declaring its exact expected class, and a literal
  `-text` line for that path in the repository-root `.gitattributes`. The declaration
  without the git protection is not enough — the repository default is `* text eol=lf`,
  so unprotected bytes are git's to rewrite.
- Protection is resolved by `git check-attr text -- <path>`, not by reading
  `.gitattributes`. Git owns attribute resolution: rules are last-match-wins across the
  whole file, patterns use git's glob language, macros such as `binary` expand to
  `-text`, and per-directory attribute files participate. A reader that collected
  `-text` lines would call a path protected even after a later rule restored `text`, and
  so would admit a member git is free to rewrite. An unavailable or unparseable answer
  is an instrument failure, never a silent pass.
- An existing `$GIT_DIR/info/attributes` blocks a protection verdict outright. That file
  is untracked, clone-local, and overrides the committed `.gitattributes`, and no
  `git check-attr` invocation excludes it — `--source=<tree>`, `GIT_ATTR_NOSYSTEM`, and
  `core.attributesFile` were each verified not to. Since the contract's subject is
  repository-wide protection, a verdict derived from state only one clone has would be
  dishonest. Only a declared deviation reaches this check.
- The `gen` module is written as `r#gen` in Rust source.
