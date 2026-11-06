#!/usr/bin/env bash
# scripts/xtask-quiet.sh
#
# Quiet xtask dispatch for agents (#17240, #17292).
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
#   1. run `scripts/cargo-admitted build -p xtask --locked` with stderr
#      captured (the same admitted producer `cargo xtask` uses via
#      `scripts/cargo-admitted run -p xtask --locked -- …`; docs/agents/CARGO_STORAGE.md);
#   2. on build or admission failure, print the captured output verbatim and
#      propagate the exit status — fresh errors are never hidden, and the
#      wrapper never retries raw Cargo after refusal;
#   3. on success, read the admitted `resources.target` from the captured
#      descriptor (not `cargo metadata` or a constructed debug path from an
#      unadmitted target dir), discard the captured build noise, and exec the
#      freshly built xtask binary directly so the command's own stdout and
#      stderr are passed through untouched and the exit status is the xtask
#      exit status.
#
# Disclosed tradeoff: stderr of a *successful* build (cargo status lines,
# compiler warnings, and cargo-admitted descriptor/settlement lines) is
# dropped. The workspace lint enforcement contract is
# `cargo clippy -D warnings`, not plain-build warnings; run
# `scripts/cargo-admitted build -p xtask --locked` directly when you need to
# see them.
#
# Arguments are forwarded verbatim. `XTASK_QUIET_BUILD_FLAGS` (word-split) is
# inserted into the admitted build invocation, e.g.
# `XTASK_QUIET_BUILD_FLAGS="--features parser-tasks"`. Unsupported flags are
# refused here or by cargo-admitted; this wrapper does not retry them as raw Cargo.
# Requires the default dev profile (`<admitted-target>/debug`). Explicit profile
# selection (`-r`, `--release`, or either form of `--profile`, even `--profile dev`)
# and cross-compilation targets (`CARGO_BUILD_TARGET` or `--target`) are refused
# before building, because dispatch would only ever find the default-profile binary.
#
# Remaining #17292 follow-up (not this wrapper): compiler-artifact JSON
# identity, immutable capture/custody, nested-operation reuse, and exact-pin
# qualification (#18430). This path admits the build and consumes the
# descriptor's target directory; it does not claim pinned artifact identity.

set -euo pipefail

# Resolve this script's directory before changing cwd: a relative
# BASH_SOURCE such as `xtask-quiet.sh` (invoked from `scripts/`) would
# otherwise look for cargo-admitted in the workspace root after the cd.
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "${script_dir}/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

# Bind both Cargo invocations to this repository's workspace: invoked by path
# from outside the checkout, Cargo would resolve the caller's manifest (or
# fail to find one) and could build and dispatch an unrelated workspace's
# `xtask`. The exec'd xtask below restores the caller's working directory,
# matching `cargo xtask` semantics.
repo_root="$(cd -- "${script_dir}/.." && pwd)"
caller_pwd="$PWD"
cd -- "$repo_root"

# Optional extra cargo build flags (e.g. --features parser-tasks), word-split.
# shellcheck disable=SC2086
build_flags="${XTASK_QUIET_BUILD_FLAGS:-}"

# An explicit build target would desynchronize the dispatch: a triple places
# the binary under `target/<triple>/debug`, so a stale default-profile binary
# could be executed. Refuse instead.
if [ -n "${CARGO_BUILD_TARGET:-}" ] || [[ "$build_flags" == *"--target"* ]]; then
  printf 'xtask-quiet: CARGO_BUILD_TARGET, --target or --target-dir is unsupported; the dispatch expects the default dev-profile binary; run `cargo xtask` instead\n' >&2
  exit 70
fi

# A selected profile can build outside debug and leave a stale debug binary to
# dispatch. Match the same word-split tokens passed to the producer so flag text
# inside a feature or output-format value does not become a profile selector.
# shellcheck disable=SC2086
for build_flag in $build_flags; do
  case "$build_flag" in
    -r|--release|--profile|--profile=*)
      printf 'xtask-quiet: explicit profile selection in XTASK_QUIET_BUILD_FLAGS is unsupported; the dispatch expects the default dev-profile binary; run `cargo xtask` instead\n' >&2
      exit 70
      ;;
  esac
done

# Production always uses this repository's admitted producer. Tests may bind
# XTASK_QUIET_CARGO_ADMITTED to a fake that preserves the same argv/descriptor
# contract; that override is not a raw-Cargo fallback.
admitted="${XTASK_QUIET_CARGO_ADMITTED:-${script_dir}/cargo-admitted}"

build_log="$(mktemp)"
trap 'rm -f "$build_log"' EXIT

set +e
# shellcheck disable=SC2086
"$admitted" build -p xtask --locked $build_flags 2>"$build_log"
build_status=$?
set -e

if [ "$build_status" -ne 0 ]; then
  cat "$build_log" >&2
  exit "$build_status"
fi

# Locate the freshly built binary from the admitted resource descriptor.
# cargo-admitted binds CARGO_TARGET_DIR only for the child; after it returns
# the parent environment is unchanged, so `cargo metadata` would report an
# unadmitted target directory and could select a stale sibling binary.
if command -v python3 >/dev/null 2>&1; then
  python=python3
elif command -v python >/dev/null 2>&1; then
  python=python
else
  printf 'xtask-quiet: Python 3 is required to read the cargo-admitted resource descriptor; run `cargo xtask` instead\n' >&2
  exit 70
fi

set +e
target_dir="$("$python" - "$build_log" <<'PY'
import json
import sys

path = sys.argv[1]
targets = []
try:
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        for raw in handle:
            line = raw.lstrip("\ufeff").rstrip("\r\n")
            prefix = "cargo-admitted resources: "
            if not line.startswith(prefix):
                continue
            payload = json.loads(line[len(prefix):])
            target = payload.get("resources", {}).get("target")
            if not isinstance(target, str) or not target.strip():
                sys.exit(2)
            targets.append(target)
except (OSError, json.JSONDecodeError, TypeError, AttributeError):
    sys.exit(2)
if len(targets) != 1:
    sys.exit(1)
sys.stdout.write(targets[0])
PY
)"
parse_status=$?
set -e

if [ "$parse_status" -ne 0 ] || [ -z "${target_dir:-}" ]; then
  printf 'xtask-quiet: cannot resolve the admitted target directory from cargo-admitted resources; run `cargo xtask` instead\n' >&2
  exit 70
fi

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
