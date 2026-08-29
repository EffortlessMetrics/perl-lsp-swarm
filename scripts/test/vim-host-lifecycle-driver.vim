" Bounded driver for the #11401 host-reopen lifecycle hermetic journey.
"
" Same ownership laws as scripts/test/vim-host-driver.vim (#10944),
" scripts/test/vim-host-diagnostics-driver.vim (#10946), and
" scripts/test/vim-host-freshness-driver.vim (#11390): this driver stays
" thin — it sequences one host session's journey through the thin adapter,
" emits one typed JSON event per barrier for the Rust supervisor, and exits
" nonzero on any failure. It owns no process supervision, no deadline policy,
" no receipt writing, and no semantic expectations: the session role arrives
" from the Rust scenario through the environment and only the mechanism lives
" here. The multi-host sequence, the process ledgers, the settle probes, and
" the joined judgment are Rust-owned.
"
" Vimscript boundary laws (#12589, binding this driver too):
"   - barrier expressions are eval'd in the adapter's scope: counts and
"     generation numbers are baked into the expression string, never
"     referenced through this script's s: variables;
"   - no `let l:` at script level (function-only scope; script state is s:);
"   - paths compare through the win32unix-normalized fixture-relative form.
"
" Environment contract beyond the #10944 adapter's:
"   PERLLSP_VIM_HOST_ADAPTER             thin adapter path
"   PERLLSP_VIM_HOST_EVENT_FILE          driver event JSONL target
"   PERLLSP_VIM_HOST_CAPABILITY_SNAPSHOT initialize capability snapshot
"   PERLLSP_VIM_HOST_FIXTURE_ROOT        materialized fixture root
"   PERLLSP_VIM_HOST_CANDIDATE_SHA256    planned candidate digest
"   PERLLSP_VIM_HOST_BUDGET_MS           per-barrier wait budget
"   PERLLSP_VIM_HOST_LIFECYCLE_ROLE      full_lifecycle_session|
"                                        replacement_host_session|
"                                        assertion_failure_session|
"                                        timeout_interruption_session|
"                                        server_restart_relabel_session
"   PERLLSP_VIM_HOST_LIFECYCLE_VARIANT   canonical|server_restart_relabel
"   PERLLSP_VIM_HOST_OPENED_FILE_REL     governed source file, fixture-relative
"   PERLLSP_VIM_HOST_EXPECTED_ROOT_REL   governed root, fixture-relative
"   PERLLSP_VIM_HOST_MUTATION_LINE       1-based governed mutation line
"   PERLLSP_VIM_HOST_CLEAN_SOURCE_TEXT   the clean generation bytes
"   PERLLSP_VIM_HOST_DEFECT_SOURCE_TEXT  the defective generation bytes
"   PERLLSP_VIM_HOST_CLEAN_LINE_TEXT     clean generation, mutation line only
"   PERLLSP_VIM_HOST_DEFECT_LINE_TEXT    defect generation, mutation line only
"   PERLLSP_VIM_HOST_LATE_WINDOW_MS      bounded late-result observation window
"
" The full_lifecycle_session journey (the whole proof, judged by Rust):
"   start Vim -> load pinned vim-lsp -> register canonical server -> open the
"   governed defective source (native detection) -> lsp_server_init ->
"   lsp_buffer_enabled -> capture initialize capabilities -> #7762 root
"   observation -> defect present through the client's own state -> pending
"   documentSymbol #1 started with wire identity -> cancelled by identity
"   ($/cancelRequest) -> zero admissions proven -> pending documentSymbol #2
"   started -> buffer wiped through the real didClose path -> old request
"   completes (delivered) -> external atomic replacement writes the clean
"   generation -> reopen the same path (new document instance, unchanged
"   server generation) -> instance settles clean through its own push ->
"   late result rejected (replacement unchanged across the bounded window)
"   -> pending documentSymbol #3 started and left in flight -> orderly stop
"   -> session settled -> user-equivalent exit (:qa!).
"
" The replacement_host_session journey: bootstrap -> open the same governed
"   path at the disk generation the supervisor wrote -> settles clean through
"   its own chain (no prior session state) -> its own defect edit ->
"   its own fix edit -> session settled -> user-equivalent exit.
"
" The assertion_failure_session journey: bootstrap -> open -> settle -> a
"   typed forced assertion failure (evidence preserved) -> cquit 2.
"
" The timeout_interruption_session journey: bootstrap -> open -> settle -> a
"   deliberate indefinite barrier (the Rust supervisor's hard deadline owns
"   the bound and the kill).
"
" The server_restart_relabel_session journey (negative control): bootstrap ->
"   defect observed -> the client's own server restart (stop + lazy restart)
"   -> reopen -> settle -> session settled -> user-equivalent exit. The
"   journey offers a server restart where the host-reopen cell requires a
"   changed host instance: the Rust judgment must reject it typed.

function! s:Env(name) abort
  let l:value = getenv(a:name)
  return type(l:value) == v:t_string ? l:value : ''
endfunction

let s:adapter = s:Env('PERLLSP_VIM_HOST_ADAPTER')
let s:event_file = s:Env('PERLLSP_VIM_HOST_EVENT_FILE')
let s:capability_path = s:Env('PERLLSP_VIM_HOST_CAPABILITY_SNAPSHOT')
let s:fixture_root = substitute(s:Env('PERLLSP_VIM_HOST_FIXTURE_ROOT'), '\', '/', 'g')
let s:candidate_sha = s:Env('PERLLSP_VIM_HOST_CANDIDATE_SHA256')
let s:budget = str2nr(s:Env('PERLLSP_VIM_HOST_BUDGET_MS'))
let s:server_name = s:Env('PERLLSP_VIM_HOST_SERVER_NAME')
let s:root_markers = split(s:Env('PERLLSP_VIM_HOST_ROOT_MARKERS'), ',', v:false)
let s:role = s:Env('PERLLSP_VIM_HOST_LIFECYCLE_ROLE')
let s:variant = s:Env('PERLLSP_VIM_HOST_LIFECYCLE_VARIANT')
let s:opened_file_rel = s:Env('PERLLSP_VIM_HOST_OPENED_FILE_REL')
let s:expected_root_rel = s:Env('PERLLSP_VIM_HOST_EXPECTED_ROOT_REL')
let s:mutation_line = str2nr(s:Env('PERLLSP_VIM_HOST_MUTATION_LINE'))
let s:clean_text = s:Env('PERLLSP_VIM_HOST_CLEAN_SOURCE_TEXT')
let s:defect_text = s:Env('PERLLSP_VIM_HOST_DEFECT_SOURCE_TEXT')
" The one-line edit path replaces exactly one buffer line, so it receives
" exactly one line: a multiline setline payload corrupts the buffer line with
" embedded NULs and the generation barriers become unreliable.
let s:clean_line_text = s:Env('PERLLSP_VIM_HOST_CLEAN_LINE_TEXT')
let s:defect_line_text = s:Env('PERLLSP_VIM_HOST_DEFECT_LINE_TEXT')
let s:late_window = str2nr(s:Env('PERLLSP_VIM_HOST_LATE_WINDOW_MS'))
if s:budget <= 0
  let s:budget = 20000
endif

if empty(s:adapter) || empty(s:event_file) || empty(s:capability_path)
      \ || empty(s:fixture_root) || empty(s:candidate_sha) || empty(s:server_name)
      \ || empty(s:role) || empty(s:opened_file_rel) || empty(s:expected_root_rel)
      \ || s:mutation_line <= 0 || empty(s:clean_text) || empty(s:defect_text)
      \ || empty(s:clean_line_text) || empty(s:defect_line_text)
      \ || s:late_window <= 0
  echoerr 'vim lifecycle driver: required environment missing, failing closed'
  cquit 3
endif

let s:sequence = 0
let s:failures = []
let s:generation_index = 0

function! s:Fail(message) abort
  call add(s:failures, a:message)
endfunction

function! s:Emit(kind, details) abort
  let s:sequence += 1
  let l:event = {
        \ 'schema_version': 'vim_host_driver.v1',
        \ 'sequence': s:sequence,
        \ 'event': a:kind,
        \ 'details': a:details,
        \ }
  call writefile([json_encode(l:event)], s:event_file, 'a')
endfunction

function! s:WaitFor(expr, timeout_ms) abort
  return VimLspHostWaitFor(a:expr, a:timeout_ms)
endfunction

" Exact-bytes line list for writefile binary mode (the #11390 pattern).
function! s:ExternalFileLines(text) abort
  return split(a:text . "\n", "\n", v:true)
endfunction

" Atomic external replacement of the governed fixture file: a sibling temp
" file renamed over the target. The editor buffer is never touched.
function! s:ExternalReplaceFile(path, text) abort
  let l:temp = a:path . '.lifecycle-replace'
  call writefile(s:ExternalFileLines(a:text), l:temp, 'b')
  if rename(l:temp, a:path) != 0
    echoerr 'vim lifecycle driver: atomic replacement failed for ' . a:path
    cquit 3
  endif
  return v:true
endfunction

" Fixture-relative identity of an absolute path (the #11390 normalization).
function! s:FixtureRel(path) abort
  let l:path = tolower(substitute(a:path, '\', '/', 'g'))
  let l:path = substitute(l:path, '^/\([a-z]\)/', '\1:/', '')
  let l:root = tolower(s:fixture_root)
  if l:path ==# l:root
    return '.'
  endif
  if strpart(l:path, 0, len(l:root) + 1) ==# l:root . '/'
    return strpart(l:path, len(l:root) + 1)
  endif
  return ''
endfunction

" Native-detection listener (the #7762 law).
let s:native_filetype_observed = 0
let s:native_filetype_value = ''
augroup perllsp_vim_host_native_lifecycle
  autocmd!
  autocmd FileType perl let s:native_filetype_observed = 1
        \ | let s:native_filetype_value = expand('<amatch>')
augroup END

" Observe the current client state counts and emit one
" generation_current_observed event with the deterministic barrier already
" satisfied by the caller.
function! s:EmitGenerationCurrent(generation, errors, warnings) abort
  let s:generation_index += 1
  let l:counts = VimLspHostBufferDiagnosticsCounts()
  call s:Emit('generation_current_observed', {
        \ 'generation_index': string(s:generation_index),
        \ 'generation': a:generation,
        \ 'state_source': 'client_state',
        \ 'barrier': 'diagnostics_event_and_wire',
        \ 'errors': string(get(l:counts, 'error', 0)),
        \ 'warnings': string(get(l:counts, 'warning', 0)),
        \ })
endfunction

" The deterministic settle barrier after a client reopen: the client's own
" diagnostics update event must fire again, its own wire log must carry one
" more publishDiagnostics push, and the state claim must settle (the #11390
" MaterializeBarrier shape; counts are baked into the expressions).
function! s:SettleBarrier(update_before, wire_before, state_expr, state_fail_reason) abort
  let l:update_expr = 'VimLspHostDiagnosticsUpdatedCount() > ' . a:update_before
  let l:wire_expr = 'VimLspHostWireMarkerCount(''textDocument/publishDiagnostics'') >= '
        \ . (a:wire_before + 1)
  if !s:WaitFor(l:update_expr, s:budget)
    call s:Fail('settle_update_event_never_arrived')
    return 0
  endif
  if !s:WaitFor(l:wire_expr, s:budget)
    call s:Fail('settle_wire_push_never_arrived')
    return 0
  endif
  if !VimLspHostSettledStateBarrier(a:state_expr, 1500, s:budget)
    call s:Fail(a:state_fail_reason)
    return 0
  endif
  return 1
endfunction

" ---------------------------------------------------------------- shared
" bootstrap (every session): load, register, open, attach, capabilities,
" root.

let s:vim_version = split(execute('version'), "\n")[0]
call s:Emit('host_started', {'vim_version': s:vim_version, 'session_role': s:role})

execute 'source ' . fnameescape(s:adapter)
call VimLspHostLoadClient()
call s:Emit('client_loaded', {'plugin': 'lsp_vim', 'server_name': s:server_name})

call VimLspHostRegister()
call s:Emit('registration_selected', {
      \ 'cmd': 'perllsp--stdio',
      \ 'candidate_sha256': s:candidate_sha,
      \ 'server_name': s:server_name,
      \ })

call VimLspHostEnable()
call VimLspHostOpenFixture(s:fixture_root . '/' . s:opened_file_rel)
call s:Emit('fixture_opened', {'file': s:opened_file_rel})

if !s:WaitFor('g:perllsp_vim_host_server_init > 0', s:budget)
  call s:Fail('lsp_server_init never fired within budget')
else
  call s:Emit('server_initialized', {'status': VimLspHostServerStatus()})
endif

if !s:WaitFor('g:perllsp_vim_host_buffer_enabled > 0', s:budget)
  call s:Fail('lsp_buffer_enabled never fired within budget')
else
  let s:filetype = VimLspHostEffectiveFiletype()
  let s:detection = s:native_filetype_observed ? 'native_vim' : 'unobserved'
  call s:Emit('buffer_enabled', {
        \ 'filetype': s:filetype,
        \ 'detection': s:detection,
        \ 'filetype_at_detection': s:native_filetype_value,
        \ })
  if s:detection !=# 'native_vim'
    call s:Fail('buffer attachment happened without observed native filetype detection')
  endif
endif

if empty(s:failures)
  let s:capabilities = VimLspHostServerCapabilities()
  call writefile([json_encode(s:capabilities)], s:capability_path)
  let s:position_encoding = get(s:capabilities, 'positionEncoding', 'utf-16')
  call s:Emit('initialize_observed', {
        \ 'capabilities_written': '1',
        \ 'position_encoding': s:position_encoding,
        \ })
endif

if empty(s:failures)
  let s:root_uri = VimLspHostRootUri()
  let s:root_path = substitute(lsp#utils#uri_to_path(s:root_uri), '\', '/', 'g')
  let s:observed_root_rel = s:FixtureRel(s:root_path)
  let s:root_source = 'cwd_fallback'
  let s:root_marker = ''
  for s:marker in s:root_markers
    if filereadable(s:root_path . '/' . s:marker)
      let s:root_source = 'activation_root_marker'
      let s:root_marker = s:marker
      break
    endif
  endfor
  if s:root_source !=# 'activation_root_marker'
    call s:Fail('root did not resolve through an activation marker (marker_file_absent)')
  endif
  call s:Emit('root_selected', {
        \ 'root_source': s:root_source,
        \ 'root_marker': s:root_marker,
        \ 'expected_root': s:expected_root_rel,
        \ 'observed_root': s:observed_root_rel,
        \ })
  if s:observed_root_rel !=# s:expected_root_rel
    call s:Fail('root_mismatch')
  endif
endif

if !VimLspHostWaitForWireMarkerCount('textDocument/publishDiagnostics', 1, s:budget)
  call s:Fail('wire publishDiagnostics never arrived within budget')
else
  call s:Emit('diagnostics_observed', {'mode': 'push', 'evidence': 'client_log'})
endif

" ---------------------------------------------------------------- the
" full lifecycle session (host 1 of the canonical journey).

if empty(s:failures) && s:role ==# 'full_lifecycle_session'
  " Establish the exact current state: the shipped defect generation is
  " present through the client's own state and wire.
  if s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] >= 1", s:budget)
        \ && VimLspHostSettledStateBarrier(
        \   "VimLspHostBufferDiagnosticsCounts()['error'] >= 1", 1500, s:budget)
    call s:EmitGenerationCurrent('defect_present', 1, 0)
  else
    call s:Fail('defect_state_never_arrived')
  endif
endif

if empty(s:failures) && s:role ==# 'full_lifecycle_session'
  " Pending action #1: start through the client's public request path and
  " bind its wire identity.
  let s:old_bufnr = bufnr('%')
  call VimLspHostStartPendingDocumentSymbol()
  if !s:WaitFor('VimLspHostPendingRequestId() > 0', s:budget)
    call s:Fail('pending_request_never_sent')
  elseif !VimLspHostWaitForWireMarker('textDocument/documentSymbol', s:budget)
    call s:Fail('pending_request_never_on_wire')
  else
    call s:Emit('pending_action_started', {
          \ 'pending_index': '1',
          \ 'method': 'textDocument/documentSymbol',
          \ 'request_id': string(VimLspHostPendingRequestId()),
          \ 'target_bufnr': string(s:old_bufnr),
          \ })
  endif
endif

if empty(s:failures) && s:role ==# 'full_lifecycle_session'
  " Cancel by identity through the client's own public cancellation path,
  " then prove zero admissions across a bounded stable window (a cancelled
  " result the client delivered is a typed contract violation).
  call VimLspHostCancelPendingDocumentSymbol()
  if !VimLspHostWaitForWireMarker('$/cancelRequest', s:budget)
    call s:Fail('cancel_notification_never_on_wire')
  elseif !VimLspHostStableStateWindow(
        \   'VimLspHostPendingNotificationCount() == 0 && VimLspHostPendingDone() == 0',
        \   s:late_window)
    call s:Fail('cancelled_result_admitted')
  else
    call s:Emit('pending_action_cancelled', {
          \ 'cancel_index': '1',
          \ 'pending_index': '1',
          \ 'request_id': string(VimLspHostPendingRequestId()),
          \ 'cancel_sent': '1',
          \ 'notification_count': '0',
          \ })
  endif
endif

if empty(s:failures) && s:role ==# 'full_lifecycle_session'
  " Pending action #2 (the late-result document route): start, invalidate the
  " document instance through the real wipe path, let the old request
  " complete, then prove the replacement instance never moved.
  call VimLspHostStartPendingDocumentSymbol()
  if !s:WaitFor('VimLspHostPendingRequestId() > 0', s:budget)
    call s:Fail('late_request_never_sent')
  else
    let s:late_request_id = VimLspHostPendingRequestId()
    call s:Emit('pending_action_started', {
          \ 'pending_index': '2',
          \ 'method': 'textDocument/documentSymbol',
          \ 'request_id': string(s:late_request_id),
          \ 'target_bufnr': string(s:old_bufnr),
          \ })
    let s:didclose_before = VimLspHostWireMarkerCount('textDocument/didClose')
    silent bwipeout!
    if !VimLspHostWaitForWireMarkerCount(
          \ 'textDocument/didClose', s:didclose_before + 1, s:budget)
      call s:Fail('didclose_never_on_wire')
    else
      call s:Emit('buffer_wiped', {
            \ 'wipe_index': '1',
            \ 'bufnr': string(s:old_bufnr),
            \ 'didclose_sent': '1',
            \ })
    endif
  endif
endif

if empty(s:failures) && s:role ==# 'full_lifecycle_session'
  " Release/complete the old operation: the response must arrive (delivered
  " to the subscription) while no governed document exists.
  if !s:WaitFor('VimLspHostPendingNotificationCount() >= 1', s:budget)
    call s:Fail('late_response_never_delivered')
  endif
endif

if empty(s:failures) && s:role ==# 'full_lifecycle_session'
  " External atomic replacement writes the clean generation to disk: the
  " reopened instance must reflect disk truth, never the wiped instance's
  " state.
  call s:ExternalReplaceFile(s:fixture_root . '/' . s:opened_file_rel, s:clean_text)
  call s:Emit('external_mutation_applied', {
        \ 'mutation_index': '1',
        \ 'mutation': 'atomic_replace',
        \ 'target': 'governed',
        \ 'disk_generation': 'clean_restored',
        \ })
endif

if empty(s:failures) && s:role ==# 'full_lifecycle_session'
  " Reopen the same path: a new document instance on the unchanged server
  " generation, settling clean through its own push.
  let s:update_before = VimLspHostDiagnosticsUpdatedCount()
  let s:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
  call VimLspHostOpenFixture(s:fixture_root . '/' . s:opened_file_rel)
  let s:new_bufnr = bufnr('%')
  if s:new_bufnr == s:old_bufnr
    call s:Fail('reopen_produced_same_buffer_instance')
  elseif s:SettleBarrier(s:update_before, s:wire_before,
        \   "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
        \   . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0",
        \   'instance2_state_never_arrived')
    call s:Emit('buffer_reopened', {
          \ 'reopen_index': '1',
          \ 'old_bufnr': string(s:old_bufnr),
          \ 'new_bufnr': string(s:new_bufnr),
          \ 'same_path': '1',
          \ 'server_init_count': string(VimLspHostServerInitCount()),
          \ 'document_generation': 'instance2_clean',
          \ })
    call s:EmitGenerationCurrent('instance2_clean', 0, 0)
  endif
endif

if empty(s:failures) && s:role ==# 'full_lifecycle_session'
  " Late-result rejection: across the bounded window the replacement
  " instance's state stays exactly its own settled generation and no further
  " delivery occurs (the old request already completed; nothing new may
  " arrive for it).
  if !VimLspHostStableStateWindow(
        \   "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
        \   . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0",
        \   s:late_window)
    call s:Fail('replacement_instance_changed')
  else
    call s:Emit('late_result_rejected', {
          \ 'late_index': '1',
          \ 'pending_index': '2',
          \ 'request_id': string(s:late_request_id),
          \ 'response_delivered': '1',
          \ 'replacement_state_unchanged': '1',
          \ 'window_ms': string(s:late_window),
          \ })
  endif
endif

if empty(s:failures) && s:role ==# 'full_lifecycle_session'
  " Pending action #3 (the host route): started and left in flight — the
  " session exits with the old request unresolved, and the replacement host
  " must prove its results come only from its own fresh channel.
  let s:inflight_bufnr = bufnr('%')
  call VimLspHostStartPendingDocumentSymbol()
  if !s:WaitFor('VimLspHostPendingRequestId() > 0', s:budget)
    call s:Fail('inflight_request_never_sent')
  elseif !VimLspHostWaitForWireMarker('textDocument/documentSymbol', s:budget)
    call s:Fail('inflight_request_never_on_wire')
  else
    call s:Emit('pending_action_started', {
          \ 'pending_index': '3',
          \ 'method': 'textDocument/documentSymbol',
          \ 'request_id': string(VimLspHostPendingRequestId()),
          \ 'target_bufnr': string(s:inflight_bufnr),
          \ })
  endif
endif

" ---------------------------------------------------------------- the
" replacement host session (host 2): disk-current opening state and its own
" edit-cycle product result.

if empty(s:failures) && s:role ==# 'replacement_host_session'
  " The disk generation the supervisor wrote (clean) settles through this
  " host's own chain: a prior session's in-memory state must never appear.
  if s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] == 0", s:budget)
        \ && VimLspHostSettledStateBarrier(
        \   "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
        \   . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0", 1500, s:budget)
    call s:EmitGenerationCurrent('replacement_open_clean', 0, 0)
  else
    call s:Fail('replacement_open_state_never_arrived')
  endif
endif

if empty(s:failures) && s:role ==# 'replacement_host_session'
  " Its own defect edit through the real buffer didChange path.
  let s:update_before = VimLspHostDiagnosticsUpdatedCount()
  let s:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
  call VimLspHostSetLineAndFlush(s:mutation_line, s:defect_line_text)
  if s:SettleBarrier(s:update_before, s:wire_before,
        \   "VimLspHostBufferDiagnosticsCounts()['error'] >= 1",
        \   'replacement_own_defect_never_arrived')
    call s:EmitGenerationCurrent('replacement_own_defect', 1, 0)
  endif
endif

if empty(s:failures) && s:role ==# 'replacement_host_session'
  " Its own fix edit back to the clean generation.
  let s:update_before = VimLspHostDiagnosticsUpdatedCount()
  let s:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
  call VimLspHostSetLineAndFlush(s:mutation_line, s:clean_line_text)
  if s:SettleBarrier(s:update_before, s:wire_before,
        \   "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
        \   . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0",
        \   'replacement_own_current_never_arrived')
    call s:EmitGenerationCurrent('replacement_own_current', 0, 0)
  endif
endif

" ---------------------------------------------------------------- the
" assertion-failure session (host 3): a typed forced assertion failure with
" evidence preserved before the nonzero exit.

if empty(s:failures) && s:role ==# 'assertion_failure_session'
  if s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] == 0", s:budget)
        \ && VimLspHostSettledStateBarrier(
        \   "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
        \   . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0", 1500, s:budget)
    call s:EmitGenerationCurrent('failure_session_open', 0, 0)
  else
    call s:Fail('failure_session_open_state_never_arrived')
  endif
endif

" ---------------------------------------------------------------- the
" timeout/interruption session (host 4): settle, then a deliberate indefinite
" barrier — the Rust supervisor's hard deadline owns the bound and the kill.

if empty(s:failures) && s:role ==# 'timeout_interruption_session'
  if s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] == 0", s:budget)
        \ && VimLspHostSettledStateBarrier(
        \   "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
        \   . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0", 1500, s:budget)
    call s:EmitGenerationCurrent('timeout_session_open', 0, 0)
  else
    call s:Fail('timeout_session_open_state_never_arrived')
  endif
endif

" ---------------------------------------------------------------- the
" server-restart relabel control: the client's own server restart where the
" host-reopen cell requires a changed host instance.

if empty(s:failures) && s:role ==# 'server_restart_relabel_session'
  if s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] >= 1", s:budget)
        \ && VimLspHostSettledStateBarrier(
        \   "VimLspHostBufferDiagnosticsCounts()['error'] >= 1", 1500, s:budget)
    call s:EmitGenerationCurrent('defect_present', 1, 0)
  else
    call s:Fail('defect_state_never_arrived')
  endif
endif

if empty(s:failures) && s:role ==# 'server_restart_relabel_session'
  " Stop the server through the client's own path, then reopen the governed
  " file: the fresh FileType fires the client's lazy start and a second
  " initialize lands on the wire (a server restart — the required false
  " subject for full host reopen).
  call VimLspHostCloseBuffer()
  if !VimLspHostStopServerAndWait()
    call s:Fail('relabel_stop_failed')
  else
    let s:update_before = VimLspHostDiagnosticsUpdatedCount()
    let s:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
    call VimLspHostOpenFixture(s:fixture_root . '/' . s:opened_file_rel)
    if !s:WaitFor('VimLspHostServerInitCount() >= 2', s:budget)
      call s:Fail('relabel_restart_never_arrived')
    elseif s:SettleBarrier(s:update_before, s:wire_before,
          \   "VimLspHostBufferDiagnosticsCounts()['error'] >= 1",
          \   'relabel_state_never_arrived')
      call s:EmitGenerationCurrent('post_restart_defect', 1, 0)
    endif
  endif
endif

" ---------------------------------------------------------------- shared
" terminal paths.

if s:role ==# 'timeout_interruption_session' && empty(s:failures)
  " The deliberate indefinite barrier: the expression is never true and the
  " wait is bounded only by the Rust supervisor's hard deadline (the typed
  " timeout/interruption shape; events and logs written so far are preserved
  " evidence).
  call s:Emit('session_iteration_settled', {
        \ 'iteration_index': '4',
        \ 'session_role': s:role,
        \ 'product_result': 'typed_timeout_pending',
        \ })
  call VimLspHostWaitFor('0', 600000)
  " Unreachable within the supervisor's deadline; failing closed if it ever
  " returns.
  call s:Fail('timeout_barrier_unexpectedly_returned')
elseif empty(s:failures)
  " Orderly sessions: settle the iteration first (its product result is the
  " denominator's per-iteration observation), then stop through the public
  " path (the #10944 teardown law) and exit through the user-equivalent path.
  let s:iteration = s:role ==# 'full_lifecycle_session' ? '1'
        \ : s:role ==# 'replacement_host_session' ? '2'
        \ : s:role ==# 'assertion_failure_session' ? '3'
        \ : s:role ==# 'timeout_interruption_session' ? '4' : '1'
  let s:product = s:role ==# 'full_lifecycle_session' ? 'defect_to_current'
        \ : s:role ==# 'replacement_host_session' ? 'own_edit_cycle'
        \ : s:role ==# 'assertion_failure_session' ? 'typed_assertion_failure'
        \ : s:role ==# 'server_restart_relabel_session' ? 'server_restart_relabel'
        \ : 'typed_timeout'
  call s:Emit('session_iteration_settled', {
        \ 'iteration_index': s:iteration,
        \ 'session_role': s:role,
        \ 'product_result': s:product,
        \ })
  call s:Emit('shutdown_started', {'server_stopping': '1'})
  let s:server_exited = VimLspHostStopServerAndWait()
  if s:server_exited
    call s:Emit('shutdown_completed', {'server_exited': '1'})
  else
    call s:Emit('shutdown_completed', {
          \ 'server_exited': '0',
          \ 'exit_evidence': 'deferred_to_editor_teardown',
          \ })
  endif
  if s:role ==# 'assertion_failure_session'
    " The forced assertion failure: evidence is preserved above (events,
    " client log, snapshots); the nonzero exit is the designed terminal path.
    call s:Fail('forced_assertion_failure')
  else
    call s:Emit('host_exit_initiated', {'exit_path': 'user_qa'})
  endif
endif

if !empty(s:failures)
  " The receipt is Rust-owned: this driver only reports its typed failure and
  " exits nonzero, so an instrument failure can never masquerade as a pass.
  call s:Emit('driver_failed', {'reason': s:failures[0]})
  cquit 2
endif
qa!
