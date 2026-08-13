#!/usr/bin/env bash
# Interactive Vim/vim-lsp measurement substrate for #7771.
#
# Measures real editor/client/server boundaries for three bounded client modes:
#   omnifunc      - baseline vim-lsp completion path
#   async         - vim-lsp g:lsp_async_completion=1
#   asyncomplete  - upstream-recommended asyncomplete.vim integration
#
# Correctness is a prerequisite: the canonical #7691 journey runs first.
# This script emits observations only; it does not invent a latency SLO or pick
# a winner before actual distributions exist.

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
vim_bin=${VIM:-vim}
: "${VIM_LSP_DIR:?VIM_LSP_DIR must point at the pinned vim-lsp checkout}"
: "${PERLLSP:?PERLLSP must point at the exact perllsp candidate}"
out=${RECEIPT_DIR:-"${repo_root}/target/receipts/vim-ux"}
mkdir -p "${out}"

if ! command -v "${vim_bin}" >/dev/null 2>&1; then
  echo "vim UX FAILED: Vim executable not found: ${vim_bin}" >&2
  exit 1
fi

# Any faster-but-wrong configuration is invalid. Establish the canonical
# correctness baseline before measuring variants.
VIM="${vim_bin}" VIM_LSP_DIR="${VIM_LSP_DIR}" PERLLSP="${PERLLSP}" \
  RECEIPT="${out}/canonical-correctness.json" \
  EXPECT_INCREMENTAL="${EXPECT_INCREMENTAL:-0}" \
  "${repo_root}/scripts/ux/vim_vim_lsp_smoke.sh" >/dev/null

run_mode() {
  local mode=$1
  local workload=$2
  local receipt="${out}/${mode}-${workload}.json"

  if [[ ${mode} == asyncomplete ]]; then
    : "${ASYNCOMPLETE_DIR:?ASYNCOMPLETE_DIR is required for asyncomplete mode}"
    : "${ASYNCOMPLETE_LSP_DIR:?ASYNCOMPLETE_LSP_DIR is required for asyncomplete mode}"
    [[ -f ${ASYNCOMPLETE_DIR}/plugin/asyncomplete.vim ]] || {
      echo "vim UX FAILED: asyncomplete.vim plugin missing" >&2; return 1;
    }
    [[ -d ${ASYNCOMPLETE_LSP_DIR} ]] || {
      echo "vim UX FAILED: asyncomplete-lsp.vim checkout missing" >&2; return 1;
    }
  fi

  local tmpdir
  tmpdir=$(mktemp -d)
  local workspace="${tmpdir}/workspace"
  mkdir -p "${workspace}/lib"
  : >"${workspace}/.perl-lsp.toml"
  cat >"${workspace}/lib/Widget.pm" <<'PERL'
package Widget;
use strict;
sub answer { 42 }
1;
PERL
  cat >"${workspace}/main.pl" <<'PERL'
use strict;
use lib 'lib';
use Widget;
my $value = Widget::answer();
my $broken = $val
print $value;
PERL
  if [[ ${workload} == large ]]; then
    for i in $(seq 1 1500); do
      printf 'sub generated_%04d { return %d; }\n' "${i}" "${i}" >>"${workspace}/main.pl"
    done
  fi

  export PERLLSP_UX_MODE="${mode}"
  export PERLLSP_UX_WORKLOAD="${workload}"
  export PERLLSP_UX_WORKSPACE="${workspace}"
  export PERLLSP_UX_RECEIPT="${receipt}"
  export PERLLSP_UX_VIM_LSP_DIR="${VIM_LSP_DIR}"
  export PERLLSP_UX_BIN="${PERLLSP}"
  export PERLLSP_UX_ASYNCOMPLETE_DIR="${ASYNCOMPLETE_DIR:-}"
  export PERLLSP_UX_ASYNCOMPLETE_LSP_DIR="${ASYNCOMPLETE_LSP_DIR:-}"

  cat >"${tmpdir}/measure.vim" <<'VIM'
set nocompatible
set nomore
set hidden
filetype on

let s:mode = expand('$PERLLSP_UX_MODE')
let s:workload = expand('$PERLLSP_UX_WORKLOAD')
let s:workspace = expand('$PERLLSP_UX_WORKSPACE')
let s:receipt = expand('$PERLLSP_UX_RECEIPT')
let s:lsp_dir = expand('$PERLLSP_UX_VIM_LSP_DIR')
let s:perllsp = expand('$PERLLSP_UX_BIN')
let s:failures = []
let s:metrics = {}
let s:responses = {}

function! s:Fail(msg) abort
  call add(s:failures, a:msg)
endfunction
function! s:Wait(expr, timeout_ms) abort
  let l:start = reltime()
  while !eval(a:expr)
    if reltimefloat(reltime(l:start)) * 1000.0 > a:timeout_ms | return 0 | endif
    sleep 10m
  endwhile
  return 1
endfunction
function! s:Capture(key, data) abort
  let s:responses[a:key] = a:data
endfunction
function! s:RoundTrip(key, method, params) abort
  let s:responses[a:key] = v:null
  let l:start = reltime()
  call lsp#send_request('perllsp-ux', {
        \ 'method': a:method,
        \ 'params': a:params,
        \ 'on_notification': function('s:Capture', [a:key]),
        \ })
  if !s:Wait('type(s:responses[' . string(a:key) . ']) == type({})', 8000)
    call s:Fail(a:method . ' timeout')
    return [{}, -1.0]
  endif
  return [s:responses[a:key].response, reltimefloat(reltime(l:start)) * 1000.0]
endfunction

execute 'set runtimepath^=' . fnameescape(s:lsp_dir)
if s:mode ==# 'asyncomplete'
  execute 'set runtimepath^=' . fnameescape(expand('$PERLLSP_UX_ASYNCOMPLETE_DIR'))
  execute 'set runtimepath^=' . fnameescape(expand('$PERLLSP_UX_ASYNCOMPLETE_LSP_DIR'))
endif
let g:lsp_auto_enable = 0
let g:lsp_async_completion = s:mode ==# 'async' ? 1 : 0
let g:lsp_log_verbose = 0
runtime plugin/lsp.vim
if s:mode ==# 'asyncomplete'
  runtime plugin/asyncomplete.vim
  runtime plugin/asyncomplete-lsp.vim
endif

let g:ux_init = 0
let g:ux_buffer = 0
let g:ux_diag = 0
augroup perllsp_vim_ux
  autocmd!
  autocmd User lsp_server_init let g:ux_init = 1
  autocmd User lsp_buffer_enabled let g:ux_buffer = 1
  autocmd User lsp_diagnostics_updated let g:ux_diag += 1
augroup END

function! s:RootUri(server_info) abort
  let l:root = lsp#utils#find_nearest_parent_file_directory(
        \ expand('%:p'),
        \ ['.perl-lsp.toml', 'Makefile.PL', 'Build.PL', 'cpanfile', 'dist.ini', '.git'])
  if empty(l:root) | let l:root = getcwd() | endif
  return lsp#utils#path_to_uri(l:root)
endfunction
call lsp#register_server({
      \ 'name': 'perllsp-ux',
      \ 'cmd': {server_info -> [s:perllsp, '--stdio']},
      \ 'allowlist': ['perl'],
      \ 'root_uri': function('s:RootUri'),
      \ })
call lsp#enable()

let s:start = reltime()
execute 'lcd ' . fnameescape(s:workspace)
execute 'silent edit ' . fnameescape(s:workspace . '/main.pl')
if !s:Wait('g:ux_init && g:ux_buffer', 10000)
  call s:Fail('attach timeout')
endif
let s:metrics.attach_ms = reltimefloat(reltime(s:start)) * 1000.0
let s:metrics.vim_filetype = &l:filetype
let s:metrics.async_completion = g:lsp_async_completion
let s:metrics.asyncomplete_loaded = exists('*asyncomplete#register_source')

let s:diag_start = reltime()
if !s:Wait('g:ux_diag > 0', 10000) | call s:Fail('first diagnostics timeout') | endif
let s:metrics.first_diagnostics_ms = reltimefloat(reltime(s:diag_start)) * 1000.0

call cursor(4, strlen(getline(4)) + 1)
let s:position = lsp#get_position()
let [s:completion, s:completion_ms] = s:RoundTrip('completion', 'textDocument/completion', {
      \ 'textDocument': lsp#get_text_document_identifier(),
      \ 'position': s:position,
      \ 'context': {'triggerKind': 1},
      \ })
let s:metrics.completion_roundtrip_ms = s:completion_ms
if has_key(s:completion, 'result')
  let s:convert_start = reltime()
  let s:converted = lsp#omni#get_vim_completion_items({
        \ 'server': lsp#get_server_info('perllsp-ux'),
        \ 'position': s:position,
        \ 'response': s:completion,
        \ })
  let s:metrics.completion_convert_ms = reltimefloat(reltime(s:convert_start)) * 1000.0
  let s:metrics.completion_items = len(s:converted.items)
  if empty(s:converted.items) | call s:Fail('no completion items') | endif
else
  call s:Fail('completion response missing result')
endif

let [s:hover, s:hover_ms] = s:RoundTrip('hover', 'textDocument/hover', {
      \ 'textDocument': lsp#get_text_document_identifier(),
      \ 'position': lsp#get_position(),
      \ })
let s:metrics.hover_roundtrip_ms = s:hover_ms

let s:before_diag = g:ux_diag
let s:edit_start = reltime()
call setline(5, 'my $broken = $value;')
doautocmd <nomodeline> TextChanged
if !s:Wait('g:ux_diag > s:before_diag', 10000) | call s:Fail('post-edit diagnostics timeout') | endif
let s:metrics.edit_to_diagnostics_ms = reltimefloat(reltime(s:edit_start)) * 1000.0

let s:shutdown_start = reltime()
call lsp#stop_server('perllsp-ux')
if !s:Wait('!lsp#is_server_running("perllsp-ux")', 7000) | call s:Fail('shutdown timeout') | endif
let s:metrics.shutdown_ms = reltimefloat(reltime(s:shutdown_start)) * 1000.0

let s:result = {
      \ 'schema_version': 1,
      \ 'kind': 'vim_interactive_ux',
      \ 'mode': s:mode,
      \ 'workload': s:workload,
      \ 'vim_version': split(execute('version'), "\n")[0],
      \ 'metrics': s:metrics,
      \ 'failures': s:failures,
      \ 'ok': empty(s:failures),
      \ }
call writefile([json_encode(s:result)], s:receipt)
if !empty(s:failures) | cquit 2 | endif
qa!
VIM

  local rc=0
  "${vim_bin}" -Nu NONE -n -es -S "${tmpdir}/measure.vim" || rc=$?
  rm -rf "${tmpdir}"
  if [[ ${rc} -ne 0 ]]; then
    echo "vim UX FAILED: ${mode}/${workload}" >&2
    [[ -f ${receipt} ]] && cat "${receipt}" >&2
    return "${rc}"
  fi
  cat "${receipt}"
  echo
}

for workload in small large; do
  run_mode omnifunc "${workload}"
  run_mode async "${workload}"
  if [[ -n ${ASYNCOMPLETE_DIR:-} && -n ${ASYNCOMPLETE_LSP_DIR:-} ]]; then
    run_mode asyncomplete "${workload}"
  else
    printf '{"schema_version":1,"kind":"vim_interactive_ux","mode":"asyncomplete","workload":"%s","ok":false,"state":"not_proven","reason":"ASYNCOMPLETE_DIR_or_ASYNCOMPLETE_LSP_DIR_not_supplied"}\n' "${workload}" \
      >"${out}/asyncomplete-${workload}.json"
  fi
done

cat >"${out}/README.txt" <<'EOF'
These receipts are observations, not a hard SLO. Compare correctness first,
then attach/completion/diagnostic/edit/shutdown envelopes. A recommendation may
be made only after multiple real-host runs show a stable improvement without
stale or incorrect results. Missing asyncomplete inputs remain not_proven.
EOF
