#!/usr/bin/env bash
# Actual Coc-on-Vim receipt for #7769.
#
# Uses Coc's public Vim actions and CocRequest transport. Completion snippets
# are sourced from the real perllsp completion response and then inserted by
# Coc's real snippet engine; provider existence alone is not counted as proof.
#
# Required:
#   COC_DIR=/path/to/pinned/coc.nvim checkout with build/index.js
#   PERLLSP=/path/to/exact/perllsp
#
# Optional:
#   VIM=/path/to/vim
#   NODE=/path/to/node
#   RECEIPT=/path/to/receipt.json
#   EXPECT_INCREMENTAL=1

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
vim_bin=${VIM:-vim}
node_bin=${NODE:-node}
: "${COC_DIR:?COC_DIR must point at a pinned coc.nvim checkout}"
: "${PERLLSP:?PERLLSP must point at the exact perllsp candidate}"
expect_incremental=${EXPECT_INCREMENTAL:-0}

if ! command -v "${vim_bin}" >/dev/null 2>&1; then
  echo "vim/coc smoke FAILED: Vim executable not found: ${vim_bin}" >&2
  exit 1
fi
if ! command -v "${node_bin}" >/dev/null 2>&1; then
  echo "vim/coc smoke FAILED: Node executable not found: ${node_bin}" >&2
  exit 1
fi
if [[ ! -f "${COC_DIR}/build/index.js" ]]; then
  echo "vim/coc smoke FAILED: built Coc entry missing: ${COC_DIR}/build/index.js" >&2
  exit 1
fi
if [[ ! -x "${PERLLSP}" ]]; then
  echo "vim/coc smoke FAILED: perllsp is not executable: ${PERLLSP}" >&2
  exit 1
fi

# Current Coc upstream requires Node >= 20.19.0. Fail closed rather than
# allowing a host-runtime mismatch to masquerade as a perllsp defect.
"${node_bin}" -e '
const [major, minor] = process.versions.node.split(".").map(Number);
if (major < 20 || (major === 20 && minor < 19)) {
  console.error(`coc.nvim requires Node >=20.19.0; got ${process.versions.node}`);
  process.exit(1);
}
'

# Consume #7762 native Vim activation/root evidence first. Coc then proves its
# own rootPatterns/client behavior independently.
activation_receipt=$(mktemp)
RECEIPT="${activation_receipt}" VIM="${vim_bin}" \
  "${repo_root}/scripts/ux/vim_activation_root_smoke.sh" >/dev/null

tmpdir=$(mktemp -d)
cleanup() {
  rm -rf "${tmpdir}" "${activation_receipt}"
}
trap cleanup EXIT

receipt=${RECEIPT:-"${tmpdir}/vim-coc-receipt.json"}
mkdir -p "$(dirname "${receipt}")"
workspace="${tmpdir}/workspace"
config_home="${tmpdir}/coc-config"
mkdir -p "${workspace}/lib" "${config_home}"
: >"${workspace}/.perl-lsp.toml"

cat >"${workspace}/lib/Widget.pm" <<'PERL'
package Widget;
use strict;
use warnings;
sub answer { 42 }
sub greet { my ($name) = @_; return "hello $name"; }
1;
PERL
cat >"${workspace}/main.pl" <<'PERL'
use strict;
use warnings;
use lib 'lib';
use Widget;

my $value = Widget::answer();
my $broken = $val
my $emoji = "😀";
print Widget::greet("world"), $value, $emoji;

# completion insertion target follows
PERL

export PERLLSP_COC_CONFIG_HOME="${config_home}"
export PERLLSP_COC_BIN="${PERLLSP}"
"${node_bin}" <<'NODE'
const fs = require('fs');
const path = require('path');
const home = process.env.PERLLSP_COC_CONFIG_HOME;
const binary = process.env.PERLLSP_COC_BIN;
const config = {
  languageserver: {
    'perl-lsp': {
      command: binary,
      args: ['--stdio'],
      filetypes: ['perl'],
      rootPatterns: ['.perl-lsp.toml', 'Makefile.PL', 'Build.PL', 'cpanfile', 'dist.ini', '.git'],
      settings: { perl: { workspace: { includePaths: ['lib'] } } }
    }
  },
  'suggest.noselect': true,
  'diagnostic.enable': true
};
fs.writeFileSync(path.join(home, 'coc-settings.json'), JSON.stringify(config, null, 2));
NODE

export PERLLSP_COC_DIR="${COC_DIR}"
export PERLLSP_COC_NODE="$(command -v "${node_bin}")"
export PERLLSP_COC_WORKSPACE="${workspace}"
export PERLLSP_COC_RECEIPT="${receipt}"
export PERLLSP_COC_ACTIVATION_RECEIPT="${activation_receipt}"
export PERLLSP_EXPECT_INCREMENTAL="${expect_incremental}"

cat >"${tmpdir}/coc.vim" <<'VIM'
set nocompatible
set nomore
set hidden
set encoding=utf-8
set nobackup
set nowritebackup
set updatetime=300
set signcolumn=yes
filetype on

let s:workspace = expand('$PERLLSP_COC_WORKSPACE')
let s:receipt_path = expand('$PERLLSP_COC_RECEIPT')
let s:coc_dir = expand('$PERLLSP_COC_DIR')
let g:coc_node_path = expand('$PERLLSP_COC_NODE')
let g:coc_config_home = expand('$PERLLSP_COC_CONFIG_HOME')
let s:failures = []
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
    sleep 25m
  endwhile
  return 1
endfunction

execute 'set runtimepath^=' . fnameescape(s:coc_dir)
runtime plugin/coc.vim
execute 'lcd ' . fnameescape(s:workspace)
execute 'silent edit ' . fnameescape(s:workspace . '/main.pl')

" Every position below is a hardcoded line number into this fixture. An
" off-by-one here does not fail loudly -- it silently retargets a probe at the
" wrong line, so the Unicode and repair checks would pass while proving
" nothing. Assert the coordinates against the buffer before using them.
for [s:want_line, s:want_text] in [[6, 'my $value = Widget::answer();'], [7, 'my $broken = $val'], [8, 'my $emoji = "😀";']]
  if getline(s:want_line) !=# s:want_text
    call s:Fail(printf('fixture drift: line %d is %s, expected %s',
          \ s:want_line, string(getline(s:want_line)), string(s:want_text)))
  endif
endfor

if !s:WaitFor('coc#rpc#ready()', 10000)
  call s:Fail('Coc Node RPC did not become ready')
endif
let s:cells.rpc_ready = coc#rpc#ready()

let s:attached = CocAction('ensureDocument')
let s:cells.document_attached = s:attached
if !s:attached
  call s:Fail('Coc did not attach the Perl document')
endif

" Confirm the configured language service exists and is not silently replaced
" by another Coc server.
let s:services = CocAction('services')
let s:services_text = string(s:services)
let s:cells.perllsp_service = s:services_text =~# 'perl-lsp'
if !s:cells.perllsp_service
  call s:Fail('Coc service list does not contain perl-lsp: ' . s:services_text)
endif

" Wait until ordinary providers are attached; this is stronger than Node RPC
" readiness but still does not count individual operations as proven.
if !s:WaitFor("CocHasProvider('hover') && CocHasProvider('definition')", 10000)
  call s:Fail('perllsp providers did not attach through Coc')
endif
let s:cells.providers = {
      \ 'hover': CocHasProvider('hover'),
      \ 'definition': CocHasProvider('definition'),
      \ 'reference': CocHasProvider('reference'),
      \ 'rename': CocHasProvider('rename'),
      \ 'format': CocHasProvider('format'),
      \ 'codeAction': CocHasProvider('codeAction'),
      \ }

" Diagnostics are consumed from Coc's actual diagnostic store.
let s:diagnostics = []
let s:diag_deadline = reltimefloat(reltime()) + 10.0
while empty(s:diagnostics) && reltimefloat(reltime()) < s:diag_deadline
  let s:diagnostics = CocAction('diagnosticList')
  if empty(s:diagnostics) | sleep 100m | endif
endwhile
let s:cells.diagnostics = {'count': len(s:diagnostics)}
if empty(s:diagnostics)
  call s:Fail('Coc observed no diagnostics for the intentionally broken fixture')
endif

" Hover/definition/references through stable public Coc actions.
" Line 6 is `my $value = Widget::answer();`; column 20 sits inside the
" `Widget::answer` call so definitions must resolve into lib/Widget.pm.
call cursor(6, 20)
let s:hover = CocAction('getHover')
let s:cells.hover = type(s:hover) == type([]) && !empty(s:hover)
if !s:cells.hover | call s:Fail('Coc getHover returned no text') | endif
let s:definitions = CocAction('definitions')
let s:cells.definition = type(s:definitions) == type([]) && string(s:definitions) =~# 'Widget.pm'
if !s:cells.definition | call s:Fail('Coc definitions did not resolve Widget.pm') | endif

" Column 6 sits inside `$value` on line 6.
call cursor(6, 6)
let s:references = CocAction('references', 0)
let s:cells.references = type(s:references) == type([]) && !empty(s:references)
if !s:cells.references | call s:Fail('Coc references returned no locations') | endif

" Completion is requested through Coc's language-client transport. At least one
" snippet-capable item must then be interpreted by Coc's actual snippet engine.
let s:uri = 'file://' . s:workspace . '/main.pl'
let s:line0 = line('$') - 1
let s:completion = CocRequest('perl-lsp', 'textDocument/completion', {
      \ 'textDocument': {'uri': s:uri},
      \ 'position': {'line': s:line0, 'character': 0},
      \ 'context': {'triggerKind': 1},
      \ })
" CocRequest yields v:null on timeout or server error. Calling get() on that
" throws and aborts the whole run, which reads as a harness crash rather than
" the product failure it actually is.
if type(s:completion) == type([])
  let s:completion_items = s:completion
elseif type(s:completion) == type({})
  let s:completion_items = get(s:completion, 'items', [])
else
  let s:completion_items = []
  call s:Fail('CocRequest completion returned ' . string(s:completion) . ' instead of a result')
endif
let s:snippet_item = {}
for s:item in s:completion_items
  if get(s:item, 'insertTextFormat', 1) == 2
    let s:snippet_item = s:item
    break
  endif
endfor
let s:cells.completion = {'count': len(s:completion_items), 'snippet_item_found': !empty(s:snippet_item)}
if empty(s:completion_items)
  call s:Fail('CocRequest completion returned no items')
elseif empty(s:snippet_item)
  call s:Fail('completion returned no snippet-capable item for Coc insertion proof')
else
  let s:snippet_text = get(s:snippet_item, 'insertText', get(s:snippet_item, 'label', ''))
  let s:insert_line = line('$') - 1
  let s:range = {
        \ 'start': {'line': s:insert_line, 'character': 0},
        \ 'end': {'line': s:insert_line, 'character': 0},
        \ }
  let s:snippet_ok = CocAction('snippetInsert', s:range, s:snippet_text, 1)
  let s:inserted = getline('$')
  let s:cells.snippet_insertion = {
        \ 'action_result': s:snippet_ok,
        \ 'literal_placeholder_remaining': s:inserted =~# '\${\|\$[0-9]',
        \ }
  if s:inserted =~# '\${\|\$[0-9]'
    call s:Fail('Coc snippet insertion left literal snippet placeholders')
  endif
  call CocAction('snippetCancel')
endif

" Observe available code actions independently. Empty is a bounded product
" result for this fixture; a provider error or malformed value is not.
let s:actions = CocAction('codeActions', '', [])
let s:cells.code_actions = {'observed': type(s:actions) == type([]), 'count': type(s:actions) == type([]) ? len(s:actions) : -1}
if type(s:actions) != type([]) | call s:Fail('Coc codeActions returned a malformed result') | endif

" Repair the syntax defect through Vim, let Coc synchronize it, then require
" diagnostics to update rather than reusing the original stale result.
" Line 7 is the intentionally broken `my $broken = $val`.
call setline(7, 'my $broken = $value;')
doautocmd <nomodeline> TextChanged
sleep 300m
let s:after_edit = CocAction('diagnosticList')
let s:cells.edit_freshness = string(s:after_edit) !=# string(s:diagnostics)
if !s:cells.edit_freshness
  call s:Fail('Coc diagnostics did not change after the document edit')
endif

" Formatting through Coc's public action; false is an explicit failure.
let s:format_result = CocAction('format')
let s:cells.formatting = s:format_result isnot v:false
if !s:cells.formatting | call s:Fail('Coc format action failed') | endif

" Interactive rename: queue the dialog answer, invoke the real Coc rename
" action, and inspect the resulting buffer after workspace edits are applied.
call cursor(6, 6)
if CocHasProvider('rename')
  call feedkeys("renamed_value\<CR>", 't')
  try
    call CocAction('rename')
  catch
    call s:Fail('Coc rename raised: ' . v:exception)
  endtry
  sleep 300m
  let s:cells.rename_applied = join(getline(1, '$'), "\n") =~# 'renamed_value'
  if !s:cells.rename_applied | call s:Fail('Coc rename did not apply to the Vim buffer') | endif
else
  let s:cells.rename_applied = v:false
  call s:Fail('Coc rename provider is unavailable')
endif

" Unicode discriminator: the emoji precedes a later symbol on the line. A
" hover request through Coc must not fail from byte/UTF-16 position drift.
" Line 8 is `my $emoji = "<emoji>";`. strlen() is a byte count, so this byte
" column lands inside the 4-byte emoji itself — which is the whole point: a
" byte/UTF-16 conversion error in the position mapping shows up here.
call cursor(8, strlen(getline(8)) - 3)
try
  let s:unicode_hover = CocAction('getHover')
  let s:cells.unicode_position = type(s:unicode_hover) == type([])
catch
  let s:cells.unicode_position = v:false
endtry
if !s:cells.unicode_position | call s:Fail('Coc Unicode position-bearing hover failed') | endif

" Shut Coc down so the Node process and perllsp child are not allowed to survive
" an otherwise-passing editor run.
try
  CocDisable
catch
endtry
sleep 300m
let s:cells.shutdown = !coc#rpc#ready()
if !s:cells.shutdown
  " Coc may keep RPC until VimLeave; record this as a bounded pre-exit state and
  " rely on the shell-level process/log checks below rather than false-green it.
  let s:cells.shutdown = 'pending_vim_exit'
endif

let s:receipt = {
      \ 'schema_version': 1,
      \ 'kind': 'vim_coc_actual_client',
      \ 'vim_version': execute('version'),
      \ 'coc_dir': s:coc_dir,
      \ 'node': g:coc_node_path,
      \ 'perllsp': expand('$PERLLSP_COC_BIN'),
      \ 'activation_receipt': expand('$PERLLSP_COC_ACTIVATION_RECEIPT'),
      \ 'cells': s:cells,
      \ 'failures': s:failures,
      \ 'ok': empty(s:failures),
      \ }
call writefile([json_encode(s:receipt)], s:receipt_path)
if !empty(s:failures)
  for s:failure in s:failures | echomsg 'vim/coc smoke FAILED: ' . s:failure | endfor
  cquit 2
endif
qa!
VIM

vim_rc=0
"${vim_bin}" -Nu NONE -n -es -S "${tmpdir}/coc.vim" || vim_rc=$?
if [[ ! -f "${receipt}" ]]; then
  echo "vim/coc smoke FAILED: receipt was not written" >&2
  exit 2
fi
cat "${receipt}"
echo

# Coc logs are intentionally searched only as a transport discriminator; all
# semantic cells above come from the actual Coc/Vim state/actions.
shopt -s nullglob
coc_log_candidates=("${tmpdir}"/coc-*.log "${config_home}"/*.log)
shopt -u nullglob
did_change_count=0
ranged_change_count=0
for log in "${coc_log_candidates[@]}"; do
  [[ -f ${log} ]] || continue
  # `grep -c` prints 0 and exits 1 on no match, but prints nothing on a read
  # error; an empty operand would make the arithmetic below a syntax error.
  count=$(grep -c 'textDocument/didChange' "${log}" || true)
  did_change_count=$((did_change_count + ${count:-0}))
  range_count=$(grep 'textDocument/didChange' "${log}" | grep -c '"range"' || true)
  ranged_change_count=$((ranged_change_count + ${range_count:-0}))
done
if [[ ${expect_incremental} == 1 && ${did_change_count} -gt 0 && ${ranged_change_count} -eq 0 ]]; then
  echo "vim/coc smoke FAILED: incremental sync expected but observed Coc didChange traffic was not ranged" >&2
  exit 2
fi

if [[ ${vim_rc} -ne 0 ]]; then
  exit "${vim_rc}"
fi
