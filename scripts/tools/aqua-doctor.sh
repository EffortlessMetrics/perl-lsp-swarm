#!/usr/bin/env bash
# Validate and install the checked-in portable contract-tool set.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
AQUA_CONFIG_PATH="${AQUA_CONFIG:-${REPO_ROOT}/aqua.yaml}"
AQUA_BOOTSTRAP_VERSION="v2.57.0"

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
aqua -v

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

echo "portable toolchain: OK — Changie, actionlint, and Zizmor match aqua.yaml"
