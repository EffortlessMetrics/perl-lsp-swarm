#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-check}"
# Reject wrapper usage errors before probing or launching a Rust toolchain.
if [[ "$#" -gt 1 ]] || [[ "$MODE" != check && "$MODE" != doctor ]]; then
  echo "Usage: $0 [check|doctor]" >&2
  exit 2
fi

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

# Doctor diagnoses the task's selected environment. Do not replace an explicit
# stable/direct toolchain with the repository pin before inspecting it. This
# legacy xtask wrapper can compile; it does not itself admit Cargo work.
if [[ "$MODE" == doctor ]]; then
  cd "$REPO_ROOT"
  export RUSTUP_AUTO_INSTALL=0
  . "$SCRIPT_DIR/lib/cargo-toolchain-guard.sh"
  cargo_toolchain_guard
  exec cargo xtask check-toolchain --doctor
fi

# Default/check retains its existing pin-selection and fallback semantics.
TOOLCHAIN=""
if [ -f "$REPO_ROOT/rust-toolchain.toml" ]; then
  TOOLCHAIN=$(awk -F'"' '/channel/{print $2; exit}' "$REPO_ROOT/rust-toolchain.toml")
fi

RUSTUP_CMD=""
if command -v rustup >/dev/null 2>&1; then
  RUSTUP_CMD="$(command -v rustup)"
elif [ -n "${CARGO_HOME:-}" ] && [ -x "$CARGO_HOME/bin/rustup" ]; then
  RUSTUP_CMD="$CARGO_HOME/bin/rustup"
elif [ -n "${HOME:-}" ] && [ -x "$HOME/.cargo/bin/rustup" ]; then
  RUSTUP_CMD="$HOME/.cargo/bin/rustup"
fi

CARGO_CMD=(cargo)
if [ -n "${TOOLCHAIN:-}" ] && [ -n "$RUSTUP_CMD" ]; then
  CARGO_CMD=("$RUSTUP_CMD" run "$TOOLCHAIN" cargo)
else
  # No rustup (or no pin): the plain PATH cargo is what runs, so the
  # toolchain guard applies — this is exactly the WSL apt-cargo case (#12593).
  . "$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard
fi

cd "$REPO_ROOT"
exec "${CARGO_CMD[@]}" xtask check-toolchain
