#!/usr/bin/env bash
# Smallest real exact Vim + vim-lsp smoke for the #11380 specialized driver.
#
# Establishes that the specialized action adapter's selected public APIs are
# executable against the pinned subject: it runs one bounded session per mode
# (activation, save_format, freshness, recovery) and validates every emitted
# observation through the single classification authority:
#
#   cargo run --quiet -p xtask --locked -- \
#     check-vim-lsp-specialized-observations --file <jsonl>
#
# This is an API-executability smoke, not a behavior proof: first-class
# journeys remain successor host leaves (#11381/#11384/#11386/#11387/#11388),
# process supervision stays with #10894/#10944, and semantic expectations
# arrive with #11378. Missing subjects, an unpinned checkout, or any invalid
# observation fails closed.

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
. "${repo_root}/scripts/lib/cargo-toolchain-guard.sh"
cargo_toolchain_guard
# #11369: the pinned vim-lsp subject lives in one governed manifest.
subject_manifest="${repo_root}/.ci/editor-clients/vim-vim-lsp-subject.v1.json"
vim_bin=${VIM:-vim}
: "${VIM_LSP_DIR:?VIM_LSP_DIR must point at a pinned vim-lsp checkout}"
: "${PERLLSP:?PERLLSP must point at the exact perllsp candidate}"
adapter="${repo_root}/scripts/ux/vim_vim_lsp_specialized.vim"
out=${RECEIPT_DIR:-"${repo_root}/target/receipts/vim-vim-lsp-specialized"}
mkdir -p "${out}"

for required in "${adapter}" "${subject_manifest}" "${VIM_LSP_DIR}/plugin/lsp.vim"; do
  [[ -f ${required} ]] || { echo "specialized smoke FAILED: missing ${required}" >&2; exit 1; }
done
if ! command -v "${vim_bin}" >/dev/null 2>&1; then
  echo "specialized smoke FAILED: Vim executable not found: ${vim_bin}" >&2
  exit 1
fi
if ! command -v git >/dev/null 2>&1; then
  echo "specialized smoke FAILED: git is required to bind the vim-lsp checkout" >&2
  exit 1
fi
if ! command -v perl >/dev/null 2>&1; then
  echo "specialized smoke FAILED: perl is required for manifest binding" >&2
  exit 1
fi
if [[ ! -x ${PERLLSP} ]]; then
  echo "specialized smoke FAILED: perllsp is not executable: ${PERLLSP}" >&2
  exit 1
fi

expected_vim_lsp_ref=$(SUBJECT_MANIFEST="${subject_manifest}" perl -MJSON::PP -0777 -e '
  open my $fh, "<", $ENV{SUBJECT_MANIFEST} or die $!;
  my $subject = decode_json(<$fh>);
  die "subject manifest schema drift\n" unless $subject->{schema_version} eq "vim_lsp_subject.v1";
  my $commit = $subject->{upstream}{selected_commit};
  die "pinned vim-lsp commit missing\n" unless defined $commit && $commit =~ /^[0-9a-f]{40}$/;
  print $commit;
')
if [[ -z ${expected_vim_lsp_ref} ]]; then
  echo "specialized smoke FAILED: could not resolve the pinned commit" >&2
  exit 1
fi
actual_vim_lsp_ref=$(git -C "${VIM_LSP_DIR}" rev-parse HEAD)
if [[ ${actual_vim_lsp_ref} != "${expected_vim_lsp_ref}" ]]; then
  echo "specialized smoke FAILED: VIM_LSP_DIR HEAD ${actual_vim_lsp_ref} != pinned ${expected_vim_lsp_ref}" >&2
  exit 1
fi
if [[ -n $(git -C "${VIM_LSP_DIR}" status --porcelain) ]]; then
  echo "specialized smoke FAILED: VIM_LSP_DIR worktree is dirty" >&2
  exit 1
fi

adapter_sha="sha256:$(perl -MDigest::SHA -e 'my $f = shift; open my $fh, "<", $f or die $!; binmode $fh; print Digest::SHA::sha256_hex(<$fh>);' "${adapter}")"
if [[ ! ${adapter_sha} =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "specialized smoke FAILED: could not digest the adapter" >&2
  exit 1
fi

# Canonicalize caller-supplied subject paths before any directory change:
# a relative PERLLSP would otherwise be resolved against the fixture
# workspace when vim-lsp launches the server.
canonicalize() {
  local value=$1
  if [[ ${value} != /* ]]; then
    value="${repo_root}/${value}"
  fi
  printf '%s' "${value}"
}
VIM_LSP_DIR=$(canonicalize "${VIM_LSP_DIR}")
PERLLSP=$(canonicalize "${PERLLSP}")
tmpdir=$(mktemp -d)
trap '[[ -n ${tmpdir:-} && -d ${tmpdir} ]] && rm -rf "${tmpdir}"' EXIT
workspace="${tmpdir}/workspace"
mkdir -p "${workspace}/lib"
: >"${workspace}/.perl-lsp.toml"
cat >"${workspace}/lib/Widget.pm" <<'PERL'
package Widget;
use strict;
use warnings;

sub answer { return 42; }
sub greet { my ($self, $name) = @_; return "hello " . $name; }

1;
PERL
# Line 4 carries the Widget::answer discriminator for the semantic probe;
# the deliberately misaligned assignment gives the save-format owner real
# work if the server formats this fixture.
cat >"${workspace}/main.pl" <<'PERL'
use strict;
use warnings;
use lib 'lib';
use Widget;

my $value    =    Widget::answer();
my $greeting = Widget::greet('world');
print "$greeting $value\n";
PERL

run_mode() {
  local mode=$1
  local receipt="${out}/observations-${mode}.jsonl"
  local log="${tmpdir}/vim-lsp.${mode}.log"
  rm -f "${receipt}"
  echo "--- specialized mode: ${mode}"
  PERLLSP_VIM_WORKSPACE="${workspace}" \
  PERLLSP_VIM_BIN="${PERLLSP}" \
  PERLLSP_VIM_LSP_DIR="${VIM_LSP_DIR}" \
  PERLLSP_VIM_RECEIPT="${receipt}" \
  PERLLSP_VIM_LOG="${log}" \
  PERLLSP_VIM_SERVER_TRACE="${tmpdir}/perllsp-${mode}" \
  PERLLSP_VIM_MODE="${mode}" \
  PERLLSP_VIM_ADAPTER_SHA="${adapter_sha}" \
    "${vim_bin}" -Nu NONE -n -es -S "${adapter}" </dev/null
  if [[ ! -s ${receipt} ]]; then
    echo "specialized smoke FAILED: mode ${mode} produced no observations" >&2
    [[ -f ${log} ]] && tail -40 "${log}" >&2
    return 1
  fi
}

for mode in activation save_format freshness recovery; do
  if ! run_mode "${mode}"; then
    echo "specialized smoke FAILED in mode ${mode}" >&2
    exit 1
  fi
done

combined="${out}/observations.jsonl"
: >"${combined}"
cat "${out}"/observations-activation.jsonl \
    "${out}"/observations-save_format.jsonl \
    "${out}"/observations-freshness.jsonl \
    "${out}"/observations-recovery.jsonl >"${combined}"

echo "--- validating observations through the xtask classification authority"
cargo run --quiet -p xtask --locked -- \
  check-vim-lsp-specialized-observations --file "${combined}"

echo "specialized smoke OK: $(wc -l <"${combined}") validated observations in ${combined}"
