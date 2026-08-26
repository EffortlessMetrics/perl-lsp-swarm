" Bounded driver for the #11403 expanded-activation hermetic journey.
"
" Same ownership laws as scripts/test/vim-host-diagnostics-driver.vim
" (#10946): this driver stays thin — it sequences one pass over the finite
" #7762 activation-root denominator through the thin adapter, emits typed
" JSON events per barrier for the Rust supervisor, and exits nonzero on any
" failure. It owns no process supervision, no deadline policy, no receipt
" writing, and no expectations: the row denominator, its order, its
" override boundaries, and the claimed/unclaimed semantics arrive from the
" Rust scenario through the environment and are only executed here. The
" adapter never forces a filetype — the single exception is the
" `preset_filetype_claimed` negative control, which exists precisely to be
" rejected (a pre-forced filetype can never be relabeled native).
"
" Event-stream law inherited from the supervisor substrate: the supervisor
" parses event streams fail-closed, so this driver emits an event only when
" its barrier actually bound what the stream claims — a degraded or forged
" state skips its emission and lands a typed failure instead of ever lying
" into the stream.
"
" Per row (artifact order, isolated native-first state):
"   open without a preset filetype -> retain native &filetype ->
"   [activated] bounded attachment + languageId barrier, and where the row
"   claims Perl support the settled client-state semantic discriminator ->
"   [manual_override rows whose native detection missed Perl] one narrow
"   exact-buffer setf rule applied after the native observation ->
"   close through the real client didClose path so one row cannot
"   contaminate the next.
"
" Environment contract beyond the #10944 adapter's:
"   PERLLSP_VIM_HOST_ADAPTER               thin adapter path
"   PERLLSP_VIM_HOST_EVENT_FILE            driver event JSONL target
"   PERLLSP_VIM_HOST_CAPABILITY_SNAPSHOT   initialize capability snapshot
"   PERLLSP_VIM_HOST_FIXTURE_ROOT          materialized fixture root
"   PERLLSP_VIM_HOST_CANDIDATE_SHA256      planned candidate digest
"   PERLLSP_VIM_HOST_BUDGET_MS             per-barrier wait budget
"   PERLLSP_VIM_HOST_SERVER_NAME           canonical server name (#11369)
"   PERLLSP_VIM_HOST_ROOT_MARKERS          comma-separated #7762 markers
"   PERLLSP_VIM_HOST_EXPECTED_ROOT_REL     governed root, fixture-relative
"   PERLLSP_VIM_HOST_DECOY_ROOT_REL        decoy root, fixture-relative
"   PERLLSP_VIM_HOST_ACTIVATION_ROWS_JSON  the finite denominator payload
"   PERLLSP_VIM_HOST_ACTIVATION_PHASE      canonical | preset_filetype_claimed
"                                          | blanket_override_steal

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
let s:expected_root_rel = s:Env('PERLLSP_VIM_HOST_EXPECTED_ROOT_REL')
let s:decoy_root_rel = s:Env('PERLLSP_VIM_HOST_DECOY_ROOT_REL')
let s:rows_json = s:Env('PERLLSP_VIM_HOST_ACTIVATION_ROWS_JSON')
let s:phase = s:Env('PERLLSP_VIM_HOST_ACTIVATION_PHASE')
if s:budget <= 0
  let s:budget = 20000
endif

if empty(s:adapter) || empty(s:event_file) || empty(s:capability_path)
      \ || empty(s:fixture_root) || empty(s:candidate_sha) || empty(s:server_name)
      \ || empty(s:root_markers) || empty(s:expected_root_rel)
      \ || empty(s:decoy_root_rel) || empty(s:rows_json)
      \ || index(['canonical', 'preset_filetype_claimed', 'blanket_override_steal'], s:phase) < 0
  echoerr 'vim activation driver: required environment missing or invalid, failing closed'
  cquit 3
endif

if !exists('*json_decode') || !exists('*VimLspHostWaitFor')
  echoerr 'vim activation driver: adapter or json support unavailable, failing closed'
  cquit 3
endif
let s:rows = json_decode(s:rows_json)
if type(s:rows) != v:t_list || empty(s:rows)
  echoerr 'vim activation driver: activation row payload is not a non-empty list, failing closed'
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

" Token-safe rendering of an observed filetype for the typed event stream:
" an undetected buffer reports `unset`; anything outside the reason-token
" alphabet collapses so the stream stays parseable while the exact value
" remains derivable from the retained artifacts.
function! s:FiletypeToken(value) abort
  if empty(a:value)
    return 'unset'
  endif
  return substitute(tolower(a:value), '[^a-z0-9_.-]', '_', 'g')
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
  if strpart(l:path, 0, len(l:root) + 1) ==# l:root . '/'
    return strpart(l:path, len(l:root) + 1)
  endif
  return ''
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

" The forbidden ambient rule: in canonical state no autocmd beyond the
" harness baselines may exist. Only the `blanket_override_steal` negative
" control ever installs exactly the kind of broad rule #7762 forbids
" (`*.t` -> perl); it must steal the TADS control row and be caught.
if s:phase ==# 'blanket_override_steal'
  augroup perllsp_vim_host_blanket
    autocmd!
    autocmd BufRead *.t setf perl
  augroup END
endif

" --- row-0 bootstrap on the first denominator row -------------------------
" Row 0 is a claimed Perl row on every landed denominator, so it binds the
" substrate lifecycle singletons once: open (native detection), server
" initialization, buffer enablement, initialize capabilities, #7762 root
" selection with decoy discrimination, and the wire push evidence.

let s:row0 = s:rows[0]
let s:absolute0 = s:fixture_root . '/' . get(s:row0, 'path', '')
let s:native0 = ''

call s:Emit('fixture_opened', {'bootstrap_row': get(s:row0, 'row', '')})

if s:phase ==# 'preset_filetype_claimed'
  " The manufactured-state control: force a Perl filetype right after the
  " open, retain that the observation is synthetic, and void the whole claim
  " with the typed reason. Nothing else is emitted — a pre-forced filetype
  " can never ride through a stream that calls anything native.
  setf perl
  let s:native0 = VimLspHostEffectiveFiletype()
  call s:Emit('activation_native_observed', {
        \ 'row_index': '0',
        \ 'row': get(s:row0, 'row', ''),
        \ 'observed_filetype': s:FiletypeToken(s:native0),
        \ 'detection': 'pre_forced',
        \ 'preset': '1',
        \ })
  call s:Fail('pre_forced_filetype_not_native')
else
  call VimLspHostOpenFixture(s:absolute0)
  let s:native0 = VimLspHostEffectiveFiletype()
  let s:enable_before0 = g:perllsp_vim_host_buffer_enabled
  let s:publish_before0 = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')

  " Slot-staged observation: every barrier's outcome is captured when it
  " arrives, but its event is emitted strictly in the substrate lifecycle
  " rank order (server_initialized -> buffer_enabled -> initialize -> root
  " -> diagnostics push) before any repeating activation-tier observation.
  let s:init_status = ''
  let s:attached0 = 0
  let s:attached_ft = ''
  if s:WaitFor('g:perllsp_vim_host_server_init > 0', s:budget)
    let s:init_status = VimLspHostServerStatus()
  else
    call s:Fail('lsp_server_init_never_fired')
  endif

  if s:native0 ==# 'perl'
        \ && s:WaitFor('g:perllsp_vim_host_buffer_enabled > ' . s:enable_before0, s:budget)
    let s:attached0 = 1
    let s:attached_ft = VimLspHostEffectiveFiletype()
  endif

  if !empty(s:init_status)
    call s:Emit('server_initialized', {'status': s:init_status})
  endif
  " Emission law: buffer_enabled rides the stream only when the real client
  " enablement bound a natively detected Perl buffer; otherwise the barrier
  " failure stays typed and the stream keeps telling the truth. The row-0
  " attachment observation itself rides the repeating activation tier after
  " all singletons, so it is only captured here.
  if s:attached0
    call s:Emit('buffer_enabled', {'filetype': s:attached_ft, 'detection': 'native_vim'})
  else
    call s:Fail('activation_buffer_never_enabled')
  endif

  call writefile([json_encode(VimLspHostServerCapabilities())], s:capability_path)
  call s:Emit('initialize_observed', {'capabilities_written': '1'})

  try
    let s:root_uri = VimLspHostRootUri()
    let s:root_path = substitute(lsp#utils#uri_to_path(s:root_uri), '\', '/', 'g')
    let s:observed_root0 = s:FixtureRel(s:root_path)
  catch
    let s:observed_root0 = ''
    call s:Fail('root_uri_unresolvable')
  endtry
  if !empty(s:observed_root0)
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
      call s:Fail('marker_file_absent')
    endif
    call s:Emit('root_selected', {
          \ 'root_source': s:root_source,
          \ 'root_marker': s:root_marker,
          \ 'expected_root': s:expected_root_rel,
          \ 'observed_root': s:observed_root0,
          \ 'decoy_root': s:decoy_root_rel,
          \ })
    if s:observed_root0 !=# s:expected_root_rel
      call s:Fail('root_mismatch')
    endif
  endif

  if VimLspHostWaitForWireMarkerCount(
        \ 'textDocument/publishDiagnostics', s:publish_before0 + 1, s:budget)
    call s:Emit('diagnostics_observed', {'mode': 'push', 'evidence': 'client_log'})
  else
    call s:Fail('wire_publish_diagnostics_never_arrived')
  endif

  " Activation tier for row 0: the retained native observation, the
  " attachment disposition captured during bootstrap, then the claimed-row
  " semantic discriminator (settled client-state error count plus a wire
  " batch ordered after this row's didOpen), then the reset.
  call s:Emit('activation_native_observed', {
        \ 'row_index': '0',
        \ 'row': get(s:row0, 'row', ''),
        \ 'observed_filetype': s:FiletypeToken(s:native0),
        \ 'detection': 'native_vim',
        \ 'preset': '0',
        \ })

  if s:attached0
    call s:Emit('activation_attachment_observed', {
          \ 'row_index': '0',
          \ 'row': get(s:row0, 'row', ''),
          \ 'language_id': s:FiletypeToken(s:attached_ft),
          \ 'attached': '1',
          \ })
  endif

  let s:quiet0 = s:budget / 5 < 2000 ? 2000 : s:budget / 5
  let s:settled0 = VimLspHostSettledStateBarrier(
        \ "VimLspHostBufferDiagnosticsCounts()['error'] >= 1", s:quiet0, s:budget)
  let s:publish_now0 = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
  if s:settled0 == 1 && s:publish_now0 > s:publish_before0
    call s:Emit('activation_semantic_observed', {
          \ 'row_index': '0',
          \ 'row': get(s:row0, 'row', ''),
          \ 'state_source': 'client_state',
          \ 'errors': string(VimLspHostBufferDiagnosticsCounts()['error']),
          \ })
  else
    call s:Fail('semantic_discriminator_absent')
  endif

  " Between-rows reset through the real client didClose path so one row
  " cannot contaminate the next.
  call VimLspHostCloseBuffer()
  call s:Emit('activation_row_reset', {
        \ 'row_index': '0',
        \ 'row': get(s:row0, 'row', ''),
        \ 'reset': 'buffer_close',
        \ })
endif

" --- remaining rows: one isolated native-first pass per denominator row ----

if empty(s:failures)
  let s:i = 1
  while s:i < len(s:rows)
    let s:row = s:rows[s:i]
    let s:slug = get(s:row, 'row', '')
    let s:absolute = s:fixture_root . '/' . get(s:row, 'path', '')
    let s:enable_before = g:perllsp_vim_host_buffer_enabled
    let s:publish_before = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')

    call VimLspHostOpenFixture(s:absolute)
    let s:native = VimLspHostEffectiveFiletype()

    call s:Emit('activation_native_observed', {
          \ 'row_index': string(s:i),
          \ 'row': s:slug,
          \ 'observed_filetype': s:FiletypeToken(s:native),
          \ 'detection': 'native_vim',
          \ 'preset': '0',
          \ })

    if s:native ==# 'perl'
      if get(s:row, 'negative_control', '0') ==# '1'
        " An ambiguous false subject resolved to Perl through an ambient
        " rule: the exact theft #7762 forbids. Report it typed and stop.
        call s:Fail('adjacent_language_stolen')
        break
      endif
      if s:WaitFor('g:perllsp_vim_host_buffer_enabled > ' . s:enable_before, s:budget)
        call s:Emit('activation_attachment_observed', {
              \ 'row_index': string(s:i),
              \ 'row': s:slug,
              \ 'language_id': s:FiletypeToken(VimLspHostEffectiveFiletype()),
              \ 'attached': '1',
              \ })
        if get(s:row, 'claimed', '0') ==# '1'
          " Two-source semantic discriminator on this row's own document.
          let s:quiet = s:budget / 5 < 2000 ? 2000 : s:budget / 5
          let s:settled = VimLspHostSettledStateBarrier(
                \ "VimLspHostBufferDiagnosticsCounts()['error'] >= 1", s:quiet, s:budget)
          let s:publish_now = VimLspHostWireMarkerCount('textDocument/publishDiagnostics')
          if s:settled == 1 && s:publish_now > s:publish_before
            call s:Emit('activation_semantic_observed', {
                  \ 'row_index': string(s:i),
                  \ 'row': s:slug,
                  \ 'state_source': 'client_state',
                  \ 'errors': string(VimLspHostBufferDiagnosticsCounts()['error']),
                  \ })
          else
            call s:Fail('semantic_discriminator_absent')
          endif
        endif
      else
        call s:Emit('activation_attachment_observed', {
              \ 'row_index': string(s:i),
              \ 'row': s:slug,
              \ 'language_id': s:FiletypeToken(s:native),
              \ 'attached': '0',
              \ })
        call s:Fail('activation_buffer_never_enabled')
        break
      endif
    elseif !empty(get(s:row, 'manual_override', ''))
      " Bounded override: only here, only after the native observation was
      " retained, only a narrow exact-buffer rule shaped like a reviewed
      " user equivalent. It is removed immediately after its observation.
      augroup perllsp_vim_host_row_override
        autocmd!
        execute 'autocmd BufRead ' . fnameescape(s:absolute) . ' setf perl'
      augroup END
      execute 'silent edit! ' . fnameescape(s:absolute)
      let s:after = VimLspHostEffectiveFiletype()
      call s:Emit('activation_override_applied', {
            \ 'row_index': string(s:i),
            \ 'row': s:slug,
            \ 'rule': 'narrow_exact_buffer_setf_perl',
            \ 'boundary': get(s:row, 'manual_override', ''),
            \ 'filetype_after': s:FiletypeToken(s:after),
            \ })
      if s:after ==# 'perl' && s:WaitFor(
            \ 'g:perllsp_vim_host_buffer_enabled > ' . s:enable_before, s:budget)
        call s:Emit('activation_attachment_observed', {
              \ 'row_index': string(s:i),
              \ 'row': s:slug,
              \ 'language_id': s:FiletypeToken(VimLspHostEffectiveFiletype()),
              \ 'attached': '1',
              \ })
      else
        call s:Emit('activation_attachment_observed', {
              \ 'row_index': string(s:i),
              \ 'row': s:slug,
              \ 'language_id': s:FiletypeToken(s:after),
              \ 'attached': '0',
              \ })
        call s:Fail('override_activation_never_enabled')
      endif
      silent! augroup! perllsp_vim_host_row_override
    endif

    call VimLspHostCloseBuffer()
    call s:Emit('activation_row_reset', {
          \ 'row_index': string(s:i),
          \ 'row': s:slug,
          \ 'reset': 'buffer_close',
          \ })
    let s:i += 1
  endwhile
endif

silent! augroup! perllsp_vim_host_blanket
silent! augroup! perllsp_vim_host_row_override

" Shutdown mirrors the prior drivers: stop through the public path, bind the
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
