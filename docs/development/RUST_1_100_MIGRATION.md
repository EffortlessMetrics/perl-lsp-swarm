# Rust 1.100 migration

Owner: [#9864](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9864).
Selection: [#11234](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11234).

Rust 1.100.0 became stable on November 12, 2026. The selected pre-release product
compiler is exactly `1.100.0`, and the source-build minimum is `1.100`. The
[Clippy policy](../CLIPPY_POLICY.md#toolchain-currentness) requires Cargo MSRV,
toolchain, Clippy configuration and ledger, and gate-policy MSRV to agree.
This migration changes those authorities together while retaining the Rust
edition and workspace package versions. Its single dependency adjustment is the
targeted `eyre` 0.6.12 to 0.6.14 compatibility repair described below; this is not
a broad dependency refresh. Previously released binaries do not acquire a Rust
installation requirement from this source-build change.

The selected version is a target for qualification. A changed pin, successful
setup, or one platform's green result does not establish release readiness.
Current proof and remaining platform obligations belong on #9864 and its PR.

## What the upgrade offers

The five intervening releases provide compiler and library changes without
requiring an edition migration. These are available capabilities, not measured
perl-lsp performance gains:

| Release | Relevant capability | Intended use and boundary |
| --- | --- | --- |
| [1.96](https://blog.rust-lang.org/2026/05/28/Rust-1.96.0/) | New `core::range` types and `assert_matches!` | Possible clearer range ownership and test diagnostics. Ordinary range syntax is unchanged; no mechanical range rewrite is part of this migration. |
| [1.97](https://blog.rust-lang.org/2026/07/09/Rust-1.97.0/) | Cargo warning control and surfaced linker diagnostics | Future warning-policy work can avoid changing compiler flags merely to change warning treatment. Existing strict lint levels remain intact. |
| [1.98](https://blog.rust-lang.org/2026/08/20/Rust-1.98.0/) | Integer formatting buffers and string subslice/range helpers | Candidates for measured allocation reductions in output and text processing. UTF-16 byte decoding APIs do not replace LSP UTF-16 position mapping. |
| [1.99](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/) | Owned lossy UTF-8 conversion and `VecDeque::retain_back` | Potential improvements at subprocess-output and bounded-queue seams after semantic and allocation measurements. |
| [1.100](https://blog.rust-lang.org/2026/11/12/Rust-1.100.0/) | Allocator APIs for `Vec`/`Box`, `DropGuard`, and stabilized never type | Optional ownership, cleanup, and memory-management work after compiler parity; none is necessary just to adopt the toolchain. |
| [Cargo 1.100](https://doc.rust-lang.org/cargo/CHANGELOG.html) | Native unused-dependency diagnostics, dependency cooldowns, and a new intermediate artifact layout | Useful dependency and build tooling. Do not automatically remove flagged dependencies or enable a new resolution policy inside the pin change. |

The new Cargo layout reduces certain cache collisions. It does not establish
automatic build-artifact garbage collection, cross-workspace deduplication, or
fine-grained locking. Retain the repository's private target/build paths and
[storage admission](../agents/CARGO_STORAGE.md). No build-time, runtime, memory,
or disk-size improvement is claimed without comparable measurements.

## Compatibility work

The [exact Rust release notes](https://github.com/rust-lang/rust/blob/1.100.0/RELEASES.md)
are the source for language and library compatibility changes. The practical
qualification seams are:

- **Build artifacts:** Cargo 1.100 places fingerprints under
  `build/<package>/<hash>/fingerprint` within the selected build/profile root.
  Tools that assume `.fingerprint` can silently miss evidence. Prefer Cargo JSON
  messages for supported artifact discovery; version-sensitive measurement must
  recognize the layout or refuse unsupported input. See Cargo's
  [build-cache contract](https://doc.rust-lang.org/cargo/reference/build-cache.html).
- **Test executable consumers:** the DAP exact-session matrix obtains its
  default product binary from Cargo's `CARGO_BIN_EXE_perl-dap` binding instead
  of climbing the test executable's parent directories; an explicit
  `PERL_DAP_TEST_BINARY` still takes precedence and must name a file. Status
  inventory attributes Cargo's announced hashed test-executable basename
  independently of intermediate directories, retaining old, new, custom, and
  Windows paths. Unknown `Running unittests` headers reset attribution so their
  tests remain unattributed instead of being credited to the preceding crate.
- **Driver identity:** the admitted native Clippy route binds the exact installed
  Cargo, rustc, rustdoc, cargo-clippy, clippy-driver and compiler runtimes. Run its
  real fail/clean, named-target and keep-going controls on 1.100 before calling
  the new driver qualified. A changed admission literal is not that proof.
- **Language behavior:** never-type fallback now uses `!` in all editions and
  `Infallible` is an alias for `!`. More precise closure capture, trait-object
  checking, and assertion temporary lifetimes can expose source assumptions.
  Keep new diagnostics visible and repair their owning source separately.
- **Lint and documentation behavior:** new diagnostics and the
  `invalid_markdown_table` rustdoc lint require actual lint/doc runs. The
  `manual_pop_if` and `manual_clear` ledger rows become available at the new
  floor, so they move to owned `deferred_due` entries under #1803 and #9913.
  Existing lint levels remain unchanged.
- **Error-reporting macros:** Rust's new
  `semicolon_in_expressions_from_non_local_macros` warning exposes the trailing
  semicolon in `eyre` 0.6.12's `bail!` macro at expression-position call sites.
  The existing strict Clippy policy promotes that warning to an error. Update
  only the locked `eyre` package to 0.6.14, which contains the
  [upstream macro fix](https://github.com/eyre-rs/eyre/pull/294), and retain
  `color-eyre` 0.6.5. This fixes the shared macro implementation while preserving
  application error paths and the existing warning policy. Requalify strict
  linting and error-path tests after the lockfile change.
- **Atomic counters:** replace six uses of the deprecated integer-atomic
  `fetch_update` name with `try_update` in workspace monitoring, runtime
  generations, resolve-token sequences, document revisions, and allocation
  tracking. The old method forwards to
  [`try_update`](https://doc.rust-lang.org/1.100.0/std/sync/atomic/struct.AtomicU64.html#method.try_update).
  Preserve success/failure orderings, update closures, exhaustion behavior, and
  result handling; the existing boundary and concurrency tests remain the proof.
- **Feature resolution and CI:** Cargo 1.99 respects inherited
  `default-features = false` in edition-2024 workspaces and disables incremental
  compilation by default under `CI`. Test the actual feature graph. This repo
  already explicitly disables incremental compilation; retain that setting.
- **Text semantics:** standard-library Unicode tables now use Unicode 18.
  Character-classification, identifier, case and escaping tests must retain
  their Perl-specific expected behavior rather than accepting new snapshots
  solely because rustc changed.
- **Debugging and coverage:** v0 symbol mangling became the default in Rust 1.97.
  Check debugger/profiler consumers. The compiler LLVM baseline moves from
  [22.1.2](https://github.com/rust-lang/llvm-project/blob/1cb4e3833c1919c2e6fb579a23ac0e2b22587b7e/cmake/Modules/LLVMVersion.cmake)
  to [23.1.5](https://github.com/rust-lang/llvm-project/blob/0a5579eb649d3634a9c30ac873300cc2944fa070/cmake/Modules/LLVMVersion.cmake).
  Use compiler-matched `llvm-tools-preview` and fresh coverage/PGO artifacts;
  [raw coverage processing may require the exact compiler LLVM](https://doc.rust-lang.org/rustc/instrument-coverage.html).
- **Platforms and packaging:** hosted installs, Docker builders, Nix and source
  installers must select the new compiler. Self-hosted `em-ci-rust:1.100` images
  require actual provisioning; preserve the existing explicit hosted fallback
  when they are unavailable. A declared image name does not prove availability.

## Required evidence

Record the source identity, exact command, target/features/profile, tool versions,
exit result and limitations for each row. Use the
[admitted Cargo route](../agents/CARGO_STORAGE.md) for compilation and retain
private build paths. Do not relabel old receipts as 1.100 evidence.

| Subject | Acceptance |
| --- | --- |
| Product authorities and Cargo metadata | Every active workspace package reports floor 1.100; all product authorities agree on 1.100/1.100.0. |
| Same-source compiler comparison | Old and new toolchain observations identify actual compatibility changes without dependency or source churn. |
| Toolchain guards and product doctor | 1.95 and 1.99 are stale; 1.100 is accepted; numeric minor comparison survives the 99/100 boundary and provenance checks remain effective. |
| Formatter receipts | Actual 1.100 formatting passes; an old 1.95 receipt is rejected even when its selected compiler agrees with that receipt. |
| Native Clippy admission | Installed-driver dispatch and real failure/clean, target-selection, and keep-going controls pass on the exact new tools. |
| Product proof | Required formatting, workspace/feature checks, affected tests, strict Clippy, docs, parser/LSP/DAP and corpus subjects pass or have explicit owning repair/blocker dispositions. |
| Platform and package consumers | Required Linux, Windows and macOS, source installation, Docker/Nix, package and release consumers are independently qualified; missing rows remain `NOT_PROVEN`. |
| Integration | Independent review, current required checks, mergeability and repository protection permit landing. |

Useful lightweight controls include the toolchain-guard shell suites and Python
formatter-receipt, workflow, and Cargo-admission contracts under `scripts/tests`
and `scripts/ci`. Their fixture results do not replace compiler or platform runs.
Preserve historical benchmark and release receipts under their original identities.

### Initial Linux qualification

The initial qualification on 2026-11-13 UTC used source base
`db923e29241dbe0c298b618bb3b95201b6cb8416`, separate private build directories,
and the installed stable x86_64 Linux compilers. The observations below are
bounded results; current integration evidence remains on #9864 and its PR.

| Subject | Observed result and boundary |
| --- | --- |
| Original source with Rust 1.95 | `check --workspace --all-targets --profile agent --locked` passed with default features. Existing warnings remain visible. |
| Initial synchronized candidate with Rust 1.100 | The same workspace/all-target request passed with default features before the targeted `eyre` update. This is compilation evidence, not execution of all tests or all-feature qualification. |
| Formatting on original source | `cargo fmt --all -- --check` passed with both 1.95 and 1.100 without source formatting changes. |
| Candidate package metadata | All 48 active members report Rust 1.100; all retain edition 2024. |
| Locked Nix evaluation | The default shell, CI shell, and `perllsp` package's Cargo/rustc native inputs all resolve `rust-default-1.100.0` under `--no-update-lock-file --no-write-lock-file`. This does not establish a Nix product build. |
| Native Clippy admission | The complete dependency-free 1.100 fixture passed its fail/clean, independent keep-going failures, blocked-prerequisite, named keep-going, and named-target fail/clean controls. Every invocation settled and released its owned lease; an invalid unselected target remained excluded. This qualifies the native Linux driver route, not product lint cleanliness or native Windows. |
| Strict lint qualification | Pending execution after the targeted `eyre` repair. A successful ordinary check does not establish that the strict Clippy gate passes. |

## Follow-on work and rollback

Cargo-native minimum dependency age remains a separate policy change under the
[dependency management guide](../how-to/DEPENDENCY_MANAGEMENT.md#release-age-boundary).
New allocator, formatting, cleanup and collection APIs require bounded semantic
review and before/after measurement. Native unused-dependency reports need review
for build scripts, proc macros, target features and intentional side effects.

If required compatibility proof exposes an unresolved regression, revert the
synchronized toolchain candidate and retain the failing evidence. Do not hide it
with broad lint suppression, removed targets, refreshed baselines, or rewritten
historical receipts. This migration does not create a tag, publish a release,
change branch protection, or declare previously untested platforms supported.
