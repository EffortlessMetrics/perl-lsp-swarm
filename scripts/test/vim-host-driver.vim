" Bounded driver for the hermetic Vim + vim-lsp host runner (#10944).
"
" This driver is the only thing that runs inside Vim, and it stays thin: it
" sequences the minimal host journey through the thin adapter
" (scripts/test/vim-clients/vim-lsp-adapter.vim), emits one typed JSON event
" per lifecycle barrier for the Rust supervisor, and exits nonzero on any
" failure. It owns no process supervision, no deadline policy (the parent
" owns the hard deadline), no receipt writing, and no semantic expectations.
"
" Environment contract beyond the adapter's:
"   PERLLSP_VIM_HOST_ADAPTER             thin adapter path
"   PERLLSP_VIM_HOST_EVENT_FILE          driver event JSONL target
"   PERLLSP_VIM_HOST_CAPABILITY_SNAPSHOT initialize capability snapshot
"   PERLLSP_VIM_HOST_FIXTURE_ROOT        materialized fixture root
"   PERLLSP_VIM_HOST_CANDIDATE_SHA256    planned candidate digest (echoed in
"                                        the registration attestation)
"   PERLLSP_VIM_HOST_BUDGET_MS           per-barrier wait budget (default 20000)
"
" The minimal journey (the whole proof this harness claims):
"   start Vim -> load pinned vim-lsp -> register canonical server ->
"   open fixture main.pl (native detection) -> lsp_server_init ->
"   lsp_buffer_enabled -> capture initialize capabilities -> observe #7762
"   root selection -> stop server -> server exit -> quit.

" getenv() (not expand()) so unset variables stay empty and the fail-closed
" checks below fire: expand('$NAME') returns the literal text for unset names.
function! s:Env(name) abort
  let l:value = getenv(a:name)
  return type(l:value) == v:t_string ? l:value : ''
endfunction

let s:adapter = s:Env('PERLLSP_VIM_HOST_ADAPTER')
let s:event_file = s:Env('PERLLSP_VIM_HOST_EVENT_FILE')
let s:capability_path = s:Env('PERLLSP_VIM_HOST_CAPABILITY_SNAPSHOT')
let s:fixture_root = s:Env('PERLLSP_VIM_HOST_FIXTURE_ROOT')
let s:candidate_sha = s:Env('PERLLSP_VIM_HOST_CANDIDATE_SHA256')
let s:budget = str2nr(s:Env('PERLLSP_VIM_HOST_BUDGET_MS'))
let s:server_name = s:Env('PERLLSP_VIM_HOST_SERVER_NAME')
let s:root_markers = split(s:Env('PERLLSP_VIM_HOST_ROOT_MARKERS'), ',', v:false)
if s:budget <= 0
  let s:budget = 20000
endif

if empty(s:adapter) || empty(s:event_file) || empty(s:capability_path)
      \ || empty(s:fixture_root) || empty(s:candidate_sha) || empty(s:server_name)
  echoerr 'vim host driver: required environment missing, failing closed'
  cquit 3
endif

let s:sequence = 0
let s:failures = []

function! s:Fail(message) abort
  call add(s:failures, a:message)
endfunction

function! s:Emit(kind, details) abort
  " One typed event per barrier, appended immediately so a killed run
  " retains the prefix it reached.
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

" Native-detection listener: a FileType autocommand for `perl` fired by Vim's
" own detection is the only admitted activation observation (#7762 law: a
" pre-forced filetype cannot manufacture activation).
let s:native_filetype_observed = 0
augroup perllsp_vim_host_native
  autocmd!
  autocmd FileType perl let s:native_filetype_observed = 1
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
call VimLspHostOpenFixture(s:fixture_root . '/main.pl')
call s:Emit('fixture_opened', {'file': 'main.pl'})

if !s:WaitFor('g:perllsp_vim_host_server_init > 0', s:budget)
  call s:Fail('lsp_server_init never fired within budget')
else
  call s:Emit('server_initialized', {'status': VimLspHostServerStatus()})
endif

if !s:WaitFor('g:perllsp_vim_host_buffer_enabled > 0', s:budget)
  call s:Fail('lsp_buffer_enabled never fired within budget')
else
  " Script-level scope: `l:` is function-only and aborts the whole block
  " with E461 at script level, so the journey variables use script scope.
  let s:filetype = VimLspHostEffectiveFiletype()
  let s:detection = s:native_filetype_observed ? 'native_vim' : 'unobserved'
  call s:Emit('buffer_enabled', {'filetype': s:filetype, 'detection': s:detection})
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

  " #7762 root observation: the effective root must resolve through the
  " activation markers, not through an inherited working directory.
  let s:root_uri = VimLspHostRootUri()
  let s:root_path = lsp#utils#uri_to_path(s:root_uri)
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
    " The reason must stay a safe identity token: embedding the root URI
    " would violate the Rust event contract and discard the whole stream.
    call s:Fail('root did not resolve through an activation marker (marker_file_absent)')
  endif
  call s:Emit('root_selected', {'root_source': s:root_source, 'root_marker': s:root_marker})
endif

" Attach-completion mechanism observation: the wire push-diagnostics update
" for the opened buffer (perllsp publishes — possibly empty — diagnostics
" after didOpen). The barrier binds to the protocol itself through the
" client log; no diagnostic content is asserted and no diagnostics semantic
" cell is promoted.
if !VimLspHostWaitForWireMarker('textDocument/publishDiagnostics', s:budget)
  call s:Fail('wire publishDiagnostics never arrived within budget')
else
  call s:Emit('diagnostics_observed', {'mode': 'push', 'evidence': 'client_log'})
endif

" Shutdown: the stop is issued through the public vim-lsp stop path and the
" client's own exit evidence is awaited with one bounded re-issue. When the
" client exit event stays absent, the driver does NOT fail the run: the
" pinned vim-lsp loses the job-exit callback whenever the kill races an
" in-flight channel write (observed deterministically on CI linux and
" locally on Git-vim windows), while the OS process dies. The event carries
" the typed evidence state and the supervisor — which reads the client log
" after the editor's normal teardown and owns the deterministic process-set
" comparison — turns it into the honest cell result plus a recorded finding.
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
