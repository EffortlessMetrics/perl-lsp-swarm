#!/usr/bin/env bash
# Public/install-channel Neovim first-mile receipt for issue #7770.
#
# This wrapper never downloads or publishes artifacts. The caller installs an
# exact public/package-manager perllsp into an isolated prefix, then supplies:
#
#   PERLLSP                 exact installed executable path
#   PERLLSP_INSTALL_SOURCE  e.g. release-archive, crates-io, homebrew, mason
#   PERLLSP_EXPECTED_SHA256 expected digest of the installed executable
#   PERLLSP_EXPECTED_VERSION optional substring required in `perllsp --version`
#   NEOVIM                  optional nvim executable override
#
# Missing identity metadata is NOT_PROVEN. A wrong binary/hash/version or a
# broken actual-Neovim journey is a hard failure.

set -euo pipefail

invocation_cwd=$(pwd)
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
perllsp_bin=${PERLLSP:-${1:-}}
install_source=${PERLLSP_INSTALL_SOURCE:-}
expected_sha256=${PERLLSP_EXPECTED_SHA256:-}
expected_version=${PERLLSP_EXPECTED_VERSION:-}
nvim_bin=${NEOVIM:-nvim}

if [[ -z "${perllsp_bin}" || -z "${install_source}" || -z "${expected_sha256}" ]]; then
  echo "NOT_PROVEN: PERLLSP, PERLLSP_INSTALL_SOURCE, and PERLLSP_EXPECTED_SHA256 are required" >&2
  exit 1
fi
if [[ "${perllsp_bin}" != /* ]]; then
  perllsp_bin="${invocation_cwd}/${perllsp_bin}"
fi
if ! command -v "${nvim_bin}" >/dev/null 2>&1; then
  echo "NOT_PROVEN: nvim not found (set NEOVIM=/path/to/nvim)" >&2
  exit 1
fi
if [[ ! -x "${perllsp_bin}" ]]; then
  echo "NOT_PROVEN: installed perllsp not executable at ${perllsp_bin}" >&2
  exit 1
fi

repo_root=$(cd "${repo_root}" && pwd)
perllsp_bin=$(cd "$(dirname "${perllsp_bin}")" && pwd)/$(basename "${perllsp_bin}")

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$1" | awk '{print $NF}'
  else
    echo "NOT_PROVEN: no sha256sum, shasum, or openssl available" >&2
    return 1
  fi
}

actual_sha256=$(sha256_file "${perllsp_bin}" | tr '[:upper:]' '[:lower:]')
expected_sha256_lower=$(printf '%s' "${expected_sha256}" | tr '[:upper:]' '[:lower:]')
if [[ "${actual_sha256}" != "${expected_sha256_lower}" ]]; then
  echo "installed perllsp SHA-256 mismatch" >&2
  echo "expected: ${expected_sha256}" >&2
  echo "actual:   ${actual_sha256}" >&2
  exit 2
fi

version_output=$("${perllsp_bin}" --version 2>&1)
if [[ -n "${expected_version}" && "${version_output}" != *"${expected_version}"* ]]; then
  echo "installed perllsp version mismatch" >&2
  echo "expected substring: ${expected_version}" >&2
  echo "actual: ${version_output}" >&2
  exit 2
fi

fixture_root=$(mktemp -d)
trap 'rm -rf "${fixture_root}"' EXIT
mkdir -p "${fixture_root}/lib"
touch "${fixture_root}/.perl-lsp.toml"

cat >"${fixture_root}/lib/App.pm" <<'EOF'
package App;
use strict;
use warnings;

my $broken = 41

sub value {
my $copy = $bro
return $copy;
}

1;
EOF

receipt="${fixture_root}/receipt.json"
if ! REPO_ROOT="${repo_root}" \
  FIXTURE_ROOT="${fixture_root}" \
  PERLLSP="${perllsp_bin}" \
  PERLLSP_INSTALL_SOURCE="${install_source}" \
  PERLLSP_ACTUAL_SHA256="${actual_sha256}" \
  PERLLSP_VERSION_OUTPUT="${version_output}" \
  "${nvim_bin}" --headless -u NONE -l \
    "${repo_root}/scripts/ux/neovim/neovim_installed_perllsp_smoke.lua" \
    >"${receipt}" 2>"${fixture_root}/nvim.err"; then
  echo "installed-perllsp Neovim smoke FAILED" >&2
  cat "${fixture_root}/nvim.err" >&2
  [[ -s "${receipt}" ]] && cat "${receipt}" >&2
  exit 2
fi

if [[ ! -s "${receipt}" ]]; then
  echo "installed-perllsp Neovim smoke FAILED: receipt missing" >&2
  cat "${fixture_root}/nvim.err" >&2
  exit 2
fi

cat "${receipt}"
