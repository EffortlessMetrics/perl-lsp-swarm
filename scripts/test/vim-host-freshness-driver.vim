" Bounded driver for the #11390 freshness-generations hermetic journey.
"
" Same ownership laws as scripts/test/vim-host-driver.vim (#10944) and
" scripts/test/vim-host-diagnostics-driver.vim (#10946): this driver stays
" thin — it sequences the journey through the thin adapter, emits one typed
" JSON event per barrier for the Rust supervisor, and exits nonzero on any
" failure. It owns no process supervision, no deadline policy, no receipt
" writing, and no semantic expectations: the source generations, the settings
" channel, the TOML variants, and the expected/decoy root identities arrive
" from the Rust scenario through the environment and are only applied and
" observed here.
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
"   PERLLSP_VIM_HOST_FRESHNESS_VARIANT   canonical|wrong_root_decoy|
"                                        live_reload_claimed|ambient_path_only
"   PERLLSP_VIM_HOST_OPENED_FILE_REL     governed source file, fixture-relative
"   PERLLSP_VIM_HOST_EXPECTED_ROOT_REL   governed root, fixture-relative
"   PERLLSP_VIM_HOST_DECOY_ROOT_REL      decoy root, fixture-relative
"   PERLLSP_VIM_HOST_DECOY_FILE_REL      same-named decoy file, fixture-relative
"   PERLLSP_VIM_HOST_SETTINGS_FILE_REL   client-settings file, fixture-relative
"   PERLLSP_VIM_HOST_CONFIG_FILE_REL     project-config file, fixture-relative
"   PERLLSP_VIM_HOST_MUTATION_LINE       1-based governed mutation line
"   PERLLSP_VIM_HOST_CLEAN_SOURCE_TEXT   the clean generation bytes
"   PERLLSP_VIM_HOST_DEFECT_SOURCE_TEXT  the defective generation bytes
"   PERLLSP_VIM_HOST_DECOY_DEFECT_TEXT   the decoy mutation bytes
"   PERLLSP_VIM_HOST_STALE_WINDOW_MS     bounded absence-observation window
"   PERLLSP_VIM_HOST_SETTINGS_INCLUDE_PATHS  the settings channel content
"   PERLLSP_VIM_HOST_TOML_EXCLUDE_TEXT   the valid project-config generation
"   PERLLSP_VIM_HOST_TOML_MALFORMED_TEXT the malformed project-config generation
"
" The journey (the whole proof this driver claims, judged by Rust):
"   start Vim -> load pinned vim-lsp -> register canonical server -> open the
"   governed clean source (native detection) -> lsp_server_init ->
"   lsp_buffer_enabled -> capture initialize capabilities -> #7762 root
"   observation with decoy discrimination -> clean generation current ->
"   external in-place mutation (defect) -> bounded stale hold (no watcher
"   route: nothing moves) -> explicit client reload picks the defect up ->
"   external atomic replacement (old clean bytes) -> stale hold (old
"   generation never repopulates) -> explicit reload restores -> decoy
"   mutation -> governed reload stays clean (wrong root never supplies) ->
"   settings file: PL701 present -> identical reopen keeps it (control) ->
"   update_workspace_config push -> reopen clears it -> config file: critic
"   warning present -> TOML exclude created -> no live effect -> server
"   restart clears it -> malformed TOML -> no live effect -> restart returns
"   it (honest rejection) -> repair -> restart clears it -> stop -> exit.

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
let s:variant = s:Env('PERLLSP_VIM_HOST_FRESHNESS_VARIANT')
let s:opened_file_rel = s:Env('PERLLSP_VIM_HOST_OPENED_FILE_REL')
let s:expected_root_rel = s:Env('PERLLSP_VIM_HOST_EXPECTED_ROOT_REL')
let s:decoy_root_rel = s:Env('PERLLSP_VIM_HOST_DECOY_ROOT_REL')
let s:decoy_file_rel = s:Env('PERLLSP_VIM_HOST_DECOY_FILE_REL')
let s:settings_file_rel = s:Env('PERLLSP_VIM_HOST_SETTINGS_FILE_REL')
let s:config_file_rel = s:Env('PERLLSP_VIM_HOST_CONFIG_FILE_REL')
let s:clean_text = s:Env('PERLLSP_VIM_HOST_CLEAN_SOURCE_TEXT')
let s:defect_text = s:Env('PERLLSP_VIM_HOST_DEFECT_SOURCE_TEXT')
let s:decoy_defect_text = s:Env('PERLLSP_VIM_HOST_DECOY_DEFECT_TEXT')
let s:stale_window = str2nr(s:Env('PERLLSP_VIM_HOST_STALE_WINDOW_MS'))
let s:settings_paths = s:Env('PERLLSP_VIM_HOST_SETTINGS_INCLUDE_PATHS')
let s:toml_exclude_text = s:Env('PERLLSP_VIM_HOST_TOML_EXCLUDE_TEXT')
let s:toml_malformed_text = s:Env('PERLLSP_VIM_HOST_TOML_MALFORMED_TEXT')
if s:budget <= 0
  let s:budget = 20000
endif

if empty(s:adapter) || empty(s:event_file) || empty(s:capability_path)
      \ || empty(s:fixture_root) || empty(s:candidate_sha) || empty(s:server_name)
      \ || empty(s:variant) || empty(s:opened_file_rel) || empty(s:expected_root_rel)
      \ || empty(s:decoy_root_rel) || empty(s:decoy_file_rel)
      \ || empty(s:settings_file_rel) || empty(s:config_file_rel)
      \ || empty(s:clean_text) || empty(s:defect_text) || empty(s:decoy_defect_text)
      \ || s:stale_window <= 0 || empty(s:settings_paths)
      \ || empty(s:toml_exclude_text) || empty(s:toml_malformed_text)
  echoerr 'vim freshness driver: required environment missing, failing closed'
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

" Exact-bytes line list for writefile binary mode: split keeping empties and
" append one trailing empty element so the final newline is preserved (binary
" mode omits the separator only after the last item).
function! s:ExternalFileLines(text) abort
  return split(a:text . "\n", "\n", v:true)
endfunction

" In-place external mutation of a fixture file: the editor buffer is never
" touched, exactly like an out-of-editor actor overwriting the bytes. The
" bytes are Rust-authored (delivered through the environment); this driver
" owns only the mechanism. File writes are journey actions here, not adapter
" surface: the adapter stays read-only and the driver already appends typed
" events (#10944 adapter law).
function! s:ExternalWriteFile(path, text) abort
  call writefile(s:ExternalFileLines(a:text), a:path, 'b')
  return v:true
endfunction

" Atomic external replacement: write a sibling temp file, then rename over
" the target (rename(2)/MoveFileEx-replace semantics on every supported
" host). Readers observe either the old or the new complete generation,
" never a partial write.
function! s:ExternalReplaceFile(path, text) abort
  let l:temp = a:path . '.freshness-replace'
  call writefile(s:ExternalFileLines(a:text), l:temp, 'b')
  if rename(l:temp, a:path) != 0
    echoerr 'vim freshness driver: atomic replacement failed for ' . a:path
    cquit 3
  endif
  return v:true
endfunction

" Fixture-relative identity of an absolute path, or '' when the path is
" outside the fixture. Report-only: the Rust scenario owns the expectation;
" the driver never derives it. Normalization is case-insensitive and maps the
" win32unix drive form (`/f/...`, what Git-vim's uri_to_path yields) back to
" `F:/...` so both sides compare (#12589 boundary law).
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

" Native-detection listener: the observed filetype is retained at the moment
" Vim's own detection fires, before any LSP-specific override could exist
" (#7762 law).
let s:native_filetype_observed = 0
let s:native_filetype_value = ''
augroup perllsp_vim_host_native_freshness
  autocmd!
  autocmd FileType perl let s:native_filetype_observed = 1
        \ | let s:native_filetype_value = expand('<amatch>')
augroup END

" Observe the current client state counts and emit one
" generation_current_observed event with the deterministic barrier
" (diagnostics update event + wire push + state) already satisfied by the
" caller.
function! s:EmitGenerationCurrent(index, generation) abort
  let l:counts = VimLspHostBufferDiagnosticsCounts()
  call s:Emit('generation_current_observed', {
        \ 'generation_index': string(a:index),
        \ 'generation': a:generation,
        \ 'state_source': 'client_state',
        \ 'barrier': 'diagnostics_event_and_wire',
        \ 'errors': string(get(l:counts, 'error', 0)),
        \ 'warnings': string(get(l:counts, 'warning', 0)),
        \ })
endfunction

" The deterministic materialization barrier after any client reopen or
" restart: the client's own diagnostics update event must fire again, its own
" wire log must carry one more publishDiagnostics push, and then the state
" condition must hold. Generation numbers are baked into the expressions
" (adapter scope; this script's s: variables do not exist there). The state
" failure reason is caller-owned so the negative variants fail with exactly
" their typed reason.
function! s:MaterializeBarrier(update_before, wire_before, state_expr, state_fail_reason) abort
  let l:update_expr = 'VimLspHostDiagnosticsUpdatedCount() > ' . a:update_before
  let l:wire_expr = 'VimLspHostWireMarkerCount(''textDocument/publishDiagnostics'') >= '
        \ . (a:wire_before + 1)
  if !s:WaitFor(l:update_expr, s:budget)
    call s:Fail('materialization_update_event_never_arrived')
    return 0
  endif
  if !s:WaitFor(l:wire_expr, s:budget)
    call s:Fail('materialization_wire_push_never_arrived')
    return 0
  endif
  " The state claim settles rather than first-passes: a document open begins
  " with a leading empty publishDiagnostics batch and the computed batch
  " follows, so a negated claim (== 0) observed on the transient would accept
  " a generation that is not current. The settled barrier requires the claim
  " to hold across a bounded quiet window of the wire push count.
  if !VimLspHostSettledStateBarrier(a:state_expr, 1500, s:budget)
    call s:Fail(a:state_fail_reason)
    return 0
  endif
  return 1
endfunction

" One bounded stale-generation hold: the client's own wire push count must
" not move for the whole window, and the state expression must hold at both
" ends. A spontaneous republish inside the window is a typed route violation.
function! s:ObserveStaleHold(index, held_generation, current_generation, state_expr) abort
  let l:stable = VimLspHostStableStateWindow(a:state_expr, s:stale_window)
  if l:stable < 0
    call s:Fail('spontaneous_state_change_observed')
    return 0
  endif
  if l:stable == 0
    call s:Fail('stale_state_claim_false')
    return 0
  endif
  call s:Emit('stale_generation_held', {
        \ 'hold_index': string(a:index),
        \ 'held_generation': a:held_generation,
        \ 'current_generation': a:current_generation,
        \ 'window_ms': string(s:stale_window),
        \ 'state_held': '1',
        \ })
  return 1
endfunction

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

" #7762 root observation with decoy discrimination (#10946 pattern, with the
" cpanfile governed marker).
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

" ---------------------------------------------------------------- source
" generations

if empty(s:failures) && s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] == 0", s:budget)
      \ && VimLspHostSettledStateBarrier(
      \   "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
      \   . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0", 1500, s:budget)
  call s:EmitGenerationCurrent(1, 'g1_clean')
elseif empty(s:failures)
  call s:Fail('g1_clean_state_never_arrived')
endif

if empty(s:failures)
  " Mutation 1: external in-place defect generation.
  call s:ExternalWriteFile(s:fixture_root . '/' . s:opened_file_rel, s:defect_text)
  call s:Emit('external_mutation_applied', {
        \ 'mutation_index': '1',
        \ 'mutation': 'in_place',
        \ 'target': 'governed',
        \ 'disk_generation': 'g2_defect',
        \ })
endif

if empty(s:failures) && s:variant ==# 'live_reload_claimed'
  " Negative control: claim the client refreshes open-document semantics
  " spontaneously. No watcher route exists for this subject, so the state
  " must never arrive and the journey fails with the typed reason.
  if s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] >= 1", s:budget)
    call s:Fail('live_freshness_unexpectedly_observed')
  else
    call s:Fail('live_freshness_absent')
  endif
endif

if empty(s:failures)
  " Hold 1: nothing moves without a client materialization.
  if s:ObserveStaleHold(1, 'g2_defect', 'g1_clean',
        \ "VimLspHostBufferDiagnosticsCounts()['error'] == 0")
    " Materialization 1: the explicit client reload route picks the defect
    " generation up (didClose + didOpen with the current disk bytes).
    let s:update_before = VimLspHostDiagnosticsUpdatedCount()
    let s:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
    call VimLspHostCloseReopen()
    call s:Emit('client_materialization_applied', {
          \ 'materialization_index': '1',
          \ 'materialization': 'client_close_reopen',
          \ 'picks_generation': 'g2_defect',
          \ })
    if s:MaterializeBarrier(s:update_before, s:wire_before,
          \ "VimLspHostBufferDiagnosticsCounts()['error'] >= 1",
          \ 'materialization_state_never_arrived')
      call s:EmitGenerationCurrent(2, 'g2_defect')
    endif
  endif
endif

if empty(s:failures)
  " Mutation 2: atomic replacement restores the old clean generation.
  call s:ExternalReplaceFile(s:fixture_root . '/' . s:opened_file_rel, s:clean_text)
  call s:Emit('external_mutation_applied', {
        \ 'mutation_index': '2',
        \ 'mutation': 'atomic_replace',
        \ 'target': 'governed',
        \ 'disk_generation': 'g3_old_clean',
        \ })
endif

if empty(s:failures)
  " Hold 2: the released old generation must not repopulate the defective
  " current state.
  if s:ObserveStaleHold(2, 'g3_old_clean', 'g2_defect',
        \ "VimLspHostBufferDiagnosticsCounts()['error'] >= 1")
    let s:update_before = VimLspHostDiagnosticsUpdatedCount()
    let s:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
    call VimLspHostCloseReopen()
    call s:Emit('client_materialization_applied', {
          \ 'materialization_index': '2',
          \ 'materialization': 'client_close_reopen',
          \ 'picks_generation': 'g3_old_clean',
          \ })
    if s:MaterializeBarrier(s:update_before, s:wire_before,
          \ "VimLspHostBufferDiagnosticsCounts()['error'] == 0",
          \ 'materialization_state_never_arrived')
      call s:EmitGenerationCurrent(3, 'g3_old_clean')
    endif
  endif
endif

if empty(s:failures)
  " Mutation 3: the same-named decoy at the wrong root receives the defective
  " generation. The governed buffer must never see it.
  call s:ExternalWriteFile(s:fixture_root . '/' . s:decoy_file_rel, s:decoy_defect_text)
  call s:Emit('external_mutation_applied', {
        \ 'mutation_index': '3',
        \ 'mutation': 'in_place',
        \ 'target': 'decoy',
        \ 'disk_generation': 'decoy_defect',
        \ })
  let s:update_before = VimLspHostDiagnosticsUpdatedCount()
  let s:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
  call VimLspHostCloseReopen()
  call s:Emit('client_materialization_applied', {
        \ 'materialization_index': '3',
        \ 'materialization': 'client_close_reopen',
        \ 'picks_generation': 'g3_old_clean',
        \ })
  if s:MaterializeBarrier(s:update_before, s:wire_before,
        \ "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
        \ . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0",
        \ 'materialization_state_never_arrived')
    call s:EmitGenerationCurrent(4, 'g3_decoy_control')
  endif
endif

" ---------------------------------------------------------------- client
" settings generations

if empty(s:failures)
  call VimLspHostCloseBuffer()
  call VimLspHostOpenFixture(s:fixture_root . '/' . s:settings_file_rel)
  if s:WaitFor("VimLspHostBufferDiagnosticsCounts()['warning'] >= 1", s:budget)
        \ && s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] == 0", s:budget / 3)
    call s:EmitGenerationCurrent(5, 'settings_pl701_present')
  else
    call s:Fail('settings_warning_never_arrived')
  endif
endif

if empty(s:failures)
  " Control: the identical client reopen without any settings push must keep
  " the discriminator present — the reopen alone can never clear it.
  let s:update_before = VimLspHostDiagnosticsUpdatedCount()
  let s:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
  call VimLspHostCloseReopen()
  call s:Emit('client_materialization_applied', {
        \ 'materialization_index': '4',
        \ 'materialization': 'client_close_reopen',
        \ 'picks_generation': 'settings_control_present',
        \ })
  if s:MaterializeBarrier(s:update_before, s:wire_before,
        \ "VimLspHostBufferDiagnosticsCounts()['warning'] >= 1",
        \ 'materialization_state_never_arrived')
    call s:EmitGenerationCurrent(6, 'settings_control_present')
  endif
endif

if empty(s:failures)
  " The stable public settings channel: lsp#update_workspace_config pushes
  " workspace/didChangeConfiguration with the delivered include paths. The
  " wire must carry the push before the effect materializes.
  let s:push_before = VimLspHostWireMarkerCount('workspace/didChangeConfiguration')
  call VimLspHostUpdateWorkspaceConfig(s:settings_paths)
  let s:push_expr = 'VimLspHostWireMarkerCount(''workspace/didChangeConfiguration'') >= '
        \ . (s:push_before + 1)
  if !s:WaitFor(s:push_expr, s:budget)
    call s:Fail('settings_push_never_reached_wire')
  else
    call s:Emit('client_materialization_applied', {
          \ 'materialization_index': '5',
          \ 'materialization': 'settings_push',
          \ 'picks_generation': 'settings_post_push',
          \ })
  endif
endif

if empty(s:failures)
  " Effect: the next document open materializes the pushed configuration. In
  " the ambient negative variant the pushed absolute path is rejected by the
  " server, the discriminator must stay, and the journey fails typed.
  let s:update_before = VimLspHostDiagnosticsUpdatedCount()
  let s:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
  call VimLspHostCloseReopen()
  call s:Emit('client_materialization_applied', {
        \ 'materialization_index': '6',
        \ 'materialization': 'client_close_reopen',
        \ 'picks_generation': 'settings_push_cleared',
        \ })
  let s:effect_reason = s:variant ==# 'ambient_path_only'
        \ ? 'settings_effect_absent' : 'materialization_state_never_arrived'
  if s:MaterializeBarrier(s:update_before, s:wire_before,
        \ "VimLspHostBufferDiagnosticsCounts()['warning'] == 0", s:effect_reason)
        \ && s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] == 0", s:budget / 3)
    call s:EmitGenerationCurrent(7, 'settings_push_cleared')
  endif
endif

" ---------------------------------------------------------------- project
" config generations (restart required for this subject)

if empty(s:failures)
  call VimLspHostCloseBuffer()
  call VimLspHostOpenFixture(s:fixture_root . '/' . s:config_file_rel)
  if s:WaitFor("VimLspHostBufferDiagnosticsCounts()['warning'] >= 1", s:budget)
        \ && s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] == 0", s:budget / 3)
    call s:EmitGenerationCurrent(8, 'config_critic_present')
  else
    call s:Fail('config_warning_never_arrived')
  endif
endif

" One server restart through the client's own lifecycle: close the governed
" buffer (didClose to the live server), stop through the public path, then
" reopen — the fresh FileType fires the client's lazy start and a new
" initialize lands on the wire. The restart generation number is baked into
" the barrier expressions (adapter scope law).
function! s:RestartAndMaterialize(index, init_before, picks_generation, state_expr) abort
  call VimLspHostCloseBuffer()
  if !VimLspHostStopServerAndWait()
    call s:Fail('server_restart_stop_failed')
    return 0
  endif
  " The materialization baselines are captured BEFORE the reopen: a restarted
  " server that processes the queued didOpen while the init wait runs has
  " already published by the time the wait returns, and baselines taken after
  " it would wait for a second publish that never comes. The barrier's
  " strictly-greater comparisons accept the already-arrived publish.
  let l:update_before = VimLspHostDiagnosticsUpdatedCount()
  let l:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
  let l:init_expr = 'VimLspHostServerInitCount() >= ' . (a:init_before + 1)
  call VimLspHostOpenFixture(s:fixture_root . '/' . s:config_file_rel)
  if !s:WaitFor(l:init_expr, s:budget)
    call s:Fail('server_restart_init_never_arrived')
    return 0
  endif
  call s:Emit('client_materialization_applied', {
        \ 'materialization_index': string(a:index),
        \ 'materialization': 'server_restart',
        \ 'picks_generation': a:picks_generation,
        \ })
  if s:MaterializeBarrier(l:update_before, l:wire_before, a:state_expr,
        \ 'materialization_state_never_arrived')
    return 1
  endif
  return 0
endfunction

if empty(s:failures)
  " Create the exclude config generation on disk.
  call s:ExternalWriteFile(
        \ s:fixture_root . '/' . s:expected_root_rel . '/.perl-lsp.toml', s:toml_exclude_text)
  call s:Emit('external_mutation_applied', {
        \ 'mutation_index': '4',
        \ 'mutation': 'in_place',
        \ 'target': 'project_config',
        \ 'disk_generation': 'toml_exclude_created',
        \ })
endif

if empty(s:failures)
  " Hold 3: a project-config generation never reaches the live server.
  if s:ObserveStaleHold(3, 'toml_exclude_created', 'config_critic_present',
        \ "VimLspHostBufferDiagnosticsCounts()['warning'] >= 1")
    if s:RestartAndMaterialize(7, VimLspHostServerInitCount(), 'config_exclude_active',
          \ "VimLspHostBufferDiagnosticsCounts()['warning'] == 0")
      call s:EmitGenerationCurrent(9, 'config_exclude_active')
    endif
  endif
endif

if empty(s:failures)
  " Replace with the malformed generation on disk (atomic replacement).
  call s:ExternalReplaceFile(
        \ s:fixture_root . '/' . s:expected_root_rel . '/.perl-lsp.toml', s:toml_malformed_text)
  call s:Emit('external_mutation_applied', {
        \ 'mutation_index': '5',
        \ 'mutation': 'atomic_replace',
        \ 'target': 'project_config',
        \ 'disk_generation': 'toml_malformed',
        \ })
endif

if empty(s:failures)
  " Hold 4: still no live effect; the exclude stays active.
  if s:ObserveStaleHold(4, 'toml_malformed', 'config_exclude_active',
        \ "VimLspHostBufferDiagnosticsCounts()['warning'] == 0")
    " Restart 2: the malformed config is honestly rejected at initialize and
    " the prior/default semantics return — the critic warning comes back.
    if s:RestartAndMaterialize(8, VimLspHostServerInitCount(), 'config_malformed_rejected',
          \ "VimLspHostBufferDiagnosticsCounts()['warning'] >= 1")
      call s:EmitGenerationCurrent(10, 'config_malformed_rejected')
    endif
  endif
endif

if empty(s:failures)
  " Repair the config generation on disk (atomic replacement).
  call s:ExternalReplaceFile(
        \ s:fixture_root . '/' . s:expected_root_rel . '/.perl-lsp.toml', s:toml_exclude_text)
  call s:Emit('external_mutation_applied', {
        \ 'mutation_index': '6',
        \ 'mutation': 'atomic_replace',
        \ 'target': 'project_config',
        \ 'disk_generation': 'toml_exclude_repaired',
        \ })
endif

if empty(s:failures)
  " Hold 5: no live effect from the repair either.
  if s:ObserveStaleHold(5, 'toml_exclude_repaired', 'config_malformed_rejected',
        \ "VimLspHostBufferDiagnosticsCounts()['warning'] >= 1")
    if s:RestartAndMaterialize(9, VimLspHostServerInitCount(), 'config_exclude_repaired',
          \ "VimLspHostBufferDiagnosticsCounts()['warning'] == 0")
      call s:EmitGenerationCurrent(11, 'config_exclude_repaired')
    endif
  endif
endif

" Shutdown mirrors the #10944 driver: stop through the public path, bind the
" client's own exit evidence, and defer to the editor teardown when the
" pinned client loses the job-exit callback in the stop/kill race.
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
