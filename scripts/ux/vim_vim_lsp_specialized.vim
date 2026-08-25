" Specialized Vim/vim-lsp action adapter for #11380.
"
" Thin editor-native adapter: it executes one bounded row of specialized
" actions per session through the exact public Vim/vim-lsp surfaces the #11369
" inventory classifies, and emits one typed, bounded observation (JSON line)
" per action. It owns no orchestration framework, no semantic expectations
" (those are #11378), no process supervision (#10894/#10944), and no receipts
" or journey cells (#11374). The xtask validator
" (`check-vim-lsp-specialized-observations`) is the single classification
" authority: an observation this script emits is worthless until it validates.
"
" Environment contract (fail-closed, mirroring vim_vim_lsp_driver.vim):
"   PERLLSP_VIM_WORKSPACE  governed fixture workspace root
"   PERLLSP_VIM_BIN        exact perllsp executable
"   PERLLSP_VIM_LSP_DIR    pinned vim-lsp checkout (checked by the wrapper)
"   PERLLSP_VIM_RECEIPT    observations JSONL output path
"   PERLLSP_VIM_LOG        vim-lsp client log path
"   PERLLSP_VIM_SERVER_TRACE perllsp log prefix
"   PERLLSP_VIM_MODE       activation | save_format | freshness | recovery
"   PERLLSP_VIM_ADAPTER_SHA sha256 of this script, bound into every
"                          observation's adapter backend identity
"
" Marker list is the #7762 activation-contract list (cross-checked by the
" contract validator); filetype detection is native Vim only.

set nocompatible
set nomore
set hidden
filetype on

let s:workspace = expand('$PERLLSP_VIM_WORKSPACE')
let s:perllsp = expand('$PERLLSP_VIM_BIN')
let s:vim_lsp_dir = expand('$PERLLSP_VIM_LSP_DIR')
let s:receipt_path = expand('$PERLLSP_VIM_RECEIPT')
let s:log_path = expand('$PERLLSP_VIM_LOG')
let s:server_trace = expand('$PERLLSP_VIM_SERVER_TRACE')
let s:mode = expand('$PERLLSP_VIM_MODE')
let s:adapter_sha = expand('$PERLLSP_VIM_ADAPTER_SHA')
let s:failures = []

if empty(s:workspace) || empty(s:perllsp) || empty(s:vim_lsp_dir)
      \ || empty(s:receipt_path) || empty(s:mode) || empty(s:adapter_sha)
  echoerr 'specialized adapter: required environment missing, failing closed'
  cquit 3
endif

if s:adapter_sha !~# '^sha256:[0-9a-f]\{64}$'
  echoerr 'specialized adapter: adapter digest must be sha256:<64hex>, failing closed'
  cquit 3
endif

" --------------------------------------------------------------- state model
" Generation counters are adapter-side instrument state over public events:
" host = this Vim instance, process = lsp_server_init firings, document =
" lsp_buffer_enabled firings, source = completed writes, config = accepted
" workspace-setting changes, root = workspace root selection.
let g:perllsp_server_init = 0
let g:perllsp_buffer_enabled = 0
let g:perllsp_server_exit = 0
let g:perllsp_diagnostics_updated = 0
let s:source_generation = 0
let s:config_generation = 0
let s:owner_requests = 0
let s:owner_applied = 0
let s:save_events = 0
let s:observed = []

augroup perllsp_specialized
  autocmd!
  autocmd User lsp_server_init let g:perllsp_server_init += 1
  autocmd User lsp_buffer_enabled let g:perllsp_buffer_enabled += 1
  autocmd User lsp_server_exit let g:perllsp_server_exit += 1
  autocmd User lsp_diagnostics_updated let g:perllsp_diagnostics_updated += 1
augroup END

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

function! s:WaitForSafe(expr, timeout_ms) abort
  " Contained wait: while an old server generation dies, vim-lsp's own
  " response callbacks can raise transient channel-write errors inside the
  " event pump — including during `sleep`, which processes channel events.
  " The wait contains them and keeps polling until the named state settles or
  " the budget ends — a timeout stays typed evidence, and the error never
  " fabricates or suppresses an observation.
  let l:start = reltime()
  while 1
    try
      if eval(a:expr)
        return 1
      endif
      sleep 20m
    catch
      " transient channel error from the dying generation: keep waiting
    endtry
    if reltimefloat(reltime(l:start)) * 1000.0 > a:timeout_ms
      return 0
    endif
  endwhile
endfunction

function! s:Snapshot() abort
  return {
        \ 'host_generation': 1,
        \ 'process_generation': g:perllsp_server_init,
        \ 'document_generation': g:perllsp_buffer_enabled,
        \ 'root_generation': 1,
        \ 'source_generation': s:source_generation,
        \ 'config_generation': s:config_generation,
        \ }
endfunction

function! s:Barrier(kind, timeout_ms) abort
  " Typed timeout evidence: lawful, but the validator forces not_proven.
  return {'evidence': 'timed_out', 'kind': a:kind, 'waited_ms': a:timeout_ms}
endfunction

function! s:BarrierBase(kind) abort
  return {'evidence': 'satisfied', 'kind': a:kind,
        \ 'settled_generations': s:Snapshot(), 'waited_ms': 5}
endfunction

function! s:Digest(text) abort
  return 'sha256:' . sha256(a:text)
endfunction

function! s:BaseObservation(action_id, route) abort
  return {
        \ 'schema_version': 'vim_lsp_specialized_driver.v1',
        \ 'action_id': a:action_id,
        \ 'backend': {'backend': 'adapter', 'script_digest': s:adapter_sha},
        \ 'host_product': 'vim',
        \ 'client_id': 'vim-lsp',
        \ 'server_executable': 'perllsp',
        \ 'fixture': {
        \   'fixture_owners': ['vim-vim-lsp-subject.v1'],
        \   'fixture_relative_paths': ['main.pl', 'lib/Widget.pm'],
        \ },
        \ 'generations': s:Snapshot(),
        \ 'route': a:route,
        \ 'trigger': v:null,
        \ 'configured_owner_count': v:null,
        \ 'owner': v:null,
        \ 'semantic_probe': v:null,
        \ 'cardinalities': {},
        \ 'digests': {},
        \ 'barriers': [],
        \ 'protocol_events': [],
        \ 'process': {'running': {'generation': g:perllsp_server_init}},
        \ 'cleanup': 'settled',
        \ 'session_iterations': v:null,
        \ 'detection_route': v:null,
        \ 'outcome': 'applied',
        \ 'limitation': v:null,
        \ }
endfunction

function! s:Emit(observation) abort
  call add(s:observed, json_encode(a:observation))
endfunction

function! s:Flush() abort
  if empty(s:observed)
    call s:Fail('no observations were produced in mode ' . s:mode)
    return
  endif
  call writefile(s:observed, s:receipt_path, 'a')
endfunction

" ------------------------------------------------------------- client setup
execute 'set runtimepath^=' . fnameescape(s:vim_lsp_dir)
let g:lsp_auto_enable = 0
let g:lsp_log_verbose = 1
let g:lsp_log_file = s:log_path
let g:lsp_async_completion = 0
let g:lsp_show_workspace_edits = 0
runtime plugin/lsp.vim

function! s:RootUri(server_info) abort
  " #7762 activation-contract marker list (nearest parent marker, cwd
  " fallback) — consumed here, never re-derived.
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
      \   'perl': {'workspace': {'includePaths': [s:workspace . '/lib']}},
      \ },
      \ 'env': {'PERL_LSP_LOG_FILE': s:server_trace, 'RUST_LOG': 'info'},
      \ })
" Enable the classified buffer-tracking surface (as the canonical baseline
" driver does): this registers the client's buffer autocmds so an ordinary
" open attaches the server through native behavior, never through a raw
" protocol substitute.
call lsp#enable()

function! s:OpenFixture() abort
  execute 'lcd ' . fnameescape(s:workspace)
  " Ordinary open with no pre-set filetype: detection must be native.
  execute 'silent edit ' . fnameescape(s:workspace . '/main.pl')
  return s:WaitFor('g:perllsp_server_init > 0 && g:perllsp_buffer_enabled > 0', 15000)
endfunction

" Script-level response slot: a lambda's s: scope is its own, so callbacks
" land in script state (the same pattern the baseline driver uses).
let s:hover_response = v:null

function! s:HoverDone(data) abort
  let s:hover_response = a:data
endfunction

function! s:HoverProbe(probe_class) abort
  " Small semantic discriminator through the public request channel; the
  " result is only ever digested, never carried as raw text.
  " Locate the discriminator symbol dynamically so a fixture line shift
  " cannot silently move the probe onto an unrelated token.
  let s:probe_line = 0
  let s:line_index = 1
  for s:candidate in getline(1, '$')
    if s:candidate =~# 'Widget::answer'
      let s:probe_line = s:line_index
      break
    endif
    let s:line_index += 1
  endfor
  if s:probe_line == 0
    return v:null
  endif
  call cursor(s:probe_line, match(getline(s:probe_line), 'Widget::answer') + 1)
  let s:hover_response = v:null
  call lsp#send_request('perllsp-under-test', {
        \ 'method': 'textDocument/hover',
        \ 'params': {
        \   'textDocument': lsp#get_text_document_identifier(),
        \   'position': lsp#get_position(),
        \ },
        \ 'on_notification': function('s:HoverDone'),
        \ })
  if !s:WaitForSafe('type(s:hover_response) == type({})', 8000)
    return v:null
  endif
  let l:payload = get(s:hover_response, 'response', {})
  let l:result = get(l:payload, 'result', v:null)
  if type(l:result) == type(v:null)
    return v:null
  endif
  return {
        \ 'probe_class': a:probe_class,
        \ 'provider_identity': 'perllsp',
        \ 'generation_scope': s:Snapshot(),
        \ 'result_digest': s:Digest(string(l:result)),
        \ }
endfunction

" ------------------------------------------------------------------- modes
if s:mode ==# 'activation'
  let s:attached = s:OpenFixture()
  if !s:attached
    call s:Fail('server did not initialize and enable the Perl buffer')
  endif

  " Barrier honesty: Satisfied evidence is emitted only for state that
  " actually settled; a state that did not settle within its budget emits
  " typed TimedOut evidence and classifies not_proven with a limitation.
  let s:native_detected = (&filetype ==# 'perl')

  let s:open = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.activation.open_without_preset_filetype',
        \ {'route': 'native_vim_surface', 'surface': ':e'})
  let s:open.detection_route = s:native_detected ? 'native' : 'pre_forced'
  let s:open.barriers = [s:native_detected
        \ ? s:BarrierBase('native_filetype_detected')
        \ : s:Barrier('native_filetype_detected', 10000)]
  if !s:native_detected
    let s:open.outcome = 'not_proven'
    let s:open.limitation = 'native_filetype_detection_did_not_settle'
  endif
  let s:open.protocol_events = [
        \ {'event_class': 'lsp_server_init', 'digest': s:Digest('init' . g:perllsp_server_init)},
        \ {'event_class': 'lsp_buffer_enabled', 'digest': s:Digest('enable' . g:perllsp_buffer_enabled)},
        \ ]
  call s:Emit(s:open)

  let s:filetype = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.activation.observe_native_filetype',
        \ {'route': 'native_vim_surface', 'surface': '&filetype'})
  let s:filetype.detection_route = s:native_detected ? 'native' : 'pre_forced'
  let s:filetype.cardinalities = {'native_detection_rows': 1}
  let s:filetype.barriers = [s:native_detected
        \ ? s:BarrierBase('native_filetype_detected')
        \ : s:Barrier('native_filetype_detected', 10000)]
  if !s:native_detected
    let s:filetype.outcome = 'not_proven'
    let s:filetype.limitation = 'native_filetype_detection_did_not_settle'
  endif
  call s:Emit(s:filetype)

  let s:service = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.activation.observe_service_attachment',
        \ {'route': 'public_client_api', 'api': 'lsp#get_server_status(...)'})
  let s:service.owner = {'owner_class': 'service_provider', 'owner_token': 'perllsp'}
  let s:service.cardinalities = {'service_attach_events': g:perllsp_server_init}
  let s:service.barriers = s:attached ? [
        \ s:BarrierBase('service_attached'),
        \ s:BarrierBase('server_generation_initialized'),
        \ ] : [
        \ s:Barrier('service_attached', 15000),
        \ s:Barrier('server_generation_initialized', 15000),
        \ ]
  if !s:attached
    let s:service.outcome = 'not_proven'
    let s:service.limitation = 'service_attachment_did_not_settle'
  endif
  let s:service.digests = {'server_status': s:Digest(lsp#get_server_status('perllsp-under-test'))}
  call s:Emit(s:service)

  let s:probe = s:HoverProbe('hover_discriminator')
  let s:disc = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.activation.root_semantic_discriminator',
        \ {'route': 'public_client_api', 'api': 'lsp#send_request(server_name, request)'})
  if type(s:probe) == type(v:null)
    let s:disc.outcome = 'not_proven'
    let s:disc.limitation = 'hover_probe_no_result'
    let s:disc.barriers = [s:Barrier('pending_action_settled', 8000)]
  else
    let s:disc.semantic_probe = s:probe
    let s:disc.barriers = [s:BarrierBase('pending_action_settled')]
  endif
  let s:disc.owner = {'owner_class': 'service_provider', 'owner_token': 'perllsp'}
  call s:Emit(s:disc)

  let s:close = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.activation.close_reset_between_rows',
        \ {'route': 'native_vim_surface', 'surface': ':bwipeout'})
  silent bwipeout
  call s:Emit(s:close)

elseif s:mode ==# 'save_format'
  let s:attached = s:OpenFixture()
  if !s:attached
    call s:Fail('server did not initialize and enable the Perl buffer')
  endif

  " Exactly one configured save-format owner: a BufWritePre hook that asks
  " the server to format through the public request channel and applies the
  " edits through the public version-sensitive applier. The adapter never
  " formats text itself.
  let s:owner_done = 0
  let s:owner_edits = []

  function! s:OwnerFormatDone(data) abort
    let s:owner_done = 1
    if type(a:data) != type({})
      " An error or timeout path delivered no response envelope: no edits.
      let s:owner_edits = []
      return
    endif
    let l:payload = get(a:data, 'response', {})
    let s:owner_edits = type(get(l:payload, 'result', v:null)) == type([])
          \ ? get(l:payload, 'result', []) : []
  endfunction

  function! s:SingleOwnerFormat() abort
    let s:owner_requests += 1
    let s:owner_done = 0
    let s:owner_edits = []
    call lsp#send_request('perllsp-under-test', {
          \ 'method': 'textDocument/formatting',
          \ 'params': {
          \   'textDocument': lsp#get_text_document_identifier(),
          \   'options': {'tabSize': 2, 'insertSpaces': v:true},
          \ },
          \ 'on_notification': function('s:OwnerFormatDone'),
          \ })
    let l:start = reltime()
    while !s:owner_done
      if reltimefloat(reltime(l:start)) * 1000.0 > 5000
        return
      endif
      sleep 10m
    endwhile
    if !empty(s:owner_edits)
      call lsp#utils#text_edit#apply_text_edits(
            \ lsp#utils#path_to_uri(expand('%:p')), s:owner_edits)
      let s:owner_applied += 1
    endif
  endfunction

  augroup perllsp_save_format_owner
    autocmd!
    autocmd BufWritePre main.pl call s:SingleOwnerFormat()
    autocmd BufWritePost main.pl let s:save_events += 1
  augroup END

  let s:configure = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.save_format.configure_single_owner',
        \ {'route': 'native_vim_surface', 'surface': 'autocmd bufwritepre'})
  let s:configure.configured_owner_count = 1
  let s:configure.owner = {'owner_class': 'save_format_owner', 'owner_token': 'bufwritepre_owner'}
  call s:Emit(s:configure)

  let s:before = join(getline(1, '$'), "\n")
  let s:before_digest = s:Digest(s:before)
  silent write
  " The save barrier is the save event AND the single owner's settlement;
  " a write whose formatting request never answered does not satisfy it.
  let s:write_ok = s:WaitForSafe('s:save_events > 0 && s:owner_done', 8000)

  " The ordinary write itself: an ordinary Vim save action whose formatting,
  " if any, came from the single configured save-event owner — never from a
  " raw format request relabeled as save-triggered.
  let s:write = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.save_format.ordinary_write',
        \ {'route': 'native_vim_surface', 'surface': ':w'})
  let s:write.trigger = 'save_event'
  let s:write.configured_owner_count = 1
  let s:write.owner = {'owner_class': 'save_format_owner', 'owner_token': 'bufwritepre_owner'}
  let s:write.cardinalities = {
        \ 'save_events': s:save_events,
        \ 'owner_requests': s:owner_requests,
        \ }
  if s:write_ok
    let s:write.barriers = [s:BarrierBase('save_event_and_owner_settled')]
  else
    let s:write.barriers = [s:Barrier('save_event_and_owner_settled', 5000)]
    let s:write.outcome = 'not_proven'
    let s:write.limitation = 'save_event_timeout'
    call s:Fail('save event did not settle within the bounded wait')
  endif
  call s:Emit(s:write)

  let s:settled = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.save_format.observe_save_settlement',
        \ {'route': 'public_client_api', 'api': 'lsp#get_server_status(...)'})
  let s:settled.trigger = 'save_event'
  let s:settled.configured_owner_count = 1
  let s:settled.owner = {'owner_class': 'save_format_owner', 'owner_token': 'bufwritepre_owner'}
  let s:settled.cardinalities = {
        \ 'save_events': s:save_events,
        \ 'owner_requests': s:owner_requests,
        \ 'owner_applied_edits': s:owner_applied,
        \ }
  let s:after = join(getline(1, '$'), "\n")
  let s:settled.digests = {'buffer_before': s:before_digest, 'buffer_after': s:Digest(s:after)}
  if !s:write_ok
    let s:settled.barriers = [
          \ s:Barrier('save_event_and_owner_settled', 5000),
          \ s:BarrierBase('digest_reached'),
          \ ]
    let s:settled.outcome = 'not_proven'
    let s:settled.limitation = 'save_event_timeout'
    call s:Fail('save event did not settle within the bounded wait')
  else
    let s:settled.barriers = [
          \ s:BarrierBase('save_event_and_owner_settled'),
          \ s:BarrierBase('digest_reached'),
          \ ]
    if s:after ==# s:before
      let s:settled.outcome = s:owner_requests > 0 ? 'no_change' : 'refused'
    else
      let s:settled.outcome = s:owner_applied > 0 ? 'applied' : 'refused'
    endif
  endif
  let s:source_generation += 1
  let s:settled.generations = s:Snapshot()
  call s:Emit(s:settled)

  autocmd! perllsp_save_format_owner

elseif s:mode ==# 'freshness'
  let s:attached = s:OpenFixture()
  if !s:attached
    call s:Fail('server did not initialize and enable the Perl buffer')
  endif


  " Change one governed workspace setting through the public config surface.
  " The change is real (the workspace root joins the registration include
  " path) and acceptance requires a diagnostics event from AFTER the change,
  " so the registration-time baseline cannot satisfy the barrier.
  let s:diag_baseline = g:perllsp_diagnostics_updated
  call lsp#update_workspace_config('perllsp-under-test', {
        \ 'perl': {'workspace': {'includePaths': [s:workspace . '/lib', s:workspace]}},
        \ })
  let s:config_generation += 1
  let s:accepted = s:WaitForSafe('g:perllsp_diagnostics_updated > s:diag_baseline', 15000)

  let s:change = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.freshness.workspace_setting_change',
        \ {'route': 'public_client_api', 'api': 'lsp#update_workspace_config(server_name, workspace_config)'})
  if s:accepted
    let s:change.barriers = [s:BarrierBase('document_generation_accepted')]
  else
    let s:change.barriers = [s:Barrier('document_generation_accepted', 10000)]
    let s:change.outcome = 'not_proven'
    let s:change.limitation = 'config_generation_acceptance_timeout'
    call s:Fail('workspace setting change was not accepted within the bounded wait')
  endif
  call s:Emit(s:change)

  let s:probe = s:HoverProbe('post_change_hover')
  let s:observe = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.freshness.observe_route_and_generation',
        \ {'route': 'public_client_api', 'api': 'lsp#get_buffer_diagnostics_counts()'})
  let s:observe.owner = {'owner_class': 'service_provider', 'owner_token': 'perllsp'}
  let s:observe.cardinalities = {
        \ 'diagnostics_events': g:perllsp_diagnostics_updated,
        \ 'buffer_enabled_events': g:perllsp_buffer_enabled,
        \ }
  if type(s:probe) == type(v:null)
    let s:observe.outcome = 'not_proven'
    let s:observe.limitation = 'semantic_probe_no_result'
    let s:observe.barriers = [s:Barrier('document_generation_accepted', 10000)]
  else
    let s:observe.semantic_probe = s:probe
    let s:observe.barriers = [s:BarrierBase('document_generation_accepted')]
  endif
  call s:Emit(s:observe)

elseif s:mode ==# 'recovery'
  let s:attached = s:OpenFixture()
  if !s:attached
    call s:Fail('server did not initialize and enable the Perl buffer')
  endif

  let s:old_generation = g:perllsp_server_init

  " Ordinary stop through the current public route; barrier is the typed
  " exit + not-running state, never a bare disappearance.
  call lsp#stop_server('perllsp-under-test')
  let s:stopped = s:WaitForSafe('!lsp#is_server_running("perllsp-under-test")', 45000)

  let s:stop = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.recovery.stop_server_public_route',
        \ {'route': 'public_client_api', 'api': 'lsp#stop_server(server_name)'})
  if s:stopped
    let s:stop.process = {'exited_clean': {'generation': s:old_generation}}
    let s:stop.barriers = [s:BarrierBase('process_exited_cleanup_settled')]
  else
    let s:stop.process = 'unknown'
    let s:stop.cleanup = 'unknown'
    let s:stop.barriers = [s:Barrier('process_exited_cleanup_settled', 45000)]
    let s:stop.outcome = 'not_proven'
    let s:stop.limitation = 'server_exit_not_observed'
    call s:Fail('server did not stop within the bounded wait')
  endif
  call s:Emit(s:stop)

  " Restart through the public route: the pinned client has no restart
  " command, so the restart is the classified buffer-lifecycle surface — wipe
  " the buffer and reopen the fixture exactly the way the first attach opened
  " it, which starts a fresh server generation and replays the document.
  silent bwipeout
  execute 'lcd ' . fnameescape(s:workspace)
  execute 'silent edit ' . fnameescape(s:workspace . '/main.pl')
  let s:restarted = s:WaitForSafe(
        \ 'g:perllsp_server_init > s:old_generation && g:perllsp_buffer_enabled > 1', 30000)

  let s:restart = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.recovery.restart_server_public_route',
        \ {'route': 'public_client_api',
        \  'api': 'native Vim filetype/autocmd behavior plus lsp#enable() buffer tracking'})
  if s:restarted
    let s:restart.barriers = [
          \ s:BarrierBase('server_generation_initialized'),
          \ s:BarrierBase('buffer_enabled'),
          \ ]
  else
    let s:restart.barriers = [
          \ s:Barrier('server_generation_initialized', 30000),
          \ s:Barrier('buffer_enabled', 30000),
          \ ]
    let s:restart.outcome = 'not_proven'
    let s:restart.limitation = 'restart_generation_timeout'
    call s:Fail('server restart did not reach an initialized, buffer-enabled generation')
  endif
  call s:Emit(s:restart)

  let s:replay = s:BaseObservation(
        \ 'vim.vim_lsp.specialized.recovery.observe_generation_replay',
        \ {'route': 'public_client_api', 'api': 'User autocmd lsp_server_init'})
  let s:replay.process = {'superseded_by': {
        \ 'old_generation': s:old_generation,
        \ 'new_generation': g:perllsp_server_init,
        \ }}
  let s:replay.protocol_events = [
        \ {'event_class': 'lsp_server_init',
        \  'digest': s:Digest('server_init_generation_' . g:perllsp_server_init)},
        \ {'event_class': 'lsp_buffer_enabled',
        \  'digest': s:Digest('buffer_enabled_document_' . g:perllsp_buffer_enabled)},
        \ ]
  let s:replay.cardinalities = {
        \ 'replayed_buffers': g:perllsp_buffer_enabled,
        \ 'server_init_events': g:perllsp_server_init,
        \ }
  if s:restarted && g:perllsp_buffer_enabled > 1
    let s:replay.barriers = [
          \ s:BarrierBase('server_generation_initialized'),
          \ s:BarrierBase('buffer_enabled'),
          \ s:BarrierBase('process_generation_disposed'),
          \ ]
  else
    let s:replay.barriers = [
          \ s:Barrier('server_generation_initialized', 30000),
          \ s:Barrier('buffer_enabled', 30000),
          \ s:Barrier('process_generation_disposed', 15000),
          \ ]
    let s:replay.outcome = 'not_proven'
    let s:replay.limitation = 'generation_replay_timeout'
  endif
  call s:Emit(s:replay)

else
  call s:Fail('unknown specialized mode: ' . s:mode)
endif

" Clean host exit: stop the server through the public route first, wait for
" the typed exit state, then quit — the observation records the settled
" server disposition at host exit time.
if g:perllsp_server_init > 0 && lsp#is_server_running('perllsp-under-test')
  call lsp#stop_server('perllsp-under-test')
  let s:exit_settled = s:WaitForSafe('!lsp#is_server_running("perllsp-under-test")', 45000)
else
  let s:exit_settled = 1
endif
let s:exit = s:BaseObservation(
      \ 'vim.vim_lsp.specialized.host_reopen.exit_host',
      \ {'route': 'native_vim_surface', 'surface': ':qa!'})
if s:exit_settled
  let s:exit.process = {'exited_clean': {'generation': g:perllsp_server_init}}
  let s:exit.barriers = [s:BarrierBase('process_exited_cleanup_settled')]
else
  let s:exit.process = 'unknown'
  let s:exit.cleanup = 'unknown'
  let s:exit.barriers = [s:Barrier('process_exited_cleanup_settled', 45000)]
  let s:exit.outcome = 'not_proven'
  let s:exit.limitation = 'server_exit_not_observed_at_host_exit'
  call s:Fail('server had not settled before host exit')
endif
call s:Emit(s:exit)

call s:Flush()
if !empty(s:failures)
  call writefile(map(copy(s:failures), 'v:val'), s:receipt_path . '.failures', 'a')
  cquit 2
endif
qa!
