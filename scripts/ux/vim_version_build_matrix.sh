#!/usr/bin/env bash
# Bounded actual-Vim compatibility replay for #7766.
#
# Reuses #7691's deep journey. This script does not create a second semantic
# harness; it binds exact Vim build/platform/binary evidence to each replay row.
# Missing required rows are failures, not skips.
#
# Required environment:
#   VIM_FLOOR=/path/to/selected-floor-vim
#   VIM_CURRENT=/path/to/current-vim-9.2
#   VIM_LSP_DIR=/path/to/pinned/vim-lsp
#   PERLLSP=/path/to/exact-source/perllsp
#
# Optional:
#   PUBLIC_PERLLSP=/path/to/release-shaped/perllsp
#   RECEIPT_DIR=/path/to/output-directory
#   EXPECT_INCREMENTAL=1

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
: "${VIM_FLOOR:?VIM_FLOOR must identify the selected support-floor Vim executable}"
: "${VIM_CURRENT:?VIM_CURRENT must identify a current Vim 9.2 executable}"
: "${VIM_LSP_DIR:?VIM_LSP_DIR must point at the pinned vim-lsp checkout}"
: "${PERLLSP:?PERLLSP must point at the exact-source perllsp candidate}"
expect_incremental=${EXPECT_INCREMENTAL:-0}

out=${RECEIPT_DIR:-"${repo_root}/target/receipts/vim-version-matrix"}
mkdir -p "${out}"

hash_file() {
  local path=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{print $1}'
  else
    echo "unavailable"
  fi
}

run_row() {
  local row=$1
  local vim_bin=$2
  local perllsp_bin=$3
  local stage=$4

  if [[ ! -x "${vim_bin}" ]] && ! command -v "${vim_bin}" >/dev/null 2>&1; then
    echo "vim matrix FAILED: ${row}: Vim executable missing: ${vim_bin}" >&2
    return 1
  fi
  if [[ ! -x "${perllsp_bin}" ]]; then
    echo "vim matrix FAILED: ${row}: perllsp executable missing: ${perllsp_bin}" >&2
    return 1
  fi

  local version_file="${out}/${row}.vim-version.txt"
  local receipt_file="${out}/${row}.journey.json"
  local meta_file="${out}/${row}.meta"

  "${vim_bin}" --version >"${version_file}"
  local vim_digest
  vim_digest=$(hash_file "${version_file}")
  local perllsp_digest
  perllsp_digest=$(hash_file "${perllsp_bin}")

  if [[ ${row} == linux-current-9.2* ]] && ! grep -Eq 'Vi IMproved 9\.2|VIM - Vi IMproved 9\.2' "${version_file}"; then
    echo "vim matrix FAILED: ${row}: VIM_CURRENT is not a Vim 9.2 build" >&2
    head -5 "${version_file}" >&2
    return 1
  fi

  VIM="${vim_bin}" \
  VIM_LSP_DIR="${VIM_LSP_DIR}" \
  PERLLSP="${perllsp_bin}" \
  RECEIPT="${receipt_file}" \
  EXPECT_INCREMENTAL="${expect_incremental}" \
    "${repo_root}/scripts/ux/vim_vim_lsp_smoke.sh"

  cat >"${meta_file}" <<EOF
schema_version=1
row=${row}
evidence_stage=${stage}
platform=$(uname -s 2>/dev/null || echo unknown)
architecture=$(uname -m 2>/dev/null || echo unknown)
vim_executable=${vim_bin}
vim_version_digest=${vim_digest}
vim_lsp_dir=${VIM_LSP_DIR}
perllsp_executable=${perllsp_bin}
perllsp_sha256=${perllsp_digest}
expect_incremental=${expect_incremental}
journey_receipt=${receipt_file}
EOF

  echo "vim matrix row OK: ${row} (${stage})"
}

run_row linux-support-floor "${VIM_FLOOR}" "${PERLLSP}" exact_source_local
run_row linux-current-9.2 "${VIM_CURRENT}" "${PERLLSP}" exact_source_local

if [[ -n ${PUBLIC_PERLLSP:-} ]]; then
  run_row linux-current-9.2-public-artifact "${VIM_CURRENT}" "${PUBLIC_PERLLSP}" public_artifact
else
  cat >"${out}/linux-current-9.2-public-artifact.meta" <<EOF
schema_version=1
row=linux-current-9.2-public-artifact
evidence_stage=public_artifact
state=not_proven
reason=PUBLIC_PERLLSP_not_supplied
EOF
fi

cat >"${out}/matrix-summary.txt" <<EOF
schema_version=1
contract=.ci/editor-clients/vim-version-build-platform-matrix.v1.json
support_floor=linux-support-floor
current=linux-current-9.2
public_artifact=$(if [[ -n ${PUBLIC_PERLLSP:-} ]]; then echo proven_by_run; else echo not_proven; fi)
windows=not_proven_until_direct_receipt
macos=not_proven_until_direct_receipt
EOF

cat "${out}/matrix-summary.txt"
