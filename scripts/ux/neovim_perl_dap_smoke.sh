#!/usr/bin/env bash
# Actual Neovim+nvim-dap+perl-dap native-preview receipt for #7773.
#
# Environment:
#   PERL_DAP      exact perl-dap executable path (required)
#   NVIM_DAP_RTP  exact nvim-dap checkout/runtimepath (required)
#   NEOVIM        nvim executable override (defaults to nvim)
#
# This script does not install nvim-dap or publish/debug-package artifacts. A
# missing client checkout or adapter binary is NOT_PROVEN; protocol/runtime
# failures after both subjects are selected are hard failures.

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
perl_dap_bin=${PERL_DAP:-${1:-}}
nvim_dap_rtp=${NVIM_DAP_RTP:-${2:-}}
nvim_bin=${NEOVIM:-nvim}

if [[ -z "${perl_dap_bin}" || -z "${nvim_dap_rtp}" ]]; then
  echo "NOT_PROVEN: PERL_DAP and NVIM_DAP_RTP are required" >&2
  exit 1
fi
if ! command -v "${nvim_bin}" >/dev/null 2>&1; then
  echo "NOT_PROVEN: nvim not found" >&2
  exit 1
fi
if [[ ! -x "${perl_dap_bin}" ]]; then
  echo "NOT_PROVEN: perl-dap not executable at ${perl_dap_bin}" >&2
  exit 1
fi
if [[ ! -f "${nvim_dap_rtp}/lua/dap.lua" ]]; then
  echo "NOT_PROVEN: NVIM_DAP_RTP does not look like an nvim-dap checkout: ${nvim_dap_rtp}" >&2
  exit 1
fi

repo_root=$(cd "${repo_root}" && pwd)
perl_dap_bin=$(cd "$(dirname "${perl_dap_bin}")" && pwd)/$(basename "${perl_dap_bin}")
nvim_dap_rtp=$(cd "${nvim_dap_rtp}" && pwd)

if git -C "${nvim_dap_rtp}" rev-parse HEAD >/dev/null 2>&1; then
  nvim_dap_identity=$(git -C "${nvim_dap_rtp}" rev-parse HEAD)
else
  nvim_dap_identity="unversioned:${nvim_dap_rtp}"
fi

fixture_root=$(mktemp -d)
trap 'rm -rf "${fixture_root}"' EXIT

cat >"${fixture_root}/debug_target.pl" <<'EOF'
use strict;
use warnings;

my $value = 41;
$value++;
print "$value\n";
EOF

receipt="${fixture_root}/receipt.json"
if ! REPO_ROOT="${repo_root}" \
  FIXTURE_ROOT="${fixture_root}" \
  PERL_DAP="${perl_dap_bin}" \
  NVIM_DAP_RTP="${nvim_dap_rtp}" \
  NVIM_DAP_IDENTITY="${nvim_dap_identity}" \
  "${nvim_bin}" --headless -u NONE -l \
    "${repo_root}/scripts/ux/neovim/neovim_perl_dap_smoke.lua" \
    >"${receipt}" 2>"${fixture_root}/nvim.err"; then
  echo "Neovim nvim-dap perl-dap smoke FAILED" >&2
  cat "${fixture_root}/nvim.err" >&2
  [[ -s "${receipt}" ]] && cat "${receipt}" >&2
  exit 2
fi

if [[ ! -s "${receipt}" ]]; then
  echo "Neovim nvim-dap perl-dap smoke FAILED: receipt missing" >&2
  cat "${fixture_root}/nvim.err" >&2
  exit 2
fi

cat "${receipt}"
