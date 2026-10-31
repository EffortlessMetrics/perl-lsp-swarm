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
  cargo_guard_is_rustup_shim "/fixture/profile/.cargo/bin/cargo"
expect_true  "shim: windows-mounted home" \
  cargo_guard_is_rustup_shim "/mnt/c/fixture/profile/.cargo/bin/cargo.exe"
expect_true  "shim: windows separators normalize" \
  cargo_guard_is_rustup_shim 'C:\fixture\profile\.cargo\bin\cargo.exe'
expect_true  "shim: rustup toolchain binary" \
  cargo_guard_is_rustup_shim "/fixture/profile/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo"
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
printf 'unexpected cargo invocation: %s\n' "\$*" >&2
exit 97
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

# A configured but off-PATH Cargo must be reported without running it or
# silently changing which executable the calling entrypoint uses.
HIDDEN_HOME="${TMPDIR_BASE}/hidden cargo"
mkdir -p "${HIDDEN_HOME}/bin"
printf '#!/bin/sh\n: > "$HIDDEN_MARKER"\n' > "${HIDDEN_HOME}/bin/cargo"
chmod +x "${HIDDEN_HOME}/bin/cargo"
# Prove the sentinel works in the same empty PATH environment.
env -i PATH="${TMPDIR_BASE}/empty-path" HIDDEN_MARKER="${TMPDIR_BASE}/hidden-ran" "${HIDDEN_HOME}/bin/cargo"
if [[ -f "${TMPDIR_BASE}/hidden-ran" ]]; then
  pass "guard: off-PATH sentinel creates marker with empty PATH"
else
  fail "guard: off-PATH sentinel failed to create marker"
fi
rm -f "${TMPDIR_BASE}/hidden-ran"
code=0
env -i PATH="${TMPDIR_BASE}/empty-path" CARGO_HOME="$HIDDEN_HOME" HIDDEN_MARKER="${TMPDIR_BASE}/hidden-ran" "$(command -v bash)" \
  -c ". \"$LIB\"; cargo_toolchain_guard" 2> "${TMPDIR_BASE}/hidden.err" || code=$?
if [[ "$code" -eq 78 && ! -e "${TMPDIR_BASE}/hidden-ran" ]]; then
  pass "guard: off-PATH Cargo is reported without execution or implicit selection"
else
  fail "guard: off-PATH Cargo changed refusal or was executed"
fi
assert_contains "guard: reports configured off-PATH Cargo" \
  "${HIDDEN_HOME}/bin/cargo, outside PATH (not probed)" "${TMPDIR_BASE}/hidden.err"
assert_contains "guard: install is conditional" \
  "install rustup only if no suitable installation exists" "${TMPDIR_BASE}/hidden.err"

# A searchable directory named cargo is not an executable Cargo file.
DIR_HOME="${TMPDIR_BASE}/directory cargo"
mkdir -p "${DIR_HOME}/bin/cargo"
code=0
env -i PATH="${TMPDIR_BASE}/empty-path" CARGO_HOME="$DIR_HOME" "$(command -v bash)" \
  -c ". \"$LIB\"; cargo_toolchain_guard" 2> "${TMPDIR_BASE}/directory.err" || code=$?
if [[ "$code" -eq 78 ]] && ! grep -Fq 'found an executable Cargo' "${TMPDIR_BASE}/directory.err"; then
  pass "guard: directory named cargo is not reported as an executable"
else
  fail "guard: directory named cargo was falsely reported as an executable"
fi

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

# ── old rustup proxies must refuse before any install-capable probe ────────
PROXY_BIN="${TMPDIR_BASE}/proxy bin"
mkdir -p "$PROXY_BIN"
cat > "${PROXY_BIN}/rustup" <<'STUB'
#!/usr/bin/env bash
printf '%s %s\n' "${0##*/}" "$*" >> "$PROXY_TRACE"
if [[ "${0##*/}" == rustup && "$*" == --help ]]; then
  printf '%s\n' "$PROXY_HEADER"
  exit "${PROXY_HELP_EXIT:-0}"
fi
# Both cargo --version and rustup --version are unsafe on old rustup.
printf '%s\n' "${RUSTUP_AUTO_INSTALL-unset}" >> "$PROXY_CARGO_ENV"
printf 'cargo 1.95.0 (proxy fixture)\n'
STUB
chmod +x "${PROXY_BIN}/rustup"
ln "${PROXY_BIN}/rustup" "${PROXY_BIN}/cargo"

for header in 'rustup 1.27.1 (fixture)' 'rustup 1.28.0 (fixture)' \
  'rustup 1.29.2 (fixture)' 'rustup 1.28.0-beta.1' 'unrelated 9.99.0'; do
  expected=78
  case "$header" in 'rustup 1.28.0 (fixture)'|'rustup 1.29.2 (fixture)') expected=0 ;; esac
  : > "${TMPDIR_BASE}/proxy.trace"
  : > "${TMPDIR_BASE}/proxy.env"
  code=0
  PATH="${PROXY_BIN}:$PATH" PROXY_HEADER="$header" \
    PROXY_TRACE="${TMPDIR_BASE}/proxy.trace" PROXY_CARGO_ENV="${TMPDIR_BASE}/proxy.env" \
    bash -c '. "$1"; cargo_toolchain_guard' guard-test "$LIB" \
    2> "${TMPDIR_BASE}/proxy.err" || code=$?
  expect_eq "proxy: capability boundary for '$header'" "$expected" "$code"
  if [[ "$expected" == 78 ]]; then
    expect_eq "proxy: unsafe/unverified manager uses help only ('$header')" \
      'rustup --help' "$(cat "${TMPDIR_BASE}/proxy.trace")"
    expect_eq "proxy: rejected manager never invokes Cargo ('$header')" \
      '' "$(cat "${TMPDIR_BASE}/proxy.env")"
  else
    expect_eq "proxy: released safe manager reaches suppressed Cargo ('$header')" \
      0 "$(cat "${TMPDIR_BASE}/proxy.env")"
  fi
done

code=0
PATH="${PROXY_BIN}:$PATH" PROXY_HEADER='rustup 1.29.2 (fixture)' PROXY_HELP_EXIT=7 \
  PROXY_TRACE="${TMPDIR_BASE}/help-failed.trace" PROXY_CARGO_ENV="${TMPDIR_BASE}/help-failed.env" \
  bash -c '. "$1"; cargo_toolchain_guard' guard-test "$LIB" \
  2> "${TMPDIR_BASE}/help-failed.err" || code=$?
expect_eq 'proxy: failed help is not a verified manager' 78 "$code"
expect_eq 'proxy: failed help never invokes Cargo' 'rustup --help' \
  "$(cat "${TMPDIR_BASE}/help-failed.trace")"

# Identity follows standard symlinks and a relocated proxy whose manager is
# found on PATH; neither filename heuristics nor an unrelated manager suffice.
RELOCATED_BIN="${TMPDIR_BASE}/relocated"
mkdir -p "$RELOCATED_BIN"
ln -s "${PROXY_BIN}/rustup" "${RELOCATED_BIN}/cargo"
expect_eq 'proxy: symlink outside manager directory resolves through PATH' \
  "${PROXY_BIN}/rustup" \
  "$(PATH="${PROXY_BIN}:$PATH" cargo_guard_proxy_manager "${RELOCATED_BIN}/cargo")"
expect_false 'proxy: standalone Cargo is not the unrelated old PATH manager' \
  cargo_guard_proxy_manager "${NEW_BIN}/cargo"
expect_false 'proxy: a function name is not a same-named file in cwd' \
  cargo_guard_proxy_manager cargo
code=0
PATH="${NEW_BIN}:${PROXY_BIN}:$PATH" PROXY_HEADER='rustup 1.27.1 (fixture)' \
  PROXY_TRACE="${TMPDIR_BASE}/unrelated-manager.trace" \
  bash -c '. "$1"; cargo_toolchain_guard' guard-test "$LIB" \
  2> "${TMPDIR_BASE}/unrelated-manager.err" || code=$?
expect_eq 'proxy: standalone Cargo passes with an unrelated old PATH manager' 0 "$code"
expect_false 'proxy: unrelated manager was never executed' \
  test -e "${TMPDIR_BASE}/unrelated-manager.trace"

# Exercise the function case with an actual same-named proxy file in cwd.
code=0
PATH="${PROXY_BIN}:$PATH" bash -c '
  cd "$2"
  cargo() { printf "cargo 1.95.0 (function fixture)\n"; }
  . "$1"
  cargo_toolchain_guard
' guard-test "$LIB" "$PROXY_BIN" 2> "${TMPDIR_BASE}/function.err" || code=$?
expect_eq 'proxy: Cargo function is not replaced by cwd file identity' 0 "$code"

# ── version probes never opt into implicit toolchain installation ──────────
# The shared library is also used by legacy callers that do not export the
# admitted wrapper's no-install setting. Observe the subprocess environment,
# and separately verify that the guard does not change its caller's policy.
PROBE_BIN="${TMPDIR_BASE}/probe-bin"
mkdir -p "$PROBE_BIN"
cat > "${PROBE_BIN}/cargo" <<'STUB'
#!/usr/bin/env bash
[[ "$*" == "--version" ]] || exit 91
printf '%s\n' "${RUSTUP_AUTO_INSTALL-unset}" > "$PROBE_ENV_LOG"
printf 'cargo 1.95.0 (installed fixture)\n'
STUB
chmod +x "${PROBE_BIN}/cargo"

for setting in unset '' 0 1; do
  code=0
  PATH="${PROBE_BIN}:$PATH" PROBE_ENV_LOG="${TMPDIR_BASE}/probe.env" \
    bash -c '
      if [[ "$2" == unset ]]; then unset RUSTUP_AUTO_INSTALL;
      else export RUSTUP_AUTO_INSTALL="$2"; fi
      . "$1"
      cargo_toolchain_guard
      printf "%s\n" "${RUSTUP_AUTO_INSTALL-unset}"
    ' guard-test "$LIB" "$setting" \
    > "${TMPDIR_BASE}/caller.env" 2> "${TMPDIR_BASE}/probe.err" || code=$?
  expect_eq "guard: installed probe passes with caller setting '$setting'" 0 "$code"
  expect_eq "guard: probe disables auto-install with caller setting '$setting'" \
    0 "$(cat "${TMPDIR_BASE}/probe.env")"
  expect_eq "guard: preserves caller setting '$setting'" \
    "$setting" "$(cat "${TMPDIR_BASE}/caller.env")"
done

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

  # No inherited Cargo, Rustup, installation, or temp paths. Every external
  # network/install entrypoint used by this installer is intercepted.
  SENTINEL_BIN="${TMPDIR_BASE}/installer-sentinels"
  mkdir -p "$SENTINEL_BIN" "${TMPDIR_BASE}/installer-home" \
    "${TMPDIR_BASE}/installer-dest" "${TMPDIR_BASE}/installer-tmp"
  for cmd in curl wget git rustup; do
    cat > "${SENTINEL_BIN}/${cmd}" <<'STUB'
#!/bin/sh
printf '%s\n' "${0##*/}" >> "$SENTINEL_LOG"
exit 97
STUB
    chmod +x "${SENTINEL_BIN}/${cmd}"
  done
  cat > "${SENTINEL_BIN}/cargo" <<'STUB'
#!/bin/sh
if [ "${1:-}" = --version ]; then
  printf 'cargo %s (fixture)\n' "$FIXTURE_CARGO_VERSION"
  exit 0
fi
printf 'cargo %s\n' "${1:-}" >> "$SENTINEL_LOG"
exit 97
STUB
  chmod +x "${SENTINEL_BIN}/cargo"

  run_installer_fixture() {
    local script="$1" version="$2" log="$3" output="$4"
    : > "$log"
    ( cd "$STANDALONE_DIR"
      env -i PATH="${SENTINEL_BIN}:/usr/bin:/bin" HOME="${TMPDIR_BASE}/installer-home" \
        CARGO_HOME="${TMPDIR_BASE}/installer-home/cargo" \
        RUSTUP_HOME="${TMPDIR_BASE}/installer-home/rustup" \
        TMPDIR="${TMPDIR_BASE}/installer-tmp" INSTALL_DIR="${TMPDIR_BASE}/installer-dest" \
        VERSION=latest BUILD_FROM_SOURCE=1 FIXTURE_CARGO_VERSION="$version" \
        SENTINEL_LOG="$log" bash "$script"
    ) > "${output}.out" 2> "${output}.err"
  }

  code=0
  run_installer_fixture ./install.sh 1.75.0 "${TMPDIR_BASE}/stale-sentinel.log" \
    "${TMPDIR_BASE}/standalone" || code=$?
  if [[ "$code" -ne 0 ]] && grep -Fq "cargo-toolchain-guard: REFUSED" "${TMPDIR_BASE}/standalone.err" &&
    [[ ! -s "${TMPDIR_BASE}/stale-sentinel.log" ]]; then
    pass "install.sh standalone: stale cargo refused before network or install"
  else
    fail "install.sh standalone: stale cargo must refuse before network or install"
  fi

  code=0
  run_installer_fixture ./install.sh 1.96.1 "${TMPDIR_BASE}/new-sentinel.log" \
    "${TMPDIR_BASE}/standalone-new" || code=$?
  if [[ "$code" -ne 0 ]] && ! grep -Fq "cargo-toolchain-guard: REFUSED" "${TMPDIR_BASE}/standalone-new.err" &&
    [[ "$(cat "${TMPDIR_BASE}/new-sentinel.log")" == 'cargo install' ]] &&
    [[ -z "$(ls -A "${TMPDIR_BASE}/installer-dest")" ]]; then
    pass "install.sh standalone: satisfying cargo reaches blocked install without network or destination writes"
  else
    fail "install.sh standalone: expected only the blocked cargo install boundary"
  fi

  # Inject a network call into a disposable copy. The same sentinel-log
  # predicate used above must reject it even if installer output is otherwise
  # indistinguishable from the stale-cargo case.
  sed 's/^    main "\$@"$/    curl https:\/\/example.invalid; main "\$@"/' \
    "${STANDALONE_DIR}/install.sh" > "${STANDALONE_DIR}/injected.sh"
  code=0
  run_installer_fixture ./injected.sh 1.75.0 "${TMPDIR_BASE}/injected-sentinel.log" \
    "${TMPDIR_BASE}/standalone-injected" || code=$?
  if [[ "$code" -ne 0 && "$(cat "${TMPDIR_BASE}/injected-sentinel.log")" == curl ]]; then
    pass "install.sh standalone: injected network attempt is blocked and detected"
  else
    fail "install.sh standalone: injected network attempt escaped detection"
  fi

  sed 's/^    main "\$@"$/    cargo install unexpected; main "\$@"/' \
    "${STANDALONE_DIR}/install.sh" > "${STANDALONE_DIR}/injected-install.sh"
  code=0
  run_installer_fixture ./injected-install.sh 1.75.0 "${TMPDIR_BASE}/injected-install.log" \
    "${TMPDIR_BASE}/standalone-injected-install" || code=$?
  if [[ "$code" -ne 0 && "$(cat "${TMPDIR_BASE}/injected-install.log")" == 'cargo install' ]]; then
    pass "install.sh standalone: injected install attempt is blocked and detected"
  else
    fail "install.sh standalone: injected install attempt escaped detection"
  fi

else
  pass "install.sh standalone: skipped on Windows (installer supports Linux/macOS only)"
fi

# ── guarded fallback entrypoints refuse before Cargo work ─────────────────────
# These two scripts were previously missed by the first implementation: one
# still runs Cargo in SKIP_INSTALL mode, and the other reaches Cargo only after
# exhausting its prebuilt xtask ladder.

# Run both entrypoints from a disposable fixture root. Do not consult a real
# target tree, git repository, Python validators, or caller-owned smoke paths.
ENTRY_ROOT="${TMPDIR_BASE}/entry-root"
ENTRY_BIN="${TMPDIR_BASE}/entry-bin"
mkdir -p "$ENTRY_ROOT/scripts/lib" "$ENTRY_ROOT/target/debug" "$ENTRY_BIN" \
  "$ENTRY_ROOT/home" "$ENTRY_ROOT/tmp" "$ENTRY_ROOT/smoke-install" "$ENTRY_ROOT/smoke-work" \
  "${TMPDIR_BASE}/hostile-target/debug"
cp "${REPO_ROOT}/scripts/post-publish-smoke.sh" "${REPO_ROOT}/scripts/check_release_history.sh" "$ENTRY_ROOT/scripts/"
cp "$LIB" "$ENTRY_ROOT/scripts/lib/"
cp "${REPO_ROOT}/Cargo.toml" "${REPO_ROOT}/rust-toolchain.toml" "$ENTRY_ROOT/"
: > "$ENTRY_ROOT/RELEASE_HISTORY.md"
cp "${STALE_BIN}/cargo" "$ENTRY_BIN/cargo"
for cmd in curl wget rustup; do
  cat > "$ENTRY_BIN/$cmd" <<'STUB'
#!/bin/sh
printf '%s\n' "${0##*/}" >> "$ENTRY_SENTINEL_LOG"
exit 97
STUB
  chmod +x "$ENTRY_BIN/$cmd"
done
# Fixed empty fixture input: no real Git metadata or Python validators execute.
for cmd in git python3; do
  cat > "$ENTRY_BIN/$cmd" <<'STUB'
#!/bin/sh
printf '%s\n' "${0##*/}" >> "$ENTRY_INPUT_LOG"
exit 0
STUB
  chmod +x "$ENTRY_BIN/$cmd"
done
cat > "${TMPDIR_BASE}/hostile-target/debug/xtask" <<'STUB'
#!/bin/sh
: > "$HOSTILE_EXEC_MARKER"
exit 0
STUB
chmod +x "${TMPDIR_BASE}/hostile-target/debug/xtask"
# A non-executable fixture sentinel proves the fallback skips fixture prebuilts.
cp "${TMPDIR_BASE}/hostile-target/debug/xtask" "$ENTRY_ROOT/target/debug/xtask"
chmod -x "$ENTRY_ROOT/target/debug/xtask"
: > "${TMPDIR_BASE}/entry-sentinel.log"
: > "${TMPDIR_BASE}/entry-input.log"
run_entry_fixture() {
  local entry="$1"; shift
  ( cd "$ENTRY_ROOT"
    env -i PATH="${ENTRY_BIN}:/usr/bin:/bin" HOME="$ENTRY_ROOT/home" \
      TMPDIR="$ENTRY_ROOT/tmp" CARGO_HOME="$ENTRY_ROOT/home/cargo" \
      RUSTUP_HOME="$ENTRY_ROOT/home/rustup" \
      SMOKE_INSTALL_ROOT="$ENTRY_ROOT/smoke-install" SMOKE_WORK_DIR="$ENTRY_ROOT/smoke-work" \
      ENTRY_SENTINEL_LOG="${TMPDIR_BASE}/entry-sentinel.log" \
      ENTRY_INPUT_LOG="${TMPDIR_BASE}/entry-input.log" \
      HOSTILE_EXEC_MARKER="${TMPDIR_BASE}/hostile-executed" \
      SKIP_INSTALL=1 WSL_DISTRO_NAME=Ubuntu bash "scripts/$entry" "$@"
  )
}
# Simulate hostile caller overrides. env -i inside the runner must remove them.
export CARGO_TARGET_DIR="${TMPDIR_BASE}/hostile-target"
export SMOKE_INSTALL_ROOT="${TMPDIR_BASE}/hostile-install"
export SMOKE_WORK_DIR="${TMPDIR_BASE}/hostile-work"
code=0
run_entry_fixture post-publish-smoke.sh 0.0.0 > "${TMPDIR_BASE}/post-publish-stale.out" \
  2> "${TMPDIR_BASE}/post-publish-stale.err" || code=$?
if [[ "$code" -eq 78 ]] && grep -Fq 'cargo-toolchain-guard: REFUSED' "${TMPDIR_BASE}/post-publish-stale.err" &&
  [[ -d "$ENTRY_ROOT/smoke-install/bin" && -d "$ENTRY_ROOT/smoke-work/cargo-home" &&
     ! -e "$SMOKE_INSTALL_ROOT" && ! -e "$SMOKE_WORK_DIR" && ! -s "${TMPDIR_BASE}/entry-sentinel.log" ]]; then
  pass "post-publish smoke: clean fixture refuses stale Cargo and ignores inherited directories"
else
  fail "post-publish smoke: fixture escaped its roots or failed guard"
fi
code=0
run_entry_fixture check_release_history.sh > "${TMPDIR_BASE}/release-history-stale.out" \
  2> "${TMPDIR_BASE}/release-history-stale.err" || code=$?
if [[ "$code" -eq 78 ]] && grep -Fq 'cargo-toolchain-guard: REFUSED' "${TMPDIR_BASE}/release-history-stale.err" &&
  [[ ! -e "${TMPDIR_BASE}/hostile-executed" && ! -s "${TMPDIR_BASE}/entry-sentinel.log" ]] &&
  grep -qx git "${TMPDIR_BASE}/entry-input.log" && grep -qx python3 "${TMPDIR_BASE}/entry-input.log"; then
  pass "release history: clean fixture refuses stale Cargo and ignores inherited prebuilt xtask"
else
  fail "release history: fixture executed a prebuilt or failed guard"
fi
# Prove the hostile prebuilt is executable and would create its marker if selected.
HOSTILE_EXEC_MARKER="${TMPDIR_BASE}/hostile-control" \
  "${TMPDIR_BASE}/hostile-target/debug/xtask"
if [[ -f "${TMPDIR_BASE}/hostile-control" ]]; then
  pass "release history: hostile prebuilt control can create its marker"
else
  fail "release history: hostile prebuilt control is inert"
fi
unset CARGO_TARGET_DIR SMOKE_INSTALL_ROOT SMOKE_WORK_DIR

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
