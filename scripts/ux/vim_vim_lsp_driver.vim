set nocompatible
set nomore
set hidden
filetype on

let s:workspace = expand('$PERLLSP_VIM_WORKSPACE')
let s:sibling = expand('$PERLLSP_VIM_SIBLING')
let s:perllsp = expand('$PERLLSP_VIM_BIN')
let s:vim_lsp_dir = expand('$PERLLSP_VIM_LSP_DIR')
let s:receipt_path = expand('$PERLLSP_VIM_RECEIPT')
let s:log_path = expand('$PERLLSP_VIM_LOG')
let s:capability_path = expand('$PERLLSP_VIM_SERVER_CAPABILITIES')
let s:server_trace = expand('$PERLLSP_VIM_SERVER_TRACE')
let s:mode = expand('$PERLLSP_VIM_MODE')
let s:failures = []
let s:responses = {}
let s:cells = {}

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

function! s:Capture(key, data) abort
  let s:responses[a:key] = a:data
endfunction

function! s:Request(key, method, params) abort
  let s:responses[a:key] = v:null
  call lsp#send_request('perllsp-under-test', {
        \ 'method': a:method,
        \ 'params': a:params,
        \ 'on_notification': function('s:Capture', [a:key]),
        \ })
  if !s:WaitFor('type(s:responses[' . string(a:key) . ']) == type({})', 7000)
    call s:Fail(a:method . ' timed out')
    return {}
  endif
  let l:data = s:responses[a:key]
  if !has_key(l:data, 'response')
    call s:Fail(a:method . ' returned no response envelope')
    return {}
  endif
  let l:response = l:data.response
  if has_key(l:response, 'error')
    call s:Fail(a:method . ' returned error: ' . string(l:response.error))
  endif
  return l:response
endfunction

function! s:PositionParams() abort
  return {
        \ 'textDocument': lsp#get_text_document_identifier(),
        \ 'position': lsp#get_position(),
        \ }
endfunction

function! s:CompletionItems(response) abort
  if !has_key(a:response, 'result')
    return []
  endif
  let l:result = a:response.result
  if type(l:result) == type([])
    return l:result
  endif
  if type(l:result) == type({})
    return get(l:result, 'items', [])
  endif
  return []
endfunction

execute 'set runtimepath^=' . fnameescape(s:vim_lsp_dir)
let g:lsp_auto_enable = 0
let g:lsp_log_verbose = 1
let g:lsp_log_file = s:log_path
let g:lsp_async_completion = 0
let g:lsp_show_workspace_edits = 0
let g:lsp_experimental_workspace_folders = s:mode ==# 'workspace_folders' ? 1 : 0
runtime plugin/lsp.vim

let g:perllsp_server_init = 0
let g:perllsp_buffer_enabled = 0
let g:perllsp_diagnostics_updated = 0
let g:perllsp_server_exit = 0
augroup perllsp_deep_receipt
  autocmd!
  autocmd User lsp_server_init let g:perllsp_server_init += 1
  autocmd User lsp_buffer_enabled let g:perllsp_buffer_enabled += 1
  autocmd User lsp_diagnostics_updated let g:perllsp_diagnostics_updated += 1
  autocmd User lsp_server_exit let g:perllsp_server_exit += 1
augroup END

function! s:RootUri(server_info) abort
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
      \   'perl': {
      \     'workspace': {
      \       'includePaths': [s:workspace . '/lib'],
      \     },
      \   },
      \ },
      \ 'env': {
      \   'PERL_LSP_LOG_FILE': s:server_trace,
      \   'RUST_LOG': 'info',
      \ },
      \ })
call lsp#enable()

execute 'lcd ' . fnameescape(s:workspace)
execute 'silent edit ' . fnameescape(s:workspace . '/main.pl')
if !s:WaitFor('g:perllsp_server_init > 0 && g:perllsp_buffer_enabled > 0', 10000)
  call s:Fail('server did not initialize and enable the Perl buffer')
endif
let s:cells.initialize = g:perllsp_server_init > 0
      \ && g:perllsp_buffer_enabled > 0
      \ && lsp#get_server_status('perllsp-under-test') ==# 'running'
let s:cells.filetype = &l:filetype
let s:cells.root_uri = lsp#get_server_root_uri('perllsp-under-test')
let s:server_capabilities = lsp#get_server_capabilities('perllsp-under-test')
call writefile([json_encode(s:server_capabilities)], s:capability_path)
let s:cells.position_encoding = get(s:server_capabilities, 'positionEncoding', 'utf-16')
let s:sync = get(s:server_capabilities, 'textDocumentSync', v:null)
let s:cells.text_sync_change = type(s:sync) == type({}) ? get(s:sync, 'change', v:null) : s:sync

if s:mode ==# 'workspace_folders'
  " An autoload script is not sourced until one of its functions is called, so a
  " bare exists('*lsp#capabilities#...') returns 0 for a helper that is present
  " and working - recording a false negative in the receipt. Source the script
  " first, then test.
  silent! runtime autoload/lsp/capabilities.vim
  let s:helper_support = exists('*lsp#capabilities#has_workspace_folders_change_notifications')
        \ ? lsp#capabilities#has_workspace_folders_change_notifications('perllsp-under-test')
        \ : v:false
  execute 'silent edit ' . fnameescape(s:sibling . '/other.pl')
  call s:WaitFor('g:perllsp_buffer_enabled > 1', 5000)
  let s:cells.workspace_folders = {
        \ 'experimental_enabled': v:true,
        \ 'change_notifications_helper': s:helper_support,
        \ 'second_buffer_enabled': g:perllsp_buffer_enabled > 1,
        \ 'server_workspace_capability': get(s:server_capabilities, 'workspace', {}),
        \ }
  call lsp#stop_server('perllsp-under-test')
  let s:cells.shutdown = s:WaitFor('g:perllsp_server_exit > 0 || !lsp#is_server_running("perllsp-under-test")', 7000)
  let s:receipt = {
        \ 'schema_version': 2,
        \ 'kind': 'vim_vim_lsp_workspace_folder_observation',
        \ 'mode': s:mode,
        \ 'cells': s:cells,
        \ 'failures': s:failures,
        \ 'ok': empty(s:failures),
        \ }
  call writefile([json_encode(s:receipt)], s:receipt_path)
  if !empty(s:failures) | cquit 2 | endif
  qa!
endif

if !s:WaitFor('g:perllsp_diagnostics_updated > 0', 10000)
  call s:Fail('vim-lsp did not publish a diagnostics-updated event')
endif
let s:initial_diagnostics = lsp#get_buffer_diagnostics_counts()
let s:cells.diagnostics = {
      \ 'events': g:perllsp_diagnostics_updated,
      \ 'initial_counts': s:initial_diagnostics,
      \ }

" main.pl intentionally has no `use lib`; successful navigation to Widget.pm
" makes the documented workspace_config includePaths setting behavior-bearing.
call cursor(5, match(getline(5), 'Widget::answer') + 1)
let s:hover = s:Request('hover', 'textDocument/hover', s:PositionParams())
let s:cells.hover = has_key(s:hover, 'result') && type(s:hover.result) != type(v:null)
if !s:cells.hover | call s:Fail('hover returned no useful result') | endif
let s:def = s:Request('definition', 'textDocument/definition', s:PositionParams())
let s:def_text = string(get(s:def, 'result', v:null))
let s:cells.definition = s:def_text =~# 'Widget.pm'
let s:cells.workspace_configuration = s:cells.definition
if !s:cells.definition
  call s:Fail('definition did not resolve Widget.pm through workspace includePaths: ' . s:def_text)
endif

let s:unicode_col = match(getline(7), 'Widget::answer') + 1
call cursor(7, s:unicode_col)
let s:unicode_def = s:Request('unicode_definition', 'textDocument/definition', s:PositionParams())
let s:unicode_text = string(get(s:unicode_def, 'result', v:null))
let s:cells.unicode_position = s:unicode_text =~# 'Widget.pm'
if !s:cells.unicode_position
  call s:Fail('definition after a non-BMP character did not resolve Widget.pm: ' . s:unicode_text)
endif

let s:before_diag_events = g:perllsp_diagnostics_updated
call setline(6, 'my $copy = $value;')
doautocmd <nomodeline> TextChanged
if !s:WaitFor('g:perllsp_diagnostics_updated > s:before_diag_events', 10000)
  call s:Fail('diagnostics did not refresh after the Vim edit')
endif
let s:post_edit_diagnostics = lsp#get_buffer_diagnostics_counts()
call cursor(5, match(getline(5), 'Widget::answer') + 1)
let s:post_edit_def = s:Request('post_edit_definition', 'textDocument/definition', s:PositionParams())
let s:cells.edit_requery = string(get(s:post_edit_def, 'result', v:null)) =~# 'Widget.pm'
      \ && string(s:post_edit_diagnostics) !=# string(s:initial_diagnostics)
if !s:cells.edit_requery
  call s:Fail('post-edit diagnostics/provider state did not advance to the accepted generation')
endif
let s:cells.diagnostics.post_edit_counts = s:post_edit_diagnostics

call append(line('$'), 'subr')
doautocmd <nomodeline> TextChanged
call cursor(line('$'), strlen(getline('$')) + 1)
let s:snippet_pos = lsp#get_position()
let s:snippet_response = s:Request('snippet_completion', 'textDocument/completion', {
      \ 'textDocument': lsp#get_text_document_identifier(),
      \ 'position': s:snippet_pos,
      \ 'context': {'triggerKind': 1},
      \ })
let s:snippet_items = s:CompletionItems(s:snippet_response)
let s:snippet_item = {}
for s:item in s:snippet_items
  if get(s:item, 'label', '') ==# 'subr'
    let s:snippet_item = s:item
    break
  endif
endfor
let s:wire_insert = empty(s:snippet_item) ? ''
      \ : has_key(s:snippet_item, 'textEdit')
      \   ? get(s:snippet_item.textEdit, 'newText', '')
      \   : get(s:snippet_item, 'insertText', get(s:snippet_item, 'label', ''))
let s:plain_wire = !empty(s:snippet_item)
      \ && get(s:snippet_item, 'insertTextFormat', 1) == 1
      \ && s:wire_insert !~# '\${\|\$[0-9]'
if !s:plain_wire
  call s:Fail('subr completion was not degraded to a plain-text wire item')
endif
let s:converted = lsp#omni#get_vim_completion_items({
      \ 'server': lsp#get_server_info('perllsp-under-test'),
      \ 'position': s:snippet_pos,
      \ 'response': s:snippet_response,
      \ })
let s:converted_subr = filter(copy(s:converted.items), {_, item -> get(item, 'abbr', '') =~# '^subr'})
let s:actual_plain = v:false
if !empty(s:converted_subr)
  let g:perllsp_receipt_completion_startcol = s:converted.startcol
  let g:perllsp_receipt_completion_items = [s:converted_subr[0]]
  function! PerllspReceiptComplete() abort
    call complete(g:perllsp_receipt_completion_startcol, g:perllsp_receipt_completion_items)
    return ''
  endfunction
  set completeopt=menuone,noselect
  call feedkeys("A\<C-r>=PerllspReceiptComplete()\<CR>\<C-n>\<C-y>\<Esc>", 'xt')
  delfunction PerllspReceiptComplete
  unlet g:perllsp_receipt_completion_startcol
  unlet g:perllsp_receipt_completion_items
  let s:inserted_tail = join(getline(max([1, line('$') - 8]), '$'), "\n")
  let s:actual_plain = s:inserted_tail =~# 'sub name'
        \ && s:inserted_tail !~# '\${\|\$[0-9]'
        \ && s:inserted_tail !~# '^subr$'
else
  let s:inserted_tail = join(getline(max([1, line('$') - 8]), '$'), "\n")
endif
let s:cells.completion = {
      \ 'wire_item_count': len(s:snippet_items),
      \ 'snippet_trigger_found': !empty(s:snippet_item),
      \ 'wire_plain_text': s:plain_wire,
      \ 'client_converted_item_found': !empty(s:converted_subr),
      \ 'actual_buffer_plain_text': s:actual_plain,
      \ 'inserted_tail': s:inserted_tail,
      \ }
if !s:actual_plain
  call s:Fail('actual Vim completion insertion did not produce valid plain snippet fallback text')
endif

call cursor(5, match(getline(5), '\$value') + 1)
let s:ref_params = s:PositionParams()
let s:ref_params.context = {'includeDeclaration': v:true}
let s:refs = s:Request('references', 'textDocument/references', s:ref_params)
let s:cells.references = type(get(s:refs, 'result', v:null)) == type([]) && !empty(s:refs.result)
if !s:cells.references | call s:Fail('references returned no locations') | endif
let s:rename = s:Request('rename', 'textDocument/rename', extend(s:PositionParams(), {'newName': 'renamed_value'}))
if has_key(s:rename, 'result') && type(s:rename.result) == type({})
  call lsp#utils#workspace_edit#apply_workspace_edit(s:rename.result)
endif
let s:renamed_text = join(getline(1, '$'), "\n")
let s:cells.rename_applied = s:renamed_text =~# 'renamed_value'
if !s:cells.rename_applied
  call s:Fail('rename WorkspaceEdit was not applied to the Vim buffer')
endif

let s:before_format = join(getline(1, '$'), "\n")
let s:format = s:Request('formatting', 'textDocument/formatting', {
      \ 'textDocument': lsp#get_text_document_identifier(),
      \ 'options': {'tabSize': &shiftwidth > 0 ? &shiftwidth : 4, 'insertSpaces': &expandtab ? v:true : v:false},
      \ })
if has_key(s:format, 'result') && type(s:format.result) == type([])
  if !empty(s:format.result)
    call lsp#utils#text_edit#apply_text_edits(lsp#get_text_document_identifier().uri, s:format.result)
  endif
  let s:after_format = join(getline(1, '$'), "\n")
  let s:cells.formatting = {
        \ 'observed': v:true,
        \ 'edit_count': len(s:format.result),
        \ 'changed': s:after_format !=# s:before_format,
        \ 'resulting_text': s:after_format,
        \ }
else
  let s:cells.formatting = {'observed': v:false}
  call s:Fail('formatting response did not contain an edit list')
endif

silent write
silent bwipeout!
execute 'silent edit ' . fnameescape(s:workspace . '/main.pl')
let s:reopened = s:WaitFor("&l:filetype ==# 'perl'", 3000)
let s:cells.close_reopen = s:reopened
      \ && lsp#is_server_running('perllsp-under-test')
      \ && join(getline(1, '$'), "\n") =~# 'renamed_value'
if !s:cells.close_reopen
  call s:Fail('close/reopen did not preserve running session and persisted workspace edit')
endif

call lsp#stop_server('perllsp-under-test')
let s:stopped = s:WaitFor('g:perllsp_server_exit > 0 || !lsp#is_server_running("perllsp-under-test")', 7000)
let s:cells.shutdown = s:stopped && !lsp#is_server_running('perllsp-under-test')
if !s:cells.shutdown
  call s:Fail('vim-lsp did not stop perllsp cleanly')
endif

let s:receipt = {
      \ 'schema_version': 2,
      \ 'kind': 'vim_vim_lsp_actual_client',
      \ 'mode': s:mode,
      \ 'vim_version': execute('version'),
      \ 'vim_lsp_dir': s:vim_lsp_dir,
      \ 'perllsp': s:perllsp,
      \ 'workspace': s:workspace,
      \ 'activation_receipt': expand('$PERLLSP_VIM_ACTIVATION_RECEIPT'),
      \ 'workspace_config': {'perl': {'workspace': {'includePaths': [s:workspace . '/lib']}}},
      \ 'server_capabilities': s:server_capabilities,
      \ 'cells': s:cells,
      \ 'failures': s:failures,
      \ 'ok': empty(s:failures),
      \ }
call writefile([json_encode(s:receipt)], s:receipt_path)
if !empty(s:failures)
  cquit 2
endif
qa!
