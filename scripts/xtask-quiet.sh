#!/usr/bin/env bash
# scripts/xtask-quiet.sh
#
# Quiet xtask dispatch for agents (#17240).
#
# `cargo xtask` is the alias `run --package xtask --`, and stable cargo
# replays the cached compiler diagnostics of every fresh workspace-local unit
# on each run. This workspace carries the intentional Phase-1 `missing_docs`
# baseline (#4911), so every invocation — including read-only queries like
# `cargo xtask ci doctor --help` — re-emits ~200KB of warning noise on stderr
# before the command runs. `cargo run -q` does not suppress it: cargo gates
# replay only on `unit.show_warnings` (always true for workspace-local
# crates) and on `build.warnings`, which is inert without the unstable
# `-Zwarnings` flag (cargo 0.95.0 `src/cargo/core/compiler/mod.rs`,
# `replay_output_cache`). xtask cannot fix this internally — the noise is
# emitted by its parent `cargo run` before `main` starts.
#
# This wrapper dispatches the same command without the preamble noise:
#
#   1. run `cargo build --package xtask --locked` with stderr captured;
#   2. on build failure, print the captured output verbatim and propagate the
#      exit status — fresh build errors are never hidden;
#   3. on success, discard the captured build noise and exec the freshly
#      built xtask binary directly, so the command's own stdout and stderr
#      are passed through untouched and the exit status is the xtask exit
#      status.
#
# Disclosed tradeoff: stderr of a *successful* build (cargo status lines and
# compiler warnings, replayed or fresh) is dropped. The workspace lint
# enforcement contract is `cargo clippy -D warnings`, not plain-build
# warnings; run `cargo build -p xtask` directly when you need to see them.
#
# Arguments are forwarded verbatim. `XTASK_QUIET_BUILD_FLAGS` (word-split) is
# inserted into the cargo build invocation, e.g.
# `XTASK_QUIET_BUILD_FLAGS="--features parser-tasks"`. The binary is located
# via `cargo metadata`'s `target_directory`, so `CARGO_TARGET_DIR` and
# machine-local `.cargo/config.local.toml` target-dir overrides are honored.
# Requires the default dev profile (`target/debug`), which is what this
# invocation builds; an explicit cross-compilation target (`CARGO_BUILD_TARGET`
# or `--target`) is refused before building, because the dispatch would only
# ever find the default-profile binary (a `build.target` set in a
# `.cargo/config.toml` file is not detected; `cargo config get` is
# nightly-only on stable cargo).
#
# Known limitation: this wrapper performs a raw, un-admitted Cargo build. It
# does not route through `scripts/cargo-admitted` (Cargo storage admission,
# docs/agents/CARGO_STORAGE.md) and is not cleared for hosts that rely on
# per-worktree private target/build paths; the admission route is tracked in
# #17292.

set -euo pipefail

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

# Bind both Cargo invocations to this repository's workspace: invoked by path
# from outside the checkout, Cargo would resolve the caller's manifest (or
# fail to find one) and could build and dispatch an unrelated workspace's
# `xtask`. The exec'd xtask below restores the caller's working directory,
# matching `cargo xtask` semantics.
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
caller_pwd="$PWD"
cd -- "$repo_root"

# Optional extra cargo build flags (e.g. --features parser-tasks), word-split.
# shellcheck disable=SC2086
build_flags="${XTASK_QUIET_BUILD_FLAGS:-}"

# An explicit build target or target-dir override would desynchronize the
# dispatch: a triple places the binary under `target/<triple>/debug`, and a
# CLI `--target-dir` is invisible to the `cargo metadata` lookup below, so
# either way a stale binary could be executed. Refuse instead.
if [ -n "${CARGO_BUILD_TARGET:-}" ] || [[ "$build_flags" == *"--target"* ]]; then
  printf 'xtask-quiet: CARGO_BUILD_TARGET, --target or --target-dir is unsupported; the dispatch expects the default dev-profile binary; run `cargo xtask` instead\n' >&2
  exit 70
fi

build_log="$(mktemp)"
trap 'rm -f "$build_log"' EXIT

set +e
cargo build --package xtask --locked $build_flags 2>"$build_log"
build_status=$?
set -e

if [ "$build_status" -ne 0 ]; then
  cat "$build_log" >&2
  exit "$build_status"
fi

# Locate the freshly built binary. `cargo metadata` resolves the same
# target directory the build above used (env and config included). JSON
# escapes Windows separators as `\\`; normalize to `/`, which every loader
# here accepts. With `pipefail`, a metadata failure must be caught here —
# otherwise `set -e` aborts the assignment without any diagnostic.
target_dir="$(cargo metadata --no-deps --format-version 1 \
  | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')" || target_dir=""
if [ -z "$target_dir" ]; then
  printf 'xtask-quiet: cannot resolve the target directory via cargo metadata; run `cargo xtask` instead\n' >&2
  exit 70
fi
target_dir="${target_dir//\\\\//}"
target_dir="${target_dir//\\//}"

bin=""
if [ -x "${target_dir}/debug/xtask.exe" ]; then
  bin="${target_dir}/debug/xtask.exe"
elif [ -x "${target_dir}/debug/xtask" ]; then
  bin="${target_dir}/debug/xtask"
fi

if [ -z "$bin" ]; then
  printf 'xtask-quiet: built binary not found under %s/debug; run `cargo xtask` instead\n' \
    "$target_dir" >&2
  exit 70
fi

# Remove the captured build log before the exec handoff: `exec` replaces this
# shell without running the EXIT trap, which would leak the temp file into
# TMPDIR on every successful invocation.
rm -f -- "$build_log"
cd -- "$caller_pwd"
exec "$bin" "$@"
