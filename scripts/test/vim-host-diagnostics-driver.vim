" Bounded driver for the #10946 bootstrap/diagnostics hermetic journey.
"
" Same ownership laws as scripts/test/vim-host-driver.vim (#10944): this
" driver stays thin — it sequences the journey through the thin adapter,
" emits one typed JSON event per barrier for the Rust supervisor, and exits
" nonzero on any failure. It owns no process supervision, no deadline policy,
" no receipt writing, and no semantic expectations: the governed defect line,
" its fix, and the expected/decoy root identities arrive from the Rust
" scenario through the environment and are only applied/observed here.
"
" Environment contract beyond the #10944 adapter's:
"   PERLLSP_VIM_HOST_ADAPTER             thin adapter path
"   PERLLSP_VIM_HOST_EVENT_FILE          driver event JSONL target
"   PERLLSP_VIM_HOST_CAPABILITY_SNAPSHOT initialize capability snapshot
"   PERLLSP_VIM_HOST_FIXTURE_ROOT        materialized fixture root
"   PERLLSP_VIM_HOST_CANDIDATE_SHA256    planned candidate digest
"   PERLLSP_VIM_HOST_BUDGET_MS           per-barrier wait budget
"   PERLLSP_VIM_HOST_OPENED_FILE_REL     governed file, fixture-relative
"   PERLLSP_VIM_HOST_EXPECTED_ROOT_REL   governed root, fixture-relative
"   PERLLSP_VIM_HOST_DECOY_ROOT_REL      decoy root, fixture-relative
"   PERLLSP_VIM_HOST_DEFECT_LINE         1-based governed defect line
"   PERLLSP_VIM_HOST_FIX_LINE            the fixed line text
"
" The journey (the whole proof this driver claims, judged by Rust):
"   start Vim -> load pinned vim-lsp -> register canonical server ->
"   open the governed broken file (native detection) -> lsp_server_init ->
"   lsp_buffer_enabled (filetype before any LSP override) -> capture
"   initialize capabilities -> #7762 root observation with decoy
"   discrimination -> governed defect visible through the client's own
"   diagnostics state -> fix the defect through the real buffer/didChange
"   path -> deterministic currentness barrier -> old discriminator absent ->
"   stop server -> server exit -> quit.

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
let s:opened_file_rel = s:Env('PERLLSP_VIM_HOST_OPENED_FILE_REL')
let s:expected_root_rel = s:Env('PERLLSP_VIM_HOST_EXPECTED_ROOT_REL')
let s:decoy_root_rel = s:Env('PERLLSP_VIM_HOST_DECOY_ROOT_REL')
let s:defect_line = str2nr(s:Env('PERLLSP_VIM_HOST_DEFECT_LINE'))
let s:fix_line = s:Env('PERLLSP_VIM_HOST_FIX_LINE')
if s:budget <= 0
  let s:budget = 20000
endif

if empty(s:adapter) || empty(s:event_file) || empty(s:capability_path)
      \ || empty(s:fixture_root) || empty(s:candidate_sha) || empty(s:server_name)
      \ || empty(s:opened_file_rel) || empty(s:expected_root_rel)
      \ || empty(s:decoy_root_rel) || s:defect_line <= 0 || empty(s:fix_line)
  echoerr 'vim diagnostics driver: required environment missing, failing closed'
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

" Fixture-relative identity of an absolute path, or '' when the path is
" outside the fixture. Report-only: the Rust scenario owns the expectation;
" the driver never derives it. Normalization is case-insensitive (Windows
" path casing is not stable) and maps the win32unix drive form (`/f/...`,
" what Git-vim's uri_to_path yields) back to `F:/...` so both sides compare.
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

" Native-detection listener: a FileType autocommand for `perl` fired by Vim's
" own detection is the only admitted activation observation (#7762 law), and
" the observed filetype is retained at the moment of detection, before any
" later LSP-specific override could exist.
let s:native_filetype_observed = 0
let s:native_filetype_value = ''
augroup perllsp_vim_host_native
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

  " #7762 root observation with decoy discrimination: the observed root must
  " resolve through the activation markers to the governed project root, and
  " the same-named outer decoy identity is retained so Rust can prove the
  " server did not answer from it. The identities are fixture-relative tokens;
  " the expectation itself stays Rust-owned.
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
    " A server starting from the wrong project root cannot satisfy this
    " slice merely because diagnostics might still appear: the lifecycle
    " barriers below are skipped and the run fails with the typed reason.
    call s:Fail('root_mismatch')
  endif
endif

" Attach-completion wire barrier: the client's own protocol log must carry
" the publishDiagnostics push for the opened buffer before any state claim.
if !VimLspHostWaitForWireMarkerCount('textDocument/publishDiagnostics', 1, s:budget)
  call s:Fail('wire publishDiagnostics never arrived within budget')
else
  call s:Emit('diagnostics_observed', {'mode': 'push', 'evidence': 'client_log'})
endif

if empty(s:failures)
  " Governed defect through the client's own diagnostics state: the public
  " counts surface must report the error after the client processed its
  " update. A fast negative witness is honored: the update event fired, the
  " state is settled, and the error count is zero — the governed defect is
  " genuinely absent and the run fails without burning the whole budget
  " (defect_absent negative control).
  if !s:WaitFor('VimLspHostDiagnosticsUpdatedCount() > 0', s:budget)
    call s:Fail('defect_update_never_arrived')
  elseif s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] >= 1", s:budget / 3)
    call s:Emit('defect_state_observed', {
          \ 'state_source': 'client_state',
          \ 'errors': string(VimLspHostBufferDiagnosticsCounts()['error']),
          \ })
  elseif VimLspHostBufferDiagnosticsCounts()['error'] == 0
    call s:Fail('defect_state_absent')
  else
    call s:Fail('defect_state_never_visible')
  endif
endif

if empty(s:failures)
  " Deterministic pre-edit generation markers: the client's update-event
  " count and its own wire push count.
  let s:generation_before = VimLspHostDiagnosticsUpdatedCount()
  let s:marker_count_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')

  " The governed fix edit through the real buffer/change path.
  call VimLspHostSetLineAndFlush(s:defect_line, s:fix_line)
  call s:Emit('defect_fix_applied', {'edit_path': 'buffer_did_change'})

  " Currentness barrier (no fixed sleep): the client must emit a new
  " diagnostics update event AND its own log must carry one more
  " publishDiagnostics push AND the client state must show the governed
  " discriminator gone. All three together are the accepted generation; a
  " reused pre-edit push or an unchanged stale state cannot satisfy it. The
  " generation number is baked into the expression (the barrier is eval'd in
  " the adapter's scope, where this script's variables do not exist) and the
  " expression rides in script scope (`l:` is function-only at script level).
  let s:generation_expr =
        \ 'VimLspHostDiagnosticsUpdatedCount() > ' . s:generation_before
  if !s:WaitFor(s:generation_expr, s:budget)
    call s:Fail('post_edit_diagnostics_event_never_arrived')
  elseif !VimLspHostWaitForWireMarkerCount('textDocument/publishDiagnostics', s:marker_count_before + 1, s:budget)
    call s:Fail('post_edit_wire_push_never_arrived')
  elseif !s:WaitFor("VimLspHostBufferDiagnosticsCounts()['error'] == 0", s:budget)
    call s:Fail('post_edit_defect_still_visible')
  else
    call s:Emit('current_state_observed', {
          \ 'state_source': 'client_state',
          \ 'errors': '0',
          \ 'discriminator_absent': '1',
          \ 'barrier': 'diagnostics_event_and_wire',
          \ })
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
