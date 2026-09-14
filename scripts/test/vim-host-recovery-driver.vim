" Bounded driver for the #11398 server-generation recovery hermetic journey.
"
" Same ownership laws as scripts/test/vim-host-driver.vim (#10944),
" scripts/test/vim-host-diagnostics-driver.vim (#10946), and
" scripts/test/vim-host-freshness-driver.vim (#11390): this driver stays
" thin — it sequences the journey through the thin adapter, emits one typed
" JSON event per barrier for the Rust supervisor, and exits nonzero on any
" failure. It owns no process supervision, no deadline policy, no receipt
" writing, and no semantic expectations: the governed source generations,
" the expected/decoy root identities, and the observation windows arrive
" from the Rust scenario through the environment and are only applied and
" observed here.
"
" The crash stimulus is Rust-owned: this driver writes one typed marker file
" into the stimulus channel (a journey action, exactly like #11390's
" external file mutations) and then observes only the client's own exit
" evidence (the lsp_server_exit event or the client's own running state).
" It never spawns, kills, signals, or addresses any process, and it never
" manufactures a private restart path: every replacement generation goes
" through the pinned client's own lifecycle (the public lsp#stop_server
" stop plus the next document open, whose FileType fires the client's lazy
" start).
"
" Vimscript boundary laws (#12589, binding this driver too):
"   - barrier expressions are eval'd in the adapter's scope: generation
"     numbers are baked into the expression string, never referenced through
"     this script's s: variables;
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
"   PERLLSP_VIM_HOST_RECOVERY_VARIANT    canonical|wrong_root_decoy|
"                                        auto_recovery_claimed|
"                                        replay_skipped_claimed
"   PERLLSP_VIM_HOST_OPENED_FILE_REL     governed source file, fixture-relative
"   PERLLSP_VIM_HOST_EXPECTED_ROOT_REL   governed root, fixture-relative
"   PERLLSP_VIM_HOST_DECOY_ROOT_REL      decoy root, fixture-relative
"   PERLLSP_VIM_HOST_DECOY_FILE_REL      same-named decoy file, fixture-relative
"   PERLLSP_VIM_HOST_DEFECT_SOURCE_TEXT  the old (defective) generation bytes
"   PERLLSP_VIM_HOST_CLEAN_SOURCE_TEXT   the replacement (clean) generation bytes
"   PERLLSP_VIM_HOST_STALE_WINDOW_MS     bounded absence-observation window
"   PERLLSP_VIM_HOST_STIMULUS_DIR        Rust-owned crash-stimulus marker channel
"
" The journey (the whole proof this driver claims, judged by Rust):
"   start Vim -> load pinned vim-lsp -> register canonical server -> open the
"   governed defective source (native detection) -> lsp_server_init ->
"   lsp_buffer_enabled -> capture initialize capabilities -> #7762 root
"   observation with decoy discrimination -> old-generation defect current
"   -> EXPLICIT RESTART (public stop + reopen): old process terminates, new
"   generation initializes, readiness, didOpen replay, recomputed defect
"   -> UNEXPECTED EXIT 1 (external stimulus): client exit evidence, bounded
"   no-retry window, manual_restart_required -> MANUAL RECOVERY (reopen):
"   new generation, readiness, replay, recomputed defect
"   -> external clean-generation replacement on disk
"   -> UNEXPECTED EXIT 2 (external stimulus) with the old result held ->
"   bounded window -> MANUAL RECOVERY: new generation, replay picks the
"   clean bytes, clean current, old defect signature never settles
"   -> UNEXPECTED EXIT 3 -> recovery pending -> HOST SHUTDOWN during the
"   pending recovery -> exit.

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
let s:variant = s:Env('PERLLSP_VIM_HOST_RECOVERY_VARIANT')
let s:opened_file_rel = s:Env('PERLLSP_VIM_HOST_OPENED_FILE_REL')
let s:expected_root_rel = s:Env('PERLLSP_VIM_HOST_EXPECTED_ROOT_REL')
let s:decoy_root_rel = s:Env('PERLLSP_VIM_HOST_DECOY_ROOT_REL')
let s:decoy_file_rel = s:Env('PERLLSP_VIM_HOST_DECOY_FILE_REL')
let s:defect_text = s:Env('PERLLSP_VIM_HOST_DEFECT_SOURCE_TEXT')
let s:clean_text = s:Env('PERLLSP_VIM_HOST_CLEAN_SOURCE_TEXT')
let s:stale_window = str2nr(s:Env('PERLLSP_VIM_HOST_STALE_WINDOW_MS'))
let s:stimulus_dir = substitute(s:Env('PERLLSP_VIM_HOST_STIMULUS_DIR'), '\', '/', 'g')
if s:budget <= 0
  let s:budget = 20000
endif

if empty(s:adapter) || empty(s:event_file) || empty(s:capability_path)
      \ || empty(s:fixture_root) || empty(s:candidate_sha) || empty(s:server_name)
      \ || empty(s:variant) || empty(s:opened_file_rel) || empty(s:expected_root_rel)
      \ || empty(s:decoy_root_rel) || empty(s:decoy_file_rel)
      \ || empty(s:defect_text) || empty(s:clean_text)
      \ || s:stale_window <= 0 || empty(s:stimulus_dir)
  echoerr 'vim recovery driver: required environment missing, failing closed'
  cquit 3
endif

let s:sequence = 0
let s:failures = []

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

" Exact-bytes line list for writefile binary mode (the #11390 law).
function! s:ExternalFileLines(text) abort
  return split(a:text . "\n", "\n", v:true)
endfunction

" Atomic external replacement of the governed source: the old-generation
" result stays held in the client state while the disk moves to the clean
" generation (a journey action, Rust-authored bytes; #11390 pattern).
function! s:ExternalReplaceFile(path, text) abort
  let l:temp = a:path . '.recovery-replace'
  call writefile(s:ExternalFileLines(a:text), l:temp, 'b')
  if rename(l:temp, a:path) != 0
    echoerr 'vim recovery driver: atomic replacement failed for ' . a:path
    cquit 3
  endif
  return v:true
endfunction

" Fixture-relative identity of an absolute path (#11390 boundary law).
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

" The client's own exit evidence for one generation end: either the public
" lsp_server_exit event fires (its per-stop generation is baked by the
" adapter) or the client's own running state leaves running.
function! s:WaitForExitEvidence(exit_before) abort
  let l:expr = 'g:perllsp_vim_host_server_exit > ' . a:exit_before
        \ . ' || VimLspHostServerStatus() ==# ''exited'''
        \ . ' || !VimLspHostServerRunning()'
  return s:WaitFor(l:expr, s:budget)
endfunction

" One crash stimulus: emit the typed action, write the Rust-owned marker,
" and observe only the client's own exit evidence. This driver never
" addresses a process.
function! s:ApplyStimulus(index, serving_generation) abort
  let l:marker = 'kill-' . a:index . '.req'
  call s:Emit('recovery_stimulus_applied', {
        \ 'stimulus_index': string(a:index),
        \ 'stimulus': 'terminate_server_process',
        \ 'marker': l:marker,
        \ 'serving_generation': string(a:serving_generation),
        \ })
  call writefile(['stimulus'], s:stimulus_dir . '/' . l:marker, 'b')
  let l:exit_before = VimLspHostServerExitCount()
  if !s:WaitForExitEvidence(l:exit_before)
    call s:Fail('exit_stimulus_never_landed')
    return 0
  endif
  return 1
endfunction

" One bounded no-automatic-retry observation window: no new client init
" event, no new initialize on the wire, and the server still not running,
" for the whole window.
function! s:ObserveNoRetryWindow(index, init_events_before, init_wire_before) abort
  let l:expr = 'VimLspHostServerInitCount() == ' . a:init_events_before
        \ . ' && VimLspHostWireMarkerCount(''initialize'') == ' . a:init_wire_before
        \ . ' && !VimLspHostServerRunning()'
  let l:stable = VimLspHostStableStateWindow(l:expr, s:stale_window)
  if l:stable < 0
    call s:Fail('spontaneous_recovery_observed')
    return 0
  endif
  if l:stable == 0
    call s:Fail('exit_evidence_unstable_at_window_start')
    return 0
  endif
  call s:Emit('recovery_disposition_observed', {
        \ 'disposition_index': string(a:index),
        \ 'stimulus': 'unexpected_exit',
        \ 'disposition': 'manual_restart_required',
        \ 'retry_count': '0',
        \ 'window_ms': string(s:stale_window),
        \ 'exit_observed': '1',
        \ })
  return 1
endfunction

" The readiness barrier of one replacement generation: the client's own
" init event count and buffer-enabled count must have advanced to the new
" generation, and the governed document's didOpen must be back on the wire.
function! s:WaitForGenerationReadiness(generation, opens_before) abort
  let l:init_expr = 'VimLspHostServerInitCount() >= ' . a:generation
  let l:enabled_expr = 'VimLspHostBufferEnabledCount() >= ' . a:generation
  let l:opens_expr = 'VimLspHostWireMarkerCount(''textDocument/didOpen'') >= '
        \ . (a:opens_before + 1)
  if !s:WaitFor(l:init_expr, s:budget)
    call s:Fail('new_generation_init_never_arrived')
    return 0
  endif
  if !s:WaitFor(l:enabled_expr, s:budget)
    call s:Fail('new_generation_readiness_never_arrived')
    return 0
  endif
  if !s:WaitFor(l:opens_expr, s:budget)
    call s:Fail('document_replay_absent')
    return 0
  endif
  return 1
endfunction

" One complete replacement observation: readiness (already proven by the
" caller), the governed root re-selected, one didOpen replay, and the
" settled current state. Returns 1 when the replacement observation is
" complete, 0 when it is not.
function! s:EmitReplay(replay_index, generation) abort
  let l:root_uri = VimLspHostRootUri()
  let l:root_rel = s:FixtureRel(substitute(lsp#utils#uri_to_path(l:root_uri), '\', '/', 'g'))
  if l:root_rel !=# s:expected_root_rel
    call s:Fail('replay_root_mismatch')
    return 0
  endif
  call s:Emit('generation_replay_observed', {
        \ 'replay_index': string(a:replay_index),
        \ 'initialize_generation': string(a:generation),
        \ 'document': s:opened_file_rel,
        \ 'root': l:root_rel,
        \ 'did_open_replayed': '1',
        \ 'client_init_events': string(VimLspHostServerInitCount()),
        \ 'buffer_enabled_events': string(VimLspHostBufferEnabledCount()),
        \ })
  return 1
endfunction

" The released old result must never resettle: a bounded quiet window where
" the wire push count is stable and the clean state holds. Function-scoped
" (the #12589 boundary law: no `let l:` at script level).
function! s:ObserveOldRejection() abort
  let l:wire_before_reject = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
  let l:reject_expr = "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
        \ . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0"
  let l:held = VimLspHostStableStateWindow(l:reject_expr, s:stale_window)
  if l:held < 0
    call s:Fail('old_generation_result_resettled')
    return 0
  endif
  if l:held == 0
    call s:Fail('clean_generation_state_unstable')
    return 0
  endif
  if VimLspHostWireMarkerCount('textDocument/publishDiagnostics') != l:wire_before_reject
    call s:Fail('old_generation_result_republished')
    return 0
  endif
  call s:Emit('old_generation_rejected', {
        \ 'rejection_index': '1',
        \ 'held_generation': 'g3_manual_recovery_defect',
        \ 'released_after_generation': 'g4_clean_current',
        \ 'held_result': 'defect_error_signature',
        \ 'old_signature_settled': '0',
        \ })
  return 1
endfunction

" The settled current-state barrier (client state plus a bounded quiet wire
" window; #11390 settled law), then the typed current observation.
function! s:EmitCurrent(generation_index, generation, state_expr, fail_reason) abort
  if !VimLspHostSettledStateBarrier(a:state_expr, 1500, s:budget)
    call s:Fail(a:fail_reason)
    return 0
  endif
  let l:counts = VimLspHostBufferDiagnosticsCounts()
  call s:Emit('generation_current_observed', {
        \ 'generation_index': string(a:generation_index),
        \ 'generation': a:generation,
        \ 'state_source': 'client_state',
        \ 'barrier': 'diagnostics_event_and_wire',
        \ 'errors': string(get(l:counts, 'error', 0)),
        \ 'warnings': string(get(l:counts, 'warning', 0)),
        \ })
  return 1
endfunction

" Native-detection listener (#7762 law).
let s:native_filetype_observed = 0
let s:native_filetype_value = ''
augroup perllsp_vim_host_native_recovery
  autocmd!
  autocmd FileType perl let s:native_filetype_observed = 1
        \ | let s:native_filetype_value = expand('<amatch>')
augroup END

" ---------------------------------------------------------------- journey

let s:vim_version = split(execute('version'), "\n")[0]
call s:Emit('host_started', {'vim_version': s:vim_version})

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

" #7762 root observation with decoy discrimination (#10946 pattern).
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
        \ 'decoy_root': s:decoy_root_rel,
        \ })
  if s:observed_root_rel !=# s:expected_root_rel
    call s:Fail('root_mismatch')
  endif
endif

" Attach-completion wire barrier (#10946 pattern).
if !VimLspHostWaitForWireMarkerCount('textDocument/publishDiagnostics', 1, s:budget)
  call s:Fail('wire publishDiagnostics never arrived within budget')
else
  call s:Emit('diagnostics_observed', {'mode': 'push', 'evidence': 'client_log'})
endif

" ------------------------------------------------- old generation current

if empty(s:failures)
  call s:EmitCurrent(1, 'g1_defect_current',
        \ "VimLspHostBufferDiagnosticsCounts()['error'] >= 1"
        \ . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0",
        \ 'old_generation_defect_state_never_arrived')
endif

" ---------------------------------------------------------------- phase B
" explicit restart through the pinned client's public route

if empty(s:failures)
  let s:opens_before = VimLspHostWireMarkerCount('textDocument/didOpen')
  call VimLspHostCloseBuffer()
  if !VimLspHostStopServerAndWait()
    call s:Fail('explicit_restart_stop_failed')
  else
    call s:Emit('server_restart_applied', {
          \ 'restart_index': '1',
          \ 'route': 'public_stop_reopen',
          \ 'old_init_generation': '1',
          \ 'new_init_generation': '2',
          \ })
    if s:variant ==# 'replay_skipped_claimed'
      " Negative control: claim the replacement without replaying the
      " governed document. No buffer is opened, so the didOpen replay never
      " arrives and the journey fails with the typed reason.
      if s:WaitFor('VimLspHostWireMarkerCount(''textDocument/didOpen'') >= '
            \ . (s:opens_before + 1), s:budget)
        call s:Fail('document_replay_unexpectedly_observed')
      else
        call s:Fail('document_replay_absent')
      endif
    else
      " The reopen fires the client's lazy start: a new process generation
      " initializes and replays the governed document.
      call VimLspHostOpenFixture(s:fixture_root . '/' . s:opened_file_rel)
      if s:WaitForGenerationReadiness(2, s:opens_before)
            \ && s:EmitReplay(1, 2)
        call s:EmitCurrent(2, 'g2_recomputed_defect',
              \ "VimLspHostBufferDiagnosticsCounts()['error'] >= 1"
              \ . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0",
              \ 'recomputed_defect_state_never_arrived')
      endif
    endif
  endif
endif

" ---------------------------------------------------------------- phase C
" unexpected exit 1: external crash stimulus, honest disposition, manual
" recovery through the client's own next-open route

if empty(s:failures)
  if s:ApplyStimulus(1, 2)
    let s:init_events_before = VimLspHostServerInitCount()
    let s:init_wire_before = VimLspHostWireMarkerCount('initialize')
    if s:variant ==# 'auto_recovery_claimed'
      " Negative control: claim automatic bounded recovery. The pinned client
      " has no automatic recovery, so a new generation without any user
      " action never arrives and the journey fails with the typed reason.
      if s:WaitFor('VimLspHostServerInitCount() >= 3', s:budget)
        call s:Fail('automatic_recovery_unexpectedly_observed')
      else
        call s:Fail('automatic_recovery_absent')
      endif
    elseif s:ObserveNoRetryWindow(1, s:init_events_before, s:init_wire_before)
      " The manual route: the user reopens the governed document, the
      " client's own lazy start fires the new generation, and the complete
      " replacement chain is proven — a new PID alone is never enough.
      let s:opens_before = VimLspHostWireMarkerCount('textDocument/didOpen')
      call VimLspHostCloseReopen()
      call s:Emit('server_restart_applied', {
            \ 'restart_index': '2',
            \ 'route': 'manual_reopen_after_exit',
            \ 'old_init_generation': '2',
            \ 'new_init_generation': '3',
            \ })
      if s:WaitForGenerationReadiness(3, s:opens_before)
            \ && s:EmitReplay(2, 3)
        call s:EmitCurrent(3, 'g3_manual_recovery_defect',
              \ "VimLspHostBufferDiagnosticsCounts()['error'] >= 1"
              \ . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0",
              \ 'manual_recovery_defect_state_never_arrived')
      endif
    endif
  endif
endif

" ---------------------------------------------------------------- phase D
" old-generation rejection: the defect result stays held across an external
" clean-generation replacement and a second crash; the replacement
" generation replays the clean bytes and the old signature never settles

if empty(s:failures)
  " The old-generation result is held: the buffer keeps the defective
  " generation, and the disk moves to the authored clean generation.
  call s:ExternalReplaceFile(s:fixture_root . '/' . s:opened_file_rel, s:clean_text)
  if s:ApplyStimulus(2, 3)
    let s:init_events_before = VimLspHostServerInitCount()
    let s:init_wire_before = VimLspHostWireMarkerCount('initialize')
    if s:ObserveNoRetryWindow(2, s:init_events_before, s:init_wire_before)
      let s:opens_before = VimLspHostWireMarkerCount('textDocument/didOpen')
      call VimLspHostCloseReopen()
      call s:Emit('server_restart_applied', {
            \ 'restart_index': '3',
            \ 'route': 'manual_reopen_after_exit',
            \ 'old_init_generation': '3',
            \ 'new_init_generation': '4',
            \ })
      if s:WaitForGenerationReadiness(4, s:opens_before)
            \ && s:EmitReplay(3, 4)
            \ && s:EmitCurrent(4, 'g4_clean_current',
            \   "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
            \   . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0",
            \   'clean_generation_state_never_arrived')
        call s:ObserveOldRejection()
      endif
    endif
  endif
endif

" ---------------------------------------------------------------- phase F
" shutdown during a pending recovery: a third crash leaves the replacement
" pending, and the host exits inside the pending state

if empty(s:failures)
  " The pending state is "the serving generation died and nothing replaced
  " it", so bind it to the counters observed immediately before the final
  " stimulus rather than to a literal generation number. A hardcoded count
  " silently stops meaning "no replacement started" the moment the journey
  " gains or loses a generation.
  let s:pending_init_before = VimLspHostServerInitCount()
  let s:pending_enabled_before = VimLspHostBufferEnabledCount()
  if s:ApplyStimulus(3, 4)
    if VimLspHostServerInitCount() == s:pending_init_before
          \ && VimLspHostBufferEnabledCount() == s:pending_enabled_before
      call s:Emit('shutdown_during_pending_observed', {
            \ 'old_generation_dead': '1',
            \ 'new_generation_started': '0',
            \ 'observed_init_events': string(s:pending_init_before),
            \ 'recovery_route': 'pending_manual_reopen',
            \ })
    else
      call s:Fail('shutdown_pending_state_not_observed')
    endif
  endif
endif

" Shutdown mirrors the #10944 driver: stop through the public path, bind the
" client's own exit evidence, and defer to the editor teardown when the
" pinned client loses the job-exit callback.
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

if !empty(s:failures)
  " The receipt is Rust-owned: this driver only reports its typed failure and
  " exits nonzero, so an instrument failure can never masquerade as a pass.
  call s:Emit('driver_failed', {'reason': s:failures[0]})
  cquit 2
endif
qa!
