#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TMPDIR_BASE="$(mktemp -d)"
FIXTURE_ROOT="${TMPDIR_BASE}/fixture"
FAKE_BIN="${TMPDIR_BASE}/bin"

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

assert_not_contains() {
  local label="$1"
  local needle="$2"
  local haystack="$3"
  if grep -Fq -- "${needle}" "${haystack}"; then
    fail "${label} (unexpected '${needle}')"
  else
    pass "${label}"
  fi
}

assert_exact_invocations() {
  local label="$1"
  local log_path="$2"
  local expected_path="$3"
  local mode="$4"
  shift 4

  : > "${expected_path}"
  for crate in "$@"; do
    printf '%s\tpublic-api -p %s --simplified\n' "${mode}" "${crate}" >> "${expected_path}"
  done

  if cmp -s "${expected_path}" "${log_path}"; then
    pass "${label}"
  else
    diff -u "${expected_path}" "${log_path}" >&2 || true
    fail "${label} (unexpected generator invocations)"
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

printf '%s\t%s\n' "${FAKE_PUBLIC_API_MODE:-unset}" "$*" >> "${FAKE_PUBLIC_API_LOG}"
case "${FAKE_PUBLIC_API_MODE:-}" in
  empty)
    exit 0
    ;;
  failure)
    printf 'simulated generator failure\n' >&2
    exit 42
    ;;
  mismatch)
    printf 'pub struct DifferentSurface;\n'
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
  local log_path="$4"
  local code=0

  : > "${log_path}"

  (
    cd "${FIXTURE_ROOT}"
    PATH="${FAKE_BIN}:${PATH}" \
      FAKE_PUBLIC_API_MODE="${mode}" \
      FAKE_PUBLIC_API_LOG="${log_path}" \
      just --justfile "${FIXTURE_ROOT}/justfile" "${recipe}"
  ) > "${output_path}" 2>&1 || code=$?
  return "${code}"
}

echo "=== public-api ratchet executable regression fixture ==="

mkdir -p "${FIXTURE_ROOT}/scripts" "${FIXTURE_ROOT}/.ci/public-api-baselines"
cp "${REPO_ROOT}/justfile" "${FIXTURE_ROOT}/justfile"
write_fake_cargo_safe

# The recipes read their crate set from the fixture's own ratchet list
# (#14607), so the invocation assertions below prove the recipes are
# list-driven rather than restating a hard-coded facade set: the real
# repository list is longer than this fixture's.
write_ratchet_list() {
  {
    printf '# fixture ratchet list\n\n'
    for crate in "$@"; do
      printf '  %s  # trailing comment\n' "${crate}"
    done
  } > "${FIXTURE_ROOT}/.ci/public-api-baselines/ratchet-crates.txt"
}
write_ratchet_list perl-lsp-rs perl-parser perl-uri perl-dap perllsp

for crate in perl-lsp-rs perl-parser perl-uri perl-dap perllsp; do
  printf 'pub struct ExistingSurface;\n' \
    > "${FIXTURE_ROOT}/.ci/public-api-baselines/${crate}.txt"
done

CHECK_EMPTY_OUTPUT="${TMPDIR_BASE}/check-empty.txt"
CHECK_EMPTY_LOG="${TMPDIR_BASE}/check-empty.log"
CHECK_EMPTY_EXPECTED="${TMPDIR_BASE}/check-empty.expected"
if run_recipe empty public-api-check "${CHECK_EMPTY_OUTPUT}" "${CHECK_EMPTY_LOG}"; then
  check_empty_code=0
else
  check_empty_code=$?
fi
assert_exit_nonzero "successful empty generation fails public-api-check closed" "${check_empty_code}"
assert_contains "check classifies empty generation as instrument failure" \
  "generated API surface is empty" "${CHECK_EMPTY_OUTPUT}"
assert_not_contains "empty generation is not classified as an API diff" \
  "FAIL Public API changed" "${CHECK_EMPTY_OUTPUT}"
assert_exact_invocations "empty check invokes the generator once per facade with exact arguments" \
  "${CHECK_EMPTY_LOG}" "${CHECK_EMPTY_EXPECTED}" empty \
  perl-lsp-rs perl-parser perl-uri perl-dap perllsp

BASELINE_BEFORE="${TMPDIR_BASE}/perl-lsp-rs-before.txt"
cp "${FIXTURE_ROOT}/.ci/public-api-baselines/perl-lsp-rs.txt" "${BASELINE_BEFORE}"

CHECK_FAILURE_OUTPUT="${TMPDIR_BASE}/check-failure.txt"
CHECK_FAILURE_LOG="${TMPDIR_BASE}/check-failure.log"
CHECK_FAILURE_EXPECTED="${TMPDIR_BASE}/check-failure.expected"
if run_recipe failure public-api-check "${CHECK_FAILURE_OUTPUT}" "${CHECK_FAILURE_LOG}"; then
  check_failure_code=0
else
  check_failure_code=$?
fi
assert_exit_nonzero "generator command failure fails public-api-check closed" "${check_failure_code}"
assert_contains "check classifies generator failure as instrument failure" \
  "INSTRUMENT-FAIL perl-lsp-rs: cargo public-api failed" "${CHECK_FAILURE_OUTPUT}"
assert_not_contains "generator failure is not classified as an API diff" \
  "FAIL Public API changed" "${CHECK_FAILURE_OUTPUT}"
assert_exact_invocations "failed check invokes every facade once with exact arguments" \
  "${CHECK_FAILURE_LOG}" "${CHECK_FAILURE_EXPECTED}" failure \
  perl-lsp-rs perl-parser perl-uri perl-dap perllsp

UPDATE_FAILURE_OUTPUT="${TMPDIR_BASE}/update-failure.txt"
UPDATE_FAILURE_LOG="${TMPDIR_BASE}/update-failure.log"
UPDATE_FAILURE_EXPECTED="${TMPDIR_BASE}/update-failure.expected"
if run_recipe failure public-api-update "${UPDATE_FAILURE_OUTPUT}" "${UPDATE_FAILURE_LOG}"; then
  update_failure_code=0
else
  update_failure_code=$?
fi
assert_exit_nonzero "generator command failure refuses baseline update" "${update_failure_code}"
assert_contains "generator failure reports baseline preservation refusal" \
  "cargo public-api failed; refusing to overwrite the baseline" "${UPDATE_FAILURE_OUTPUT}"
assert_same_file "generator failure preserves the existing baseline" \
  "${BASELINE_BEFORE}" "${FIXTURE_ROOT}/.ci/public-api-baselines/perl-lsp-rs.txt"
assert_exact_invocations "failed update invokes only the first facade with exact arguments" \
  "${UPDATE_FAILURE_LOG}" "${UPDATE_FAILURE_EXPECTED}" failure perl-lsp-rs

UPDATE_EMPTY_OUTPUT="${TMPDIR_BASE}/update-empty.txt"
UPDATE_EMPTY_LOG="${TMPDIR_BASE}/update-empty.log"
UPDATE_EMPTY_EXPECTED="${TMPDIR_BASE}/update-empty.expected"
if run_recipe empty public-api-update "${UPDATE_EMPTY_OUTPUT}" "${UPDATE_EMPTY_LOG}"; then
  update_empty_code=0
else
  update_empty_code=$?
fi
assert_exit_nonzero "successful empty generation refuses baseline update" "${update_empty_code}"
assert_contains "update reports baseline preservation refusal" \
  "refusing to overwrite the baseline" "${UPDATE_EMPTY_OUTPUT}"
assert_same_file "empty generation preserves the existing baseline" \
  "${BASELINE_BEFORE}" "${FIXTURE_ROOT}/.ci/public-api-baselines/perl-lsp-rs.txt"
assert_exact_invocations "empty update invokes only the first facade with exact arguments" \
  "${UPDATE_EMPTY_LOG}" "${UPDATE_EMPTY_EXPECTED}" empty perl-lsp-rs

CHECK_MISMATCH_OUTPUT="${TMPDIR_BASE}/check-mismatch.txt"
CHECK_MISMATCH_LOG="${TMPDIR_BASE}/check-mismatch.log"
CHECK_MISMATCH_EXPECTED="${TMPDIR_BASE}/check-mismatch.expected"
if run_recipe mismatch public-api-check "${CHECK_MISMATCH_OUTPUT}" "${CHECK_MISMATCH_LOG}"; then
  check_mismatch_code=0
else
  check_mismatch_code=$?
fi
assert_exit_nonzero "mismatched non-empty generation fails public-api-check" "${check_mismatch_code}"
assert_contains "mismatched non-empty generation is classified as an API diff" \
  "FAIL Public API changed in perl-lsp-rs" "${CHECK_MISMATCH_OUTPUT}"
assert_not_contains "mismatched non-empty generation is not an instrument failure" \
  "INSTRUMENT-FAIL" "${CHECK_MISMATCH_OUTPUT}"
assert_exact_invocations "mismatch check invokes the generator once per facade with exact arguments" \
  "${CHECK_MISMATCH_LOG}" "${CHECK_MISMATCH_EXPECTED}" mismatch \
  perl-lsp-rs perl-parser perl-uri perl-dap perllsp

UPDATE_NONEMPTY_OUTPUT="${TMPDIR_BASE}/update-nonempty.txt"
UPDATE_NONEMPTY_LOG="${TMPDIR_BASE}/update-nonempty.log"
UPDATE_NONEMPTY_EXPECTED="${TMPDIR_BASE}/update-nonempty.expected"
if run_recipe nonempty public-api-update "${UPDATE_NONEMPTY_OUTPUT}" "${UPDATE_NONEMPTY_LOG}"; then
  update_nonempty_code=0
else
  update_nonempty_code=$?
fi
if [[ "${update_nonempty_code}" -eq 0 ]]; then
  pass "non-empty generation reaches the normal update path"
else
  fail "non-empty generation unexpectedly failed (exit ${update_nonempty_code})"
fi
assert_exact_invocations "non-empty update invokes the generator once per facade with exact arguments" \
  "${UPDATE_NONEMPTY_LOG}" "${UPDATE_NONEMPTY_EXPECTED}" nonempty \
  perl-lsp-rs perl-parser perl-uri perl-dap perllsp

CHECK_NONEMPTY_OUTPUT="${TMPDIR_BASE}/check-nonempty.txt"
CHECK_NONEMPTY_LOG="${TMPDIR_BASE}/check-nonempty.log"
CHECK_NONEMPTY_EXPECTED="${TMPDIR_BASE}/check-nonempty.expected"
if run_recipe nonempty public-api-check "${CHECK_NONEMPTY_OUTPUT}" "${CHECK_NONEMPTY_LOG}"; then
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
assert_exact_invocations "non-empty check invokes the generator once per facade with exact arguments" \
  "${CHECK_NONEMPTY_LOG}" "${CHECK_NONEMPTY_EXPECTED}" nonempty \
  perl-lsp-rs perl-parser perl-uri perl-dap perllsp

# List-driven proof (#14607): shrinking the fixture list shrinks the crate
# set both recipes act on, and a listed crate without a baseline fails the
# check closed instead of being skipped.
write_ratchet_list perl-uri
CHECK_LIST_OUTPUT="${TMPDIR_BASE}/check-list.txt"
CHECK_LIST_LOG="${TMPDIR_BASE}/check-list.log"
CHECK_LIST_EXPECTED="${TMPDIR_BASE}/check-list.expected"
if run_recipe nonempty public-api-check "${CHECK_LIST_OUTPUT}" "${CHECK_LIST_LOG}"; then
  check_list_code=0
else
  check_list_code=$?
fi
if [[ "${check_list_code}" -eq 0 ]]; then
  pass "single-entry ratchet list checks cleanly"
else
  fail "single-entry ratchet list unexpectedly failed (exit ${check_list_code})"
fi
assert_exact_invocations "check invokes only the listed crate" \
  "${CHECK_LIST_LOG}" "${CHECK_LIST_EXPECTED}" nonempty perl-uri

UPDATE_LIST_OUTPUT="${TMPDIR_BASE}/update-list.txt"
UPDATE_LIST_LOG="${TMPDIR_BASE}/update-list.log"
UPDATE_LIST_EXPECTED="${TMPDIR_BASE}/update-list.expected"
if run_recipe nonempty public-api-update "${UPDATE_LIST_OUTPUT}" "${UPDATE_LIST_LOG}"; then
  update_list_code=0
else
  update_list_code=$?
fi
if [[ "${update_list_code}" -eq 0 ]]; then
  pass "single-entry ratchet list updates cleanly"
else
  fail "single-entry ratchet list update unexpectedly failed (exit ${update_list_code})"
fi
assert_exact_invocations "update regenerates only the listed crate" \
  "${UPDATE_LIST_LOG}" "${UPDATE_LIST_EXPECTED}" nonempty perl-uri

write_ratchet_list perl-uri perl-unbaselined
CHECK_MISSING_OUTPUT="${TMPDIR_BASE}/check-missing.txt"
CHECK_MISSING_LOG="${TMPDIR_BASE}/check-missing.log"
if run_recipe nonempty public-api-check "${CHECK_MISSING_OUTPUT}" "${CHECK_MISSING_LOG}"; then
  check_missing_code=0
else
  check_missing_code=$?
fi
assert_exit_nonzero "listed crate without a baseline fails public-api-check closed" "${check_missing_code}"
assert_contains "missing baseline is reported for the listed crate" \
  "FAIL Missing baseline: .ci/public-api-baselines/perl-unbaselined.txt" "${CHECK_MISSING_OUTPUT}"

echo "=== public-api ratchet regression fixture passed ==="
