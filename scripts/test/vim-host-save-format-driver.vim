" Bounded driver for the #11396 format-on-save hermetic journey.
"
" Same ownership laws as scripts/test/vim-host-driver.vim (#10944),
" scripts/test/vim-host-diagnostics-driver.vim (#10946), and
" scripts/test/vim-host-freshness-driver.vim (#11390): this driver stays
" thin — it sequences the journey through the thin adapter, emits one typed
" JSON event per barrier for the Rust supervisor, and exits nonzero on any
" failure. It owns no process supervision, no deadline policy, no receipt
" writing, and no semantic expectations: the canonical/no-change byte
" identities, the config generations, and the expected/decoy root identities
" arrive from the Rust scenario through the environment and are only applied
" and observed here. This driver NEVER edits a governed buffer: the only
" buffer mutation paths in the whole journey are the client's own
" apply_text_edits through the save owner (#11396 falsifier 8).
"
" Vimscript boundary laws (#12589, binding this driver too):
"   - barrier expressions are eval'd in the adapter's scope: counts and
"     digests are baked into the expression string, never referenced through
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
"   PERLLSP_VIM_HOST_SAVE_VARIANT        canonical|manual_comparator_only|
"                                        duplicate_owner|wrong_root_decoy
"   PERLLSP_VIM_HOST_OPENED_FILE_REL     governed source file, fixture-relative
"   PERLLSP_VIM_HOST_EXPECTED_ROOT_REL   governed root, fixture-relative
"   PERLLSP_VIM_HOST_DECOY_ROOT_REL      decoy root, fixture-relative
"   PERLLSP_VIM_HOST_DECOY_FILE_REL      same-named decoy file, fixture-relative
"   PERLLSP_VIM_HOST_BULK_FILE_REL       the large stale-leg document
"   PERLLSP_VIM_HOST_CANONICAL_SHA256    exact canonical bytes identity
"   PERLLSP_VIM_HOST_NON_CANONICAL_SHA256 exact non-canonical bytes identity
"   PERLLSP_VIM_HOST_BULK_SHA256         exact bulk document bytes identity
"                                        (all three raw 64-char hex, the form
"                                        Vim's own sha256() returns)
"   PERLLSP_VIM_HOST_NON_CANONICAL_TEXT  non-canonical bytes (disabled-leg
"                                        external re-mutation only)
"   PERLLSP_VIM_HOST_SAVE_SYNC_TIMEOUT_MS    bounded owner timeout (ordinary)
"   PERLLSP_VIM_HOST_STALE_SYNC_TIMEOUT_MS   stale-leg owner timeout (1ms)
"   PERLLSP_VIM_HOST_STALE_WINDOW_MS     bounded bytes-held window
"   PERLLSP_VIM_HOST_TOML_OFF_TEXT       the engine-off config generation
"   PERLLSP_VIM_HOST_TOML_EXTERNAL_TEXT  the external-engine config generation
"
" The journey (the whole proof this driver claims, judged by Rust):
"   start Vim -> load pinned vim-lsp -> register canonical server -> open the
"   governed non-canonical source (native detection) -> lsp_server_init ->
"   lsp_buffer_enabled -> capture initialize capabilities (formatting
"   advertised) -> #7762 root observation with decoy discrimination ->
"   diagnostics push observed -> arm exactly one documented BufWritePre owner
"   -> ordinary save #1 applies the canonical bytes (one request, edits) ->
"   ordinary save #2 legitimate no-change (one request, empty) -> stale leg:
"   bulk document, 1ms owner timeout, save #3 times out, the late result
"   settles and provably never applies (bytes-held window) -> owner re-armed
"   -> save #4 route-health no-change -> disabled leg: owners removed,
"   non-canonical re-mutation materialized, save #5 zero requests ->
"   refused leg: engine-off config generation through a server restart,
"   save #6 one request, empty result, bytes retained -> failure leg:
"   external-engine config generation with a missing profile through a
"   server restart, save #7 error response, bytes retained -> stop -> exit.

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
let s:variant = s:Env('PERLLSP_VIM_HOST_SAVE_VARIANT')
let s:opened_file_rel = s:Env('PERLLSP_VIM_HOST_OPENED_FILE_REL')
let s:expected_root_rel = s:Env('PERLLSP_VIM_HOST_EXPECTED_ROOT_REL')
let s:decoy_root_rel = s:Env('PERLLSP_VIM_HOST_DECOY_ROOT_REL')
let s:decoy_file_rel = s:Env('PERLLSP_VIM_HOST_DECOY_FILE_REL')
let s:bulk_file_rel = s:Env('PERLLSP_VIM_HOST_BULK_FILE_REL')
let s:canonical_sha = s:Env('PERLLSP_VIM_HOST_CANONICAL_SHA256')
let s:non_canonical_sha = s:Env('PERLLSP_VIM_HOST_NON_CANONICAL_SHA256')
let s:bulk_sha = s:Env('PERLLSP_VIM_HOST_BULK_SHA256')
let s:non_canonical_text = s:Env('PERLLSP_VIM_HOST_NON_CANONICAL_TEXT')
let s:save_timeout = str2nr(s:Env('PERLLSP_VIM_HOST_SAVE_SYNC_TIMEOUT_MS'))
let s:stale_timeout = str2nr(s:Env('PERLLSP_VIM_HOST_STALE_SYNC_TIMEOUT_MS'))
let s:stale_window = str2nr(s:Env('PERLLSP_VIM_HOST_STALE_WINDOW_MS'))
let s:toml_off_text = s:Env('PERLLSP_VIM_HOST_TOML_OFF_TEXT')
let s:toml_external_text = s:Env('PERLLSP_VIM_HOST_TOML_EXTERNAL_TEXT')
if s:budget <= 0
  let s:budget = 20000
endif

if empty(s:adapter) || empty(s:event_file) || empty(s:capability_path)
      \ || empty(s:fixture_root) || empty(s:candidate_sha) || empty(s:server_name)
      \ || empty(s:variant) || empty(s:opened_file_rel) || empty(s:expected_root_rel)
      \ || empty(s:decoy_root_rel) || empty(s:decoy_file_rel)
      \ || empty(s:bulk_file_rel) || empty(s:canonical_sha)
      \ || empty(s:non_canonical_sha) || empty(s:bulk_sha)
      \ || empty(s:non_canonical_text) || s:save_timeout <= 0
      \ || s:stale_timeout <= 0 || s:stale_window <= 0
      \ || empty(s:toml_off_text) || empty(s:toml_external_text)
  echoerr 'vim save-format driver: required environment missing, failing closed'
  cquit 3
endif

let s:sequence = 0
let s:failures = []
let s:owner_index = 0
let s:save_index = 0
let s:hold_index = 0
let s:mutation_index = 0
let s:materialization_index = 0

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

" Exact-bytes line list for writefile binary mode: writefile joins the items
" with newlines and omits any separator after the last item, so a text that
" ends in a newline must carry its own trailing empty item. Splitting the
" text itself (never the text plus an appended newline) round-trips the exact
" bytes — this journey's byte oracle is exact, so the #11390 append-style
" helper's extra trailing newline would be a forgery here.
function! s:ExternalFileLines(text) abort
  return split(a:text, "\n", v:true)
endfunction

function! s:ExternalWriteFile(path, text) abort
  call writefile(s:ExternalFileLines(a:text), a:path, 'b')
  return v:true
endfunction

" Fixture-relative identity of an absolute path (#12589 normalization law).
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

" Native-detection listener (#7762 law).
let s:native_filetype_observed = 0
let s:native_filetype_value = ''
augroup perllsp_vim_host_native_save_format
  autocmd!
  autocmd FileType perl let s:native_filetype_observed = 1
        \ | let s:native_filetype_value = expand('<amatch>')
augroup END

" The deterministic materialization barrier after a client reopen or a
" restart (#11390 law, adapted): the client's own diagnostics update event,
" one more wire push, then the settled state claim.
function! s:MaterializeBarrier(update_before, wire_before, state_expr) abort
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
  if !VimLspHostSettledStateBarrier(a:state_expr, 1500, s:budget)
    call s:Fail('materialization_state_never_arrived')
    return 0
  endif
  return 1
endfunction

" One server restart through the client's own lifecycle (#11390 law).
function! s:RestartAndOpenGoverned(picks_generation) abort
  call VimLspHostCloseBuffer()
  if !VimLspHostStopServerAndWait()
    call s:Fail('server_restart_stop_failed')
    return 0
  endif
  let l:update_before = VimLspHostDiagnosticsUpdatedCount()
  let l:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
  let l:init_before = VimLspHostServerInitCount()
  let l:init_expr = 'VimLspHostServerInitCount() >= ' . (l:init_before + 1)
  call VimLspHostOpenFixture(s:fixture_root . '/' . s:opened_file_rel)
  if !s:WaitFor(l:init_expr, s:budget)
    call s:Fail('server_restart_init_never_arrived')
    return 0
  endif
  let s:materialization_index += 1
  call s:Emit('client_materialization_applied', {
        \ 'materialization_index': string(s:materialization_index),
        \ 'materialization': 'server_restart',
        \ 'picks_generation': a:picks_generation,
        \ })
  return s:MaterializeBarrier(l:update_before, l:wire_before,
        \ "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
        \ . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0")
endfunction

" One owner configuration barrier: emit the typed owner event with the
" adapter-observed armed owner count.
function! s:EmitOwnerConfigured(timeout_ms) abort
  let s:owner_index += 1
  let l:count = VimLspHostSaveOwnerCount()
  call s:Emit('save_owner_configured', {
        \ 'owner_index': string(s:owner_index),
        \ 'owner_count': string(l:count),
        \ 'route': l:count > 0 ? 'bufwritepre_autocmd' : 'none',
        \ 'action': 'lsp_document_format_sync',
        \ 'timeout_ms': string(a:timeout_ms),
        \ })
  return l:count
endfunction

" One ordinary-save settlement: perform the ordinary write, boundedly wait
" for the settled response (unless an absence is expected), classify the
" response kind from the wire's own direction-aware counts, and emit the
" typed settlement with the exact buffer and file byte identities.
function! s:OrdinarySaveAndSettle(disposition, expect_response, trigger) abort
  let l:req_before = VimLspHostWireRequestCount('textDocument/formatting')
  let l:resp_before = VimLspHostWireResponseCount('textDocument/formatting')
  let l:err_before = VimLspHostWireErrorResponseCount('textDocument/formatting')
  let l:empty_before = VimLspHostWireEmptyResponseCount('textDocument/formatting')
  let l:edits_before = VimLspHostWireEditsResponseCount('textDocument/formatting')
  call VimLspHostOrdinaryWrite()
  let l:response_kind = 'absent'
  if a:expect_response
    let l:resp_expr = 'VimLspHostWireResponseCount(''textDocument/formatting'') >= '
          \ . (l:resp_before + 1)
    if !s:WaitFor(l:resp_expr, s:budget)
      call s:Fail('save_response_never_settled')
      return 0
    endif
    if VimLspHostWireErrorResponseCount('textDocument/formatting') > l:err_before
      let l:response_kind = 'error'
    elseif VimLspHostWireEmptyResponseCount('textDocument/formatting') > l:empty_before
      let l:response_kind = 'empty'
    elseif VimLspHostWireEditsResponseCount('textDocument/formatting') > l:edits_before
      let l:response_kind = 'edits'
    endif
  endif
  let s:save_index += 1
  call s:Emit('save_settlement_observed', {
        \ 'save_index': string(s:save_index),
        \ 'trigger': a:trigger,
        \ 'owner_count': string(VimLspHostSaveOwnerCount()),
        \ 'disposition': a:disposition,
        \ 'requests_before': string(l:req_before),
        \ 'requests_after': string(VimLspHostWireRequestCount('textDocument/formatting')),
        \ 'response_kind': l:response_kind,
        \ 'buffer_sha256': 'sha256:' . VimLspHostBufferTextSha256(),
        \ 'file_sha256': 'sha256:' . VimLspHostFileTextSha256(expand('%:p')),
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
  let s:formats_documents = get(s:capabilities, 'documentFormattingProvider', v:null)
  call s:Emit('initialize_observed', {
        \ 'capabilities_written': '1',
        \ 'position_encoding': s:position_encoding,
        \ 'document_formatting_advertised':
        \   type(s:formats_documents) == v:t_number || type(s:formats_documents) == v:t_bool
        \     ? (s:formats_documents ? '1' : '0')
        \   : type(s:formats_documents) == v:t_dict ? '1' : '0',
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

if empty(s:failures) && s:variant ==# 'manual_comparator_only'
  " Negative control: the comparator-only route. No owner is armed; the
  " identical canonical command runs manually and produces the canonical
  " bytes, but the load-bearing save trigger is absent by construction.
  call VimLspHostRemoveSaveOwners()
  call s:EmitOwnerConfigured(s:save_timeout)
  let s:req_before = VimLspHostWireRequestCount('textDocument/formatting')
  let s:resp_before = VimLspHostWireResponseCount('textDocument/formatting')
  let s:err_before = VimLspHostWireErrorResponseCount('textDocument/formatting')
  let s:empty_before = VimLspHostWireEmptyResponseCount('textDocument/formatting')
  let s:edits_before = VimLspHostWireEditsResponseCount('textDocument/formatting')
  call VimLspHostManualComparatorFormat()
  let s:resp_expr = 'VimLspHostWireResponseCount(''textDocument/formatting'') >= '
        \ . (s:resp_before + 1)
  if !s:WaitFor(s:resp_expr, s:budget)
    call s:Fail('comparator_response_never_settled')
  else
    " Classify from the wire like every other settlement (#12763 thread
    " 3864145173): the settled result may equally be empty or an error.
    if VimLspHostWireErrorResponseCount('textDocument/formatting') > s:err_before
      let s:response_kind = 'error'
    elseif VimLspHostWireEmptyResponseCount('textDocument/formatting') > s:empty_before
      let s:response_kind = 'empty'
    elseif VimLspHostWireEditsResponseCount('textDocument/formatting') > s:edits_before
      let s:response_kind = 'edits'
    else
      let s:response_kind = 'unknown'
    endif
    let s:save_index += 1
    call s:Emit('save_settlement_observed', {
          \ 'save_index': string(s:save_index),
          \ 'trigger': 'manual_comparator',
          \ 'owner_count': '0',
          \ 'disposition': 'applied',
          \ 'requests_before': string(s:req_before),
          \ 'requests_after': string(VimLspHostWireRequestCount('textDocument/formatting')),
          \ 'response_kind': s:response_kind,
          \ 'buffer_sha256': 'sha256:' . VimLspHostBufferTextSha256(),
          \ 'file_sha256': 'sha256:' . VimLspHostFileTextSha256(expand('%:p')),
          \ })
    call VimLspHostOrdinaryWrite()
  endif
  call s:Fail('save_trigger_absent')
endif

if empty(s:failures) && s:variant ==# 'duplicate_owner'
  " Negative control: two armed owners. One ordinary save must issue two
  " formatting invocations; the cardinality law refuses both counts that are
  " not exactly one.
  call VimLspHostConfigureSaveOwner(s:save_timeout)
  call VimLspHostDuplicateSaveOwner(s:save_timeout)
  call s:EmitOwnerConfigured(s:save_timeout)
  let s:req_before = VimLspHostWireRequestCount('textDocument/formatting')
  let s:resp_before = VimLspHostWireResponseCount('textDocument/formatting')
  let s:err_before = VimLspHostWireErrorResponseCount('textDocument/formatting')
  let s:empty_before = VimLspHostWireEmptyResponseCount('textDocument/formatting')
  let s:edits_before = VimLspHostWireEditsResponseCount('textDocument/formatting')
  call VimLspHostOrdinaryWrite()
  let s:req_expr = 'VimLspHostWireRequestCount(''textDocument/formatting'') >= '
        \ . (s:req_before + 2)
  let s:resp_expr = 'VimLspHostWireResponseCount(''textDocument/formatting'') >= '
        \ . (s:resp_before + 2)
  if s:WaitFor(s:req_expr, s:budget) && s:WaitFor(s:resp_expr, s:budget)
    " Classify from the wire like every other settlement (#12763 thread
    " 3864145173).
    if VimLspHostWireErrorResponseCount('textDocument/formatting') > s:err_before
      let s:response_kind = 'error'
    elseif VimLspHostWireEmptyResponseCount('textDocument/formatting') > s:empty_before
      let s:response_kind = 'empty'
    elseif VimLspHostWireEditsResponseCount('textDocument/formatting') > s:edits_before
      let s:response_kind = 'edits'
    else
      let s:response_kind = 'unknown'
    endif
    let s:save_index += 1
    call s:Emit('save_settlement_observed', {
          \ 'save_index': string(s:save_index),
          \ 'trigger': 'bufwritepre_save',
          \ 'owner_count': '2',
          \ 'disposition': 'applied',
          \ 'requests_before': string(s:req_before),
          \ 'requests_after': string(VimLspHostWireRequestCount('textDocument/formatting')),
          \ 'response_kind': s:response_kind,
          \ 'buffer_sha256': 'sha256:' . VimLspHostBufferTextSha256(),
          \ 'file_sha256': 'sha256:' . VimLspHostFileTextSha256(expand('%:p')),
          \ })
    " The expected negative control is only lawful once its falsifier was
    " actually observed (#12763 thread 3864145196): this exact reason is what
    " the CLI accepts as a successful duplicate-owner run.
    call s:Fail('duplicate_invocation_observed')
  else
    " The waits never observed two invocations and two settlements: report
    " the instrument failure instead of the accepted control reason so a
    " never-exercised control cannot pass CI.
    call s:Fail('duplicate_invocations_never_observed')
  endif
endif

if empty(s:failures) && s:variant ==# 'canonical'
  " Owner 1: exactly one documented BufWritePre owner.
  call VimLspHostConfigureSaveOwner(s:save_timeout)
  call s:EmitOwnerConfigured(s:save_timeout)

  " Save 1: applied formatting. The ordinary write is the only trigger; the
  " client's own apply_text_edits is the only mutation path.
  call s:OrdinarySaveAndSettle('applied', 1, 'bufwritepre_save')
  if empty(s:failures)
        \ && (VimLspHostBufferTextSha256() !=# s:canonical_sha
        \     || VimLspHostFileTextSha256(expand('%:p')) !=# s:canonical_sha)
    call s:Fail('applied_bytes_mismatch')
  endif

  " Save 2: legitimate no-change. The route must execute (one request, one
  " settled empty response) over the already-canonical source.
  if empty(s:failures)
    call s:OrdinarySaveAndSettle('no_change', 1, 'bufwritepre_save')
    if empty(s:failures)
          \ && (VimLspHostBufferTextSha256() !=# s:canonical_sha
          \     || VimLspHostFileTextSha256(expand('%:p')) !=# s:canonical_sha)
      call s:Fail('no_change_bytes_mismatch')
    endif
  endif

  " Stale leg: open the bulk document, arm the owner with the 1ms timeout,
  " and let the ordinary write time the sync format out. The write completes
  " with the non-canonical bytes; the late result settles afterwards and must
  " never apply, proven by the bytes-held window.
  if empty(s:failures)
    call VimLspHostOpenFixture(s:fixture_root . '/' . s:bulk_file_rel)
    if !s:WaitFor('g:perllsp_vim_host_buffer_enabled > 1', s:budget)
      call s:Fail('bulk buffer never attached')
    else
      let s:stale_owner_count = VimLspHostConfigureSaveOwner(s:stale_timeout)
      call s:EmitOwnerConfigured(s:stale_timeout)
      if s:stale_owner_count != 1
        call s:Fail('stale_owner_not_single')
      endif
      let s:req_before = VimLspHostWireRequestCount('textDocument/formatting')
      let s:resp_before = VimLspHostWireResponseCount('textDocument/formatting')
      let s:err_before = VimLspHostWireErrorResponseCount('textDocument/formatting')
      let s:empty_before = VimLspHostWireEmptyResponseCount('textDocument/formatting')
      let s:edits_before = VimLspHostWireEditsResponseCount('textDocument/formatting')
      call VimLspHostOrdinaryWrite()
      " The response settles AFTER the write returned: that is the released
      " stale result on the wire, classified from these same counters below.
      let s:resp_expr = 'VimLspHostWireResponseCount(''textDocument/formatting'') >= '
            \ . (s:resp_before + 1)
      if !s:WaitFor(s:resp_expr, s:budget)
        call s:Fail('stale_response_never_released')
      else
        let s:req_after = VimLspHostWireRequestCount('textDocument/formatting')
        " Classify the specific settled response from the wire's own
        " direction-aware counters (#12763 thread 3864145173): held bytes
        " alone cannot separate a rejected edit result from a settled empty
        " or error, so every claim below is gated on this classification.
        if VimLspHostWireErrorResponseCount('textDocument/formatting') > s:err_before
          let s:response_kind = 'error'
        elseif VimLspHostWireEmptyResponseCount('textDocument/formatting') > s:empty_before
          let s:response_kind = 'empty'
        elseif VimLspHostWireEditsResponseCount('textDocument/formatting') > s:edits_before
          let s:response_kind = 'edits'
        else
          let s:response_kind = 'unknown'
        endif
        " Bounded bytes-held window: the buffer stays exactly the bulk bytes
        " and no further formatting request is issued while the late result
        " is already settled (adapter scope law: digests baked in).
        let s:held_expr = 'VimLspHostBufferTextSha256() == ''' . s:bulk_sha . ''''
              \ . ' && VimLspHostWireRequestCount(''textDocument/formatting'') == ' . s:req_after
        let s:stable = VimLspHostStableStateWindow(s:held_expr, s:stale_window)
        if s:stable < 0
          call s:Fail('stale_result_applied_or_state_moved')
        elseif s:stable == 0
          call s:Fail('stale_bytes_claim_false')
        else
          if s:response_kind !=# 'edits'
            call s:Fail('stale_late_response_not_edits')
          endif
          let s:hold_index += 1
          call s:Emit('stale_result_hold_observed', {
                \ 'hold_index': string(s:hold_index),
                \ 'window_ms': string(s:stale_window),
                \ 'requests_before': string(s:req_before),
                \ 'requests_after': string(s:req_after),
                \ 'bytes_held': '1',
                \ 'late_response_rejected': s:response_kind ==# 'edits' ? '1' : '0',
                \ })
        endif
        let s:save_index += 1
        call s:Emit('save_settlement_observed', {
              \ 'save_index': string(s:save_index),
              \ 'trigger': 'bufwritepre_save',
              \ 'owner_count': string(VimLspHostSaveOwnerCount()),
              \ 'disposition': 'stale_rejected',
              \ 'requests_before': string(s:req_before),
              \ 'requests_after': string(s:req_after),
              \ 'response_kind': s:response_kind,
              \ 'buffer_sha256': 'sha256:' . VimLspHostBufferTextSha256(),
              \ 'file_sha256': 'sha256:' . VimLspHostFileTextSha256(expand('%:p')),
              \ })
        if VimLspHostFileTextSha256(expand('%:p')) !=# s:bulk_sha
          call s:Fail('stale_write_bytes_mismatch')
        endif
      endif
      " Owner 2: re-arm with the ordinary bounded timeout.
      call VimLspHostConfigureSaveOwner(s:save_timeout)
      call s:EmitOwnerConfigured(s:save_timeout)
    endif
  endif

  " Save 4: route-health recovery. The governed file is canonical on disk;
  " after the reopen the ordinary save must settle normally (one request,
  " empty response, exact bytes) — the earlier non-application was
  " staleness, not a broken route.
  if empty(s:failures)
    call VimLspHostCloseBuffer()
    call VimLspHostOpenFixture(s:fixture_root . '/' . s:opened_file_rel)
    if !s:WaitFor('g:perllsp_vim_host_buffer_enabled > 2', s:budget)
      call s:Fail('governed buffer never re-attached')
    else
      call s:OrdinarySaveAndSettle('no_change', 1, 'bufwritepre_save')
      if empty(s:failures)
            \ && (VimLspHostBufferTextSha256() !=# s:canonical_sha
            \     || VimLspHostFileTextSha256(expand('%:p')) !=# s:canonical_sha)
        call s:Fail('recovery_bytes_mismatch')
      endif
    endif
  endif

  " Disabled leg: remove every owner, externally restore the non-canonical
  " generation, materialize it through the client's own reopen, and prove the
  " ordinary save issues zero formatting requests while the bytes survive.
  if empty(s:failures)
    call VimLspHostRemoveSaveOwners()
    call s:EmitOwnerConfigured(s:save_timeout)
    let s:mutation_index += 1
    call s:ExternalWriteFile(s:fixture_root . '/' . s:opened_file_rel, s:non_canonical_text)
    call s:Emit('external_mutation_applied', {
          \ 'mutation_index': string(s:mutation_index),
          \ 'mutation': 'in_place',
          \ 'target': 'governed',
          \ 'disk_generation': 'g1_non_canonical_restored',
          \ })
    let s:update_before = VimLspHostDiagnosticsUpdatedCount()
    let s:wire_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
    call VimLspHostCloseReopen()
    let s:materialization_index += 1
    call s:Emit('client_materialization_applied', {
          \ 'materialization_index': string(s:materialization_index),
          \ 'materialization': 'client_close_reopen',
          \ 'picks_generation': 'g1_non_canonical_restored',
          \ })
    call s:MaterializeBarrier(s:update_before, s:wire_before,
          \ "VimLspHostBufferDiagnosticsCounts()['error'] == 0"
          \ . " && VimLspHostBufferDiagnosticsCounts()['warning'] == 0")
    if empty(s:failures)
      let s:req_before = VimLspHostWireRequestCount('textDocument/formatting')
      call VimLspHostOrdinaryWrite()
      " Bounded absence window: no formatting request may appear.
      let s:absent_expr = 'VimLspHostWireRequestCount(''textDocument/formatting'') == '
            \ . s:req_before
      let s:stable = VimLspHostStableStateWindow(s:absent_expr, s:stale_window)
      if s:stable < 0
        call s:Fail('disabled_save_issued_request')
      elseif s:stable == 0
        call s:Fail('disabled_absence_claim_false')
      else
        let s:save_index += 1
        call s:Emit('save_settlement_observed', {
              \ 'save_index': string(s:save_index),
              \ 'trigger': 'bufwritepre_save',
              \ 'owner_count': '0',
              \ 'disposition': 'disabled',
              \ 'requests_before': string(s:req_before),
              \ 'requests_after': string(VimLspHostWireRequestCount('textDocument/formatting')),
              \ 'response_kind': 'absent',
              \ 'buffer_sha256': 'sha256:' . VimLspHostBufferTextSha256(),
              \ 'file_sha256': 'sha256:' . VimLspHostFileTextSha256(expand('%:p')),
              \ })
        if VimLspHostBufferTextSha256() !=# s:non_canonical_sha
              \ || VimLspHostFileTextSha256(expand('%:p')) !=# s:non_canonical_sha
          call s:Fail('disabled_bytes_mismatch')
        endif
      endif
    endif
  endif

  " Owner 3: re-arm the single owner for the config-generation legs.
  if empty(s:failures)
    call VimLspHostConfigureSaveOwner(s:save_timeout)
    call s:EmitOwnerConfigured(s:save_timeout)
  endif

  " Refused leg: the engine-off config generation reaches the server through
  " a restart; the ordinary save issues one request and the server answers an
  " empty edit list — the wire shape no-change shares, distinct only through
  " the non-canonical bytes.
  if empty(s:failures)
    let s:mutation_index += 1
    call s:ExternalWriteFile(
          \ s:fixture_root . '/' . s:expected_root_rel . '/.perl-lsp.toml', s:toml_off_text)
    call s:Emit('external_mutation_applied', {
          \ 'mutation_index': string(s:mutation_index),
          \ 'mutation': 'in_place',
          \ 'target': 'project_config',
          \ 'disk_generation': 'toml_formatting_off',
          \ })
    call s:RestartAndOpenGoverned('toml_formatting_off')
    call s:OrdinarySaveAndSettle('refused', 1, 'bufwritepre_save')
    if empty(s:failures)
          \ && (VimLspHostBufferTextSha256() !=# s:non_canonical_sha
          \     || VimLspHostFileTextSha256(expand('%:p')) !=# s:non_canonical_sha)
      call s:Fail('refused_bytes_mismatch')
    endif
  endif

  " Failure leg: the external-engine config generation with a missing
  " profile is a real engine failure — the server answers a JSON-RPC error,
  " the client surfaces its format-error path, and the bytes survive.
  if empty(s:failures)
    let s:mutation_index += 1
    call s:ExternalWriteFile(
          \ s:fixture_root . '/' . s:expected_root_rel . '/.perl-lsp.toml', s:toml_external_text)
    call s:Emit('external_mutation_applied', {
          \ 'mutation_index': string(s:mutation_index),
          \ 'mutation': 'in_place',
          \ 'target': 'project_config',
          \ 'disk_generation': 'toml_external_missing_profile',
          \ })
    call s:RestartAndOpenGoverned('toml_external_missing_profile')
    call s:OrdinarySaveAndSettle('failure', 1, 'bufwritepre_save')
    if empty(s:failures)
          \ && (VimLspHostBufferTextSha256() !=# s:non_canonical_sha
          \     || VimLspHostFileTextSha256(expand('%:p')) !=# s:non_canonical_sha)
      call s:Fail('failure_bytes_mismatch')
    endif
    if empty(s:failures)
          \ && VimLspHostWireErrorResponseCount('textDocument/formatting') == 0
      call s:Fail('failure_response_not_error')
    endif
  endif
endif

" Shutdown mirrors the #10944 driver.
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
