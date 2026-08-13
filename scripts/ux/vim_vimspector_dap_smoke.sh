#!/usr/bin/env bash
# Real Vim + Vimspector + perl-dap launch receipt for #7702.
#
# Uses Vimspector's actual project configuration, breakpoint, frame, stack,
# variable/watch, stepping, and session-end UI surfaces. A DAP unit test or
# direct adapter peer cannot satisfy this receipt.
#
# Required:
#   VIMSPECTOR_DIR=/path/to/pinned/puremourning/vimspector
#   PERL_DAP=/path/to/exact/perl-dap
#
# Optional:
#   VIM=/path/to/vim
#   PERL=/path/to/perl
#   PUBLIC_PERL_DAP=/path/to/release-shaped/perl-dap
#   RECEIPT_DIR=/path/to/output-directory

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
vim_bin=${VIM:-vim}
perl_bin=${PERL:-perl}
: "${VIMSPECTOR_DIR:?VIMSPECTOR_DIR must point at pinned Vimspector}"
: "${PERL_DAP:?PERL_DAP must point at exact perl-dap}"
out=${RECEIPT_DIR:-"${repo_root}/target/receipts/vimspector-perl-dap"}
mkdir -p "${out}"

if ! command -v "${vim_bin}" >/dev/null 2>&1; then
  echo "Vimspector DAP FAILED: Vim not found: ${vim_bin}" >&2
  exit 1
fi
if ! command -v "${perl_bin}" >/dev/null 2>&1; then
  echo "Vimspector DAP FAILED: Perl not found: ${perl_bin}" >&2
  exit 1
fi
if [[ ! -f "${VIMSPECTOR_DIR}/plugin/vimspector.vim" ]]; then
  echo "Vimspector DAP FAILED: plugin missing under ${VIMSPECTOR_DIR}" >&2
  exit 1
fi
if [[ ! -x "${PERL_DAP}" ]]; then
  echo "Vimspector DAP FAILED: perl-dap not executable: ${PERL_DAP}" >&2
  exit 1
fi
if ! "${vim_bin}" --version | grep -q '+python3'; then
  echo "Vimspector DAP FAILED: Vim must be compiled with +python3" >&2
  exit 1
fi

hash_file() {
  local path=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{print $1}'
  else
    echo unavailable
  fi
}

run_stage() {
  local stage=$1
  local adapter=$2
  local receipt="${out}/${stage}.json"
  local tmpdir
  tmpdir=$(mktemp -d)
  local workspace="${tmpdir}/workspace"
  mkdir -p "${workspace}/lib"

  cat >"${workspace}/debug_me.pl" <<'PERL'
use strict;
use warnings;
my $value = 41;
$value += 1;
my $message = "value=$value";
print "$message\n";
PERL

  # One deterministic launch configuration means Vimspector does not need a
  # user-choice dialog. The adapter path is exact and absolute.
  ADAPTER="${adapter}" PERL_BIN="$(command -v "${perl_bin}")" WORKSPACE="${workspace}" perl -MJSON::PP -e '
    my $cfg = {
      q{$schema} => q{https://puremourning.github.io/vimspector/schema/vimspector.schema.json},
      adapters => {
        q{perl-dap-under-test} => {
          command => [ $ENV{ADAPTER}, q{--stdio} ],
        },
      },
      configurations => {
        q{Launch Perl} => {
          adapter => q{perl-dap-under-test},
          configuration => {
            request => q{launch},
            program => q{${workspaceRoot}/debug_me.pl},
            perlPath => $ENV{PERL_BIN},
            args => [],
            includePaths => [ q{${workspaceRoot}/lib} ],
            cwd => q{${workspaceRoot}},
            env => {},
          },
        },
      },
    };
    open my $fh, q{>}, "$ENV{WORKSPACE}/.vimspector.json" or die $!;
    print {$fh} JSON::PP->new->canonical(1)->pretty(1)->encode($cfg);
    close $fh;
  '

  export PERLLSP_VIMSPECTOR_DIR="${VIMSPECTOR_DIR}"
  export PERLLSP_DAP_WORKSPACE="${workspace}"
  export PERLLSP_DAP_RECEIPT="${receipt}"
  export PERLLSP_DAP_ADAPTER="${adapter}"
  export PERLLSP_DAP_STAGE="${stage}"

  cat >"${tmpdir}/verify.vim" <<'VIM'
set nocompatible
set nomore
set hidden
let s:vimspector_dir = expand('$PERLLSP_VIMSPECTOR_DIR')
let s:workspace = expand('$PERLLSP_DAP_WORKSPACE')
let s:receipt = expand('$PERLLSP_DAP_RECEIPT')
let s:adapter = expand('$PERLLSP_DAP_ADAPTER')
let s:stage = expand('$PERLLSP_DAP_STAGE')
let s:failures = []
let s:cells = {}
let g:perl_dap_frame_events = 0
let g:perl_dap_debug_ended = 0
let g:perl_dap_ui_created = 0

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
function! s:WindowLines(name) abort
  if !exists('g:vimspector_session_windows') || !has_key(g:vimspector_session_windows, a:name)
    return []
  endif
  let l:winid = g:vimspector_session_windows[a:name]
  let l:buf = winbufnr(l:winid)
  return l:buf > 0 ? getbufline(l:buf, 1, '$') : []
endfunction

execute 'set runtimepath^=' . fnameescape(s:vimspector_dir)
let g:vimspector_enable_mappings = ''
runtime plugin/vimspector.vim
if !exists('g:loaded_vimpector')
  call s:Fail('Vimspector did not load')
endif
augroup perllsp_vimspector_receipt
  autocmd!
  autocmd User VimspectorUICreated let g:perl_dap_ui_created += 1
  autocmd User VimspectorJumpedToFrame let g:perl_dap_frame_events += 1
  autocmd User VimspectorDebugEnded let g:perl_dap_debug_ended += 1
augroup END

execute 'lcd ' . fnameescape(s:workspace)
execute 'silent edit ' . fnameescape(s:workspace . '/debug_me.pl')
call cursor(4, 1)
call vimspector#SetLineBreakpoint(expand('%:p'), 4)

try
  call vimspector#LaunchWithSettings(#{ configuration: 'Launch Perl' })
catch
  call s:Fail('Vimspector launch raised: ' . v:exception)
endtry

if !s:Wait('g:perl_dap_frame_events > 0', 15000)
  call s:Fail('no Vimspector stopped-frame event observed')
endif
let s:stopped_path = expand('%:p')
let s:stopped_line = line('.')
let s:cells.adapter_launch = g:perl_dap_ui_created > 0 || g:perl_dap_frame_events > 0
let s:cells.verified_breakpoint = g:perl_dap_frame_events > 0
let s:cells.stopped_source_line = fnamemodify(s:stopped_path, ':p') ==# fnamemodify(s:workspace . '/debug_me.pl', ':p') && s:stopped_line == 4
if !s:cells.stopped_source_line
  call s:Fail('debugger did not stop at debug_me.pl:4; got ' . s:stopped_path . ':' . s:stopped_line)
endif

let s:stack = s:WindowLines('stack_trace')
let s:variables = s:WindowLines('variables')
let s:cells.stack_trace_visible = !empty(s:stack) && string(s:stack) =~# 'debug_me.pl'
let s:cells.variables_visible = !empty(s:variables) && string(s:variables) =~# 'value'
if !s:cells.stack_trace_visible | call s:Fail('Vimspector stack trace did not expose debug_me.pl') | endif
if !s:cells.variables_visible | call s:Fail('Vimspector variables window did not expose value') | endif

" Evaluate/watch is optional at the product level, but when the adapter/client
" accepts it we retain the actual visible watch result. Failure is classified
" rather than fabricated from adapter logs.
let s:evaluate = {'state': 'not_proven'}
try
  call vimspector#AddWatch('$value')
  if s:Wait("string(s:WindowLines('watches')) =~# 'value'", 5000)
    let s:watch_lines = s:WindowLines('watches')
    let s:evaluate = {'state': 'observed', 'lines': s:watch_lines}
  else
    let s:evaluate = {'state': 'unsupported_or_no_visible_result'}
  endif
catch
  let s:evaluate = {'state': 'unsupported_or_error', 'error': v:exception}
endtry
let s:cells.evaluate_watch = s:evaluate

let s:before_step = g:perl_dap_frame_events
let s:before_line = line('.')
try
  call vimspector#StepOver()
catch
  call s:Fail('StepOver raised: ' . v:exception)
endtry
if !s:Wait('g:perl_dap_frame_events > s:before_step', 10000)
  call s:Fail('StepOver produced no new frame event')
endif
let s:after_step_line = line('.')
let s:cells.step_over = g:perl_dap_frame_events > s:before_step && s:after_step_line != s:before_line
if !s:cells.step_over
  call s:Fail('StepOver did not advance the actual Vim frame')
endif

try
  call vimspector#Continue()
catch
  call s:Fail('Continue raised: ' . v:exception)
endtry
if !s:Wait('g:perl_dap_debug_ended > 0', 15000)
  call s:Fail('debug session did not end after Continue')
endif
let s:cells.continue_to_end = g:perl_dap_debug_ended > 0

try
  call vimspector#Reset({'interactive': v:false})
catch
endtry
sleep 200m
let s:cells.clean_shutdown = g:perl_dap_debug_ended > 0

let s:result = {
      \ 'schema_version': 1,
      \ 'kind': 'vim_vimspector_perl_dap',
      \ 'stage': s:stage,
      \ 'vim_version': split(execute('version'), "\n")[0],
      \ 'vimspector_dir': s:vimspector_dir,
      \ 'adapter': s:adapter,
      \ 'workspace': s:workspace,
      \ 'cells': s:cells,
      \ 'stack_lines': s:stack,
      \ 'variable_lines': s:variables,
      \ 'failures': s:failures,
      \ 'ok': empty(s:failures),
      \ }
call writefile([json_encode(s:result)], s:receipt)
if !empty(s:failures) | cquit 2 | endif
qa!
VIM

  local rc=0
  "${vim_bin}" -Nu NONE -n -es -S "${tmpdir}/verify.vim" || rc=$?
  if [[ ! -f ${receipt} ]]; then
    echo "Vimspector DAP FAILED: receipt missing for ${stage}" >&2
    rm -rf "${tmpdir}"
    return 2
  fi
  cat "${receipt}"
  echo
  rm -rf "${tmpdir}"
  return "${rc}"
}

run_stage exact_source_local "${PERL_DAP}"

if [[ -n ${PUBLIC_PERL_DAP:-} ]]; then
  [[ -x ${PUBLIC_PERL_DAP} ]] || { echo "PUBLIC_PERL_DAP is not executable" >&2; exit 1; }
  run_stage public_artifact "${PUBLIC_PERL_DAP}"
else
  cat >"${out}/public_artifact.json" <<EOF
{"schema_version":1,"kind":"vim_vimspector_perl_dap","stage":"public_artifact","ok":false,"state":"not_proven","reason":"PUBLIC_PERL_DAP_not_supplied"}
EOF
fi

cat >"${out}/subject.txt" <<EOF
schema_version=1
vimspector_ref=34099d18d8957bb3db5f396c8ca993ffb246a437
perl_dap=${PERL_DAP}
perl_dap_sha256=$(hash_file "${PERL_DAP}")
public_perl_dap=${PUBLIC_PERL_DAP:-not_supplied}
attach=not_proven
EOF
