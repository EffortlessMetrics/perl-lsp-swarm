#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TMPDIR_BASE="$(mktemp -d)"
FIXTURE_ROOT="${TMPDIR_BASE}/fixture"
FAKE_BIN="${TMPDIR_BASE}/bin"
FAKE_LOG="${TMPDIR_BASE}/cargo-safe.log"

cleanup() {
  rm -rf "${TMPDIR_BASE}"
}
trap cleanup EXIT

pass() {
  printf 'PASS %s\n' "$1"
}

fail() {
  printf 'FAIL %s\n' "$1" >&2
  exit 1
}

assert_exit_nonzero() {
  local label="$1"
  local code="$2"
  if [[ "${code}" -ne 0 ]]; then
    pass "${label} (exit ${code} as expected)"
  else
    fail "${label} (expected a non-zero exit)"
  fi
}

assert_contains() {
  local label="$1"
  local needle="$2"
  local haystack="$3"
  if grep -Fq -- "${needle}" "${haystack}"; then
    pass "${label} (found '${needle}')"
  else
    fail "${label} (missing '${needle}')"
  fi
}

assert_same_file() {
  local label="$1"
  local expected="$2"
  local actual="$3"
  if cmp -s "${expected}" "${actual}"; then
    pass "${label}"
  else
    fail "${label} (baseline changed unexpectedly)"
  fi
}

write_fake_cargo_safe() {
  mkdir -p "${FAKE_BIN}"
  cat > "${FIXTURE_ROOT}/scripts/cargo-safe" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "public-api" ]]; then
  printf 'unexpected cargo-safe invocation: %s\n' "$*" >&2
  exit 2
fi

printf '%s\n' "${FAKE_PUBLIC_API_MODE:-unset}" >> "${FAKE_PUBLIC_API_LOG}"
case "${FAKE_PUBLIC_API_MODE:-}" in
  empty)
    exit 0
    ;;
  nonempty)
    printf 'pub struct GeneratedSurface;\n'
    ;;
  *)
    printf 'unsupported fake public-api mode\n' >&2
    exit 2
    ;;
esac
STUB
  chmod +x "${FIXTURE_ROOT}/scripts/cargo-safe"

  # _public-api-install only needs to see the pinned tool as installed.
  : > "${FAKE_BIN}/cargo-public-api"
  chmod +x "${FAKE_BIN}/cargo-public-api"
}

run_recipe() {
  local mode="$1"
  local recipe="$2"
  local output_path="$3"
  local code=0

  (
    cd "${FIXTURE_ROOT}"
    PATH="${FAKE_BIN}:${PATH}" \
      FAKE_PUBLIC_API_MODE="${mode}" \
      FAKE_PUBLIC_API_LOG="${FAKE_LOG}" \
      just --justfile "${FIXTURE_ROOT}/justfile" "${recipe}"
  ) > "${output_path}" 2>&1 || code=$?
  return "${code}"
}

echo "=== public-api ratchet executable regression fixture ==="

mkdir -p "${FIXTURE_ROOT}/scripts" "${FIXTURE_ROOT}/.ci/public-api-baselines"
cp "${REPO_ROOT}/justfile" "${FIXTURE_ROOT}/justfile"
write_fake_cargo_safe

for crate in perl-lsp-rs perl-parser perl-uri perl-dap perllsp; do
  printf 'pub struct ExistingSurface;\n' \
    > "${FIXTURE_ROOT}/.ci/public-api-baselines/${crate}.txt"
done

CHECK_EMPTY_OUTPUT="${TMPDIR_BASE}/check-empty.txt"
if run_recipe empty public-api-check "${CHECK_EMPTY_OUTPUT}"; then
  check_empty_code=0
else
  check_empty_code=$?
fi
assert_exit_nonzero "successful empty generation fails public-api-check closed" "${check_empty_code}"
assert_contains "check classifies empty generation as instrument failure" \
  "generated API surface is empty" "${CHECK_EMPTY_OUTPUT}"

BASELINE_BEFORE="${TMPDIR_BASE}/perl-lsp-rs-before.txt"
cp "${FIXTURE_ROOT}/.ci/public-api-baselines/perl-lsp-rs.txt" "${BASELINE_BEFORE}"
UPDATE_EMPTY_OUTPUT="${TMPDIR_BASE}/update-empty.txt"
if run_recipe empty public-api-update "${UPDATE_EMPTY_OUTPUT}"; then
  update_empty_code=0
else
  update_empty_code=$?
fi
assert_exit_nonzero "successful empty generation refuses baseline update" "${update_empty_code}"
assert_contains "update reports baseline preservation refusal" \
  "refusing to overwrite the baseline" "${UPDATE_EMPTY_OUTPUT}"
assert_same_file "empty generation preserves the existing baseline" \
  "${BASELINE_BEFORE}" "${FIXTURE_ROOT}/.ci/public-api-baselines/perl-lsp-rs.txt"
assert_contains "fixture records the successful empty generator control" \
  "empty" "${FAKE_LOG}"

UPDATE_NONEMPTY_OUTPUT="${TMPDIR_BASE}/update-nonempty.txt"
if run_recipe nonempty public-api-update "${UPDATE_NONEMPTY_OUTPUT}"; then
  update_nonempty_code=0
else
  update_nonempty_code=$?
fi
if [[ "${update_nonempty_code}" -eq 0 ]]; then
  pass "non-empty generation reaches the normal update path"
else
  fail "non-empty generation unexpectedly failed (exit ${update_nonempty_code})"
fi

CHECK_NONEMPTY_OUTPUT="${TMPDIR_BASE}/check-nonempty.txt"
if run_recipe nonempty public-api-check "${CHECK_NONEMPTY_OUTPUT}"; then
  check_nonempty_code=0
else
  check_nonempty_code=$?
fi
if [[ "${check_nonempty_code}" -eq 0 ]]; then
  pass "non-empty generation reaches the normal comparison path"
else
  fail "non-empty comparison unexpectedly failed (exit ${check_nonempty_code})"
fi
assert_contains "normal comparison reports the expected crate" \
  "OK perl-lsp-rs" "${CHECK_NONEMPTY_OUTPUT}"

echo "=== public-api ratchet regression fixture passed ==="
