# LSP4IJ Compatibility and Support Burndown

> **Substrate (already built)**: `perllsp` public binary facade, `perl-dap`
> binary, GitHub release downloader logic in the VS Code extension, server-native
> configuration schema, DAP implementation, LSP4IJ user-defined template support,
> LSP4IJ installer support, and LSP4IJ DAP template support.
>
> **Connector gap**: submitting a JSON template is not enough. We need a durable
> compatibility contract that proves perl-lsp installs, starts, configures, and
> behaves correctly under LSP4IJ across OSes, workspace strategies, settings,
> diagnostics, semantic tokens, formatting, and DAP.
>
> **User-visible upside**: JetBrains users can install LSP4IJ, open a Perl file,
> accept the perl-lsp template, and get reliable Perl language support with clear
> docs, tested settings, sane workspace behavior, and a known support boundary.

## Why this rail exists

The upstream submission-prep rail answers:

```text
What files do we need to contribute upstream?
```

This compatibility rail answers:

```text
How do we know the integration keeps working?
What do we support?
What do we test before changing release assets/settings/protocol behavior?
What does a JetBrains user do when something breaks?
```

LSP4IJ contribution and installer mechanics mean the template files are necessary but
not sufficient. This rail exists to carry a compatibility/support matrix, receipts,
and support boundary that survive beyond the initial upstream submission.

## Lane assignment

- **Rail lane:** `codex`
- **Open phases:** `12` (starting with Phase 1 documentation/contract)
- **Builder-ready now:** Phase 1 only (doc + index entry)
- **Separation rule:** do not combine with the upstream LSP4IJ submission-prep rail.

## Requirements

### R0 — Integration contract

Define what “supported under LSP4IJ” means.

Minimum supported path:

```text
Install LSP4IJ
Open Perl file
Create Perl Language Server from template
Install perllsp from GitHub release
Initialize server
Open .pl/.pm/.t
Receive diagnostics
Use completion
Use hover
Use document symbols
Use formatting if enabled
```

Optional / staged:

```text
DAP debugging
semantic tokens
inlay hints
workspace symbols
lazy workspace folders
external Perl::Critic
large repo performance
```

### R1 — Template fixture lives in our repo

Before upstreaming to LSP4IJ, keep a local staging copy:

```text
tools/lsp4ij/templates/lsp/perl-lsp/
  template.json
  installer.json
  settings.json
  settings.schema.json
  initializationOptions.json
  README.md
```

This is our source fixture. The upstream PR copies from here.

### R2 — Release asset compatibility

Validate that release assets work with LSP4IJ installer selection and extraction.

Required asset names (or documented aliases):

```text
perllsp-x86_64-unknown-linux-gnu.tar.gz
perllsp-aarch64-unknown-linux-gnu.tar.gz
perllsp-x86_64-unknown-linux-musl.tar.gz
perllsp-aarch64-unknown-linux-musl.tar.gz
perllsp-x86_64-apple-darwin.tar.gz
perllsp-aarch64-apple-darwin.tar.gz
perllsp-x86_64-pc-windows-msvc.zip
perllsp-aarch64-pc-windows-msvc.zip
SHA256SUMS
```

Archive payload expectation:

```text
perllsp / perllsp.exe
perl-dap / perl-dap.exe      # optional for initial LSP submission, required for DAP follow-up
```

### R3 — Server-native settings/schema contract

`settings.json` for LSP4IJ must use server-native settings shape, not VS Code extension keys.
Validate against server configuration parsing contract (workspace include paths,
`@INC` toggles, `PERL5LIB`, formatting, critic/perlcritic, inlay hints).

### R4 — Workspace strategy

Document and test LSP4IJ workspace behavior across:

```text
PROJECT_BASE eager mode
PROJECT_BASE lazy mode
marker-based workspace folders
single-file/no-root behavior
large repo behavior
```

Default recommendation target: lazy workspace folders for large projects (if template supports cleanly).

### R5 — File mapping scope

Initial conservative mapping:

```text
*.pl
*.PL
*.pm
*.t
*.psgi
*.cgi
*.fcgi
*.pod
*.xs
*.xsi
Makefile.PL
Build.PL
cpanfile
dist.ini
```

Deferred until validated:

```text
*.tt
*.tt2
*.ep
*.mason
*.mas
*.i
```

### R6 — LSP protocol compatibility smoke

Receipt matrix minimum:

```text
initialize
initialized
didOpen
didChange
completion
hover
definition
documentSymbol
publish diagnostics or pull diagnostics
formatting
shutdown
```

Track additional behavior:

```text
semantic tokens full/range/delta behavior
workspace/configuration behavior
workspace folders behavior
file watcher behavior
```

### R7 — DAP support follow-up

Stage DAP after LSP fixture stabilizes:

```text
tools/lsp4ij/templates/dap/perl-dap/
  template.json
  installer.json
  README.md
```

DAP smoke targets:

```text
create DAP server
launch perl-dap over stdio
set breakpoint
launch script
stop on entry
continue
variables
stdout/stderr
shutdown
```

### R8 — Manual validation receipt

Add stable receipt artifacts:

```text
docs/status/LSP4IJ_COMPATIBILITY.md
target/receipts/lsp4ij/manual-validation.md
```

Required receipt sections:

```text
LSP4IJ version
IntelliJ product/version
OS/arch
perl-lsp version
template source
release asset
install result
initialize result
feature matrix
logs/artifacts
known gaps
claim boundary
```

### R9 — Automated checks where cheap

Planned commands:

```bash
cargo xtask lsp4ij-template validate
cargo xtask lsp4ij-template render-upstream
cargo xtask lsp4ij-template receipt
```

Validation coverage:

```text
JSON parses
template id is stable
installer asset names match release contract
settings keys match server-native schema
file mappings are expected
README exists
DAP template is either absent or complete
```

### R10 — Support docs

Add docs:

```text
docs/reference/LSP4IJ.md
docs/status/LSP4IJ_COMPATIBILITY.md
```

Coverage targets:

```text
Install LSP4IJ
Use perl-lsp template
Manual binary path
Managed installer path
Workspace folder recommendations
Formatting
Diagnostics
DAP status
Troubleshooting
Known limitations
How to report bugs
```

## PR sequence

1. **PR 1 — rail doc only** (`docs(lsp4ij): add compatibility and support rollout rail`)
   - Files: `docs/development/LSP4IJ_COMPATIBILITY_AND_SUPPORT_ROLLOUT.md`, `docs/project/RAILS_INDEX.md`
   - Proof: `git diff --check`
2. **PR 2 — local template fixture** (`tools(lsp4ij): add local Perl LSP template fixture`)
3. **PR 3 — template validator** (`xtask: add LSP4IJ template validator`)
4. **PR 4 — release asset contract** (`release: document LSP4IJ asset contract`)
5. **PR 5 — LSP4IJ support docs** (`docs(lsp4ij): add JetBrains/LSP4IJ setup guide`)
6. **PR 6 — manual validation receipt** (`docs(lsp4ij): add compatibility validation receipt`)
7. **PR 7 — upstream render command** (`xtask: render LSP4IJ upstream template payload`)
8. **PR 8 — raw LSP4IJ-compatible protocol smoke receipt** (`ux: add LSP4IJ-compatible raw LSP smoke receipt`)
9. **PR 9 — DAP fixture** (`tools(lsp4ij): add local Perl DAP template fixture`)
10. **PR 10 — DAP smoke receipt** (`ux: add LSP4IJ-compatible DAP smoke receipt`)
11. **PR 11 — manual IntelliJ/LSP4IJ validation** (`docs(lsp4ij): record manual compatibility receipt`)
12. **PR 12 — upstream PR playbook** (`docs(lsp4ij): add upstream PR playbook`)

Each PR remains one semantic change; no upstream LSP4IJ PR is opened from this rail.

## LSP fixture plan

- Maintain LSP fixture at `tools/lsp4ij/templates/lsp/perl-lsp/`.
- Treat this fixture as canonical source for upstream copy.
- Keep server-native settings/schema in fixture and validate locally before any upstream sync.

## DAP fixture plan

- Maintain staged DAP fixture at `tools/lsp4ij/templates/dap/perl-dap/`.
- Do not bundle DAP into first LSP submission unless manually validated.
- Advance DAP only after LSP rail receipts are present.

## Release asset contract

- Maintain cross-platform asset naming contract and payload expectations.
- Add doc + optional validator to ensure tag artifacts remain installable from LSP4IJ descriptors.

## Settings/schema contract

- Enforce server-native settings hierarchy and schema compatibility.
- Explicitly ban substituting VS Code extension-only keys in LSP4IJ settings payloads.

## Workspace strategy

- Validate eager/lazy/marker/no-root paths and publish a recommendation.
- Prefer lazy workspace folders for large projects where behavior remains stable.

## Manual validation receipt

- Record reproducible manual validation in `docs/status` and receipt artifact under `target/receipts/lsp4ij/`.
- Include versions, OS/arch, template source, asset, feature outcomes, and known gaps.

## Automated validation plan

- Add low-cost `xtask` validators (template structure, schema keys, asset mapping, render, receipt generation).
- Use raw protocol smoke tests before any attempt at IntelliJ automation.

## Claim boundary

This rail proves:

```text
perl-lsp has a maintained LSP4IJ compatibility contract
template files are generated/validated locally
release assets are compatible with LSP4IJ installers
server-native settings are documented and validated
manual receipts exist for JetBrains behavior
DAP support is staged instead of bundled accidentally
```

This rail does not prove:

```text
upstream LSP4IJ PR acceptance
all JetBrains IDE products
all OS/arch installer paths
all Perl project layouts
true IntelliJ headless automation
Neovim latency fixes
incremental parser behavior
```

## Do not combine

Do not combine this rail with:

```text
upstream LSP4IJ submission PR
Neovim latency rail
incremental parsing rail
PR comments/gate control plane
ripr/tokmd
CodeCov/Clippy/file-policy/release prep
parser grammar changes
```

The upstream submission rail may consume artifacts from this rail, but submission PRs remain small and boring.
