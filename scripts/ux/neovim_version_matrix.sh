#!/usr/bin/env bash
# Bounded actual-Neovim supported-version matrix for #7716.
#
# Required:
#   PERLLSP         exact candidate perllsp executable
#   NEOVIM_FLOOR    exact 0.11.x Neovim executable
#   NEOVIM_CURRENT  exact current-stable 0.12.x Neovim executable
#
# Optional:
#   NEOVIM_FLOOR_PREFIX   expected version prefix (default 0.11)
#   NEOVIM_CURRENT_PREFIX expected version prefix (default 0.12)
#
# Each row runs with isolated XDG state so LSP trace evidence cannot leak from
# another Neovim/version/candidate. Output is JSON Lines, one receipt per row.

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
perllsp_bin=${PERLLSP:-${1:-}}
nvim_floor=${NEOVIM_FLOOR:-${2:-}}
nvim_current=${NEOVIM_CURRENT:-${3:-}}
floor_prefix=${NEOVIM_FLOOR_PREFIX:-0.11}
current_prefix=${NEOVIM_CURRENT_PREFIX:-0.12}

if [[ -z "${perllsp_bin}" || -z "${nvim_floor}" || -z "${nvim_current}" ]]; then
  echo "NOT_PROVEN: PERLLSP, NEOVIM_FLOOR, and NEOVIM_CURRENT are required" >&2
  exit 1
fi
for executable in "${perllsp_bin}" "${nvim_floor}" "${nvim_current}"; do
  if [[ ! -x "${executable}" ]]; then
    echo "NOT_PROVEN: executable missing or not executable: ${executable}" >&2
    exit 1
  fi
done

repo_root=$(cd "${repo_root}" && pwd)
perllsp_bin=$(cd "$(dirname "${perllsp_bin}")" && pwd)/$(basename "${perllsp_bin}")
nvim_floor=$(cd "$(dirname "${nvim_floor}")" && pwd)/$(basename "${nvim_floor}")
nvim_current=$(cd "$(dirname "${nvim_current}")" && pwd)/$(basename "${nvim_current}")

fixture_root=$(mktemp -d)
trap 'rm -rf "${fixture_root}"' EXIT
mkdir -p "${fixture_root}/lib" "${fixture_root}/customlib/Custom"
touch "${fixture_root}/.perl-lsp.toml"

cat >"${fixture_root}/customlib/Custom/Thing.pm" <<'EOF'
package Custom::Thing;
use strict;
sub answer { 42 }
1;
EOF

cat >"${fixture_root}/lib/App.pm" <<'EOF'
package App;
use strict;
use warnings;
use Custom::Thing;

my $broken = 41

my $copy = $bro
sub value {
  return Custom::Thing::answer() + $broken;
}

1;
EOF

run_row() {
  local label=$1
  local nvim_bin=$2
  local expected_prefix=$3
  local row_state="${fixture_root}/xdg-${label}"
  mkdir -p \
    "${row_state}/config" \
    "${row_state}/data" \
    "${row_state}/state" \
    "${row_state}/cache"

  local receipt="${fixture_root}/${label}.json"
  local err="${fixture_root}/${label}.err"

  if ! XDG_CONFIG_HOME="${row_state}/config" \
    XDG_DATA_HOME="${row_state}/data" \
    XDG_STATE_HOME="${row_state}/state" \
    XDG_CACHE_HOME="${row_state}/cache" \
    REPO_ROOT="${repo_root}" \
    FIXTURE_ROOT="${fixture_root}" \
    PERLLSP="${perllsp_bin}" \
    NEOVIM_ROW_LABEL="${label}" \
    NEOVIM_EXPECTED_PREFIX="${expected_prefix}" \
    "${nvim_bin}" --headless -u NONE -l \
      "${repo_root}/scripts/ux/neovim/neovim_version_row.lua" \
      >"${receipt}" 2>"${err}"; then
    echo "Neovim compatibility row ${label} FAILED" >&2
    cat "${err}" >&2
    [[ -s "${receipt}" ]] && cat "${receipt}" >&2
    return 2
  fi

  if [[ ! -s "${receipt}" ]]; then
    echo "Neovim compatibility row ${label} FAILED: receipt missing" >&2
    cat "${err}" >&2
    return 2
  fi

  cat "${receipt}"
}

run_row support-floor "${nvim_floor}" "${floor_prefix}"
run_row current-stable "${nvim_current}" "${current_prefix}"
