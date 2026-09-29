#!/usr/bin/env bash
# Install the reviewed Aqua bootstrap binary from GitHub Releases.
#
# This is the one repository-controlled Aqua acquisition path. It does not use
# `go install`, the Go module proxy, or sum.golang.org (#15235). A transient
# download failure may retry the same asset identity; a checksum or version
# mismatch is NOT PROVEN and never becomes a clean result.

set -euo pipefail

AQUA_BOOTSTRAP_VERSION="v2.57.0"
AQUA_LINUX_AMD64_ASSET="aqua_linux_amd64.tar.gz"
# Official row from:
# https://github.com/aquaproj/aqua/releases/download/v2.57.0/aqua_2.57.0_checksums.txt
AQUA_LINUX_AMD64_SHA256="042146fec6b7aae6ed6123fd5033144744acaa9afa476dd9a47e2aad50d5d5c6"
AQUA_DOWNLOAD_BASE="https://github.com/aquaproj/aqua/releases/download/${AQUA_BOOTSTRAP_VERSION}"

AQUA_INSTALL_MAX_ATTEMPTS="${AQUA_INSTALL_MAX_ATTEMPTS:-3}"
AQUA_INSTALL_RETRY_SLEEP_SECONDS="${AQUA_INSTALL_RETRY_SLEEP_SECONDS:-2}"

aqua_bootstrap_version() {
    printf '%s\n' "${AQUA_BOOTSTRAP_VERSION}"
}

aqua_asset_for_uname() {
    local os="$1"
    local arch="$2"
    case "${os}:${arch}" in
        Linux:x86_64 | Linux:amd64)
            printf '%s\n' "${AQUA_LINUX_AMD64_ASSET}"
            ;;
        *)
            echo "portable toolchain: NOT PROVEN — unsupported Aqua bootstrap platform: ${os}/${arch}" >&2
            return 2
            ;;
    esac
}

aqua_sha256_for_asset() {
    local asset="$1"
    case "${asset}" in
        aqua_linux_amd64.tar.gz)
            printf '%s\n' "${AQUA_LINUX_AMD64_SHA256}"
            ;;
        *)
            echo "portable toolchain: NOT PROVEN — no reviewed checksum for ${asset}" >&2
            return 2
            ;;
    esac
}

aqua_download_url() {
    local asset="$1"
    printf '%s/%s\n' "${AQUA_DOWNLOAD_BASE}" "${asset}"
}

aqua_sha256_file() {
    local path="$1"
    local output
    if command -v sha256sum >/dev/null 2>&1; then
        output="$(sha256sum "${path}")"
    elif command -v shasum >/dev/null 2>&1; then
        output="$(shasum -a 256 "${path}")"
    else
        echo "portable toolchain: NOT PROVEN — no sha256sum or shasum available" >&2
        return 2
    fi
    printf '%s\n' "${output%% *}"
}

aqua_verify_checksum() {
    local file="$1"
    local expected="$2"
    local actual

    if [[ ! -f "${file}" ]]; then
        echo "portable toolchain: NOT PROVEN — checksum subject is missing: ${file}" >&2
        return 2
    fi
    if [[ ! "${expected}" =~ ^[0-9a-f]{64}$ ]]; then
        echo "portable toolchain: NOT PROVEN — expected checksum is not 64 hexadecimal characters" >&2
        return 2
    fi
    actual="$(aqua_sha256_file "${file}")"
    if [[ "${actual}" != "${expected}" ]]; then
        echo "portable toolchain: NOT PROVEN — Aqua checksum mismatch (expected ${expected}, got ${actual})" >&2
        return 2
    fi
}

aqua_fetch_with_retry() {
    local url="$1"
    local dest="$2"
    local attempt=1
    local max="${AQUA_INSTALL_MAX_ATTEMPTS}"
    local sleep_seconds="${AQUA_INSTALL_RETRY_SLEEP_SECONDS}"

    if [[ "${max}" -lt 1 ]]; then
        echo "portable toolchain: NOT PROVEN — AQUA_INSTALL_MAX_ATTEMPTS must be >= 1" >&2
        return 2
    fi

    while [[ "${attempt}" -le "${max}" ]]; do
        rm -f "${dest}"
        if curl -fsSL --connect-timeout 15 --max-time 120 "${url}" -o "${dest}"; then
            return 0
        fi
        echo "portable toolchain: Aqua download attempt ${attempt}/${max} failed: ${url}" >&2
        if [[ "${attempt}" -eq "${max}" ]]; then
            echo "portable toolchain: NOT PROVEN — Aqua download failed after ${max} attempts: ${url}" >&2
            rm -f "${dest}"
            return 2
        fi
        sleep "${sleep_seconds}"
        attempt=$((attempt + 1))
    done
}

aqua_extract_and_install() {
    local archive="$1"
    local dest_dir="$2"

    mkdir -p "${dest_dir}"
    if ! tar -xzf "${archive}" -C "${dest_dir}" aqua; then
        echo "portable toolchain: NOT PROVEN — Aqua archive did not contain an aqua binary" >&2
        return 2
    fi
    chmod +x "${dest_dir}/aqua"
}

aqua_require_version() {
    local aqua_bin="$1"
    local expected="$2"
    local output

    if [[ ! -x "${aqua_bin}" ]]; then
        echo "portable toolchain: NOT PROVEN — installed aqua is not executable: ${aqua_bin}" >&2
        return 2
    fi
    output="$("${aqua_bin}" version 2>&1 || true)"
    printf '%s\n' "${output}"
    if [[ "${output}" != *"${expected#v}"* ]]; then
        echo "portable toolchain: NOT PROVEN — installed aqua version did not contain ${expected}: ${output}" >&2
        return 2
    fi
}

aqua_usage() {
    cat <<EOF
Usage: bash scripts/tools/install-aqua.sh --dest DIR

Install Aqua ${AQUA_BOOTSTRAP_VERSION} from the official GitHub release asset
after verifying the reviewed SHA-256. Does not use go install or sumdb.
EOF
}

aqua_parse_dest() {
    local dest="${AQUA_DEST:-}"
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --dest)
                if [[ $# -lt 2 ]]; then
                    echo "portable toolchain: NOT PROVEN — --dest requires a directory" >&2
                    return 2
                fi
                dest="$2"
                shift 2
                ;;
            *)
                echo "portable toolchain: NOT PROVEN — unknown argument: $1" >&2
                aqua_usage >&2
                return 2
                ;;
        esac
    done
    if [[ -z "${dest}" ]]; then
        echo "portable toolchain: NOT PROVEN — destination is required (--dest DIR)" >&2
        return 2
    fi
    printf '%s\n' "${dest}"
}

aqua_install() {
    local dest_dir="$1"
    local os
    local arch
    local asset
    local expected
    local url
    local work
    local archive

    os="$(uname -s)"
    arch="$(uname -m)"
    asset="$(aqua_asset_for_uname "${os}" "${arch}")"
    expected="$(aqua_sha256_for_asset "${asset}")"
    url="$(aqua_download_url "${asset}")"

    work="$(mktemp -d)"
    archive="${work}/${asset}"
    # shellcheck disable=SC2064
    trap "rm -rf '${work}'" EXIT

    echo "installing Aqua ${AQUA_BOOTSTRAP_VERSION} from ${url}"
    aqua_fetch_with_retry "${url}" "${archive}"
    aqua_verify_checksum "${archive}" "${expected}"
    aqua_extract_and_install "${archive}" "${dest_dir}"
    aqua_require_version "${dest_dir}/aqua" "${AQUA_BOOTSTRAP_VERSION}"
    echo "aqua bootstrap: OK — ${dest_dir}/aqua is ${AQUA_BOOTSTRAP_VERSION}"
}

aqua_main() {
    local dest
    case "${1:-}" in
        --help | -h)
            aqua_usage
            return 0
            ;;
    esac
    dest="$(aqua_parse_dest "$@")" || return "$?"
    aqua_install "${dest}"
}

if [[ "${PERL_LSP_AQUA_INSTALLER_LIBRARY_ONLY:-}" == "1" ]]; then
    return 0 2>/dev/null || exit 0
fi

aqua_main "$@"
