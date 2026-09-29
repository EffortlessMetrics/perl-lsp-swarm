#!/usr/bin/env bash
# Offline proof for the Aqua bootstrap identity (#15235).
#
# Discriminates: no Go/sumdb acquisition, reviewed checksum fail-closed,
# bounded same-identity retry, persistent failure is NOT PROVEN, and both
# CI consumers share this script. Does not download the live Aqua release.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALLER="$ROOT/scripts/tools/install-aqua.sh"
DOCTOR="$ROOT/scripts/tools/aqua-doctor.sh"
CI_WORKFLOW="$ROOT/.github/workflows/ci.yml"
PORTABLE_WORKFLOW="$ROOT/.github/workflows/portable-contract-tools.yml"
CHECKSUMS="$ROOT/scripts/tests/fixtures/aqua-bootstrap/aqua_2.57.0_checksums.txt"
DOCS="$ROOT/docs/how-to/PORTABLE_CONTRACT_TOOLS.md"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAIL=0
LAST_STATUS=0
LAST_OUTPUT=""

pass() {
    printf 'PASS  %s\n' "$1"
    PASS=$((PASS + 1))
}

fail_case() {
    printf 'FAIL  %s\n%s\n' "$1" "$2" >&2
    FAIL=$((FAIL + 1))
}

run_capture() {
    set +e
    LAST_OUTPUT="$("$@" 2>&1)"
    LAST_STATUS=$?
    set -e
}

assert_status() {
    local name="$1"
    local expected="$2"
    if [[ "${LAST_STATUS}" -eq "${expected}" ]]; then
        pass "${name}"
    else
        fail_case "${name}" "expected status ${expected}, got ${LAST_STATUS}: ${LAST_OUTPUT}"
    fi
}

assert_contains() {
    local name="$1"
    local needle="$2"
    if [[ "${LAST_OUTPUT}" == *"${needle}"* ]]; then
        pass "${name}"
    else
        fail_case "${name}" "expected output to contain '${needle}': ${LAST_OUTPUT}"
    fi
}

assert_lacks() {
    local name="$1"
    local needle="$2"
    if [[ "${LAST_OUTPUT}" != *"${needle}"* ]]; then
        pass "${name}"
    else
        fail_case "${name}" "output must not contain '${needle}': ${LAST_OUTPUT}"
    fi
}

file_lacks() {
    local name="$1"
    local file="$2"
    local needle="$3"
    if ! grep -q -- "${needle}" "${file}"; then
        pass "${name}"
    else
        fail_case "${name}" "${file} still contains '${needle}'"
    fi
}

file_contains() {
    local name="$1"
    local file="$2"
    local needle="$3"
    if grep -q -- "${needle}" "${file}"; then
        pass "${name}"
    else
        fail_case "${name}" "${file} does not contain '${needle}'"
    fi
}

PERL_LSP_AQUA_INSTALLER_LIBRARY_ONLY=1
# shellcheck disable=SC1090
source "${INSTALLER}"

printf '=== Aqua bootstrap identity (#15235) ===\n'

if [[ "$(aqua_bootstrap_version)" == "v2.57.0" ]]; then
    pass "bootstrap version is the reviewed Aqua pin"
else
    fail_case "bootstrap version is the reviewed Aqua pin" "got $(aqua_bootstrap_version)"
fi

run_capture aqua_asset_for_uname Linux x86_64
assert_status "linux amd64 selects the official linux asset" 0
assert_contains "linux amd64 asset name" "aqua_linux_amd64.tar.gz"

run_capture aqua_asset_for_uname Darwin arm64
assert_status "unsupported platform is NOT PROVEN" 2
assert_contains "unsupported platform names NOT PROVEN" "NOT PROVEN"
assert_contains "unsupported platform names Darwin" "Darwin/arm64"

run_capture aqua_sha256_for_asset aqua_linux_amd64.tar.gz
assert_status "reviewed linux amd64 checksum is available" 0
PINNED_SHA="${LAST_OUTPUT}"
OFFICIAL_SHA="$(awk '/ aqua_linux_amd64.tar.gz$/ {print $1; exit}' "${CHECKSUMS}")"
if [[ "${PINNED_SHA}" == "${OFFICIAL_SHA}" && "${#PINNED_SHA}" -eq 64 ]]; then
    pass "pin matches official aqua_2.57.0_checksums.txt linux amd64 row"
else
    fail_case "pin matches official aqua_2.57.0_checksums.txt linux amd64 row" \
        "pinned=${PINNED_SHA} official=${OFFICIAL_SHA}"
fi

if [[ "$(aqua_download_url aqua_linux_amd64.tar.gz)" == \
    "https://github.com/aquaproj/aqua/releases/download/v2.57.0/aqua_linux_amd64.tar.gz" ]]; then
    pass "download URL is the official v2.57.0 GitHub release asset"
else
    fail_case "download URL is the official v2.57.0 GitHub release asset" \
        "$(aqua_download_url aqua_linux_amd64.tar.gz)"
fi

GOOD_FILE="${TMP}/good.bin"
printf 'aqua-bootstrap-proof\n' >"${GOOD_FILE}"
GOOD_SHA="$(aqua_sha256_file "${GOOD_FILE}")"
run_capture aqua_verify_checksum "${GOOD_FILE}" "${GOOD_SHA}"
assert_status "matching checksum is accepted" 0

run_capture aqua_verify_checksum "${GOOD_FILE}" "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
assert_status "checksum mismatch fails closed" 2
assert_contains "checksum mismatch is NOT PROVEN" "NOT PROVEN"
assert_contains "checksum mismatch names the digest" "checksum mismatch"

run_capture aqua_verify_checksum "${TMP}/missing.bin" "${GOOD_SHA}"
assert_status "missing checksum subject fails closed" 2

run_capture aqua_verify_checksum "${GOOD_FILE}" "abc123"
assert_status "short checksum pin fails closed" 2

FAKE_AQUA="${TMP}/payload/aqua"
mkdir -p "${TMP}/payload"
cat >"${FAKE_AQUA}" <<'EOF'
#!/usr/bin/env bash
printf 'aqua version 2.57.0\n'
EOF
chmod +x "${FAKE_AQUA}"
ARCHIVE="${TMP}/aqua_linux_amd64.tar.gz"
tar -C "${TMP}/payload" -czf "${ARCHIVE}" aqua
EXTRACT_DIR="${TMP}/extract"
run_capture aqua_extract_and_install "${ARCHIVE}" "${EXTRACT_DIR}"
assert_status "archive member aqua is extracted" 0
run_capture aqua_require_version "${EXTRACT_DIR}/aqua" "v2.57.0"
assert_status "matching installed version is accepted" 0

cat >"${FAKE_AQUA}" <<'EOF'
#!/usr/bin/env bash
printf 'aqua version 9.9.9\n'
EOF
chmod +x "${FAKE_AQUA}"
WRONG_ARCHIVE="${TMP}/wrong.tar.gz"
tar -C "${TMP}/payload" -czf "${WRONG_ARCHIVE}" aqua
WRONG_DIR="${TMP}/wrong"
run_capture aqua_extract_and_install "${WRONG_ARCHIVE}" "${WRONG_DIR}"
run_capture aqua_require_version "${WRONG_DIR}/aqua" "v2.57.0"
assert_status "wrong installed version is NOT PROVEN" 2
assert_contains "wrong version names NOT PROVEN" "NOT PROVEN"

cat >"${FAKE_AQUA}" <<'EOF'
#!/usr/bin/env bash
printf 'aqua version 2.57.0\n'
exit 1
EOF
chmod +x "${FAKE_AQUA}"
FAILING_ARCHIVE="${TMP}/failing-version.tar.gz"
tar -C "${TMP}/payload" -czf "${FAILING_ARCHIVE}" aqua
FAILING_DIR="${TMP}/failing-version"
run_capture aqua_extract_and_install "${FAILING_ARCHIVE}" "${FAILING_DIR}"
run_capture aqua_require_version "${FAILING_DIR}/aqua" "v2.57.0"
assert_status "version command that exits non-zero is NOT PROVEN" 2
assert_contains "failed version check names NOT PROVEN" "NOT PROVEN"
assert_contains "failed version check names the exit status" "exit 1"

CURL_LOG="${TMP}/curl-log"
CURL_BIN="${TMP}/bin"
mkdir -p "${CURL_BIN}"
cat >"${CURL_BIN}/curl" <<EOF
#!/usr/bin/env bash
set -euo pipefail
echo "\$*" >>"${CURL_LOG}"
dest=""
url=""
while [[ \$# -gt 0 ]]; do
    case "\$1" in
        -o)
            dest="\$2"
            shift 2
            ;;
        --connect-timeout|--max-time)
            shift 2
            ;;
        -*)
            shift
            ;;
        *)
            url="\$1"
            shift
            ;;
    esac
done
count=0
if [[ -f "${CURL_LOG}" ]]; then
    count="\$(wc -l <"${CURL_LOG}")"
fi
if [[ "\${count}" -lt 3 ]]; then
    exit 22
fi
printf 'recovered-body\n' >"\${dest}"
EOF
chmod +x "${CURL_BIN}/curl"

export PATH="${CURL_BIN}:${PATH}"
export AQUA_INSTALL_MAX_ATTEMPTS=3
export AQUA_INSTALL_RETRY_SLEEP_SECONDS=0
: >"${CURL_LOG}"
FETCH_DEST="${TMP}/fetched.bin"
run_capture aqua_fetch_with_retry "https://example.test/aqua_linux_amd64.tar.gz" "${FETCH_DEST}"
assert_status "transient download recovers on the third same-identity attempt" 0
if [[ "$(cat "${FETCH_DEST}")" == "recovered-body" ]]; then
    pass "recovered download wrote the same destination"
else
    fail_case "recovered download wrote the same destination" "$(cat "${FETCH_DEST}")"
fi
ATTEMPTS="$(grep -c 'example.test/aqua_linux_amd64.tar.gz' "${CURL_LOG}" || true)"
if [[ "${ATTEMPTS}" -eq 3 ]]; then
    pass "retry uses the same URL three times"
else
    fail_case "retry uses the same URL three times" "attempts=${ATTEMPTS} log=$(cat "${CURL_LOG}")"
fi

cat >"${CURL_BIN}/curl" <<EOF
#!/usr/bin/env bash
echo "\$*" >>"${CURL_LOG}"
exit 22
EOF
chmod +x "${CURL_BIN}/curl"
: >"${CURL_LOG}"
run_capture aqua_fetch_with_retry "https://example.test/aqua_linux_amd64.tar.gz" "${FETCH_DEST}"
assert_status "persistent download failure is NOT PROVEN" 2
assert_contains "persistent download names NOT PROVEN" "NOT PROVEN"
assert_lacks "persistent download is not a clean result" "aqua bootstrap: OK"
ATTEMPTS="$(wc -l <"${CURL_LOG}")"
if [[ "${ATTEMPTS}" -eq 3 ]]; then
    pass "persistent failure is bounded to three attempts"
else
    fail_case "persistent failure is bounded to three attempts" "attempts=${ATTEMPTS}"
fi
if [[ ! -e "${FETCH_DEST}" ]]; then
    pass "failed download does not leave a dest artifact"
else
    fail_case "failed download does not leave a dest artifact" "dest still exists"
fi

run_capture aqua_parse_dest
assert_status "missing dest is NOT PROVEN" 2
assert_contains "missing dest names destination" "destination is required"

file_lacks "install script does not go-install Aqua" "${INSTALLER}" "go install github.com/aquaproj/aqua"
file_lacks "install script does not disable sumdb" "${INSTALLER}" "GOSUMDB"
file_lacks "CI contract job does not go-install Aqua" "${CI_WORKFLOW}" "go install github.com/aquaproj/aqua"
file_lacks "portable tools job does not go-install Aqua" "${PORTABLE_WORKFLOW}" "go install github.com/aquaproj/aqua"
file_contains "CI contract job uses the shared installer" "${CI_WORKFLOW}" "scripts/tools/install-aqua.sh"
file_contains "portable tools job uses the shared installer" "${PORTABLE_WORKFLOW}" "scripts/tools/install-aqua.sh"
file_contains "portable tools path filter watches ci.yml" "${PORTABLE_WORKFLOW}" ".github/workflows/ci.yml"
file_contains "doctor points at the shared installer" "${DOCTOR}" "scripts/tools/install-aqua.sh"
file_lacks "doctor no longer prescribes go install" "${DOCTOR}" "go install github.com/aquaproj/aqua"
file_contains "docs point at the shared installer" "${DOCS}" "scripts/tools/install-aqua.sh"
file_lacks "docs no longer prescribe go install" "${DOCS}" "go install github.com/aquaproj/aqua"

printf '\n%d passed, %d failed\n' "${PASS}" "${FAIL}"
if [[ "${FAIL}" -ne 0 ]]; then
    exit 1
fi
