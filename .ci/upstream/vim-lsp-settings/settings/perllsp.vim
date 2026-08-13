" Candidate upstream file for mattn/vim-lsp-settings.
" Authority: #7712. Do not treat this checked-in candidate as upstream presence.
call lsp_settings#register_server({
    \ 'name': 'perllsp',
    \ 'cmd': {server_info->lsp_settings#get('perllsp', 'cmd', [lsp_settings#exec_path('perllsp')]+lsp_settings#get('perllsp', 'args', ['--stdio']))},
    \ 'root_uri':{server_info->lsp_settings#get('perllsp', 'root_uri', lsp_settings#root_uri('perllsp'))},
    \ 'initialization_options': lsp_settings#get('perllsp', 'initialization_options', v:null),
    \ 'allowlist': lsp_settings#get('perllsp', 'allowlist', ['perl']),
    \ 'blocklist': lsp_settings#get('perllsp', 'blocklist', []),
    \ 'config': lsp_settings#get('perllsp', 'config', lsp_settings#server_config('perllsp')),
    \ 'workspace_config': lsp_settings#get('perllsp', 'workspace_config', {}),
    \ 'semantic_highlight': lsp_settings#get('perllsp', 'semantic_highlight', {}),
    \ })
