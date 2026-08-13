#!/usr/bin/env bash
# Actual Neovim virtual-perldoc receipt for #7764.
#
# This probe intentionally exits 0 with result=upstream_dependency when stock
# Neovim does not yet advertise/consume workspace/textDocumentContent. Server
# failure, wrong content, or a broken advertised client path remains a hard fail.

set -euo pipefail

invocation_cwd=$(pwd)
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
perllsp_bin=${PERLLSP:-${1:-"${repo_root}/target/release/perllsp"}}
nvim_bin=${NEOVIM:-nvim}

if [[ "${perllsp_bin}" != /* ]]; then
  perllsp_bin="${invocation_cwd}/${perllsp_bin}"
fi

if ! command -v "${nvim_bin}" >/dev/null 2>&1; then
  echo "NOT_PROVEN: nvim not found (set NEOVIM=/path/to/nvim)" >&2
  exit 1
fi
if [[ ! -x "${perllsp_bin}" ]]; then
  echo "NOT_PROVEN: perllsp not executable at ${perllsp_bin}" >&2
  exit 1
fi

repo_root=$(cd "${repo_root}" && pwd)
perllsp_bin=$(cd "$(dirname "${perllsp_bin}")" && pwd)/$(basename "${perllsp_bin}")
fixture_root=$(mktemp -d)
trap 'rm -rf "${fixture_root}"' EXIT

mkdir -p "${fixture_root}/lib/Local"
touch "${fixture_root}/.perl-lsp.toml"

cat >"${fixture_root}/main.pl" <<'EOF'
use strict;
use warnings;
use Local::Doc;
print Local::Doc::value();
EOF

cat >"${fixture_root}/lib/Local/Doc.pm" <<'EOF'
package Local::Doc;
use strict;

=head1 NAME

Local::Doc - workspace POD marker

=head1 DESCRIPTION

workspace POD marker

=cut

sub value { 42 }
1;
EOF

receipt="${fixture_root}/receipt.json"
if ! REPO_ROOT="${repo_root}" \
  FIXTURE_ROOT="${fixture_root}" \
  PERLLSP="${perllsp_bin}" \
  "${nvim_bin}" --headless -u NONE -l \
    "${repo_root}/scripts/ux/neovim/neovim_perldoc_virtual_document_smoke.lua" \
    >"${receipt}" 2>"${fixture_root}/nvim.err"; then
  echo "neovim virtual-perldoc smoke FAILED" >&2
  cat "${fixture_root}/nvim.err" >&2
  [[ -s "${receipt}" ]] && cat "${receipt}" >&2
  exit 2
fi

if [[ ! -s "${receipt}" ]]; then
  echo "neovim virtual-perldoc smoke FAILED: receipt missing" >&2
  cat "${fixture_root}/nvim.err" >&2
  exit 2
fi

cat "${receipt}"
