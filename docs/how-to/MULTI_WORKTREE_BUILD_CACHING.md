# Multi-Worktree Build Caching

Share dependency downloads while keeping each worktree's Cargo build state
separate. This guide applies to multiple worktrees or clones of `perl-lsp-swarm`
on one machine, including concurrent PR, review and agent work.

## Correctness boundary

Both `CARGO_TARGET_DIR` (final outputs) and `CARGO_BUILD_BUILD_DIR`
(intermediate artifacts and freshness metadata) must belong to the current
worktree. Sharing either mutable build-state namespace can mix different local
sources with the same package name, version and workspace-relative path.

Cargo can mark a sibling library fresh when both worktrees' source mtimes predate
the first build. Rebuilding only the outer executable can then produce the new
Git stamp with old library behavior. A lock prevents concurrent mutation but does
not establish source compatibility. Disabling incremental compilation does not
fix it. This was reproduced on Cargo 1.95 and 1.99 in
[#11650](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11650), matching
[Cargo #12516](https://github.com/rust-lang/cargo/issues/12516).

A passing test build may use a separate feature-set artifact from the ordinary
product binary. Qualify the actual ordinary executable and keep an immutable copy
with its hash before releasing build ownership. Version strings and successful
Cargo exit status alone are insufficient evidence.

A verified main `xtask` executable can target another worktree explicitly:
`/path/to/copied-xtask --worktree-root /path/to/worktree change-set --base HEAD^`.
The target may be a directory inside that worktree. Selection validates the Git
root, workspace repository identity and xtask package, then uses that root as the
process working directory and default task project root. Missing,
non-repository or unrelated workspace targets fail without falling back; inherited
`GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_INDEX_FILE`,
`GIT_OBJECT_DIRECTORY` and `GIT_ALTERNATE_OBJECT_DIRECTORIES` must be unset,
including empty values. Without this option,
existing defaults remain unchanged. Per-command explicit fixture roots and
intentional compiled-source manifest/corpus consumers (including CPAN and legacy
corpus lookup) retain their contracts. The option is global; invoking a copied
runner from another directory alone does not select that worktree. Git path
output must be UTF-8. This selects a runtime task target; it does not make mutable
Cargo build outputs transferable between worktrees.

## Recommended route for agent work

After the root admits host capacity and existing consumers, use the existing
strict route:

```bash
scripts/cargo-admitted check --workspace --all-targets --locked
scripts/cargo-admitted test -p perl-lsp-rs --locked
```

The [Cargo storage admission contract](../agents/CARGO_STORAGE.md) governs this
route. It retains one common-repository/host lease and independently reusable
Cargo home, while assigning both target and build paths to a canonical-worktree
hash under that slot. It preserves toolchain pins, checks every admitted storage
destination and refuses incompatible inherited target/build overrides.

Before building, inspect the smallest exact operation with
`scripts/cargo-admitted --preflight <exact Cargo command>`. For a host below the
default reserve, use the explicit scope-bound `--budget-file` route described in
[Cargo work on a constrained host](CONSTRAINED_CARGO_BUILDS.md). An accepted file
is an operator declaration with `basis_verified: false`, not a measured host
profile. Both preflight and execution evaluate every destination; execution
rechecks inside the lease. Neither reserves bytes, enforces quotas, admits
compiler caches, apportions growth per filesystem, or accounts for independent
repositories and consumers. Do not lower legacy thresholds to make a build fit.

`DEVPLANE` selects the machine storage root; it does not make sibling worktrees'
mutable artifacts compatible. There is no automatic migration or deletion of old
shared outputs. Account for per-worktree artifact growth before admission, and
retain old evidence/resources for separately authorized ownership-aware cleanup.
This route is opt-in; it does not retrofit running commands.

The admitted route supports `build`, `check`, `test`, `run`, `bench` and `doc`.
External commands such as Clippy and nextest require a separately admitted route.
Do not bypass a refusal by silently retrying raw Cargo or the legacy wrapper.

## Plain Cargo and existing integrations

Cargo's unconfigured default uses `<workspace-root>/target` for both outputs and
intermediates. That provides worktree separation when no environment or Cargo
configuration overrides either path. Inspect both effective directories, including
inherited settings, rather than trusting a shell's current directory:

```bash
cargo metadata --no-deps --format-version 1
```

The `target_directory` and `build_directory` fields identify those two destinations
on Cargo 1.95+. A target-dir command-line argument alone does not override an
independently configured build-dir. Rust-analyzer and independent later binary
consumers need their own admitted resource ownership.

Plain Cargo does not provide this repository's host disk admission or conservative
lease. Root orchestration still owns compute capacity and active consumers.
Worktree-private paths address cross-worktree reuse; they do not make backdated
source restoration or arbitrary path reuse content-safe.

## Legacy compatibility route

`scripts/cargo-safe`, `just cached`, and legacy recipes that delegate to that
wrapper remain for caller compatibility. They do **not** establish worktree
isolation or the strict admission guarantees above. The wrapper honors existing
`CARGO_TARGET_DIR`, `CARGO_BUILD_BUILD_DIR` and `DEVPLANE` settings, which can route
divergent worktrees into one mutable target/build namespace.

Its default devplane uses the checkout basename, so different basenames usually
separate state. Matching basenames or an explicitly common `DEVPLANE` can still
collide. Do not infer safety from a wrapper name, two jobs, a flock, or sccache.
The legacy wrapper's `xtask` branch also bypasses its heavy-command flock.

Retain the legacy interface for existing callers; choose the strict admitted
route for new agent work. Existing running commands and retained outputs remain
under their current owner's control.

## Reusable caches and storage

Cargo registry/download and Git dependency caches may remain shared independently
of private target/build state. A compiler cache such as sccache can avoid identical
compilations, but it is reached only when Cargo decides to invoke rustc; it cannot
repair Cargo's false-fresh decision. The strict route requires a separately admitted
compiler-cache configuration and does not automatically enable wrappers.

Do not clean a shared root to switch candidates. `scripts/target-gc.sh` is advisory;
age and a free lock do not prove consumer inactivity or permission to remove data.
Any cleanup must preserve source, unique work, proof artifacts and a restore path.
Cargo's dependency-cache GC does not bound retained target/build output storage.

## Verification and measurement

Run the lightweight storage fixtures, then the opt-in real Cargo behavior test
using an installed toolchain and an admitted small-build budget:

```bash
python3 scripts/tests/test_cargo_admitted_storage.py
CARGO_ADMITTED_REAL_BUILD_TEST=1 python3 scripts/tests/test_cargo_admitted_storage.py
```

The real regression builds a two-crate workspace and its linked worktree with
intentionally different library behavior, both created before the first build.
Alternating A/B/A must return the corresponding behavior with private target and
build paths, while retaining a common lease and shared Cargo home. Its capacity
observation is a test fixture, not proof that an arbitrary host budget is admitted.

Cache-strategy measurements remain with #9178 and the existing build-measurement
programme. Compare equivalent source, toolchain, profile and feature sets, retain
actual behavior and executable hashes, and account for queue time and all storage
volumes. Do not equate a fast false cache hit with useful reuse. The build-timing
receipt's no-flag default cleans first; never invoke it against active or retained
state merely to obtain a timing number.
## Tradeoffs — read before adopting

### Shared-target staleness

One shared `target/` means toolchain, edition, Rust version, or crate-version
moves invalidate artifacts for **every** worktree at once. A workspace-wide
version bump (e.g. the 0.18.0-rc.1 churn) effectively cold-builds the next
command in each worktree's first use after the bump — but only once total,
not once per worktree.

After a toolchain switch (`rustup update`, MSRV bump) or when builds behave
strangely across a version move, clean the shared root once:

```bash
cargo clean --target-dir "$DEVPLANE/target"
```

Between full cleans, reap trees nothing has touched recently — whole stale
`target/` directories, not files inside active ones — with `scripts/target-gc.sh`:

```bash
just target-gc              # dry-run: reports which targets are stale (default 30d)
just target-gc --apply      # delete the stale candidates
bash scripts/target-gc.sh --self-test   # built-in discrimination test
```

The tool never touches lockfiles or the cargo registry, refuses to run while
the devplane build flock is held, and keeps any tree in which even one file was
modified inside the window — so a fresh build's output is immune by
construction. A tree that stays continuously hot accumulates internal orphaned
hash-versioned artifacts that this whole-tree rule will not reclaim; pruning
those would need per-file mtime semantics and remains future work.

### Lock serialization

Concurrent builds against one shared `CARGO_TARGET_DIR` serialize. Cargo's
own package cache lock is per-`CARGO_HOME`; the target dir itself is not safe
for truly parallel full builds. This is why `cargo-safe` wraps heavy commands
in an `flock`: parallel lanes **queue** instead of corrupting each other.

Rules of thumb:

- Parallel lanes (several worktrees building at once): always go through
  `just cached` / `agent-*` / `scripts/cargo-safe`. Never run two plain
  `cargo build`s against the same `CARGO_TARGET_DIR`.
- A single interactive lane: exported variables without the wrapper are
  tolerable for light commands, but the wrapper costs nothing and keeps the
  disk gate.
- Do not point rust-analyzer at the shared target dir while also building in
  terminals — rust-analyzer holds its own long-lived build lock and will
  stall your CLI builds (or vice versa). Give the IDE its own target dir.

### `sccache` vs shared `target/`

These solve different halves of the problem and compose well:

| | Shared `CARGO_TARGET_DIR` | `sccache` |
|---|---|---|
| What is shared | Final artifacts, dep graph, fingerprints | Compiled crate objects only |
| Cross-worktree hit | Immediate (same dir) | Via `SCCACHE_BASEDIRS` covering the worktree parent |
| Disk cost | One tree total | One cache (bounded, default 15G here) + per-worktree metadata |
| Locking needs | Real serialization (flock required) | Safe for concurrent use |
| Failure mode | Stale-after-bump confusion | Cache miss → normal compile |

`cargo-safe` enables both when `sccache` is present: per-invocation safety
from the cache, full reuse from the shared target. If you can only pick one,
pick **sccache with `SCCACHE_BASEDIRS`** — it is concurrency-safe and degrades
to a normal compile on a miss, where a mismanaged shared target dir can
poison every worktree at once.

## Measuring whether it helps

Do not take the strategy on faith — measure it on your box class. One
measurement trap first: `scripts/build-timing-receipt.sh` (i.e. `cargo xtask
build-timing-receipt`) **with no flags runs `cargo clean` before timing** —
with a shared target exported, that deletes the warm artifacts you meant to
measure. A warm-vs-cold comparison that preserves the cache:

```bash
# 1. Cold baseline in a scratch worktree (receipt cleans, then times):
scripts/build-timing-receipt.sh --clean --output artifacts/timing-cold.json

# 2. Warm the devplane once from any worktree:
just cached check --workspace --all-targets --locked

# 3. Warm measurement in a *second* worktree at the same revision — no clean:
scripts/build-timing-receipt.sh --incremental --output artifacts/timing-warm.json
#    or, simplest honest wall-clock: time just cached check --workspace --locked

# 4. Receipt-level delta:
cargo xtask compare-build-timing artifacts/timing-cold.json artifacts/timing-warm.json
```

The cache-strategy measurement programme lives in **#9178**; adoption
evidence for this guide should land there as receipts, not anecdotes.

Expected shape on a build-bound box: first worktree pays the full cold build
(~10m class); every subsequent sibling worktree's first build of the same
revision should be dominated by linking and workspace-member rebuilds, not by
recompiling the shared dependency graph.
