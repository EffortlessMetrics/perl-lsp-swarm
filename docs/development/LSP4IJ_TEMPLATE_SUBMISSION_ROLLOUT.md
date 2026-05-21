# LSP4IJ Template Submission Rollout

Umbrella: **#0000** (placeholder until orchestrator files the tracking issue).

This rail is for **submission-readiness of LSP4IJ templates** and is explicitly
separate from Neovim latency rails or PR-comment/CI-gate rails.

## Goal

Contribute `perl-lsp` to LSP4IJ as a built-in **user-defined language server
template** so a user can install LSP4IJ, open a Perl file, and be prompted to
install/configure Perl language support from an official template.

## Submission scope

| Stage | Scope | Why |
|---|---|---|
| PR 1 | LSP template only | Smallest viable scope aligned with Angelo's ask. |
| PR 2 | DAP template follow-up | Valuable but should not block LSP template landing. |

## Rollout ladder

### R0 — Executable contract

Define and document stable commands used by template and installer flows:

- `perllsp --stdio`
- `perllsp --health`
- `perllsp --info`
- `perllsp --version`

For DAP follow-up:

- `perl-dap`
- `perl-dap --stdio`
- `perl-dap --socket --port 13603`
- `perl-dap --help`
- `perl-dap --version`

### R1 — Release asset naming contract

Adopt/verify non-versioned release asset names suitable for direct LSP4IJ
installer mapping:

- `perllsp-x86_64-unknown-linux-gnu.tar.gz`
- `perllsp-aarch64-unknown-linux-gnu.tar.gz`
- `perllsp-x86_64-unknown-linux-musl.tar.gz`
- `perllsp-aarch64-unknown-linux-musl.tar.gz`
- `perllsp-x86_64-apple-darwin.tar.gz`
- `perllsp-aarch64-apple-darwin.tar.gz`
- `perllsp-x86_64-pc-windows-msvc.zip`
- `perllsp-aarch64-pc-windows-msvc.zip`
- `SHA256SUMS`

Archive contents contract:

- required: `perllsp` / `perllsp.exe`
- recommended: `perl-dap` / `perl-dap.exe`

### R2 — LSP template descriptor plan

Target LSP4IJ path:

- `src/main/resources/templates/lsp/perl-lsp/template.json`

Template requirements:

- unique `id`: `perl-lsp`
- correct `programArgs` (`perllsp --stdio`)
- conservative Perl file mappings for first submission
- avoid broad embedded-template mappings in PR 1 (`.tt`, `.tt2`, `.ep`,
  `.mason`, `.mas`, `.i`)

### R3 — Installer descriptor plan

Target LSP4IJ path:

- `src/main/resources/templates/lsp/perl-lsp/installer.json`

Installer requirements:

- GitHub release download mapping by OS/architecture
- post-download server command configuration
- command health check strategy validated against LSP4IJ variable expansion
  behavior

### R4 — Server-native settings/schema plan

Target LSP4IJ paths:

- `src/main/resources/templates/lsp/perl-lsp/settings.json`
- `src/main/resources/templates/lsp/perl-lsp/settings.schema.json`
- `src/main/resources/templates/lsp/perl-lsp/initializationOptions.json`

Settings contract:

- `settings.json` rooted at `perl` object expected by server parser
- initial `initializationOptions` limited to:
  - `{"disabledFeatures": []}`
- avoid VS Code-extension-specific setting shape in LSP4IJ template defaults

### R5 — File mapping scope

Initial mapping list for first submission:

- `*.pl`, `*.PL`, `*.pm`, `*.t`, `*.psgi`, `*.cgi`, `*.fcgi`, `*.pod`, `*.xs`,
  `*.xsi`
- `Makefile.PL`, `Build.PL`, `cpanfile`, `dist.ini`

Open item to validate in LSP4IJ custom template import:

- plain filename pattern handling (without wildcard prefix)

### R6 — LSP4IJ docs page plan

LSP4IJ-side files expected in submission PR:

- `src/main/resources/templates/lsp/perl-lsp/README.md`
- `docs/user-defined-ls/perl-lsp.md`
- update `docs/UserDefinedLanguageServer.md` default-template list

### R7 — Local custom-template validation receipt

Generate local receipts before upstream submission:

- `target/receipts/lsp4ij-template/perl-lsp-template.json`
- `target/receipts/lsp4ij-template/manual-validation.md`

Manual validation matrix must cover:

- import custom template
- installer download/configure flow
- `.pl` open/startup
- initialize success in LSP console
- completion, hover, diagnostics, formatting
- `.pm` symbols
- large repo behavior with lazy workspace folder mode
- Windows and Unix/macOS command forms

### R8 — DAP follow-up descriptor

Second PR target (after LSP template lands):

- `src/main/resources/templates/dap/perl-dap/template.json`
- `src/main/resources/templates/dap/perl-dap/installer.json`
- `src/main/resources/templates/dap/perl-dap/README.md`
- `docs/dap/user-defined-dap/perl-dap.md`

## Lane assignment

- **Lane**: `codex`
- **Open phases**: `8` (R0-R8; R8 is DAP follow-up)

## Claim boundary

This rail proves planning scope, descriptor contracts, and validation
expectations for LSP4IJ submission-readiness.

This rail does **not** prove:

- that release automation already emits the exact non-versioned assets
- that LSP4IJ template files are already merged upstream
- that DAP template support ships in PR 1

## Do-not-combine

For PR 1 (this rail doc + index entry):

- do not include runtime/server code changes
- do not include unrelated docs cleanup
- do not include Neovim latency rail updates
- do not include LSP4IJ upstream template payload files yet

## Immediate execution plan (after this PR)

1. Add/verify release asset contract for `perllsp` and `perl-dap`.
2. Stage LSP4IJ template JSON files locally in this repo for validation.
3. Validate template JSON and command behavior with custom-template import.
4. Add local user-facing docs for template install/usage guidance.
5. Open upstream LSP4IJ PR with LSP template files and docs.
6. Open DAP follow-up PR after LSP template validation/landing.
