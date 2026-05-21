# VS Code Extension Quality Burndown

> **Substrate (already built)**: VS Code extension manifest, Perl/Gherkin
> language contributions, TextMate grammars, debugger contribution, command
> palette actions, menus, keybindings, snippets, task definitions, walkthroughs,
> health/onboarding, binary downloader, managed install/update logic, Test
> Explorer integration, formatting, PerlCritic, refactoring, MCP support, Jest
> tests, integration-test scripts, Marketplace/Open VSX packaging, published
> extension smoke workflow, and release publish workflow.
>
> **Connector gap**: VS Code support is broad but not yet governed by a single
> quality contract. Install/update, activation, configuration sync, language
> features, debugger behavior, Test Explorer, command wiring, walkthroughs,
> packaging, Marketplace/Open VSX publishing, and published smoke all need
> stable receipts and regression ownership.
>
> **User-visible upside**: VS Code users get a reliable extension experience:
> install works, the right binary runs, settings map to server behavior, core
> LSP features respond, debugging/test/formatting flows stay healthy, published
> artifacts smoke cleanly, and regressions have one place to land.

## Lane assignment

- **Lane**: codex
- **Separation rule**: this rail stays isolated from LSP4IJ compatibility,
  Neovim interactive latency, and repo control-plane/proof-stack work.

## Supported VS Code surface (R0)

### Supported by default

- activation on Perl files
- managed perllsp install/update
- manual `serverPath`
- server startup / restart / health check
- completion
- hover
- diagnostics
- semantic tokens toggle
- formatting
- document symbols / outline
- go to definition/references
- PerlCritic command
- run tests in file / at cursor / all tests
- POD preview
- debug launch/attach
- status widget / output channel
- Marketplace and Open VSX publish
- published extension smoke

### Experimental/advisory

- AI completion
- MCP server definitions
- safe delete / package rename previews
- provider decision receipts
- workspace trust report
- advanced refactoring claims

## Requirements

### R1 — VS Code quality policy ledger

Create `policy/vscode-quality.toml` with the `vscode-quality` policy header,
surface entries, proof commands, receipt pointers, and review windows.

### R2 — Quality receipt schema

Create `.ci/receipts/schemas/vscode-quality.schema.json` for normalized VS Code
quality receipts (`pass|warn|fail|not_applicable`) and explicit claim boundary.

### R3 — Manifest contract validation

Add `cargo xtask vscode-quality manifest` (or Node-local `quality:manifest` as a
first step) to validate extension manifest integrity, command/menu wiring,
debugger contribution shape, settings metadata, and referenced resource paths.

### R4 — Managed install/update quality

Keep downloader/install behavior under named quality coverage for platform
resolution, checksum enforcement, secure transport, archive extraction, install
pointer semantics, prune behavior, co-install paths, and manual fallback paths.

### R5 — Extension-host smoke

Emit an extension-host smoke receipt lane from integration tests covering
activation, startup, core commands, LSP flows, semantic token toggle behavior,
format routing, and POD preview opening.

### R6 — Debugger quality

Add debugger quality receipts for launch/attach schema validity, command paths,
perl-dap resolution/failure handling, and key launch option mappings.

### R7 — Test Explorer quality

Add fixture-based receipt coverage for discovery/parsing and run-command paths
(`runTests`, `runCurrentTest`, `runTestAtCursor`, `runAllTests`).

### R8 — Settings sync quality

Validate key VS Code settings map to server-native behavior and initialization
options (include paths, diagnostics, semantic tokens, formatting, PerlCritic,
feature profiles, disabled features, trace).

### R9 — Published artifact quality

Standardize published smoke receipts for Marketplace and Open VSX per OS at:

- `target/receipts/vscode-smoke/marketplace-<os>.json`
- `target/receipts/vscode-smoke/open-vsx-<os>.json`

### R10 — Publish gate quality

Harden VSIX publish proof: version consistency checks, expected package content,
secret scanning, bundle-lsp assertions, and stable-release post-publish smoke
requirements.

### R11 — Coexistence/collision quality

Track inter-extension conflict risks (activation, diagnostics, formatter,
debugger type, grammars, command/menu clutter), starting with docs and promoting
to receipts as coverage matures.

### R12 — Support docs and troubleshooting

Add/harden VS Code extension support docs for install/update behavior,
`serverPath`, health checks, logs/output channel, PerlCritic/formatting/debug
setup, Test Explorer usage, and common failure modes.

## Status

| Phase | Issue | Builder-ready? | PR | Receipt |
|---|---|---:|---|---|
| 1. Rail doc + index row | file after doc PR | yes | — | `git diff --check` |
| 2. VS Code quality policy ledger | file after phase 1 | yes | — | `policy/vscode-quality.toml parses` |
| 3. Manifest quality checker | file after phase 1 | yes | — | `vscode-quality manifest` |
| 4. Downloader/install receipt hardening | file after phase 1 | yes | — | Jest downloader receipt |
| 5. Extension-host smoke receipt | file after phase 1 | yes | — | `npm run test:integration` |
| 6. Debugger quality receipt | file after phase 1 | yes | — | debugger fixture receipt |
| 7. Test Explorer quality receipt | file after phase 1 | yes | — | test fixture receipt |
| 8. Settings sync quality receipt | file after phase 1 | yes | — | settings sync fixture |
| 9. Published smoke receipt schema | file after phase 1 | yes | — | `target/receipts/vscode-smoke/*.json` |
| 10. Publish gate hardening | file after phase 1 | yes | — | VSIX manifest/package receipt |
| 11. Coexistence/troubleshooting docs | file after phase 1 | yes | — | docs receipt |
| 12. Advisory CI lane | file after receipts | yes | — | workflow artifact |
| 13. Narrow blocking promotion | after burn-in | no | — | policy update |

## PR sequence

1. **PR 1** — docs only (`docs(vscode): add VS Code quality rollout rail`):
   `docs/development/VS_CODE_QUALITY_ROLLOUT.md` + `docs/project/RAILS_INDEX.md`.
2. **PR 2** — policy ledger (`policy(vscode): add VS Code quality surface ledger`):
   `policy/vscode-quality.toml` + `docs/status/VS_CODE_EXTENSION_QUALITY.md`.
3. **PR 3** — manifest checker (`xtask: validate VS Code extension manifest`).
4. **PR 4** — downloader/install receipt hardening.
5. **PR 5** — extension-host smoke receipt.
6. **PR 6** — debugger quality receipt.
7. **PR 7** — Test Explorer receipt.
8. **PR 8** — settings sync receipt.
9. **PR 9** — published smoke receipt schema.
10. **PR 10** — publish gate hardening.
11. **PR 11** — support docs/troubleshooting.
12. **PR 12** — advisory CI lane.
13. **PR 13** — narrow blocking promotion after burn-in.

## Exit criteria

Rail closes when policy ledger, manifest checker, downloader/install receipt,
extension-host smoke receipt, debugger receipt, Test Explorer receipt, settings
sync receipt, published smoke schema, VSIX/package proof, support docs,
advisory quality lane, and post-burn-in blocking promotion are all in place and
consumed (or referenced) by normal release flow.

## Claim boundary

### Proves

- VS Code extension quality has a repo-owned contract.
- Manifest surfaces receive explicit validation.
- Managed binary install/update remains covered.
- Activation/startup paths have named smoke receipts.
- Debugger/test/settings/publish flows emit named receipts.
- Published Marketplace/Open VSX artifacts are smoke-tested.

### Does not prove

- all VS Code/VSCodium/Open VSX client variants
- every OS/arch install path
- every Perl project layout
- all debugger scenarios
- all Marketplace propagation behavior
- LSP4IJ compatibility
- Neovim latency
- true incremental parsing

## Do not combine

This rail must not be combined with:

- LSP4IJ template submission / compatibility support
- Neovim interactive latency work
- incremental parser architecture work
- PR comments/gate control-plane changes
- ripr/tokmd changes
- Clippy/Codecov/file-policy/release-prep rails
- parser grammar changes

It may reference release workflows, but it is not a general release-readiness
umbrella.

## Rollback

If quality lane signal is noisy or flaky, rollback by demoting new VS Code checks
to advisory-only receipts while preserving evidence artifacts for debugging.
