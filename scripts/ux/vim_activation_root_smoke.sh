#!/usr/bin/env bash
# Real-Vim activation/root receipt for #7762.
#
# The default pass proves native Vim filetype discrimination and the canonical
# root-selection contract. `--integration` additionally loads a pinned local
# checkout of prabirshrestha/vim-lsp and launches the exact perllsp candidate.
# Missing tools are failures: a skipped editor/plugin/binary is not evidence.
#
# Usage:
#   ./scripts/ux/vim_activation_root_smoke.sh
#   VIM_LSP_DIR=/path/to/vim-lsp PERLLSP=/path/to/perllsp \
#     ./scripts/ux/vim_activation_root_smoke.sh --integration
#
# Environment:
#   VIM          Vim executable (default: vim)
#   VIM_LSP_DIR  pinned vim-lsp checkout; required by --integration
#   PERLLSP      exact perllsp binary; required by --integration
#   RECEIPT      output JSON path (default: temporary path printed on success)

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
contract="${repo_root}/.ci/editor-clients/vim-vim-lsp-activation-root.v1.json"
vim_bin=${VIM:-vim}
mode=contract
if [[ ${1:-} == "--integration" ]]; then
  mode=integration
elif [[ $# -gt 0 ]]; then
  echo "usage: $0 [--integration]" >&2
  exit 64
fi

if ! command -v "${vim_bin}" >/dev/null 2>&1; then
  echo "vim activation/root FAILED: Vim executable not found: ${vim_bin}" >&2
  exit 1
fi
if [[ ! -f "${contract}" ]]; then
  echo "vim activation/root FAILED: contract missing: ${contract}" >&2
  exit 1
fi

# A Vim built without these cannot produce the receipt at all: `json_encode`
# would abort the run, and in integration mode vim-lsp needs jobs, channels,
# timers and lambdas to reach `lsp_server_init`. A build that silently lacks
# them must fail here rather than emit a receipt that looks like evidence.
require_features() {
  local missing=()
  local feature
  for feature in "$@"; do
    if ! "${vim_bin}" -Nu NONE -n -es -c "if !has('${feature}') | cquit 3 | endif" -c 'qa!' >/dev/null 2>&1; then
      missing+=("+${feature}")
    fi
  done
  if [[ ${#missing[@]} -gt 0 ]]; then
    echo "vim activation/root FAILED: ${vim_bin} lacks required features: ${missing[*]}" >&2
    exit 1
  fi
}
require_features eval

if [[ ${mode} == integration ]]; then
  require_features job channel timers lambda reltime
  : "${VIM_LSP_DIR:?VIM_LSP_DIR must point at a pinned vim-lsp checkout}"
  : "${PERLLSP:?PERLLSP must point at the exact perllsp candidate}"
  if [[ ! -f "${VIM_LSP_DIR}/plugin/lsp.vim" ]]; then
    echo "vim activation/root FAILED: vim-lsp plugin not found under ${VIM_LSP_DIR}" >&2
    exit 1
  fi
  if [[ ! -x "${PERLLSP}" ]]; then
    echo "vim activation/root FAILED: perllsp is not executable: ${PERLLSP}" >&2
    exit 1
  fi
fi

tmpdir=$(mktemp -d)
cleanup() {
  if [[ -n ${tmpdir:-} && -d ${tmpdir} ]]; then
    rm -rf "${tmpdir}"
  fi
}
trap cleanup EXIT

receipt=${RECEIPT:-"${tmpdir}/vim-activation-root-receipt.json"}
mkdir -p "$(dirname "${receipt}")"

# Filetype fixtures. Native detection is always measured before any custom
# autocmd or LSP registration is installed.
#
# These live in their own subtree, never beside the root fixtures: several of
# them (`cpanfile` in particular) are themselves canonical root markers, and a
# shared parent directory would make the nearest-marker walk terminate on a
# filetype fixture instead of exercising the intended root case.
ftdir="${tmpdir}/filetypes"
mkdir -p "${ftdir}/bin" "${ftdir}/script"
cat >"${ftdir}/sample.pl" <<'EOF'
use strict;
my $value = 1;
EOF
# Distinct stem, not a case variant of `sample.pl`: the uppercase-extension
# case must still be observable on case-insensitive filesystems (macOS, Windows)
# where `sample.pl` and `sample.PL` are the same file.
cp "${ftdir}/sample.pl" "${ftdir}/legacy.PL"
cat >"${ftdir}/Sample.pm" <<'EOF'
package Sample;
use strict;
1;
EOF
cat >"${ftdir}/Image.pm" <<'EOF'
/* XPM */
static char * icon[] = { "1 1 1 1", "a c #000000", "a" };
EOF
cat >"${ftdir}/sample.t" <<'EOF'
use strict;
use Test::More tests => 1;
ok 1;
EOF
cat >"${ftdir}/game.t" <<'EOF'
#charset "us-ascii"
#include <adv3.h>
EOF
cat >"${ftdir}/app.psgi" <<'EOF'
use strict;
sub { [200, ['Content-Type' => 'text/plain'], ['ok']] };
EOF
cat >"${ftdir}/app.cgi" <<'EOF'
#!/usr/bin/env perl
use strict;
print "Content-Type: text/plain\n\nok\n";
EOF
cat >"${ftdir}/app.fcgi" <<'EOF'
#!/usr/bin/env perl
use strict;
print "ok\n";
EOF
cat >"${ftdir}/cpanfile" <<'EOF'
requires 'Test::More';
EOF
for path in "${ftdir}/bin/tool" "${ftdir}/script/tool"; do
  cat >"${path}" <<'EOF'
#!/usr/bin/env perl
use strict;
print "ok\n";
EOF
done
cat >"${ftdir}/notes.pod" <<'EOF'
=head1 NAME
Example
=cut
EOF
cat >"${ftdir}/Native.xs" <<'EOF'
MODULE = Native PACKAGE = Native
EOF
printf '<%%= $value %%>\n' >"${ftdir}/view.ep"
printf '[%% value %%]\n' >"${ftdir}/view.tt"
printf '[%% value %%]\n' >"${ftdir}/view.tt2"
printf '<%%perl>my $x = 1;</%%perl>\n' >"${ftdir}/view.mason"

# Root fixtures. `.git` is deliberately outer in one case so a nearer Perl
# marker has to win.
mkdir -p \
  "${tmpdir}/roots/perl-config/lib" \
  "${tmpdir}/roots/makefile/lib" \
  "${tmpdir}/roots/git-only/lib" \
  "${tmpdir}/roots/monorepo/.git" \
  "${tmpdir}/roots/monorepo/subproject/lib" \
  "${tmpdir}/roots/no-marker"
: >"${tmpdir}/roots/perl-config/.perl-lsp.toml"
: >"${tmpdir}/roots/perl-config/lib/App.pm"
: >"${tmpdir}/roots/makefile/Makefile.PL"
: >"${tmpdir}/roots/makefile/lib/App.pm"
mkdir -p "${tmpdir}/roots/git-only/.git"
: >"${tmpdir}/roots/git-only/lib/App.pm"
: >"${tmpdir}/roots/monorepo/subproject/cpanfile"
: >"${tmpdir}/roots/monorepo/subproject/lib/App.pm"
: >"${tmpdir}/roots/no-marker/solo.pl"

export PERLLSP_VIM_CONTRACT="${contract}"
export PERLLSP_VIM_TMP="${tmpdir}"
export PERLLSP_VIM_RECEIPT="${receipt}"
export PERLLSP_VIM_MODE="${mode}"
export PERLLSP_VIM_LSP_DIR="${VIM_LSP_DIR:-}"
export PERLLSP_VIM_BIN="${PERLLSP:-}"

cat >"${tmpdir}/verify.vim" <<'VIM'
set nocompatible
set nomore
filetype on

let s:tmp = expand('$PERLLSP_VIM_TMP')
let s:contract = json_decode(join(readfile(expand('$PERLLSP_VIM_CONTRACT')), "\n"))
let s:receipt_path = expand('$PERLLSP_VIM_RECEIPT')
let s:mode = expand('$PERLLSP_VIM_MODE')
let s:failures = []
let s:filetypes = []
let s:roots = []

function! s:RecordFailure(message) abort
  call add(s:failures, a:message)
endfunction

function! s:ObserveFiletype(row) abort
  execute 'silent edit ' . fnameescape(s:tmp . '/filetypes/' . a:row.path)
  let l:observed = &l:filetype
  let l:fixed = a:row.expect !=# 'observe'
  let l:ok = !l:fixed || l:observed ==# a:row.expect
  call add(s:filetypes, {
        \ 'case': a:row.case,
        \ 'path': a:row.path,
        \ 'expected': a:row.expect,
        \ 'observed': l:observed,
        \ 'fixed_expectation': l:fixed,
        \ 'ok': l:ok,
        \ })
  if !l:ok
    call s:RecordFailure('filetype ' . a:row.case . ': expected ' . a:row.expect . ', got ' . string(l:observed))
  endif
  silent! bwipeout!
endfunction

for s:row in s:contract.filetypes
  call s:ObserveFiletype(s:row)
endfor

" The ascent stops at the fixture root. Without that bound a "no marker
" anywhere" case would keep climbing into the host filesystem and could pick up
" an unrelated `.git`/`cpanfile` above the temporary directory, making the
" no-marker fallback pass or fail for reasons that have nothing to do with the
" contract.
function! s:NearestRoot(path) abort
  let l:stop = fnamemodify(s:tmp, ':p:h')
  let l:dir = fnamemodify(a:path, ':p:h')
  while 1
    for l:marker in s:contract.root.markers
      let l:candidate = l:dir . '/' . l:marker
      if filereadable(l:candidate) || isdirectory(l:candidate)
        return fnamemodify(l:dir, ':p')
      endif
    endfor
    let l:parent = fnamemodify(l:dir, ':h')
    if l:parent ==# l:dir || l:dir ==# l:stop
      return ''
    endif
    let l:dir = l:parent
  endwhile
endfunction

function! s:CheckRoot(name, relative_path, expected_relative) abort
  let l:path = s:tmp . '/' . a:relative_path
  let l:expected = fnamemodify(s:tmp . '/' . a:expected_relative, ':p')
  let l:observed = s:NearestRoot(l:path)
  if empty(l:observed) && s:contract.root.no_marker ==# 'cwd_fallback'
    let l:observed = fnamemodify(getcwd(), ':p')
  endif
  let l:ok = l:observed ==# l:expected
  call add(s:roots, {
        \ 'case': a:name,
        \ 'path': a:relative_path,
        \ 'expected': l:expected,
        \ 'observed': l:observed,
        \ 'ok': l:ok,
        \ })
  if !l:ok
    call s:RecordFailure('root ' . a:name . ': expected ' . l:expected . ', got ' . string(l:observed))
  endif
endfunction

call s:CheckRoot('perl_config', 'roots/perl-config/lib/App.pm', 'roots/perl-config')
call s:CheckRoot('makefile_pl', 'roots/makefile/lib/App.pm', 'roots/makefile')
call s:CheckRoot('git_only', 'roots/git-only/lib/App.pm', 'roots/git-only')
call s:CheckRoot('nearer_perl_marker_beats_outer_git', 'roots/monorepo/subproject/lib/App.pm', 'roots/monorepo/subproject')

" No-marker semantics are explicitly cwd fallback, matching the current
" first-party vim-lsp recipe rather than fabricating a file parent root.
execute 'lcd ' . fnameescape(s:tmp . '/roots/no-marker')
call s:CheckRoot('no_marker_cwd', 'roots/no-marker/solo.pl', 'roots/no-marker')
execute 'lcd ' . fnameescape(s:tmp)

let s:integration = {'requested': s:mode ==# 'integration', 'ok': v:null}
if s:mode ==# 'integration'
  let s:vim_lsp_dir = expand('$PERLLSP_VIM_LSP_DIR')
  let s:perllsp = expand('$PERLLSP_VIM_BIN')
  execute 'set runtimepath^=' . fnameescape(s:vim_lsp_dir)
  let g:lsp_auto_enable = 0
  let g:lsp_log_verbose = 1
  let g:lsp_log_file = s:tmp . '/vim-lsp.log'
  runtime plugin/lsp.vim

  let g:perllsp_server_init = 0
  let g:perllsp_buffer_enabled = 0
  let g:perllsp_root_callback = ''
  augroup perllsp_vim_receipt
    autocmd!
    autocmd User lsp_server_init let g:perllsp_server_init = 1
    autocmd User lsp_buffer_enabled let g:perllsp_buffer_enabled = 1
  augroup END

  function! s:PerllspRootUri(server_info) abort
    let l:root = lsp#utils#find_nearest_parent_file_directory(expand('%:p'), s:contract.root.markers)
    if empty(l:root)
      let l:root = getcwd()
    endif
    let g:perllsp_root_callback = fnamemodify(l:root, ':p')
    return lsp#utils#path_to_uri(l:root)
  endfunction

  call lsp#register_server({
        \ 'name': 'perllsp-under-test',
        \ 'cmd': {server_info -> [s:perllsp, '--stdio']},
        \ 'allowlist': ['perl'],
        \ 'root_uri': function('s:PerllspRootUri'),
        \ })
  call lsp#enable()

  execute 'silent edit ' . fnameescape(s:tmp . '/roots/perl-config/lib/App.pm')
  let s:deadline = reltimefloat(reltime()) + 8.0
  while (!g:perllsp_server_init || !g:perllsp_buffer_enabled) && reltimefloat(reltime()) < s:deadline
    sleep 50m
  endwhile
  let s:expected_root = fnamemodify(s:tmp . '/roots/perl-config', ':p')
  let s:integration = {
        \ 'requested': v:true,
        \ 'server_init': g:perllsp_server_init,
        \ 'buffer_enabled': g:perllsp_buffer_enabled,
        \ 'root_callback': g:perllsp_root_callback,
        \ 'expected_root': s:expected_root,
        \ 'ok': g:perllsp_server_init && g:perllsp_buffer_enabled && g:perllsp_root_callback ==# s:expected_root,
        \ 'vim_lsp_log': g:lsp_log_file,
        \ }
  if !s:integration.ok
    call s:RecordFailure('vim-lsp integration did not initialize/attach at the expected root')
  endif
  silent! call lsp#stop_server('perllsp-under-test')
endif

let s:receipt = {
      \ 'schema_version': 1,
      \ 'kind': 'vim_activation_root',
      \ 'vim_version': execute('version'),
      \ 'contract': expand('$PERLLSP_VIM_CONTRACT'),
      \ 'mode': s:mode,
      \ 'filetypes': s:filetypes,
      \ 'roots': s:roots,
      \ 'integration': s:integration,
      \ 'failures': s:failures,
      \ 'ok': empty(s:failures),
      \ }
call writefile([json_encode(s:receipt)], s:receipt_path)

if !empty(s:failures)
  for s:failure in s:failures
    echomsg 'vim activation/root FAILED: ' . s:failure
  endfor
  cquit 2
endif
qa!
VIM

if ! "${vim_bin}" -Nu NONE -n -es -S "${tmpdir}/verify.vim"; then
  echo "vim activation/root FAILED" >&2
  if [[ -f "${receipt}" ]]; then
    cat "${receipt}" >&2
  fi
  if [[ -f "${tmpdir}/vim-lsp.log" ]]; then
    echo "--- vim-lsp log ---" >&2
    cat "${tmpdir}/vim-lsp.log" >&2
  fi
  exit 2
fi

if [[ ! -f "${receipt}" ]]; then
  echo "vim activation/root FAILED: receipt was not written" >&2
  exit 2
fi

# Preserve a caller-selected receipt; for the default temporary receipt, print
# it before cleanup so the run is still inspectable in CI logs.
cat "${receipt}"
echo
