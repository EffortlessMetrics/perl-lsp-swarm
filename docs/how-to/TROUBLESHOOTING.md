# Troubleshooting Guide

Use this page when `perllsp` is installed but something still does not work:
the binary is not found, the server does not start, diagnostics are missing, or
the editor feels slow.

## Start With The Basics

```bash
perllsp --version
perllsp --health
perllsp --info
```

If those fail, fix the binary installation and `PATH` first. If they pass, the
problem is usually in editor integration, workspace roots, or a stale cache.

## The Server Will Not Start

1. Run the server in the foreground:

   ```bash
   perllsp --stdio
   ```

2. Turn on logging and read stderr:

   ```bash
   perllsp --log --stdio
   ```

   Or use `RUST_LOG` for finer-grained control:

   ```bash
   RUST_LOG=perl_lsp=debug perllsp --stdio
   ```

3. Check the editor's LSP log panel or buffer.

## Sublime Text Does Not Start `perllsp`

1. Confirm `perllsp` works outside Sublime:

   ```bash
   perllsp --version
   perllsp --health
   perllsp --info
   ```

2. Confirm the `LSP` package is installed.

3. Confirm `Preferences: LSP Server Configurations` contains:

   ```json
   {
     "perl-lsp": {
       "enabled": true,
       "command": ["perllsp", "--stdio"],
       "selector": "source.perl"
     }
   }
   ```

4. Run `Tools > Developer > Show Scope Name` in a Perl file and confirm the root
   scope matches the configured selector.

5. Run `LSP: Troubleshoot Server` and `LSP: Toggle Log Panel`.

6. If Sublime cannot find `perllsp`, use an absolute path in `command`.

## The Editor Connects, But Nothing Happens

- Confirm the file type is Perl.
- Confirm the workspace root is the repository root, not a parent directory.
- Confirm the editor command really starts `perllsp --stdio`.

If the editor is using a helper extension or plugin, check its own logs too.

## Diagnostics Or Completions Are Missing

- Re-check the install with `perllsp --health`.
- Make sure the file is inside the indexed workspace.
- Restart the editor after changing language-server settings.
- If the project is large, try a smaller workspace root first.

## The Server Feels Slow

- Close unrelated files and trim the workspace to the project root.
- Disable any editor-side preview features that trigger extra refreshes.
- Compare behavior with a fresh shell session so stale environment state does
  not hide the problem.

## Module Resolution Problems

- Confirm the module lives under the workspace or configured include paths.
- Open the project root that contains the module tree, not just a subdirectory.
- If you are using vendored or local libraries, make sure the editor config
  points at them explicitly.
- For PL701 or `@INC` mismatches, run **Perl LSP: Explain Missing Module
  Lookup** and **Perl LSP: Show Workspace Trust Report**. See
  [Perl Setup Troubleshooting](PERL_SETUP_TROUBLESHOOTING.md) for the full
  setup checklist.

## Formatting Or Code Actions Are Missing

- Verify the editor has the relevant capability enabled.
- Check whether the current file actually has a Perl mode or file type.
- Inspect the LSP log for capability negotiation or request errors.

## Neovim Does Not Start `perllsp`

1. Confirm the binary works outside Neovim:

   ```bash
   perllsp --version
   perllsp --health
   perllsp --info
   ```

2. Confirm the buffer filetype in Neovim:

   ```vim
   :set filetype?
   ```

   It must be `perl`.

3. Confirm the LSP config is enabled:

   ```vim
   :checkhealth vim.lsp
   ```

4. For Neovim 0.11+, make sure config name and enable name match:

   ```lua
   vim.lsp.config('perllsp', { cmd = { 'perllsp', '--stdio' } })
   vim.lsp.enable('perllsp')
   ```

5. Use `perllsp --check path/to/file.pl` for manual diagnostics.
   Do not test stdio mode by piping unframed JSON.

## coc.nvim Does Not Start `perllsp`

1. Confirm coc.nvim is running:

   ```vim
   :CocInfo
   ```

2. Confirm the filetype:

   ```vim
   :set filetype?
   :CocCommand document.echoFiletype
   ```

   It must be `perl`.

3. Confirm `perllsp` works outside Neovim:

   ```bash
   perllsp --version
   perllsp --health
   perllsp --info
   perllsp --check path/to/file.pl
   ```

4. Inspect logs:

   ```vim
   :CocOpenLog
   :CocCommand workspace.showOutput
   ```

5. If `perllsp` is not found, use an absolute
   `languageserver.perl-lsp.command` path.

6. Do not test stdio mode with raw JSON. LSP stdio traffic requires
   `Content-Length` framing.

## DAP Or Debugging Issues

If you are debugging with `perl-dap`, check the DAP guide:
[DAP_USER_GUIDE.md](../tutorials/DAP_USER_GUIDE.md).

## Zed Does Not Start `perllsp`

This is currently an integration-availability problem, not an ordinary binary
troubleshooting path. The public Perl extension does not register `perllsp`, so
installing `perllsp` alone cannot make Zed start it.

The extension's existing IDs belong to other products:

```text
perlnavigator-server -> Perl Navigator
perl-lsp             -> tree-sitter-perl/perl-tree-sitter-lsp
```

Do not override the existing `perl-lsp` ID to run `perllsp`. Doing so hides the
winning product identity and invalidates support evidence.

For contributors preparing the upstream change, use the exact-base candidate
and apply script documented in
[ZED_UPSTREAM_SUBMISSION.md](../integrations/ZED_UPSTREAM_SUBMISSION.md). That
candidate remains `not_proven` until an actual Zed development-extension receipt
binds the host, extension, binary, platform, requests, edits, and shutdown.

Public-user runtime troubleshooting will be added only after a released Perl
extension registers `perllsp` and a public-artifact host receipt exists.

## When To Escalate

Report an issue when you can include:

- `perllsp --version`
- `perllsp --health`
- **Perl LSP: Show Workspace Trust Report** output for setup or module-path
  problems
- the editor name and version
- the workspace layout
- the smallest code sample that reproduces the problem

Open issues at [GitHub Issues](https://github.com/EffortlessMetrics/perl-lsp/issues).
