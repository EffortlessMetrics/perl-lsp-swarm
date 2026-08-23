#!/usr/bin/env bash
# Actual Neovim+nvim-dap+perl-dap native-preview receipt for #7773.
#
# Environment:
#   PERL_DAP      exact perl-dap executable path (required)
#   NVIM_DAP_RTP  exact nvim-dap checkout/runtimepath (required)
#   NEOVIM        nvim executable override (defaults to nvim)
#
# This script does not install nvim-dap or publish/debug-package artifacts. A
# missing/inexact client checkout or adapter binary is NOT_PROVEN; protocol/
# runtime failures after both subjects are selected are hard failures.

set -euo pipefail

invocation_cwd=$(pwd)
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
perl_dap_bin=${PERL_DAP:-${1:-}}
nvim_dap_rtp=${NVIM_DAP_RTP:-${2:-}}
nvim_bin=${NEOVIM:-nvim}

if [[ -z "${perl_dap_bin}" || -z "${nvim_dap_rtp}" ]]; then
  echo "NOT_PROVEN: PERL_DAP and NVIM_DAP_RTP are required" >&2
  exit 1
fi
if [[ "${perl_dap_bin}" != /* ]]; then
  perl_dap_bin="${invocation_cwd}/${perl_dap_bin}"
fi
if [[ "${nvim_dap_rtp}" != /* ]]; then
  nvim_dap_rtp="${invocation_cwd}/${nvim_dap_rtp}"
fi
if ! command -v "${nvim_bin}" >/dev/null 2>&1; then
  echo "NOT_PROVEN: nvim not found" >&2
  exit 1
fi
# The receipt executes Lua through `nvim -l`; reject older hosts before the
# actual test so an unsupported client is NOT_PROVEN rather than a cryptic run.
probe_lua=$(mktemp)
printf '%s\n' 'return' >"${probe_lua}"
if ! "${nvim_bin}" --headless -u NONE -l "${probe_lua}" >/dev/null 2>&1; then
  rm -f "${probe_lua}"
  echo "NOT_PROVEN: nvim does not support the required '-l' Lua entrypoint" >&2
  exit 1
fi
rm -f "${probe_lua}"
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

# Bind the receipt to the exact plugin tree actually put on runtimepath. Git's
# upward repository discovery is not enough: the supplied directory itself must
# be the repository root and it must be clean, including untracked files.
if ! nvim_dap_git_root=$(git -C "${nvim_dap_rtp}" rev-parse --show-toplevel 2>/dev/null); then
  echo "NOT_PROVEN: NVIM_DAP_RTP must be an exact Git checkout" >&2
  exit 1
fi
nvim_dap_git_root=$(cd "${nvim_dap_git_root}" && pwd)
if [[ "${nvim_dap_git_root}" != "${nvim_dap_rtp}" ]]; then
  echo "NOT_PROVEN: NVIM_DAP_RTP is nested inside another Git repository" >&2
  echo "runtime path: ${nvim_dap_rtp}" >&2
  echo "git root:     ${nvim_dap_git_root}" >&2
  exit 1
fi
if [[ -n "$(git -C "${nvim_dap_rtp}" status --porcelain --untracked-files=all)" ]]; then
  echo "NOT_PROVEN: nvim-dap checkout is dirty; receipt identity would not match executed source" >&2
  exit 1
fi
nvim_dap_identity=$(git -C "${nvim_dap_rtp}" rev-parse HEAD)

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
