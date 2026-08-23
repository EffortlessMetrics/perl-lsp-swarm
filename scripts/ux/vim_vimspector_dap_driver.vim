set nocompatible
set nomore
set hidden

let s:vimspector_dir = expand('$PERLLSP_VIMSPECTOR_DIR')
let s:workspace = expand('$PERLLSP_DAP_WORKSPACE')
let s:receipt = expand('$PERLLSP_DAP_RECEIPT')
let s:adapter = expand('$PERLLSP_DAP_ADAPTER')
let s:expected_source = expand('$PERLLSP_DAP_EXPECTED_SOURCE')
let s:shadow_source = expand('$PERLLSP_DAP_SHADOW_SOURCE')
let s:stderr_artifact = expand('$PERLLSP_DAP_STDERR_ARTIFACT')
let s:output_artifact = expand('$PERLLSP_DAP_OUTPUT_ARTIFACT')
let s:stage = expand('$PERLLSP_DAP_STAGE')
let s:failures = []
let s:cells = {}
let g:perl_dap_frame_events = 0
let g:perl_dap_debug_ended = 0
let g:perl_dap_ui_created = 0

function! s:Fail(msg) abort
  call add(s:failures, a:msg)
endfunction

" Poll a predicate to a deadline. The predicate is evaluated defensively: several
" callers use py3eval against Vimspector's session object, which can raise while
" the session is still being built. An uncaught throw here used to abort this
" function (and, through `abort`, the whole driver) so no receipt was written at
" all; swallowing it per poll keeps transient failures survivable while still
" reporting a predicate that never stopped throwing.
function! s:Wait(expr, timeout_ms) abort
  let l:start = reltime()
  let l:last_error = ''
  while 1
    try
      if eval(a:expr)
        return 1
      endif
      let l:last_error = ''
    catch
      let l:last_error = v:exception
    endtry
    if reltimefloat(reltime(l:start)) * 1000.0 > a:timeout_ms
      if !empty(l:last_error)
        call s:Fail('wait predicate kept erroring: ' . a:expr . ': ' . l:last_error)
      endif
      return 0
    endif
    sleep 25m
  endwhile
endfunction

function! s:WindowLines(name) abort
  if !exists('g:vimspector_session_windows') || !has_key(g:vimspector_session_windows, a:name)
    return []
  endif
  let l:winid = g:vimspector_session_windows[a:name]
  let l:buf = winbufnr(l:winid)
  return l:buf > 0 ? getbufline(l:buf, 1, '$') : []
endfunction

function! s:NamedBufferLines(name) abort
  let l:buf = bufnr(a:name)
  return l:buf > 0 ? getbufline(l:buf, 1, '$') : []
endfunction

execute 'set runtimepath^=' . fnameescape(s:vimspector_dir)
let g:vimspector_enable_mappings = ''
runtime plugin/vimspector.vim
" Upstream spells its load guard `g:loaded_vimpector` (missing the 's') in
" plugin/vimspector.vim at the pinned ref. Do not "correct" this name.
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
execute 'silent edit ' . fnameescape(s:expected_source)
" Deliberately park the cursor away from the breakpoint line. Pre-positioning it
" on line 4 made stopped_source_line a circular oracle: the cell asserted the very
" cursor position the driver had just set, so it read true even when the debugger
" never stopped at all.
call cursor(1, 1)
call vimspector#SetLineBreakpoint(s:expected_source, 4)

try
  call vimspector#Launch()
catch
  call s:Fail('Vimspector launch raised: ' . v:exception)
endtry

if !s:Wait('g:perl_dap_ui_created > 0', 10000)
  call s:Fail('Vimspector UI was not created')
endif
let s:init_launch_settled = s:Wait(
      \ "py3eval('_vimspector_session is not None and _vimspector_session._connection is not None and _vimspector_session._init_complete and _vimspector_session._launch_complete')",
      \ 10000)
let s:connection_present = 0
try
  let s:connection_present = py3eval('_vimspector_session is not None and _vimspector_session._connection is not None')
catch
  call s:Fail('could not read Vimspector session connection state: ' . v:exception)
endtry
let s:cells.adapter_launch = g:perl_dap_ui_created > 0 && s:connection_present
let s:cells.initialize_launch = s:init_launch_settled
if !s:cells.adapter_launch
  call s:Fail('Vimspector did not retain a live adapter connection')
endif
if !s:cells.initialize_launch
  call s:Fail('DAP initialize/launch did not settle before the deadline')
endif

let s:frame_observed = s:Wait('g:perl_dap_frame_events > 0', 15000)
if !s:frame_observed
  call s:Fail('no Vimspector stopped-frame event observed')
endif

let s:breakpoints = []
try
  let s:breakpoints = vimspector#GetBreakpointsAsQuickFix()
catch
  call s:Fail('could not read Vimspector breakpoint model: ' . v:exception)
endtry
let s:matching_bp = filter(copy(s:breakpoints),
      \ {_, bp -> get(bp, 'type', '') ==# 'L'
      \   && fnamemodify(get(bp, 'filename', ''), ':p') ==# fnamemodify(s:expected_source, ':p')
      \   && get(bp, 'lnum', 0) == 4})
let s:verified_bp = len(s:matching_bp) == 1
      \ && get(s:matching_bp[0], 'valid', 0) == 1
      \ && get(s:matching_bp[0], 'text', '') =~# 'VERIFIED'
let s:cells.verified_breakpoint = s:verified_bp
if !s:verified_bp
  call s:Fail('Vimspector did not expose a VERIFIED breakpoint at the expected source line')
endif

let s:stopped_path = fnamemodify(expand('%:p'), ':p')
let s:stopped_line = line('.')
let s:expected_path = fnamemodify(s:expected_source, ':p')
let s:shadow_path = fnamemodify(s:shadow_source, ':p')
" Both cells require a real stopped-frame event: the buffer under the cursor is
" only evidence of where the debugger jumped if the debugger actually jumped.
let s:cells.stopped_source_line = s:frame_observed
      \ && s:stopped_path ==# s:expected_path && s:stopped_line == 4
let s:cells.source_discriminator = s:frame_observed
      \ && s:stopped_path ==# s:expected_path
      \ && s:stopped_path !=# s:shadow_path
if !s:cells.stopped_source_line
  call s:Fail('debugger did not stop at exact debug_me.pl:4; got ' . s:stopped_path . ':' . s:stopped_line)
endif
if !s:cells.source_discriminator
  if s:stopped_path ==# s:shadow_path
    call s:Fail('same-named shadow source satisfied the stopped-frame path')
  else
    call s:Fail('no stopped frame resolved to the exact expected source; got '
          \ . s:stopped_path)
  endif
endif

let s:stack = s:WindowLines('stack_trace')
let s:variables = s:WindowLines('variables')
let s:cells.stack_trace_visible = !empty(s:stack) && string(s:stack) =~# 'debug_me.pl'
let s:cells.variables_visible = !empty(s:variables) && string(s:variables) =~# 'value'
if !s:cells.stack_trace_visible
  call s:Fail('Vimspector stack trace did not expose debug_me.pl')
endif
if !s:cells.variables_visible
  call s:Fail('Vimspector variables window did not expose value')
endif

let s:evaluate = {'state': 'not_proven'}
try
  call vimspector#AddWatch('$value')
  if s:Wait("string(s:WindowLines('watches')) =~# 'value'", 5000)
    let s:evaluate = {'state': 'observed', 'lines': s:WindowLines('watches')}
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

let s:session_id = ''
try
  let s:session_id = vimspector#GetSessionID()
catch
  call s:Fail('could not read Vimspector session id: ' . v:exception)
endtry
let s:console_name = 'vimspector.Console[' . s:session_id . ']'
let s:server_name = 'vimspector.Output:server[' . s:session_id . ']'
call s:Wait("string(s:NamedBufferLines(s:console_name)) =~# 'value=42'", 5000)
let s:console_lines = s:NamedBufferLines(s:console_name)
let s:server_lines = s:NamedBufferLines(s:server_name)
call writefile(s:console_lines, s:output_artifact)
call writefile(s:server_lines, s:stderr_artifact)
let s:cells.debuggee_output_visible = string(s:console_lines) =~# 'value=42'
if !s:cells.debuggee_output_visible
  call s:Fail('debuggee output value=42 was not consumed in the Vimspector console')
endif

try
  call vimspector#Reset({'interactive': v:false})
catch
  call s:Fail('Vimspector reset raised: ' . v:exception)
endtry
let s:client_connection_closed = s:Wait("py3eval('_vimspector_session is None or _vimspector_session._connection is None')", 7000)
let s:ui_closed = s:Wait("py3eval('_vimspector_session is None or _vimspector_session._uiTab is None')", 7000)
let s:cells.clean_shutdown = g:perl_dap_debug_ended > 0 && s:client_connection_closed && s:ui_closed
if !s:cells.clean_shutdown
  call s:Fail('Vimspector session state remained live after reset')
endif

let s:cells.adapter_identity_bound = filereadable(expand('$PERLLSP_DAP_IDENTITY'))
      \ && strlen(expand('$PERLLSP_DAP_ADAPTER_SHA')) == 64
if !s:cells.adapter_identity_bound
  call s:Fail('adapter identity/hash evidence was not bound')
endif

let s:result = {
      \ 'schema_version': 2,
      \ 'kind': 'vim_vimspector_perl_dap',
      \ 'stage': s:stage,
      \ 'vim_version': split(execute('version'), "\n")[0],
      \ 'vimspector_dir': s:vimspector_dir,
      \ 'adapter': s:adapter,
      \ 'adapter_sha256': expand('$PERLLSP_DAP_ADAPTER_SHA'),
      \ 'adapter_identity': expand('$PERLLSP_DAP_IDENTITY'),
      \ 'driver_sha256': expand('$PERLLSP_DAP_DRIVER_SHA'),
      \ 'workspace': s:workspace,
      \ 'configuration_sha256': expand('$PERLLSP_DAP_CONFIG_SHA'),
      \ 'fixture_sha256': expand('$PERLLSP_DAP_FIXTURE_SHA'),
      \ 'source': {
      \   'expected': s:expected_path,
      \   'shadow': s:shadow_path,
      \   'stopped': s:stopped_path,
      \   'line': s:stopped_line,
      \ },
      \ 'breakpoints': s:breakpoints,
      \ 'cells': s:cells,
      \ 'stack_lines': s:stack,
      \ 'variable_lines': s:variables,
      \ 'failures': s:failures,
      \ 'ok': empty(s:failures),
      \ }
call writefile([json_encode(s:result)], s:receipt)
if !empty(s:failures)
  cquit 2
endif
qa!
