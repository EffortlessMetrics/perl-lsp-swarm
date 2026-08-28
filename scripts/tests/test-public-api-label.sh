#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TMPDIR_BASE="$(mktemp -d)"
FAKE_BIN="${TMPDIR_BASE}/bin"
LOG="${TMPDIR_BASE}/gh.log"
STATE="${TMPDIR_BASE}/label.state"
EXPECTED_COLOR="0052cc"
EXPECTED_DESCRIPTION="Run public API surface validation"

cleanup() {
  rm -rf "${TMPDIR_BASE}"
}
trap cleanup EXIT

mkdir -p "${FAKE_BIN}"
cat > "${FAKE_BIN}/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

case "${1:-}:${2:-}" in
  auth:status)
    exit 0
    ;;
  label:list)
    case "${FAKE_GH_MODE:-existing}" in
      missing)
        exit 0
        ;;
      api-failure)
        printf 'simulated label API failure\n' >&2
        exit 41
        ;;
      malformed)
        printf 'simulated malformed label response\n' >&2
        exit 42
        ;;
      existing)
        # The existing label is intentionally stale; the script must edit it.
        printf 'ci:public-api\n'
        ;;
      *)
        exit 43
        ;;
    esac
    ;;
  label:edit)
    [[ "${3:-}" == "ci:public-api" ]] || exit 2
    [[ "${4:-}" == "--color" && "${5:-}" == "${EXPECTED_COLOR}" ]] || exit 3
    [[ "${6:-}" == "--description" && "${7:-}" == "${EXPECTED_DESCRIPTION}" ]] || exit 4
    printf '%s\n' "$*" >> "${FAKE_GH_LOG}"
    printf '%s\t%s\t%s\n' "${3}" "${5}" "${7}" > "${FAKE_GH_STATE}"
    ;;
  label:create)
    if [[ "${3:-}" == "ci:public-api" ]]; then
      [[ "${4:-}" == "--color" && "${5:-}" == "${EXPECTED_COLOR}" ]] || exit 6
      [[ "${6:-}" == "--description" && "${7:-}" == "${EXPECTED_DESCRIPTION}" ]] || exit 7
    fi
    printf '%s\n' "$*" >> "${FAKE_GH_LOG}"
    ;;
  *)
    printf 'unexpected gh invocation: %s\n' "$*" >&2
    exit 5
    ;;
esac
STUB
chmod +x "${FAKE_BIN}/gh"

PATH="${FAKE_BIN}:${PATH}" \
  FAKE_GH_LOG="${LOG}" \
  FAKE_GH_STATE="${STATE}" \
  EXPECTED_COLOR="${EXPECTED_COLOR}" \
  EXPECTED_DESCRIPTION="${EXPECTED_DESCRIPTION}" \
  bash "${REPO_ROOT}/scripts/gh/ensure-labels.sh" >/dev/null

grep -Fqx -- 'label edit ci:public-api --color 0052cc --description Run public API surface validation' "${LOG}"
grep -Fqx -- $'ci:public-api\t0052cc\tRun public API surface validation' "${STATE}"
if grep -Fq -- 'label create ci:public-api' "${LOG}"; then
  echo 'FAIL stale public API label was recreated instead of reconciled' >&2
  exit 1
fi

echo 'PASS stale public API label metadata is repaired with gh label edit'

rm -f "${LOG}" "${STATE}"
PATH="${FAKE_BIN}:${PATH}" \
  FAKE_GH_LOG="${LOG}" \
  FAKE_GH_STATE="${STATE}" \
  EXPECTED_COLOR="${EXPECTED_COLOR}" \
  EXPECTED_DESCRIPTION="${EXPECTED_DESCRIPTION}" \
  FAKE_GH_MODE=missing \
  bash "${REPO_ROOT}/scripts/gh/ensure-labels.sh" >/dev/null

grep -Fqx -- 'label create ci:public-api --color 0052cc --description Run public API surface validation' "${LOG}"
if [[ -e "${STATE}" ]]; then
  echo 'FAIL missing public API label unexpectedly used edit state' >&2
  exit 1
fi

echo 'PASS missing public API label is created with governed metadata'

for mode in api-failure malformed; do
  rm -f "${LOG}" "${STATE}"
  if PATH="${FAKE_BIN}:${PATH}" \
    FAKE_GH_LOG="${LOG}" \
    FAKE_GH_STATE="${STATE}" \
    FAKE_GH_MODE="${mode}" \
    bash "${REPO_ROOT}/scripts/gh/ensure-labels.sh" >/dev/null 2>&1; then
    echo "FAIL ${mode} unexpectedly allowed label reconciliation" >&2
    exit 1
  fi
  if [[ -e "${LOG}" || -e "${STATE}" ]]; then
    echo "FAIL ${mode} mutated label state after unavailable evidence" >&2
    exit 1
  fi
  echo "PASS ${mode} keeps label reconciliation fail-closed"
done

CATALOG="${TMPDIR_BASE}/ci-config.yml"
sed "s/0052cc/123abc/; s/Run public API surface validation/Catalog-owned public API label/" \
  "${REPO_ROOT}/.github/ci-config.yml" > "${CATALOG}"
EXPECTED_COLOR="123abc"
EXPECTED_DESCRIPTION="Catalog-owned public API label"
rm -f "${LOG}" "${STATE}"
PATH="${FAKE_BIN}:${PATH}" \
  FAKE_GH_LOG="${LOG}" \
  FAKE_GH_STATE="${STATE}" \
  EXPECTED_COLOR="${EXPECTED_COLOR}" \
  EXPECTED_DESCRIPTION="${EXPECTED_DESCRIPTION}" \
  CI_CONFIG_PATH="${CATALOG}" \
  bash "${REPO_ROOT}/scripts/gh/ensure-labels.sh" >/dev/null

grep -Fqx -- "label edit ci:public-api --color ${EXPECTED_COLOR} --description ${EXPECTED_DESCRIPTION}" "${LOG}"
echo 'PASS provisioning consumes catalog metadata instead of embedded literals'

for catalog_mode in missing malformed; do
  rm -f "${LOG}" "${STATE}"
  if [[ "${catalog_mode}" == "missing" ]]; then
    BAD_CATALOG="${TMPDIR_BASE}/missing-ci-config.yml"
    rm -f "${BAD_CATALOG}"
  else
    BAD_CATALOG="${TMPDIR_BASE}/malformed-ci-config.yml"
    printf '%s\n' 'labels:' '  ci:public-api: not-a-metadata-map' > "${BAD_CATALOG}"
  fi
  if PATH="${FAKE_BIN}:${PATH}" \
    FAKE_GH_LOG="${LOG}" \
    FAKE_GH_STATE="${STATE}" \
    EXPECTED_COLOR="0052cc" \
    EXPECTED_DESCRIPTION="Run public API surface validation" \
    CI_CONFIG_PATH="${BAD_CATALOG}" \
    bash "${REPO_ROOT}/scripts/gh/ensure-labels.sh" >/dev/null 2>&1; then
    echo "FAIL ${catalog_mode} catalog unexpectedly allowed label reconciliation" >&2
    exit 1
  fi
  if [[ -e "${LOG}" || -e "${STATE}" ]]; then
    echo "FAIL ${catalog_mode} catalog allowed label mutation" >&2
    exit 1
  fi
  echo "PASS ${catalog_mode} catalog keeps label reconciliation fail-closed"
done
