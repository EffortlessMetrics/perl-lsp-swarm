#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TMPDIR_BASE="$(mktemp -d)"
FAKE_BIN="${TMPDIR_BASE}/bin"
LOG="${TMPDIR_BASE}/gh.log"
STATE="${TMPDIR_BASE}/label.state"

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
    # The existing label is intentionally stale; the script must edit it.
    printf 'ci:public-api\n'
    ;;
  label:edit)
    [[ "${3:-}" == "ci:public-api" ]] || exit 2
    [[ "${4:-}" == "--color" && "${5:-}" == "0052cc" ]] || exit 3
    [[ "${6:-}" == "--description" && "${7:-}" == "Run public API surface validation" ]] || exit 4
    printf '%s\n' "$*" >> "${FAKE_GH_LOG}"
    printf '%s\t%s\t%s\n' "${3}" "${5}" "${7}" > "${FAKE_GH_STATE}"
    ;;
  label:create)
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
  bash "${REPO_ROOT}/scripts/gh/ensure-labels.sh" >/dev/null

grep -Fqx -- 'label edit ci:public-api --color 0052cc --description Run public API surface validation' "${LOG}"
grep -Fqx -- $'ci:public-api\t0052cc\tRun public API surface validation' "${STATE}"
if grep -Fq -- 'label create ci:public-api' "${LOG}"; then
  echo 'FAIL stale public API label was recreated instead of reconciled' >&2
  exit 1
fi

echo 'PASS stale public API label metadata is repaired with gh label edit'
