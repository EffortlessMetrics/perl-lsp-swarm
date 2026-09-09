#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
LIB="${REPO_ROOT}/scripts/lib/cargo-toolchain-guard.sh"

PASS=0
FAIL=0
TMPDIR_BASE=""

cleanup() {
  if [[ -n "${TMPDIR_BASE:-}" && -d "${TMPDIR_BASE}" ]]; then
    rm -rf "${TMPDIR_BASE}"
  fi
}
trap cleanup EXIT

pass() {
  printf 'PASS %s\n' "$1"
  PASS=$((PASS + 1))
}

fail() {
  printf 'FAIL %s\n' "$1"
  FAIL=$((FAIL + 1))
}

expect_eq() {
  local label="$1" expected="$2" actual="$3"
  if [[ "$expected" == "$actual" ]]; then
    pass "$label"
  else
    fail "$label (expected '${expected}', got '${actual}')"
  fi
}

expect_true() {
  local label="$1"
  shift
  if "$@"; then
    pass "$label"
  else
    fail "$label (expected true)"
  fi
}

expect_false() {
  local label="$1"
  shift
  if "$@"; then
    fail "$label (expected false)"
  else
    pass "$label"
  fi
}

assert_contains() {
  local label="$1" needle="$2" haystack_file="$3"
  if grep -Fq -- "$needle" "$haystack_file"; then
    pass "$label"
  else
    fail "$label (missing: $needle)"
  fi
}

echo "=== cargo-toolchain-guard library test suite ==="
echo ""

if [[ ! -f "$LIB" ]]; then
  echo "ERROR: guard library not found at ${LIB}"
  exit 1
fi

# The library is designed to be sourced; its helpers take explicit arguments.
# shellcheck source=../lib/cargo-toolchain-guard.sh
. "$LIB"

# ── version parsing ───────────────────────────────────────────────────────────

expect_eq "parse stable release" "1.75.0" \
  "$(cargo_guard_parse_version 'cargo 1.75.0 (d6df253b1 2023-11-01)')"
expect_eq "parse pinned release" "1.95.0" \
  "$(cargo_guard_parse_version 'cargo 1.95.0 (f2d3ce0bd 2026-03-21)')"
expect_eq "parse nightly strips prerelease" "1.96.0" \
  "$(cargo_guard_parse_version 'cargo 1.96.0-nightly (0abc123 2026-04-01)')"
expect_eq "parse beta strips prerelease" "1.85.0" \
  "$(cargo_guard_parse_version 'cargo 1.85.0-beta.1 (hash 2024-12-01)')"
expect_eq "parse two-component version" "1.95" \
  "$(cargo_guard_parse_version 'cargo 1.95')"
expect_eq "parse skips rustup sync progress lines" "1.95.0" \
  "$(cargo_guard_parse_version 'info: syncing channel updates for 1.95.0-x86_64-unknown-linux-gnu
cargo 1.95.0 (f2d3ce0bd 2026-03-21)')"
expect_eq "parse garbage yields empty" "" \
  "$(cargo_guard_parse_version 'this is not cargo output')"
expect_eq "parse empty yields empty" "" \
  "$(cargo_guard_parse_version '')"

# ── version comparison ────────────────────────────────────────────────────────

expect_true  "1.95.0 >= 1.95"      cargo_guard_version_ge 1.95.0 1.95
expect_true  "1.96.1 >= 1.95"      cargo_guard_version_ge 1.96.1 1.95
expect_true  "2.0.0 >= 1.95"       cargo_guard_version_ge 2.0.0 1.95
expect_true  "1.95.1 >= 1.95.0"    cargo_guard_version_ge 1.95.1 1.95.0
expect_true  "1.95 >= 1.95.0"      cargo_guard_version_ge 1.95 1.95.0
expect_false "1.94.9 < 1.95"       cargo_guard_version_ge 1.94.9 1.95
expect_false "1.75.0 < 1.95"       cargo_guard_version_ge 1.75.0 1.95
expect_false "1.95.0 < 1.95.1"     cargo_guard_version_ge 1.95.0 1.95.1
expect_false "1.9.0 < 1.95 (minor is numeric, not decimal)" \
  cargo_guard_version_ge 1.9.0 1.95
expect_false "named channel is not a numeric version" \
  cargo_guard_version_ge stable 1.95

# ── rustup shim-path detection ────────────────────────────────────────────────

expect_true  "shim: ~/.cargo/bin/cargo" \
  cargo_guard_is_rustup_shim "/home/steven/.cargo/bin/cargo"
expect_true  "shim: windows-mounted home" \
  cargo_guard_is_rustup_shim "/mnt/c/Users/steven/.cargo/bin/cargo.exe"
expect_true  "shim: windows separators normalize" \
  cargo_guard_is_rustup_shim 'C:\Users\steven\.cargo\bin\cargo.exe'
expect_true  "shim: rustup toolchain binary" \
  cargo_guard_is_rustup_shim "/home/steven/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo"
expect_false "not shim: /usr/bin/cargo (WSL apt cargo)" \
  cargo_guard_is_rustup_shim "/usr/bin/cargo"
expect_false "not shim: /usr/local/bin/cargo" \
  cargo_guard_is_rustup_shim "/usr/local/bin/cargo"
expect_false "not shim: /opt/cargo/bin/cargo" \
  cargo_guard_is_rustup_shim "/opt/cargo/bin/cargo"

# ── required/pinned version sources ───────────────────────────────────────────

TMPDIR_BASE="$(mktemp -d)"
FAKE_REPO="${TMPDIR_BASE}/fake-repo"
mkdir -p "$FAKE_REPO"

expect_eq "required: workspace Cargo.toml rust-version wins" "1.95" \
  "$(cargo_guard_required_version "$REPO_ROOT")"
expect_eq "pin: rust-toolchain.toml channel" "1.95.0" \
  "$(cargo_guard_pin_version "$REPO_ROOT")"

printf '%s\n' 'rust-version = "1.42"' > "${FAKE_REPO}/Cargo.toml"
expect_eq "required: reads rust-version from given root" "1.42" \
  "$(cargo_guard_required_version "$FAKE_REPO")"

rm -f "${FAKE_REPO}/Cargo.toml"
printf '%s\n' '[toolchain]' 'channel = "1.50.0"' > "${FAKE_REPO}/rust-toolchain.toml"
expect_eq "required: falls back to rust-toolchain.toml channel" "1.50.0" \
  "$(cargo_guard_required_version "$FAKE_REPO")"

rm -rf "$FAKE_REPO"
mkdir -p "$FAKE_REPO"
guard_out="$(cargo_guard_required_version "$FAKE_REPO" || true)"
if [[ -z "$guard_out" ]]; then
  pass "required: empty repo root yields empty (guard applies documented default)"
else
  fail "required: empty repo root should yield empty, got '${guard_out}'"
fi

# ── refusal message contents (pure builder) ───────────────────────────────────

REFUSAL="${TMPDIR_BASE}/refusal.msg"
cargo_guard_print_refusal "/usr/bin/cargo" "1.75.0" "1.95" "1.95.0" 1 "WSL_DISTRO_NAME=Ubuntu" 2> "$REFUSAL"

assert_contains "refusal: typed REFUSED banner" \
  "cargo-toolchain-guard: REFUSED" "$REFUSAL"
assert_contains "refusal: names resolved cargo path" \
  "resolved cargo : /usr/bin/cargo (cargo 1.75.0)" "$REFUSAL"
assert_contains "refusal: names workspace rust-version" \
  "workspace needs: rust-version 1.95 (Cargo.toml)" "$REFUSAL"
assert_contains "refusal: names rust-toolchain.toml pin" \
  "rust-toolchain.toml pins 1.95.0, which only rustup shims honor" "$REFUSAL"
assert_contains "refusal: explains edition-2024 confusion" \
  "feature 'edition2024' is required" "$REFUSAL"
assert_contains "refusal: forbids the manifest downgrade" \
  "Do not downgrade the manifest" "$REFUSAL"
assert_contains "refusal: names WSL non-login root cause" \
  "non-login WSL bash" "$REFUSAL"
assert_contains "refusal: names apt cargo /usr/bin/cargo" \
  "Ubuntu apt cargo" "$REFUSAL"
assert_contains "refusal: remediation installs rustup inside WSL" \
  "install rustup inside WSL" "$REFUSAL"
assert_contains "refusal: remediation names PATH ordering" \
  "~/.cargo/bin precedes /usr/bin in PATH" "$REFUSAL"

REFUSAL_NOWSL="${TMPDIR_BASE}/refusal-nowsl.msg"
cargo_guard_print_refusal "/opt/cargo/bin/cargo" "1.84.0" "1.95" "1.95.0" 0 "" 2> "$REFUSAL_NOWSL"
assert_contains "refusal (non-WSL): typed REFUSED banner" \
  "cargo-toolchain-guard: REFUSED" "$REFUSAL_NOWSL"
assert_contains "refusal (non-WSL): generic rustup remediation" \
  "rustup install 1.95.0" "$REFUSAL_NOWSL"
if grep -Fq "WSL detected" "$REFUSAL_NOWSL"; then
  fail "refusal (non-WSL): must not claim WSL context"
else
  pass "refusal (non-WSL): no WSL paragraph"
fi

# A cargo between 1.85 and the workspace pin can parse edition-2024; the
# refusal must name the workspace policy, not a nonexistent parse limitation.
REFUSAL_MID="${TMPDIR_BASE}/refusal-mid.msg"
cargo_guard_print_refusal "/usr/bin/cargo" "1.90.0" "1.95" "1.95.0" 0 "" 2> "$REFUSAL_MID"
assert_contains "refusal (1.90): names workspace toolchain policy" \
  "the refusal is workspace toolchain policy, not a manifest defect" "$REFUSAL_MID"
if grep -Fq "cannot parse edition-2024" "$REFUSAL_MID"; then
  fail "refusal (1.90): must not claim an edition-2024 parse limitation"
else
  pass "refusal (1.90): no false edition-2024 claim"
fi

# ── full guard against a fake stale cargo (WSL-shaped and plain) ──────────────

write_stale_cargo() {
  local bin_dir="$1"
  local version="$2"
  mkdir -p "$bin_dir"
  cat > "${bin_dir}/cargo" <<STUB
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then
  printf 'cargo %s (stub 2023-11-01)\n' "$version"
  exit 0
fi
printf 'stale-cargo-stub-ran\n'
exit 0
STUB
  chmod +x "${bin_dir}/cargo"
}

STALE_BIN="${TMPDIR_BASE}/stale-bin"
write_stale_cargo "$STALE_BIN" "1.75.0"

code=0
PATH="${STALE_BIN}:$PATH" WSL_DISTRO_NAME=Ubuntu \
  bash -c ". \"$LIB\"; cargo_toolchain_guard" 2> "${TMPDIR_BASE}/stale-wsl.err" || code=$?
if [[ "$code" -eq 78 ]]; then
  pass "guard: stale cargo refuses with typed exit 78 (got ${code})"
else
  fail "guard: stale cargo must refuse with exit 78, got ${code}"
fi
assert_contains "guard: stale refusal names resolved path" \
  "${STALE_BIN}/cargo (cargo 1.75.0)" "${TMPDIR_BASE}/stale-wsl.err"
assert_contains "guard: stale refusal explains WSL remediation" \
  "install rustup inside WSL" "${TMPDIR_BASE}/stale-wsl.err"

code=0
PATH="${STALE_BIN}:$PATH" \
  bash -c ". \"$LIB\"; cargo_toolchain_guard" 2> "${TMPDIR_BASE}/stale-plain.err" || code=$?
if [[ "$code" -eq 78 ]]; then
  pass "guard: stale cargo (no WSL env) still refuses with 78"
else
  fail "guard: stale cargo (no WSL env) must refuse with exit 78, got ${code}"
fi

# ── full guard with no cargo on PATH ──────────────────────────────────────────

code=0
env -i PATH="${TMPDIR_BASE}/empty-path" "$(command -v bash)" \
  -c ". \"$LIB\"; cargo_toolchain_guard" \
  2> "${TMPDIR_BASE}/missing.err" || code=$?
if [[ "$code" -eq 78 ]]; then
  pass "guard: missing cargo refuses with typed exit 78 (got ${code})"
else
  fail "guard: missing cargo must refuse with exit 78, got ${code}"
fi
assert_contains "guard: missing cargo message names the requirement" \
  "no cargo found on PATH" "${TMPDIR_BASE}/missing.err"

# ── full guard with unparseable --version output ──────────────────────────────

BROKEN_BIN="${TMPDIR_BASE}/broken-bin"
mkdir -p "$BROKEN_BIN"
printf '#!/usr/bin/env bash\necho "not a real version line"\n' > "${BROKEN_BIN}/cargo"
chmod +x "${BROKEN_BIN}/cargo"

code=0
PATH="${BROKEN_BIN}:$PATH" \
  bash -c ". \"$LIB\"; cargo_toolchain_guard" 2> "${TMPDIR_BASE}/broken.err" || code=$?
if [[ "$code" -eq 78 ]]; then
  pass "guard: unparseable --version refuses with typed exit 78"
else
  fail "guard: unparseable --version must refuse with exit 78, got ${code}"
fi

# ── full guard with a satisfying non-shim cargo: note, not refusal ────────────

NEW_BIN="${TMPDIR_BASE}/new-bin"
write_stale_cargo "$NEW_BIN" "1.96.1"

code=0
PATH="${NEW_BIN}:$PATH" \
  bash -c ". \"$LIB\"; cargo_toolchain_guard" 2> "${TMPDIR_BASE}/new.err" || code=$?
if [[ "$code" -eq 0 ]]; then
  pass "guard: satisfying non-shim cargo passes"
else
  fail "guard: satisfying non-shim cargo must pass, got exit ${code}"
fi
assert_contains "guard: non-shim note reports the ignored pin" \
  "is not a rustup shim, so the rust-toolchain.toml pin (1.95.0) is not in effect" \
  "${TMPDIR_BASE}/new.err"

# ── probe runs from the repository root, not the caller's directory ──────────
# A rustup shim picks its toolchain from the nearest rust-toolchain.toml
# relative to the current directory. A stub that would report an older cargo
# from a different working directory must not fool the guard when the guarded
# entrypoint will really run inside this repository.

CWD_BIN="${TMPDIR_BASE}/cwd-bin"
mkdir -p "$CWD_BIN"
cat > "${CWD_BIN}/cargo" <<STUB
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then
  if [ -f "./caller-project-marker" ]; then
    printf 'cargo 1.70.0 (caller-pinned stub)\n'
  else
    printf 'cargo 1.96.1 (repo-pinned stub)\n'
  fi
  exit 0
fi
exit 0
STUB
chmod +x "${CWD_BIN}/cargo"

OTHER_PROJECT="${TMPDIR_BASE}/other-project"
mkdir -p "$OTHER_PROJECT"
touch "${OTHER_PROJECT}/caller-project-marker"

code=0
(
  cd "$OTHER_PROJECT"
  PATH="${CWD_BIN}:$PATH" bash -c ". \"$LIB\"; cargo_toolchain_guard"
) 2> "${TMPDIR_BASE}/cwd.err" || code=$?
if [[ "$code" -eq 0 ]]; then
  pass "guard: probes from the repo root, ignoring the caller's pinned directory"
else
  fail "guard: probe must run from the repository root, got exit ${code}"
  cat "${TMPDIR_BASE}/cwd.err" || true
fi

# ── install.sh standalone bootstrap: inline floor when the lib is absent ─────
# install.sh supports Linux and macOS only; on Windows it exits before the
# source-build path, so the standalone checks run only where they can.

if [[ "${OSTYPE:-}" != msys* && "${OSTYPE:-}" != cygwin* && "${OS:-}" != "Windows_NT" ]]; then
  STANDALONE_DIR="${TMPDIR_BASE}/standalone"
  mkdir -p "$STANDALONE_DIR"
  cp "${REPO_ROOT}/scripts/install.sh" "${STANDALONE_DIR}/install.sh"

  code=0
  (
    cd "$STANDALONE_DIR"
    PATH="${STALE_BIN}:$PATH" BUILD_FROM_SOURCE=1 bash ./install.sh
  ) > "${TMPDIR_BASE}/standalone.out" 2> "${TMPDIR_BASE}/standalone.err" || code=$?
  if [[ "$code" -ne 0 ]] && grep -Fq "cargo-toolchain-guard: REFUSED" "${TMPDIR_BASE}/standalone.err"; then
    pass "install.sh standalone: stale cargo refused without the guard library"
  else
    fail "install.sh standalone: expected typed refusal with stale cargo, got exit ${code}"
  fi

  code=0
  (
    cd "$STANDALONE_DIR"
    PATH="${NEW_BIN}:$PATH" BUILD_FROM_SOURCE=1 bash ./install.sh
  ) > "${TMPDIR_BASE}/standalone-new.out" 2> "${TMPDIR_BASE}/standalone-new.err" || code=$?
  if grep -Fq "cargo-toolchain-guard: REFUSED" "${TMPDIR_BASE}/standalone-new.err"; then
    fail "install.sh standalone: satisfying cargo must not hit the guard refusal"
  else
    pass "install.sh standalone: satisfying cargo proceeds past the guard"
  fi
else
  pass "install.sh standalone: skipped on Windows (installer supports Linux/macOS only)"
fi

# ── guarded fallback entrypoints refuse before Cargo work ─────────────────────
# These two scripts were previously missed by the first implementation: one
# still runs Cargo in SKIP_INSTALL mode, and the other reaches Cargo only after
# exhausting its prebuilt xtask ladder.

code=0
(
  cd "$REPO_ROOT"
  PATH="${STALE_BIN}:$PATH" WSL_DISTRO_NAME=Ubuntu \
    SKIP_INSTALL=1 bash scripts/post-publish-smoke.sh 0.0.0
) > "${TMPDIR_BASE}/post-publish-stale.out" \
  2> "${TMPDIR_BASE}/post-publish-stale.err" || code=$?
if [[ "$code" -eq 78 ]]; then
  pass "post-publish smoke: SKIP_INSTALL still guards Cargo with exit 78"
else
  fail "post-publish smoke: stale Cargo must refuse before SKIP_INSTALL work, got ${code}"
fi
assert_contains "post-publish smoke: refusal banner" \
  "cargo-toolchain-guard: REFUSED" "${TMPDIR_BASE}/post-publish-stale.err"

code=0
(
  cd "$REPO_ROOT"
  PATH="${STALE_BIN}:$PATH" bash scripts/check_release_history.sh
) > "${TMPDIR_BASE}/release-history-stale.out" \
  2> "${TMPDIR_BASE}/release-history-stale.err" || code=$?
if [[ "$code" -eq 78 ]]; then
  pass "release history: Cargo fallback refuses stale toolchain with exit 78"
else
  fail "release history: stale Cargo must refuse before fallback work, got ${code}"
fi
assert_contains "release history: refusal banner" \
  "cargo-toolchain-guard: REFUSED" "${TMPDIR_BASE}/release-history-stale.err"

# ── entrypoint coverage consistency ───────────────────────────────────────────
# Every repo bash entrypoint that invokes cargo as a command must source the
# guard library and call cargo_toolchain_guard, or carry an explicit
# "cargo-toolchain-guard: exempt" marker with a reason. This keeps future
# scripts from silently dodging the guard (issue #12593).

invokes_cargo() {
  awk '
    {
      line = $0
      sub(/^[ \t]+/, "", line)
      if (line ~ /^#/) next
      if (line ~ /^(echo|printf)[ \t]/) next
      n = split(line, segs, /[;|&]+|\$\(/)
      for (i = 1; i <= n; i++) {
        s = segs[i]
        sub(/^[ \t]+/, "", s)
        while (s ~ /^(if|then|elif|else|while|until|!|exec|env)[ \t]+/)
          sub(/^(if|then|elif|else|while|until|!|exec|env)[ \t]+/, "", s)
        while (s ~ /^[A-Za-z_][A-Za-z0-9_]*=[^ ]*[ \t]+/)
          sub(/^[A-Za-z_][A-Za-z0-9_]*=[^ ]*[ \t]+/, "", s)
        if (s ~ /^cargo([ \t(]|$)/) { found = 1; exit }
        # Some entrypoints pass a command string through timeout/eval helpers.
        # Treat a quoted cargo command as executable too, while comments and
        # echo/printf examples remain excluded above.
        if (s ~ /["'\'']cargo[ \t]/) { found = 1; exit }
      }
    }
    END { exit !found }
  ' "$1"
}

has_guard_call() {
  awk '
    {
      line = $0
      sub(/[[:space:]]+#.*/, "", line)
      if (line ~ /(^|[^[:alnum:]_])cargo_toolchain_guard([[:space:];&|]|$)/) {
        found = 1
        exit
      }
    }
    END { exit !found }
  ' "$1"
}

mapfile -t ENTRYPOINTS < <(
  find scripts -type f -name '*.sh' \
    ! -path 'scripts/tests/*' \
    ! -path 'scripts/lib/*' \
    -print | sort
)
# Extensionless entrypoints are invisible to the *.sh find above, so each one
# must be named here or its guard goes unverified.
ENTRYPOINTS+=(
  scripts/cargo-safe
  scripts/fuzz-bounded
  scripts/branch-deletion-admission
  .github/run_all_tests.sh
)
for entry in "${ENTRYPOINTS[@]}"; do
  [[ -f "$entry" ]] || continue
  if ! invokes_cargo "$entry"; then
    continue
  fi
  if grep -q "cargo-toolchain-guard: exempt" "$entry"; then
    pass "coverage: ${entry} invokes cargo with an explicit exemption"
    continue
  fi
  if has_guard_call "$entry"; then
    pass "coverage: ${entry} invokes cargo and runs the guard"
  else
    fail "coverage: ${entry} invokes cargo but never calls cargo_toolchain_guard"
  fi
done

# The two front doors named by the issue are guarded no matter what the
# line-level detector sees.
for frontdoor in scripts/cargo-safe .github/run_all_tests.sh; do
  if grep -q "cargo_toolchain_guard" "$frontdoor"; then
    pass "front door guarded: ${frontdoor}"
  else
    fail "front door guarded: ${frontdoor} must call cargo_toolchain_guard"
  fi
done

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi

exit 0
