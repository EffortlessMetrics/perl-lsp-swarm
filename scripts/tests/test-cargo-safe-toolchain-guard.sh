#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
CARGO_SAFE="${REPO_ROOT}/scripts/cargo-safe"
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

TOTAL=$((PASS + FAIL))
echo ""
echo "=== Results: ${PASS}/${TOTAL} passed ==="

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi

exit 0
