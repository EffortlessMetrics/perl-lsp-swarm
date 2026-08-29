" Thin vim-lsp editor-native adapter for the hermetic Vim host runner (#10944).
"
" Ownership split (mirrors the issue contract):
"   - Rust/xtask owns orchestration, identity, boundedness, process ledgers,
"     cleanup, and receipts. This adapter never schedules, never supervises,
"     and never writes receipts.
"   - `.ci/editor-clients/vim-vim-lsp-subject.v1.json` (#11369) owns the
"     pinned plugin bytes; the wrapper verifies the checkout before launch.
"   - `.ci/editor-clients/vim-vim-lsp-configuration.v1.json` (#11369) owns
"     the registration shape; every registration value this adapter uses is
"     delivered by the Rust wrapper from that manifest through the
"     environment contract below, never re-derived here.
"   - `.ci/editor-clients/vim-vim-lsp-activation-root.v1.json` (#7762) owns
"     root markers and filetype policy; the marker list arrives through the
"     environment, and this adapter never forces a filetype — native Vim
"     detection is the only activation path.
"
" Environment contract (fail-closed; missing or malformed input exits before
" any observation is claimed):
"   PERLLSP_VIM_HOST_CANDIDATE       exact perllsp executable (absolute)
"   PERLLSP_VIM_HOST_VIM_LSP_DIR     pinned vim-lsp checkout
"   PERLLSP_VIM_HOST_SERVER_NAME     canonical server name from #11369
"   PERLLSP_VIM_HOST_ROOT_MARKERS    comma-separated #7762 marker list
"   PERLLSP_VIM_HOST_CLIENT_LOG      vim-lsp client log target
"   PERLLSP_VIM_HOST_SERVER_TRACE    perllsp log target
"
" Thin surface exposed to drivers (the deterministic commands/barriers later
" leaves need; every function is bounded and returns a plain value):
"   VimLspHostLoadClient()          load exactly the pinned plugin
"   VimLspHostRegister()            register the canonical perllsp server
"   VimLspHostEnable()              enable the client
"   VimLspHostWaitFor(expr, ms)     bounded barrier
"   VimLspHostWaitForWireMarker(m, ms)
"                                   bounded barrier over the client log's
"                                   wire evidence (public logging surface)
"   VimLspHostServerRunning()       whether the server process runs
"   VimLspHostServerStatus()        lsp#get_server_status for the server
"   VimLspHostRootUri()             effective root URI for the server
"   VimLspHostEffectiveFiletype()   the buffer's current native filetype
"   VimLspHostServerCapabilities()  lsp#get_server_capabilities
"   VimLspHostOpenFixture(path)     open a fixture file natively
"   VimLspHostApplyEditAndFlush()   buffer edit + real client didChange flush
"   VimLspHostSetLineAndFlush(l, t) one-line buffer edit + real didChange
"                                   flush (#10946: the governed fix edit)
"   VimLspHostBufferDiagnosticsCounts()
"                                   the client's own public diagnostics state
"                                   (lsp#get_buffer_diagnostics_counts())
"   VimLspHostDiagnosticsUpdatedCount()
"                                   how many User lsp_diagnostics_updated
"                                   events the client emitted
"   VimLspHostWaitForWireMarkerCount(m, n, ms)
"                                   bounded barrier until the client log's
"                                   wire marker appears at least n times
"   VimLspHostWireMarkerCount(m)    current count of the wire marker in the
"                                   client log (generation observation)
"   VimLspHostSettledStateBarrier(expr, quiet, timeout)
"                                   negated-claim barrier: passes only when
"                                   the claim holds AND the wire push count
"                                   stayed quiet for quiet_ms (#11390)
"   VimLspHostStableStateWindow(expr, ms)
"                                   bounded stale-hold window: 1 when the
"                                   state claim held, 0 when false at start,
"                                   -1 the moment it stops holding (#11390)
"   VimLspHostCloseBuffer()         close (bwipeout) the current buffer
"   VimLspHostCloseReopen()         close and reopen the current buffer
"   VimLspHostServerInitCount()     how many lsp_server_init events fired
"                                   (process-generation observation)
"   VimLspHostBufferEnabledCount()  how many lsp_buffer_enabled events fired
"   VimLspHostUpdateWorkspaceConfig(paths)
"                                   the stable public settings channel:
"                                   lsp#update_workspace_config over a
"                                   comma-separated relative include-path list
"   VimLspHostStopServer()          stop the server through vim-lsp
"   VimLspHostStopServerAndWait()   stop + bounded wait for client exit
"                                   evidence (one bounded stop re-issue)
"   VimLspHostConfigureSaveOwner(t) arm exactly one documented save-format
"                                   owner: BufWritePre *.pl delegating to the
"                                   canonical sync action, bounded by
"                                   g:lsp_format_sync_timeout = t (#11396)
"   VimLspHostRemoveSaveOwners()    disarm every journey save owner
"   VimLspHostDuplicateSaveOwner(t) arm a SECOND identical owner (the
"                                   duplicate-owner negative control only)
"   VimLspHostSaveOwnerCount()      how many journey save owners are armed
"   VimLspHostOrdinaryWrite()       an ordinary Vim write (`:w`) — the only
"                                   save action the journey performs
"   VimLspHostManualComparatorFormat() run LspDocumentFormatSync manually
"                                   (comparator control only, never a save)
"   VimLspHostWireRequestCount(m)   outgoing (`--->`) request count for a
"                                   method on the client's own protocol log
"   VimLspHostWireResponseCount(m)  settled (`<---`) response count for a
"                                   method (the envelope echoes the request)
"   VimLspHostWireErrorResponseCount(m)
"                                   settled responses carrying a JSON-RPC
"                                   error for a method
"   VimLspHostWireEmptyResponseCount(m)
"                                   settled responses whose result is an
"                                   empty edit list
"   VimLspHostWireEditsResponseCount(m)
"                                   settled responses carrying at least one
"                                   edit
"   VimLspHostWaitForWireRequestCount(m, n, ms)
"                                   bounded barrier until n outgoing requests
"   VimLspHostWaitForWireResponseCount(m, n, ms)
"                                   bounded barrier until n settled responses
"   VimLspHostBufferTextSha256()    sha256 of the current buffer's exact bytes
"   VimLspHostFileTextSha256(path)  sha256 of a file's exact bytes
"   VimLspHostQuit()                exit the editor

set nocompatible
set nomore
set hidden
filetype on

" Resolve an environment variable to a string, with a genuinely empty value
" for unset variables. (expand('$NAME') returns the literal "$NAME" text for
" an unset variable, so the fail-closed empty checks below would never fire
" through expand; getenv() returns v:null and is the correct boundary.)
function! s:Env(name) abort
  let l:value = getenv(a:name)
  return type(l:value) == v:t_string ? l:value : ''
endfunction

let s:candidate = s:Env('PERLLSP_VIM_HOST_CANDIDATE')
let s:vim_lsp_dir = s:Env('PERLLSP_VIM_HOST_VIM_LSP_DIR')
let s:server_name = s:Env('PERLLSP_VIM_HOST_SERVER_NAME')
let s:root_markers = split(s:Env('PERLLSP_VIM_HOST_ROOT_MARKERS'), ',', v:false)
let s:client_log = s:Env('PERLLSP_VIM_HOST_CLIENT_LOG')
let s:server_trace = s:Env('PERLLSP_VIM_HOST_SERVER_TRACE')

if empty(s:candidate) || empty(s:vim_lsp_dir) || empty(s:server_name)
      \ || empty(s:root_markers) || empty(s:client_log) || empty(s:server_trace)
  echoerr 'vim-lsp adapter: required environment missing, failing closed'
  cquit 3
endif
if !filereadable(s:candidate)
  echoerr 'vim-lsp adapter: exact candidate executable is not readable: ' . s:candidate
  cquit 3
endif
if !filereadable(s:vim_lsp_dir . '/plugin/lsp.vim')
  echoerr 'vim-lsp adapter: pinned checkout has no plugin/lsp.vim entry: ' . s:vim_lsp_dir
  cquit 3
endif

" vim-lsp's public root helper decides file-vs-directory by spelling alone:
" a marker ending in `/` or `\` is searched with finddir(), every other marker
" with findfile(). The cross-editor #7762 contract deliberately keeps the
" semantic name `.git`, so adapt it here into both client spellings. `.git/`
" covers ordinary repositories; `.git` covers linked worktrees and submodules
" where Git writes a gitdir file instead of a directory.
function! VimLspHostClientRootMarkers() abort
  let l:markers = []
  for l:marker in s:root_markers
    if l:marker ==# '.git'
      call extend(l:markers, ['.git/', '.git'])
    else
      call add(l:markers, l:marker)
    endif
  endfor
  return l:markers
endfunction

" State counters over public vim-lsp User events (the same proven event
" surface the #7810 harness mined).
let g:perllsp_vim_host_server_init = 0
let g:perllsp_vim_host_buffer_enabled = 0
let g:perllsp_vim_host_server_exit = 0
let g:perllsp_vim_host_diagnostics_updated = 0

augroup perllsp_vim_host
  autocmd!
  autocmd User lsp_server_init let g:perllsp_vim_host_server_init += 1
  autocmd User lsp_buffer_enabled let g:perllsp_vim_host_buffer_enabled += 1
  autocmd User lsp_server_exit let g:perllsp_vim_host_server_exit += 1
  autocmd User lsp_diagnostics_updated let g:perllsp_vim_host_diagnostics_updated += 1
augroup END

function! VimLspHostLoadClient() abort
  " Load exactly the pinned checkout: runtimepath prepend plus explicit
  " plugin entry after the client flags are set. No other plugin location is
  " consulted, so ambient HOME plugins cannot load.
  "
  " Load proof combines the plugin's entry marker with a command the entry
  " only defines deep in its body (`:LspStopServer`, line 161 of the pinned
  " bytes): the entry sets `g:lsp_loaded` near the top, so the marker alone
  " would prove only that sourcing started, while the command proves sourcing
  " reached the server lifecycle surface this harness drives. (The original
  " `exists('*lsp#register_server')` probe was not a load proof at all —
  " `exists()` does not search runtimepath for not-yet-called autoload
  " functions, so it reported the plugin missing after a clean load.)
  let g:lsp_auto_enable = 0
  let g:lsp_log_verbose = 1
  let g:lsp_log_file = s:client_log
  let g:lsp_async_completion = 0
  let g:lsp_show_workspace_edits = 0
  execute 'set runtimepath^=' . fnameescape(s:vim_lsp_dir)
  runtime plugin/lsp.vim
  if !get(g:, 'lsp_loaded', 0) || !exists(':LspStopServer')
    echoerr 'vim-lsp adapter: pinned plugin did not load'
    cquit 3
  endif
  return v:true
endfunction

function! s:RootUri(_server_info) abort
  " #7762 consumption: nearest parent marker from the environment-delivered
  " list, cwd fallback. Client-specific directory syntax is projected at this
  " boundary; no second semantic marker policy lives here.
  let l:root = lsp#utils#find_nearest_parent_file_directory(
        \ expand('%:p'), VimLspHostClientRootMarkers())
  if empty(l:root)
    let l:root = getcwd()
  endif
  let g:perllsp_vim_host_root_callback = fnamemodify(l:root, ':p')
  return lsp#utils#path_to_uri(l:root)
endfunction

function! VimLspHostRegister() abort
  " Canonical #11369 registration shape: server name, exact absolute
  " candidate with --stdio, perl-only allowlist, #7762 root authority,
  " workspace includePaths delivered as the reviewed workspace-contained
  " relative positive example, and the bounded instrument log hooks.
  call lsp#register_server({
        \ 'name': s:server_name,
        \ 'cmd': {server_info -> [s:candidate, '--stdio']},
        \ 'allowlist': ['perl'],
        \ 'root_uri': function('s:RootUri'),
        \ 'workspace_config': {
        \   'perl': {
        \     'workspace': {
        \       'includePaths': ['lib'],
        \     },
        \   },
        \ },
        \ 'env': {
        \   'PERL_LSP_LOG_FILE': s:server_trace,
        \   'RUST_LOG': 'info',
        \ },
        \ })
  return v:true
endfunction

function! VimLspHostEnable() abort
  call lsp#enable()
  return v:true
endfunction

function! VimLspHostWaitFor(expr, timeout_ms) abort
  " Bounded barrier. Errors inside the waited expression, or inside client
  " callbacks delivered while sleeping, must not abort the caller's script
  " (Vim propagates callback errors raised during :sleep through the whole
  " sourcing stack — a killed server with a pending request response is the
  " known race). Both windows are contained here: the condition simply stays
  " unmet and the barrier keeps its parent-owned time bound.
  let l:start = reltime()
  let l:met = 0
  while !l:met
    if reltimefloat(reltime(l:start)) * 1000.0 > a:timeout_ms
      return 0
    endif
    try
      let l:met = eval(a:expr)
    catch
      let l:met = 0
    endtry
    if !l:met
      try
        sleep 20m
      catch
        " A callback error surfaced through :sleep is contained; re-check
        " the condition on the next bounded iteration.
      endtry
    endif
  endwhile
  return 1
endfunction

" Bounded barrier over the client's own protocol log (`g:lsp_log_file`, the
" public logging surface this adapter configured): waits until the log
" carries the exact wire marker (for example `textDocument/publishDiagnostics`
" as a sent or received method). This observes the protocol itself — the
" editor's state-update events alone do not prove a wire message.
function! VimLspHostWaitForWireMarker(marker, timeout_ms) abort
  let s:wire_needle = '"method":"' . a:marker . '"'
  return VimLspHostWaitFor(
        \ 'filereadable(s:client_log) && s:WireMarkerPresent()',
        \ a:timeout_ms)
endfunction

function! s:WireMarkerPresent() abort
  try
    return stridx(join(readfile(s:client_log, 'b'), "\n"), s:wire_needle) >= 0
  catch
    return 0
  endtry
endfunction

function! VimLspHostServerRunning() abort
  return lsp#is_server_running(s:server_name)
endfunction

function! VimLspHostServerStatus() abort
  return lsp#get_server_status(s:server_name)
endfunction

function! VimLspHostRootUri() abort
  return lsp#get_server_root_uri(s:server_name)
endfunction

function! VimLspHostEffectiveFiletype() abort
  return &l:filetype
endfunction

function! VimLspHostServerCapabilities() abort
  return lsp#get_server_capabilities(s:server_name)
endfunction

function! VimLspHostOpenFixture(path) abort
  " Native open only: detection is Vim's own filetype mechanism. This
  " adapter never sets a filetype (#7762 law).
  execute 'silent edit ' . fnameescape(a:path)
  return v:true
endfunction

function! VimLspHostApplyEditAndFlush() abort
  " Apply a deterministic one-line edit and flush the real client event path
  " (TextChanged -> didChange). Exposed for successor leaves; the minimal
  " harness journey does not call it.
  let l:line = line('$')
  call append(l:line, '# host adapter edit flush sentinel')
  doautocmd <nomodeline> TextChanged
  return v:true
endfunction

function! VimLspHostSetLineAndFlush(line_number, text) abort
  " Replace one buffer line through the real buffer/change path and flush the
  " real client event path (TextChanged -> didChange). The caller owns which
  " line and text; this adapter owns only the mechanism (#10946 fix edit).
  call setline(a:line_number, a:text)
  doautocmd <nomodeline> TextChanged
  return v:true
endfunction

function! VimLspHostBufferDiagnosticsCounts() abort
  " The client's own public diagnostics state for the current buffer
  " (#11369-classified stable surface): {'error': N, 'warning': N, ...}.
  return lsp#get_buffer_diagnostics_counts()
endfunction

function! VimLspHostDiagnosticsUpdatedCount() abort
  " How many times the client emitted its public diagnostics update event.
  return g:perllsp_vim_host_diagnostics_updated
endfunction

function! VimLspHostWaitForWireMarkerCount(marker, min_count, timeout_ms) abort
  " Bounded barrier until the client's own protocol log carries the exact
  " wire marker at least min_count times: the deterministic currentness
  " barrier surface for post-edit generations (#10946). The threshold rides
  " in script scope because the waited expression is eval'd outside a:
  " scope.
  let s:wire_needle = '"method":"' . a:marker . '"'
  let s:wire_needle_min = a:min_count
  return VimLspHostWaitFor(
        \ 'filereadable(s:client_log) && s:WireMarkerCount() >= s:wire_needle_min',
        \ a:timeout_ms)
endfunction

function! VimLspHostWireMarkerCount(marker) abort
  " Current number of times the client's own protocol log carries the exact
  " wire marker: the pre-edit generation observation for currentness
  " barriers (#10946).
  let s:wire_needle = '"method":"' . a:marker . '"'
  return s:WireMarkerCount()
endfunction

function! s:WireMarkerCount() abort
  try
    let l:text = join(readfile(s:client_log, 'b'), "\n")
    let l:count = 0
    let l:index = 0
    while v:true
      let l:hit = stridx(l:text, s:wire_needle, l:index)
      if l:hit < 0
        break
      endif
      let l:count += 1
      let l:index = l:hit + len(s:wire_needle)
    endwhile
    return l:count
  catch
    return 0
  endtry
endfunction

function! VimLspHostCloseReopen() abort
  " Close and reopen the current buffer through the real client path:
  " bwipeout fires the client's textDocument/didClose, the fresh edit fires
  " native detection and textDocument/didOpen with the current disk bytes.
  " The #11390 freshness explicit-reload route.
  let l:path = expand('%:p')
  silent bwipeout!
  execute 'silent edit ' . fnameescape(l:path)
  return v:true
endfunction

function! VimLspHostCloseBuffer() abort
  " Close the current buffer through the real client didClose path without
  " reopening (#11390 buffer transitions between governed files).
  silent bwipeout!
  return v:true
endfunction

function! VimLspHostServerInitCount() abort
  " How many times the client emitted its public server-init event: the
  " process-generation observation for restart routes (#11390).
  return g:perllsp_vim_host_server_init
endfunction

function! VimLspHostBufferEnabledCount() abort
  return g:perllsp_vim_host_buffer_enabled
endfunction

function! VimLspHostUpdateWorkspaceConfig(include_paths_csv) abort
  " The stable public settings channel (#11369-classified
  " lsp#update_workspace_config): merges the delivered include-path list into
  " the registered workspace configuration and pushes
  " workspace/didChangeConfiguration to the server. The caller owns the
  " channel's content (Rust-authored); this adapter owns only the push.
  call lsp#update_workspace_config(s:server_name, {
        \ 'perl': {
        \   'workspace': {
        \     'includePaths': split(a:include_paths_csv, ',', v:false),
        \   },
        \ },
        \ })
  return a:include_paths_csv
endfunction

function! VimLspHostSettledStateBarrier(expr, quiet_ms, timeout_ms) abort
  " Settled-state barrier for negated claims (errors == 0, warnings == 0):
  " after a document open the server first publishes a leading empty batch
  " and the computed batch follows, so a first-true wait can pass on the
  " transient. The claim is accepted only once it holds AND the client log's
  " publishDiagnostics count has stayed unchanged for quiet_ms — the settled
  " generation — still bounded by the parent-owned timeout.
  let s:wire_needle = '"method":"textDocument/publishDiagnostics"'
  let l:deadline = reltime()
  while v:true
    if reltimefloat(reltime(l:deadline)) * 1000.0 >= a:timeout_ms
      return 0
    endif
    let l:count = s:WireMarkerCount()
    if eval(a:expr)
      let l:quiet_start = reltime()
      while v:true
        if reltimefloat(reltime(l:quiet_start)) * 1000.0 >= a:quiet_ms
          return 1
        endif
        if reltimefloat(reltime(l:deadline)) * 1000.0 >= a:timeout_ms
          return 0
        endif
        if s:WireMarkerCount() != l:count
          break
        endif
        sleep 50m
      endwhile
    endif
    sleep 50m
  endwhile
endfunction

function! VimLspHostStableStateWindow(expr, window_ms) abort
  " Bounded stale-hold window over the client's own state: the state claim
  " (a boolean expression over the public client surfaces) must hold for the
  " whole window. Returns 1 when it held, 0 when it was false at the start,
  " or -1 the moment it stops holding — a spontaneous semantic update inside
  " a hold window is a typed route violation, never a silent pass. The wire
  " count may legitimately move: the server re-publishes idempotent
  " refreshes (index-ready) that do not change any generation, so state
  " semantics, not wire stillness, is the honest oracle. Poll granularity
  " 100ms; no fixed sleep is used as a positive barrier anywhere else.
  if !eval(a:expr)
    return 0
  endif
  let l:start = reltime()
  while v:true
    if !eval(a:expr)
      return -1
    endif
    if reltimefloat(reltime(l:start)) * 1000.0 >= a:window_ms
      return 1
    endif
    sleep 100m
  endwhile
endfunction

function! VimLspHostStopServer() abort
  call lsp#stop_server(s:server_name)
  return v:true
endfunction

" Stop the server through the public vim-lsp stop path and boundedly wait for
" the client's own exit evidence (`User lsp_server_exit`, or the server state
" leaving `running`). Some Vim builds lose the job exit callback when the kill
" races an in-flight channel write; one bounded stop re-issue recovers the
" lost-`job_stop` variant of that race, and everything stays inside the
" parent-owned wait budget either way.
function! VimLspHostStopServerAndWait() abort
  let l:budget = str2nr(s:Env('PERLLSP_VIM_HOST_BUDGET_MS'))
  if l:budget <= 0
    let l:budget = 90000
  endif
  let l:grace = l:budget / 4
  if l:grace < 5000
    let l:grace = 5000
  endif
  " Per-stop exit generation (#11390 restart routes): the session-global
  " exit counter is stale across restarts, so waiting for `> 0` would return
  " immediately on the second stop — before the process actually died — and
  " the next lazy start would see the old lsp_id and never start. The exit
  " generation is captured before the stop and baked into the waited
  " expression (adapter scope law).
  let l:exit_before = g:perllsp_vim_host_server_exit
  let l:exit_expr = 'g:perllsp_vim_host_server_exit > ' . l:exit_before
        \ . ' || !VimLspHostServerRunning()'
  call VimLspHostStopServer()
  if VimLspHostWaitFor(l:exit_expr, l:grace)
    return 1
  endif
  call VimLspHostStopServer()
  return VimLspHostWaitFor(l:exit_expr, l:grace)
endfunction

function! VimLspHostQuit() abort
  qa!
endfunction

" ---------------------------------------------------------------- #11396 save
" format surface. The owner is the documented route the pinned client itself
" recommends for format-on-save (doc/vim-lsp.txt, :LspDocumentFormatSync:
" "Useful when running |:autocmd| commands such as formatting before save";
" README.md's `autocmd! BufWritePre *.rs,*.go call execute('LspDocument
" FormatSync')`): one BufWritePre autocmd delegating to the canonical sync
" action, bounded by the documented g:lsp_format_sync_timeout option. This
" adapter owns only the mechanism; the journey owns when and how often it is
" armed, and the negative controls own the ownerless and duplicate shapes.

function! s:SaveOwnerAugroup() abort
  return 'perllsp_vim_host_save_owner'
endfunction

function! VimLspHostConfigureSaveOwner(timeout_ms) abort
  augroup perllsp_vim_host_save_owner
    autocmd!
    autocmd BufWritePre *.pl call execute('LspDocumentFormatSync')
  augroup END
  let g:lsp_format_sync_timeout = a:timeout_ms
  return VimLspHostSaveOwnerCount()
endfunction

function! VimLspHostRemoveSaveOwners() abort
  augroup perllsp_vim_host_save_owner
    autocmd!
  augroup END
  return VimLspHostSaveOwnerCount()
endfunction

function! VimLspHostDuplicateSaveOwner(timeout_ms) abort
  " The duplicate-owner negative control: a second, identical owner in its
  " own augroup, so one save must issue two formatting invocations.
  augroup perllsp_vim_host_save_owner_duplicate
    autocmd!
    autocmd BufWritePre *.pl call execute('LspDocumentFormatSync')
  augroup END
  let g:lsp_format_sync_timeout = a:timeout_ms
  return VimLspHostSaveOwnerCount()
endfunction

function! VimLspHostSaveOwnerCount() abort
  let l:total = 0
  for l:group in [s:SaveOwnerAugroup(), s:SaveOwnerAugroup() . '_duplicate']
    try
      let l:rules = execute('autocmd ' . l:group . ' BufWritePre')
    catch
      continue
    endtry
    for l:line in split(l:rules, "\n")
      if l:line =~# 'LspDocumentFormatSync'
        let l:total += 1
      endif
    endfor
  endfor
  return l:total
endfunction

function! VimLspHostOrdinaryWrite() abort
  " The ordinary user save action: nothing more than `:w`. Every save-trigger
  " fact is observed from the wire, never manufactured here.
  silent write
  return v:true
endfunction

function! VimLspHostManualComparatorFormat() abort
  " Comparator control ONLY (#11380 manual_comparator action): the identical
  " canonical command run manually. A journey may never label this
  " save-triggered; only the duplicate/manual negative variants call it.
  execute 'LspDocumentFormatSync'
  return v:true
endfunction

" Direction-aware wire counting over the client's own protocol log: outgoing
" requests are `["--->", ...]` envelopes whose payload carries the method;
" settled responses are `["<---", ...]` envelopes whose payload carries both
" the response and the echoed request (so the method appears on both lines
" and only direction separates them — the #12660 mining law).

function! s:WireCountCore(marker, needle, require_error) abort
  if !filereadable(s:client_log)
    return 0
  endif
  let l:count = 0
  try
    for l:line in readfile(s:client_log, 'b')
      if stridx(l:line, a:marker) < 0
        continue
      endif
      if stridx(l:line, a:needle) < 0
        continue
      endif
      if a:require_error && stridx(l:line, '"error":{') < 0
        continue
      endif
      let l:count += 1
    endfor
  catch
    return 0
  endtry
  return l:count
endfunction

function! VimLspHostWireRequestCount(method) abort
  return s:WireCountCore('["--->"', '"method":"' . a:method . '"', 0)
endfunction

function! VimLspHostWireResponseCount(method) abort
  return s:WireCountCore('["<---"', '"method":"' . a:method . '"', 0)
endfunction

function! VimLspHostWireErrorResponseCount(method) abort
  return s:WireCountCore('["<---"', '"method":"' . a:method . '"', 1)
endfunction

function! s:WireCountBoth(marker, needle_a, needle_b) abort
  " Lines carrying the direction marker and BOTH needles. The response and
  " the echoed request live in one envelope, so a settled formatting response
  " with an empty result is a `<---` line carrying both the method echo and
  " `"result":[]` regardless of key order.
  if !filereadable(s:client_log)
    return 0
  endif
  let l:count = 0
  try
    for l:line in readfile(s:client_log, 'b')
      if stridx(l:line, a:marker) < 0
        continue
      endif
      if stridx(l:line, a:needle_a) < 0
        continue
      endif
      if stridx(l:line, a:needle_b) < 0
        continue
      endif
      let l:count += 1
    endfor
  catch
    return 0
  endtry
  return l:count
endfunction

function! VimLspHostWireEmptyResponseCount(method) abort
  return s:WireCountBoth('["<---"', '"method":"' . a:method . '"', '"result":[]')
endfunction

function! VimLspHostWireEditsResponseCount(method) abort
  " Settled responses whose result carries at least one edit: settled
  " formatting responses that are neither error nor empty-result.
  return VimLspHostWireResponseCount(a:method)
        \ - VimLspHostWireErrorResponseCount(a:method)
        \ - VimLspHostWireEmptyResponseCount(a:method)
endfunction

function! VimLspHostWaitForWireRequestCount(method, min_count, timeout_ms) abort
  let s:wire_needle = '"method":"' . a:method . '"'
  let s:wire_needle_min = a:min_count
  return VimLspHostWaitFor(
        \ 'filereadable(s:client_log) && s:OutgoingNeedleCount() >= s:wire_needle_min',
        \ a:timeout_ms)
endfunction

function! s:OutgoingNeedleCount() abort
  return s:WireCountCore('["--->"', s:wire_needle, 0)
endfunction

function! VimLspHostWaitForWireResponseCount(method, min_count, timeout_ms) abort
  let s:wire_needle = '"method":"' . a:method . '"'
  let s:wire_needle_min = a:min_count
  return VimLspHostWaitFor(
        \ 'filereadable(s:client_log) && s:IncomingNeedleCount() >= s:wire_needle_min',
        \ a:timeout_ms)
endfunction

function! s:IncomingNeedleCount() abort
  return s:WireCountCore('["<---"', s:wire_needle, 0)
endfunction

" Exact-bytes identities (#12763 thread 3864145199): each surface reconstructs
" its exact bytes without collapsing trailing newline states — a text and that
" same text plus one more blank line must hash differently, while the same
" bytes read through either surface hash identically.

" The buffer stores line CONTENT only: the document-final newline lives in
" end-of-line state (never as a trailing empty item), and a genuine blank
" final line IS a content item. Joining the content lines with newline
" separators and appending the terminator therefore round-trips the exact
" buffer bytes and keeps both trailing states distinct.
function! s:BufferTextSha256(lines) abort
  return sha256(join(a:lines, "\n") . "\n")
endfunction

" Binary-mode reads keep a trailing empty item as the final-newline artifact,
" so the item list itself encodes whether the file ends in a newline:
" joining with newline separators and appending the terminator only when the
" final item carries content reconstructs the exact bytes — the identity the
" buffer helper assigns to the same loaded text.
function! s:FileTextSha256(path) abort
  let l:lines = readfile(a:path, 'b')
  let l:text = join(l:lines, "\n")
  if !empty(l:lines) && l:lines[-1] !=# ''
    let l:text .= "\n"
  endif
  return sha256(l:text)
endfunction

function! VimLspHostBufferTextSha256() abort
  return s:BufferTextSha256(getline(1, '$'))
endfunction

function! VimLspHostFileTextSha256(path) abort
  return s:FileTextSha256(a:path)
endfunction
