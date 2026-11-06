#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CARGO_SAFE="${CARGO_SAFE_UNDER_TEST:-${REPO_ROOT}/scripts/cargo-safe}"
PREFLIGHT="${REPO_ROOT}/scripts/preflight.sh"

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

assert_contains() {
  local label="$1" needle="$2" haystack_file="$3"
  if grep -Fq -- "$needle" "$haystack_file"; then
    pass "$label"
  else
    fail "$label (missing: $needle)"
  fi
}

assert_not_contains() {
  local label="$1" needle="$2" haystack_file="$3"
  if grep -Fq -- "$needle" "$haystack_file"; then
    fail "$label (unexpectedly found: $needle)"
  else
    pass "$label"
  fi
}

echo "=== cargo-safe toolchain guard integration test suite ==="
echo ""

if [[ ! -f "$CARGO_SAFE" ]]; then
  echo "ERROR: cargo-safe not found at ${CARGO_SAFE}"
  exit 1
fi

TMPDIR_BASE="$(mktemp -d)"

# A stub cargo that answers the guard's --version probe with a configurable
# version and logs any other argument set it receives, so tests can prove the
# entrypoint refused BEFORE delegating build work.
write_stub_cargo() {
  local bin_dir="$1"
  local version="$2"
  local log_path="$3"
  mkdir -p "$bin_dir"
  cat > "${bin_dir}/cargo" <<STUB
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then
  printf 'cargo %s (stub d6df253b1 2023-11-01)\n' "$version"
  exit 0
fi
printf '%s\n' "\$*" >> "$log_path"
printf 'stub-cargo-ran: %s\n' "\$*"
exit 0
STUB
  chmod +x "${bin_dir}/cargo"
}

# ── 1. stale cargo on PATH: typed refusal, no build work ──────────────────────

STALE_BIN="${TMPDIR_BASE}/stale-bin"
STALE_LOG="${TMPDIR_BASE}/stale-args.log"
: > "$STALE_LOG"
write_stub_cargo "$STALE_BIN" "1.75.0" "$STALE_LOG"

code=0
(
  cd "$REPO_ROOT"
  PATH="${STALE_BIN}:$PATH" WSL_DISTRO_NAME=Ubuntu bash "$CARGO_SAFE" test -p perl-parser --locked
) > "${TMPDIR_BASE}/stale.out" 2> "${TMPDIR_BASE}/stale.err" || code=$?
if [[ "$code" -eq 78 ]]; then
  pass "cargo-safe: stale cargo refuses with typed exit 78 (got ${code})"
else
  fail "cargo-safe: stale cargo must refuse with exit 78, got ${code}"
fi
assert_contains "cargo-safe: refusal banner" \
  "cargo-toolchain-guard: REFUSED" "${TMPDIR_BASE}/stale.err"
assert_contains "cargo-safe: refusal names resolved stub and version" \
  "cargo 1.75.0" "${TMPDIR_BASE}/stale.err"
assert_contains "cargo-safe: refusal names the workspace pin" \
  "rust-toolchain.toml" "${TMPDIR_BASE}/stale.err"
assert_contains "cargo-safe: refusal names the edition-2024 symptom" \
  "edition2024" "${TMPDIR_BASE}/stale.err"
assert_contains "cargo-safe: WSL remediation present" \
  "install rustup inside WSL" "${TMPDIR_BASE}/stale.err"
assert_not_contains "cargo-safe: refusal, not a cargo parse error" \
  "failed to load manifest" "${TMPDIR_BASE}/stale.err"
if [[ ! -s "$STALE_LOG" ]]; then
  pass "cargo-safe: no build args reached the stale cargo (failed before work)"
else
  fail "cargo-safe: stale cargo received delegated args: $(cat "$STALE_LOG")"
fi
assert_not_contains "cargo-safe: stale cargo never executed work" \
  "stub-cargo-ran" "${TMPDIR_BASE}/stale.out"

# ── 2. satisfying non-shim cargo: guard passes, delegation happens ───────────

FRESH_BIN="${TMPDIR_BASE}/fresh-bin"
FRESH_LOG="${TMPDIR_BASE}/fresh-args.log"
: > "$FRESH_LOG"
write_stub_cargo "$FRESH_BIN" "1.96.1" "$FRESH_LOG"

code=0
(
  cd "$REPO_ROOT"
  PATH="${FRESH_BIN}:$PATH" bash "$CARGO_SAFE" --version
) > "${TMPDIR_BASE}/fresh.out" 2> "${TMPDIR_BASE}/fresh.err" || code=$?
if [[ "$code" -eq 0 ]]; then
  pass "cargo-safe: satisfying cargo passes the guard"
else
  fail "cargo-safe: satisfying cargo must pass, got exit ${code}"
fi
if grep -Fq "cargo 1.96.1" "${TMPDIR_BASE}/fresh.out"; then
  pass "cargo-safe: satisfying cargo still receives the delegation"
else
  fail "cargo-safe: expected stub version on stdout, got: $(cat "${TMPDIR_BASE}/fresh.out")"
fi
assert_contains "cargo-safe: non-shim note reports the ignored pin" \
  "is not a rustup shim" "${TMPDIR_BASE}/fresh.err"

# Failed version probes are not evidence that Cargo itself needs replacing.
# Reach the real entrypoint and prove neither a missing rustup pin nor an
# unrelated probe failure can authorize build delegation or storage allocation.
FAILED_BIN="${TMPDIR_BASE}/failed-probe-bin"
mkdir -p "$FAILED_BIN"
cat > "${FAILED_BIN}/cargo" <<'FAILED_PROBE'
#!/usr/bin/env bash
if [[ "${1:-}" == "--version" ]]; then
  printf '%s\n' "$PROBE_ERROR" >&2
  exit 9
fi
printf '%s\n' "$*" >> "$PROBE_WORK_LOG"
exit 0
FAILED_PROBE
chmod +x "${FAILED_BIN}/cargo"

for scenario in missing-pin unrelated-failure; do
  case "$scenario" in
    missing-pin)
      probe_error="error: toolchain '1.95.0-x86_64-unknown-linux-gnu' is not installed"
      ;;
    unrelated-failure)
      # Even version-looking output must not override a failed probe status.
      probe_error=$'cargo 1.99.0 (fixture)\nerror: permission denied'
      ;;
  esac
  code=0
  PATH="${FAILED_BIN}:$PATH" PROBE_ERROR="$probe_error" \
    PROBE_WORK_LOG="${TMPDIR_BASE}/${scenario}.work" \
    DEVPLANE="${TMPDIR_BASE}/${scenario}.plane" \
    bash "$CARGO_SAFE" check -p perl-test-must --locked \
    > "${TMPDIR_BASE}/${scenario}.out" 2> "${TMPDIR_BASE}/${scenario}.err" || code=$?
  if [[ "$code" -eq 78 && ! -e "${TMPDIR_BASE}/${scenario}.work" && ! -e "${TMPDIR_BASE}/${scenario}.plane" ]]; then
    pass "$scenario: failed probe refuses before work or allocation"
  else
    fail "$scenario: expected exit 78 without work/allocation, got $code"
  fi
  if [[ "$(< "${TMPDIR_BASE}/${scenario}.err")" == *"$probe_error"* ]]; then
    pass "$scenario: preserves the complete underlying probe error"
  else
    fail "$scenario: missing underlying probe error"
  fi
  assert_contains "$scenario: names the read-only rustup inventory" \
    'rustup toolchain list' "${TMPDIR_BASE}/${scenario}.err"
  assert_contains "$scenario: distinguishes stable from the repository pin" \
    'Installing stable alone does not install that pin' "${TMPDIR_BASE}/${scenario}.err"
  assert_contains "$scenario: makes pin installation conditional on the error" \
    'if the error reports the pinned toolchain missing: rustup toolchain install 1.95.0' \
    "${TMPDIR_BASE}/${scenario}.err"
  assert_not_contains "$scenario: does not prescribe removing Cargo" \
    'repair or remove this cargo from PATH' "${TMPDIR_BASE}/${scenario}.err"
done

# ── 3. real toolchain happy path stays green ─────────────────────────────────

code=0
(
  cd "$REPO_ROOT"
  bash "$CARGO_SAFE" --version
) > "${TMPDIR_BASE}/real.out" 2> "${TMPDIR_BASE}/real.err" || code=$?
if [[ "$code" -eq 0 ]]; then
  pass "cargo-safe: real toolchain happy path exits 0"
else
  fail "cargo-safe: real toolchain happy path failed with exit ${code}: $(cat "${TMPDIR_BASE}/real.err")"
fi
if grep -Eq '^cargo [0-9]+\.[0-9]+' "${TMPDIR_BASE}/real.out"; then
  pass "cargo-safe: real toolchain reports its version"
else
  fail "cargo-safe: expected a cargo version on stdout, got: $(cat "${TMPDIR_BASE}/real.out")"
fi

# ── 4. xtask delegate entrypoint refuses the same way ─────────────────────────

code=0
(
  cd "$REPO_ROOT"
  PATH="${STALE_BIN}:$PATH" bash "$PREFLIGHT"
) > "${TMPDIR_BASE}/preflight.out" 2> "${TMPDIR_BASE}/preflight.err" || code=$?
if [[ "$code" -eq 78 ]]; then
  pass "preflight.sh: stale cargo refuses with typed exit 78"
else
  fail "preflight.sh: stale cargo must refuse with exit 78, got ${code}"
fi
assert_contains "preflight.sh: refusal banner present" \
  "cargo-toolchain-guard: REFUSED" "${TMPDIR_BASE}/preflight.err"
if [[ ! -s "$STALE_LOG" ]]; then
  pass "preflight.sh: xtask delegation never ran under the stale cargo"
else
  fail "preflight.sh: stale cargo received delegated args: $(cat "$STALE_LOG")"
fi

# Every shared guard probe must suppress automatic toolchain installation,
# including legacy callers without the admitted route's exported setting.
# Model a missing pin with a fake rustup Cargo proxy: without suppression even
# --version would attempt installation. No network or toolchain mutation occurs.
{
  MISSING_BIN="${TMPDIR_BASE}/missing-pin-bin"
  mkdir -p "$MISSING_BIN"
  cat > "${MISSING_BIN}/cargo" <<'MISSING_PROXY'
#!/usr/bin/env bash
if [[ "${RUSTUP_AUTO_INSTALL:-1}" != "0" ]]; then
  printf 'attempted install\n' > "$INSTALL_MARKER"
fi
printf 'missing pinned toolchain (fixture)\n' >&2
exit 1
MISSING_PROXY
  chmod +x "${MISSING_BIN}/cargo"
  INSTALL_MARKER="${TMPDIR_BASE}/control-install" RUSTUP_AUTO_INSTALL=1 "${MISSING_BIN}/cargo" --version >/dev/null 2>&1 || true
  if [[ -f "${TMPDIR_BASE}/control-install" ]]; then
    pass "missing-pin control would attempt install during version probe"
  else
    fail "missing-pin control did not exercise automatic install"
  fi
  code=0
  PATH="${MISSING_BIN}:$PATH" INSTALL_MARKER="${TMPDIR_BASE}/guarded-install" RUSTUP_AUTO_INSTALL=1 \
    DEVPLANE="${TMPDIR_BASE}/unallocated-plane" bash "$CARGO_SAFE" check >"${TMPDIR_BASE}/missing.out" 2>"${TMPDIR_BASE}/missing.err" || code=$?
  if [[ "$code" -eq 78 && ! -e "${TMPDIR_BASE}/guarded-install" && ! -e "${TMPDIR_BASE}/unallocated-plane" ]]; then
    pass "missing pin refuses before installation or storage allocation"
  else
    fail "missing pin must refuse without installation/allocation (exit $code)"
  fi
  assert_contains "missing-pin refusal preserves the probe diagnostic" \
    "missing pinned toolchain (fixture)" "${TMPDIR_BASE}/missing.err"
}

# Old rustup ignores RUSTUP_AUTO_INSTALL; even its own --version installs.
# The standard hardlinked proxy must be refused using only manager --help.
OLD_PROXY_BIN="${TMPDIR_BASE}/old-proxy-bin"
mkdir -p "$OLD_PROXY_BIN"
cat > "${OLD_PROXY_BIN}/rustup" <<'OLD_PROXY'
#!/usr/bin/env bash
printf '%s %s\n' "${0##*/}" "$*" >> "$OLD_PROXY_TRACE"
if [[ "${0##*/}" == rustup && "$*" == --help ]]; then
  printf 'rustup 1.27.1 (fixture)\n'
  exit 0
fi
printf 'attempted install despite override\n' > "$INSTALL_MARKER"
exit 1
OLD_PROXY
chmod +x "${OLD_PROXY_BIN}/rustup"
ln "${OLD_PROXY_BIN}/rustup" "${OLD_PROXY_BIN}/cargo"
OLD_PROXY_TRACE="${TMPDIR_BASE}/old-control.trace" INSTALL_MARKER="${TMPDIR_BASE}/old-control-install" \
  RUSTUP_AUTO_INSTALL=0 "${OLD_PROXY_BIN}/cargo" --version >/dev/null 2>&1 || true
if [[ -f "${TMPDIR_BASE}/old-control-install" ]]; then
  pass "old proxy control ignores the no-install override"
else
  fail "old proxy control did not exercise automatic installation"
fi
code=0
PATH="${OLD_PROXY_BIN}:$PATH" OLD_PROXY_TRACE="${TMPDIR_BASE}/old-guard.trace" \
  INSTALL_MARKER="${TMPDIR_BASE}/old-guard-install" DEVPLANE="${TMPDIR_BASE}/old-unallocated-plane" \
  bash "$CARGO_SAFE" check >"${TMPDIR_BASE}/old.out" 2>"${TMPDIR_BASE}/old.err" || code=$?
if [[ "$code" == 78 && ! -e "${TMPDIR_BASE}/old-guard-install" && ! -e "${TMPDIR_BASE}/old-unallocated-plane" &&
      "$(cat "${TMPDIR_BASE}/old-guard.trace")" == 'rustup --help' ]]; then
  pass "old proxy refuses with help only before installation or allocation"
else
  fail "old proxy must refuse before install-capable probes (exit $code)"
fi
assert_contains "old proxy refusal names the safe manager floor" \
  'needs rustup >= 1.28.0' "${TMPDIR_BASE}/old.err"

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi

exit 0
