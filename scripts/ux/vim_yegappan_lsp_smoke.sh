#!/usr/bin/env bash
# Actual Vim9 + yegappan/lsp evaluation harness for #7717.
#
# This is a comparison/evaluation receipt, not a canonical-support harness.
# Passing all cells may produce a support-row candidate, but it cannot switch
# the canonical Vim client without a separate reviewed decision.
#
# Required:
#   YEGAPPAN_LSP_DIR=/path/to/pinned/yegappan/lsp
#   PERLLSP=/path/to/exact/perllsp
#
# Optional:
#   VIM=/path/to/vim9
#   RECEIPT=/path/to/receipt.json

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
vim_bin=${VIM:-vim}
: "${YEGAPPAN_LSP_DIR:?YEGAPPAN_LSP_DIR must point at pinned yegappan/lsp}"
: "${PERLLSP:?PERLLSP must point at exact perllsp}"

if ! command -v "${vim_bin}" >/dev/null 2>&1; then
  echo "yegappan/lsp evaluation FAILED: Vim missing: ${vim_bin}" >&2
  exit 1
fi
if ! "${vim_bin}" --version | grep -Eq 'Vi IMproved 9\.'; then
  echo "yegappan/lsp evaluation FAILED: Vim 9+ is required" >&2
  exit 1
fi
if [[ ! -f "${YEGAPPAN_LSP_DIR}/plugin/lsp.vim" ]]; then
  echo "yegappan/lsp evaluation FAILED: plugin/lsp.vim missing" >&2
  exit 1
fi
if [[ ! -x "${PERLLSP}" ]]; then
  echo "yegappan/lsp evaluation FAILED: perllsp not executable" >&2
  exit 1
fi

# Consume the shared native Vim filetype/root evidence before loading another
# client. This prevents a client-specific config from manufacturing Perl input.
activation_receipt=$(mktemp)
RECEIPT="${activation_receipt}" VIM="${vim_bin}" \
  "${repo_root}/scripts/ux/vim_activation_root_smoke.sh" >/dev/null

tmpdir=$(mktemp -d)
cleanup() { rm -rf "${tmpdir}" "${activation_receipt}"; }
trap cleanup EXIT
workspace="${tmpdir}/workspace"
mkdir -p "${workspace}/lib"
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
my $emoji = "😀"; my $unicode_value = Widget::answer();
print Widget::greet("world"), $value, $emoji, $unicode_value;
PERL

receipt=${RECEIPT:-"${tmpdir}/vim-yegappan-receipt.json"}
export PERLLSP_YEG_DIR="${YEGAPPAN_LSP_DIR}"
export PERLLSP_YEG_BIN="${PERLLSP}"
export PERLLSP_YEG_WORKSPACE="${workspace}"
export PERLLSP_YEG_RECEIPT="${receipt}"
export PERLLSP_YEG_ACTIVATION="${activation_receipt}"

cat >"${tmpdir}/evaluate.vim" <<'VIM'
vim9script
set nocompatible
set nomore
set hidden
filetype on

var clientDir = expand('$PERLLSP_YEG_DIR')
var perllsp = expand('$PERLLSP_YEG_BIN')
var workspace = expand('$PERLLSP_YEG_WORKSPACE')
var receipt = expand('$PERLLSP_YEG_RECEIPT')
var failures: list<string> = []
var cells: dict<any> = {}
g:yegAttached = 0

export def Fail(msg: string)
  failures->add(msg)
enddef

export def WaitFor(Predicate: func(): bool, timeoutMs: number): bool
  var start = reltime()
  while !Predicate()
    if reltimefloat(reltime(start)) * 1000.0 > timeoutMs
      return false
    endif
    sleep 25m
  endwhile
  return true
enddef

execute 'set runtimepath^=' .. fnameescape(clientDir)
g:lsp_enable = false
runtime plugin/lsp.vim
if !get(g:, 'loaded_lsp', false)
  Fail('yegappan/lsp did not load')
endif

g:LspOptionsSet({
  autoComplete: false,
  omniComplete: true,
  omniCompleteAllowBare: true,
  autoHighlightDiags: true,
  showDiagWithSign: true,
  snippetSupport: false,
  semanticHighlight: true,
  showInlayHints: false
})

g:LspAddServer([{
  name: 'perllsp-yegappan',
  filetype: ['perl'],
  path: perllsp,
  args: ['--stdio'],
  syncInit: true,
  rootSearch: ['.perl-lsp.toml', 'Makefile.PL', 'Build.PL', 'cpanfile', 'dist.ini', '.git/'],
  workspaceConfig: {
    perl: {
      workspace: {
        includePaths: ['lib']
      }
    }
  }
}])

augroup perllsp_yegappan_receipt
  autocmd!
  autocmd User LspAttached g:yegAttached += 1
augroup END

g:LspEnable()
execute 'lcd ' .. fnameescape(workspace)
execute 'silent edit ' .. fnameescape(workspace .. '/main.pl')

if !WaitFor(() => g:LspServerReady(), 10000)
  Fail('language server did not become ready')
endif
if !WaitFor(() => g:yegAttached > 0, 5000)
  Fail('LspAttached was not observed')
endif
cells.attach_and_root = {
  ready: g:LspServerReady(),
  attached_events: g:yegAttached,
  filetype: &filetype,
  server_listing: execute('LspShowAllServers')
}
if &filetype !=# 'perl'
  Fail($'native Vim filetype is {&filetype}, expected perl')
endif

# Diagnostics are read from the client's actual buffer diagnostic store.
var diags: list<any> = []
if !WaitFor(() => {
  diags = lsp#diag#GetDiagsForBuf()
  return !diags->empty()
}, 10000)
  Fail('no diagnostics observed for broken fixture')
endif
cells.diagnostics = {count: diags->len()}

# Completion goes through the public omnifunc implemented by this client. It
# both sends the LSP request and converts the result into Vim completion items.
cursor(6, strlen(getline(6)) + 1)
var completionStart = g:LspOmniFunc(1, '')
var completionItems: any = g:LspOmniFunc(0, '')
var completionOk = completionItems->type() == v:t_list && !completionItems->empty()
cells.completion = {
  start: completionStart,
  count: completionOk ? completionItems->len() : 0,
  literal_snippet_placeholder: completionOk && completionItems->string() =~# '\${\|\$[0-9]'
}
if !completionOk
  Fail('public LspOmniFunc returned no completion items')
elseif cells.completion.literal_snippet_placeholder
  Fail('non-snippet yegappan completion exposed literal snippet placeholders')
endif

# Hover must create a visible popup through the real client UI.
cursor(5, 20)
var popupBefore = popup_list()->len()
try
  LspHover
catch
  Fail('LspHover raised: ' .. v:exception)
endtry
var hoverVisible = WaitFor(() => popup_list()->len() > popupBefore, 5000)
cells.hover = hoverVisible
if !hoverVisible
  Fail('LspHover did not create a visible client popup')
endif
for id in popup_list()
  try
    popup_close(id)
  catch
  endtry
endfor

# Definition must navigate to the actual workspace module.
try
  LspGotoDefinition
catch
  Fail('LspGotoDefinition raised: ' .. v:exception)
endtry
var definitionOk = WaitFor(() => expand('%:t') ==# 'Widget.pm', 5000)
cells.definition = definitionOk
if !definitionOk
  Fail('definition did not navigate to Widget.pm')
endif
execute 'silent edit ' .. fnameescape(workspace .. '/main.pl')
WaitFor(() => g:LspServerReady(), 3000)

# References must populate Vim's location list.
cursor(5, 6)
try
  LspShowReferences
catch
  Fail('LspShowReferences raised: ' .. v:exception)
endtry
var refsVisible = WaitFor(() => getloclist(0)->len() > 0, 5000)
cells.references = {count: getloclist(0)->len()}
if !refsVisible
  Fail('references produced no location-list entries')
endif

# Unicode position discriminator: the emoji occurs before a later symbol on the
# same line. Definition must still land on Widget.pm.
cursor(7, strlen(getline(7)) - 8)
try
  LspGotoDefinition
catch
  Fail('Unicode definition raised: ' .. v:exception)
endtry
var unicodeOk = WaitFor(() => expand('%:t') ==# 'Widget.pm', 5000)
cells.unicode_position = unicodeOk
if !unicodeOk
  Fail('Unicode position-bearing definition did not resolve Widget.pm')
endif
execute 'silent edit ' .. fnameescape(workspace .. '/main.pl')
WaitFor(() => g:LspServerReady(), 3000)

# Fix the syntax error through Vim, flush the client's listener, and require the
# client's diagnostic state to change. This is currentness evidence; exact
# didChange batching remains a separate observed field until host logging binds it.
var beforeDiag = lsp#diag#GetDiagsForBuf()->string()
setline(6, 'my $broken = $value;')
listener_flush()
var changed = WaitFor(() => lsp#diag#GetDiagsForBuf()->string() !=# beforeDiag, 10000)
cells.edit_freshness = changed
if !changed
  Fail('client diagnostics did not change after accepted Vim edit')
endif
cells.did_change_shape = 'not_instrumented_by_this_evaluation_harness'

# Rename accepts an explicit new name as the command argument, avoiding an
# interactive dialog. Inspect actual Vim buffer state afterwards.
cursor(5, 6)
try
  execute 'LspRename renamed_value'
catch
  Fail('LspRename raised: ' .. v:exception)
endtry
var renameOk = WaitFor(() => getline(1, '$')->join("\n") =~# 'renamed_value', 5000)
cells.rename_workspace_edit = renameOk
if !renameOk
  Fail('rename did not apply a workspace edit to the Vim buffer')
endif

# Formatting is an actual client operation. Preserve before/after text so a
# no-op formatter remains distinguishable from command failure.
var beforeFormat = getline(1, '$')->join("\n")
var formatError = ''
try
  LspFormat
catch
  formatError = v:exception
endtry
sleep 200m
var afterFormat = getline(1, '$')->join("\n")
cells.formatting = {
  command_succeeded: formatError ==# '',
  changed: beforeFormat !=# afterFormat
}
if formatError !=# ''
  Fail('LspFormat raised: ' .. formatError)
endif

# Workspace configuration is part of the registered subject identity. The
# server listing is retained so root/config divergence is visible in the receipt.
cells.workspace_configuration = {
  include_paths: ['lib'],
  root_search: ['.perl-lsp.toml', 'Makefile.PL', 'Build.PL', 'cpanfile', 'dist.ini', '.git/'],
  listing: execute('LspShowAllServers')
}

# Optional modern cells are recorded, not promoted from the upstream feature list.
cells.semantic_tokens = {configured: true, proven: 'not_exercised'}
cells.inlay_hints = {configured: false, proven: 'not_exercised'}
cells.workspace_folders = {proven: 'not_exercised'}

# Shutdown through the public client lifecycle.
g:LspDisable()
var stopped = WaitFor(() => !g:LspServerRunning('perl'), 7000)
cells.clean_shutdown = stopped
if !stopped
  Fail('yegappan/lsp did not stop the Perl server cleanly')
endif

var preliminary = failures->empty() ? 'support_row_candidate' :
  (cells->get('attach_and_root', {})->get('ready', false) ? 'configuration_only' : 'not_proven')
var result = {
  schema_version: 1,
  kind: 'vim_yegappan_lsp_evaluation',
  vim_version: execute('version')->split("\n")[0],
  client_ref: '6ab67121fa1364d95e4f282580d99b6aa85f808a',
  client_dir: clientDir,
  perllsp: perllsp,
  activation_receipt: expand('$PERLLSP_YEG_ACTIVATION'),
  cells: cells,
  failures: failures,
  ok: failures->empty(),
  preliminary_disposition: preliminary,
  final_disposition: 'pending_reviewed_actual_host_evidence'
}
writefile([json_encode(result)], receipt)
if !failures->empty()
  cquit 2
endif
qa!
VIM

rc=0
"${vim_bin}" -Nu NONE -n -es -S "${tmpdir}/evaluate.vim" || rc=$?
if [[ ! -f "${receipt}" ]]; then
  echo "yegappan/lsp evaluation FAILED: receipt missing" >&2
  exit 2
fi
cat "${receipt}"
echo
exit "${rc}"
