#!/usr/bin/env bash
# scripts/lib/cargo-toolchain-guard.sh
#
# Toolchain guard for repo bash entrypoints (issue #12593).
#
# Sourced — not executed — by cargo-invoking bash entrypoints
# (scripts/cargo-safe, scripts/*.sh, .github/run_all_tests.sh) before any
# build work. It resolves the cargo the entrypoint is about to use, verifies
# its version against the workspace `rust-version`, and refuses with a typed,
# remediation-bearing message when the resolved cargo is too old.
#
# Why this exists: on Windows, `bash` frequently resolves to WSL bash, and a
# WSL *non-login* shell (exactly how `#!/usr/bin/env bash` shebangs run)
# resolves /usr/bin/cargo — Ubuntu's apt cargo 1.75.0, not a rustup shim. It
# ignores rust-toolchain.toml, predates edition-2024 stabilization, and fails
# manifest parsing with `feature 'edition2024' is required`, which presents as
# "the workspace manifest is broken" and invites manifest edition downgrades.
# See scripts/tests/test-cargo-toolchain-guard.sh for the decision-function
# unit tests and scripts/tests/test-cargo-safe-toolchain-guard.sh for the
# entrypoint integration test.
#
# Cost: one local `cargo --version` probe. No network, no toolchain installs.
#
# Guard contract:
#   - refuse (exit 78, EX_CONFIG) when no cargo is on PATH, when
#     `cargo --version` fails or cannot be parsed, or when the resolved
#     version is below the workspace rust-version;
#   - pass silently when the resolved cargo is a rustup shim at or above the
#     requirement;
#   - pass with a stderr note when a non-shim cargo still satisfies the
#     requirement but diverges from the rust-toolchain.toml pin (non-shim
#     cargo ignores the pin; that context drift is worth reporting).
#
# The decision helpers take explicit arguments so tests can exercise them
# without touching the real toolchain. Call them in a condition position
# (if/while/`!`) when the sourcing script runs under `set -e`, because a
# false verdict is a normal return, not an error.

# Exit code for a typed toolchain-configuration refusal. Distinct from
# scripts/cargo-safe's disk-space DENY (75) and from cargo's own exits.
CARGO_GUARD_EXIT_CODE=78

# Repo root derived from this library's own location (scripts/lib/..).
CARGO_GUARD_REPO_ROOT="$(cd -- "${BASH_SOURCE[0]%/*}/../.." && pwd)"

# Print the workspace-required minimum cargo version: `rust-version` from the
# workspace Cargo.toml, falling back to the rust-toolchain.toml channel, then
# to the last-known pin. Usage: cargo_guard_required_version [repo_root]
cargo_guard_required_version() {
  local root="${1:-$CARGO_GUARD_REPO_ROOT}"
  local version
  if [ -f "${root}/Cargo.toml" ]; then
    version="$(awk -F'"' '/^rust-version[[:space:]]*=/{print $2; exit}' "${root}/Cargo.toml")"
    if [ -n "$version" ]; then
      printf '%s\n' "$version"
      return 0
    fi
  fi
  cargo_guard_pin_version "$root"
}

# Print the rust-toolchain.toml channel pin ("" when absent).
# Usage: cargo_guard_pin_version [repo_root]
cargo_guard_pin_version() {
  local root="${1:-$CARGO_GUARD_REPO_ROOT}"
  if [ -f "${root}/rust-toolchain.toml" ]; then
    awk -F'"' '/^[[:space:]]*channel[[:space:]]*=/{print $2; exit}' "${root}/rust-toolchain.toml"
  fi
}

# Extract the dotted version (X.Y[.Z]) from `cargo --version` output.
# Scans every line, because a rustup shim first downloading the pinned
# toolchain prints progress lines (`info: syncing channel updates ...`)
# before the real `cargo X.Y.Z ...` line. Prints nothing when no version
# token is present. Pre-release and build suffixes (-nightly, +build) are
# dropped: 1.96.0-nightly compares as 1.96.0. Usage: cargo_guard_parse_version <output>
cargo_guard_parse_version() {
  printf '%s\n' "$1" | awk '
    {
      for (i = 1; i <= NF; i++) {
        if ($i ~ /^[0-9]+\.[0-9]+(\.[0-9]+)?([-+][0-9A-Za-z.-]+)?$/) {
          sub(/[-+][0-9A-Za-z.-]+$/, "", $i)
          print $i
          exit
        }
      }
    }'
}

# Succeed when version `actual` >= version `required` (numeric major.minor
# comparison; missing components are zero). Usage: cargo_guard_version_ge <actual> <required>
cargo_guard_version_ge() {
  local actual="$1" required="$2"
  local a_major a_minor a_patch r_major r_minor r_patch
  if [[ ! "$actual" =~ ^[0-9]+\.[0-9]+([.][0-9]+)?$ ||
        ! "$required" =~ ^[0-9]+\.[0-9]+([.][0-9]+)?$ ]]; then
    return 1
  fi
  IFS='.' read -r a_major a_minor a_patch <<< "$actual"
  IFS='.' read -r r_major r_minor r_patch <<< "$required"
  a_major="${a_major:-0}" a_minor="${a_minor:-0}" a_patch="${a_patch:-0}"
  r_major="${r_major:-0}" r_minor="${r_minor:-0}" r_patch="${r_patch:-0}"
  if [ "$a_major" -ne "$r_major" ]; then
    [ "$a_major" -gt "$r_major" ]
    return
  fi
  if [ "$a_minor" -ne "$r_minor" ]; then
    [ "$a_minor" -gt "$r_minor" ]
    return
  fi
  [ "$a_patch" -ge "$r_patch" ]
}

# Normalize Windows separators to forward slashes, then succeed when the path
# looks rustup-managed: a ~/.cargo/bin shim (cargo or cargo.exe, any home,
# including /mnt/c/... views) or a rustup-installed toolchain binary.
# Usage: cargo_guard_is_rustup_shim <cargo-path>
cargo_guard_is_rustup_shim() {
  local path="${1//\\//}"
  case "$path" in
    */.cargo/bin/cargo|*/.cargo/bin/cargo.exe|*/.rustup/toolchains/*) return 0 ;;
    *) return 1 ;;
  esac
}

# Succeed when the current shell looks like WSL bash: WSL_DISTRO_NAME in the
# environment (set by WSL for both login and non-login shells) or a Microsoft
# kernel marker in /proc/version.
cargo_guard_detect_wsl() {
  if [ -n "${WSL_DISTRO_NAME:-}" ]; then
    return 0
  fi
  if [ -r /proc/version ] && grep -qi microsoft /proc/version 2>/dev/null; then
    return 0
  fi
  return 1
}

# Print the typed refusal for a too-old resolved cargo. Pure function of its
# arguments so tests can assert the exact contents.
# Usage: cargo_guard_print_refusal <resolved-path> <actual> <required> <pin> <wsl:0|1> [wsl-detail]
cargo_guard_print_refusal() {
  local resolved="$1" actual="$2" required="$3" pin="$4" wsl="$5" detail="${6:-}"
  printf 'cargo-toolchain-guard: REFUSED: the cargo this entrypoint resolved is older than this workspace Rust requirement.\n' >&2
  printf '  resolved cargo : %s (cargo %s)\n' "$resolved" "$actual" >&2
  printf '  workspace needs: rust-version %s (Cargo.toml)' "$required" >&2
  if [ -n "$pin" ]; then
    printf '; rust-toolchain.toml pins %s, which only rustup shims honor' "$pin" >&2
  fi
  printf '\n' >&2
  if cargo_guard_version_ge "$actual" "1.85"; then
    printf '  why: cargo %s can parse edition-2024 manifests, but this workspace pins rust-version %s — the refusal is workspace toolchain policy, not a manifest defect. Do not downgrade the manifest.\n' "$actual" "$required" >&2
  else
    printf '  why: cargo %s cannot parse edition-2024 manifests, so builds fail with "feature '\''edition2024'\'' is required" -- a toolchain-selection problem, not a broken manifest. Do not downgrade the manifest.\n' "$actual" >&2
  fi
  if [ "$wsl" = "1" ]; then
    printf 'WSL detected%s: non-login WSL bash (how shebang entrypoints run) resolves /usr/bin/cargo, the Ubuntu apt cargo, which is not a rustup shim and ignores rust-toolchain.toml.\n' "${detail:+ (${detail})}" >&2
    printf '  fix: run this entrypoint from the Windows toolchain (Git Bash or pwsh, where the rustup shim in ~/.cargo/bin resolves first), or install rustup inside WSL (https://rustup.rs) and make sure ~/.cargo/bin precedes /usr/bin in PATH for non-login shells.\n' >&2
  else
    printf '  fix: use a rustup-managed toolchain >= %s (rustup install %s) or fix PATH ordering so the rustup shim in ~/.cargo/bin resolves before any system cargo.\n' "$required" "${pin:-$required}" >&2
  fi
}

# Main guard. Resolve cargo the way the entrypoint's upcoming `cargo ...`
# would, probe it locally, decide, and on refusal print the typed message and
# exit CARGO_GUARD_EXIT_CODE. Returns 0 on pass (silent, or with a stderr
# note for a non-shim cargo that diverges from the pin).
cargo_toolchain_guard() {
  local required pin resolved probe actual
  required="$(cargo_guard_required_version)"
  required="${required:-1.95}"
  pin="$(cargo_guard_pin_version)"

  resolved="$(command -v cargo 2>/dev/null || true)"
  if [ -z "$resolved" ]; then
    printf 'cargo-toolchain-guard: REFUSED: no cargo found on PATH; this entrypoint requires a Rust toolchain >= %s (rust-toolchain.toml pin: %s).\n' "$required" "${pin:-none}" >&2
    printf '  fix: install rustup (https://rustup.rs) so the shim in ~/.cargo/bin resolves first, then retry.\n' >&2
    exit "$CARGO_GUARD_EXIT_CODE"
  fi

  # Probe from the repository root, not the caller's directory: a rustup shim
  # selects its toolchain from the nearest rust-toolchain.toml relative to the
  # current directory, so a wrapper invoked from inside another pinned Rust
  # project would otherwise be judged against the caller's toolchain rather
  # than the one the entrypoint will actually use after it enters the repo.
  if ! probe="$(cd "$CARGO_GUARD_REPO_ROOT" 2>/dev/null && "$resolved" --version 2>&1)"; then
    printf 'cargo-toolchain-guard: REFUSED: `%s --version` failed at %s: %s\n' "cargo" "$resolved" "${probe:-<no output>}" >&2
    printf '  fix: repair or remove this cargo from PATH; the workspace needs >= %s.\n' "$required" >&2
    exit "$CARGO_GUARD_EXIT_CODE"
  fi
  actual="$(cargo_guard_parse_version "$probe")"
  if [ -z "$actual" ]; then
    printf 'cargo-toolchain-guard: REFUSED: could not read a cargo version from `%s --version` at %s (got: "%s").\n' "cargo" "$resolved" "${probe%%$'\n'*}" >&2
    printf '  fix: point PATH at a real rustup-managed cargo >= %s and retry.\n' "$required" >&2
    exit "$CARGO_GUARD_EXIT_CODE"
  fi

  if ! cargo_guard_version_ge "$actual" "$required"; then
    local wsl=0 detail=""
    if cargo_guard_detect_wsl; then
      wsl=1
      if [ -n "${WSL_DISTRO_NAME:-}" ]; then
        detail="WSL_DISTRO_NAME=${WSL_DISTRO_NAME}"
      else
        detail="Microsoft kernel in /proc/version"
      fi
    fi
    cargo_guard_print_refusal "$resolved" "$actual" "$required" "$pin" "$wsl" "$detail"
    exit "$CARGO_GUARD_EXIT_CODE"
  fi

  if ! cargo_guard_is_rustup_shim "$resolved"; then
    # Report only when the non-shim cargo is not exactly at the pin: an
    # unrelated-but-equal version behaves like the pin for our purposes.
    if [ -z "$pin" ] || [ "$actual" != "$pin" ]; then
      printf 'cargo-toolchain-guard: note: %s is not a rustup shim, so the rust-toolchain.toml pin (%s) is not in effect; resolved cargo %s satisfies rust-version %s -- continuing.\n' \
        "$resolved" "${pin:-none}" "$actual" "$required" >&2
    fi
  fi

  return 0
}
