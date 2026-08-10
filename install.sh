#!/usr/bin/env bash
# Compatibility wrapper for the canonical Linux/macOS installer.
#
# Clone-local use executes the sibling scripts/install.sh directly.
# Remote/piped use is a non-authoritative convenience bootstrap: it requires an
# immutable commit identity plus the reviewed SHA-256 digest of that exact
# scripts/install.sh before any installer logic is executed. The wrapper itself
# must be fetched from the same commit SHA — release tags are not accepted
# because the piped wrapper bytes execute before any digest check can run.
#
# Example shape (replace placeholders with release-closeout values):
#   curl -fsSL https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/<commit>/install.sh \
#     | PERL_LSP_INSTALLER_REF=<commit> \
#       PERL_LSP_INSTALLER_SHA256=<sha256> bash

set -euo pipefail

fail() {
    echo "Error: $*" >&2
    exit 1
}

# `${BASH_SOURCE[0]:-}` — not a bare `${BASH_SOURCE[0]}`. When this script is
# read from stdin, BASH_SOURCE is an empty array under older Bash versions.
SCRIPT_SOURCE="${BASH_SOURCE[0]:-}"
CANONICAL_INSTALLER=""
if [ -n "$SCRIPT_SOURCE" ]; then
    SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_SOURCE")" 2>/dev/null && pwd || pwd)"
    CANONICAL_INSTALLER="$SCRIPT_DIR/scripts/install.sh"
fi

# Every expansion of ARGS below uses `${ARGS[@]+"${ARGS[@]}"}`. Under `set -u`,
# expanding an empty array as `"${arr[@]}"` is an unbound-variable error on
# Bash < 4.4, including the Bash 3.2 shipped by macOS.
ARGS=("$@")

if [ "${1:-}" != "" ] && [[ "${1:-}" != -* ]]; then
    if [ -z "${VERSION:-}" ]; then
        export VERSION="$1"
    fi
    shift

    if [ "${1:-}" != "" ] && [[ "${1:-}" != -* ]]; then
        if [ -z "${INSTALL_DIR:-}" ]; then
            export INSTALL_DIR="$1"
        fi
        shift
    fi

    ARGS=("$@")
fi

if [ -n "$CANONICAL_INSTALLER" ] && [ -f "$CANONICAL_INSTALLER" ]; then
    exec "$CANONICAL_INSTALLER" ${ARGS[@]+"${ARGS[@]}"}
fi

if ! command -v curl >/dev/null 2>&1; then
    fail "curl is required to fetch the canonical installer"
fi

INSTALLER_REF="${PERL_LSP_INSTALLER_REF:-}"
EXPECTED_SHA256="${PERL_LSP_INSTALLER_SHA256:-}"

if [ -z "$INSTALLER_REF" ]; then
    fail "remote bootstrap requires PERL_LSP_INSTALLER_REF (a full lowercase commit SHA)"
fi

# Only a full commit SHA is immutable for both the piped wrapper and the
# canonical installer fetch. Branch names, release tags, HEAD, slashes,
# whitespace, and shell-shaped values are rejected rather than interpreted as
# raw GitHub refs.
valid_ref=false
if [ "${#INSTALLER_REF}" -eq 40 ]; then
    case "$INSTALLER_REF" in
        *[!0-9a-f]*) ;;
        *) valid_ref=true ;;
    esac
fi

if [ "$valid_ref" != "true" ]; then
    fail "PERL_LSP_INSTALLER_REF must be a full lowercase commit SHA"
fi

if [ "${#EXPECTED_SHA256}" -ne 64 ]; then
    fail "PERL_LSP_INSTALLER_SHA256 must be exactly 64 lowercase hexadecimal characters"
fi
case "$EXPECTED_SHA256" in
    *[!0-9a-f]*)
        fail "PERL_LSP_INSTALLER_SHA256 must be exactly 64 lowercase hexadecimal characters"
        ;;
esac

TMP_INSTALLER="$(mktemp)"
trap 'rm -f "$TMP_INSTALLER"' EXIT HUP INT TERM
CANONICAL_INSTALLER_URL="https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/${INSTALLER_REF}/scripts/install.sh"

# Do not follow redirects. The selected repository/ref/path is part of the
# installer identity; a redirect is not an equivalent source.
HTTP_STATUS="$(
    curl \
        --proto '=https' \
        --silent \
        --show-error \
        --output "$TMP_INSTALLER" \
        --write-out '%{http_code}' \
        "$CANONICAL_INSTALLER_URL"
)" || fail "failed to fetch the canonical installer"

if [ "$HTTP_STATUS" != "200" ]; then
    fail "canonical installer request returned HTTP $HTTP_STATUS; redirects and non-success responses are rejected"
fi

ACTUAL_SHA256=""
if command -v sha256sum >/dev/null 2>&1; then
    SHA_OUTPUT="$(sha256sum "$TMP_INSTALLER")" || fail "sha256sum failed"
    ACTUAL_SHA256="${SHA_OUTPUT%% *}"
elif command -v shasum >/dev/null 2>&1; then
    SHA_OUTPUT="$(shasum -a 256 "$TMP_INSTALLER")" || fail "shasum failed"
    ACTUAL_SHA256="${SHA_OUTPUT%% *}"
else
    fail "sha256sum or shasum is required to verify the canonical installer"
fi

if [ "$ACTUAL_SHA256" != "$EXPECTED_SHA256" ]; then
    fail "canonical installer SHA-256 mismatch"
fi

# Do not exec here: returning through this shell guarantees the EXIT trap
# removes the verified temporary installer on both success and failure.
bash "$TMP_INSTALLER" ${ARGS[@]+"${ARGS[@]}"}
