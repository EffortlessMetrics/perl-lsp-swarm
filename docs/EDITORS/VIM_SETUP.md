# Vim Setup Guide for perl-lsp

This guide covers classic Vim setup, not Neovim's built-in LSP client. Use this
when you want `perllsp` features such as diagnostics, go-to-definition,
references, hover, formatting, and rename directly in Vim.

> [!NOTE]
> A working setup recipe is configuration evidence, not proof for every Vim
> version, build, operating system, or LSP client plugin. `Vim + vim-lsp` and
> `Vim + coc.nvim` are separate client subjects and are verified independently.

## Prerequisites

- Vim with filetype detection enabled
- `perllsp` installed and available on your `PATH`
- one LSP client plugin:
  - [`vim-lsp`](https://github.com/prabirshrestha/vim-lsp), or
  - [`coc.nvim`](https://github.com/neoclide/coc.nvim)

Client-specific notes:

- `vim-lsp` is the lightweight Vim-native path. Its upstream plugin runtime
  compatibility is broader than the set of Vim builds that perl-lsp has
  directly verified; do not treat the plugin's minimum Vim version as a
  perl-lsp support guarantee.
- current `coc.nvim` upstream requires Vim 9.0.0438+ and Node.js 20.19.0+.
  Those are Coc runtime prerequisites, not a statement that every such Vim
  build is independently proven with `perllsp`.

Verify `perllsp` before changing Vim configuration:

```bash
perllsp --version
perllsp --health
perllsp --info
```

## Perl Filetype Detection

The LSP client starts only when Vim sets the buffer filetype to `perl`.

Check a Perl buffer with:

```vim
:set filetype?
```

For an ordinary Perl source file it should print:

```text
filetype=perl
```

Prefer Vim's native detection before adding custom autocmds. Vim already has
non-trivial discrimination for Perl-related names: for example, `.pm` is also
used by XPM and `.t` is intentionally ambiguous between Perl tests, Nroff, and
TADS. A blanket rule such as `*.t setfiletype perl` can therefore steal valid
non-Perl files.

If a particular Perl-bearing file is not detected correctly, first confirm the
current buffer manually:

```vim
:setfiletype perl
```

Then add a persistent rule only when it is narrow enough for your project and
does not override an ambiguous file family globally. Do not force `.t`, `.cgi`,
or `.fcgi` to Perl solely from the extension.

## Option A: vim-lsp

Install `vim-lsp` with your preferred plugin manager, then add this to `.vimrc`
after the plugin is loaded.

```vim
function! s:perl_lsp_root_uri(server_info) abort
  let l:root = lsp#utils#find_nearest_parent_file_directory(
        \ expand('%:p'),
        \ ['.perl-lsp.toml', 'Makefile.PL', 'Build.PL', 'cpanfile', 'dist.ini', '.git']
        \ )

  if empty(l:root)
    let l:root = getcwd()
  endif

  return lsp#utils#path_to_uri(l:root)
endfunction

if executable('perllsp')
  augroup perllsp_vim_lsp
    autocmd!
    autocmd User lsp_setup call lsp#register_server({
          \ 'name': 'perl-lsp',
          \ 'cmd': {server_info -> ['perllsp', '--stdio']},
          \ 'allowlist': ['perl'],
          \ 'root_uri': function('s:perl_lsp_root_uri'),
          \ })
  augroup END
endif

function! s:on_perl_lsp_buffer_enabled() abort
  setlocal omnifunc=lsp#complete
  setlocal signcolumn=yes

  nmap <buffer> gd <plug>(lsp-definition)
  nmap <buffer> gr <plug>(lsp-references)
  nmap <buffer> K  <plug>(lsp-hover)
  nmap <buffer> <leader>rn <plug>(lsp-rename)
  nmap <buffer> <leader>ca <plug>(lsp-code-action)
endfunction

augroup perllsp_vim_lsp_mappings
  autocmd!
  autocmd User lsp_buffer_enabled
        \ if &filetype ==# 'perl' |
        \   call s:on_perl_lsp_buffer_enabled() |
        \ endif
augroup END
```

The root helper above chooses the nearest parent containing one of the Perl
project markers and falls back to Vim's current working directory when no
marker exists. If your project needs different root semantics, change them
intentionally rather than adding a second competing marker list elsewhere in
your config.

### Optional vim-lsp server settings

Prefer `.perl-lsp.toml` for project-wide settings shared across editors. For
Vim-specific settings, pass workspace configuration through `workspace_config`:

```vim
if executable('perllsp')
  augroup perllsp_vim_lsp
    autocmd!
    autocmd User lsp_setup call lsp#register_server({
          \ 'name': 'perl-lsp',
          \ 'cmd': {server_info -> ['perllsp', '--stdio']},
          \ 'allowlist': ['perl'],
          \ 'root_uri': function('s:perl_lsp_root_uri'),
          \ 'workspace_config': {
          \   'perl': {
          \     'workspace': {
          \       'includePaths': ['lib', '.', 'local/lib/perl5']
          \     },
          \     'inlayHints': {
          \       'enabled': v:true
          \     }
          \   }
          \ },
          \ })
  augroup END
endif
```

For inlay hints in Vim, you may also need a recent Vim 9 build and:

```vim
let g:lsp_inlay_hints_enabled = 1
```

Optional UI features such as inlay hints are separate from the baseline LSP
journey; a build without that UI does not automatically mean diagnostics,
completion, navigation, or editing are unsupported.

## Option B: coc.nvim

Current Coc upstream recommends several Vim-specific options before the LSP
configuration. Add the ones that fit your setup:

```vim
set encoding=utf-8
set nobackup
set nowritebackup
set updatetime=300
set signcolumn=yes
```

Install `coc.nvim`, then open Coc configuration:

```vim
:CocConfig
```

Add:

```json
{
  "languageserver": {
    "perl-lsp": {
      "command": "perllsp",
      "args": ["--stdio"],
      "filetypes": ["perl"],
      "rootPatterns": [
        ".perl-lsp.toml",
        "Makefile.PL",
        "Build.PL",
        "cpanfile",
        "dist.ini",
        ".git"
      ],
      "settings": {
        "perl": {
          "workspace": {
            "includePaths": ["lib", ".", "local/lib/perl5"]
          }
        }
      }
    }
  }
}
```

Coc uses Vim filetypes, not filename extensions. Check the active document with:

```vim
:CocCommand document.echoFiletype
```

It should report `perl` for an ordinary Perl buffer. If it does not, fix Vim's
filetype detection rather than adding a broad Coc extension map.

### Optional Coc mappings

`coc.nvim` does not install opinionated mappings for every user. Add mappings
only if they do not conflict with your existing Vim setup.

```vim
nmap <silent> gd <Plug>(coc-definition)
nmap <silent> gr <Plug>(coc-references)
nmap <silent> K  :call CocActionAsync('doHover')<CR>
nmap <leader>rn <Plug>(coc-rename)
nmap <leader>ca <Plug>(coc-codeaction)
```

Useful Vim-specific Coc commands include:

```vim
:CocDiagnostics
:CocInfo
:CocOpenLog
:CocCommand document.echoFiletype
:CocCommand workspace.showOutput
```

The separate [COC_NEOVIM_SETUP.md](./COC_NEOVIM_SETUP.md) contains additional
Coc material for Neovim. Do not copy Neovim-only keybindings or paths into Vim;
the language-server JSON model is shared, but the host behavior is verified
separately.

## Verify It Is Running

1. Open an ordinary Perl file such as `lib/My/Module.pm` or `script/app.pl`.
2. Run `:set filetype?` and confirm `filetype=perl`.
3. Introduce a temporary syntax error.
4. Confirm diagnostics appear in the selected client.
5. Try completion plus a hover, definition lookup, or references lookup.
6. Apply one edit and confirm subsequent diagnostics/navigation reflect the
   changed buffer rather than stale state.
7. Remove the syntax error after testing.

For `.t` and other ambiguous names, verify the detected filetype instead of
assuming the extension alone means Perl.

Useful commands:

```vim
" vim-lsp
:LspStatus
:LspDocumentDiagnostics

" coc.nvim
:CocInfo
:CocOpenLog
:CocCommand workspace.showOutput
```

## Troubleshooting

### Vim cannot find `perllsp`

Check from Vim:

```vim
:echo executable('perllsp')
```

It should print `1`.

Check from a shell:

```bash
command -v perllsp
perllsp --version
perllsp --health
perllsp --info
```

On Windows PowerShell:

```powershell
where perllsp
perllsp --version
perllsp --health
perllsp --info
```

If Vim was launched from a GUI, it may not inherit the same `PATH` as your
terminal. Use an absolute path in the LSP config if needed.

### Server starts but no Perl features appear

Check the filetype:

```vim
:set filetype?
```

If it is empty or wrong, set it manually for the current buffer while
diagnosing the problem:

```vim
:setfiletype perl
```

Before making that persistent, determine why Vim's native detector chose its
current result. Persistent rules should be project-specific or otherwise narrow
enough not to steal ambiguous `.t`, `.pm`, CGI, template, POD, or XS files.

### Diagnostics do not appear

For `vim-lsp`:

```vim
:LspStatus
:LspDocumentDiagnostics
```

For `coc.nvim`:

```vim
:CocInfo
:CocOpenLog
:CocCommand document.echoFiletype
:CocCommand workspace.showOutput
```

Also verify outside Vim:

```bash
perllsp --check path/to/file.pl
```

### Module resolution is wrong

Prefer project-level `.perl-lsp.toml` for shared include paths:

```toml
[perl]
include_paths = ["lib", "local/lib/perl5", "vendor/lib"]
```

Or pass client-specific settings through `vim-lsp` `workspace_config` or Coc
`settings`.

### `perllsp --stdio` appears to hang

That is expected. In stdio mode, `perllsp` waits for framed LSP JSON-RPC input
from the editor. Use these commands for manual checks instead:

```bash
perllsp --health
perllsp --info
perllsp --check path/to/file.pl
```

For server-side behavior and configuration details, see
[docs/reference/CONFIG.md](../reference/CONFIG.md) and
[docs/how-to/TROUBLESHOOTING.md](../how-to/TROUBLESHOOTING.md).
