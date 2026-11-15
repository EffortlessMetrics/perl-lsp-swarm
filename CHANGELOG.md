# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

<!-- changelog coverage: retrospective_covered_through = da86c123a (this PR's
     base commit, conservative floor for the audited range). Post-audit
     user-facing PRs merged after this PR was authored are NOT covered here
     and are bridge-backfill candidates for the Changie ledger (#3784). -->

### Changed

- **Breaking (`perl-lsp-rs-core`): `performance::IncrementalParser` removed.**
  The type performed no parsing: it silently swapped reversed endpoints,
  fabricated one-byte extents for zero-length insertions, and answered
  byte-overlap queries, presenting a misleading second incremental-parser
  authority beside the canonical machinery (#8308; #6701/#7292 remain
  canonical). It had no repository consumers outside its own tests, and no
  equivalent range-tracker replacement exists — byte-range overlap was never
  parser work. Remove imports via `perl_lsp_rs_core::performance` and
  `perl_lsp_rs_core::tooling::performance`; incremental parsing needs belong
  to the canonical `perl-incremental-parsing` / `perl-parser` machinery.
  Per `docs/reference/STABILITY.md` this public-item removal requires the
  next minor release; it must not ship in a 0.17.x patch.

- **Breaking (`perl-lsp-rs-core`): `JsonRpcResponse::jsonrpc` is now
  `&'static str`.** Always `"2.0"` (see `JSONRPC_VERSION`); removes a
  per-response `String` allocation (#5053 item 7). Struct literals that
  passed `"2.0".to_string()` must pass `"2.0"`. Wire JSON is unchanged
  (serde serializes `&str` identically to `String`).

- **Breaking (`perl-lsp-rs-core`): `AiCompletionConfig` / `AiStreamingConfig`
  authority fields.** Public structs now carry `user_enabled` /
  `project_opt_out` (and streaming `user_enabled`) so project `.perl-lsp.toml`
  can only opt out of AI, never enable it (#4997 / #5022). Exhaustive struct
  literals must set the new fields or use `..Default::default()`. Semver minor
  bump lands with the next published release; `Default` is implemented for
  migration. See [docs/reference/AI_COMPLETION.md](docs/reference/AI_COMPLETION.md).

- **VS Code extension toolchain modernized to TypeScript 7 / Oxc / Rolldown**,
  shrinking the packaged VSIX by ~77% (1.25 MB to 291 KB). See
  [vscode-extension/CHANGELOG.md](vscode-extension/CHANGELOG.md) for detail.
  (#3645, #3690, #3721, #3736, #3755)

### Added

- **`perl-source-identity`: the canonical root-relative logical-path invariant is
  now enforced, not just documented.** `LogicalSourceId::from_root_and_path` has
  always required a forward-slash, root-relative path with no leading separator
  and no `..` segments, but nothing checked it — so a host absolute path, a
  traversal sequence or an empty string could mint a perfectly well-formed durable
  ID for the wrong source. New `RootRelativeLogicalPath::parse` returns a typed,
  **path-free** `LogicalPathError` (absolute, backslash, Windows drive prefix,
  `.`/`..` segment, empty segment, control character), and new
  `LogicalSourceId::from_root_and_logical_path` accepts only the validated type.
  Nothing is normalized: case and Unicode spellings stay distinct, because
  identity must reflect what the producer named rather than this crate's folding
  rules. The governed constructor delegates to the primitive, so published wire
  vectors are unchanged, and the crate still depends only on `serde` + `sha2`
  (#15555, under #7655/#4851).

- **Breaking (`perl-lsp-rs`): `PullDiagnosticsContext` gains a public
  `identity_root_path: Option<PathBuf>` field.** The struct has public fields, so
  any downstream exhaustive struct literal must add it; `PullDiagnosticsContext::new()`
  and the other constructors are unaffected. **Migration:** add
  `identity_root_path: None` to preserve today's behavior for a context that never
  had one, or set it to the same resolved root that produced `identity_root_key`
  to get root-relative identities. Per `docs/reference/STABILITY.md` this
  public-field addition requires the next minor release; it must not ship in a
  0.17.x patch. `NotReusable` also gains variants and becomes `#[non_exhaustive]`,
  so downstream matches need a wildcard arm — once — rather than a new arm per
  future refusal reason (#15555).

- **Pull diagnostics: report identity is now positioned relative to the owning
  workspace root.** The composer previously used `url::Url::path()` — the *host
  absolute* path — with its leading slash stripped, so the same logical file in
  two checkout locations produced two different logical sources, and a URI that
  failed to parse was passed through raw. A document inside its owning root is now
  identified by its path relative to that root; a document no root owns keeps a
  reusable result ID but is identified relative to its own directory, so host
  location lands in the root key rather than in the logical source. Absent root
  authority still yields no reusable ID (#7480), and `NotReusable` gained typed
  variants for the new refusals and is now `#[non_exhaustive]` (#15555).

- **Test2 framework awareness (reader/integration, not a Test2 runtime).**
  `perl-lsp` now reads Test2 source and Test2 runner output and drives the
  project's real runner, without replacing `yath`/`prove`/`perl` or executing
  `subtest` blocks in isolation:
  - **Imports** — `use Test2::V0;` (and common `Test2::Tools::*`) are understood
    so assertions like `ok`/`is`/`subtest`/`done_testing` are in scope, with
    exclusion (`!ok`), rename (`ok => {-as => 'my_ok'}`), and
    `-no_strict`/`-no_warnings`/`-no_pragmas` handled. Export lists are verified
    against the canonical Test2-Suite source.
  - **Critic** — `use Test2::V0;` satisfies the native `require_use_strict` /
    `require_use_warnings` rules (unless disabled via an import option).
  - **Subtest structure** — nested subtests are discovered as a tree and appear
    in the document-symbol outline and code lenses; dynamic names are reported
    as dynamic rather than guessed.
  - **TAP output** — runner output is parsed into structured failures mapped to
    source (`file`/`line`/`got`/`expected`); TODO/SKIP are not hard failures;
    raw output/exit code/runner are preserved.
  - **Run/Debug** — "Run Subtest" runs the whole file and focuses output on the
    named subtest; `perl.debugTestFile` returns a real `perl-dap` launch
    configuration for `.t` files (replacing the previous placeholder). Test2
    editor snippets (`usetest2`, `dies`, `lives`)
    are shipped. See [docs/reference/TEST2_INTEGRATION.md](docs/reference/TEST2_INTEGRATION.md).
- **Hover docs and completion for Perl 5.36–5.40 `use builtin` functions.**
  All 16 functions introduced by the `builtin` pragma (`true`/`false`/
  `is_bool`, `weaken`/`unweaken`/`is_weak`, `refaddr`/`reftype`,
  `ceil`/`floor`, `inf`/`nan`, `trim`, `indexed`, `load_module`,
  `export_lexically`) now have typed signatures, versioned descriptions, and
  resolve identically whether called bare or as `builtin::name(...)`.
  (#3118) `blessed` (5.36) and `is_tainted` (5.38) were added to the same
  catalog. (#3275)
- **`textDocument/semanticTokens/full/delta` support.** The server previously
  advertised `semanticTokensProvider.full` with no working delta handler and no
  `resultId`, so delta-capable clients had nothing to request against. Full
  requests now mint a tracked `resultId`, and the new delta handler returns the
  minimal edit set between a client's cached prior result and the current
  tokens (falling back to a full response when the prior result is unknown).
  (#1917)
- **Named-function-call semantic token class.** Bareword calls (`name(...)`)
  now participate in the same live-verified semantic-token cutover as
  sub/package/method/variable names, continuing the token-accuracy series.
  (#2609)
- **Native class fields (Perl 5.38+) extracted into symbols.** Fields declared
  with the native `field` keyword are now indexed and appear in document/
  workspace symbols and hover, alongside existing Moo/Moose `has` attributes.
  (#3218)
- **Perl 5.44 named-parameter signature help.** Signature help now models
  Perl 5.44 named/optional/slurpy subroutine parameters and the `//=`/`||=`
  named-parameter default operators, instead of treating them as positional.
  (#3331, #3352, #3354)
- **Completions route indirect-object calls through method completion.**
  `new Foo::Bar->` -style indirect-object call sites now offer the same method
  completions as `Foo::Bar->new->`. (#1914)
- **Declared-but-unindexed CPAN dependencies surface as advisory hover/
  completion text.** The server now statically extracts declared dependency
  facts from `cpanfile`, `Makefile.PL`, `Build.PL`, `dist.ini`, `META.json`,
  and `META.yml`, and when a module is declared but not indexed (not yet
  installed/vendored), hover and completion mention it instead of treating it
  as fully unknown. Per-folder config reload now also clears stale
  metadata-derived state instead of leaking it across reloads. (#3137, #3170,
  #3171)
- **`perl-dap` external debugger peer seam (ptkdb-ready).** A backend-neutral
  debug model plus a small Content-Length-framed peer protocol let `perl-dap`
  host an external Perl debugger frontend/backend (`Devel::ptkdb` is the first
  partner), reachable via `perl-dap --external-peer HOST:PORT` or a VS Code
  launch config's `externalPeer`. The native `DebugAdapter`/`DapServer` path is
  unchanged; this first cut wires a live, drivable **mirror-mode** listen
  session (proven with fake-peer tests), with real end-to-end `ptkdb` sessions
  deferred. (#3321, #3404)
- **Native analyzer/formatter/debugger stack is now the only shipped default.**
  `perl.runCritic` defaults to the native critic analyzer even when
  `perlcritic` happens to be on `PATH` (external only when explicitly
  configured); native-engine critic code actions carry native diagnostic
  identity (`perl-lsp-critic` / `native.*` codes) instead of the external
  tool's brand; the shipped `perl-dap` CLI no longer exposes `--bridge`
  (`Perl::LanguageServer`/`Devel::TSPerlDAP` bridging remains a library-only
  conformance reference, never a shipped path); and release archives now fail
  validation if they bundle external Perl tooling or legacy bridge payloads.
  (#3277, #3279, #3282, #3348)

### Fixed

- Keep POD-shaped regex text out of local subroutine discovery, preserving
  division for later undeclared names and declarations after the pattern (#17440).

#### LSP integration

- **Wire diagnostics resolve their published identity on every transport
  (#17241).** Native-critic (`native.*`) and dead-code (`dead-code-*`) codes were
  emitted on the wire but unknown to every metadata registry the server consults,
  so push enrichment downgraded them to `data.category: "Other"` with no
  `codeDescription`. Enrichment now resolves each code through its owning
  registry — the catalog for built-in `PL*` codes, the shared critic identity
  registry for native codes (the reviewed built-in alias's documentation page
  when exactly one exists; shape-split codes such as `native.security.qx_readpipe`
  resolve no page rather than guess), and the dead-code provider's own identity
  table — so push, pull, and code-action rows report the same category and
  documentation link for the same fact.
- **Pull no longer double-reports facts push collapsed (#17241, #5088).**
  `textDocument/diagnostic` appended native critic rows beside their built-in
  twins (`PL100` next to `native.testing.require_use_strict`, `PL405` next to
  `native.common.printf_format_arity`, …) with no overlap collapse. Pull now
  applies the same severity-exact, built-in-preferring collapse as push, and the
  code-action transport embeds the published twin's identity so clients can
  still associate a quick fix with its published row.
- **Critic-seam pull rows carry `codeDescription`** (`PL601`/`PL603`/`PL606`
  rows previously rendered without the documentation link the same fact has on
  push).
- **"Find All References" no longer silently degrades for its default request
  shape.** VS Code's default `includeDeclaration: true` request bailed out of
  the high-fidelity source-backed references tier entirely, falling through to
  the lower-fidelity workspace-index tier on every "Find All References" call.
  (#3050)
- **Stale post-edit index answers are guarded against.** Providers now wait on
  index readiness instead of racing a just-applied edit. (#3149)
- **Package rename fails closed on a partial index** instead of silently
  applying an incomplete rename. (#3176)
- **`callHierarchy/incomingCalls` returns top-level and script callers**
  instead of omitting call sites outside a named sub. (#3191)
- **Cross-construct sub resolution** now covers anonymous subs, typeglobs, and
  `use constant`-declared subs. (#3199)
- **Dynamic typeglob dereferences (`*{...}`) emit a `DynamicBoundary` fact**
  instead of being reported as a literal symbol. (#3209)
- **Completion keeps resolved imports when an export tag is unresolvable**
  instead of dropping the whole import. (#3213)
- **Same-package `our` redeclaration is flagged.** (#3217)
- **`ModuleNotFound` diagnostics use a boundary-aware `.pm` match** instead of
  a substring match that could misclassify unrelated paths. (#3221)
- **The outbound notification queue is bounded**, closing a DoS vector where
  an unbounded queue could grow without limit. (#3223)
- **`documentSymbol` `selectionRange` covers just the name**, not the whole
  declaration. (#3226)
- **Lexical `my`/`state` declarations are excluded from the bare-name unused
  check**, removing false-positive "unused" warnings. (#3227)
- **Malformed pull-diagnostic requests return `InvalidParams`** instead of a
  generic error. (#3229)
- **Completion adds regex-match array special variables** (`@-`/`@+` family)
  to the special-variable set. (#3255)
- **`eval`-scoped sub suppression for PL109 is now order-aware**, closing a
  false-negative where sub order affected suppression. (#3286)
- **Bare-block package context is scoped to the block** instead of leaking
  into sibling scopes. (#3353)
- **Clicking the package-prefix of a qualified name no longer navigates to the
  sub** — only the sub-name segment does. (#3360)
- **`typeHierarchy` operations support cancellation.** (#3455)
- **References refuse exact-tier promotion across dynamic boundaries**,
  avoiding a false-precision result where the reference set can't actually be
  proven exact. (#3461)
- **Hover and references guards restored** after a regression reintroduced the
  gap they closed. (#3466)
- **The PIR-A lexical references slice is promoted**, raising fidelity for
  lexical-variable "Find All References". (#3478)
- **`textDocument/typeDefinition` and `/implementation` fallback is pinned to
  the document generation captured at request time**, preventing a
  stale-vs-fresh mismatch mid-edit. (#3622)
- **Completion emits its analyze span on cancellation** instead of dropping
  timing data for cancelled requests. (#3623)
- **Hover documents `@+`, `@-`, `@EXPORT`, `@EXPORT_OK`, `%!`, and
  `%EXPORT_TAGS`** special variables, which previously returned no
  documentation. (#1866)
- **`DESTROY`/`AUTOLOAD` are recognized as special method hooks**, not
  ordinary `UNIVERSAL` methods, in hover and navigation. (#1836)
- **`positionEncoding` negotiation honors the client's preference list**
  instead of ignoring it. (#1856)
- **Document symbols nested under an offset-0 package are deduplicated.**
  (#1583)
- **Folding ranges are deduplicated**, and a region-start-line assertion
  corrected. (#3168)
- **Rename ignores non-code package text during scope lookup.** (#3112)
- **Rename validates keywords by context** — `$if`/`@while` are allowed as
  variable names, while `sub if` is still rejected. (#3109)
- **The workspace reference index no longer silently drops references inside
  block-form `package Foo { ... }`/`class Foo { ... }` bodies, typeglob
  aliasing, `goto &coderef`, regex-bind expressions (`=~`), `tie` argument
  lists, indirect-object call arguments, subroutine signature default
  values, and non-variable assignment/increment targets** — "Find All
  References", rename, and `workspace/symbol` now see these previously
  unindexed reference sites. Consolidating the workspace indexer's two
  independent reference walks into one traversal closed these
  pre-existing coverage gaps as a side effect. (#1711)

#### Debugger (DAP)

- **Breakpoint line-adjustment messages are preserved when the breakpoint
  condition is invalid**, instead of being dropped. (#3216)
- **Breakpoint file matching requires a path boundary**, preventing an
  unrelated file with a matching suffix from being treated as the same file.
  (#3225)
- **Process/thread ID casts saturate instead of silently truncating.** (#3233)
- **Launch remediation guidance points at the configured `perlPath`.** (#3245)
- **`DebugAdapter` implements `Drop`**, ensuring session cleanup runs even on
  an unclean shutdown path. (#3247)
- **Stale `Child` `variablesReference`s are short-circuited on a cache miss
  after resume**, instead of serving a reference from before the resume.
  (#3369)
- **Runtime-module generation clock fails closed at the saturating ceiling.**
  `RuntimeModuleGenerationClock::apply` no longer reports `advanced = true`
  when `next()` saturates at `u64::MAX` (the generation did not actually move),
  `GenerationAdvance::is_contiguous` now rejects an advance that claims to move
  without moving, and `reference_is_stale` returns `true` whenever the current
  generation is exhausted. Together with the existing executor admission
  refusal (`execute_reload` already refuses at an exhausted clock before
  mutating), the wire projector now refuses any attempt to publish
  `{previous: MAX, current: MAX, advanced: true}` via
  `GenerationAdvanceMismatch`, and any retained reference becomes stale at
  exhaustion independent of its bind point. (#14643)

#### Diagnostics

- **PL401 (two-argument `open` security check) fires for the parenthesized
  2-arg form** (`open(FH, ">file")`), which previously slipped through because
  the parser wraps parenthesized call args in a single `ArrayLiteral` node.
  (#3674)
- **PL404 no longer false-positives on sub-local lexicals**, and ties between
  same-offset scopes are now broken deterministically (innermost scope wins)
  instead of depending on hash-iteration order. (#3659, #3705)
- **PL100/PL101's implicit-strict module list corrected** against real Perl
  module behavior — several modules on the "implies `use strict`/`use
  warnings`" allowlist did not actually do so. (#3729)
- **Regexes with nested quantifiers no longer downgrade a document to partial
  semantics.** They're now recorded as an advisory diagnostic (still a warning
  in the editor) rather than a blocking parse error. (#3682, #3698)

#### Parser correctness

- **`goto &sub` frame replacement is distinguished from `goto LABEL` and `goto
  EXPR`.** (#1923)
- **`NodeKind::VString` added** for v-string literals, distinguishing them
  from other string forms. (#1871)
- **Incomplete-brace recovery marks the error explicitly** instead of masking
  it. (#1906)
- **A reversed `HashLiteral` span is fixed** by capturing the closing `}`
  token directly. (#3364)
- **Parenthesized `try` calls are disambiguated** from the `try`/`catch`
  statement form. (#3572)
- **`${name}` parses as a scalar variable**, not a symbolic dereference.
  (#3590)
- **Unparenthesized declaration lists are supported** (e.g. `my $a, $b`
  parses as a declaration list). (#3627)
- **Heredoc body offsets are tracked**, fixing downstream position mapping for
  heredoc content. (#3650)
- **Unparenthesized `my`/`our`/`state` declares only the first variable**,
  matching real Perl semantics instead of treating the remaining
  comma-separated names as part of the declaration. (#3738)
- **Encoding-aware file reading is consolidated**, fixing a crash on Latin-1
  and UTF-16 Perl source files. (#3054)
- **`line_index` overflow/panic sites hardened** against out-of-range offsets.
  (#3042)
- **Mid-surrogate UTF-16 columns are clamped** in `position_to_offset` instead
  of panicking. (#3040)
- **AST traversal recursion depth is bounded**, preventing a stack overflow on
  deeply nested or adversarial input. (#3207)

### Performance

- **Editor responsiveness: `didChange` returns before parsing completes.**
  Moved full parsing and parent-map construction out of the
  `textDocument/didChange` critical path and onto a bounded, latest-only
  background worker. Stale parses and parse-derived effects remain
  generation-gated. In the 78 KB, 20-edit Neovim receipt, maximum
  `didChange` handler time fell from about 269 ms to 1.6 ms. (#3618, Fresh
  Facts Fast program #3396)
- **Type inference is cached per document.** Hover and completion previously
  rebuilt the type-inference engine on every request for an unchanged
  document; it's now cached by URI and content hash and invalidated on
  `didChange`/`didClose`. (#3254)
- **`workspace/symbol` lookup is indexed** instead of an O(n) scan over every
  file's symbols on every query. (#3211)
- **Workspace-symbol search exits early once the result cap is reached**,
  instead of collecting and cloning every match before truncating. (#3228)

### Security

- **`anyhow` updated** to resolve RUSTSEC-2026-0190. (#3195)
- **Heredoc anti-pattern detector regexes bounded to a single line**, closing
  a ReDoS vector where an unclosed heredoc delimiter in a large document could
  trigger catastrophic backtracking. (#3568)

### Language intelligence
#### Added
- Code after an unconditional `goto LABEL` is now flagged as unreachable (PL406), matching the existing detection for die/exit/croak, including correct handling of forward goto targets inside continue blocks. ([#4699](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4699))
- Requesting the "Extract variable" refactor with a zero-width cursor (no text selected) now returns it as a greyed-out, disabled action with a "requires code selection" reason, per the LSP 3.16 disabled-action contract, instead of omitting it or offering an action that does nothing useful. ([#4727](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4727))
- Added a transport-neutral semantic provider query contract that keeps exact, degraded, fallback, refusal, stale, dynamic, ambiguous, and unavailable outcomes distinct from their fact producers and proof classes. ([#6844](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/6844))
- Added truthful provider-port adapters for current parser, semantic-analyzer, workspace-index, and already-canonical compiler or framework facts, with explicit completeness, generation, freshness, and producer boundaries. ([#6846](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/6846))
- Add parser-owned ParseDiagnosticAnchor API so downstream consumers preserve diagnostic positions across ParseError growth. Eliminates wildcard fallbacks that silently placed Recovered errors and future unknown variants at byte zero. ([#6886](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/6886))
- Add the transport-neutral framework-adapter SDK vocabulary to perl-semantic-facts, keeping durable input snapshots separate from live execution controls and exposing checked authority predicates so adapter output becomes publication authority only under validated production disposition, schema, generation, sink identity, and per-fact coherence. ([#6842](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/6842))
- Claude Code integration now has an evidence-backed `perl-lsp-rs` ↔ `perllsp` compatibility contract that distinguishes proven compatible, proven incompatible, and not-yet-proven pairs instead of inferring support from matching version numbers. ([#7924](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7924))
- The Linux/macOS installer can now compose binary installation with the Rust-owned Claude Code setup path through `--with-claude`, while keeping binary integrity, PATH proof, and Claude plugin lifecycle as separate verified stages. ([#7925](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7925))
- `perllsp` now has a Rust-owned Claude Code integration lifecycle with an idempotent `setup claude` surface and a read-only `doctor --client claude` diagnostic boundary, keeping marketplace/plugin state out of shell wrappers and prompt logic. ([#7916](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7916))
- Perl regex capture groups are now modeled from source with explicit numbering, named-capture families, and per-declaration confidence, so named, numbered, nested, duplicate, and branch-reset captures keep their real identity and unknown numbering is reported honestly instead of guessed. ([#7243](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7243))
- Register LSP 3.17 window-message payload validators on the protocol schema envelope. ([#10532](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/10532))
- Seeded the versioned builtin semantic catalog and admission guard (#6679): typed argument roles/contexts/access, side effects, boundaries, and proof states are validated in both directions, proven status requires typed implementation evidence, and the complete flag must cover the canonical lexer builtin inventory. ([#6767](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/6767))
- Add the pure #8048 shift-left seam for formatting: an atomic LF/CRLF/bare-CR final-newline policy module with independent insert/trim semantics and evidence bound to final returned bytes, plus a fallible independent edit-application oracle in UTF-16 and UTF-8-byte encodings that rejects reversed, unreachable, invalid overlapping, and mid-code-point edits instead of clamping them, while accepting distinct same-position zero-width insertions in oracle input order. Both remain offline-only and unwired in production pending the #10237/#10239/#10242 byte-native train; fixtures encode every #8048 negative control including str::lines() trailing-loss, force-LF into CRLF, insert-only collapse, trim-all, partial CRLF split, evidence timing, and no-op normalization. ([#11900](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/11900))
- Defined the MutationStructuredValue.v1 contract — versioned json:-prefixed bounded structured data for DAP mutation (exact integers/decimals, no f64, duplicate-key rejection, pinned size/depth/node budgets) with fresh unblessed ARRAY/HASH referent semantics, as domain types and validation only (S0 of the structured-mutation train, ([#12185](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12185))
- Defined the FilePirLexicalContributionV1 envelope — one immutable compiler-produced lexical fact object binding exact source/generation/body-HIR identities with ([#12180](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12180))
- Defined the compiler_lexical_cutline_cases.v1 manifest — the proof population for the first compiler-backed lexical references/rename cohort — with 16 pinned fixtures, 43 case rows, 37 controlled mutations, 18 work invariants, a JSON Schema, an xtask validator with list/validate/explain, and the 32-test compiler_lexical_cutline_manifest proof target; no product behavior (CP-LX01, ([#12470](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12470))
- Emit one versioned deterministic AST structural schema identity and a freshness-gated NodeKind inventory from the compiled structural registry. Field-only structural change is observable; stale checked status is non-green. Parser and AST structure are unchanged. ([#12806](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12806))
- Carmel R1 root/state leaf of the S8 research contract (#13642). perl-workspace-core gains an additive `BuildSystemKind::Carmel` identity (key `carmel`) and a bounded, non-executing detection module (`carmel`): marker facts for `cpanfile`, `cpanfile.snapshot`, `.carmel/MySetup.pm`, and the `local/.carmel` rollout sentinel; the post-rollout `local/lib/perl5` root candidate admitted only through the sentinel at WorkspaceConvention authority; dev-mode artifact roots parsed from the generated MySetup environment block (source-receipt-pinned to Carmel::App::dump_bootstrap's Data::Dumper shape) in producer order at Ambient authority with explicit `carmel_artifact_unverified`, `carmel_base_mismatch`, and `carmel_dev_state_unparsable` limitations; the Carton/Carmel `manager_ambiguous` lock window that attributes no roots; and the five relative watcher inputs for #13640 (ambient repository deliberately unwatched; interpreter identity is an invalidation event, not a glob). The dependency detector retires the baseless `vendor/lib/perl5` Carmel marker (no such path in Carmel source; `vendor/cache` is a tarball dir), admits the shared install-base root through the rollout sentinel, keeps Carmel dev-mode out of the relative-literal detector, and preserves the #4998 workspace-contained recurrence gate. ([#14213](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14213))
- perl-workspace-core gains a deterministic population compiler for ProjectEnvironmentSnapshot (#4833 residue, #2981 plan slice S1). WorkspaceEnvironmentDeclaration hand-feeds the environment sources that already exist on main (configured and admitted include roots, PERL5LIB activation policy, system @INC probe outcomes, explicit interpreter selection, bounded build-metadata facts, denied ambient observations) into the landed precedence model, and compiles to a fingerprinted snapshot with selected-and-rejected receipts: EnvironmentSnapshotReceipts classifies every input as selected or rejected with a typed reason, rejected_include_entries projects ineffective include candidates, and EnvironmentSnapshotSlot provides the generation-tagged refresh-consumer seam for later slices. The compiler is pure (no filesystem or process access, proven by an embedded self-source conduct check), slot feed order never changes the compiled snapshot (all-permutation proof), and the full pairwise precedence_rank contract table is enforced. ([#0000](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13940))
- The AST now carries a field-level registry of every payload field that holds its own byte offsets, recording each field's shape, the rule a coordinate remap must apply to it, and whether it anchors exact source, a source boundary, or recovery material. Recovery tokens are separated from ordinary ranges, since a token's byte width is validated against its text when the token is built and only its start may move; a remap must carry that recorded width across rather than recompute it. A geometry-bearing field added to an existing node variant now fails to build until it is classified, so it cannot silently remain in an old source generation while an operation reports success. ([#13985](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13985))
- Write the lexer budget-stop recovery contract into the crate docs and pin every reachable over-budget token shape with five executable contract tests: empty-text UnknownRest over the degraded remainder through EOF for regex scan steps, regex bytes, and heredoc bytes. Document MAX_DELIM_NEST as a separate local quote-like recovery path and pin its crate-private guard invariant. Also pin the two bounded-payload exceptions of EOF-inside-budget heredoc recovery and the MAX_HEREDOC_DEPTH heredoc-header error. ([#14362](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14362))
- Modules that declare their exports with Sub::Exporter's `-setup` hash now contribute those exports to importers, and a configuration that cannot be read statically records a dynamic export boundary instead of a partial export list. ([#14530](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14530))
- M01 of the module train (#8497): `perl-module` gains a protocol-neutral validated request and typed resolution-outcome vocabulary. `ModuleName` validates a canonical module name and records the source `PackageSeparatorForm`, making the legacy `'` separator an explicit `LegacySeparatorProfile` decision instead of an unconditional rewrite; `ModuleFilePath` validates a literal relative filename so a quoted `require "Foo::Bar"` stays a filename and is never rewritten to `Foo/Bar.pm`; `ModuleRequest` keeps bareword, literal-file, partially-static, and dynamic operands distinct, with `RequestBoundary` reasons and retained span/fragment evidence for the inexact forms, and refuses to expose a module identity for a filename; `ModuleResolutionOutcome` separates invalid, dynamic, ambiguous, outside-authority, environment-unavailable, timed-out, and I/O-limited states from a genuine `NotFound`, with `has_complete_denominator()` as the honesty predicate for reporting a proven absence. Compatibility adapters `outcome_from_uri_resolution` and `uri_resolution_from_outcome` carry their caller inventory and removal owner (#8521); the narrowing adapter returns `None` rather than collapsing a classification the legacy three-state enum cannot mean. `is_lookup_safe_module_name` becomes the boolean projection of `ModuleName::parse` so the module-identifier grammar has one authority instead of two, guarded by an equivalence oracle over a decision-boundary corpus, a valid-name-biased property generator, and a deterministic fuzzer. No root ordering, open-document precedence, filesystem policy, or provider behaviour changes. ([#14596](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14596))
- Operation execution authorization for the project environment model (#11095). perl-workspace-core gains a pure, versioned vocabulary that answers whether a specific user- or CI-requested operation may execute now, with these inputs, under this scope and authority: nine capability axes, a reviewed 15-entry operation registry mapping each profile to its exact required capabilities and scope class, an 11-class input-risk vocabulary, and a total evaluator compiling ExecutionIntent plus AuthorizationEvidence into an ExecutionAuthorizationDecision. Workspace source trust, environment input authority, input risk, user intent, capability requirement, policy denial, and execution authorization stay distinct facts; no boolean or enum ordinal stands in for the decision. Evaluation fails closed: every non-permitting outcome grants exactly nothing, the decision has no public constructor and no capability-set mutation, so an allowed-limited grant cannot be widened downstream, and a decision that is re-dated to another generation, widened in transit, or presented under a foreign schema version fails revalidation. Ambient PATH and Perl-environment inputs, and paths escaping the workspace root, are evaluated whether or not an intent declares them, so narrowing a declaration cannot buy authority; a disqualifying tool input is found before an enabling one, so a verified interpreter cannot mask a project-supplied wrapper beside it, nor a peer tool the user refused. Most-restrictive-wins holds within an input class as well as across classes, so enabling a cadence in one user setting does not override disabling it in another. Public explanations carry only stable classes, digests, generations, and reason codes while still naming the blocked capability and the authority a user can act on. No configuration read, trust transition, process spawn, product behavior, or client UI is introduced, and no production surface consumes it yet. ([#14696](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14696))
- Publish regex diagnostics from the parser's canonical analysis, with exact source ranges and stable PL1000-PL1007 codes. ([#14751](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14751))
- Mojo::Base classes now produce canonical object facts (#9682): generated read-write accessors anchored at their real `has` declaration, a distinct read result and fluent setter `ReceiverSelf` relation per accessor, and the literal parent inheritance relation. Facts mint only over an exact #9681 activation, so a same-named `has` call in an unactivated package emits nothing. Shadow-only — no provider surface changes. ([#9682](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/9682))
- `perl.explainMissingModuleLookup` reports the stored interpreter startup `@INC` acquisition state (configured off, not observed, transient or terminal timeout, unavailable, failed, legitimate empty, exact) and whether a not-found search was complete, without launching or retrying Perl. ([#14958](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14958))
- Record an explicit disposition for every public item on the bounded
`perl_parser::dead_code` compatibility surface, with a drift check that fails
when the record and the code disagree.

The ledger names what the surface cannot represent rather than implying it can:
two `DeadCodeType` variants that no producer ever constructs, an entry-point API
whose stored value is never read, a `confidence` field that is a single constant
on every finding, and six result states — including a partial analysis and an
incomplete computation — that arrive looking like ordinary results. Each claim is
held down by an executable fixture.

No analysis behavior changes.
 ([#0000](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15086))
- Add clean-parse summary and packaged injection-query helpers to tree-sitter-perl-c; retain the highlights snapshot as a documented drift-check source until the grammar and query snapshots are refreshed together. ([#12789](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12789))
- Preserve first-cohort scalar literal payloads (plain decimal numbers, noninterpolating strings, undef) as typed HIR body leaves with exact payloads and fabrication markers, resolved through PIR-A literal operations. Out-of-cohort literal forms retain their documented opaque behavior. ([#17641](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17641))
- Formatting options that have no effect with the native formatter (such as formatting.verticalAlignment) now produce a one-time client-visible warning naming the ignored keys, instead of only a server log line. The warning explains that the keys apply only to the external legacy formatter. ([#18075](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18075))
- perl-workspace-core gains non-executing ExtUtils::MakeMaker / Module::Build detection and typed source/generated root candidates (#2981 / #13636). `detect_eumm_mb` classifies Makefile.PL and Build.PL markers without executing build scripts, admits `lib` and `t/lib` as source roots with explicit test vs canonical precedence, and selects or rejects `blib/arch` and `blib/lib` with freshness, symlink/out-of-root guards, and typed rejection reasons for downstream environment snapshot wiring. ([#18767](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18767))
#### Changed
- Completion now reads types from a generation-owned type environment, so results stay correct under rapid edits. ([#3765](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3765))
- Add source-derived released-stable and current-master Helix capability profiles, replayed through the shipped server process to cover the watcher, workspace-configuration, and position-encoding negotiation boundaries that differ between the two clients. ([#7701](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7701))
- Derive PL406 unreachable-code diagnostics from typed local control-flow summaries so complete terminating branches close fallthrough while nested, dynamic, and partially proven paths remain conservative. ([#10004](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/10004))
- **Breaking:** perl-module implementation modules (`resolution`, `reference`, `rename`, `import`, `name`, `path`, `token`, `token_core`, `token_parser`, `import_match`, `boundary`) are now private (#8810 M10 seal). Every supported public item is reachable only through the crate-root facade (`perl_module::*`), including the newly published `use lib` include-path family, `build_effective_inc_roots`, and `parse_qw_arg_list`. All workspace consumers migrated to facade imports; compile-fail doctests prove internal paths no longer compile. ([#0000](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12257))
- **Breaking:** Sealed public perl-token geometry. Token, TokenRef, and TokenSpan start/end fields are private; kind is a read accessor so empty-span policy cannot be bypassed by assignment; unchecked constructors and silent reversed-span clamping are removed. Empty spans remain valid only for Eof and Unknown. This is an intentional pre-1.0 public API break. ([#12725](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12725))
- **Breaking:** Public perl-token constructors now require source text byte length to equal span width, including empty Eof and Unknown tokens. Retained public types keep their ([#12765](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12765))
- **Breaking:** Made DiagnosticCode iteration and serde use the stable PL### registry identity, added public FromStr parsing with ParseDiagnosticCodeError, and kept default PL001 metadata canonical. ([#13867](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13867))
- Removed the unreachable MAX_DELIM_NEST depth arm from the lexer's budget_guard (#14389) — its only caller passed depth 0, and delimiter nesting is enforced earlier by consume_nested_opener's rejection with local recovery in the balanced-segment helpers. budget_guard is now a byte-budget-only guard and the contract docs name the early rejection as the delimiter-nesting budget stop; no token stream changes. ([#14389](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14389))
- **Breaking:** LexerCheckpoint is now an opaque, versioned live-boundary snapshot. Production capture comes only from a live lexer; restore is typed and atomic and fails closed on source, content, generation, configuration, schema, UTF-8, unsupported-boundary, incomplete, or invalidated identity. Caller-selected at_position labels are no longer restart authority. This is an intentional pre-1.0 public API break. ([#14896](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14896))
- Provider decision explanations can select the latest references trace by its exact numeric or string request ID, refusing overwritten or unavailable request evidence instead of presenting another request's receipt. ([#15619](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15619))
- **Breaking:** The `compat` formatter mode is retired from the public contract (#7129). `compat` was a bare alias: it selected the native formatter and produced byte-identical output, so it named no behavior. It is removed from every public surface — the settings schema, its LSP4IJ projection, the `formatting.engine` valid-value lists, and the configuration reference — and the retired `compat` / `perltidy-compat` tokens are rejected on both configuration channels (#15624 closed the deprecation window before any release accepted the aliases). Migration: set `[formatting] engine = "native"` (or remove the key; `native` is the default). No formatting output changes — the alias always ran the native formatter. ([#15621](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15621))
- Dancer2 request gating now takes the earliest 1.x or 2.x `use Dancer2` site before resolving the module, so a 2.x-shaped document is not skipped as Dancer2-free. ([#18659](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18659))
#### Fixed
- Package-qualified braced scalars like ${Foo::bar} now fold to the plain scalar $Foo::bar instead of parsing as a symbolic dereference. ([#3939](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3939))
- `my $a = 1, $b;` now binds the initializer of $a to just `1`, matching Perl's `=`-binds-tighter-than-`,` precedence, instead of folding the trailing comma term into the initializer. ([#3908](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3908))
- Diagnostics now carry one of two coherent `source` strings (`perl-lsp` for all built-in diagnostics, `perl-lsp-critic` for external Perl::Critic findings) instead of the four divergent values (`perl-lsp`, `perl-lsp-critic`, `perlcritic`, `perl-parser`) that previously broke editor-side source filtering. ([#4681](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4681))
- Replaced internal 'Trust boundary' developer jargon in user-facing diagnostic `relatedInformation` messages with plain-language wording that Perl programmers can act on. ([#4705](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4705))
- Debugging on Windows: pause no longer risks killing the debuggee when the interrupt fallback fails, continue no longer misreports failure when there's nothing to resume, and terminate now sends a graceful Ctrl+C before force-killing, mirroring the Unix SIGTERM-then-SIGKILL escalation. ([#4719](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4719))
- Inline debug values no longer produce false-positive entries for variable names that appear inside comments or POD documentation blocks, and JetBrains-family clients now receive a one-time log message explaining why dynamic file-watcher registration was disabled for them. ([#4809](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4809))
- A bare `return;` with no value now gets a well-formed source range spanning the `return` keyword, instead of a degenerate/inverted range that could confuse features relying on accurate node spans. ([#4887](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4887))
- &foo(...) calls are now represented by a distinct AmperCall AST node instead of being indistinguishable from foo(...), letting the security diagnostics, semantic highlighting, and reference-tracking that were extended in this change treat the two calling conventions correctly. ([#4704](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4704))
- Go-to-definition/hover on a module reference no longer crashes the server with a char-boundary panic when the line contains multi-byte characters (emoji, accented letters, CJK) before the cursor. ([#4941](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4941))
- Document links for `use`/`require` statements inside POD now report correct UTF-16 columns for lines containing multi-byte characters, instead of using a locally-duplicated conversion helper that silently returned the wrong (raw byte) offset. ([#4965](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4965))
- A batch of 19 consolidated fix PRs across core language features; user-visible highlights: hover responses now include a `range` field so editors highlight the hovered token; overlapping critic/built-in diagnostics for the same issue are deduplicated instead of double-reporting; recovered parse errors now show user-friendly messages instead of leaking internal enum names; array rename now correctly distinguishes element access ($arr[0]) from bare scalars ($arr); the default workspace scan budget was raised from 500ms to 10s so large workspaces no longer silently index only partially; the file watcher no longer overwrites unsaved edits in open documents; and `use experimental`/`require VERSION` pragmas are now recognized for feature-gating. ([#5141](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5141))
- Lint diagnostics are now suppressed when the parser reports a blocking error, instead of piling spurious PL* findings from an unreliable recovered AST on top of the real parse error; a duplicate heredoc-folding path that produced overlapping fold ranges was removed; and workspace symbol search now supports fuzzy subsequence matching (e.g. 'gpn' matches 'get_page_name'). ([#5145](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5145))
- Completion no longer suggests lexical (my) variables that are declared later in the file than the cursor position, since they are not yet in scope; package globals (our) remain visible regardless of position. ([#5146](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5146))
- Range formatting now preserves the document's CRLF line endings instead of silently converting them to LF; a type mismatch on well-known boolean config fields now logs a warning instead of being silently ignored; and the `select` builtin's hover signature now shows both its filehandle and 4-argument forms. ([#5147](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5147))
- Inlay hints now show parameter names for subs using the common `my ($a, $b) = @_` unpacking idiom (including the `my $self = shift;` pattern), instead of only working for subs with an explicit formal signature. ([#5148](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5148))
- Hover's inherited-method lookup and document-links' use/require ranges now correctly index by UTF-16 column instead of raw byte offset, fixing both features for source lines containing multi-byte characters (emoji, CJK, accented letters). ([#5100](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5100))
- Type hierarchy now recursively lists all ancestor classes instead of only direct parents; call hierarchy traversal now descends into BEGIN/END blocks, try/catch, and other previously-skipped containers instead of dropping calls made inside them; and method calls like ->throw/->abort are now recognized as unconditional exits for unreachable-code analysis. ([#5151](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5151))
- Native Perl::Critic fixes marked as merely 'Suggested' (not 'Safe') are no longer offered as one-click quickfixes, preventing them from corrupting declaration-only renames. ([#5030](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5030))
- Rename now finds and renames $var references inside interpolated heredoc bodies instead of silently skipping them, and the debugger now displays ARRAY(0x..)/HASH(0x..)/CODE(0x..) reference values with their type and address instead of an empty, non-expandable container. ([#5152](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5152))
- documentHighlight now scopes highlighted occurrences of a subroutine-local variable to its enclosing subroutine instead of highlighting every same-named variable across the whole file. ([#5156](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5156))
- Typing a partial hash-reference key such as $ref->{ke now offers completion of keys seen in $ref->{key} = ... assignments (previously bailed out with no suggestions), and an unterminated string literal now only breaks lexing to the end of that line instead of losing diagnostics/completion for the rest of the file. ([#5157](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5157))
- The "Add use strict"/"use warnings" quick fixes now insert after the shebang line instead of at byte offset 0, so applying them no longer breaks script execution by inserting text before #!. ([#5169](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5169))
- Statement keywords (package, sub, use, etc.) are no longer suggested in expression positions (after =, commas, operators); '.' was added as a completion trigger character for string-concat and chained method access; and hover documentation was added for 14 Perl operators and 12 keywords that previously had none. ([#5179](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5179))
- Opening a single .pl file with no workspace folder now discovers .perl-lsp.toml from that file's own directory instead of silently ignoring it, so a lone script sitting next to its config is configured rather than served with defaults. Applies to every LSP client, not just the VS Code extension. ([#5184](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5184))
- A leading UTF-8 BOM (U+FEFF) in editor-provided document text is now stripped on both didOpen and full-document didChange, instead of shifting every column-0 offset by one character and producing stray glyphs in rename previews on Windows/BOM-encoded files. ([#5219](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5219))
- Variable completion now works inside string interpolation (e.g. "Hello $na|me") instead of returning nothing, and `package Foo 1.23 { }` no longer pollutes the package name with its version number in diagnostics and package-to-file mapping. ([#5271](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5271))
- Hash-slice dereferences (@$href{k}, @{$href}{k}, %$href{k}, %{$href}{k}) are now classified as HashSlice/KeyValueSlice AST nodes instead of falling through to a generic binary-expression shape that discarded their slice semantics. ([#5185](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5185))
- PL400 (BarewordFilehandle) and PL601 (SecurityBacktickExec) diagnostics now render at Warning severity as documented, instead of the Information severity the providers had hardcoded — PL601 in particular is a security finding that was appearing lower-priority than intended. ([#5286](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5286))
- &foo() (ampersand-sigil) calls are now recognized by call hierarchy, inlay hints, hover, moniker, and symbol extraction, instead of being silently skipped by those five providers as if they didn't exist. ([#5059](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5059))
- Six perl.limits.* settings (documentSymbolCap, codeLensCap, diagnosticsPerFileCap, inlayHintsCap) previously had no effect when set via LSP client settings; they are now correctly applied. ([#5307](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5307))
- The "Add use strict;"/"Add use warnings;" pragma quick fixes offered from the missing-pragma path now insert after the shebang line rather than at byte offset 0, and handle a shebang with no trailing newline by inserting a line break first, so applying them no longer breaks script execution or joins the pragma onto the #! line. ([#5166](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5166))
- Workspace/symbol fuzzy search no longer matches nearly every symbol for a 1-character query; subsequence matching now requires at least 2 characters, with exact/substring matching still available for shorter queries. ([#5337](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5337))
- Emacs setup guidance now distinguishes released lsp-mode 10.0.0 (Emacs 28.1+, with Emacs 28.0 outside that row) from the pinned upstream-source 10.0.1 subject with an unverified source minimum, qualifies stock-discovery wording against dated repository authority, keeps manual perllsp registration distinct, and directs wrong-server troubleshooting through explicit client selection instead of unbounded priority escalation. ([#7796](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7796))
- Defer workspace/configuration pulls until after the initialize response so strict LSP clients no longer receive an out-of-order server request during startup. Server-to-client requests are now rejected at the common request seam until initialization completes, and a pending configuration pull is cleared on shutdown. ([#7905](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7905))
- Fix two perl-regex event-scanner honesty defects: (1) scan \Q...\E bodies for Perl variable interpolation and record it as a quoted-literal runtime-text fact without degrading capture-numbering confidence, because the interpolated value is quoted before regex interpretation and cannot inject captures; escaped sigils (\Q\$x\E, \Q\@x\E) and backslash parity are resolved by escape pairs, and only genuinely interpolating sigils are recorded; (2) mark embedded-code constructs without a closing ) as malformed rather than silently treating them as well-formed. The PR also adds a missing regression proof for s///c (without /g), whose behavior was already correct on the base. ([#9768](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/9768))
- Stop the file-local sub prepass from learning declaration names out of comments, ordinary and quote-like strings, pattern bodies, POD, heredoc bodies, and data sections. A `sub NAME` spelled only in one of those regions could previously make a later `NAME /pattern/` lex as a regex call instead of division, so these were tokenization changes rather than indexing noise. ([#6755](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/6755))
- Route every single-range formatting request through one exact admitted-range contract shared with the atomic multi-range planner (#7082). Both requested UTF-16 endpoints now map through the same strict source geometry (LF, CRLF one-separator, supported bare CR, non-BMP two code units, terminal separator yielding a final empty line) before any formatter runs, so out-of-document end lines, past-line-end characters, mid-surrogate splits, and reversed ranges refuse as one typed unsafe-range outcome instead of being clamped or executed by a permissive engine validator. Applied range edits are contained against the admitted bytes plus canonical complete-line widening and any escape downgrades the projection to one typed not-proven result with no edits. The legacy line-slicing whitespace fallback is replaced by whitespace options applied strictly inside the admitted interval — character endpoints and end exclusivity are honored, final-newline options act only when the admitted target reaches true EOF, and nothing runs after refusal, failure, stale state, or ambiguous geometry. Single-range and one-element multi-range requests now admit or refuse identical geometry rows. ([#7082](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7082))
- DAP structured mutation values now decode strict JSON unicode escapes, including surrogate pairs, instead of refusing every escape form (#11328). ([#12581](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12581))
- Go-to-definition on the quoted path of a literal require (require "Foo/Bar.pm") now resolves to the module file through the position-aware @INC resolver instead of returning nothing (#12559). ([#12559](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12559))
- Make public AST whole-tree reads exact and stack-safe: count_nodes and find_deepest_containing_offset now walk the #8424 visit table iteratively with no silent MAX_AST_DEPTH truncation. Bounded variants return Complete/Truncated/InstrumentFailure instead of a plausible usize or Some after a caller-selected bound. Half-open containment is preserved; deepest lookup prefers greater depth then the earliest canonical path. ([#12639](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12639))
- Preserve complete embedded-code source ranges in canonical regex facts and dynamic regions while retaining opener-anchored diagnostics. ([#13073](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13073))
- Parse scalar-filehandle print calls whose first message term is numeric through the established indirect-call AST path in statement and expression contexts, while leaving comma-form calls and scalar subscripts on their existing paths. ([#13125](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13125))
- Accepted parser-supported keyword and composite term forms after Perl's string-repetition operator without treating `x =>` keys as repetition. ([#13102](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13102))
- Unicode letter/digit package names now remain byte-exact through direct `use`/`require` reference extraction, lookup safety, and module-path projection instead of being rejected as ASCII-only. ([#13159](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13159))
- Recognize Perl's contiguous string-repetition assignment operator (`x=`) as a right-associative assignment in direct expressions, variable-declaration initializers, and the `pos` named-unary call path. Spaced `x =` is never normalized into the operator; the same-line leftover parses as ordinary tokens without a repetition diagnostic. ([#13169](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13169))
- Incremental parsing now reuses the prior tree for exact whitespace-only edits without shifting nodes that precede the edit. The fast path verifies the full source transition and lexer token identity, preserves fresh-parse span geometry, and leaves comments or structural replacements on the conservative fallback path. ([#13180](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13180))
- Perl 5.42 native-class and Object::Pad fields using :writer(NAME) now expose the explicitly named writer to language intelligence instead of silently dropping it. ([#13283](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13283))
- Retired the incremental parser's partial node-position index: construction no longer clones every indexed subtree, and containment lookup now walks the canonical child traversal, so `find_containing_node` reports the smallest containing node under assignments, loops, method calls, and every other structural family instead of an ancestor or nothing. Simple value edits inside subroutine bodies, loop bodies, and method-call arguments are now admitted by the incremental path and produce trees identical to a fresh parse; the shifted-clone fallback shifts every canonical child slot, fixing stale sibling positions after length-changing edits. ([#0000](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13917))
- Route parenthesized `substr`, `vec`, and `pos` lvalue assignments through the contextual assignment parser so contiguous Perl `x=` is recognized without changing ordinary binary `x` or trivia-separated `x =` forms. ([#13496](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13496))
- Preserve contiguous Perl `x=` when a list declaration or a declaration used as a call argument is the assignment target, while retaining ordinary list assignment and binary repetition behavior. ([#13497](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13497))
- Preserve simple and compound assignments embedded in `local` declarations in canonical body HIR, including read-modify-write access for contiguous `x=`, and make `local $x = EXPR` declaration nodes span the whole assignment instead of stopping at the localized variable. ([#13500](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13500))
- Report the exercised invalid same-line residual syntax as a typed recovered parse instead of silently splitting it into additional clean statements, while preserving the selected tested low-precedence and statement continuations. ([#13505](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13505))
- Bound heredoc body collection with a deterministic source-byte budget instead of a five-second wall clock. The old timer spanned the whole enclosing statement, so a loaded host, tracing, or a debugger pause could drop heredoc bodies from valid source and report the loss as a syntax error. Identical source and parser configuration now produce identical diagnostics and AST on every host, and exhausting the budget is reported as a typed resource limit (`ParseStopCause::HeredocBudgetExhausted`) that leaves the affected heredoc placeholders explicitly unresolved rather than silently empty. ([#13781](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13781))
- Keep conditional cpanfile declarations out of dependency advisories: canonical `on '<phase>' => sub` declarations are attributed as `{phase}.{relation}`, while declarations inside `feature` blocks, platform conditionals, loops, arbitrary callbacks, and unknown phases are suppressed instead of being reported as unconditional. ([#13890](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13890))
- Incremental parsing no longer accepts trees whose payload sub-spans point at pre-edit offsets. Edits inside numeric, string, variable, or identifier leaves that change text length now remap heredoc body spans, package and subroutine/method/class/format name spans, phase-block keyword spans, try/catch variable locations, and recovery token spans together with the node spans they anchor, so navigation after an incremental edit lands on the bytes the range claims instead of silently wrong nearby text. ([#14350](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14350))
- The navigation provider-decision receipts now tell the truth about how current their answer is. Goto-definition and type-definition `freshness` is derived from what those providers actually answered from (`ProviderDecisionFreshness` snake_case) instead of always reporting `fresh`. An empty goto-definition result over a stale workspace index reports `unknown`; a live-document answer and an empty result over a current index still report `fresh`. Type-definition continues to report `stale` for a request behind the live document version. Receipt/observability fields only; no change to navigation results. ([#14193](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14193))
- `perl-parser-pest` now selects an explicit heredoc contract (#8220): an opener owns the lines below it up to its terminator, those bytes become `AstNode::Heredoc.content`, and they leave the text handed to Pest so following code resumes after the terminator instead of the body being re-parsed as statements. A heredoc that cannot be owned truthfully — missing terminator, body over budget, too many openers on one line, or Perl-illegal `<< MARKER` — now carries a typed diagnostic through the new `parse_heredoc_outcome`, so an empty content can no longer masquerade as a complete parse. Two cases are reported rather than fixed, and are visible through the same surface: `<< MARKER` stays admitted by the grammar and is recorded as unsupported syntax, and a residual disagreement between the pre-pass and the grammar about which `<<` opens a heredoc is reported instead of returning a clean parse. Consumers reading fixture classifications should note that `heredoc.bare-eof` moves to `valid` as a result. ([#8220](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8220))
- Scope diagnostics defer new `my` and `state` bindings until the whole modified statement ends. Both `print $x if my $x = 1;` and `my $x = 1 if $x;` now report `UndeclaredVariable` under `use strict`, matching Perl. Existing and outer bindings remain visible, and completed nested blocks expose only their own declarations. The two halves are still analyzed in evaluation order, so a match in the condition still sets `$1` for the statement. `our` is not deferred and is still reported by its own condition — a pre-existing source-order alias boundary tracked separately. The deferred bindings become available to following statements while retaining redeclaration, shadowing, and unused-variable diagnostics. ([#14840](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14840))
- The server now advertises `workspace.workspaceFolders.supported` from its own implementation truth instead of mirroring the client's advertised bit or the number of folders that happened to be open, so a client that omits or disables its own workspace-folder capability no longer sees the server claim it lacks the feature. The standard top-level `workspaceFolders`, `rootUri`, and `rootPath` inputs are classified once into an explicit disposition, keeping four separate facts apart: what the server implements, what the client advertises, which folders are active, and where the standard root input came from. A present `workspaceFolders` of `[]` or `null` is an explicit no-active-folder declaration and no longer silently adopts `rootUri`, `rootPath`, or the server's working directory; the legacy `rootUri` then `rootPath` fallback applies only when the field is genuinely omitted. Separately, a client that sends `initializationOptions` carrying no recognized compatibility root stays rootless rather than indexing the launcher's working directory. ([#14181](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14181))
- Split completion keywords by syntactic role so value positions still offer anonymous sub/do/eval while suppressing statement-only keywords like package. ([#14883](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14883))
- The `perl-lexer` local-subroutine prepass no longer learns declaration names out of Perl `format` bodies, so report text such as `sub NAME` on a picture line can no longer create a false known sub and flip a later `name /pattern/` from division to the regex path. Terminator handling matches perl exactly: a line whose content is exactly `.`, allowing trailing horizontal whitespace and rejecting leading whitespace. The opener covers the single-line forms — an optional name (qualified, or absent for `format =`), optionally preceded by statement labels, followed by `=` as the last non-comment token on the line; openers that perl accepts across several lines are not recognized and keep their existing behavior. A body is treated as a region only when a terminator is present, so an opener-shaped line that is really prose cannot hide the declarations after it. A `sub` declared on a format *argument* line is deliberately not recorded; both bounded limitations are documented on `LocalSymbolTable::scan_subs`. ([#14943](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14943))
- Reject invalid UTF-8 in incoming Content-Length JSON-RPC bodies instead of rewriting them with U+FFFD, and surface malformed JSON and JSON-RPC shape failures as typed one-frame outcomes. The reader reports; the runtime currently logs payload-private metadata and continues. Exact respond/close/exit disposition remains owned by the malformed-input programme. ([#14890](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14890))
- Hover on a special variable interpolated inside a double-quoted string (for example `$!` in `die "Cannot open: $!"`) returns its documentation again; the #4967 proven-code routing had failed it closed with the rest of the literal, while builtin and keyword cards inside strings stay suppressed (#14860). ([#14871](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14871))
- Bind an unbracketed, unparenthesized qw list immediately after a lowercase bareword list operator into one FunctionCall with flattened words, so `has qw(a b)` is one call instead of a bare identifier plus an orphaned list. ([#14909](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14909))
- Live definition, document-symbol, workspace-symbol, and references runtime proofs now identify themselves as semantic source-backed (`source_backed_receipt`) instead of claiming a compiler/PIR producer, with one recurrence guard covering all four cohorts. ([#14918](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14918))
- Complex-lvalue `local` (elements, slices, parenthesized argument form) no longer lowers to a placeholder `<unknown>` Let/StashWrite; body HIR and PIR-A keep the real target place or an explicit fail-closed marker. Named typeglob `local *FH` and arrow-postfix `my $cache->{key}` recover the real declared name instead of dropping the binding. ([#14930](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14930))
- EnvironmentFingerprint now hashes each published path's redacted public_id alongside the internal normalized half, so a different redaction map cannot share a fingerprint. ([#14939](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14939))
- Ordinary binary string repetition now accepts already-tokenized v-string, quote-word, quote-command, regex, substitution, transliteration, eval, and heredoc terms after `x`, without treating key, method, package, hash, or direct-call uses of `x` as repetition. ([#14963](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14963))
- Explicitly named `field` traits are no longer discarded. The compatibility class model compared attribute spellings against exact bare strings, so `field $x :reader(read_x)` generated no reader at all and `field $x :param(external)` left the field out of the constructor entirely; only `:writer` had named support, on its own inline parser. One decoder now owns argument identity for `:param`, `:reader`, `:writer`, `:accessor`, and `:mutator`, distinguishing bare, static-identifier, literal-text, empty, and unclosed forms. A `:param` argument is treated as a constructor key rather than a method name, since Perl accepts arbitrary key text there (`:param(foo-bar)` and `C->new('foo-bar' => 1)` compile on 5.38), while a generated method still requires a callable identifier. The default key for a bare `:param` is framework-specific: Object::Pad removes one leading underscore, the core `class` feature keeps the name the source wrote. Constructor-parameter completion now offers the key `new` actually accepts and quotes it when it is not a bareword, so selecting a non-identifier key no longer inserts Perl that fails to compile or silently names a different argument. Profile admission is unchanged — a named form is admitted exactly where its family's bare form already was, so `:writer` keeps its existing gating. ([#14997](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14997))
- Complete the reviewed Dancer2 1.x keyword vocabulary, correct request-only keyword scopes, and keep version-specific completion, hover, diagnostics, and canonical handler-context provenance aligned with the observed module. Withhold unusable deprecated completion suggestions, explain deprecated keyword hovers, and respect package ownership when suppressing diagnostics for local subroutines. ([#13178](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13178))
- Legacy source-symbol search retains the strongest matching alias before geometry deduplication, so an exact bare name does not lose its ranking to a weaker qualified alias visited first. ([#12652](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12652))
- Recover incomplete postfix dereferences with diagnostics while preserving selector spans and the following statement. ([#14740](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14740))
- Exporter metadata extraction handles UTF-8 word-list delimiters safely while preserving exported names, ordering, and definition filtering (#15249). ([#15252](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15252))
- Preserve parenthesized tie expressions and their source spans in nested constructor arguments, function calls, and initializers. ([#16155](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16155))
- Keep scalar print-filehandle heredoc prose out of local-sub discovery and preserve later slash disambiguation. Pending heredoc bodies now take precedence over POD/comment skipping at the header newline. ([#16169](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16169))
- The local-subroutine prepass now recognizes definite term-slot heredocs after `return` and after fat commas, including under statement modifiers, so heredoc body prose stays out of known-sub discovery and later slash disambiguation. ([#16336](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16336))
- Report invalid spaced repetition assignment and missing repetition operands while preserving useful partial expressions for editing. ([#16145](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16145))
- Bare q and qq arguments remain inside no directives with their complete spelling, and malformed quote tokens in raw no arguments and shared import pair values retain diagnostics. ([#16184](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16184))
- Preserve Perl word-not operand boundaries in string repetition, including nested lists, assignments, explicit parentheses, and malformed-operand recovery. ([#16177](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16177))
- Bare quote-word arguments now remain inside no directives instead of becoming a separate expression, preserving the following statement. ([#16146](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16146))
- **Breaking:** Preserve whether a regex expression came from qr//, m//, or a bare /.../ in the parser AST and canonical body HIR. This changes the public Regex AST and HIR shapes. ([#16512](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16512))
- The perl.runCritic command reports a bounded error when external perlcritic exits non-zero without producing violations (most commonly an unparsable .perlcriticrc) instead of reporting the file clean, labels the built-in fallback analyzer as "builtin" instead of "native", and warns once per session when the configured external perlcritic binary is unavailable. ([#16634](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16634))
- A `textDocument/completion` sent while the workspace index was building no longer permanently wedges the server (#16651). The startup scan's per-file critical section held the `workspace_folders` mutex across each `index_file` commit, while a diagnostics publication running inside `with_semantic_queries_for_uri` held the workspace index's four semantic-map read guards and resolved modules through that same mutex from inside its callback — an ABBA deadlock that froze indexing mid-scan and left every subsequent request (completion, hover, definition, didOpen, and eventually shutdown) unanswered for the rest of the session. The folder-membership recheck now scopes its guard to the check itself, so the indexer never holds the folders mutex while blocking on the semantic-map commit locks. Wire receipts on a ~2,025-file workspace: mid-scan completion answered in 2.0s (100 items) with indexing reaching `index-ready ready` afterwards, versus pre-fix 170-260s of total inbound silence and a permanently frozen scan. Regression scenario `ux_scenario_71_completion_during_indexing` reproduces the wedge against the unfixed server (completion deadline expires at 45s with the stream still live) and passes on the fix. ([#16651](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16651))
- Stop recommending `cpanm` when hovering a workspace-qualified `Pkg::sub` that is already indexed. ([#16686](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16686))
- textDocument/references no longer counts hash-key literals (fat-comma keys and `{name}` subscripts) as subroutine-name hits, keeps qualified `Pkg::name` calls, interpolated variables in double quotes, block bare calls, and braced array/hash dereferences on their intended query kind, emits sigil-inclusive variable ranges, and collapses identical (uri, range) locations before the result cap. ([#16683](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16683))
- Extract to subroutine preserves ordinary scalar sigils and first-use parameter order, retains the call-site lexical block, chooses an unused name, and withholds edits for unsupported captures, control flow, scope, or insertion points such as sub text inside strings. ([#16667](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16667))
- Hover no longer requires an open editor: a workspace file that is indexed on disk but not open now answers hover from the same disk index references, go-to-definition, and workspace symbols already use. Lexical variables and sub declarations in never-opened files return the same cards open documents get (declaration, declared-at line, lexical scope), instead of null. Files the index does not hold, or whose disk content is unreadable, still answer nothing; if the file is opened while a hover on its disk content is in flight, the open buffer supersedes the disk answer. ([#16743](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16743))
- The native parser now rejects sigil-less items in lexical declaration lists (`my (base) = @_;`), so `perllsp --check` is non-clean while valid `my ($base) = @_;` stays clean. ([#16769](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16769))
- Withhold the basic Extract to function fallback for all blocks because it cannot prove lexical-capture and brace safety; supported enhanced subroutine extraction remains available for admitted selections. ([#16793](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16793))
- Method completion offers same-file composed-role methods before the workspace index has published symbols. ([#16850](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16850))
- An `our` alias declared in a statement-modifier statement (for example `our $x = 1 if $x;`) is now visible to the modifier condition instead of being falsely reported as undeclared, while condition-side `our` declarations still take effect only at their own source position, placeholders stay uninitialized until the guarded assignment runs, and existing bindings and same-scope redeclaration diagnostics are preserved. ([#16108](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16108))
- Method completion again offers inherited and workspace-indexed methods when the edited buffer declares the receiver package. The same-file completion change composed open-buffer methods with persisted members but dropped the persisted `use parent`/role chain and persisted explicit members for open packages, so `$self->` in a partially-edited module lost `inherited_method` and workspace-only methods; open-buffer facts now keep precedence while persisted members and inheritance edges compose back in. Persisted facts from the edited document itself now respect document freshness: a deleted or renamed method no longer resurrects from the stale index generation, an explicit `our @ISA = ();` reset and a removed `use parent`/`with` line suppress the persisted edges, and cross-file plus generated members still compose. ([#17045](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17045))
- Preserve immediately parenthesized uppercase and qualified calls passed to print, printf, or say, avoiding false strict-subs bareword errors while retaining explicit filehandles. ([#17079](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17079))
- Accept implicit-topic for/foreach lists beginning with my declarations, including Perl core parent.pm, and complete declaration expressions with binary/ternary operators and exponentiation in loops, conditions and arguments while preserving C-style separator diagnostics. ([#17083](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17083))
- Server-spawned Perl subprocesses no longer inherit the LSP stdin pipe. On Windows, every console child of the language server that held that inherited handle blocked inside process initialization until killed, so perl.runFile, perl.runScript, perl.runTests, perl.runTestSub, perl.runSubtest, and the perl fallback always burned their full 30-second timeout and returned no output, and the system-@INC probe silently timed out. Children now get the null device on stdin at every spawn seam (execute-command runner, @INC/version probes, and the shared subprocess runtime), so short scripts run in well under a second. ([#17305](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17305))
- textDocument/foldingRange heredoc folds now cover the entire body: the last body line no longer stays unfolded for bodies of 3+ lines (2-line bodies were already correct), and the terminator line remains visible, matching the block-fold convention. ([#17316](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17316))
- textDocument/codeAction no longer offers command-bearing actions no component can execute: "Generate test for '<sub>'" (perl.generateTest), "Add debug print" (perl.addDebugPrint), and "Convert globals to 'my' declarations" (perl.convertToMyDeclarations) were advertised in the lightbulb menu but registered in no executeCommand dispatch, no executeCommandProvider capability, and no editor extension, so executing them always failed with -32601 Unknown command. A contract test now pins every offered code-action command to a registered executor. ([#17304](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17304))
- Report a syntax error for direct subscripts after a postfix hash slice, while preserving parenthesized and explicit-arrow forms. ([#17207](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17207))
- In --socket/--port mode, the LSP `exit` notification from one connection no longer terminates the whole server process. The exit handler now scopes teardown to the sending connection: it settles that session's outbound writer, releases only that session's serve loop, and the frontend closes the peer socket, so the listener keeps accepting and concurrent sessions keep serving (#17331). A harness that runs sessions sequentially no longer has to restart the server per session, and a second concurrent client no longer loses its session with a bare connection reset when another client exits. Stdio keeps today's process-exit semantics because its single client owns the process, and the shutdown-received exit-status rule is unchanged. ([#17374](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17374))
- The symbol-table prepass no longer treats a line-initial POD directive directly after a code line ending with a bare `/` as a POD opener: the directive may be the body of a multiline bare regex (`my $r = /` newline `=pod`), which the prepass cannot track, and trusting it hid every following `sub` declaration so later `name /regex/` sites could disagree with the lexer. The guard arms only from code-state scans, quote-like closers never arm it, and it clears after one line so a real POD opener is delayed by at most one line. ([#17324](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17324))
- textDocument/references now resolves a `package` declaration cursor to the same symbol identity as a usage cursor (find-references on `package Animal;` returns the full cross-file usage set instead of only the declaration), collapses cross-file duplicate locations (call-expression vs name-token spans, declaration span vs name token, and Windows drive-case URI spellings) so one occurrence answers once, and goto-definition on a hash-key word inside a method call resolves to the exact-name sub of the receiver's package or stays honestly empty instead of jumping to the constructor. ([#17157](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17157))
- Make the public complete-program HIR body entry reuse canonical lowering, preserving structured expressions, source maps and scope-derived bindings. Non-Program AST fragments retain their documented legacy behavior. ([#17541](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17541))
- Rename preparation and direct rename now refuse tokens classified as line comments, POD, or data sections, including cursor positions just after a comment word, instead of offering a rename box for non-code text (#17364). ([#17554](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17554))
- Honor existing Dancer1 static quoted-target references in definition navigation and reject containing-route answers for missing named CodeRefs. ([#17488](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17488))
- Perl qw word lists retain literal hash characters and following words, including tags and URL fragments, with exact word spans and existing incomplete-list recovery. ([#17585](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17585))
- workspaceSymbol/resolve now answers spec-shaped client requests instead of returning Method not found, matching the resolve support the server advertises. Rename, prepareRename, and workspace-symbol contract tests assert exact edits, UTF-16 ranges, and symbol results so empty or null responses fail. ([#17692](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17692))
- Workspace symbol discovery in projects without Git now keeps explicit dependency include paths scoped to their configured subtrees, excluding unrelated sibling files under vendor or local directories. ([#17788](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17788))
- The parser no longer panics on malformed heredoc input that produced an inverted statement span; the fuzz-found crash input is committed as a regression artifact so the failure cannot silently return. ([#17766](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17766))
- Closing-brace quick fixes preserve the block body, handle trailing comments, and withhold incomplete repairs. Stale client parser diagnostics no longer authorize edits on repaired source. ([#17794](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17794))
- Prevent a UTF-8 boundary panic when the core rename provider scans Unicode subroutine names with optional comment or string renaming enabled. ([#17791](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17791))
- Clear pull diagnostics and their reusable result IDs after rejected text changes until a complete document replacement restores synchronization. ([#17756](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17756))
- Filter generic object-method completion candidates by the typed method prefix, retaining matching methods and the empty-prefix catalog. ([#17767](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17767))
- Lexical array and hash renames preserve scalar element-access sigils, avoiding accidental slices while leaving unrelated and shadowed bindings unchanged. ([#17792](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17792))
- Preserve known signature help and active argument selection in unfinished calls after Unicode text, including DBI method signatures. ([#17787](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17787))
- Fixed document-symbol name selections for package and named subroutine declarations whose names also occur in keywords or bodies, including declarations inside conditional blocks and malformed editable code. ([#17778](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17778))
- Show declared method parameters, including defaults and slurpy parameters, in signature help with method labels, omitting explicit receivers in the parser-supported colon dialect. ([#17811](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17811))
- Preserve variable sigils in hover fallback so unresolved builtin-name variables receive neutral token cards while actual builtin calls retain their documentation. ([#17864](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17864))
- Package rename previews preserve qualified names on the canonical two-file scenario, refuse unsupported target names before compiler planning, and avoid presenting stale open-document facts as a fresh compiler plan. Previews continue to apply and return no live edits. ([#17980](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17980))
- Reject malformed pull-diagnostic cache IDs with actionable parameter errors while preserving valid cached reports and same-session recovery. ([#17956](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17956))
- Workspace method rename preserves a same-named scalar receiver and its arguments, including Unicode names, instead of editing the receiver in place of the method. ([#18012](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18012))
- Hovering a built-in or a call-site package name inside a subroutine no longer returns the enclosing subroutine's definition card; hover now answers for the token under the cursor or declines. ([#18025](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18025))
- Workspace-wide diagnostic pulls now honor syntax-only diagnostic mode, matching push and per-document pull. Previously workspace/diagnostic ignored the mode and ran the full semantic, critic, and module-resolution analysis for every open file, so latency measurements and reduced-mode clients silently received full-cost results from that transport. ([#18100](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18100))
- Restoring, recreating, or externally editing a dependency file now refreshes diagnostics in open files that use it. Previously only deletions triggered a republish, so a module brought back by git checkout or code generation left stale module-not-found warnings in open editors until the buffer was edited; pull-diagnostics clients are now asked to re-pull as well. ([#18099](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18099))
- The completion-candidate ledger digest no longer moves on comment-only, newline-only, or other position-only edits. Inline span byte ranges in the stripped-AST rendering are blanked before hashing, so the ledger gate stays green unless the shipped producer surface actually changes. ([#18086](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18086))
- Folding ranges for #region/#endregion markers now keep exact byte offsets in CRLF files. Previously each CRLF line shifted later folds by one byte, folding the wrong lines and, with multi-byte characters nearby, risking a server panic. ([#18076](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18076))
- Stop a server panic when a use statement carries an unterminated or mismatched qw delimiter (for example use parent qw•Exporter; typed mid-edit). Exporter detection now treats the inverted delimiter range as no qw content instead of slicing past the end. ([#18074](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18074))
- Workspace diagnostic pull now returns an empty full row for previously reported documents that are no longer open, so editors clear stale diagnostics for closed or deleted files instead of keeping them indefinitely. ([#18051](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18051))
- Loop variables declared with for my are now indexed as lexicals in the semantic symbol table, so analyzer-backed definition and hover resolution returns the variable instead of falling back to the enclosing subroutine. ([#18042](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18042))
- Hover documentation for a subroutine now shows only its immediately preceding POD block. Previously every POD block from the first =head1 onward, including intermediate subroutine source, was presented as the hovered sub's documentation. ([#18039](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18039))
- Workspace diagnostic pulls now clear stale diagnostics for closed files even when the client echoes a malformed previous result ID for them. Previously an entry with a missing or non-string value was dropped entirely, so the closed file never received its clearing report and its old warnings stayed in the editor until restart. ([#18102](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18102))
- Two byte-offset helpers used by folding ranges and native critic findings now floor offsets to UTF-8 character boundaries instead of panicking when an offset lands inside a multi-byte character. Externally derived or drifted spans can no longer crash the server through these paths. ([#18103](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18103))
- POD-coverage warnings and debugger inline values now use exact byte offsets in CRLF files. Previously each CRLF line shifted later positions by one byte, so coverage warnings pointed at the wrong text and inline values after multi-line strings could be misclassified and dropped. ([#18106](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18106))
- Choosing a local subroutine or constant completion now replaces a qualified suffix after the cursor without leaving `::Tail` behind, including ampersand calls. A single ternary colon remains untouched. Ordinary mid-token and Unicode suffix support landed in #18265. ([#18114](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18114))
- Package-member and use/require module completion replace the remaining identifier segment after a mid-token cursor, including Unicode suffixes, while preserving a following `::` segment and whitespace-separated arguments. Core-module member fallback keeps its package qualifier when applying the edit. Builtin and keyword suffix support landed in #18265. ([#18114](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18114))
- Open files now refresh their diagnostics after a dependency is saved, created, or renamed into place. Saved-text updates refresh consumers after the new source is indexed, and successive file operations also refresh clients using pull diagnostics. Previously these operations could leave other open files showing stale errors or stale clean state until they were edited. ([#18120](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18120))
- Parser-error diagnostics now underline complete Unicode characters, including astral characters after multiline edits. ([#18147](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18147))
- Keep the last import line inside the imports folding range so folding a use/no block no longer drops its final import. ([#18180](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18180))
- Snippet plain-text fallbacks for clients without snippet support now preserve literal Perl backslashes: the snippet plaintext renderer only treats backslash as an escape before $, } and backslash in ordinary text (comma, pipe and backslash inside choices), so bodies like the shipped printfh snippet insert "data\n" instead of corrupting it to "datan". Snippet-capable clients are unchanged. ([#18232](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18232))
- A statement-form package inside a nested block no longer leaks its scope frame past the block end: the HIR lowerer restores the enclosing scope stack and package context on block exit, so lexicals declared after such a block are recorded in their true enclosing scope and their reads resolve to them instead of being lost under the leaked package. ([#18257](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18257))
- The bare-regex POD guard no longer re-arms on the line that closes a multiline bare regex: a code line ending in a bare slash while the guard is armed is treated as the close of the possible body, so a genuine POD opener after the closed regex is honored again and POD declarations no longer leak into the known-sub table (which had flipped later same-name slashes from division onto the regex path). ([#18248](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18248))
- Diagnostics adopted from fused typeglob sub-parses now report outer-source byte offsets for DoWhileTrailingBlock, CStyleForContinueBlock, QualifiedLoopControlLabel, and HeredocBudgetExhausted (previously they kept inner-source coordinates and could point into unrelated text, including the middle of a comment); the offset translation match is now exhaustive so future located variants must choose a translation deliberately. ([#18243](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18243))
- Quote-like spellings used as bare subroutine prototype text (such as sub foo (q) { }) no longer enter quote mode at the closing paren: the prototype payload is preserved as an identifier, so the declaration, its body, and the existing invalid-prototype warning survive instead of being consumed by an unclosed quote error. ([#18254](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18254))
- Body-HIR binding resolution now treats a foreach lexical iterator as loop-private: uses inside the loop bind to the iterator, and uses after the loop bind to an outer variable of the same name again. Previously every use collapsed onto the loop iterator's binding. ([#18189](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18189))
- Variables declared with my at package top level now resolve as lexicals in body HIR instead of being misreported as package globals, so reads after a package statement bind to the declaration in that package. ([#18194](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18194))
- The three-dot operator between operands now parses as a range (Binary with the source spelling preserved) instead of leaking an Ellipsis term plus residue, while a standalone three-dot term remains the yada-yada operator. ([#18260](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18260))
- The legacy position_to_offset utility now maps CRLF row starts after the complete separator instead of onto the LF byte, and treats a bare CR as ordinary content under the LF-delimited source contract, so inline completion environment positions on CRLF documents land on the intended byte. ([#18252](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18252))
- Deduplicate Moose/Moo `has` attributes in document and workspace symbols: the synthesized same-name accessor at the same span no longer appears alongside the attribute, while explicit readers, explicit subs, and multi-attribute declarations are kept. ([#18192](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18192))
- Document symbols for framework packages (Moo, Moose, Moo::Role, Moose::Role, Role::Tiny, Class::Tiny) now select the declared package name instead of a substring inside the `package` keyword: when the compatibility extractor presents the package as Class or Role, the provider retries the canonical Package declaration record by exact name and span, preserving wire kinds and fail-closed anchor behavior. ([#18196](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18196))
- Coalesce duplicate reference spans for method calls into the name token and emit one canonical URI spelling per document. ([#18181](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18181))
- Document symbols now parent symbols by package extent: statement-form packages collect the symbols that follow them until the next package at the same level or the enclosing block ends, block-form packages and classes use their block extent, and Moose/Moo roles act as containers, so multi-package files outline each package with only its own children and no symbol is dropped or duplicated. ([#18184](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18184))
- Accepting a workspace-symbol, in-file function, or method completion with the cursor in the middle of a name no longer duplicates the rest of the name. Previously the completion replaced only the typed prefix, so completing `reg` inside `register` produced `registerister`; the edit now replaces the whole token. ([#18123](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18123))
- Body HIR now preserves lexical reads after multiply labelled package statements and retains statements inside block-form packages, including nested foreach loops. ([#18208](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18208))
- A dynamic indirect constructor receiver such as the $class in `new $class` was recorded with the same bareword role as a static class name in `new Foo`. It now records a distinct DynamicIndirectObject role, so consumers can tell a runtime constructor target from a static one without inspecting the name. ([#18279](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18279))
- A mutating substitution or transliteration target is now marked read-modify-write in body HIR, matching compound assignment, so write and data-flow queries can see that the target variable is reassigned. Non-destructive /r forms still mark the target as a plain read. ([#18240](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18240))
- Compound declaration initializers such as my $value x= 3 lower again to a simple declaration assignment wrapping the compound assignment, restoring HIR declaration reachability for repetition assignment. ([#18288](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18288))
- catch (my $e), catch (our $e), and catch (state $e) now produce a typed Can't-redeclare-catch-variable diagnostic on the keyword with exact source geometry, while the catch binding is retained as for the bare form. ([#18282](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18282))
- A workspace/configuration pull that flips useSystemInc, usePerl5lib, or perl5libPrecedence without changing include paths now invalidates workspace identity and refreshes diagnostics, instead of leaving published diagnostics describing the old resolution state. ([#18274](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18274))
- Classic sub prototypes now paint as parameters and sub attribute names as modifiers in semantic tokens, and the TextMate grammar scopes prototypes as variable.parameter.prototype and attribute names as entity.other.attribute-name so $$ inside a prototype no longer paints as the PID variable. ([#16589](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16589))
- Document links for absolute drive and UNC require paths now apply literal parent-directory components while preserving filename URI encoding and directory targets. ([#18324](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18324))
- Tied-variable hover no longer presents interpolated or command string expressions as exact class names. The optional class annotation is conservatively omitted for all interpolating quote modes, including constant double-quoted names. ([#18308](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18308))
- Document symbols now select the exact parser-owned name of native methods, including names that also occur inside the method keyword or body. ADJUST blocks retain their existing outline behavior. ([#18309](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18309))
- Keep calls inside nested subroutine bodies out of the enclosing caller's outgoing call hierarchy while preserving direct callsite ranges. ([#18321](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18321))
- Preserve heredoc messages and following statements after scalar output handles in print, printf, and say calls. ([#18330](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18330))
- Preserve semicolon-leading quoted strings after Unicode multiplication operands without broadening punctuation typeglob recognition. ([#18336](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18336))
- Document-symbol children no longer order nondeterministically across processes. Child symbols previously fell through a complete (priority, start, end, name) sort-key tie to the symbol table's HashMap bucket order, which permutes between runs; a final `kind` tiebreak now decides those ties, so repeated outline requests return a byte-identical tree. ([#18354](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18354))
- Treat a line-initial `=cut` outside POD as opening a POD block that runs to the next `=cut` or to EOF, matching perl. Such a file was previously reported as a syntax failure even though `perl -c` accepts it. ([#16607](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16607))
- Shrink Find References declaration spans from UTF-8 line bytes instead of LSP columns so a non-ASCII prefix cannot panic or shift the name token. ([#18529](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18529))
- source_identity.v1 length-prefix encoding now rejects fields longer than u32::MAX bytes fail-closed instead of silently truncating the prefix, with fallible try_* constructors and pinned regression tests. ([#18672](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18672))
- Find References now returns one identifier-only range per call or declaration instead of overlapping call-expression spans. ([#18863](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18863))
#### Removed
- Deleted the orphaned perl-parser IDE surface — src/ide.rs re-exported the perl-lsp-providers crate removed in Wave G1b, and the undeclared src/ide/lsp_compat provider tree duplicated live providers now owned by perl-lsp-rs-core; neither path was declared in lib.rs, compiled, or had any consumer. ([#12022](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12022))
- **Breaking:** perl-semantic-analyzer's SemanticQueryFacade is now strictly per-file: the cross-file `visible_imports` method and the `VisibleImport` struct are removed from its public API, along with the facade's `WorkspaceIndex` import (#7588). The method only forwarded to `WorkspaceIndex::file_dependencies` and sorted the result — it never read the facade's own model — so it added no semantic value while forcing the analyzer-owned workspace store into the signature of an otherwise per-file layer. It was retired rather than relocated because no consumer exists: no production or test caller anywhere in the workspace outside its own two unit tests, and all nine crates.io reverse dependencies are first-party workspace crates, none of which reference either item. Cross-file dependency queries remain available on `WorkspaceIndex::file_dependencies`, which is unchanged, so no capability is lost. A recurrence guard in the module now fails if `WorkspaceIndex`, `workspace_index`, `std::fs` or `Parser::new` reappears in the facade's production code, proven non-vacuous by mutation. ([#15633](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15633))
- Six perl.limits.* settings (astCacheMaxEntries, astCacheTtlSecs, symbolCacheMaxEntries, maxIndexedFiles, maxTotalSymbols, workspaceScanDeadlineMs) were parsed but never read by any runtime consumer. They are removed from LspLimits and the limits.* configuration-authority catalog together with their settings keys. The settings schema rejects them (additionalProperties: false), so clients still sending them now get an explicit unknown-property diagnostic instead of a silent no-op. No enforcement existed to lose, and all documented result caps, deadlines, file-size and memory-budget limits remain behavior-backed. ([#16176](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16176))
#### Security
- The regex embedded-code-execution security scanner no longer false-flags `(?{ ... })`-shaped text that appears only inside a `(?#...)` regex comment as a live code-execution risk. ([#4682](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4682))
- A project's .perl-lsp.toml can no longer set the AI-completion endpoint, API key environment variable, header, or prefix — those are now only settable via the trusted LSP client/server settings channel, closing a path where a cloned repository could redirect a named secret to an attacker-chosen destination. ([#4955](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4955))
- include_paths set via a workspace's own .perl-lsp.toml are now rejected (with a visible warning) when absolute or when they resolve outside the workspace, instead of being silently honored — closing a path-traversal vector where a hostile cloned repository could redirect module resolution to read files outside the workspace during hover/go-to-definition. ([#4991](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4991))
- The 'explain missing module' feature no longer reveals filesystem path-existence information for names outside the workspace, closing a path-existence oracle in module-lookup diagnostics. ([#5029](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5029))
- A project's .perl-lsp.toml can now only opt OUT of AI completion, not turn it on or choose its provider/model — those remain controlled solely by user/machine settings, closing a path where a cloned repository could silently activate AI completion. ([#5022](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5022))
- AI-completion requests now validate the destination host against policy before sending, no longer follow redirects, and bind credentials to the approved endpoint identity, closing an SSRF/credential-exfiltration path (including an IPv6/DNS TOCTOU bypass) that a malicious AI-endpoint configuration could previously exploit. ([#5025](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5025))
- Client-supplied includePaths are now split into machine-scoped external roots versus resource-scoped paths and validated server-side (absolute-path and control-character checks), and the VS Code extension now forwards only global externalIncludePaths, closing a trust-boundary gap that let workspace-level settings reach outside the intended scope. ([#5021](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5021))
- Legacy Perl::Critic/perltidy subprocess profile settings (and critic.engine=legacy) can no longer be armed via workspace editor settings or the LSP settings channel — only machine-scoped settings can enable them, closing a trust-boundary gap where a cloned repository's settings could re-enable a legacy code-execution path. ([#5023](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5023))
- **Breaking:** Remove generic client testRunner executable, argv, timeout, and enabled authority; legacy payloads are inert while server-owned test execution remains available. Migration: remove perl.testRunner.command, perl.testRunner.args, perl.testRunner.timeout, and perl.testRunner.enabled from client settings; existing server-owned test execution is unchanged. ([#11845](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/11845))
### Editor behavior
#### Added
- Added a fail-closed real-Vim activation and workspace-root receipt contract for the canonical vim-lsp integration path. ([#7799](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7799))
- Added a fail-closed real Vim plus vim-lsp interoperability journey for the canonical perllsp client path. ([#7810](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7810))
- Added a fail-closed actual Coc-on-Vim interoperability journey for the perllsp client path. ([#7819](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7819))
- Added a fail-closed actual Vim and Vimspector launch journey for the native perl-dap adapter. ([#7834](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7834))
- Add an exact-source Sublime Text LSP-perllsp helper package with pinned, verified managed binaries, semantic-token mappings, and deterministic package contracts. ([#7840](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7840))
- Add a receipt-producing real Sublime Text interoperability journey for the exact-source helper package and current-source perllsp candidate. ([#7872](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7872))
- Add a curated Sublime Text command palette that routes through the active LSP-perllsp session with checked command IDs and bounded result surfaces. ([#7887](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7887))
- Register perl-dap directly with Sublime Debugger using trusted binary authority, native stdio transport, and bound exact-source host/runtime receipts. ([#7922](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7922))
- Added a fail-closed Zed managed-download projection that binds each offered perllsp target to the canonical release topology and exact v0.17.0 public asset metadata while retaining extraction, execution, and actual-host cells as not proven. ([#7847](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7847))
- Added a checked Zed settings contract that keeps binary path, arguments, and environment separate from the canonical perl.* workspace configuration route while retaining precedence and live-update behavior as not proven pending actual-host evidence. ([#7847](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7847))
- Added an exact-base Zed core submission packet that preserves Perl Navigator as the enabled default and keeps the independent perl-lsp and perllsp alternatives dormant until selected, with compatibility and merge-order evidence left fail closed. ([#7934](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7934))
- Added a fail-closed actual-Zed receipt schema, exact-source not-run template, bounded host runbook, and false-green controls that bind the Zed, extension, perllsp process, platform, fixture, configuration, activation, journey, and shutdown subjects independently. ([#7958](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7958))
- Added a fail-closed Zed upstream submission fan-in with exact subject fields, changed-file authority, blocked PR material, and validation that prevents submission-ready status until settings, managed assets, dormant defaults, and actual-host evidence converge. ([#7950](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7950))
- Added a fail-closed packet for updating the existing official Zed Perl extension entry after upstream merge, binding the current registry subject, allowed two-path diff, branch-reachable submodule requirement, version equality, validation, and public-install evidence boundary. ([#7952](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7952))
- Added a public Zed registry receipt template and subject boundary that keeps official registry, merged extension, released defaults, exact perllsp asset, clean-profile, managed-download, PATH, and documentation-promotion evidence independently fail closed. ([#7958](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7958))
- Added a read-only executable receipt lane for Zed-managed public perllsp assets, including current-stable drift detection, independent byte hashing, safe archive extraction, exact host-runnable stdio lifecycle proof, and explicit cross-build non-execution. ([#8365](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8365))
- Add a deterministic one-authority source manifest and export receipt for transferring the reviewed LSP-perllsp package into its canonical public repository. ([#8025](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8025))
- Add an exact, fail-closed compatibility and currentness authority for the LSP-perllsp, Sublime LSP, host, and managed perllsp subject tuple. ([#8027](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8027))
- Add read-only Sublime integration status with bounded structural and compatibility reason tokens. ([#8250](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8250))
- Added a Zed settings behavior contract and fail-closed validator that bind canonical perl.* keys to exact host receipts, reversible project-versus-Zed precedence effects, process-setting isolation, and directly observed live-or-restart behavior. ([#8373](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8373))
- Added a Zed defaults compatibility matrix and derived publication-order validator that preserve three independent Perl provider IDs, require a quiet final combination, prove explicit selection and no fallback, and prevent unresolved ordering from entering submission packets. ([#8379](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8379))
- Added the official-registry perl-dap journey receipt authority for the Zed debug-adapter route: a fail-closed validator that binds the checked #9516 managed-download contract and committed aggregate asset receipt by recomputed digest (never re-deriving public asset selection), evaluates the DU01 merged-and-released registry acceptance predicate over the acceptance manifest, enforces the clean official-registry profile and managed-public-artifact route against PATH/prior-cache/override/perllsp substitution, requires the complete bounded real-Zed journey cells with the wrong-root same-basename discriminator and orphan/restart proofs, and rejects exact-source or asset receipts relabeled public; a committed blocked_external receipt records the exact absent entry gates. ([#12463](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12463))
- Added the executable public perl-dap asset receipt matrix for the Zed debug-adapter route: a product-specific managed-download projection over the shared perllsp release family, dual independent digest authorities (checked contract plus release SHA256SUMS and in-archive checksums), exact-member extraction with ambiguity rejection, a bounded DAP initialize/disconnect stdio lifecycle proof with protocol-only stdout and orphan checks, an offline managed-cache known-good preservation suite, and a committed matching-host receipt for windows-x86_64 against release v0.17.0. ([#12333](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12333))
- Added the Zed debugger support projection (#9489): a fail-closed generator that consumes the landed #9487 official-registry journey validator and its committed blocked_external receipt exactly as they are, projecting the earned debugger cells into a separate machine-readable registry (policy/zed-dap-support.toml) and generated documentation (docs/EDITORS/ZED_DAP_SUPPORT.md). The projection keeps the Zed LSP row and the debugger surface independent, preserves the static/exact-source/public-registry evidence stages, managed/PATH routes, platform rows, and unsupported cells as not-proven with their blockers, quotes every gate, blocker, and limitation from the receipt without re-deriving the #9516 asset matrix, and fails closed on gate staleness, a landed registry subject, a committed exact-source pass, lying pass shapes, adapter identity aliasing, and committed-output drift. ([#12703](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12703))
#### Changed
- Separate the ptkdb bootstrap helper from the experimental live-peer protocol and require real partner evidence before support claims. ([#7313](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7313))
- Converged the repository-owned Zed integration substrates onto one current-main review surface while keeping executable and public support evidence separate. ([#7847](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7847))
- Freeze stdio as the sole product editor transport for perl-dap, with a checked consumer inventory that classifies native and external-peer editor sockets as retire while keeping authenticated debugger-peer TCP separate. ([#12573](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12573))
- DAP responds promptly to unsupported cancellation requests while debugger inspection is waiting, bounds queued work, reports intake failures truthfully, allows retry after failed disconnect cleanup, and shuts down cleanly after output failure. ([#15614](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15614))
- Native stdio debugging can cancel a blocked evaluation and recover for later requests and replacement sessions; other debugger transports retain their existing cancellation limits. ([#15629](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15629))
#### Fixed
- Stop fabricating package/global variables when debugger inspection is unavailable and refuse to treat unrelated unframed output as an evaluate result. ([#10177](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/10177))
- Downgraded Zed from an unearned support claim to planned/not proven, removed instructions that repointed the independent `perl-lsp` Zed server ID to `perllsp`, and added a checked submission packet for a distinct `perllsp` extension route with exact product identity, managed-release targets, semantic-token defaults, fail-closed dispatch, and copy-ready upstream instructions. ([#7847](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7847))
- Corrected every documented Helix registration to the reviewed Perl 5 override so Raku/NQP/P6 files no longer launch the Perl 5 server, standardized the Helix-local server ID on perllsp, and added an executable drift check binding the setup guides to the canonical languages.toml fixture. ([#7818](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7818))
- Preserve distinct dependency declarations across metadata phases, constraints, and sources. ([#13681](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13681))
- VS Code inline completion now runs through one provider owner instead of two competing registrations, and forwards the editor's actual trigger kind, selected completion, and cancellation. Explicit invocations previously reached the server labelled as automatic, which the server refuses before any backend call, so streamed AI suggestions could never appear. ([#13796](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13796))
- **Breaking:** External-tool availability checks now share one policy and refuse tools found only through relative PATH entries or the server's current directory. Put installed tools in an absolute PATH directory outside the server's current directory. Unix process-launch behavior is unchanged. ([#14815](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14815))
- Require a separator after a single WSL drive letter so adjacent mount names such as /mnt/cx are not converted into Windows drive-relative paths. ([#13076](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13076))
- Publish DAP stopOnEntry only after the Perl debugger provides a real source frame and ordered prompt evidence, preserving event ordering and exact entry frame identity. ([#15220](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15220))
- Preserve main-file breakpoints set before launch until the Perl engine acknowledges them, then replay them with their original IDs and verified state. ([#15488](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15488))
- Unknown or late cancellation no longer poisons reused request IDs. ([#15491](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15491))
- **Breaking:** Diagnostics and code actions now share native Critic settings and service routing; the public pull-diagnostics context API now accepts per-URI context factories so multi-root results preserve configured profiles and result identity. ([#15418](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15418))
- Reject malformed document-change batches without replacing accepted buffer or parser state, while stopping obsolete editor completion streams. ([#15506](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15506))
- TCP debugger attach now rejects unsupported stopOnEntry requests before connecting and directs users to configure the debugger peer to pause. ([#15521](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15521))
- Refuse unsupported process ID debugger attach before inspecting the target or changing the active session, and remove PID attach suggestions from the editor. ([#14402](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14402))
- Signature-help requests with no available signature now receive an explicit null result instead of leaving the editor waiting for a response. ([#15539](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15539))
- Go-to-definition requests inside comments or on a qualified-name package prefix now receive an empty result instead of leaving the editor waiting for a response. ([#15542](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15542))
- Completion-item and code-action resolve requests now reject missing parameters and invalid required labels or titles with useful errors, while preserving valid lazy resolution and extension fields. ([#15543](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15543))
- Provider explanations no longer mark generic request traces fresh when dispatch has no freshness evidence. ([#15545](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15545))
- Explain Provider Decision now separates provider policy from attached request evidence and leads with request freshness or its absence. ([#15547](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15547))
- Native debugger evaluation preserves numeric-leading Perl expressions, including spaced arithmetic such as 6 * 7. ([#15631](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15631))
- Ignore stale debugger expression results requested for a stack frame after execution resumes or the session changes, and distinguish cancellation and session loss from timeouts. ([#15566](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15566))
- Discard stale locals results after execution resumes or the debug session changes. ([#15576](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15576))
- Reject stack-trace results from an earlier stop or debug session. ([#15584](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15584))
- Reject stale inline runtime values after the debugger exits or its session changes. ([#14453](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14453))
- DAP disconnect and terminate requests retire stale pending terminal producers before request dispatch, preventing a queue-handoff deadlock while preserving no-session behavior. ([#15738](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15738))
- Refuse textDocument/formatting with ContentModified after a text-bearing sync notification for a stored document is rejected by the serialized-size ceiling, instead of returning edits computed against the stale snapshot. A warning names the limit once per affected file. A rejected didSave without replacement text leaves the stored document current and formatting available. This closes the format-on-save corruption where the post-format didChange exceeded the ~2.1 MiB sync ceiling, was silently dropped, and the next save re-applied a whole-document edit over the client's already-formatted buffer, duplicating its tail until the file no longer parsed. ([#16709](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16709))
- Show a bounded warning when an oversized first didOpen is refused before the document is stored, so the editor user can tell that the buffer was not opened. ([#16782](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16782))
- Reject full-document change batches when an earlier replacement exceeds the line-length limit, preserving last-good text and refusing stale answers until recovery. ([#17560](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17560))
- Preserve a final bare carriage return in UTF-16 source positions so ranges ending at the end of a file include that character, while retaining CRLF handling. ([#17564](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17564))
- Reject malformed debugger launch arguments instead of silently dropping non-string entries or launching with an empty argument list. ([#17738](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17738))
- Preserve earlier workspace settings across valid partial client configuration updates, keeping module resolution and navigation consistent while applying new options. ([#17768](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17768))
- AST-unavailable completion now supplies exact name-token replacement edits, including mid-token completion, preserving variable sigils and method receivers when accepting existing fallback suggestions. ([#17779](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17779))
- Malformed legacy test commands now identify the missing or non-string first argument and show a corrected request instead of claiming request parameters are absent. ([#17934](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17934))
- In DAP external-peer mode, a positive debuggeeTimeoutSeconds is now refused with a clear error instead of being accepted but never enforced, since the peer owns the debuggee process and no watchdog can be armed there. ([#18070](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18070))
- The DAP external-peer launch path now refuses wrong-typed stopOnEntry, debuggeeTimeoutSeconds, and env values instead of silently coercing them, matching the main launch handler. ([#18050](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18050))
- Withdraw workspace-symbol capability advertisement when the workspace-symbol feature is disabled, including resolve options that previously kept the disabled parent advertised. ([#18325](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18325))
- Reject full-document change members that carry rangeLength without range, including explicit null, so Full sync keeps last-good text instead of installing invalid replacements. ([#18516](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18516))
#### Security
- **Breaking:** Native perl-dap no longer binds an editor-facing TCP listener. `perl-dap --socket` / `--port` without `--external-peer` fail before bind and name `perl-dap --stdio`. Authenticated debugger-peer TCP is unchanged. ([#12641](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12641))
- External-peer perl-dap modes now expose DAP only over child stdio. Editor `--socket`/`--port` fail before bind with a stdio migration that preserves `--external-peer` / `--external-peer-listen`. Authenticated debugger-peer TCP remains the only retained listener. ([#12723](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12723))
### Performance and reliability
#### Changed
- Documented that perl-ast Node destruction is iterative and that derived Clone, Debug, and PartialEq remain recursive with a parser-produced-tree precondition (#6900). ([#12425](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12425))
- Made perl-ast Node clone iterative and stack-safe on a 50,000-node 256 KiB worker; Debug and PartialEq remain derived-recursive (#8837). ([#12428](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12428))
- Made perl-ast Node structural equality iterative and stack-safe on a 50,000-node 256 KiB worker; Debug remains derived-recursive (#8839). ([#12489](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12489))
#### Fixed
- Formatting, code actions, semantic tokens (full), document symbol, and rename requests now respond promptly to $/cancelRequest instead of always running to completion, improving responsiveness on rapid edits. ([#4707](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4707))
- Configured workspace-index resource limits (max files, max symbols, max cache bytes) are now actually enforced during indexing instead of being silently bypassed, so a workspace that exceeds them degrades in a controlled, observable way rather than growing memory use unbounded. ([#4701](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4701))
- The document lock is now released before save reconciliation runs, instead of being held across it, removing a lock-contention/hang risk on save in larger workspaces. ([#5224](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5224))
- Removed `xtask/tests/agent_config_validation_tests.rs`, which validated the `.claude/agents/` directory retired in #5329. All 12 of its tests failed on main because the directory they assert about no longer exists, contributing to `cargo test --tests -p xtask` — and so the `unit_routed_full` gate behind PR Smoke — being red. ([#5529](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5529))
- The LSP process now closes outbound message admission and waits for already-accepted frames to settle before exit, while reporting writer failures and settlement timeouts separately from the protocol exit status. ([#15186](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15186))
- Configuration reloads now preserve metadata from open buffers while accepting new workspace settings, retain known facts when metadata sources are temporarily unreadable, and reconcile detected include roots without discarding explicit user roots (#15088). ([#15118](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15118))
- Preserve document generation and readiness when text synchronization rejects an overlong line, while still cancelling obsolete completion streams. ([#15533](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15533))
- Required LSP response delivery failures now close ingress instead of leaving requests waiting; normal EOF drains accepted responses, and cancelled queue admission clears pending state. ([#15615](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15615))
- Classify fork pull-request runs held for maintainer approval (`status: waiting`) as a separate `awaiting_approval` advisory with a `neutral` conclusion instead of `infra-no-proof`/`failure`. The reporter now names the human remedy rather than attributing a maintainer decision to the scheduler. ([#16403](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16403))
- After shutdown, a request that races exit is either delivered or refused without being written to the wire, and exit settlement reports a refused required response instead of claiming a fully delivered normal close. ([#16678](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16678))
- Complete malformed JSON frames now receive a ParseError with a null ID while following valid LSP frames remain processable. ([#17754](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17754))
#### Performance
- Document formatting now holds the documents lock for a narrower scope, reducing lock contention with other requests while a format is in progress. ([#4711](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4711))
- Completion no longer assembles the @INC module-resolution context twice per keystroke (once for module completions, once for workspace-symbol filtering); the two paths now share one memoized computation, reducing redundant workspace-folder locking and PERL5LIB parsing on every request. ([#4936](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4936))
- Completion no longer holds the client-capabilities lock across the full completion-item JSON serialization loop, freeing it for concurrent requests instead of blocking them for the duration of large result sets. ([#5194](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5194))
- The dispatched completion path now releases the client-capabilities lock before serializing completion items, matching the direct path's contention behavior instead of holding the lock across the full serialization loop. ([#5196](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5196))
- Repeated initialize-time and diagnostics-cycle environment probes (Perl interpreter discovery, perltidy/perlcritic tool presence) are memoized per process with environment-keyed invalidation, removing a repeated PATH walk from every diagnostics cycle. ([#13651](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13651))
- Reuse the immutable source digest across lexer checkpoints, removing repeated whole-file hashing during parsing and breakpoint validation while preserving source, configuration, and generation checks. ([#15457](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15457))
#### Security
- Windows DAP trusted-root checks use the full 128-bit directory ID and a no-follow RAII handle, rejecting unavailable identity and replacements with matching creation timestamps. ([#16986](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16986))
### Packaging and distribution
#### Added
- Add one machine-readable identity contract for perl-lsp, the perllsp server package and executable, the perl-lsp-rs implementation crate and compatibility binary, the VS Code extension, and perl-dap. Validate the contract alongside version synchronization so matching version strings cannot hide package, executable, extension, repository, or DAP identity drift. ([#6754](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/6754))
- Add a fail-closed publication-drift classifier exposed through the canonical cargo xtask command and compatibility binary. Bind the independently loaded manifest to exact swarm/public repositories, commits, tree digests, rules, and required invariants; reject authority paths that resolve outside the repository. Preserve the v0.17 Windows ARM64 managed-download mismatch as a same-version product-drift fixture and emit clean/drift/not-proven receipts. ([#6756](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/6756))
- Added a first-party Claude Code native-LSP plugin and root marketplace that launches the separately installed `perllsp --stdio`, binds to `${CLAUDE_PROJECT_DIR}`, and keeps Claude support claims bounded to directly proven activation and host behavior. ([#7462](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7462))
- Add an exact installed-binary Neovim first-mile receipt for public distribution channels. ([#7817](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7817))
- Stage a versioned Mason registry package candidate for perllsp. ([#7861](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7861))
- Added the managed release selection contract: canonical compatibility and exact target availability are resolved before semantic-version recency, stable/latest/tag policies stay explicit, and unresolved or incompatible release metadata is refused instead of silently substituting another server. ([#9978](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/9978))
- Added a strict typed binary source-selection authority that treats configured paths as hard choices, keeps packaged, managed, and external PATH roles distinct, and refuses unsupported, mismatched, stale, or unproven candidates instead of silently switching sources. ([#9992](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/9992))
#### Changed
- **Breaking:** Remove the historical Perl::LanguageServer DAP proxy, bridge mode, and published compatibility features; perl-dap now ships only the native runtime and explicit optional peer seam. ([#6991](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/6991))
- **Breaking:** Future releases publish only the perllsp runtime Docker image. Unsuffixed builder tags stop receiving new releases; existing registry tags are not rewritten. Use the documented runtime image references to run perllsp. The Rust toolchain Dockerfile remains a local CI scan input. ([#15089](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15089))
#### Fixed
- Delete the orphaned homebrew/perl-lsp.rb formula (v0.11.0, wrong shape — single binary, outdated version, not tracked by version_sync or any workflow). Reconcile CHANGELOG.md and RELEASE_HISTORY.md so version 0.16.0 is marked as skipped (changes rolled into 0.17.0) instead of "Unreleased". Correct docs/RELEASE_PROCESS.md crates.io rollback section to document cargo yank instead of the false claim that crates.io does not support yanking. ([#4717](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4717))
- Windows installer (install.ps1) now installs the binary as `perllsp.exe` to match POSIX, the docs, and the release archive naming; also corrected the POSIX installer's Windows-fallback URL from the non-existent `scripts/install.ps1` to the root `install.ps1`. ([#4788](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4788))
- The published perl-module and perl-diagnostics crates on crates.io now carry complete metadata (keywords, categories, readme, docs.rs links) and inherit workspace version/license/repository instead of hardcoding perl-module's own copies, which would have silently drifted on the next version bump. ([#5284](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5284))
- The documented `curl -fsSL .../install.sh | bash` bootstrap no longer aborts under `set -u` (`${BASH_SOURCE[0]}` is unbound when the script is read from stdin, and empty-array expansion is an error on macOS's bash 3.2). `install.ps1` now installs `perl-dap.exe` alongside `perllsp.exe` like every other channel. The Scoop, winget, Chocolatey, and Linux manifests now state the public-beta posture and the real `MIT OR Apache-2.0` dual license, the Chocolatey nuspec is well-formed XML again, the Linux package templates point at the release asset names that actually exist, and the unreferenced duplicate `distribution/homebrew/perllsp.rb` is gone. Two new CI self-tests (`scripts/tests/test-installer-zero-args.sh`, `scripts/tests/test-distribution-manifest-consistency.sh`) gate all of it. ([#5452](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5452))
- Align the LSP4IJ Perl LSP and DAP installer definitions with current release targets and add native Windows ARM64 selection. ([#7920](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7920))
- Managed release discovery for first-use, repair, and update routes now selects through one compatibility-first selector; the GitHub /releases/latest convenience endpoint and route-local picks are gone, a tag channel without versionTag fails closed, and releases without a host-target asset are refused before any download. ([#11613](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/11613))
- **Breaking:** Standalone POSIX and PowerShell installers promote a verified perllsp plus perl-dap product unit through an immutable candidate directory and an atomic current pointer. Interrupted or failed transitions leave the previous complete unit in place instead of a mixed pair. Migration impact: Windows installs now launch perllsp and perl-dap through PATH-visible .cmd shims and remove legacy install-directory .exe entry points, while POSIX publishes PATH names as links into .perl-lsp/current; consumers that invoke the installed executables by absolute path or copy installed files must move to the shim or link entry points. ([#12815](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12815))
- Historical Kwalitee release checks no longer accept detached passing evidence when no staged `dist` input is named (#13067). ([#13100](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13100))
- Retain META 1.x configure and build prerequisites in canonical workspace distribution facts. ([#13157](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13157))
- Keep conditional and feature-gated cpanfile declarations out of unconditional distribution facts, and stop a brace inside a quoted or quote-like literal from dropping every later declaration in the file. ([#13649](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13649))
- Release `toolchain_digest` now binds the immutable Cross container image digest for cross-compiled targets, so a changed build container can no longer produce an identical release toolchain identity. ([#14376](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14376))
- Make PATH selector creation commit-atomic across source-only and release product-unit transitions, and require the focused installer tests to distinguish absent selectors from dangling ones. ([#14052](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14052))
- Run the cargo toolchain guard before any network work in the `BUILD_FROM_SOURCE=1` path of `scripts/install.sh`. A user with a stale cargo and no internet now sees the typed `cargo-toolchain-guard: REFUSED` banner instead of the misleading `Check your internet connection` message. ([#15030](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15030))
- Point POSIX piped-bootstrap failures at a Linux/macOS manual archive procedure instead of the Windows zip section. ([#16753](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16753))
- Fail Windows installation verification on native nonzero exit or execution failure, with diagnostics before PATH guidance or completion. ([#17681](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17681))
#### Removed
- **Breaking:** Retired the duplicate `perl-lsp` executable from the `perl-lsp-rs` implementation crate; install and launch the canonical `perllsp` product command instead. ([#7497](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7497))
#### Security
- Publishing the VS Code extension now refuses to attach a VSIX when the release tag does not point at the commit the VSIX was built from, so a dispatch naming an older version can no longer overwrite that release's published asset. ([#9597](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/9597))
- The post-publish smoke test now verifies the version the publish workflow actually published, taken from a receipt written only after the crates are confirmed on the crates.io index, instead of parsing a version out of the triggering ref's name. A branch merely named like a release tag can no longer produce a smoke result that reads as proof a version was published. ([#9593](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/9593))
- The post-publish smoke test now runs its verification scripts from the commit the publish workflow actually published, taken from the publication receipt, and refuses to certify a publication it cannot bind to a commit. Automatic post-publish runs are bound end to end, since GitHub executes them from the default branch and the receipted subject is therefore the code that issues the verdict. A manual dispatch stays unbound, because it runs whichever copy of the workflow its selected ref carries, and the run log now labels it an operator re-check rather than publication proof. ([#9593](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/9593))
- **Breaking:** Standalone shell and PowerShell installers inspect a verified release archive against a versioned safety policy and extract only accepted topology members into a private staging root. Invalid, oversized, or ambiguous archives leave the install destination unchanged. ([#12742](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12742))
### Compatibility
#### Added
- New perl.workspace.discoveryExtensions and discoverySkippedDirs settings (via .perl-lsp.toml or LSP client config) let users extend which file extensions are indexed and which directory names are skipped during workspace discovery. ([#4715](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4715))
- Perl 5.32+ chained comparisons (e.g. `1 < $x < 10`) now parse with correct chained semantics instead of being left-associated into an incorrect nested comparison. ([#4702](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4702))
- Add one typed registry for external Perl tool roles, exact native replacement identities, selection policy, execution class, conformance evidence requirements, and trust class, covering Perl::LanguageServer, Perl::Tidy, Perl::Critic, App::perlimports, and Devel::ptkdb. ([#7343](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7343))
- Add a canonical Neovim config and actual-host activation/root receipt. ([#7805](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7805))
- Add an actual-Neovim virtual perldoc receipt with explicit upstream-dependency state. ([#7815](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7815))
- Add the first actual nvim-dap receipt for the native perl-dap preview path. ([#7822](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7822))
- Add a bounded actual-Neovim support-floor/current-stable compatibility matrix. ([#7843](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7843))
- Stage a parity-checked nvim-lspconfig submission packet for perllsp. ([#7849](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7849))
- Pin the exact released LSP4IJ 0.20.1 Perl LSP and DAP template subject as immutable upstream evidence for compatibility and drift checks. ([#9450](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/9450))
- The internal configuration observation model now represents finite ai.rate_limit_rps values with positive-rate validation and schema generation 3. Runtime observation callers remain separate work (#10386). ([#13562](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13562))
#### Changed
- The nextEdit.enabled setting is no longer public configuration: it never had an editor-visible provider, so it is removed from the settings schema, examples, and docs. Supplying the legacy nextEdit key (or a [next_edit] project section) is now ignored with one bounded deprecation reason and can never report ready or enabled; internal next-edit scaffolding remains default-off and receipt-only. ([#8311](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8311))
- **Breaking:** `perl-position-tracking` no longer substitutes a synthetic URI when a `WireLocation` carries an invalid one. `From<WireLocation> for lsp_types::Location` is replaced by a fallible `TryFrom` returning `WireLocationUriError`, which carries the rejected input; the `fallback_lsp_uri` helper that produced `file:///unknown` is removed. The conversion now also requires an absolute URI: a scheme-less relative reference such as `""`, `"foo.pl"`, or `"/abs/path.pl"` parses under the URI-reference grammar but is not an absolute URI, so it is rejected rather than emitted as a `Location`. Degraded URI projection remains the final LSP response adapter's decision (ADR-0037). Breaking for direct users of the `From` conversion: switch to `TryFrom` and decide explicitly. No governed public-API baseline changes. ([#14811](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14811))
- **Breaking:** Move the experimental tree-sitter-perl-rs semantic overlay behind the non-default semantic-overlay feature so default and queries-only consumers remain parser-only; callers using Tree::semantic_overlay must opt in. ([#7603](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7603))
- **Breaking:** Pragma snapshots expose distinct bounded Perl 5.42/5.44 declaration profiles and explicit enhanced_xx state. Tracker-produced version admission is now unknown for decimal, patch, developer, future and conditional declarations; require VERSION no longer changes lexical pragma state. The public major/minor type remains a lossy compatibility projection, not an exact version model. ([#13149](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13149))
#### Fixed
- LSP request handlers across completion, hover, definition, references, document/workspace symbol, folding, code actions, formatting, rename, inlay hints, semantic tokens, type hierarchy, diagnostics, signature help, document highlight, and notebook sync now honor perl.disabledFeatures, instead of continuing to run when a client had disabled them. ([#4708](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4708))
- .perl-lsp.toml is now discovered by walking parent directories from the workspace root, matching how .perlcriticrc is already found; opening a monorepo at a subdirectory no longer silently falls back to default formatting/inlay-hints/AI-completion settings. ([#4710](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4710))
- Settings placed under initializationOptions.perl.* (perl.workspace.*, perl.inlayHints.*, perl.limits.*, as documented) are now actually applied as a base config layer, instead of being silently dropped because only workspaceFolders/rootUri/rootPath/disabledFeatures were read from initialize params. ([#4722](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4722))
- In multi-root workspaces, per-folder .perl-lsp.toml settings ([diagnostics], [critic], [features], [formatting], [ai_completion]) no longer have the last-opened folder silently overwrite earlier folders' settings; the first folder to set a value now wins, and a warning is shown when folders disagree. ([#4734](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4734))
- The server now advertises perl.debugTests in its LSP capabilities, tolerates unknown $-prefixed client notifications instead of erroring, adds __CLASS__ (Perl 5.38) keyword support, and adds the previously-wired --ripr-facts and runtime-tuning flags to --help and all four shell completion scripts. ([#5280](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5280))
- Six numeric config fields (maximumLineLength, indentColumns, blockCommentIndentation, timeoutSecs, maxLength, resolutionTimeout) now accept float-form JSON values like 4.0, instead of silently falling back to their defaults when a config generator emitted a float. ([#5297](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5297))
- Negotiate Sublime Text file-operation and inline-completion capabilities from the client's declared LSP 2.13 surface instead of advertising unsupported operations. ([#7782](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7782))
- Add an importable LSP4IJ Perl template candidate that uses sparse server-native perl.* settings instead of copied VS Code perl-lsp.* configuration. ([#7915](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7915))
- **Breaking:** The loaded-module reload transaction is now the only component that advances the runtime-module generation clock: `reload_family::project_execution` reports the transaction's `GenerationAdvance` instead of applying the clock a second time, so one reload spends one generation and the published previous/current witness describes that reload. The projector accepts one transaction-owned `ReloadExecution`, keeping the terminal outcome and its witness inseparable even when admission later reuses a numeric operation ID after eviction. It refuses a transition with the wrong direction or non-contiguous endpoints. Rust API break in `perl_dap::reload`: `GenerationAdvance` changes from an enum to an opaque struct (its variants, their fields, and every public constructor are removed; `is_contiguous()` is added alongside the existing accessors, and a witness is now obtainable only from `RuntimeModuleGenerationClock::apply`), `ReloadExecution::{outcome, phase_reached, mutation_issued, mechanism, generation}` are private, `RuntimeModuleGenerationClock::apply` takes the transaction's operation identity, and `reload_family::project_execution` replaces the old `project_outcome(outcome, operation_id, &mut clock, reasons, remediation)` signature with a transaction-owned execution. The DAP wire shape, JSON schema, and generated TypeScript projection are unchanged, and the family remains unadvertised, so no shipped debugger behavior changes. ([#14635](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14635))
- Classify tree-sitter-perl-rs expression-recursion exhaustion as ParseFailure::RecursionLimit instead of the unclassified Other, without relabeling it as structural NestingTooDeep. ([#15593](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15593))
- **Breaking:** Parse scalar signature defaults with =, //=, and ||= while retaining exact operator ranges; reject named aggregate parameters and aggregate defaults with typed, ranged errors. Rust AST users must supply default_operator and default_operator_span when constructing OptionalParameter, and default_operator_span when constructing NamedParameter. ParseError gains InvalidSignatureParameter and its typed kind. This pre-1.0 source compatibility change does not promote editor support. ([#16204](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16204))
- Enforce positional, named, and trailing-slurpy signature ordering without dropping or reordering parameters. Diagnostics identify the complete offending parameter and preserve known ordering state across invalid parameter forms. The existing non-exhaustive ParseError gains the additive InvalidSignatureOrdering variant and InvalidSignatureOrderingKind; ordering diagnostic kinds/messages change while location consumers retain the start anchor. ([#16208](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16208))
- Core builds without the optional AI transport retain typed inline-completion budget errors, and xtask avoids compiling unused HTTP/TLS dependencies while product AI features remain enabled. ([#17797](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17797))
#### Removed
- **Breaking:** The absorbed filesystem-mutating `perl_parser::refactor::refactoring` module (`RefactoringEngine`, `RefactoringConfig`, `RefactoringOperation`, `RefactoringResult`, `RefactoringScope`, `RefactoringType`, `ModernizationPattern`, `BackupInfo`) is retired (#5231). It reread files from disk, created backups, applied multi-file work sequentially, and could report success while suppressing writes — none of which matches live editor authority. Live refactor behavior is unchanged and stays owned by its operation-specific providers: `textDocument/rename`, the refactor/refactor.extract/refactor.rewrite code actions (`perl-lsp-rs-core` code_actions::refactors), modernization, import optimization, and workspace refactor/rename modules. `PerlModernizer::modernize_file` drops its unused engine-typed `patterns` parameter. The `lsp.refactoring` feature row now names the live code-action owner instead of the engine; a future reusable pure planner is owned by #8281. ([#0000](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5231))
- **Breaking:** `perl-lsp-rs-core::protocol::JsonRpcError` no longer implements `perl_parser_core::ErrorClass`; downstream source consumers must classify originating failures in an application adapter. JSON-RPC fields and wire serialization are unchanged. ([#13071](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13071))
### Developer experience
#### Added
- Added a hard-assert regression lock for cross-file signature-help, closing the last vacuous-green gap in the scenario_21 provider dogfood suite. ([#3902](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3902))
- Add `cargo xtask workflows check` — an advisory actionlint + zizmor "Workflow Contracts" scaffold for `.github/workflows/*.yml` (local action/reusable-workflow ref integrity, explicit permissions, and action-pinning policy checks). Advisory-unarmed; cannot block a PR. ([#3885](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3885))
- Document the Swarm v3 lifecycle-state model (docs/source-of-truth/swarm-v2.md) and extend the PR review-convergence test suite with writer-set-staleness regression coverage (a reviewer who posts a fix disposition cannot verify their own fix). Internal control-plane tooling; no product behavior change. ([#3913](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3913))
- Editor intelligence scorecard now has a stable landing page (`docs/project/status/editor.md`), a rename gold-corpus fixture (`test_corpus/gold/rename_basic`), and a `test_rename_gold_corpus` regression test wired into `crates/perl-lsp-rs/tests/editor_intelligence_scorecard.rs`. ([#3445](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3445))
- `perllsp --doctor --json` now emits a machine-readable JSON report, so CI pipelines and scripts can consume doctor output without scraping text. ([#4928](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4928))
- Added a checked provider fact-read inventory that names current producers, proof assumptions, readiness inputs, fallback behavior, duplicate semantic interpretation, and migration owners before semantic-port cutover. ([#6843](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/6843))
- Publish the first behavior-backed generic LSP `perl.*` settings schema. ([#7811](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7811))
- Add a dedicated IntelliJ/LSP4IJ perl-dap guide that keeps template presence, managed installation, launch, attach, debugger feature cells, source identity, and cleanup evidence separate. ([#7954](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7954))
- Document the LSP4IJ integration maintenance lifecycle from canonical authorities through local delta preparation, manual upstream handoff, released-host re-receipting, and support promotion. ([#7957](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7957))
- Added checked test fixtures. ([#8016](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8016))
- Added an operator-assisted exact-source Zed host driver that isolates user data, binds exact Zed, extension, perllsp, workspace, settings, and prepared-run evidence, rejects stale or mutable artifacts, redacts retained logs, and requires complete schema plus semantic receipt validation. ([#8369](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8369))
- Add context-preserving perl-test-must helpers with non-redundant failure diagnostics. ([#10100](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/10100))
- Added a typed, repository-wide install and package surface inventory with deterministic discovery, explicit external-source separation, and diagnostic receipts. ([#9103](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/9103))
- Add the pure #11179 standalone install contract: an immutable candidate manifest whose domain-separated identity is recomputed over canonical bytes, a current-selection record with monotonic generations and governed previous-known-good semantics, a closed transition record keeping product, cleanup, startup, and PATH outcomes independent, three closed JSON schemas for runtime-neutral adapter consumption, and an independent verifier with 23 synthetic vectors and fail-closed falsifiers for mixed-pair, source-as-pair, preview-weakened, local-development, drift, regression, aliasing, wrong-root/target/attempt, and private-output state. ([#12213](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12213))
- Add the Emacs train E06 actor-packet adapter projecting stable train, checked spec, and exact-tree context joins into the shared agent implementation and review packet contracts with fail-closed typed refusals. ([#15573](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15573))
- Kubernetes DAP in-workspace debugger profiles are now governed by one machine-checkable contract that admits only project_image and injected_tool subjects and mechanically rejects every unsupported network, service, sidecar, operator, and Kubernetes-API topology with typed reasons before any cluster execution work begins. ([#12643](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12643))
- Report migrations that drop an .expect explanation into a bare perl-test-must helper, and re-export the context-preserving _with variants from perl-tdd-support. ([#14384](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14384))
- Record the reviewed htmx reference snapshot as typed provenance beside the catalog it describes (source URL pinned to immutable htmx commit bdc7d7d3e25d0390c7ee11049806e8279b075598 (reviewed version 2.0.10), contract, review date, and both spellings of the dynamic `hx-on*`/`hx-on:` family), and add `cargo xtask htmx-catalog-drift --reference <path>` to report a proposed diff between the committed catalog and a maintainer-supplied copy of the htmx reference document. Header direction is compared, not just header names, and extraction fails closed on a missing or unreadable section so a document the task cannot parse can never report "no drift". Nothing in the repository fetches the network: the maintainer supplies the reference document deliberately, outside CI. ([#14573](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14573))
- Add compiler transformation law, plan, and result contracts with structural legality, order-independent identity, and a closed terminal result vocabulary. ([#14650](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14650))
- `perl-workspace-core` gained `test_command`: deterministic, non-executing test-command planning (#13638). `plan_test_commands` turns one accepted `ProjectEnvironmentSnapshot` plus caller-supplied `GeneratedStateEvidence` into independent typed candidates for `prove -l`, `prove -b`, `make test`, and `Build test`, each carrying exact argv, working directory, environment-snapshot identity, trust, provenance, and the generated artifacts it requires. Missing, stale, and unobserved generated state stay distinct admission states and an unobserved artifact never reads as ready, so a runner is reported with a reason rather than silently lost or silently offered. Planning performs no filesystem access, no process execution, and no repair of stale build state. ([#14710](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14710))
- A versioned release_scope.v2 contract can represent the final v0.18 product freeze with distinct observation, frozen-product, and prepared-swarm identities, complete PR dispositions, typed evidence references, and explicit product-versus-preparation invalidation—without selecting or executing a live freeze. ([#13855](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13855))
- Add the checked perl-corpus stable authority train (perl_corpus_train.v1) with cargo xtask perl-corpus-train check, graph, and explain-static. ([#14945](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14945))
- Add perl-operation-trace, a leaf crate owning the versioned operation_trace.v1 correlation contract: ephemeral session-scoped operation identity that never uses the sha256: durable-identity prefix and depends on no hash function, an OperationContext threaded across call and background boundaries, closed operation/event/identity-reference vocabularies that fail closed on unknown input, a three-tier privacy model (public, private redacted with length, secret redacted with nothing), a field registry enforcing each field's declared tier, and a bounded in-memory recorder that rejects duplicate terminals, conflicting parents or kinds, and parent cycles, and marks truncation explicitly rather than dropping events silently. Nothing is instrumented yet: this defines the vocabulary the existing per-subsystem operation identities can later be correlated through. ([#15021](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15021))
- **Breaking:** Add `perllsp --doctor --dev-environment`, a bounded prerequisite report for Windows symlink privilege, Cargo identity versus the workspace pin, Bash flavor reachability, and native Perl identity divergence. The report carries copyable fixes and does not install or configure anything. The public `LaunchAction` enum gains this explicit command variant, so downstream exhaustive matches must account for the compatibility boundary (#12595). ([#12664](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12664))
- Added a workflow-contract test that pins the `--paginate`, `filter=all`, `per_page`, and `external_id` properties of the ripr-liveness dedup read, so the advisory `ripr+ liveness` reporter cannot regress to re-posting duplicate check runs past page one. ([#16133](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16133))
- `cargo xtask activation check/report/explain` evaluate the classified activation inventory with class-specific connection contracts. `unwired-scan` reports Cargo topology only and no longer treats dependency membership as product wiring. ([#16764](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16764))
- Admit a strict single-package Clippy workload on Linux with installed driver identity checks, private build roots and storage leases held until launched descendants settle. ([#17506](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17506))
- Admit one explicitly named test target through the strict Linux Clippy route while retaining pinned drivers, scoped storage budgets and descendant settlement. ([#17810](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17810))
- Admit optional --keep-going on the strict Linux Clippy route for all-targets and named-test selectors while preserving budgets, settlement and diagnostic authority. ([#17653](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17653))
#### Changed
- Replace the superseded Devel::TSPerlDAP bundled-shim architecture with the current native perl-dap design across the active architecture, book, and security surfaces, and add a recurrence guard that distinguishes reviewed archive references from active implementation guidance. ([#7310](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7310))
- Corrected Vim filetype guidance and Coc-on-Vim runtime prerequisites without overstating real-client support. ([#7821](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7821))
- Align Neovim setup and status docs with actual-client, distribution, and capability evidence. ([#7867](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7867))
- Split the gate-shard workflow-step extraction so a missing guarded runner step fails visibly instead of skipping and a terminal runner step extends to end-of-file, keeping the workflow-to-parser gate-policy contract check load-bearing when the step is last in ci.yml. ([#11950](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/11950))
- Routed every integration-harness server candidate through the bounded pre-build so cargo fingerprints decide freshness, making it impossible for a target-directory perllsp artifact left by an earlier checkout to be reused merely because it exists and silently test stale code. ([#12024](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12024))
- Aligned five stale call-site semantic-token expectations across the pending-parse freshness, legend-contract, and e2e suites with the ([#11933](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/11933))
- lsp_smoke CI gate now runs as nine atomic, bounded, independently terminal children with typed per-child receipts (compile/setup separated from behavior, per-child watchdog budgets, NOT_PROVEN prerequisite blocking) instead of one composite command whose timeout erased all child evidence. ([#8063](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/8063))
- Clarify that `perllsp --check` is a native listed-file parser check, `--check-project` is an 80% parsability report rather than an all-clean gate, and real-Perl compile observation is editor/DAP `perl -c`. ([#12813](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12813))
- Activate `clippy::map_err_ignore` deny for three Cohort C1 crates: `perl-position-tracking`, `perl-uri`, and `perl-semantic-analyzer`. Each production finding is covered by a `#[expect(clippy::map_err_ignore, reason = ...)]` — `LineRecord::new` impossible-rejection in `push_byte` (position-tracking), unit-typed `Url::from_file_path` error in `fs_path_to_uri` (uri), and lock-poison class in `index_file_str` (semantic-analyzer). 69 total target roots now carry the deny (8+8+53). ([#12848](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12848))
- Parser-accuracy scorecard metrics are now governed by a canonical registry that derives each row's direction and confidence and fails the artifact build closed on an unregistered, out-of-range, under-sampled, or over-confident row. ([#14554](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14554))
- Non-Rust file-policy globs now honour path segment boundaries — `*` and `?` stop at `/` and only `**` crosses a directory — so a single-segment allowlist entry such as the Changie fragment rule `.changes/unreleased/*.yaml` governs exactly the directory it names instead of silently adopting every nested file beneath it. Coverage of the current tree is unchanged; the one entry that relied on the previous behaviour now declares its breadth explicitly. ([#14588](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14588))
- Release status now distinguishes the shipped v0.17 public beta from the bounded v0.18 first-RC transaction, treats the discarded internal rc.1 preparation and closed projection leaves as historical, and preserves #13768 standing RC authorization without implying stable-channel approval. ([#14872](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14872))
- Freeze docs/policy/NON_RUST_INVENTORY.md as a pointer document that no command writes and main never changes, so it can no longer conflict on merge; the inventory is retained as target/policy evidence and the non-rust-inventory CI artifact, `cargo xtask non-rust inventory --write` is removed, and `--check` refuses a regenerated pointer with a one-command fix. ([#14688](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14688))
- Every reviewed RIPR consumer moves from 0.9.0 to 0.10.0 (#9113): the five routed execution lanes in ripr.yml, the badge-endpoints installer, the EXPECTED_RIPR_VERSION producer-receipt guard in scripts/generate-badges.py, and the documented install instructions. The 0.9.0/0.10.0 output-schema difference is dispositioned from captured producer output rather than inferred from version numbers: schema_version moves 0.1 to 0.2, findings[] gains assertion_texts and related_tests_total, and every field the repository consumer reads is unchanged, so no parser broadening was required. The version contract now also joins the badge consumer and the documented pins to the routed lanes, closing a false-green where bumping the workflows alone left generate-badges.py rejecting every produced receipt as unreviewed. ripr check output is additionally validated at ingest so a producer that drops or malforms a required summary count fails closed as an instrument failure instead of being read as zero gaps. ([#0000](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14996))
- **Breaking:** Worktree forensic JSON now uses worktree_forensic_evidence.v2 with typed observation states and omitted absent fields. Select saved-evidence decoders by schema_version or rerun inspection for v2; do not relabel v1 records. Human classifications and unavailable details are retained, but plan digests change with the serialized schema. ([#13515](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13515))
#### Fixed
- Relabel the invalid-Perl attribute-comma fixture in the unparenthesized-declaration ripr seam proof test so it no longer reads as if it exercised valid Perl syntax. ([#3900](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3900))
- `cargo xtask parser-corpus-sweep` now accepts `--profile <PROFILE>`, threading it into `SweepConfig.corpus_profile` for receipt naming instead of failing with an unexpected-argument error. ([#3929](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/3929))
- Stopped three xtask validators from emitting false-green output. `merge-ready emit` now derives its verdict from a live fan-in snapshot (`--snapshot`) and stamps an honest `not_proven` (never `valid`) when no evidence is evaluated; `verify` refuses to collapse a `not_proven` receipt to `valid`. `doc-claims` and `install-surface-check` OK messages now explicitly state they only check a fixed set of hardcoded literals and do not catch new staleness/drift. The `required-checks.toml` parser (merge-ready and queue-reconciler) now unions required `[[check]]` entries with `[[checks]]` and logs the schema-split note instead of silently dropping them. ([#4741](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/4741))
- perllsp --help now lists the PERL_LSP_TIMING and PERL_LSP_INCREMENTAL environment variables. ([#5291](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5291))
- Error messages for an unresolvable Perl interpreter no longer tell you to configure `perl.path`. That field has no user-facing writer — it is absent from the extension settings and from `.perl-lsp.toml`, and a workspace-supplied value is refused for security — so the advice was impossible to act on. The run/test/critic commands and `perl.debugFile` now give the same actionable remediation the startup detection already gave: install Perl, put it on PATH, reload the window. The fail-closed behaviour is unchanged. ([#5486](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5486))
- The GA release runbook no longer hardcodes three different versions and a push to a branch that does not exist. It carried `0.8.3` for the tag, `0.13.1` for the Homebrew formula, and `0.6.0` for the extension bump, so following the steps in order would have cut one release, published a formula for a second, and shipped an extension for a third — and `git push origin master` would have failed or landed the release commit where nothing releases from. Every step now reads one `$VERSION` set once at the top, and the templated coverage and performance headlines are replaced with pointers to the document that carries the receipts, so a release cannot republish figures nobody re-verified. `cargo xtask doc-claims` guards all of it, in both the runbook and its published book mirror. ([#5525](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5525))
- `perllsp --check-project` no longer reports valid Perl as unparsable. It now partitions recovered diagnostics into blocking and advisory the same way `--check` does, so a file whose only finding is an advisory (for example a nested-quantifier regex warning) counts as a clean parse instead of dragging down the parsable percentage and flipping the CI-facing assessment to FAIL. Advisories are still reported, in their own labelled section. The scan also reports paths it could not read — an unreadable directory or a symlink loop used to be dropped silently, leaving the report to print a confident percentage over whatever subset happened to be readable. ([#5472](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5472))
- Document the RIPR new-gap gate state where the seam count is positive but the gap list is empty, covering both routes to it — review guidance that never completed and guidance that completed and named nothing — with the JSON fields that distinguish them, the re-run recipe, and the prohibition on guessing at unnamed seams or clearing the gate with suppressions. ([#5523](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/5523))
- Pre-commit now dry-renders the exact staged Changie ledger for every configured project before accepting a commit. ([#7504](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7504))
- Make IntelliJ/LSP4IJ setup documentation distinguish template, binary-install, configuration, feature, file-family, and evidence stages instead of presenting broad JetBrains support from setup prose or server capabilities. ([#7951](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/7951))
- The lsp_smoke merge gate now builds the perllsp binary before running its tests. perllsp is its own workspace package, so `cargo test -p perl-lsp-rs` never built it; the test harness then fell through its binary-resolution chain to the `cargo run -p perllsp` fallback, which had to compile the whole server inside each test's 16s initialize timeout. Every semantic_definition test that spawns a server timed out twice and failed, which is what kept the gate red on main. With the binary prebuilt the same suite passes 16/16 in about a second. This is the same precondition already fixed for the routed integration gate. ([#9677](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/9677))
- Restored the main clippy_full advisory gate by adding the sibling-bin `clippy::print_stdout` allow to the install-surface-inventory xtask binary. ([#10533](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/10533))
- The workflow-contracts receipt now emits one self-describing schema token (workflow_contracts.v1), pinned by its wire schema and a drift-guard test, instead of a bare untyped "1" that could drift independently of the validator. ([#15768](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15768))
- Fixed apply_patch broken-pipe race in merge_integration — git apply exiting early on malformed input now surfaces the authoritative "git apply failed" error instead of an OS-level BrokenPipe. Completed the actions/checkout v7.0.1 pin sweep across all remaining checkout references. ([#11740](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/11740))
- AST s-expression rendering now carries declaration attributes: `my $x :shared = 1` no longer renders identically to `my $x = 1`, so snapshot fixtures, incremental-parse comparisons, and diagnostics can observe and assert attributes instead of silently dropping them. ([#11739](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/11739))
- Routed the forced pending-parse gap helper through `DocumentState::update_content` so the canonical `text_arc` text moves with every simulated edit; the helper previously left `text_arc` holding the pre-edit source, and hover answered from stale source text while the gap was open (#11933). ([#12094](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12094))
- Gate-receipt CI artifacts can no longer present cache-restored receipts from unrelated SHAs as fresh evidence. The merge-gate shard lane clears every uploaded receipt surface before running, the shard runner sweeps its receipt directory before executing gates, and a fail-closed freshness verifier re-checks each uploaded JSON binding (shard summary subject_sha, gate receipt metadata.git_sha) against the proven subject SHA before upload, JUnit conversion, and LEM actuals. ([#12085](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12085))
- Windows reparse-point tests now perform visible typed skips without symlink privilege instead of failing with os error 1314 (#12567). ([#12648](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12648))
- Draft pull requests now receive exact-head Rust formatting and conflict-marker feedback while the expensive merge suite stays deferred until ready for review; skipped jobs stay labeled as skipped in the check list and are framed as unverified by the draft-posture run summary. ([#13123](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13123))
- The native formatter now preserves CRLF throughout generated multiline block and wrapped-expression layouts, including unterminated final source lines, document output, and range edits, instead of mixing generated LF lines into CRLF source. ([#13191](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13191))
- Reject contradictory editor-UX operation timing evidence before it can affect compatibility scorecard metrics. ([#14076](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14076))
- The references provider-decision receipt now tells the truth about how current its answer is. Its `freshness` field is derived from the tier that answered and the workspace index state actually observed, instead of always reporting `fresh`; and an empty "no references" result now reports the index state it was really computed over instead of always reporting `none`. A complete index that finds no references is reported as a fresh, trustworthy negative, while an empty result over a stale, partial, or absent index reports `unknown`. Receipt/observability fields only; no change to references results. ([#14163](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14163))
- **Breaking:** A failure reported by an external debugger peer is no longer classified as an adapter bug. The external-peer path now returns a new `BackendError::PeerReported { command, message }` (classified `Advisory`) instead of `BackendError::Engine` (classified `Bug`), so an ordinary debuggee `die` reported over the peer protocol is not routed for adapter-bug repair. `BackendError::Engine` is retained unchanged for the in-process native backend, and the editor-visible message text is identical, so no DAP wire behavior changes. Adding a variant to the public `BackendError` enum can break downstream exhaustive matches. ([#14566](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14566))
- Made the parser-accuracy legacy-population trust contract fail closed symmetrically — one canonical lowercase digest authority, checked count arithmetic, a unique aggregate row, schema-forbidden fields refused on investigation rows, and validation gated on publication rather than only on --check. ([#14004](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14004))
- perl-parser's `incremental` feature is self-contained again: `incremental/diagnostics.rs` imported `lsp_types::Diagnostic` unconditionally while the optional `lsp-types` dependency is selected only by `lsp-compat`, so `cargo check -p perl-parser --no-default-features --features incremental` failed with E0432 and the profile could not be built at all. The legacy `ReparseResult::diagnostics` compatibility slot and the `LineIndex` import that serves the already-gated `from_lsp_change` are now gated behind `lsp-compat` alongside the type they need, rather than making `incremental` pull in the wire dependency. Behavior is unchanged for every configuration that previously built: the slot is written empty at all four construction sites, is never populated or read, and `ReparseResult` is `#[non_exhaustive]`. `just check-all-targets` now compiles the wire-free parser profiles, so a wire type reachable without `lsp-compat` fails the required compile_all_targets gate instead of merging unseen — default and `--all-features` both enable `lsp-compat` and could never catch it. ([#14978](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14978))
- The VS Code extension build now stops and reports failure when a toolchain, build, bundling, or packaging step fails. ([#15126](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15126))
- The multiline-constructor UX check now requires diagnostics for the edited document version and fails on missing evidence instead of accepting stale results or silence. ([#15549](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15549))
- Fixed the Windows platform smoke failure in `client_settings_replacement_releases_detector_ownership` (#15611): the test compared the normalized detector-ownership list against the raw forward-slash detector literal, which only matches where `normalize_include_path` re-joins components with `/`; the assertion now normalizes both sides, matching the sibling metadata-replacement test and keeping the ownership-release failure path discriminated on every platform. ([#15709](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15709))
- Tighten CP00-PROOF-LEVEL-CONTRADICTION word-boundary rules so that a proof-level term (`release`, `public`, `installed`, `packaged`, `presentation`) embedded only as part of a hyphenated or `_`-separated compound identifier (`source-release`, `source_release`, `source_public_release`, `installed_proof_requirement`) does not donate that term to a contradiction reading. This eliminates the false positive on #15504 where an Acceptance bullet naming `source-release` and a PR body discussing `any release in common` were read as contradictory even though no proof level was required.
 ([#15710](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15710))
- Clause-scope the CP00-PROOF-LEVEL-CONTRADICTION PR-side term check so that a Claim Boundary which *asserts* a public/installed/packaged/ released surface is covered does not arm the rule merely because a sibling sentence excludes an unrelated surface or because the section carries the template's exclusion vocabulary. The proof-level term and the explicit-exclusion marker now must share a sentence, list item, or coordinate clause. Resolves the false positives on #15621 and #15633.
 ([#15627](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15627))
- `perl-uri` `uri_to_fs_path_maps_empty_local_authority_to_root` no longer panics on Windows. The empty-authority `file://localhost` form cannot yield an absolute Windows path through the `url` crate (Windows requires a drive letter); the test keeps the authority-bearing inputs on all platforms (asserting resolution on Windows, where a rooted path without a drive prefix is not absolute) and supplements them with absolute drive-letter controls under `C:\` on Windows. ([#15722](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15722))
- `cargo xtask ripr-pr` no longer counts RIPR `no_static_path` / `reachable_unrevealed` / `weakly_exposed` probes whose expression text is absent near the reported head line as new gaps. `HeadLineExtents` keeps the head revision's lines for the change's file set, and `finding_is_outside_head` now reports a finding as outside the head revision when its probe expression does not anchor within a 3-line window of the reported line — closing the #6260 residual where deleted-line probes anchored inside the head file kept failing the required `ripr+ New Gap Gate` even though the guidance pass correctly named no seam for them. Head-side probes (their own line's text) and ambiguous cases (no expression, no head text, off-by-too-many lines) stay counted. ([#0000](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15993))
- Authority Transfer Review no longer reports a governed row as `FAIL_REVIEW_MISSING` when no reviewer packets were supplied at all; the workflow has no packet source configured yet, so the verdict now reads `NOT_PROVEN_GITHUB` (evidence could not be established) and the CLI exits 3 instead of 1. Packets supplied but none covering the row still emit `FAIL_REVIEW_MISSING`, preserving the typed-missing semantics for a real review-absence. ([#16100](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16100))
- Wired the `xtask release-live-controls` subcommand back into the CLI dispatch and added a regression test that runs the built binary. The `release_live_controls` module landed fully tested in #14689 but its clap variant and dispatch arm were lost in a 39-file conflict resolution, so the observer compiled and passed all 54 library tests yet was unreachable from the shell. `cargo xtask release-live-controls --json` now exits 3 (NOT_PROVEN) on a host without credentials and surfaces both repositories declared in `policy/product-identity.toml`.
 ([#16073](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16073))
- ci.yml's `check-all-targets` job now exercises the 12 `xtask/src/tasks/ci_subject_materializer.rs` unit tests and the 3 `xtask/src/tasks/file_policy.rs` preapproval unit tests on every PR that lands in the merge surface. Previously no workflow named either target — `xtask` is a bin target so the merge gate's `cargo test --workspace --lib` never compiled them — which let a future revert of `deterministic_git_command`'s `-c core.autocrlf=false -c merge.renormalize=false` git wrapper flags, a bump of `PREAPPROVED_SUBJECT_CONTRACT_VERSION` without retargeting the live workflow contract-version, or a relaxation of `validate_preapproved_subject_workflow`'s structural rules land without the merge surface ever failing. Same defect shape as #13484, which is the precedent for keeping the test gating co-located with the existing required `cargo test -p xtask --bin xtask` invocations. ([#16404](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16404))
- Keep default editor sessions free of INFO logs and indexing telemetry while preserving explicit logging and warning output. ([#16479](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16479))
- Recognise a `#[cfg(test)] mod` collapsed onto a single physical line as the test-scope boundary in the CI hygiene scanners. The boundary reader required a module head to end in `{` or `;`, so a compact test module matched neither the inline nor the declared spelling, the boundary resolved to no-boundary, and production checks scanned through test-only code and reported it as production findings. A new head predicate answers whether a line declares a module without disturbing the nesting-stack predicate, which still requires an opening brace at end of line. ([#16521](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16521))
- `perl-dap --external-peer` validates the `HOST:PORT` spec at startup with a format-naming error instead of surfacing the transport's raw "invalid socket address", and `--external-peer-listen HOST:<unparseable>` now fails startup instead of silently binding an ephemeral port that moves the rendezvous contract (#16556). ([#16675](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16675))
- The lane-history readers now honour the artifact's own version. `.ci/metrics/ci-lane-history.json` is written with a `schema_version` and the repository already gates the checked-in payload on it, but `learned_estimate.py` and `pr_plan.py` loaded the file and reached for `lanes`, `p50` and `static_floor` by name without checking it. A future version that renames one of those fields was therefore not refused, it was read with the wrong spellings: a renamed `static_floor` fell out of its guard and the planner priced a lane at a bare `p50 * 1.15`, dropping a 45-minute static floor to 4.6 while still reporting the estimate as learned. Each reader now pins the history version its own parser can read -- `learned_estimate.py`'s `SUPPORTED_SCHEMA_VERSION` and `pr_plan.py`'s `HISTORY_SCHEMA_VERSION`, both requiring an exact integer (JSON `true` and `1.0` are refused) -- and refuses a payload whose version it does not understand. On that refusal `learned_estimate.py` emits a null estimate with `learned: false`, and `pr_plan.py` raises `SystemExit` instead of producing a plan; neither silently falls back to static floors on an unfamiliar version. ([#16522](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16522))
- The one-shot binary identity surface is now discoverable and honest about a rejected mix. `perllsp --identity`, `perllsp --identity-json`, and the composed `perllsp --info --json` form now appear in `perllsp --help`, in the generated bash/zsh/fish/powershell completions, and in `man/perl-lsp.1`, each in that surface's own spelling. Asking for identity alongside other operands no longer falls through to the ordinary parser: `perllsp --identity --log` used to deny a real, supported flag as an unknown option, and `perllsp --info --json --log` used to answer a requested JSON packet with the human `--info` projection, dropping the request without a word. Both are now rejected with a message naming the flag, which is the behaviour the identity resolver's own documentation already claimed. The rejection covers the whole command line, so a valued option such as `perllsp --info --json --feature-profile prod` is refused rather than answered with human text, and extra operands after a bare `--` still count as extra. Operands after a bare `--` are still read as file paths, so checking a file named `--identity` is unaffected. ([#16527](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16527))
- `perllsp --doctor` no longer contradicts itself or leaks raw child stderr when an external tool's version probe fails. A line now names the step that failed and its exit code, and points at re-running with `--json` added for probe detail. The exit status is reported once instead of composing two status phrasings (`exited with status exit code: 29`). The JSON `error` field keeps its string shape and still carries the full stderr, with the same doubled status phrasing normalized inside it. ([#16529](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16529))
- The Sublime real-host exact-source lane now runs on digest-verified Sublime Text build 4200 on all three platforms. A controlled 4200/4213 comparison traced the newer host's zero-test failure to pinned typing_extensions 4.7.1: under Python 3.14, its ParamSpec initialization raises an AttributeError before LSP enables the window registry (#16388). The setup action still downloads the latest ST4 build before the workflow replaces that install; this pin fixes the build used for the test journey. ([#16682](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16682))
- The shared CLI's unknown-shell completion fallback now lists `pwsh` (`bash, zsh, fish, powershell, pwsh`), matching the `InvalidShell` diagnostic and `--help`; the fallback remains unreachable through `run_cli` because `parse_args` rejects unknown shells first (#16603). ([#16615](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16615))
- `scripts/ci/learned_estimate.py` stdout JSON now carries `schema_version` (`learned_estimate.v1`) on every emission path, including missing-history, decode-error, and non-object history, so a later consumer can refuse an unfamiliar shape. This token is the CLI producer field; it is not the history file's integer `schema_version`, and `pr_plan.py` still does not read this stdout. ([#16699](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16699))
- `perl-dap --ptkdb-bootstrap-rc` and `--debug-session-plan` now fail with a nonzero exit naming the unreadable program instead of silently emitting empty `source_facts` and exiting 0. ([#16702](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16702))
- Accept the installer wrapper's fixed-slot VERSION and INSTALL_DIR positionals on the canonical POSIX script, with environment values winning per slot. ([#16767](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16767))
- Stop `release-turnkey` at typed manual-merge handoffs instead of dispatching Release Orchestration on an unproven predecessor. ([#16851](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/16851))
- Every `cargo xtask` invocation replayed ~200KB (1030+) of the intentional Phase-1 `missing_docs` baseline warnings to stderr before the command ran: stable cargo re-emits the cached diagnostics of every fresh workspace-local unit and offers no suppression knob (`-q` does not gate `replay_output_cache`; `build.warnings` needs nightly `-Zwarnings`), and xtask cannot discipline its parent cargo's pre-main output. `scripts/xtask-quiet.sh` is the supported quiet dispatch for agent queries: it builds `xtask` with stderr captured, surfaces build failures verbatim with cargo's exit status, and on success execs the freshly built binary directly so the command's own stdout/stderr arrive untouched (measured 200052B → 0B on `ci doctor --help` and `ci explain`; fresh rustc errors still surface, exit 101 propagated). The `missing_docs` burn-down itself remains the separate follow-up programme deferred by #4911. ([#17267](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17267))
- The perllsp binary now self-identifies consistently across every CLI surface. `--info`, the `--doctor` report, and the `--doctor --dev-environment`, `--doctor --external-tools`, and `--doctor --critic-compatibility` headers used to hard-code the crate name `perl-lsp` while `--version`, `--help`, and the startup banner followed the name the binary was invoked under, so the same `perllsp.exe` answered `perllsp 0.17.0` to `--version` and `perl-lsp 0.17.0` to `--info`. All identity lines now take the invocation name from the single source run_cli already uses for `--version` and `--help`. Output shapes are unchanged: the `--info` block keeps its line layout, doctor headers keep their `=` underline spanning the header, and every body line, JSON surface, and the `perl_lsp.binary_identity.v1` packet are byte-identical. The LSP `initialize` `serverInfo.name` remains the fixed protocol identity `perl-lsp`. ([#17185](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17185))
- Clarify reserved MCP feedback and show Claude Code LSP plugin setup and read-only diagnosis commands. ([#17489](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17489))
- Keep admitted Linux Cargo storage leases until launched descendants settle; refuse execution when required process ownership capabilities are unavailable. ([#17507](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17507))
- Keep just doctor from silently repairing Git configuration; report core.bare corruption or inspection failure with a nonzero status and leave repair to an explicit operator action. ([#17556](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17556))
- Cargo Deny policy gate is green again: RUSTSEC-2024-0449 is dispositioned with a bounded-exposure reason for the unpublished xtask tool, stale bans skip entries assert versions actually in the dependency graph, and deny.toml changes now run the security lane before merge. ([#17718](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17718))
- Completion no longer answers from a stale parse paired with newer text while an edit is still being parsed. It returns its bounded current-text fallback marked incomplete instead, so instance methods no longer intermittently disappear behind a confident generic-only list after import edits. ([#17834](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17834))
- Nightly fuzz crash artifacts now upload when a job is cancelled by its timeout as well as when a step fails, so a crash found just before the timeout no longer loses its reproducer with the job. ([#17821](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17821))
- Project configuration errors now distinguish malformed TOML syntax from invalid setting value types, preserving the filename, location, and expected-type guidance in doctor errors and editor warnings (#16602). ([#17790](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17790))
- Report truncated benchmark comparison inputs by path without a backtrace, while preserving complete-result verdicts and comparison exit statuses. ([#18326](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18326))
- Recognize audited pure path-detector fixture spans and the executable benchmark comparison script in staged commit checks while retaining machine-path and unexpected executable rejection. ([#18311](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18311))
- Bind editor-UX scorecard JSON, status Markdown, and the optional embedded receipt to one publication generation so a mixed set is refused and a failed dest write rolls back to the previous generation. ([#18665](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18665))
- Windows badge RIPR cleanup now treats active accounting polling, each bound member wait, and terminal lifetime sampling as independent best-effort observations so one query failure cannot skip the remaining settlement checks. ([#18651](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18651))
- Match intent-diff-gate test paths and expected target paths by whole path components instead of substring heuristics (#15384). ([#15384](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/15384))
- Route `scripts/xtask-quiet.sh` through `scripts/cargo-admitted` so the quiet xtask helper no longer performs a raw, unadmitted `cargo build`, and locate the binary from the admitted resource descriptor instead of unadmitted cargo metadata. ([#18778](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18778))
#### Security
- The xtask Vim host toolchain's archive extraction no longer admits an entry name that resolves outside the extraction destination on Windows. The path guard used Path::is_absolute, which is false for a rooted-but-prefixless name such as /etc/x, while Path::join still resolves that name against the drive root; the admission predicate is now Path::has_root and the resolved target is asserted to stay inside the destination. The symlink rejection also now runs before the directory branch, so a symlink-mode entry whose name ends in a separator can no longer be admitted as a directory and skip the rejection. The file is additionally admitted to the Windows CI scope, because the proof for that guard only discriminates on Windows. ([#18278](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/18278))
- cargo-deny bans now fail closed if serde_yaml_ng or unsafe-libyaml appear outside the unpublished xtask path, complementing the RUSTSEC-2024-0449 advisory ignore until #17720 removes serde_yaml_ng. ([#17722](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/17722))

## [0.17.0] - 2026-06-28

### Added

- **DAP debug sessions use the same Perl interpreter as LSP analysis.** When a
  `launch.json` configuration omits an explicit `perl`, the debugger now
  resolves the interpreter through the shared toolchain profile
  (perlbrew → plenv → `PATH`) instead of ignoring the active toolchain and
  defaulting to a bare `"perl"`. An explicit `launch.json` `perl` is still
  honored verbatim, and resolution falls back to `"perl"` when nothing is found
  so the existing "perl not on PATH" diagnostic is preserved. Completes the
  unified executable-profile work that also centralized interpreter resolution
  and cached version probing across LSP and DAP. (#1929)
- **Automatic `.perltidyrc` discovery at the workspace root.** When no
  `perltidy_profile` is explicitly configured, the server now discovers a
  project-local `.perltidyrc` once during `initialize` — searching the
  workspace root, then perltidy's documented `PERLTIDY` environment override,
  then `$HOME/.perltidyrc` — and uses it when building the formatter config.
  Explicit configuration always takes precedence. (#1899, issue #1777)
- **Default native formatter honors the discovered `.perltidyrc`.** The
  supported scalar options in a discovered profile (line width, indent, tabs,
  brace/else placement, keyword spacing, trailing commas) are parsed at
  `initialize` and applied to the server config as a layer between the built-in
  defaults and user configuration, so project formatting applies in the default
  native engine — not just `external-legacy` mode. Precedence: built-in
  defaults < discovered profile < `.perl-lsp.toml` / `didChangeConfiguration`,
  so an explicitly configured field still wins. (#2016, #2025, issue #1953)
- **First-run doctor report.** `perllsp --doctor [dir]` now prints a read-only
  workspace setup report covering project config, Perl interpreter probing,
  configured include roots, `PERL5LIB`, system `@INC`, rejected roots, and the
  effective include-root categories the server will use. Failed Perl version
  probes preserve stderr guidance for actionable setup fixes. (#1571, issue #1818)
- **Shared Perl toolchain profile.** LSP, DAP, and first-run diagnostics now
  resolve Perl interpreter identity through a common `PerlToolchainProfile`,
  with cached version probes for fingerprinted binaries and deterministic
  handling for bare `PATH` commands. (#1951, #1978, issue #1929)
- **Workspace method signature help for `->method()` calls.** Triggering
  signature help (or hovering) on an OO method call now resolves the signature
  from the workspace symbol index for methods defined in the same project,
  rather than returning nothing. (#1301)
- **`our` variables in document and workspace symbols.** Package-scoped
  variables declared with `our` are now included in the symbol index and
  therefore visible in editor outline and go-to-symbol lookups. (#1300)
- **Moo/Moose `has` attributes in document and workspace symbols.** Object
  attributes declared with `has` (Moo, Moose, Moo::Role, Moose::Role) are now
  indexed and appear in outline and symbol search. (#1300)
- **Phase-block hover docs (`BEGIN`/`END`/`INIT`/`CHECK`/`UNITCHECK`).** Hovering
  over a Perl phase block now displays an explanation of when that block runs
  relative to compile and runtime. (#1298)
- **Framework-aware deterministic inline completions.** Try::Tiny `try`/`catch`
  scaffolds, Mojolicious::Lite and Dancer route scaffolds, and project-indexed
  package receiver method completions are now offered only when the workspace
  evidence supports them. (#1532, #1573, #1585, #1949, issue #1648)
- **DAP logpoint interpolation substrate.** Breakpoint hit registration can now
  interpolate supplied scalar variables in logpoint message templates while
  preserving existing raw-message behavior when no variable map is available.
  (#1807)
- **VS Code first-run onboarding helpers.** The extension can suggest discovered
  include paths from common Perl module directories, exposes an optional
  server-gated AI completion walkthrough/prompt, and ships an openable demo
  project for new installations. (#1898)

### Fixed

#### Debugger (DAP)

- **Stale stack frames cleared on resume.** When the debugger resumes and hits
  the next stop, the call stack and variables views now reflect the new stop
  position instead of the previous one. (#1337)
- **Degraded-transport `stackTrace` returns empty, not stale.** When the debug
  transport is in a degraded state, `stackTrace` returns an empty frame list
  rather than serving a stale snapshot from an earlier stop. (#1337)
- **Structured/container evaluate results expand in the variables view.**
  Evaluating an expression that returns a hash, array, or blessed reference now
  allocates a proper `variablesReference` so the editor can expand the result
  in the watch and variables panel. (#1219)
- **Invalid `variablesReference` returns a safe empty response.** Requests with
  an out-of-range or stale variables reference now return a protocol-safe empty
  response instead of crashing or returning garbage data. (#1227)
- **Execution-control requests with no active session return clear guidance.**
  Sending `continue`, `next`, `stepIn`, or similar when no debug session is
  active now returns an actionable error message instead of silently reporting
  success. (#1240)
- **`pause` accurately distinguishes signal-delivery failure from no-session.**
  `pause` now reports whether the failure was "no active session" or "session
  exists but signal delivery failed", giving editors and users an accurate
  explanation. (#1364)
- **`variablesReference` spaces are separated by type.** Scope, stack, and
  evaluate-result references now use a typed codec, retiring the collision class
  where one reference kind could be decoded as another. (#1430, #1444)
- **`evaluate` validates the expression before frame lookup.** Empty or unsafe
  expressions now report the expression problem even when the frame id is bad,
  avoiding misleading no-session errors. (#1496)
- **Malformed debugger stack contexts reject blank file names.** Stack parsing
  no longer accepts whitespace-only file fields as a real frame location.
  (#1498)
- **DAP request ordering fails with explicit protocol errors.** `launch` now
  requires a prior `initialize`, and `configurationDone` now requires an
  active launch or attach session instead of accepting out-of-sequence clients.
  (#1806)
- **DAP scopes expose pagination hint fields.** Scope responses now carry the
  optional `namedVariables` and `indexedVariables` fields from the DAP
  specification, preserving compatibility when counts are unavailable. (#1810)
- **DAP variables responses expose `totalVariables` when known.** Debug clients
  can now show accurate variables pagination counts without changing existing
  responses where a count is unavailable. (#1811)
- **DAP capability flags match implemented handlers.** The initialize response
  now advertises restart frame, step-in targets, and terminate-threads support
  when those routed handlers exist, so clients can discover the implemented
  operations. (#1759)
- **DAP transport handles non-request messages explicitly.** Client-originated
  Response and Event messages are now accepted and logged without producing
  spurious stdout or disrupting normal request handling. (#1790, issue #1608)
- **DAP event writes fail closed after persistent transport failure.** The event
  handler now detects repeated write/flush failures, marks the transport broken,
  and lets the main loop shut down cleanly instead of silently losing events or
  hanging on a broken socket. (#1809, issue #1609)

#### Editor settings

- **`enableSemanticTokens` and `enableFormatting` settings now take effect.**
  These settings were previously wired up but had no runtime effect; the
  underlying providers now check and honor them. Two no-op settings
  (`enableDiagnostics`, `enableRefactoring`) that appeared in configuration UIs
  but never did anything have been removed. (#1290)

#### LSP integration

- **Bare absolute file paths accepted as `file://` URIs.** Editors or scripts
  that send an absolute path (e.g. `/home/user/foo.pl`) instead of a proper
  `file:///home/user/foo.pl` URI no longer get a silent failure; the server now
  accepts both forms. (#1206)
- **Actionable error on malformed `signatureHelp` requests.** A request with a
  wrong shape now returns a descriptive error message rather than a generic
  protocol error. (#1206)
- **"Document not open" semantic-token errors explain the `didOpen` sequencing.**
  Editors that request semantic tokens before sending `textDocument/didOpen` now
  receive a message explaining the required sequencing, rather than a bare error
  code. (#1206)
- **Baseline single-root LSP smoke blockers repaired.** The runtime and tests
  now preserve hover state, wait for workspace file-operation indexing, handle
  empty workspace-folder inputs as no-ops, and keep progress harness output
  deterministic. (#1551)
- **Multi-root `workspace/symbol` is deterministic.** Workspace-symbol queries
  now wait briefly for active indexing, preserve each symbol's workspace-folder
  URI, and return repeatable results across roots. (#1522)
- **Workspace indexing counters stay truthful after duplicate parse-complete
  signals.** Pending parse metrics now saturate at zero instead of wrapping to
  `usize::MAX`, so first-open indexing/degraded status cannot report a
  permanent parse storm from out-of-order lifecycle notifications. (#2606,
  issue #2553)
- **Reference fallback avoids document-lock re-entry.** Partial-index reference
  fallback no longer re-enters the documents lock while searching open files.
  (#1597)
- **PL701 missing-module suggestions point at setup guidance.** Missing-module
  diagnostics now append `perllsp --doctor <workspace>` and the PL701 docs URL
  to legacy and context-aware suggestion text while preserving branch-specific
  `includePaths`, `useSystemInc`, `resolutionTimeout`, and `cpanm` hints.
  (#2047, issue #2049)
- **Perl documentation links share one validated target resolver.** Hover,
  document-link, resolve, and virtual perldoc surfaces now build MetaCPAN,
  `perldoc://`, and perldoc.perl.org targets through the same resolver, and
  malformed module payloads are rejected instead of turned into bad URLs.
  (#1638)
- **POD `L<>` references are clickable document links.** `textDocument/documentLink`
  now exposes module/core-pragma and same-document POD section references from
  real POD blocks, and `documentLink/resolve` validates same-document section
  fragments before returning `#section` targets. (#1795)
- **Non-standard POD sections are indexed for documentation surfaces.** Common
  `=head1` sections such as `ARGUMENTS`, `RETURN VALUES`, `EXAMPLES`, and
  `SEE ALSO` are now extracted instead of being dropped from POD-derived
  documentation. (#1834, issue #1610)
- **POD hover refreshes after external module edits.** Hover documentation
  cached from a resolved module file is refreshed when that file's mtime
  changes outside the LSP document lifecycle, so hover no longer serves stale
  POD after on-disk edits. (#1882)
- **Hover documentation escapes markdown metacharacters.** Documentation text
  containing characters such as `*`, `_`, `#`, and `[]` now renders literally
  in hover cards instead of becoming unintended markdown formatting. (#1840)
- **Context-specific completions keep semantic groups together.** Hash-key,
  Moo/Moose type and option, and Object::Pad constructor-parameter completions
  now use separate sort tiers so clients do not interleave unrelated suggestions
  alphabetically. (#1875)
- **Completion items send `filterText` to clients.** Completion responses now
  serialize the internally-computed `filterText` field, preserving expected
  client-side matching for snippets and other items whose label differs from
  the typed prefix. (#1889)
- **Completion capabilities advertise insert text modes.** Initialize responses
  now advertise `completionProvider.completionItem.insertTextModes: [1, 2]`
  for LSP 3.17 clients when completion is enabled, matching the server's
  PlainText and Snippet insertion support. (#1838, issue #1712)
- **Package-qualified method completions include inherited methods.** Completion
  for package receivers now considers inherited methods in the workspace model
  instead of limiting suggestions to methods declared directly on the receiver.
  (#1841)
- **Completions stay quiet in strings and non-code regions.** General variable,
  function, and method completions are suppressed inside ordinary strings, regex
  patterns, heredoc bodies, and POD, while path completions and intentional
  quoted module/import contexts remain available. Heredoc left-shift detection
  now keeps arrow-method and constant-shift Perl contexts from being mistaken
  for heredoc bodies. (#1808, #1813, #1821, #2573)
- **Multiline inline completions are parse-checked against the full document.**
  Inline completion candidates whose replacement ranges span lines now run
  full-document parse probes and fail closed when a range cannot be
  reconstructed, preventing syntactically damaging ghost text from being shown.
  (#1926)
- **Duplicate quick-fix code actions are collapsed.** When overlapping
  providers produce byte-identical lightbulb entries, the server now keeps one
  action and builds `source.fixAll` from the deduplicated set. (#1913)

#### Formatting

- **Range formatting works when complex syntax exists elsewhere in the file.**
  Formatting a selected range no longer fails when the rest of the file contains
  regex literals, heredocs, `qw(...)`, or POD blocks outside the selection.
  (#1314)
- **Heredoc and multiline folding boundaries are corrected.** Folding ranges for
  heredocs and multiline constructs now align with the intended source spans.
  (#1560)

#### Rename and refactor

- **Rename correctly updates dereference and string-interpolation occurrences.**
  A workspace rename now covers `$$var`, `@{$var}`, and interpolated `"…$var…"`
  occurrences in addition to bare identifier uses. (#1304)
- **Rename uses character-aware word boundaries.** The boundary check now
  handles multi-byte UTF-8 characters correctly, preventing partial-match
  renames that would corrupt identifiers containing non-ASCII characters. (#1288)
- **Package-scoped rename refuses unsafe fallback edits.** Package renames now
  prefer exact qualified-call edits and empty unsafe fallback plans instead of
  silently applying same-file edits when workspace or index facts are incomplete.
  (#2070, issue #1511)

#### Diagnostics and code actions

- **Code actions no longer panic on mid-codepoint UTF-8 ranges.** Invalid byte
  ranges inside multibyte characters now produce no action instead of slicing
  through a character boundary; valid character-boundary ranges still work.
  (#1481)
- **Arrow-deref hash keys are no longer flagged as strict barewords.**
  `$self->{name}` and `$ref->{key}` are recognized as Perl's auto-quoted hash
  key form while real strict-bareword violations still report. (#1562)
- **Missing `use strict` / `use warnings` diagnostics use Warning severity.**
  PL100 and PL101 now match the diagnostic catalog, so first-open pragma
  guidance is visible at the intended warning level. (#2061, issue #1766)
- **`source.fixAll` deduplicates strict/warnings pragma inserts.** Fix-all now
  keeps one semantic strict insert and one warnings insert, preferring the
  source-aware insertion point instead of producing duplicate pragmas from
  overlapping providers. (#2058, issue #2056)
- **Printf dynamic width and precision specifiers stay quiet.** The format
  checker no longer reports false positives for valid `%*` and `%.*` printf
  forms. (#1868, issue #1637)
- **DBI receiver completions are import-gated.** DBI-style receiver completions
  now stay quiet unless a visible `use DBI` fact supports them. (#1579)
- **Quoted hash keys with special characters appear in completion.** Hash-key
  completion now includes fully quoted fat-comma keys such as `'db-host'`,
  `'api.key'`, and `'api key'` while keeping unquoted keys conservative. (#1839)

#### Parser recovery and legacy syntax

- **Nested variable lists parse comma-separated items.** Declarations such as
  `my ($a, ($b, $c))` now recover the nested list instead of stopping at the
  inner comma. (#1457)
- **Negative keyword barewords before `=>` are treated as strings.** The parser
  no longer reports false errors for valid fat-comma hash keys such as
  `-strict => 1`. (#1460, #1483)
- **Custom sub attributes and method-call-looking string content stay quiet.**
  Common legacy syntax no longer produces false parser errors for custom
  attributes or interpolated strings containing method-call shapes. (#1461,
  #1463)
- **`s///e` substitution replacement text is classified as Perl code.** This
  improves downstream semantic analysis for executable substitution bodies.
  (#1238)
- **`given` blocks accept normal Perl statements.** The parser now handles
  postfix `when`/`default` modifiers and ordinary statements inside `given`
  blocks while preserving the classic `when { ... }` / `default { ... }` forms.
  (#1893)
- **Lexical sub declarations retain their declarator.** `my sub`, `our sub`,
  and `state sub` nodes now carry the declarator so downstream semantic
  analysis can distinguish lexical subroutines from package-scoped `sub`
  declarations. (#1845, issue #1729)

#### Module resolution

- **`qw(...)` import lists with whitespace before the delimiter are parsed.**
  `use Foo qw( Bar Baz )` (with a space before the opening delimiter) now
  correctly extracts `Bar` and `Baz` from the import list for symbol resolution
  and dependency indexing. (#1205, #1203, #1292)

---

### Under the hood (not user-facing)

- **TextMate grammar visual regression tests.** The VS Code extension's static
  syntax highlighting (`syntaxes/perl.tmLanguage.json`) is now locked down by
  scope snapshots under `vscode-extension/test/grammar/`, run via
  `npm run test:grammar` and enforced in the Extension Jest CI job. Any
  unintended change to highlighting surfaces as an explicit per-token diff.
  Closes the long-standing "visual regression testing for UI features" item in
  the E2E test strategy. (#1907, issue #1908)
- **Parser contract index.** Lexer and parser-core paired-delimiter and
  balanced-segment behavior is now covered by a conformance matrix and documented
  in `docs/reference/PARSER_CONTRACTS.md`. (#1319, #1321, #1324)
- **RIPR coverage tool upgraded from 0.5.0 to 0.9.0.** The CI seam-proof gate
  now uses the current RIPR release. (#1329)
- **AST kind inventories are compiler-derived.** `ALL_KIND_NAMES` now derives
  from `NodeKind::VARIANTS`, removing a hand-maintained mirror list that could
  drift from the enum. (#1491)
- **File-local semantic fact IDs include file identity.** Stable semantic IDs
  for anchors, entities, occurrences, and file-scoped edges now include
  `FileId`, preventing identical source in different files from colliding while
  preserving the file-neutral reference-source sentinel. (#1876)
- **File semantic bundle hashes include synthetic facts before hashing.**
  Generated-member and eval-sub synthetic entities/anchors now flow into the
  canonical shard builder before category hashes are computed, and shards carry
  an explicit producer schema version. (#1904)
- **AST child-classification flags match traversal.** `contains_children` now
  agrees with `Node::for_each_child` for every `NodeKind`, with a drift-guard so
  traversal consumers do not silently skip children. (#1891)
- **HIR lowers core control-flow shells.** Branches, loops, control transfers,
  and postfix statement modifiers now lower into PIR-v0-aligned HIR shells with
  source anchors and static shape facts. No LSP provider behavior is cut over by
  this substrate change. (#1902)
- **PIR v0 tooling IR is lowered from HIR.** The compiler substrate now exposes
  a PIR v0 intermediate representation for tooling consumers while preserving
  the no-provider-cutover boundary for this release. (#1900)
- **Compile-state layers are specified and fixture-pinned.** PLSP-SPEC-0030 now
  defines the L0-L6 compile-state stack, determinism obligations, dynamic
  boundaries, and no-provider-cutover claim boundary, with alignment tests.
  (#1895)
- **Semantic snapshot and identity invariants are documented.** The semantic
  model now has release-facing source truth for snapshot shape, identity
  stability, and consumer obligations. (#1599)
- **Provider-decision schema alignment is restored.** `provider_decision.v1`
  now matches its schema/spec model so release evidence is not built from a
  drifted provider-decision shape. (#1910)
- **`our` declaration semantic-token facts are scoped.** Semantic facts for
  package-scoped `our` declarations now carry a scoped fact class, avoiding
  ambiguity for downstream semantic consumers. (#1920, issue #1922)
- **`state` declaration semantic-token facts are scoped.** The output-neutral
  compiler-token cutover now covers the `my` / `our` / `state` lexical
  declaration trio while continuing to fall back to parser/HIR token output for
  unmatched, stale, generated, or low-confidence spans. (#2030, issue #2027)
- **Parser boundary responsibilities are documented.** POD, heredoc-body, and
  `__DATA__` / `__END__` non-executable boundaries now have a consumer contract
  in `PARSER_CONTRACTS.md`, including strict versus lenient detection posture.
  (#1896)
- **LSP transport framing uses checked body-offset arithmetic.**
  `Content-Length` frame parsing now guards the `body_start` offset calculation
  with checked arithmetic and recovers through the existing invalid-length path
  on overflow. (#1793)
- **Coverage and test gates are separated.** Patch coverage now reports
  coverage shortfall/setup/routing failures separately from routed test
  failures, so a latent unrelated routed test belongs to a test-named gate
  rather than the Codecov/Patch-95 verdict. (#1482, #1549, #1576, #1581, #1586)
- **Coverage receipt tests cover closeout helpers.** Allocation-tracker and
  active-goal manifest coverage tests keep the closeout proof paths visible to
  Patch-95 without treating routed test failures as coverage failures.
  (#1950, #2041)
- **Agent lease proof-control-plane coverage is covered.** Agent lease acquire,
  verify, expiry, stale snapshot/head, malformed input, and task-validation
  paths now have focused xtask unit and CLI tests so lease proof infrastructure
  remains visible to Patch-95. (#2045, issue #2043)
- **CPAN corpus ratchet can run a bounded top-50 profile.** The post-merge corpus
  workflow now has a bounded representative mode in addition to the full ratchet;
  release accuracy claims still require the corresponding receipt. (#1520)
- **Semantic snapshot and PackageSubTable oracle rails are available.** The
  corpus/tooling substrate now has a semantic SNAPSHOT stability rail, an
  end-to-end PackageSubTable differential runner slice, and the first HIR-body
  vertical slice for assignment-shaped expressions. These are compiler-foundation
  receipts, not live LSP provider cutovers. (#2569, #2570, #2571)
- **DAP test seed helpers are gated from production artifacts.** Integration
  tests that need debug-adapter seed helpers now opt into the `test-helpers`
  Cargo feature, and the full parser/DAP CI recipe enables that feature
  explicitly. (#2596, issue #1341)
- **DAP conditional-breakpoint behavior has a real debugger receipt.** The
  conditional-breakpoint regression now launches `perl -d` and observes the
  true stop iteration instead of simulating Perl condition semantics in Rust.
  (#1843, issue #1629)
- **Runner disk preflight and failover are explicit.** Self-hosted runner
  routing now treats disk hygiene as a preflight invariant and falls back only
  for disk-preflight failures, without masking real test or gate failures.
  (#1528)
- **Self-hosted CI removes stale workspace `target/` before checkout.** CX43
  and CX53 Rust Small, RIPR, and UB-review jobs now delete the gitignored
  workspace `target/` during pre-checkout ownership cleanup. Real Cargo output
  remains on `/mnt/ci-scratch`, while stale root-owned workspace receipts no
  longer block checkout or `target` creation. (#1886)
- **Hash-key completion regression tests have unique names.** The duplicate
  test identifier that broke `perl-lsp-rs-core` test builds was renamed without
  changing the fixture or assertions. (#1938)
- **Gate-list rendering has CLI contract coverage.** The `cargo xtask gates
  --list` path now has tests for PR-fast tier filtering, explicit gate
  filtering, and actionable unknown-gate errors without executing configured
  gates. (#1939, issue #1942)
- **PR-fast capability snapshots are current.** LSP capability YAML and JSON
  snapshots were regenerated after `insertTextModes` support so PR-fast guards
  verify the current server contract instead of stale expected output. (#2039,
  issue #2042)
- **Test::More and Test2 inline-completion packs have contract fixtures.** The
  completion-pack matrix now covers import-present positives plus no-import,
  comment, string, POD, near-match, and malformed-context quiet paths for the
  Test::More and Test2::V0 assertion packs. (#1945, issue #1943)
- **Corpus gold fixtures avoid invalid Perl syntax.** Two parser gold fixtures
  that were invalid under Perl 5 were corrected so corpus accuracy metrics no
  longer count fixture bugs as parser false negatives. (#1903)
- **Completion regression coverage covers sigil and quoted-key edges.**
  Variable completion now has regression tests for `$`/`@`/`%` sigil filtering,
  and the routed completion coverage pack exercises double-quoted special hash
  keys after the #1839 gate repair. (#1842, #1894)
- **PR summary rendering coverage was raised.** The coverage gate has additional
  tests for PR summary rendering so Patch-95 behavior stays tied to the
  coverage-reporting path. (#1890)
- **Rust toolchain documentation and CI pins match the actual 1.95 floor.**
  Normative onboarding, stability, CI, and template docs now align with
  `Cargo.toml`, `rust-toolchain.toml`, clippy policy, flake pins, and CI
  toolchain selection. (#1932, #1954, #1957)
- **Main fmt drift was repaired before release staging.** The post-refactor fmt
  drift on main was corrected, and `cargo xtask fmt --check` was restored as a
  clean release gate. (#1960, #2038)
- **Workflow privilege analysis fails closed for untrusted event expressions.**
  Jobs with write permissions must prove every event-expression branch is
  anchored to a trusted event. (#1539)
- **Execute-command routed-suite expectations were refreshed to the tightened
  contract.** The stale test setup was corrected without loosening the
  execute-command assertions. (#1530)
- **Docs/assets-only PRs no longer run the full Rust matrix while preserving
  required workflow triggers.** Pull requests that touch only documentation,
  extension media/snippets, or tree-sitter examples skip the expensive Rust CI
  jobs inside the always-triggered CI workflow; mixed code/doc PRs still run
  the full matrix, and workflow-trigger lint stays enforced. (#1688, #1816,
  #1817)
- **Release evidence git recovery works in Windows worktrees.** Coverage
  baseline, native-tooling status, and RIPR evidence tasks now retry git
  `HEAD`/diff discovery with an ancestor-discovered
  `GIT_DIR`/`GIT_WORK_TREE` fallback, so stale-receipt and release-evidence
  packets still emit when bash/WSL git cannot resolve a worktree gitdir.
  (#1833, #1878, #1881)
- **VSIX release verification is storage-safe and restart-safe.** The VS Code
  bundle script now honors `CARGO_TARGET_DIR` when locating the release binary,
  and the extension engine floor is aligned with the checked-in VS Code API
  typings so `npm run verify:marketplace` can package the release candidate
  from an agent worktree. First-run reinstall also no longer races activation
  into stale language-client state. (#1867)
- **Draft PR ripr routing treats skipped routers as neutral.** Draft pull
  requests no longer fail the ripr aggregate solely because the router
  intentionally skipped execution. (#1689)

---

## [0.16.0] - 2026-06-06

Release notes: [v0.16.0](docs/releases/v0.16.0.md)

Released as a distinct artifact: tag `v0.16.0` at
`b6d9f12b995ad8ad78ca641940bd73e4b1a3c26d`, GitHub Release published
2026-06-06 with 9 assets, and `perllsp` 0.16.0 on crates.io (2026-06-06, not
yanked). See `RELEASE_HISTORY.md` for the canonical release ledger.

> A 2026-07-22 audit incorrectly recorded this version as skipped and rolled
> into 0.17.0. That was retracted on 2026-07-25 after verification against the
> git tag, the GitHub Releases API, and the crates.io registry. The error is
> documented in `RELEASE_HISTORY.md` rather than erased, because it was
> self-reinforcing: it had been written into both this file and the ledger, so
> the repository corroborated its own mistake.

### Added

- **Perldoc / POD virtual documents** — `perldoc://` document links now resolve
  to POD content from the workspace. Labels in `=head2` and `=item` sections are
  linked targets within the virtual document; module links prefer workspace-local
  POD before falling back to external `perldoc`. (`workspace/textDocumentContent`
  extended with labeled-target link generation.) (#1186)
- **LSP 3.18 applyEdit metadata** — `workspace/applyEdit` requests from the
  server now include `metadata` (`label`, `description`, `isRefactoring`) when
  the client advertises support, providing editors with descriptive undo labels
  for server-originated refactors. (#1184)
- **LSP 3.18 receipt locks** — New negative-claim and contract-lock tests verify
  that capability-gated LSP 3.18 fields (`CompletionList.itemDefaults.data`,
  `CompletionList.applyKind`, `CodeActionOptions.documentation`,
  `SnippetTextEdit` workspace edits, `ApplyWorkspaceEditParams.metadata`,
  `textDocument.codeLens.resolveSupport.properties`) are only emitted when the
  client advertises the relevant capability. Accidental emission is now a test
  failure. (#9628)
- **Semantic tokens: label token type** — Perl statement and control labels
  (`LOOP:`, `BLOCK:`) are now classified with a `label` token type. The semantic
  token type count grows from 23 to 24.
- **Quality gate enforce-new-RIPR+** — The transition gate now blocks new RIPR+
  gaps on every PR. Existing gaps are tracked under a burndown exception
  (expires 2026-09-30). New gaps are hard-blocked immediately. (#8197)
- **Product smoke harness** — 30 UX fixture files and a 40-request smoke script
  provide a deterministic release-readiness check. 6 fixtures / 40 requests
  pass at the RC freeze commit.
- **`perl.explainProviderDecision` execute-command** — Returns the structured
  provider decision explanation payload; reports a low-confidence fallback when
  no provider-specific receipt is attached.

### Fixed

- **Inline completion hard reject zones** — The server now returns an empty
  result for positions inside string literals, comments, heredoc bodies, and
  regex literals, preventing spurious suggestions in non-code contexts. (#9631)
- **Inline completion replacement range safety** — Replacement ranges are now
  clamped to the current line and validated against the document length before
  being emitted, preventing out-of-range positions from reaching the client.
  (#9626)
- **Inline completion trigger-kind policy** — Automatic trigger requests receive
  only the single top deterministic candidate; explicit invoked requests keep the
  richer set. This matches the LSP 3.18 `triggerKind` intent. (#9621)
- **Inline completion LSP 3.18 registration** — Static clients receive top-level
  `inlineCompletionProvider`; dynamic-capable clients receive
  `client/registerCapability`; `experimental.inlineCompletionProvider` is never
  emitted.
- **Folding range refresh receipt** — The server correctly handles
  `workspace/foldingRange/refresh` round-trips in the test harness. (#9633)
- **vscode-extension: Perl-missing remediation message** — The extension now
  shows a corrected message when the `perl` executable is not found.

### Documentation

- Updated JetBrains/LSP4IJ setup docs to prefer the upstream LSP4IJ `perl-lsp`
  integration when available.
- Added the semantic inline-completion roadmap (deterministic project-aware
  ghost text as the lane goal; AI optional and gated).

## [0.15.2] - 2026-05-26

Release notes: [v0.15.2](docs/releases/v0.15.2.md)

Crates.io packaging hotfix for the `cargo install` path.

### Fixed

- **`perl-lsp-rs-core` crate now self-contained on crates.io.** `build_catalog.rs`
  is now included in the published package so the crate is self-contained after
  extraction. This fixes the `0.15.1` crates.io install failure for `perllsp`
  and `perl-dap`. (#9613)

### Added

- **Package-content gate for `perl-lsp-rs-core`** — CI now checks the `.crate`
  file list, extracts the archive, and verifies the unpacked crate with
  `cargo check --locked`. (#9613)

## [0.15.1] - 2026-05-26

Release notes: [v0.15.1](docs/releases/v0.15.1.md)

Patch release focused on LSP4IJ inline completion and lean editor-mode hardening.
Includes the Neovim interactive-latency improvements (generation-aware stale-read
cancellation, `--runtime-mode e2e`, `--diagnostic-mode syntax-only`).

### Added

- **Neovim / lean-editor latency rail** — `--runtime-mode e2e` /
  `PERL_LSP_E2E=1` profile: zero diagnostic debounce, syntax-only diagnostics,
  no eager workspace indexing, no file watchers by default.
- **Generation-aware stale-read cancellation** — hover, completion, definition,
  declaration, typeDefinition, implementation, and references are cancelled with
  `RequestCancelled` when the document generation advances between ingress and
  dispatch.
- **`perl.explainProviderDecision` execute-command surface** — returns the
  structured provider decision explanation payload; reports a low-confidence
  fallback when no provider-specific receipt is attached.
- Raw-RPC latency receipts in `perl-lsp-ux-tests::ux_latency_raw_rpc` and a
  Neovim lean smoke script at `scripts/ux/neovim_lean_smoke.sh`.

### Fixed

- **LSP4IJ inline completion registration** — static clients receive top-level
  `inlineCompletionProvider`; dynamic-capable clients receive
  `client/registerCapability`; `experimental.inlineCompletionProvider` is never
  emitted.
- **Lean editor mode watcher gate** — `--file-watchers=false` is now honoured
  during dynamic watcher registration while feature-specific registrations
  (inline completion) remain available.
- **Semantic tokens no longer advertise delta support** until the
  result-id/delta path is implemented.

### Notes

- This release does not implement true incremental AST reuse. Latency
  improvements come from skipping avoidable background work and cancelling
  stale reads earlier.

## [0.15.0] - 2026-05-22

Release notes: [v0.15.0](docs/releases/v0.15.0.md)

Minor release focused on JSON-RPC type safety and fixing the LSP4IJ
file-watcher registration crash. Breaking change in the public
`perl-lsp-rs-core::protocol` API (request/response ID field type) lifts
this to a minor version under 0.x semver.

### Fixed

- **LSP4IJ file-watcher registration crash** - Server no longer emits
  wall-clock millisecond IDs for `client/registerCapability`
  (~1.7e12 overflows i32 in strict clients including LSP4IJ). All
  server-to-client requests now route through a bounded `AtomicI32`
  allocator that emits values in `1..=i32::MAX` and wraps cleanly.
  This unblocks JetBrains users on the LSP4IJ plugin. (#221, #224)

### Added

- **Typed JSON-RPC request IDs** - `JsonRpcId` (strict-shape enum:
  integer | string; rejects null/fractional/object/array at the serde
  boundary) and `ServerRequestId` (positive-i32 newtype with no
  out-of-range constructor) added to `perl-lsp-rs-core::protocol`.
  The type system now makes the file-watcher crash structurally
  impossible to reintroduce. (#221, #224)
- **Strict inbound ID validation** - Invalid request-ID shapes
  (null, fractional, object, array) are rejected at the transport
  boundary instead of producing undefined behavior deep in the
  dispatcher. (#221)
- **LSP4IJ regression test** - File-watcher registration request ID
  asserted to be a bounded integer in `1..=i32::MAX`. Source-guard
  tests pin the fix against `lifecycle/watchers.rs` re-introducing
  wall-clock-derived IDs. (#221)

### Changed

- **BREAKING:** `JsonRpcRequest.id` and `JsonRpcResponse.id` are now
  `Option<JsonRpcId>` instead of `Option<serde_json::Value>`.
  Consumers of the published `perl-lsp-rs-core::protocol` crate must
  use `JsonRpcId::Integer(N)` / `JsonRpcId::String(...)` in tests and
  any external construction. `Value` round-trips via `to_value()` /
  `from_value()`. (#221)
- **BREAKING:** `outbound::OutboundSender::send_request` now takes
  `ServerRequestId` instead of raw `i64`. (#221)
- **Cancellation registry typed end-to-end** - `CancellationRegistry`
  tokens, cleanup contexts, and cache are keyed by `JsonRpcId`
  instead of `format!("{:?}", value)` strings.
  `PerlLspCancellationToken.request_id`,
  `RequestCleanupGuard.request_id`, `cancel_mark` / `is_cancelled` /
  `register_progress_request`, and the runtime `cancelled` /
  `progress_token_to_request` collections all move from `Value` to
  `JsonRpcId`. Integer and string IDs with the same textual form
  (e.g. `7` vs `"7"`) are now independently cancellable. (#223, #224)
- **`pending_workspace_configuration_requests`** is now keyed by
  `ServerRequestId` rather than raw `i64`. (#221)

### Looking ahead

- LSP interactive latency rollout rail at
  [`docs/development/LSP_INTERACTIVE_LATENCY_ROLLOUT.md`](docs/development/LSP_INTERACTIVE_LATENCY_ROLLOUT.md).
  Workload-profile and stale-work-cancellation work that benefits
  Neovim and LSP4IJ equally; targets 0.15.1. Umbrella tracking: **#229**.

## [0.14.0] - 2026-05-12

Release notes: [v0.14.0](docs/releases/v0.14.0.md)

> **Release boundary correction.** No `v0.13.4` tag was cut. The previous
> actual tag is `v0.13.3`, so the valid cumulative comparison is
> [`v0.13.3...v0.14.0`](https://github.com/EffortlessMetrics/perl-lsp/compare/v0.13.3...v0.14.0).
> That range includes the source state prepared while the workspace carried
> version `0.13.4`; it is not a narrow 0.14-only logical ledger.
>
> <!-- lineage-correction:0.13 -->

### Added

- **Rust 1.95 MSRV** — Minimum supported Rust version raised to 1.95.
  Consumers must use `rustup update stable` (Rust 1.95+ ships stable).
- **Runtime-owned TTL completion cache** — Prefix module scan results are
  now cached with a bounded TTL, eliminating redundant lookups across
  successive completion requests. (#8514 → PR #8667)
- **Literal `require`/`use` symbol tracking** — Symbols from `require
  'Module.pm'` and explicit `use Module` imports are now tracked and offered
  in completions. (#8623 → PR #8678)
- **Real-workspace provider baseline** — Integration tests against a real
  Perl project fixture give confidence that LSP providers work beyond
  synthetic test data. (#8637 → PRs #8682, #8694)
- **DAP module-resolution smoke tests** — Catches regressions in debug
  adapter module loading before they ship. (#8621 → PR #8677)
- **PerlOracleEnv v1 subprocess contract** — Replaces ambient
  `$ENV{PERL5LIB}` injection with an explicit typed contract; subprocess
  environment is now auditable. (#8622 → PRs #8675, #8679)
- **Non-Rust file policy advisory checker** — `cargo xtask check-file-policy
  --mode advisory` documents non-Rust file ownership with an enforced
  allowlist, removing a class of accidental scope drift. (#8566 → PR #8708;
  #8568 → PR #8711)
- **PR sticky CI summary and `ci-doctor`** — `cargo xtask ci-doctor` gives
  clear in-PR feedback without reading raw CI logs. (#4825 → PR #8697;
  #4826 → PR #8693)
- **PR title validation** — `cargo xtask pr title-check` catches malformed
  PR titles before CI. (#8614 → PR #8700)
- **Freshness-check xtask** — Prevents stale-binary false passes locally.
  (#8619 → PR #8683)

### Changed

- **Clippy temporary-allow burndown** — 4 of 5 workspace-level
  `#[allow(clippy::...)]` suppression annotations removed. Only
  `collapsible_match` remains, tracked in #8561. (PR #8712)
- **Clippy lint policy MSRV reconcile** — Clippy lint set aligned with
  actual Rust 1.95 availability. (PR #8707)

### Infrastructure

- All deferred items have verified-open successor issues — nothing dropped,
  everything tracked. See `docs/releases/0.14.0-readiness.md` for the full
  queue.

## [0.13.4] - 2026-05-07

Release notes: [0.13.4 prepared milestone](docs/releases/v0.13.4.md)

> **Prepared milestone, not a tagged release.** The repository has no
> `v0.13.4` ref. This section records changes staged while the workspace carried
> version `0.13.4`; those changes first appear in the later tagged `v0.14.0`
> tree. Do not infer a standalone asset, package, or compare boundary from this
> heading.

### Fixed

- **Known session-creep leaks across LSP caches** — Hover, text-sync,
  workspace, stream-session, and workspace-index caches were retaining
  per-session state past document close/delete lifecycle paths. Eviction is
  now wired through, and retained-state regression tests lock the behavior.
  (#8064)
- **Stream sessions cancel across URI variants** — Stale inline-completion
  stream sessions on `didChange` are cancelled even when the client mixes
  canonical `file://` and `file://localhost` spellings for the same
  document. Regression covers both spellings.
- **Regex embedded code annotated, not rejected** — The parser now
  annotates embedded code inside regex constructs instead of failing the
  parse. (#8056)
- **Status pipeline regeneration** — Parser-accuracy artifact is now
  bootstrapped before regeneration (#8069) and quality counts are parsed
  from external target dirs (#8068), so `docs/project/status/*.md`
  reflects reality after out-of-tree CI runs.
- **Stricter VS Code extension lint** — Cleared lint failures from the
  upgraded TypeScript and `@types/vscode` toolchain. (#8065)

### Added

- **Class::Tiny and Class::Tiny::RW OO framework support** — Full
  semantic analysis for the Class::Tiny family across both the
  `ClassModelBuilder` and `SymbolExtractor` pipelines. `use Class::Tiny
  qw(name email)` and bare `has 'name';` declarations now produce
  accessor symbols so go-to-definition, hover, and workspace symbol
  search work for Class::Tiny accessors. (#8062)
- **LSP churn plateau guardrails** — New `memory_plateau.json` receipt,
  nightly + PR CI gates, `scripts/repro_lsp_storm.py` reproducer, and
  `scripts/assert_rss_plateau.py` plateau assertion. Documented in
  `docs/large-workspaces/LSP_CHURN_REPRO.md` and a new
  `RETAINED_STATE_INVENTORY.md` cataloguing every retained cache, its
  owner, and its eviction rule. Runtime pressure counters expose async
  task/debounce/session pressure, and diagnostics churn now has direct
  retained-state coverage. (#8072, #8076, #8088, #8115)
- **Memory-control closeout** — Long-session retained-state memory behavior
  is now covered by lifecycle cleanup, runtime pressure counters, plateau
  receipts, trend rendering, focused subsystem regressions, and
  retained-state inventory policy. This closes the known retained-state /
  session-creep class and adds guardrails against recurrence, without
  claiming every possible memory issue is fixed. See
  [MEMORY_CONTROL_CLOSEOUT.md](docs/large-workspaces/MEMORY_CONTROL_CLOSEOUT.md).
- **Governed clippy lint policy gate** — New CI gate enforcing the
  `policy/clippy-lints.toml` allowlist. (#8066)
- **Parser coverage risk map and baseline.** (#8005)

### Changed

- **MSRV bumped to Rust 1.93.1** — Toolchain pins, CI matrix, clippy
  policy, `clippy.toml`, and `rust-toolchain.toml` aligned. (#7832)
- **Decoupled `perl-semantic-analyzer` from `perl-workspace`** —
  Removed direct coupling; analyzer no longer reaches into workspace
  internals. (#7962)
- **Internal refactor wave (non-user-visible)** — Hover receiver
  package resolver extracted (#8045); execute-command test-runner
  fallback split into helpers (#8046); completion provider
  construction helpers extracted (#8044); call-hierarchy subroutine
  item builder extracted (#8043); refactor-plan contract skeleton
  added to `perl-refactoring` (#7983).
- **Centralized VS Marketplace install badge count.** (#8049)
- **Dependency bumps** — `actions/upload-artifact` 4 → 7 (#7914),
  `actions/checkout` 4 → 6 (#7915), `actions/cache` 4 → 5 (#7916),
  `@types/vscode` (#7912), TypeScript group (#7911).
- Prepared the `v0.13.4` public-alpha patch train with workspace, crate,
  feature catalog, and VS Code extension version surfaces aligned.

## [0.13.3] - 2026-05-03

Release notes: [v0.13.3](docs/releases/v0.13.3.md)

### Fixed

- **VS Code managed binary install reliability** — Reinstall now installs
  into a versioned subdirectory and atomically updates a `current` pointer,
  so a forced reinstall while the previous `perllsp.exe` is held by a
  running process lands in a fresh sibling directory instead of failing
  with `EBUSY`.
- **Lifecycle-safe `Perl: Reinstall Server Binary`** — The command stops a
  running language client before installing, restarts with the newly
  installed binary on success, and falls back to the previous binary on
  download or health-check failure so a failed reinstall never leaves the
  user worse off than before.
- **Extended retry budget for transient managed-install file locks** —
  Total retry wait grows from ~4s to ~31s, covering the upper end of
  Windows Defender first-time signature scans on a fresh release artifact.
- **Singleflight managed install** — Activation auto-download, manual
  Reinstall, and the silent update check coalesce so two installs cannot
  race the same destination path.

### Changed

- Strengthened source and published VS Code smokes to reinstall twice
  across Windows, macOS, and Linux, with the binary held by a spawned
  process during the second pass. Smokes upload artifacts under
  `target/receipts/vscode-smoke/<source>/<os>/` on every run.
- Prepared the `v0.13.3` public-alpha patch train with workspace, crate,
  feature catalog, and VS Code extension version surfaces aligned.

## [0.13.2] - 2026-05-02

Release notes: [v0.13.2](docs/releases/v0.13.2.md)

### Changed

- Prepared the `v0.13.2` public-alpha patch train with workspace, crate,
  feature catalog, and VS Code extension version surfaces aligned.
- Made release closeout focus on the real user install surfaces: Homebrew tap,
  GitHub release assets, VS Code Marketplace, and Open VSX.
- Aligned parser scorecard truth semantics for clean-ingestion, salvage, and
  insufficient-data rows.

### Fixed

- Locked Homebrew tap, GNU/musl binary selection, installer target selection,
  and VS Code managed-binary startup paths behind release hygiene checks.
- Added release-note chooser and install-surface checks to keep future
  public-alpha release notes from drifting back to stale install guidance.

## [0.13.1] - 2026-05-01

Release notes: [v0.13.1](docs/releases/v0.13.1.md)

> **Release boundary correction.** No final `v0.13.0` tag was cut. The release
> line moved directly from `v0.13.0-rc1` to `v0.13.1`; use
> [`v0.13.0-rc1...v0.13.1`](https://github.com/EffortlessMetrics/perl-lsp/compare/v0.13.0-rc1...v0.13.1)
> for source comparison.

### Changed

- Hardened public-alpha release channels after the `v0.13.0-rc1` rehearsal.
- Decoupled Open VSX publishing from VS Code Marketplace publishing.
- Clarified release naming: package versions use normal SemVer while product
  posture remains public alpha.
- Improved CI Gate timeout headroom and diagnostics for release runs.
- Corrected Homebrew/tap naming and formula generation around the `perllsp`
  binary.

## [0.13.0-rc1] - 2026-04-30

Release notes: [v0.13.0-rc1](docs/releases/v0.13.0-rc1.md)

> **Historical outcome.** This remained the only `0.13.0` tag. No final
> `v0.13.0` source boundary was created; the next tagged release was
> `v0.13.1` on May 1, 2026.

### Fixed

- **CI cancellation cascade fix — label events no longer cancel active runs** —
  `cancel-in-progress` now scopes to `pull_request.synchronize` only, so applying
  labels (e.g. `merge-ready`, `ci-green`) does not abort an in-flight CI run on
  the same PR. Adds a `LABEL_EVENT_CANCELS_PR_RUN` xtask lint to prevent the
  failure mode from re-entering. Resolves a recurring queue-blocker that
  surfaced as `exit 143` SIGTERM aborts on PR Smoke and CI Gate. (#7581)

- **First-run error messaging now surfaces to users instead of silent logging** — When
  workspace root is not detected (e.g., opening a single file without opening a folder),
  perl-lsp now sends a `window/showMessage` notification with actionable guidance:
  "perl-lsp: workspace root not detected — module resolution disabled. To enable: open
  the project folder in your editor (File > Open Folder) rather than individual files.
  This warning appears once per server session." Previously this was logged to the server
  log only and users saw nothing. The warning flag is stored as an `Arc<AtomicBool>` on
  `LspServer`, so each server session shows the warning independently — in multi-root or
  multi-server workspace configurations, each `LspServer` instance tracks its own shown
  state rather than sharing a process-level `Once`. (#4178)

### Migration

- **Microcrate collapse complete — migration guide available** — The 0.13.0-rc1/0.13
  train drops the
  published crate count from 132 to 32 across 10+ collapse waves. All ~100 retired
  crate names stop appearing on crates.io after this release; their code lives as
  subfolder modules inside the owning published crate. See
  [`docs/MIGRATION_v0.13.md`](docs/MIGRATION_v0.13.md) for the complete
  old-path → new-path mapping for every retired crate, feature flag changes
  (`lsp-ga-lock`, `incremental`, `workspace_refactor`), and the breaking-changes
  summary per wave. (#7292, #4410)

### Internal

- **Release prep: start `v0.13.0-rc1` version staging** — bumped workspace and internal crate dependency versions to `0.13.0-rc1`, updated the feature catalog metadata version, and refreshed the top-level README release line for release-candidate signaling. (#0000)

- **`cargo xtask published-crate-count`** — new ratchet gate that monitors the
  count of entries in `[workspace.metadata.publish.allow]` and prevents accidental
  regression during the microcrate collapse (ADR-0041). Fails if the count exceeds
  the baseline in `xtask/published-crate-baseline.txt`; auto-tightens the baseline
  when count decreases. Run via `just ci-published-crate-count` or directly as
  `cargo xtask published-crate-count`. (#4416)

## [0.12.4] - 2026-04-12

Release notes: [v0.12.4](docs/releases/v0.12.4.md) · [GitHub Release](https://github.com/EffortlessMetrics/perl-lsp/releases/tag/v0.12.4)

<!-- 2026-04-11 session: 46 PRs merged across navigation, pragma scoping, incremental parsing, workspace refactoring, Windows hardening, and CI hygiene -->
<!-- 2026-04-12 session: ~25 PRs merged — DAP scorecard, rename perf, editor UX, diagnostics polish, hover improvements, workspace/config, completion ranking, Windows compat, CI hygiene -->

### Headlines

- **Inherited and role method navigation now works end-to-end** — `goto-definition`,
  hover, and workspace completion all BFS through Moo/Moose `with 'Role'` and
  `extends`/`use parent` chains. Previously only direct-method navigation worked;
  inherited methods from parent classes or composed roles returned nothing.
  AUTOLOAD-backed method calls also resolve through the fallback path, and hover
  surfaces the AUTOLOAD resolution. (#4077, #4091)

- **Pragma tracker correctness sweep** — four independent false-negative paths
  in `PragmaTracker` / `check_strict_warnings` fixed in one wave:
  - eval- and sub-scoped `use strict`/`use warnings` no longer suppress
    file-level PL100/PL101 diagnostics; `state_for_offset` is now the single
    source of truth for top-level pragma state (#4052)
  - conditional `use if` / `use unless` pragmas are tracked via suffix
    matching on the flattened argument list (#4050)
  - explicit `use feature` / `no feature` state (including `qw(...)` and
    `:X.Y` version bundles) drives lint decisions about `switch` and other
    feature-gated constructs (#4038)
  - phase-block (`BEGIN`/`END`/`INIT`/`UNITCHECK`/`CHECK`) pragmas are kept
    lexically scoped instead of leaking to the enclosing file; the bad
    `strict_warnings` phase-block override that suppressed PL100/PL101 is
    gone (#4108)

- **Incremental parsing: segment-based token cache + two-sided checkpoint
  window** — replaces the monolithic `TokenCache` with sorted segment storage
  so edits only invalidate overlapping segments, and `CheckpointCache::find_after()`
  bounds the re-lex region by the nearest left and right checkpoints instead of
  a fixed +100-byte heuristic. New `IncrementalStats` counters surface
  segment reuse, invalidation, and relex distance. 23 new correctness tests
  plus a 13-group Criterion benchmark suite. A follow-up correctness fix
  forces a full reparse when a nonzero checkpoint window has no cached prefix
  tokens, preventing a suffix-only token stream from being fed to the parser.
  (#4029, #4048, #4076)

- **Workspace-wide file-operations refactoring** — `workspace/willRenameFiles`
  now plans edits across unopened files by reading from the open-doc cache,
  the workspace index, and finally the filesystem; `workspace/willDeleteFiles`
  emits a user-visible `Warning` when deletion would break cross-file
  references discovered via `index.file_symbols()` + `find_references()`.
  Multi-file rename batches are merged per-URI via a new `append_workspace_edits`
  helper. (#4056, #4098)

- **`workspace/configuration` reverse-request flow** — the server now parses
  the client's `workspace.configuration` capability and, when advertised,
  issues a `workspace/configuration` reverse request per folder after
  `.perl-lsp.toml` is loaded, merging returned overlays into each folder's
  `effective_workspace_config` (TOML stays the base layer). Re-fetched on
  `workspace/didChangeConfiguration`. JSON-RPC responses without `method`
  are routed as an internal `$/perl-lsp/clientResponse` pseudo-notification
  through the existing dispatch system. Non-`file://` workspace folder URIs
  (`vscode-remote://`, `untitled:`, etc.) are tolerated end-to-end —
  `to_file_path()` calls are routed through a file-only URI helper and
  non-filesystem folders are skipped during indexing scans. (#4093, #4059)

- **Windows extended-length path fix for external commands** — `Path::canonicalize`
  on Windows returns paths with the `\\?\` prefix, which Win32 APIs accept but
  `perl.exe` / `prove` / `yath` do not. `normalize_path_for_external_command`
  strips the prefix before every spawn site in `execute_command::provider`.
  Unblocks Run Tests, Run File, and Run Test Sub on Windows. (#4089)

- **Perl::Critic diagnostics UX overhaul** — Critic policy codes now carry
  `source: "perlcritic"` in the LSP surface and an explicit `data.fixable`
  list; three more policy aliases route to existing quick-fixes
  (`RequireUseStrict`, `RequireUseWarnings`, `ProhibitUnusedVariables`);
  missing `perlcritic` binary and invalid profiles surface as workspace
  warnings and health-check output instead of silently skipping. (#4113)

### Added

- **Workspace `workspace/configuration` reverse-request flow** with client
  capability gating, per-folder scoping, and JSON-RPC response routing via
  `$/perl-lsp/clientResponse` (#4093).

- **`workspace/willRenameFiles` workspace-wide planning** — reads text from
  open-doc cache, workspace index, or filesystem, and merges multi-URI edits
  via a new `append_workspace_edits` helper (#4056).

- **`workspace/willDeleteFiles` safe-delete warnings** — emits `Warning`
  severity diagnostic when the delete would break cross-file references
  discovered via `index.file_symbols()` + `find_references()` (#4056).

- **`HoverExtracted::InheritedMethod` variant** with Phase 2 workspace BFS
  over parents and roles; `collect_all_package_members` BFS for workspace
  completion; `workspace_document_text` exposed as `pub(super)` so hover can
  reuse it (#4077).

- **AUTOLOAD fallback in goto-definition and hover** — explicit method-call
  navigation resolves through `AUTOLOAD` when the named method is absent;
  inherited-method hover surfaces the AUTOLOAD resolution (#4091).

- **`Readonly` and `Const::Fast` wrapper declarations** are now surfaced as
  constant symbols by `perl-symbol-surface` and `perl-semantic-analyzer`;
  declaration tokens are marked readonly in semantic-tokens output, including
  package-qualified `our` constants. Scalar, array, hash, and package
  regression coverage added. (#4040, #4043)

- **LSP semantic token delta support advertised** — `semanticTokens.full`
  switched from the legacy boolean form to the structured delta form.
  The delta handler already existed; the advertised capability is now
  aligned with LSP 3.16+ expectations. Capability-snapshot tests
  regenerated. (#4026, #4041, #4042)

- **VS Code: Run Test at Cursor** — new `perl-lsp.runTestAtCursor` command
  palette / context-menu entry that resolves the active cursor position
  against existing code lenses and runs the nearest test subroutine,
  subtest, or file-level run lens (#4025).

- **VS Code: Gherkin step-definition navigation and stubs** — navigation
  from `.feature` steps to Perl `Given`/`When`/`Then` definitions plus a
  quick-fix command that generates step-definition stubs. Unit coverage
  for matching, generation, and registration. (#4024)

- **VS Code test runner now prefers `yath` for test files** when present
  on PATH, with `prove` and `perl` fallbacks intact. Unit tests pin the
  runner preference order without depending on local tool availability.
  (#4031)

- **Segment-based incremental token cache + two-sided checkpoint window**
  — `TokenSegment` sorted storage, `CheckpointCache::find_after()`, and
  `reparse_from_checkpoint_two_sided`; new `IncrementalStats` metrics
  (`segments_reused_before`, `segments_reused_after`, `segments_invalidated`,
  `full_tail_fallbacks`, `left_checkpoint_distance`,
  `right_checkpoint_distance`, `bytes_relexed`); 23 correctness tests and
  a 13-group Criterion benchmark suite (#4029).

- **Orphaned unclosed-block recovery tests wired into `perl-parser-core`**
  — the `unclosed_block_recovery_tests` module existed on disk with six
  well-written tests but was never registered in `mod.rs` and had never
  compiled. Now wired with six additional edge-case tests covering C-style
  `for`, `foreach`, `unless`, `BEGIN` phase blocks, doubly-nested unclosed
  blocks, and nested blocks inside `sub`. (#4079)

- **Symbol visibility regression tests** for Error-partial recursion in
  `perl-semantic-analyzer`, covering arrow-truncation errors, unclosed-sub
  recovery, and missing-RHS recovery (#4071).

- **Per-edit checkpoint and cache delta assertions** for incremental
  parsing, replacing cumulative-counter assertions with per-edit deltas
  and tree-equivalence checks against a fresh full parse (#4076).

- **`require VERSION` pragma semantics test** guarding that `require 5.x`
  does not enable strict or warnings in `PragmaTracker` (#4023).

### Changed

- **Pre-push hook switched from `ci-gate` to `pr-fast`** (Tier A). The
  generated `hooks/pre-push` file is regenerated from `perl-ci-hygiene` and
  a regression test keeps the generated text and checked-in hook file in
  sync. `ci-gate` remains documented as the explicit full merge gate.
  (#4088, #4110)

- **Doc-only pre-push fast path skips code gates entirely** instead of
  running workspace-wide `cargo fmt --all -- --check`. Avoids a Windows
  long-path rustfmt crash on prose-only pushes. Regression test asserts
  the doc-only branch exits before any workspace-wide rustfmt check.
  (#4061)

- **Docs-only merge fast-track** added to the review/merge policy so
  doc-only PRs no longer falsely require `reviewed-deep`. Enforced by
  `scripts/pre-merge-check.sh` with shell-test coverage; reviewer / ops
  docs and label automation updated to match. (#4103)

- **`PragmaTracker::state_for_offset(&map, usize::MAX)`** is now the
  single authoritative source of truth for top-level pragma state in
  `check_strict_warnings`; the scope-unaware `walk_node` closure arms
  that scanned eval/sub interiors are removed (#4052).

- **`SymbolExtractor::visit_node()` Error arm** now recurses into
  `partial: Option<Box<Node>>` instead of treating Error as an opaque
  leaf, bringing symbol extraction into parity with every other
  traversal in the codebase (`semantic_tokens`, `class_model`,
  `scope_analyzer`, `for_each_child`) (#4071).

- **Metric framing scoped down** across README, VSCode marketplace
  listing, and v0.13.0 announcement draft — capabilities are now framed
  as advertised surface, not claimed conformance; entry-points table
  added to README; known UX gaps listed explicitly (#4045,
  #4046, #4049, #4051).

### Fixed

- **Parser error recovery and symbol extraction under partial `Error` nodes**
  — unclosed block recovery landed in `perl-parser-core` (PR #4079) and
  symbol extraction now descends into partial `Error` nodes (PR #4071),
  closing issue [#3499](https://github.com/EffortlessMetrics/perl-lsp/issues/3499).

- **Navigation: inherited and role methods in goto-def, hover, and
  completion** — BFS traversal now chains `model.roles` alongside
  `model.parents` in `inherited_method_definition_location`; hover
  wires the previously dead-code `resolve_inherited_method_hover` path;
  workspace completion uses a new `collect_all_package_members` BFS that
  replaces the direct `get_package_members` call in
  `add_workspace_method_completions` (#4077).

- **Navigation: AUTOLOAD-backed method calls** resolve through
  `AUTOLOAD` fallback in both goto-definition and hover when the named
  method is absent (#4091).

- **Diagnostics: eval- and sub-scoped pragmas no longer suppress
  file-level PL100/PL101** — `pragma_map.iter().any()` replaced with
  `PragmaTracker::state_for_offset` at `usize::MAX`; `walk_node` closure
  arms that descended into `NodeKind::Eval` / `NodeKind::Subroutine`
  bodies and falsely set `has_strict = true` are removed. 4 new tests
  cover eval-scoped and sub-scoped false-negative paths. (#4052)

- **Pragma: phase-block pragmas kept lexically scoped** — `BEGIN`, `END`,
  `INIT`, `UNITCHECK`, and `CHECK` block pragmas no longer leak to file
  scope; the bad `strict_warnings` phase-block override that suppressed
  PL100/PL101 is removed. Replaced with behavior-spec and integration
  coverage for block-local semantics. (#4108)

- **Pragma: conditional `use if` / `use unless`** — `PragmaTracker`
  recognises `use if CONDITION, MODULE, ...` forms and conservatively
  applies the tracked pragma semantics from the suffix target. Lint
  pipeline regressions confirm conditional strict/warnings suppress the
  missing-pragma hints. (#4050)

- **Pragma: explicit `use feature qw(...)` and `:X.Y` bundles** tracked
  in `PragmaTracker`; `version_compat` understands feature bundles; 
  `no feature` lexical disablement is honored. Regressions cover
  `switch` enablement via bundles and lexical disablement. (#4038)

- **Incremental parsing: prefix correctness at checkpoint boundaries** —
  when a nonzero checkpoint window has no cached prefix tokens, fall
  back to a full reparse instead of assembling a suffix-only token
  stream for `Parser::from_tokens`. Regression compares incremental vs
  full parse for an edit past the first checkpoint boundary. (#4048)

- **Semantic analyzer: symbol extraction descends into Error partial
  nodes** (#4071). The arrow-truncation recovery path will start
  producing new symbols if the parser begins wrapping declarations in
  `Error { partial: Some(...) }`.

- **Workspace: non-`file://` URIs tolerated as workspace roots** —
  `vscode-remote://`, `untitled:`, and other virtual schemes are kept
  in LSP string form; direct `to_file_path()` calls are routed through
  a file-only URI helper; workspace folder matching normalises trailing
  slashes; non-filesystem folders are skipped during indexing scans.
  (#4059)

- **Execute-command: Windows extended-length path prefix stripped** —
  `normalize_path_for_external_command` removes the `\\?\` prefix on
  Windows at every spawn site in `execute_command::provider` (yath,
  prove, perl primary and fallback paths, `run_test_sub`, `run_file`).
  Non-Windows is a zero-cost identity via `#[cfg(windows)]`. Closes
  the Windows Run Tests regression. (#4089)

- **DAP types: basename derivation for Windows-style source paths** —
  `Source::new()` adds a narrow fallback when `Path::file_name()` returns
  the entire input string (as it does for backslash-separated paths on
  Unix hosts), so `C:\Users\dev\project\lib\Module.pm` now derives
  `Module.pm` correctly. Unblocks the previously-failing
  `source_with_windows_path` regression. (#4028)

- **Xtask `features verify` repaired** — catalog test paths are now
  resolved from repo root, the advertised-vs-caps snapshot is read from
  `crates/perl-lsp-rs/tests/snapshots/...`, the two-document Insta snapshot
  format is parsed correctly, and the verifier compares against the
  capability-backed advertised LSP subset (#4033).

- **Clippy hygiene: hover/navigation `let_and_return` and
  `needless_borrow`** warnings cleared, plus a follow-up
  `needless_borrow` fix in workspace file-ops after #4052 and #4088
  landed (#4037, #4098).

- **Plan-review hook IO hardened** — `subagent-stop.sh` binds issue
  context from `ISSUE_NUMBER`, payload `issue_number`, or the canonical
  `plan-review-NNN` agent name (in that precedence), instead of the
  broken branch-digit scan that silently labeled random historical
  issues. Fail-loud exit 3 when no valid issue context exists. Both
  `subagent-stop.sh` and `task-completed.sh` normalise payload / receipt
  fields with `tr -d '\r'` so JSONL metrics and CRLF-sensitive receipt
  parsing no longer reject valid UTC timestamps. Canonical
  `plan-review-NNN` naming documented in `.claude/commands/swarm.md`.
  Regression coverage added in both `.ci/scripts/test-hooks.sh` and
  `cargo xtask hook-tests`. (#4064)

- **CI hygiene: allowlisted production panic paths normalised** before
  matching so both Unix and Windows-style relative paths are recognised
  (#4081).

- **CI hygiene: doc-only pre-push fast path skips code gates** entirely
  instead of running workspace-wide rustfmt (#4061).

- **Semantic-analyzer: stale method attribute assertion** fixed to look
  up extracted methods via the current symbol contract while keeping the
  attribute-preservation assertion intact (#4082).

- **Agent definitions: terminal skills made explicit** — `scout-lsp.md`
  gains the missing step 9 `/agent-wrapup` (every other scout variant
  already had it); `reviewer-deep.md` restructures step 4 and adds a
  new step 5 making `/pr-ready` an explicit required follow-up when
  the deep-review decision is "approve". Root cause for the earlier
  incidents where deep reviewers set `reviewed-deep` but forgot to
  call `/pr-ready`, leaving PRs stuck in draft. (#4087)

### Docs

- **Metric framing scoped down** — README, VSCode marketplace listing,
  and v0.13.0 announcement draft now frame capabilities as advertised
  surface, not claimed conformance. README gains an entry-points table
  and an explicit list of known UX gaps. (#4045, #4046, #4049, #4051)

### Tests / Quality

- **Test-side P0 idiom + dependency findings burned down** — three
  `.or_insert_with(Vec::new)` occurrences migrated to `.or_default()`
  in `lsp_cancellation_performance_tests.rs`,
  `test_infrastructure_mocks.rs`, and
  `documentation_validation_mutation_hardening.rs`; explicit
  `HashMap<String, Vec<Duration>>` / `HashMap<String, Vec<usize>>` type
  annotations added where inference became ambiguous;
  `perl-tdd-support` added to `[dev-dependencies]` in `perl-dap-types`,
  `perl-symbol-surface`, and `perl-ast-utils`. (#4002)

- **Match-arm panic asserts burned down** in test code across
  `perl-dap-variables`, `perl-lexer` interpolation tests, and several
  other test suites, continuing the long-running #3258 burn-down
  (#4030, #4032, #4035).

<!-- 2026-04-11 session addendum: PRs merged after the initial changelog entry was written -->

### Added (addendum)

- **Phase-scoped pragma diagnostics** (`PL502`, `PL503`) — new diagnostics flag
  `use strict` / `use warnings` placed inside phase blocks (`BEGIN`, `END`, `INIT`,
  `CHECK`, `UNITCHECK`) where they have lexical block scope rather than file scope;
  quick fixes move the pragmas to file scope preserving shebangs. (#4131)

- **`cargo xtask check-test-wiring` CLI command wired** — PR #4119 added the
  `check_test_wiring` module but omitted the `use` import in `main.rs`; the
  subcommand was returning "unrecognized subcommand". Now fully wired; also fixes
  one genuine orphan discovered by the guard: `crates/perl-lsp-rs/tests/fixtures/integration_example.rs`.
  (#4151)

- **Cross-file `use constant` and parenthesized import lists** — `find_import_source()`
  strips quotes from string args before comparison so `use Foo ('bar', 'baz')` resolves
  via goto-def; `use constant` re-exports are followed across file boundaries. (#4133)

### Changed (addendum)

- **Multi-root workspace integration tests activated in nightly gate** — the 8
  integration tests in `multi_root_workspace_tests.rs` (added in #3984, never run in CI)
  are now wired via a new `ci-workspace-multiroot` justfile recipe, placed in the
  nightly gate only until proven stable. (#4137)

### Fixed (addendum)

- **Hotfix: red master from `check_test_wiring` regex and clippy** — two runtime
  `Regex::new(...).expect(...)` calls in `check_test_wiring.rs` migrated to
  `LazyLock<Regex>` statics; `let_and_return` clippy warning in `parser_corpus_sweep`
  removed; `RUSTSEC-2026-0097` suppressed in audit paths with follow-up in #4149.
  (#4150)

- **Status: feature maturity metadata restored** — valid `maturity` value reinstated
  for the phase-scoped pragma diagnostic capability after #4131 introduced an
  invalid value; `xtask update-status --check` green again. (#4148)

- **DevEx: detect stale installed pre-push hooks** — `just status-check` now compares
  the installed `.git/hooks/pre-push` against the canonical checked-in `hooks/pre-push`,
  normalising CRLF and trailing-blank-line noise. (#4144)

### Tests / Quality (addendum)

- **Perl::Critic missing profile path test** — regression test for an explicitly
  configured but missing Perl::Critic profile path; asserts subprocess is skipped and
  no policy diagnostics are returned. (#4139)

### Added (2026-04-12)

- **DAP launch-success scorecard** — new integration harness measures DAP cold-launch
  pass rate across 5 fixture debuggees (hello, loops, eval, args, begin_end) with
  P50/P95 latency metrics; a new `docs/project/status/dap.md` page surfaces DAP
  coverage alongside the existing LSP status pages. (#4237)

- **Editor UX receipt** — machine-readable `docs/project/status/editor_ux.json`
  receipt generated by `xtask update-status` tracks the editor UX fixture matrix
  pass rate, wired into `quality.md` and the status index. (#4233, #4234)

### Fixed (2026-04-12)

- **Rename operations no longer lag on large files** — `collect_descendant_scopes`
  replaced O(n×d) parent-chain walk with a single O(n) map build + iterative BFS,
  with a cycle guard preventing hangs on pathological self-referential parent links.
  (#4240)

### Changed (2026-04-12)

- **Research-verifier is now the default for claim-heavy PRs** — agent skill
  definitions encode the research-verifier dispatch policy so orchestrators no longer
  need a reminder; claim-heavy criteria defined in three skill files. (#4235)

### Documentation (2026-04-12)

- **`perl-lsp-semantic-tokens` crate docs corrected** — CLAUDE.md updated from
  stale 15 types/7 modifiers to the actual 23 types/13 modifiers, with all token
  types and modifiers listed in index order and Perl-specific extensions called out.
  (#4239)

### Added (2026-04-12 session 2)

- **Compile-time constants hover** — `__FILE__`, `__LINE__`, `__PACKAGE__`, and
  `__SUB__` now show rich hover documentation with descriptions and caveats
  (e.g. `__SUB__` in named subs vs anonymous subs). (#4270, #4294)

- **Fast/slow diagnostic split** — parse errors are now published immediately
  (~440ms sooner) via `publish_parse_errors_fast()`, then replaced by the full
  diagnostic set on the 250ms debounce. Users see red squiggles while typing
  without waiting for scope analysis or perlcritic. (#4279, #4305)

- **Generation-aware staleness guard** — if a `didChange` arrives during slow
  computation (scope analysis, perlcritic, dead-code), the stale diagnostic
  result is suppressed and the debouncer re-fires for the latest version. (#4295)

- **`require Module; Module->import('sym')` completion** — the two-statement
  require+import pattern is now recognised for completion ranking alongside
  `use Module` imports. (#3476, #4296)

- **Module ranking tiers for completion** — completion candidates are ranked
  by import tier (direct import > workspace > CPAN) with string-context
  suppression and open-snippet triggers for module paths. (#4263, #4277)

- **`workspace/configuration` folder propagation** — `didChangeConfiguration`
  now eagerly propagates settings to each folder's `effective_workspace_config`,
  closing a stale-settings window between notification and async pull response.
  (#3515, #4289, #4307)

- **Safe-delete widened dependent detection** — `workspace/willDeleteFiles`
  now detects dependents via both `use` imports and `require` statements,
  surfacing warnings for a broader set of cross-file references. (#3513, #4293)

- **Package declaration rewrite during module rename** — `workspace/willRenameFiles`
  now rewrites `package Foo::Bar` declarations inside the renamed file to match
  the new module path. (#3522, #4291)

### Fixed (2026-04-12 session 2)

- **Package name hover** — hovering a qualified package name like `File::Path`
  previously showed broken hover text because the tokenizer stopped at `:`.
  New `get_package_name_at_position` scans across `::` separators to produce
  correct rich hover with file path, POD, and MetaCPAN link. (#4282, #4306)

- **Signature/Prototype AST byte-span** — `Signature` and `Prototype` nodes
  now carry the correct byte span from the parser, fixing off-by-one ranges
  in hover and semantic tokens. (#4243, #4281)

- **Windows compatibility** — `/proc` reads guarded behind `cfg(target_os = "linux")`;
  hardcoded `/tmp` paths replaced with `std::env::temp_dir()` for cross-platform
  correctness. (#4229, #4278)

### Tests / Quality (2026-04-12 session 2)

- **Cross-folder rename verification** — integration tests verify rename operations
  span both workspace roots correctly. (#3522, #4273, #4292)

- **Import visibility regression tests** — unit tests for `require`+`import`
  symbol resolution patterns. (#3476, #4286)

- **Heredoc `unreachable!` ratchet coverage** — tests cover all 7 heredoc
  unreachable patterns on both path separators. (#4245, #4274)

- **Unused dev-dependencies removed** from 5 crates. (#4183, #4255)


## [0.12.3] - 2026-04-09

Release notes: [v0.12.3](docs/releases/v0.12.3.md) · [GitHub Release](https://github.com/EffortlessMetrics/perl-lsp/releases/tag/v0.12.3)

<!-- Pipeline rehearsal release — validates the full publish + extension + Docker cycle before v0.13.0 public alpha -->
<!-- Rolls up publish pipeline fixes, UX P0 improvements, and CI hardening from Waves 10/11/12 -->

### Headlines

- **`tree-sitter-perl-c` first published to crates.io** — the conventional C grammar
  binding (tree-sitter FFI over the C parser) is now a proper published leaf crate,
  shedding its `libclang`/bindgen dependency in favour of vendored C sources compiled
  via `cc`. Framed as a compatibility and comparison surface alongside the native v3
  parser stack. (#3234)

- **Publish pipeline overhauled** — three layered fixes make the pipeline correct
  and fast: Tarjan SCC topological sort properly handles dev-dependency cycles (#3236);
  dev-dependencies are stripped from each manifest before publishing so circular
  workspace dev-deps no longer block `cargo publish` (#3254); and the registry
  indexing wait is replaced with progressive sparse-index probes that catch silent
  upload failures instead of proceeding on false success (#3230).

- **Archive: 7 dead tree-sitter harness crates removed from the workspace** — the
  old Pest-based `tree-sitter-perl-rs` harness and 6 `perl-ts-*` compatibility shims
  are moved to `archive/`, clearing the `tree-sitter-perl-rs` name for a planned
  Rust-native tree-sitter-style facade over the v3 parser. (#3244, #3250)

- **DevEx polish** — `just doctor` auto-detects and self-heals recurring worktree
  state-corruption bugs; the pre-push hook gains a doc-only fast path and
  self-heals `core.bare=true` corruption; `just bump-version` centralises
  version sync across 191 sites. (#3249, #3238, #3228)

- **Quality burn-down** — ~210 `eprintln!` calls in library crates migrated to
  structured `tracing` macros; three waves of `unwrap`/`expect` eliminations across
  test code; two dead `build.rs` files removed that were causing unnecessary
  recompiles. (#3245, #3229, #3241)

### Added

- **`just doctor`**: one-stop workspace health-check that auto-detects and
  (where safe) auto-fixes recurring state-corruption bugs — `core.bare=true`,
  stale branches, worktree file leaks, orphaned worktree directories, and missing
  pre-push hook. (#3249)

- **`just bump-version`**: centralised version-sync command covering all 191
  version sites (workspace Cargo.toml, every crate manifest, VS Code extension
  manifest and lockfile, `features.toml`, README, CLAUDE.md, ROADMAP). Paired with
  an updated `check-version-sync` gate that now covers all the same sites, so drift
  cannot go undetected. (#3228)

- **`perl-heredoc-anti-patterns` microcrate**: SRP extraction of
  `anti_pattern_detector` from the larger `perl-ts-heredoc-analysis` crate, which
  is now archived. The only part that production code consumed is now a clean
  publishable leaf crate. (#3199)

- **`perl-parser-bench` microcrate**: SRP extraction of the `bench_parser` binary
  that was misplaced inside the tree-sitter-perl-rs harness. Uses `perl-parser`
  (v3 native) directly. (#3198)

- **`perl-parser-pest` published to crates.io**: the legacy v2 Pest-based Perl
  parser is now a published crate, available as a learning tool and Pest reference
  implementation for the broader Perl-in-Rust ecosystem. (#3195)

- **`perl-lsp-ai-provider` published to crates.io**: filled out crates.io metadata
  and added to the publish allow-list. This was a blocker for `perl-lsp-rs`
  publication. (#3196)

- **4 orphaned workspace members registered**: `perl-workspace-folder`,
  `perl-dap-stack`, `perl-lsp-feature-policy`, and `perl-lsp-formatting-types`
  were referenced throughout the workspace but missing from `[workspace] members`,
  causing them to be silently skipped by every workspace-wide CI gate. (#3232)

- **AI streaming tests**: mock streaming-backend coverage for progress, cancel,
  and error paths; final stream sequence field assertion; relaxed error-path
  assertion for terminal final event. (#3170, #3172, #3174, #3175)

- **CPAN corpus caching in CI**: CPAN corpus is now installed and cached before
  the ratchet step, preventing spurious corpus-ratchet failures on clean CI runs.
  (#3173)

### Changed

- **`tree-sitter-perl-c` is now publishable**: vendored C sources compiled via
  `cc` replace the `libclang`/bindgen build step entirely; the single hand-written
  FFI symbol was already sufficient. Crate brought into the workspace as a proper
  member. (#3234)

- **xtask now depends on standalone crates directly**: dev tooling in `xtask` and
  `scripts/test_recursion.rs` was swapped off the archived tree-sitter-perl harness
  onto `perl-parser-pest` (Rust parser) and `tree-sitter-perl-c` (C FFI) directly,
  removing the harness's last consumers before archival. (#3206)

- **`just quick-bench` fixed to actually compare C vs Rust parsers**: previously
  both columns invoked the same `perl-parser-bench` binary (comparing a warm vs
  cold run of the native parser). The C column now invokes `bench_parser_c` from
  `tree-sitter-perl-c`, so the speedup column reflects a real C vs Rust comparison.
  (#3204, #3253)

- **Pre-push hook smarter**: doc-only fast path (markdown/text/license/docs changes
  run `cargo fmt --check` only, skip the full ci-gate); self-heals `core.bare=true`
  corruption before any git operation. (#3238)

- **Publish workflow indexing wait replaced with sparse-index probes**: progressive
  probe at 5s/15s/45s/90s elapsed replaces a fixed 5-minute wait; each crate is
  verified via the crates.io sparse index after publish; the final verify job runs
  unconditionally (`if: always()`) and lists exactly which crates failed. (#3230)

- **`eprintln!` → `tracing` in library code**: ~210 `eprintln!` calls across
  library crates replaced with structured `tracing` macros at appropriate levels
  (warn/error for failures, info for lifecycle, debug/trace for routine output).
  `tracing` added to 6 crates that lacked it. (#3224, #3245)

- **Documentation framing updated**: README Architecture section names the native
  parser/lexer/analysis stack as the architectural centre, distinguishes
  `tree-sitter-perl-c` (C FFI reference, maintained for compatibility) from the
  planned `tree-sitter-perl-rs` facade (Rust-native, in development), and frames
  tree-sitter compatibility as an interoperability surface. (#3247)

- **Per-crate CLAUDE.md headers refreshed** post-archive of tree-sitter harness
  crates. Stale references to archived crates removed. (#3240)

### Fixed

- **Publish: dev-dependency cycles no longer block `cargo publish`** — dev-deps
  are stripped from each crate's `Cargo.toml` before publishing (and restored
  afterward via a `trap` on EXIT). Fixes the 3-crate dev-dep cycle
  (`perl-parser-core` / `perl-tdd-support` / `perl-corpus`) that caused publish
  order failures. (#3254, #3256)

- **Publish: Tarjan SCC topological sort for dev-dep edges** — the previous sort
  excluded dev-dep edges, causing crates that dev-depend on later-published siblings
  to be ordered before them. The fix includes dev-dep edges in the graph, uses
  Tarjan SCC to find strongly-connected components, and retains only inter-SCC
  dev-dep edges (intra-SCC edges are the only ones that can close a cycle).
  (#3236, #3242)

- **Publish: `perl-test-must` published before `perl-tdd-support`** — ordering
  fix for the initial publish sequence that caused `perl-tdd-support` to land
  before its dependency. (#3176, #3177)

- **Corpus ratchet path mismatch** (#3189 / #3257): xtask's CPAN corpus paths are
  now anchored at the workspace root (via `env!("CARGO_MANIFEST_DIR")` at build
  time) rather than resolved against `std::env::current_dir()`. The workflow's
  `test -d` step is aligned to the same absolute path. Regression-guarded by a
  unit test that asserts `workspace_root()` contains a top-level `Cargo.toml`.

- **`hook-tests` workspace scribble** (#3203 / #3246): the hook-test scaffold's
  throwaway git repo inherited `core.hooksPath` from the parent environment,
  causing the parent pre-commit hook to fire inside the temp repo. In one observed
  run the temp repo's `README.md` write landed on the real workspace `README.md`.
  The temp repo is now explicitly isolated with `GIT_CONFIG_NOSYSTEM=1` and
  `core.hooksPath` cleared; temp dirs are created under `$TMPDIR` not the
  workspace root.

- **Windows xtask file-lock** (#3202 / #3241): two dead `build.rs` files removed —
  the root `build.rs` (workspace-only manifest, never run by cargo) and
  `crates/perl-parser/build.rs` (set environment variables that nothing read, and
  marked `perl-parser` dirty on every commit via `.git/HEAD` rerun-if-changed
  directives, propagating unnecessary rebuilds to all 50+ dependents).

- **Windows xtask: recursive subprocess eliminated** (#3221): `cmd_check_parse_errors`
  was spawning xtask as a subprocess of itself, which caused `Access is denied` (os
  error 5) on Windows due to the write-lock on the running executable. The inner
  call is now replaced with a direct function call.

- **Windows xtask: backslash mangling in `smoke-test-release.sh`** (#3214): absolute
  Windows `PathBuf` paths passed to `bash` as arguments caused backslash-escape
  collapse. Fixed by using a relative path instead.

- **Triage workflow silently aborting** (#3235): the `triage-issues` workflow was
  failing on every run that encountered an issue needing labels, silently aborting
  at the first `add_labels` call.

- **`features.toml` dead test paths repaired**: 43 dead test paths corrected to
  match the current `crates/perl-lsp-rs/tests/` layout; the
  `experimental.perlInlineCompletionStream` feature row added (shipped in v0.12.2).
  (#3222, #3251)

- **`unsafe` block documented**: `GenerateConsoleCtrlEvent` FFI call in
  `perl-dap` now carries a SAFETY comment explaining why the call is sound.
  (#3232)

### Removed

- **Archived 7 dead tree-sitter harness crates** to `archive/crates/`:
  `tree-sitter-perl-rs` (old Pest-based harness), `perl-ts-heredoc-analysis`,
  `perl-ts-statement-tracker`, `perl-ts-logos-lexer`, `perl-ts-heredoc-parser`,
  `perl-ts-partial-ast`, `perl-ts-advanced-parsers`. All workspace references,
  CI exclusion lists, and benchmark function paths updated. (#3244, #3250)

- **Dead stray LICENSE files** in `crates/perl-corpus/`, `crates/perl-lexer/`,
  `crates/perl-parser/`: byte-identical orphan files not referenced by any
  `Cargo.toml` `license-file` field. (#3196)

### Dependencies

- `similar` 2.7.0 → 3.0.0 (#3184) — only consumer is xtask; breaking changes do
  not intersect our usage
- `actions/cache` v4 → v5 (#3181) — Node 24 runtime bump; existing caches remain
  readable
- `eslint` 9.39.4 → 10.2.0 (#3179) — flat config already in use; lint passes clean
- `tokio` 1.50.0 → 1.51.0 (#3180)
- `tree-sitter` 0.26.7 → 0.26.8 (#3182)
- dependencies group with 3 updates (#3183)
- npm group in vscode-extension (#3178)

### Publish pipeline fixes (post-v0.12.2 publish run lessons)

These fixes landed after the initial v0.12.2 publish run and directly address the
partial-publish (108/129) and cascading-failure patterns observed in production:

- **HTTP 429 throttle** (#3307): publish workflow detects crates.io rate-limit
  responses and retries with exponential back-off; the 21 crates that failed in
  the v0.12.2 publish run were blocked by 429s from rapid-fire publish attempts.

- **Publish allowlist extended** (#3296): `perl-workspace-index-monitoring` and
  `perl-test-generators` added to the publish allow-list after they were found
  missing from the v0.12.2 publish set.

- **LICENSE files corrected** (#3304): missing or incorrect `LICENSE` files added
  to 4 publishable crates (`perl-lsp-ai-provider`, `perl-workspace-index`,
  `tree-sitter-perl-rs`, `tree-sitter-perl-c`); crates.io rejects publishes with
  license-file fields pointing to absent files.

- **Duplicate `[package.metadata.docs.rs]` key** (#3315): `tree-sitter-perl-c`
  had two `[package.metadata.docs.rs]` tables in `Cargo.toml`; the duplicate key
  caused `cargo publish` to emit a parse warning and was silently dropped, causing
  docs.rs to build without the intended features. Resolved by merging the two
  tables.

- **Continue-on-failure** (#3316): publish loop now tracks failures in a
  `FAILED_CRATES` array instead of `exit 1` immediately; all topologically-ready
  crates are attempted even when an earlier crate fails. On v0.12.2 run
  24126423987, 19 crates were blocked by a single cascade; on run 24133403944,
  22 crates were blocked. Re-runs safely skip already-published crates via the
  sparse-index check.

- **`tree-sitter-perl-c` polish for first publish** (#3273): vendored sources and
  FFI bindings verified clean for crates.io submission; duplicate metadata resolved
  (#3315 above).

- **docs.rs metadata** (#3299): `[package.metadata.docs.rs]` blocks added or
  corrected for feature-gated crates across the workspace; enables docs.rs to
  build documentation with the correct feature flags set.

- **Publish dry-run gate** (#3301): new CI check runs `cargo publish --dry-run` on
  every PR that modifies a `Cargo.toml`, catching publish-time errors (missing
  files, bad metadata, syntax) before they reach the release pipeline.

### UX fixes (P0 launch blockers)

Five actionability fixes for user-visible error paths that surfaced during the
v0.12.2 publish run and post-publish testing:

- **Actionable binary download errors** (#3306): extension now shows a specific
  message with platform, arch, and download URL when the LSP server binary cannot
  be fetched, instead of a generic network failure.

- **LSP startup error diagnosis** (#3308): `classifyStartupError()` maps stderr
  signatures (GLIBC version mismatch, missing shared library, Exec format error,
  permission denied) to actionable hints and remediation steps; reorders error
  dialog actions so "View Logs" appears before "Reinstall".

- **Workspace root detection warning** (#3309): when the workspace root cannot be
  determined, the server now emits a `window/showMessage` warning with the detected
  state instead of failing silently. Previously users had no indication of why
  features were degraded.

- **Enterprise binary distribution note** (#3310): documentation updated to
  explain that `perllsp` is distributed as a pre-compiled binary via `cargo
  install`, with offline-install guidance for air-gapped enterprise environments.

- **Perl interpreter missing error** (#3312): when `perl` is not found on `$PATH`,
  the extension shows the exact binary name searched and a platform-specific
  installation suggestion, replacing the previous "Perl not found" dead end.

### CI hardening

- **SHA-pinned third-party Actions** (#3294): all `uses:` references to third-party
  GitHub Actions pinned to immutable commit SHAs with version comments (e.g.,
  `uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683 # v4.2.2`).
  Prevents supply-chain attacks via tag mutation.

- **GIT_DIR cleared in hook-tests** (#3318): xtask hook-test scaffold now runs
  with `GIT_DIR` unset, preventing the worktree's inherited `GIT_DIR` value from
  causing git commands inside the temp repo to resolve against the wrong object
  store. Observed contamination: test-repo commits were silently landing in the
  agent worktree.

- **UX regression gate** (#3293): new CI check detects regressions in user-visible
  LSP, DAP, and extension behaviour on every PR that touches those surfaces.
  Backed by the UX test harness framework (#3297).

- **UX test harness framework** (#3297): systematic framework for UX regression
  tests with helpers for LSP, DAP, and extension surface validation.

## [0.12.2] - 2026-04-04

Release notes: [v0.12.2](docs/releases/v0.12.2.md) · [GitHub Release](https://github.com/EffortlessMetrics/perl-lsp/releases/tag/v0.12.2)

`v0.12.2` is the confidence-building release for the 0.12.x series. 89 commits
across 59 PRs spanning new features, performance, testing, distribution, and
documentation. The entire 0.12.x roadmap from v0.12.2 through v0.12.8 milestones
is consolidated into this single release.

The v0.12.2 publish run extended the original GitHub Release with a wave of
quality, distribution, and CI infrastructure work needed to land the full crate
set on crates.io. 108 of 129 crates published successfully in the first attempt;
the remaining 21 (including `tree-sitter-perl-c`, `tree-sitter-perl-rs`,
`perl-parser`, `perl-lsp-rs`, `perllsp`, `perl-dap`) will retry after the HTTP
429 throttle fix lands.

### New Crates (first publish)

- **`tree-sitter-perl-rs`**: v3 ergonomic facade over the native parser stack,
  published alongside `tree-sitter-perl-c` for projects that want tree-sitter
  call ergonomics on top of the Rust-native parser (#3255)
- **`tree-sitter-perl-c`**: conventional C-binding crate for the tree-sitter
  grammar, now publishable on crates.io (#3234)

### Added

- **AI inline completion**: opt-in OpenAI-compatible provider with SSE streaming,
  session management, cancellation, and deterministic fallback when AI is off
  (#3157–#3168)
- **heredoc language injection**: SQL keyword and JSON key detection in heredocs
  with multi-heredoc-per-line support (#3134)
- **type inference in hover**: `TypeInferenceEngine` wired to show inferred types
  on hover (#3150)
- **dead code highlighting**: `DiagnosticTag::Unnecessary` for unreachable code
  (#3092)
- **extract variable/subroutine**: AST-aware code action for extracting
  expressions and blocks (#3090)
- **subroutine inlining**: code action to inline simple subroutines (#3083)
- **POD preview panel**: VS Code command `Perl: Preview POD` (#3131)
- **AST explorer debug panel**: `perl/showAst` custom LSP handler (#3124)
- **Docker image**: `effortlessmetrics/perl-lsp` with perllsp + Perl runtime
  (#3113)
- **DAP cross-platform signals**: continue and interrupt signal handling on
  Linux/macOS/Windows (#3117)
- **context-sensitive quote parsing**: `qw`, `s///`, `tr///` disambiguation in
  complex expressions (#3105)
- **semantic framework coverage**: inheritance and export analysis for Moo/Moose
  patterns (#3103)
- **Linux/macOS installer**: fixed and improved install script (#3122)
- **streaming inline completion controller**: VS Code gating on AI config flags
  (#3161, #3164)

### Performance

- **incremental parsing pipeline**: token caching (#3116), checkpoint recovery
  (#3114), and `Parser::from_tokens` (#3128) complete the incremental path
- **CPAN-scale benchmarks**: 10K files indexed in 672ms, 500K symbol lookup in
  10.6µs (#3121, #3132)
- **large-workspace HashMap optimization**: faster startup for big projects
  (#3112)
- **memory profiling infrastructure**: heap tracking for workspace indexing
  (#3125)
- **completion latency benchmarks**: baseline for regression detection (#3104)

### Fixed

- **DAP attach cleanup**: removed stale mock stub and updated tests (#3135)
- **perlcritic integration**: hardened diagnostic pipeline (#3097)
- **silent error handling**: 23+ silently swallowed errors now emit trace logs
  (#3087, #3151)
- **distribution binary name**: Linux packaging templates and Windows bump
  workflows aligned with `perllsp` (#3106, #3144)
- **Homebrew asset names**: brew-bump workflow aligned (#3120)
- **CI efficiency**: 10 improvements reducing CI minutes (#3156)
- **VS Code type safety**: replaced `any` types with proper TypeScript types
  (#3154)
- **LSP capability snapshots**: regenerated stale snapshots (#3142, #3147)
- **inline completion**: removed duplicate backend type definitions (#3162)
- **pipeline-labels race**: fixed race condition on `reviewed-deep` label (#3100)

### Testing

- **147 DAP tests**: serde, edge cases, and error paths across 4 DAP crates
  (#3152)
- **AI inline completion tests**: integration tests for streaming and
  deterministic paths (#3165, #3168)
- **error builder/lexer mode tests**: missing coverage for error paths (#3091)

### Documentation

- **AI inline completion config reference** (#3167)
- **end-to-end LSP feature development guide** (#3115)
- **large-workspace testing and profiling guide** (#3126)
- **GIF recording guide** for marketing assets (#3130)
- **problem-first README rewrite** (#3119)

### Dependencies

- unified 16 scattered dependency versions via workspace deps (#3153)
- removed 8 unused dependencies across 6 crates (#3146)
- dependabot: insta 1.47.1, proptest, tar, toml 1.1.0, uuid 1.23.0,
  actions/deploy-pages 5, codecov/codecov-action 6

### Quality (publish-run additions)

- **`eprintln!` → `tracing`**: migrated all `eprintln!` / `println!` calls in
  library code to structured `tracing` spans/events; `eprintln!` now banned in
  non-binary crates (#3224, #3245)
- **unwrap burn-down**: Wave 2 (`perl-dap-security`) and Wave 3 (5 crates, 9
  eliminations) converted `unwrap()`/`expect()` calls to `?` and pattern
  matching (#3246 area)
- **error message actionability**: user-visible LSP/DAP error messages rewritten
  to be actionable — what failed, why, what to do next — ahead of v0.13.0
  launch (#3291)
- **crates.io metadata**: `description`, `keywords`, `categories`, `repository`,
  `documentation`, `readme` fields polished across all publishable crates (#3234)
- **docs.rs metadata**: `[package.metadata.docs.rs]` blocks added for
  feature-gated crates (#3234)
- **dead build.rs files removed**: stale `build.rs` files that caused publish
  errors removed from 3 crates (#3217, #3241)
- **stale harness crates archived**: dead tree-sitter harness crates moved to
  `archive/` to reduce workspace noise (#3250, #3244)

### CI (publish-run additions)

- **publish topological sort**: dev-dependencies now included in the publish
  order graph so crates publish in the correct dependency order (#3236, #3242)
- **dev-dependency stripping**: `cargo publish` now strips `[dev-dependencies]`
  before publishing to avoid version conflicts (#3254, #3256)
- **`--allow-dirty` for publish**: added after dev-dep strip leaves the working
  tree dirty (#3300)
- **HTTP 429 throttle handling**: publish workflow detects crates.io rate-limit
  responses and retries with back-off (pending)
- **sparse index wait replaced**: replaced fixed-duration index wait with
  sparse-index polling for faster, more reliable publish verification
- **UX regression gate**: PR check that detects regressions in user-visible LSP,
  DAP, and extension behavior on every PR touching those surfaces (#3293)
- **post-publish smoke test**: automated verification that published crates
  install and the binary starts correctly after each publish run (#3288)
- **version-bump automation centralized**: `just bump-version` now handles
  Cargo.toml, extension package.json, and docs in one command (#3289)
- **`just doctor`**: new workspace health-check recipe that validates the full
  workspace is in a buildable state before starting a session (#3249)
- **`vsce publish` idempotency**: marketplace publish step no longer fails on
  re-run when the version already exists (#3187, #3267)

### UX (publish-run additions)

- **Settings schema polish**: VS Code extension settings schema updated for
  launch-readiness — correct types, descriptions, and defaults (#3278)
- **VS Code Marketplace punch list**: README badges, Open VSX registration,
  extension icon, and feature highlights aligned for marketplace discovery
  (#3284)
- **test de-flake**: `empty_timer_reports_total` race condition fixed (#3278)

## [0.12.1] - 2026-03-31

Release notes: [v0.12.1](docs/releases/v0.12.1.md) · [GitHub Release](https://github.com/EffortlessMetrics/perl-lsp/releases/tag/v0.12.1)

`v0.12.1` is the fix-forward cut after the initial public alpha release. It does
not reopen the wider alpha scope; it closes the release-surface regressions that
slipped into the first `v0.12.0` tag and keeps the install and publish story
aligned.

### Fixed

- restored the top-level README and release-facing docs so the source snapshot
  no longer presents hook-test fixture content as the project front page
- hardened hook-test fixture setup so temporary repos must live outside the real
  checkout and seed commits no longer write placeholder git identities into repo
  config
- fixed local git-hook installation for worktrees and added pre-commit blocking
  for the known placeholder identities used by release and hook tests

### Changed

- workspace, feature-catalog, VS Code extension, and operator release surfaces
  now target `0.12.1`
- status and roadmap docs now treat `v0.12.0` as the latest published GitHub
  release and `v0.12.1` as the active fix-forward cut

## [0.12.0] - 2026-03-30

Release notes: [v0.12.0](docs/releases/v0.12.0.md) · [GitHub Release](https://github.com/EffortlessMetrics/perl-lsp/releases/tag/v0.12.0)

`v0.12.0` is the initial public alpha for the native Rust Perl 5 toolchain. The
headline change is not one feature in isolation; it is that the parser, language
server, debugger, install surface, and release process now line up well enough
for normal editor use.

### Highlights

#### Native editor path

- `perllsp` and `perl-dap` are now treated as first-class native binaries for editor integration and debugging.
- VS Code, manual binary install, and release surfaces were tightened for first-run setup, health checks, and issue reporting.
- `.perl-lsp.toml` gives teams a shared, editor-agnostic project configuration layer.

#### Better day-to-day language tooling

- Completion, hover, diagnostics, formatting, semantic tokens, workspace symbols, code lens, and code actions all received broad hardening.
- Hover and completion coverage expanded for Perl built-ins, special variables, module flows, and workspace-aware suggestions.
- Diagnostic wiring now consistently surfaces parser, project, and optional Perl::Critic signals through the LSP pipeline.

#### Better real-world Perl coverage

- The native recursive-descent parser was hardened against curated common-corpus and CPAN-facing receipts instead of toy examples alone.
- Semantic and workspace layers improved cross-file navigation, rename, inheritance-aware lookups, and framework-aware behavior for Moo and Moose patterns.
- Workspace indexing, cancellation, timeouts, and runtime concurrency all received reliability work aimed at larger real projects.

#### Release and contributor surface

- Release prep, package-manager manifests, docs, validation receipts, and status pages were aligned for the public-alpha launch.
- The workspace continued its crate-boundary cleanup so parser, runtime, LSP, DAP, and release tooling are easier to reason about independently.

### Notable user-facing additions

- project config via `.perl-lsp.toml`
- richer hover coverage for special variables, built-ins, and framework-aware symbols
- broader completion coverage and improved ranking
- native DAP improvements for stepping, variables, and editor integration
- stronger workspace symbol, formatting, code action, and code lens support

### Notable fixes

- parser recovery and disambiguation across real Perl edge cases such as quote operators, slash parsing, prototypes, and framework-heavy code
- deadlock, contention, and stale-state fixes in the LSP runtime and workspace index
- safer handling for empty files, binary files, Windows and macOS path quirks, and shell-launch edge cases
- stale capability drift, unwired command paths, and release-surface documentation mismatches

For the detailed receipts behind this release, see [docs/project/CURRENT_STATUS.md](docs/project/CURRENT_STATUS.md) and [docs/project/status/index.md](docs/project/status/index.md).

## [0.11.0] - 2026-03-12

Release notes: [v0.11.0](docs/releases/v0.11.0.md) · [GitHub Release](https://github.com/EffortlessMetrics/perl-lsp/releases/tag/v0.11.0)

> **Tag provenance correction.** The live `v0.11.0` ref is
> `8dfa68860cdf8fc220b1345d3b943668d1393ad2`; the previously recorded
> `d22ac7346c832db6b92c41d354eb90099f8b5d53` no longer resolves. The current
> tagged predecessor is `v0.9.1`, because `0.10.0` was changelog-only and the
> current `v0.8.5` / `v0.9.1` refs are divergent. Use
> [`v0.9.1...v0.11.0`](https://github.com/EffortlessMetrics/perl-lsp/compare/v0.9.1...v0.11.0).

This release finalizes the 0.11.0 distribution pipeline across GitHub releases,
crates.io, and the VS Code extension so the workspace can ship from a single,
repeatable release flow.

### Added
- **Turnkey Release Orchestration**: A PR-driven release path now covers version
  bumping, changelog generation, tagging, GitHub release creation, crates.io
  publishing, extension publishing, and downstream package manager automation.
- **Topological crates.io Publishing**: Workspace publish automation computes
  dependency order from `cargo metadata` and publishes only the crates in the
  workspace allowlist.
- **Release Guardrails**: Release helper scripts now validate semver inputs and
  align manual operator flows with the automated `0.11.0` release path.

### Changed
- **Workspace Release Alignment**: Workspace packages, extension metadata, and
  release workflows now target `0.11.0`.
- **Release Tooling**: Legacy release helper scripts now delegate to the current
  GitHub workflow-based release flow instead of relying on stale one-off cargo
  publish steps and outdated examples.
- **Operator Documentation in Scripts**: Manual publish and smoke-test helpers
  now accept an explicit version argument and default to the matching `vX.Y.Z`
  release ref when dispatching workflows or validating published artifacts.

### Fixed
- **Stale Release Examples**: Removed hardcoded `0.8.3` release references from
  publish and smoke-test scripts that could misdirect manual release operations.
- **Publish Version Safety**: crates.io publishing now fails early when the
  workflow target version does not match the versions resolved for workspace
  crates scheduled for publication.

## [0.10.0] - 2026-02-28

Release notes: [v0.10.0](docs/releases/v0.10.0.md) (internal milestone — no GitHub Release)

A major release campaign spanning 60+ PRs (#845-#911) focused on build reliability,
security hardening, crates.io publishing readiness, documentation, and code quality.

### Added
- **Document Highlight for Modern Perl**: try/catch parameters, method/sub signatures, and string interpolation (#882, #896).
- **Feature Governance Microcrates**: Extracted feature governance into 9 dedicated crates for modularity (#848).
- **Module Infrastructure Crates**: Content-Length framing and LSP transport hardening (#857).
- **Context-Aware Status Menu**: Perl LSP status menu with workspace-aware states (#646).
- **InlineValues Lifecycle Coverage**: Test coverage for inlineValues support (#729).
- **Tie-Interface Corpus Tests**: New corpus test fixtures for Perl tie interface syntax (#900).
- **Public API Documentation**: Comprehensive rustdoc for `perl-parser` (#904) and leaf crates (#903).
- **Copilot Instructions**: `.github/copilot-instructions.md` for AI-assisted development (#886).
- **Merge-Gate Commit Status**: CI now publishes merge-gate status checks (#880).
- **Benchmark Test Enablement**: Previously-ignored workspace benchmark test enabled with real assertions (#908).

### Changed
- **Version Bump to 0.10.0**: All 80+ workspace crates, documentation, VS Code extension, and feature catalogs updated (77+ files) (#879, #884).
- **crates.io Publishing Readiness**: All crate metadata verified, publish-ignore lists normalized, crate badges added, publish allowlist expanded (#865, #867, #871, #897).
- **VS Code Extension Polish**: Marketplace readiness with packaging fixes, runtime deps, npm lockfile (#863, #866, #869, #906).
- **Documentation Overhaul**: CONTRIBUTING.md polished for public release (#909), README.md and ROADMAP.md updated (#888), FrameworkKind/FrameworkFlags docs (#887), cargo doc warnings resolved (#894).
- **features.toml**: Version bumped to 0.10.0 with 100% LSP coverage maintained (53/53 user-visible, 97/97 protocol).
- **LSP Harness**: Replaced sleep-poll with condvar+drain-bytes pattern for deterministic testing (#846).
- **xtask Gates**: Fail closed for required timeout/error statuses (#868).
- **Unused Dependencies Removed**: cargo-machete sweep across workspace (#895).
- **Debt Ledger Updated**: Refreshed after cleanup campaign (#898).
- **Stale Files Cleaned**: Removed stale tracked files, hardened .gitignore (#889).
- **Semver-Aware Benchmark Sorting**: Correct version comparison for baseline selection (#885).

### Fixed
- **Build**: Resolved 4 compilation errors in the release candidate build (#881).
- **Clippy**: Resolved warnings across all targets (#901).
- **Document Highlight Regressions**: Fixed test regressions from modern syntax support (#896).
- **LSP Error Logging**: Improved error logging in LSP providers (#905).
- **Unresolved Review Comments**: Addressed outstanding comments from PRs #881 and #882 (#892).
- **Version Drift**: Fixed remaining v0.9.x references in satellite files (#884).
- **Checksum Verification**: Hardened verification and stabilized incremental parsing CI (#858).
- **Installer Scripts**: Hardened for security and reliability (#910).
- **Refactoring Test Isolation**: Isolated `cleanup_no_backups` backup root (#864).
- **CI Receipt Parsing**: Aligned receipt parsing and serialized BDD tests (#845).
- **CI BDD Gate**: Added `--locked` flag and timing receipts (#847).
- **CI Docs Deploy**: Skip when GitHub Pages is disabled (#859).
- **Release Workflow**: Asset naming alignment across chain (#890, #902), concurrency groups (#890).
- **Release Tooling**: git-cliff installation fixes (#873, #874, #875), cargo-release installs (#876, #877), PR-driven 0.x.y flow (#872).
- **Publish Workflow**: Dry-run quoting fix (#870), `--no-verify` for dev-dep cycles (#867).

### Security
- **[HIGH] Path Traversal in DAP Launch**: Fixed path traversal vulnerability in debug adapter (#640).
- **[HIGH] Argument Injection in TestRunner**: Fixed argument injection vulnerability (#633).
- **[MEDIUM] Safe Evaluation Bypass**: Fixed bypass for iterator/IO operations (#647).
- **GitHub Actions Hardening**: SHA-pinned all workflow action references (#911).
- **Installer Hardening**: Hardened install scripts for security and reliability (#910).
- **VS Code Extension**: Pinned minimatch to 10.2.3 to remediate CVEs (#861).

### Performance
- **Symbol Extraction**: Optimized regex compilation for faster workspace indexing (#645).
- **Semantic Analyzer**: Eliminated deep cloning of AST nodes in subroutine analysis (#632).
- **Scope Analyzer**: Optimized unused parameter detection, fixed double reporting (#638).

### Infrastructure
- **Nightly CI Stabilization**: Fuzz harness panic hardening, coverage test resilience, clippy cleanup (#860).
- **Release Orchestration**: Turnkey PR-driven 0.x.y release workflow (#872).
- **Release Tool Installs**: Deterministic git-cliff and cargo-release installation (#873-#877).
- **crates.io Dry-Run**: Unblocked dry-run packaging for all workspace crates (#865).
- **Lockfile Maintenance**: Refreshed lockfile for CI deny checks, fuzz lockfile exclusion (#885).

### Dependencies
- `rand` 0.9.2 -> 0.10.0 (#855).
- `serial_test` 3.3.1 -> 3.4.0 (#854).
- `uuid` 1.20.0 -> 1.21.0 (#856).
- `toml` 0.9.12 -> 1.0.3 (#853).
- `aquasecurity/trivy-action` 0.34.0 -> 0.34.1 (#851).
- `@types/node` 25.1.0 -> 25.3.0 (#849).
- `@types/tar` 6.1.13 -> 7.0.87 (#850).
- Additional dependency group updates (#852).

## [0.9.1] - 2026-02-20

Release notes: [v0.9.1](docs/releases/v0.9.1.md) (tag only — no GitHub Release)

> **Tag provenance correction.** The live `v0.9.1` ref is
> `0e52877de7763d8654e0fb6d7afe6a257639e584`; the previously recorded
> `c82a1604987f315868973a4e5804112e031cec92` no longer resolves. The current
> tag is on a divergent line from `v0.8.5`, so no forward comparison from
> `v0.8.5` is claimed.

### Added
- **Initial Public Alpha Release**: Substantially complete feature set for early testing.
- **Enhanced LSP Features**: 99% coverage of LSP 3.18 methods (alpha-validated).
- **Complete Semantic Analyzer**: All NodeKind handlers implemented (Phases 1, 2, 3) with 100% AST node coverage.
- **Debug Adapter Protocol (DAP) Support**: Phase 1 bridge to Perl::LanguageServer.
- **Enhanced LSP Cancellation System**: Thread-safe infrastructure for minimal latency.
- **Advanced Code Actions**: AST-aware refactoring including extraction and import optimization.
- **Security Hardening**: UTF-16 boundary fixes and path traversal prevention.
- **Comprehensive API Documentation**: Infrastructure for documentation enforcement.
- **Optimized Test Suite**: 0.31s full test suite execution via adaptive threading.

### Changed
- **Project Origins Documented**: Origins in Q2 2025, forked July 15, 2025 from `tree-sitter-perl-better`.
- **Stability Roadmap Refined**: Formal Stability Contract (contract-locked APIs) pushed to v0.15.0.
- **MSRV Updated**: Minimum Supported Rust Version bumped to 1.92 (Rust 2024 edition).
- **Parser Architecture**: Native recursive descent parser as the primary implementation.

### Fixed
- **v0.9.1 close-out receipts captured**: Workspace index state-machine transitions and early-exit behavior verified.
- **Security boundary fixes**: Resolved multi-root workspace path traversal issues.

## [0.9.0] - 2026-01-18

Release notes: [v0.9.0](docs/releases/v0.9.0.md) (internal milestone — no tag or GitHub Release)

### Added
- **Semantic Analyzer Phase 1**: 12/12 critical node handlers implemented.
- **LSP textDocument/definition Integration**: Semantic-aware definition resolution.
- **Enhanced Cross-File Navigation**: Dual indexing strategy for improved reference coverage.

### Changed
- **LSP Coverage**: Increased to 82% of trackable features.

## [0.8.8] - 2025-12-01

Release notes: [v0.8.8](docs/releases/v0.8.8.md) (internal milestone — no tag or GitHub Release)

### Added
- **Initial Workspace Configuration Support**.
- **Enhanced Formatting Fallback**: Always-available capabilities with perltidy integration.

---

## Future Milestones

### Next Release
- Enhanced DAP native implementation (Phase 2).
- Semantic depth improvements for Moo/Moose.

### v0.15.0 (Stability Contract Milestone)
- **Formal Stability Contract**: Contract-locked APIs and wire protocol invariants.
- Full protocol compliance audit.
- Multi-release deprecation cycles.

---

## Version Support Policy (Alpha Phase)

During the alpha phase (pre-v0.15.0):
- **Current Alpha (0.x.y)**: Active development and bug fixes.
- **Breaking Changes**: Allowed in minor (0.x) releases.
- **Security**: Critical patches prioritized for the latest alpha version.

---

## Links

For the full cross-channel release history, see [RELEASE_HISTORY.md](RELEASE_HISTORY.md).

<!-- Compare ranges -->
[0.13.2]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.13.1...v0.13.2
[0.13.1]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.13.0-rc1...v0.13.1
[0.12.4]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.12.3...v0.12.4
[0.12.3]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.12.2...v0.12.3
[0.12.2]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.12.1...v0.12.2
[0.12.1]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.12.0...v0.12.1
[0.12.0]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.8.5...v0.11.0
[0.10.0]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.9.1...v0.10.0
[0.9.1]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.8.5...v0.9.1
[0.9.0]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.8.5...v0.9.0
[0.8.8]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.8.5...v0.8.8
[0.13.0-rc1]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.12.4...v0.13.0-rc1
[Unreleased]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.15.2...HEAD
[0.15.2]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.15.1...v0.15.2
[0.15.1]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.15.0...v0.15.1
[0.15.0]: https://github.com/EffortlessMetrics/perl-lsp/compare/v0.14.0...v0.15.0
