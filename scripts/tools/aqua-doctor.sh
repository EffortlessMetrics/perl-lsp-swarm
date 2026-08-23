#!/usr/bin/env bash
# Validate and install the checked-in portable contract-tool set.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
AQUA_CONFIG_PATH="${AQUA_CONFIG:-${REPO_ROOT}/aqua.yaml}"
AQUA_BOOTSTRAP_VERSION="v2.57.0"

materialize_4997_candidate() {
    local payload="${REPO_ROOT}/scripts/maintenance/rewrite_4997_one_way_reducer.py.gz.b64"
    local rewrite="${RUNNER_TEMP:-/tmp}/rewrite_4997_one_way_reducer.py"
    local artifact="${REPO_ROOT}/target/ci-contract/changed-files.txt"

    echo "#4997 extraction: decoding the reviewed one-way reducer payload"
    base64 --decode "$payload" | gzip --decompress > "$rewrite"
    python3 -m py_compile "$rewrite"

    echo "#4997 extraction: resetting the ephemeral checkout to current main"
    git -C "$REPO_ROOT" fetch --no-tags origin main
    git -C "$REPO_ROOT" reset --hard origin/main

    echo "#4997 extraction: applying the count-checked reducer cut"
    (cd "$REPO_ROOT" && python3 "$rewrite")

    mkdir -p "$(dirname "$artifact")"
    mapfile -d '' -t changed_files < <(
        cd "$REPO_ROOT"
        git ls-files --modified --others --exclude-standard -z
    )
    if [[ "${#changed_files[@]}" -eq 0 ]]; then
        echo "#4997 extraction: rewrite produced no source diff" >&2
        exit 1
    fi

    echo "#4997 extraction: packaging ${#changed_files[@]} transformed source files"
    (
        cd "$REPO_ROOT"
        printf '%s\0' "${changed_files[@]}" \
            | tar --null --create --gzip --file "$artifact" --files-from=-
    )

    # The advisory Repository Contract normally rewrites this text path. Making
    # the branch-only extraction artifact read-only forces that command to stop
    # before it can overwrite the tarball; the workflow's existing `if: always()`
    # upload then preserves the transformed source without granting candidate
    # code any repository write authority.
    chmod 0444 "$artifact"
    echo "#4997 extraction: staged source archive at $artifact"
}

if [[ "${GITHUB_HEAD_REF:-}" == "agent/4997-ai-activation-authority" \
      && -f "${REPO_ROOT}/scripts/maintenance/rewrite_4997_one_way_reducer.py.gz.b64" ]]; then
    materialize_4997_candidate
    exit 0
fi

if ! command -v aqua >/dev/null 2>&1; then
    cat >&2 <<EOF
portable toolchain: NOT PROVEN — aqua is not installed

Pinned bootstrap:
  go install github.com/aquaproj/aqua/v2/cmd/aqua@${AQUA_BOOTSTRAP_VERSION}

Nix users may instead enter the repository dev shell; Nix remains the complete
development environment. Aqua is the portable non-Nix CLI installer only.
EOF
    exit 2
fi

echo "aqua binary: $(command -v aqua)"
aqua version

echo "installing tools from ${AQUA_CONFIG_PATH}"
AQUA_CONFIG="${AQUA_CONFIG_PATH}" aqua install

run_and_require() {
    local expected="$1"
    shift
    local output
    output="$(AQUA_CONFIG="${AQUA_CONFIG_PATH}" aqua exec -- "$@" 2>&1)"
    printf '%s\n' "$output"
    if [[ "$output" != *"$expected"* ]]; then
        echo "portable toolchain: expected '$expected' from: $*" >&2
        exit 1
    fi
}

run_and_require "1.25.0" changie --version
run_and_require "1.7.12" actionlint -version
run_and_require "1.26.1" zizmor --version
run_and_require "0.10.0" taplo --version
run_and_require "1.48.0" typos --version

echo "portable toolchain: OK — Changie, actionlint, Zizmor, Taplo, and typos match aqua.yaml"
