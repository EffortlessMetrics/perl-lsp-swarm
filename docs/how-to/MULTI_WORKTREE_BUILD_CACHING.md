# Multi-Worktree Build Caching

How to make several local worktrees of this repository share compiled work
instead of each one building its own multi-GB `target/` tree.

Audience: anyone running more than one local worktree or clone of
`perl-lsp-swarm` on one machine (agent swarms, stacked PR work, review
worktrees). If you have a single checkout, the default per-worktree layout is
already fine — this guide trades disk and cold-build time for shared-state
discipline you only need at N>1.

## The problem

Every worktree that runs plain `cargo` / `just pr-fast` builds into its own
`<worktree>/target/`:

- a cold full-workspace build takes on the order of **10 minutes** on a
  build-bound box (measured 10m32s on toolchain 1.95.0, #12596);
- each `target/` tree is multiple GB, multiplied by every sibling worktree;
- on Windows the per-worktree hardlink/copy cost makes the duplication worse.

The repository already ships the routing mechanism — `scripts/cargo-safe` —
but plain `cargo` and most `just` recipes bypass it. This guide makes the
shared-cache path the documented default for multi-worktree development.

## The recommended default

Use `scripts/cargo-safe` for build work in every secondary worktree. The
easiest way is the `just cached` passthrough, which forwards any cargo
command:

```bash
just cached check --workspace --all-targets --locked
just cached test -p perl-lsp-rs --locked
just cached clippy -p perl-workspace --all-targets --locked -- -D warnings
```

The `agent-*` recipes (`just agent-check`, `just agent-test`,
`just agent-clippy`, `just agent-pr-fast`) route through the same wrapper and
are equally safe for multi-worktree use.

What `cargo-safe` does (see `scripts/cargo-safe`, 74 lines, worth reading):

- redirects `CARGO_TARGET_DIR`, `CARGO_HOME`, `CARGO_BUILD_BUILD_DIR`, and
  `TMPDIR` to a machine-level devplane at
  `${XDG_CACHE_HOME:-~/.cache}/devplane/<repo-name>/` (override with
  `DEVPLANE=/path`);
- sets `CARGO_INCREMENTAL=0` and a bounded `CARGO_BUILD_JOBS=2`;
- if `sccache` is installed, wraps rustc with it and sets
  `SCCACHE_BASEDIRS` to the worktree **parent directory** — that is the key
  that lets sibling worktrees hit each other's cached compiler output;
- serializes heavy commands (`build|check|test|run|bench|doc|clippy|nextest`)
  through an `flock` on the devplane with a 180s wait
  (`CARGO_LOCK_WAIT` to tune);
- refuses to run when the devplane filesystem is nearly full
  (`MIN_FREE_GB=40`, `MAX_USED_PCT=85`).

One-time setup per machine:

```bash
just devplane-init     # creates the devplane directories
cargo install sccache --locked   # optional but recommended
```

If you prefer not to route every command through the wrapper, export the same
variables once per shell — these are the exact lines `cargo-safe` applies:

```bash
export DEVPLANE="${XDG_CACHE_HOME:-$HOME/.cache}/devplane/$(basename "$(git rev-parse --show-toplevel)")"
export CARGO_TARGET_DIR="$DEVPLANE/target"
export CARGO_HOME="$DEVPLANE/cargo-home"
export CARGO_INCREMENTAL=0
# with sccache installed:
export RUSTC_WRAPPER=sccache
export SCCACHE_DIR="$DEVPLANE/sccache"
export SCCACHE_BASEDIRS="$(dirname "$(git rev-parse --show-toplevel)")"
```

Warning: with the variables exported directly, **you lose the flock** (see
lock serialization below) and the disk gate. Prefer the wrapper for anything
heavier than `cargo check` on one crate.

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

Do not take the strategy on faith — measure it on your box class:

- `scripts/build-timing-receipt.sh` (i.e. `cargo xtask
  build-timing-receipt`) records build-time receipts; compare a cold
  per-worktree build against a second-worktree build with the devplane warm.
- The cache-strategy measurement programme lives in **#9178**; adoption
  evidence for this guide should land there as receipts, not anecdotes.

Expected shape on a build-bound box: first worktree pays the full cold build
(~10m class); every subsequent sibling worktree's first build of the same
revision should be dominated by linking and workspace-member rebuilds, not by
recompiling the shared dependency graph.
