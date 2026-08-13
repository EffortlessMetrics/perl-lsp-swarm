#!/usr/bin/env bash
# Pre-submit real-Vim verifier for the candidate mattn/vim-lsp-settings patch.
#
# This does NOT submit upstream or prove upstream availability. It overlays the
# candidate files into an exact upstream checkout, disables our manual
# lsp#register_server() path entirely, and requires vim-lsp-settings itself to
# discover and launch the exact perllsp candidate.
#
# Required:
#   VIM_LSP_DIR=/path/to/pinned/prabirshrestha/vim-lsp
#   VIM_LSP_SETTINGS_DIR=/path/to/pinned/mattn/vim-lsp-settings
#   PERLLSP=/path/to/exact/perllsp
#
# Optional:
#   VIM=/path/to/vim
#   RECEIPT=/path/to/receipt.json
#   ALLOW_UPSTREAM_DRIFT=1  # permit a newer checkout for adaptation work

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
vim_bin=${VIM:-vim}
: "${VIM_LSP_DIR:?VIM_LSP_DIR is required}"
: "${VIM_LSP_SETTINGS_DIR:?VIM_LSP_SETTINGS_DIR is required}"
: "${PERLLSP:?PERLLSP is required}"
expected_upstream=b0c9bacfe98ff6bc4c5f6b0fffdc085d252387e0
candidate_vim="${repo_root}/.ci/upstream/vim-lsp-settings/settings/perllsp.vim"
candidate_entry="${repo_root}/.ci/upstream/vim-lsp-settings/perllsp-settings-entry.json"

for required in \
  "${VIM_LSP_DIR}/plugin/lsp.vim" \
  "${VIM_LSP_SETTINGS_DIR}/plugin/lsp_settings.vim" \
  "${VIM_LSP_SETTINGS_DIR}/settings.json" \
  "${candidate_vim}" \
  "${candidate_entry}"; do
  [[ -f ${required} ]] || { echo "vim-lsp-settings smoke FAILED: missing ${required}" >&2; exit 1; }
done
[[ -x ${PERLLSP} ]] || { echo "vim-lsp-settings smoke FAILED: perllsp not executable" >&2; exit 1; }
command -v perl >/dev/null 2>&1 || { echo "vim-lsp-settings smoke FAILED: perl is required to patch JSON" >&2; exit 1; }

upstream_ref=unknown
if command -v git >/dev/null 2>&1 && [[ -d ${VIM_LSP_SETTINGS_DIR}/.git ]]; then
  upstream_ref=$(git -C "${VIM_LSP_SETTINGS_DIR}" rev-parse HEAD)
  if [[ ${ALLOW_UPSTREAM_DRIFT:-0} != 1 && ${upstream_ref} != ${expected_upstream} ]]; then
    echo "vim-lsp-settings smoke FAILED: expected upstream ${expected_upstream}, got ${upstream_ref}" >&2
    exit 1
  fi
fi

tmpdir=$(mktemp -d)
cleanup() { rm -rf "${tmpdir}"; }
trap cleanup EXIT
settings_copy="${tmpdir}/vim-lsp-settings"
cp -R "${VIM_LSP_SETTINGS_DIR}" "${settings_copy}"
cp "${candidate_vim}" "${settings_copy}/settings/perllsp.vim"

# Overlay the candidate registry entry into the pinned upstream settings.json.
ENTRY_PATH="${candidate_entry}" SETTINGS_PATH="${settings_copy}/settings.json" perl -MJSON::PP -0777 -e '
  my $entry_path = $ENV{ENTRY_PATH};
  my $settings_path = $ENV{SETTINGS_PATH};
  open my $efh, "<", $entry_path or die $!;
  my $wrapper = decode_json(do { local $/; <$efh> });
  close $efh;
  open my $sfh, "<", $settings_path or die $!;
  my $settings = decode_json(do { local $/; <$sfh> });
  close $sfh;
  my $lang = $wrapper->{language};
  $settings->{$lang} //= [];
  @{$settings->{$lang}} = grep { ($_->{command} // "") ne "perllsp" } @{$settings->{$lang}};
  unshift @{$settings->{$lang}}, $wrapper->{entry};
  open my $out, ">", $settings_path or die $!;
  print {$out} JSON::PP->new->canonical(1)->pretty(1)->encode($settings);
  close $out;
'

# Make PATH resolution exact without editing the upstream candidate entry.
mkdir -p "${tmpdir}/bin" "${tmpdir}/workspace/lib"
ln -s "${PERLLSP}" "${tmpdir}/bin/perllsp"
export PATH="${tmpdir}/bin:${PATH}"
: >"${tmpdir}/workspace/.perl-lsp.toml"
cat >"${tmpdir}/workspace/lib/Widget.pm" <<'PERL'
package Widget;
sub answer { 42 }
1;
PERL
cat >"${tmpdir}/workspace/main.pl" <<'PERL'
use strict;
use lib 'lib';
use Widget;
my $value = Widget::answer();
my $broken = $val
print $value;
PERL

receipt=${RECEIPT:-"${tmpdir}/vim-lsp-settings-receipt.json"}
export PERLLSP_SETTINGS_COPY="${settings_copy}"
export PERLLSP_VIM_LSP_DIR="${VIM_LSP_DIR}"
export PERLLSP_SETTINGS_WORKSPACE="${tmpdir}/workspace"
export PERLLSP_SETTINGS_RECEIPT="${receipt}"
export PERLLSP_SETTINGS_UPSTREAM_REF="${upstream_ref}"
export PERLLSP_SETTINGS_BIN="${PERLLSP}"

cat >"${tmpdir}/verify.vim" <<'VIM'
set nocompatible
set nomore
set hidden
filetype on
let s:lsp_dir = expand('$PERLLSP_VIM_LSP_DIR')
let s:settings_dir = expand('$PERLLSP_SETTINGS_COPY')
let s:workspace = expand('$PERLLSP_SETTINGS_WORKSPACE')
let s:receipt = expand('$PERLLSP_SETTINGS_RECEIPT')
let s:failures = []
let s:cells = {}

function! s:Fail(msg) abort
  call add(s:failures, a:msg)
endfunction
function! s:Wait(expr, timeout_ms) abort
  let l:start = reltime()
  while !eval(a:expr)
    if reltimefloat(reltime(l:start)) * 1000.0 > a:timeout_ms | return 0 | endif
    sleep 25m
  endwhile
  return 1
endfunction

" The test intentionally contains NO lsp#register_server() call.
execute 'set runtimepath^=' . fnameescape(s:lsp_dir)
execute 'set runtimepath^=' . fnameescape(s:settings_dir)
let g:lsp_auto_enable = 1
let g:lsp_log_verbose = 1
let g:lsp_log_file = s:workspace . '/vim-lsp-settings-vim-lsp.log'
runtime plugin/lsp.vim
runtime plugin/lsp_settings.vim

let g:settings_init = 0
let g:settings_buffer = 0
let g:settings_diag = 0
augroup perllsp_settings_receipt
  autocmd!
  autocmd User lsp_server_init if expand('<amatch>') ==# '' | let g:settings_init = 1 | endif
  autocmd User lsp_buffer_enabled let g:settings_buffer = 1
  autocmd User lsp_diagnostics_updated let g:settings_diag += 1
augroup END

execute 'lcd ' . fnameescape(s:workspace)
execute 'silent edit ' . fnameescape(s:workspace . '/main.pl')
if !s:Wait("lsp#is_server_running('perllsp')", 10000)
  call s:Fail('vim-lsp-settings did not start the perllsp server entry')
endif
if !s:Wait('g:settings_buffer', 8000)
  call s:Fail('vim-lsp-settings did not enable the Perl buffer')
endif
if !s:Wait('g:settings_diag > 0', 8000)
  call s:Fail('no diagnostics were observed through upstream registration')
endif
let s:cells.running = lsp#is_server_running('perllsp')
let s:cells.buffer_enabled = g:settings_buffer
let s:cells.diagnostics = lsp#get_buffer_diagnostics_counts()
let s:cells.filetype = &l:filetype
let s:cells.status = lsp#get_server_status('perllsp')

" A root-sensitive definition proves the upstream entry found the workspace
" rather than merely spawning a process.
let s:response = v:null
function! s:Capture(data) abort
  let s:response = a:data
endfunction
call cursor(4, 20)
call lsp#send_request('perllsp', {
      \ 'method': 'textDocument/definition',
      \ 'params': {'textDocument': lsp#get_text_document_identifier(), 'position': lsp#get_position()},
      \ 'on_notification': function('s:Capture'),
      \ })
if !s:Wait('type(s:response) == type({})', 7000)
  call s:Fail('definition through vim-lsp-settings timed out')
else
  let s:cells.definition = string(get(s:response.response, 'result', v:null)) =~# 'Widget.pm'
  if !s:cells.definition | call s:Fail('definition did not resolve Widget.pm') | endif
endif

call lsp#stop_server('perllsp')
call s:Wait("!lsp#is_server_running('perllsp')", 7000)
let s:cells.shutdown = !lsp#is_server_running('perllsp')
if !s:cells.shutdown | call s:Fail('perllsp remained running after stop') | endif

let s:result = {
      \ 'schema_version': 1,
      \ 'kind': 'vim_lsp_settings_candidate',
      \ 'upstream_ref': expand('$PERLLSP_SETTINGS_UPSTREAM_REF'),
      \ 'perllsp': expand('$PERLLSP_SETTINGS_BIN'),
      \ 'manual_registration_used': v:false,
      \ 'cells': s:cells,
      \ 'failures': s:failures,
      \ 'ok': empty(s:failures),
      \ }
call writefile([json_encode(s:result)], s:receipt)
if !empty(s:failures) | cquit 2 | endif
qa!
VIM

rc=0
"${vim_bin}" -Nu NONE -n -es -S "${tmpdir}/verify.vim" || rc=$?
[[ -f ${receipt} ]] || { echo "vim-lsp-settings smoke FAILED: receipt missing" >&2; exit 2; }
cat "${receipt}"
echo
if [[ ${rc} -ne 0 ]]; then
  cat "${tmpdir}/workspace/vim-lsp-settings-vim-lsp.log" >&2 || true
  exit "${rc}"
fi
