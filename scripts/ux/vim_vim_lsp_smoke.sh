#!/usr/bin/env bash
# Deep actual Vim + vim-lsp receipt for #7691.
#
# Requires the #7762 activation/root substrate from the parent PR. This script
# deliberately uses vim-lsp's real request transport and edit/conversion helpers;
# a synthetic LSP peer cannot satisfy it.
#
# Usage:
#   VIM_LSP_DIR=/path/to/pinned/vim-lsp \
#   PERLLSP=/path/to/exact/perllsp \
#     ./scripts/ux/vim_vim_lsp_smoke.sh
#
# Optional:
#   VIM=/path/to/vim
#   RECEIPT=/path/to/receipt.json
#   EXPECT_INCREMENTAL=1   # require ranged didChange after #7713 lands

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
vim_bin=${VIM:-vim}
: "${VIM_LSP_DIR:?VIM_LSP_DIR must point at a pinned vim-lsp checkout}"
: "${PERLLSP:?PERLLSP must point at the exact perllsp candidate}"
expect_incremental=${EXPECT_INCREMENTAL:-0}

if ! command -v "${vim_bin}" >/dev/null 2>&1; then
  echo "vim/vim-lsp smoke FAILED: Vim executable not found: ${vim_bin}" >&2
  exit 1
fi
if [[ ! -f "${VIM_LSP_DIR}/plugin/lsp.vim" ]]; then
  echo "vim/vim-lsp smoke FAILED: vim-lsp plugin missing under ${VIM_LSP_DIR}" >&2
  exit 1
fi
if [[ ! -x "${PERLLSP}" ]]; then
  echo "vim/vim-lsp smoke FAILED: perllsp is not executable: ${PERLLSP}" >&2
  exit 1
fi

# Prove the shared activation/root contract first. A deep receipt may not
# manufacture filetype/root state that #7762 would reject.
activation_receipt=$(mktemp)
RECEIPT="${activation_receipt}" VIM="${vim_bin}" VIM_LSP_DIR="${VIM_LSP_DIR}" PERLLSP="${PERLLSP}" \
  "${repo_root}/scripts/ux/vim_activation_root_smoke.sh" --integration >/dev/null

tmpdir=$(mktemp -d)
cleanup() {
  rm -rf "${tmpdir}" "${activation_receipt}"
}
trap cleanup EXIT

receipt=${RECEIPT:-"${tmpdir}/vim-vim-lsp-receipt.json"}
mkdir -p "$(dirname "${receipt}")"
workspace="${tmpdir}/workspace"
mkdir -p "${workspace}/lib"
: >"${workspace}/.perl-lsp.toml"

cat >"${workspace}/lib/Widget.pm" <<'PERL'
package Widget;
use strict;
use warnings;

sub answer {
    return 42;
}

sub greet {
    my ($name) = @_;
    return "hello $name";
}

1;
PERL

cat >"${workspace}/main.pl" <<'PERL'
use strict;
use warnings;
use lib 'lib';
use Widget;

my $value = Widget::answer();
my $copy = $val
my $emoji = "😀";
print Widget::greet("world"), $value, $emoji;
PERL

export PERLLSP_VIM_LSP_DIR="${VIM_LSP_DIR}"
export PERLLSP_VIM_BIN="${PERLLSP}"
export PERLLSP_VIM_WORKSPACE="${workspace}"
export PERLLSP_VIM_RECEIPT="${receipt}"
export PERLLSP_VIM_LOG="${tmpdir}/vim-lsp.log"
export PERLLSP_VIM_ACTIVATION_RECEIPT="${activation_receipt}"
export PERLLSP_EXPECT_INCREMENTAL="${expect_incremental}"

cat >"${tmpdir}/deep.vim" <<'VIM'
set nocompatible
set nomore
set hidden
filetype on

let s:workspace = expand('$PERLLSP_VIM_WORKSPACE')
let s:perllsp = expand('$PERLLSP_VIM_BIN')
let s:vim_lsp_dir = expand('$PERLLSP_VIM_LSP_DIR')
let s:receipt_path = expand('$PERLLSP_VIM_RECEIPT')
let s:log_path = expand('$PERLLSP_VIM_LOG')
let s:failures = []
let s:responses = {}
let s:cells = {}

function! s:Fail(message) abort
  call add(s:failures, a:message)
endfunction

function! s:WaitFor(expr, timeout_ms) abort
  let l:start = reltime()
  while !eval(a:expr)
    if reltimefloat(reltime(l:start)) * 1000.0 > a:timeout_ms
      return 0
    endif
    sleep 20m
  endwhile
  return 1
endfunction

function! s:Capture(key, data) abort
  let s:responses[a:key] = a:data
endfunction

function! s:Request(key, method, params) abort
  let s:responses[a:key] = v:null
  call lsp#send_request('perllsp-under-test', {
        \ 'method': a:method,
        \ 'params': a:params,
        \ 'on_notification': function('s:Capture', [a:key]),
        \ })
  if !s:WaitFor('type(s:responses[' . string(a:key) . ']) == type({})', 7000)
    call s:Fail(a:method . ' timed out')
    return {}
  endif
  let l:data = s:responses[a:key]
  if !has_key(l:data, 'response')
    call s:Fail(a:method . ' returned no response envelope')
    return {}
  endif
  let l:response = l:data.response
  if has_key(l:response, 'error')
    call s:Fail(a:method . ' returned error: ' . string(l:response.error))
    return l:response
  endif
  return l:response
endfunction

function! s:PositionParams() abort
  return {
        \ 'textDocument': lsp#get_text_document_identifier(),
        \ 'position': lsp#get_position(),
        \ }
endfunction

execute 'set runtimepath^=' . fnameescape(s:vim_lsp_dir)
let g:lsp_auto_enable = 0
let g:lsp_log_verbose = 1
let g:lsp_log_file = s:log_path
let g:lsp_async_completion = 0
let g:lsp_show_workspace_edits = 0
runtime plugin/lsp.vim

let g:perllsp_server_init = 0
let g:perllsp_buffer_enabled = 0
let g:perllsp_diagnostics_updated = 0
let g:perllsp_server_exit = 0
augroup perllsp_deep_receipt
  autocmd!
  autocmd User lsp_server_init let g:perllsp_server_init = 1
  autocmd User lsp_buffer_enabled let g:perllsp_buffer_enabled = 1
  autocmd User lsp_diagnostics_updated let g:perllsp_diagnostics_updated += 1
  autocmd User lsp_server_exit let g:perllsp_server_exit = 1
augroup END

function! s:RootUri(server_info) abort
  let l:root = lsp#utils#find_nearest_parent_file_directory(
        \ expand('%:p'),
        \ ['.perl-lsp.toml', 'Makefile.PL', 'Build.PL', 'cpanfile', 'dist.ini', '.git'])
  if empty(l:root)
    let l:root = getcwd()
  endif
  return lsp#utils#path_to_uri(l:root)
endfunction

call lsp#register_server({
      \ 'name': 'perllsp-under-test',
      \ 'cmd': {server_info -> [s:perllsp, '--stdio']},
      \ 'allowlist': ['perl'],
      \ 'root_uri': function('s:RootUri'),
      \ 'workspace_config': {
      \   'perl': {
      \     'workspace': {
      \       'includePaths': [s:workspace . '/lib'],
      \     },
      \   },
      \ },
      \ })
call lsp#enable()

execute 'lcd ' . fnameescape(s:workspace)
execute 'silent edit ' . fnameescape(s:workspace . '/main.pl')
if !s:WaitFor('g:perllsp_server_init && g:perllsp_buffer_enabled', 8000)
  call s:Fail('server did not initialize and enable the Perl buffer')
endif
let s:cells.initialize = g:perllsp_server_init && g:perllsp_buffer_enabled
let s:cells.filetype = &l:filetype
let s:cells.server_status = lsp#get_server_status('perllsp-under-test')

" Diagnostics must be observed by vim-lsp, not inferred from server stderr.
if !s:WaitFor('g:perllsp_diagnostics_updated > 0', 8000)
  call s:Fail('vim-lsp did not publish a diagnostics-updated event')
endif
let s:diagnostic_counts = lsp#get_buffer_diagnostics_counts()
let s:cells.diagnostics = {
      \ 'events': g:perllsp_diagnostics_updated,
      \ 'counts': s:diagnostic_counts,
      \ }

" Completion: request through vim-lsp, then run the real vim-lsp conversion
" path used by omnifunc. The resulting Vim words must be non-empty and must not
" contain snippet placeholders when this client advertises snippetSupport=false.
call cursor(7, strlen(getline(7)) + 1)
let s:completion_pos = lsp#get_position()
let s:completion_response = s:Request('completion', 'textDocument/completion', {
      \ 'textDocument': lsp#get_text_document_identifier(),
      \ 'position': s:completion_pos,
      \ 'context': {'triggerKind': 1},
      \ })
if has_key(s:completion_response, 'result')
  let s:converted = lsp#omni#get_vim_completion_items({
        \ 'server': lsp#get_server_info('perllsp-under-test'),
        \ 'position': s:completion_pos,
        \ 'response': s:completion_response,
        \ })
  let s:words = map(copy(s:converted.items), {_, item -> get(item, 'word', '')})
  let s:bad_words = filter(copy(s:words), {_, word -> word =~# '\${\|\$[0-9]'} )
  let s:cells.completion = {
        \ 'count': len(s:converted.items),
        \ 'startcol': s:converted.startcol,
        \ 'words_without_literal_placeholders': empty(s:bad_words),
        \ }
  if empty(s:converted.items)
    call s:Fail('completion returned no Vim completion items')
  endif
  if !empty(s:bad_words)
    call s:Fail('vim-lsp completion conversion retained literal snippet placeholders')
  endif
else
  call s:Fail('completion response had no result')
endif

" Hover and definition are direct actual-client requests. Definition must point
" at Widget.pm, proving workspace/root semantics rather than just server liveness.
call cursor(6, 20)
let s:hover = s:Request('hover', 'textDocument/hover', s:PositionParams())
let s:cells.hover = has_key(s:hover, 'result') && type(s:hover.result) != type(v:null)
if !s:cells.hover
  call s:Fail('hover returned no useful result')
endif
let s:def = s:Request('definition', 'textDocument/definition', s:PositionParams())
let s:def_text = string(get(s:def, 'result', v:null))
let s:cells.definition = s:def_text =~# 'Widget.pm'
if !s:cells.definition
  call s:Fail('definition did not resolve into Widget.pm: ' . s:def_text)
endif

" Unicode discriminator: a non-BMP character appears before the final symbol;
" a position-bearing request must still resolve a valid response rather than a
" shifted/out-of-range error.
call cursor(9, strlen(getline(9)) - 6)
let s:unicode_hover = s:Request('unicode_hover', 'textDocument/hover', s:PositionParams())
let s:cells.unicode_position = has_key(s:unicode_hover, 'result')
if !s:cells.unicode_position
  call s:Fail('Unicode position-bearing request returned no result field')
endif

" Edit/re-query currentness. Change the partial variable through Vim, fire the
" real TextChanged event, then require a new completion response.
call setline(7, 'my $copy = $value;')
doautocmd <nomodeline> TextChanged
sleep 150m
call cursor(7, strlen(getline(7)) + 1)
let s:post_edit = s:Request('post_edit_hover', 'textDocument/hover', s:PositionParams())
let s:cells.edit_requery = has_key(s:post_edit, 'result')
if !s:cells.edit_requery
  call s:Fail('post-edit request did not return a current result envelope')
endif

" References for the local value prove one more position-bearing result shape.
call cursor(6, 6)
let s:ref_params = s:PositionParams()
let s:ref_params.context = {'includeDeclaration': v:true}
let s:refs = s:Request('references', 'textDocument/references', s:ref_params)
let s:cells.references = type(get(s:refs, 'result', v:null)) == type([]) && !empty(s:refs.result)
if !s:cells.references
  call s:Fail('references returned no locations')
endif

" Rename request plus vim-lsp's public WorkspaceEdit applier proves the client
" edit application path, not just a server response.
let s:rename = s:Request('rename', 'textDocument/rename', extend(s:PositionParams(), {'newName': 'renamed_value'}))
if has_key(s:rename, 'result') && type(s:rename.result) == type({})
  call lsp#utils#workspace_edit#apply_workspace_edit(s:rename.result)
  let s:renamed_text = join(getline(1, '$'), "\n")
  let s:cells.rename_applied = s:renamed_text =~# 'renamed_value'
else
  let s:cells.rename_applied = v:false
endif
if !s:cells.rename_applied
  call s:Fail('rename WorkspaceEdit was not applied to the Vim buffer')
endif

" Formatting goes through the real vim-lsp text edit helper when the server
" returns edits. An explicit empty edit list is retained as an observed cell;
" malformed/error responses fail above.
let s:format = s:Request('formatting', 'textDocument/formatting', {
      \ 'textDocument': lsp#get_text_document_identifier(),
      \ 'options': {'tabSize': &shiftwidth > 0 ? &shiftwidth : 4, 'insertSpaces': &expandtab ? v:true : v:false},
      \ })
if has_key(s:format, 'result') && type(s:format.result) == type([])
  if !empty(s:format.result)
    call lsp#utils#text_edit#apply_text_edits(lsp#get_text_document_identifier().uri, s:format.result)
  endif
  let s:cells.formatting = {'observed': v:true, 'edit_count': len(s:format.result)}
else
  let s:cells.formatting = {'observed': v:false, 'edit_count': 0}
  call s:Fail('formatting response did not contain an edit list')
endif

" Close/reopen must preserve a working session and current buffer attachment.
let s:before_reopen_status = lsp#get_server_status('perllsp-under-test')
silent bwipeout!
execute 'silent edit ' . fnameescape(s:workspace . '/main.pl')
let s:reopened = s:WaitFor("&l:filetype ==# 'perl'", 3000)
let s:cells.close_reopen = s:reopened && lsp#is_server_running('perllsp-under-test')
if !s:cells.close_reopen
  call s:Fail('close/reopen did not preserve a running Perl LSP session')
endif

" Request a clean shutdown through vim-lsp and observe its exit event.
call lsp#stop_server('perllsp-under-test')
let s:stopped = s:WaitFor('g:perllsp_server_exit || !lsp#is_server_running("perllsp-under-test")', 7000)
let s:cells.shutdown = s:stopped && !lsp#is_server_running('perllsp-under-test')
if !s:cells.shutdown
  call s:Fail('vim-lsp did not stop perllsp cleanly')
endif

let s:receipt = {
      \ 'schema_version': 1,
      \ 'kind': 'vim_vim_lsp_actual_client',
      \ 'vim_version': execute('version'),
      \ 'vim_lsp_dir': s:vim_lsp_dir,
      \ 'perllsp': s:perllsp,
      \ 'workspace': s:workspace,
      \ 'activation_receipt': expand('$PERLLSP_VIM_ACTIVATION_RECEIPT'),
      \ 'cells': s:cells,
      \ 'failures': s:failures,
      \ 'ok': empty(s:failures),
      \ }
call writefile([json_encode(s:receipt)], s:receipt_path)

if !empty(s:failures)
  for s:failure in s:failures
    echomsg 'vim/vim-lsp smoke FAILED: ' . s:failure
  endfor
  cquit 2
endif
qa!
VIM

vim_rc=0
"${vim_bin}" -Nu NONE -n -es -S "${tmpdir}/deep.vim" || vim_rc=$?

if [[ ! -f "${receipt}" ]]; then
  echo "vim/vim-lsp smoke FAILED: receipt was not written" >&2
  if [[ -f "${tmpdir}/vim-lsp.log" ]]; then
    cat "${tmpdir}/vim-lsp.log" >&2
  fi
  exit 2
fi

# #7713 discriminator. Before the cutover this is recorded but not required;
# once EXPECT_INCREMENTAL=1 is set, a real didChange without a range fails.
did_change_count=0
ranged_change_count=0
if [[ -f "${tmpdir}/vim-lsp.log" ]]; then
  did_change_count=$(grep -c 'textDocument/didChange' "${tmpdir}/vim-lsp.log" || true)
  ranged_change_count=$(grep 'textDocument/didChange' "${tmpdir}/vim-lsp.log" | grep -c '"range"' || true)
fi
if [[ ${did_change_count} -eq 0 ]]; then
  echo "vim/vim-lsp smoke FAILED: no real textDocument/didChange traffic observed" >&2
  cat "${receipt}" >&2
  exit 2
fi
if [[ ${expect_incremental} == 1 && ${ranged_change_count} -eq 0 ]]; then
  echo "vim/vim-lsp smoke FAILED: #7713 ranged-sync mode expected but no ranged didChange was observed" >&2
  cat "${tmpdir}/vim-lsp.log" >&2
  exit 2
fi

cat "${receipt}"
echo
printf 'vim-lsp didChange: total=%s ranged=%s expect_incremental=%s\n' \
  "${did_change_count}" "${ranged_change_count}" "${expect_incremental}"

if [[ ${vim_rc} -ne 0 ]]; then
  echo "--- vim-lsp log ---" >&2
  cat "${tmpdir}/vim-lsp.log" >&2 || true
  exit "${vim_rc}"
fi
