# Zed Compatibility and Support Burndown

> **Substrate (already built)**: `perllsp` public binary, GitHub release assets,
> Zed setup guide, server-native configuration, LSP feature surface, DAP binary,
> and existing public Zed Perl extension infrastructure from the tree-sitter-perl
> ecosystem.
>
> **Connector gap**: Zed support is documented but not yet productized. The
> public Zed Perl extension currently registers `perlnavigator-server`, not
> `perllsp`, so users cannot simply install Zed’s Perl extension and get
> perl-lsp behavior. We need a maintained extension strategy, template/config
> fixture, manual validation receipts, and upstream coordination path.
>
> **User-visible upside**: Zed users can install a Perl language extension,
> open `.pl` / `.pm` / `.t` files, run `perllsp --stdio`, receive diagnostics,
> completion, hover, symbols, formatting where supported, and have clear docs
> when Zed extension support is limited or experimental.

## Current state

### What we have

The repository already has `docs/EDITORS/ZED_SETUP.md`, including the key constraint: Zed's `lsp` block configures known language server IDs and does not register new server IDs by itself. The installed extension must register `perl-lsp` for Perl.

It also already documents:

- `perllsp --version` / `--health` / `--info` checks.
- `lsp.perl-lsp.binary.path` override usage.
- optional file associations.
- initialization options and semantic token toggles.
- basic troubleshooting flow.

### What Zed expects

Zed extensions ship as repositories centered on `extension.toml`, optionally backed by Rust/Wasm extension code (`zed_extension_api`). Local dev extensions are supported through "Install Dev Extension", and troubleshooting should use `Zed.log` or `zed --foreground`.

Publishing requires the extension repo to be represented in `zed-industries/extensions` and listed in top-level `extensions.toml`, with registry/licensing expectations met.

### Current upstream Perl extension shape

`tree-sitter-perl/zed-perl` currently registers `perlnavigator-server` ("Perl Navigator"), not `perllsp`, and launches `perlnavigator-server --stdio` via PATH/npm install logic. Its language config currently targets `pl`, `pm`, and `t`.

## Strategic decision ladder

| Option | Description | Default posture |
|---|---|---|
| A. Contribute `perllsp` support to `tree-sitter-perl/zed-perl` | Add `perl-lsp` server (or coordinated default switch) | Best ecosystem posture |
| B. Maintain local/dev extension fixture in this repo | Use it for repeatable validation receipts and docs | Best near-term proof |
| C. Publish separate Zed extension | Submit standalone marketplace extension | Only if A stalls |

Decision policy: build local fixture first, coordinate with upstream extension maintainers, decide publish posture only after coordination evidence.

## Requirements

### R0 — Zed support contract

**Supported first**

- local/dev Zed extension loads.
- Perl files recognized.
- `perllsp` starts over stdio.
- `didOpen` / `didChange` flow works.
- diagnostics, hover, completion, document symbols, definitions (where server supports).
- server-native settings pass-through works.
- absolute binary path and PATH lookup path both work.

**Advisory first**

- semantic tokens.
- formatting/range formatting.
- workspace symbols.
- test running.
- DAP/debugging.
- large workspace performance.
- automatic download/install of `perllsp`.
- published extension registry support.

### R1 — local extension fixture

Create `tools/zed/perl-lsp-zed/` with:

- `extension.toml`
- `Cargo.toml`
- `src/lib.rs`
- `languages/perl/config.toml`
- `README.md`

### R2 — extension manifest

Fixture manifest registers `perl-lsp` language server with Perl grammar/source metadata and pinned grammar commit.

### R3 — language config scope

Initial scope:

- shebang detection for Perl.
- suffixes: `pl`, `PL`, `pm`, `t`, `psgi`, `cgi`, `fcgi`, `pod`.
- line comment `#`.

Template-family suffixes (`tt`, `tt2`, `ep`, `mason`, `mas`, `i`) deferred pending receipts.

### R4 — extension code launch paths

Required launch paths:

1. Zed-configured binary path.
2. PATH lookup via `worktree.which("perllsp")`.

Deferred: release asset download + extension storage install flow.

### R5 — server-native settings

Use nested Zed `lsp` object settings for `binary`, `settings`, and `initialization_options`; do not use VS Code dotted-key style.

### R6 — semantic tokens and formatting

Document and validate advisory behavior for:

- `languages.Perl.semantic_tokens = "combined"` / `"full"` (as supported).
- `languages.Perl.formatter = "language_server"`.
- `languages.Perl.format_on_save` behavior.

### R7 — Zed receipts

Add receipts at:

- `docs/status/ZED_COMPATIBILITY.md`
- `target/receipts/zed/manual-validation.md`
- `target/receipts/zed/lsp-smoke.json`

### R8 — raw protocol Zed-like smoke

Add `perl-lsp-ux-tests` scenario with Zed-like capabilities:

- initialize.
- didOpen + diagnostics path.
- completion / hover / documentSymbol / definition.
- formatting + semantic tokens (if enabled).
- shutdown.

### R9 — extension validator command

Add:

- `cargo xtask zed validate-extension --path tools/zed/perl-lsp-zed`

Checks include parse/shape, required files, server id, suffix policy alignment, README/license posture.

### R10 — upstream coordination playbook

Add `docs/handoffs/ZED_UPSTREAM_PLAYBOOK.md` documenting ownership reality, preferred contribution path, fallback path, and Zed registry mechanics.

### R11 — DAP boundary

Defer debugger work to a separate rail (`ZED_DAP_SUPPORT_ROLLOUT.md`) unless explicitly scoped later.

### R12 — publish posture closeout

Close phase one with explicit decision: upstream contribution, separate extension, or docs-only hold.

## PR sequence

1. `docs(zed): add Zed compatibility/support rollout rail` (this PR).
2. `docs(zed): align Zed setup guide with support contract`.
3. `tools(zed): add local perl-lsp Zed extension fixture`.
4. `xtask: add Zed extension fixture validator`.
5. `ux: add Zed-like raw LSP compatibility receipt`.
6. `docs(zed): document server-native Zed settings`.
7. `docs(zed): add manual validation receipt template`.
8. `docs(zed): define perllsp download strategy for Zed extension`.
9. `docs(zed): add upstream coordination playbook`.
10. `docs(zed): draft tree-sitter-perl zed-perl coordination note`.
11. `docs(zed): record first dev-extension validation receipt`.
12. `docs(zed): close out Zed compatibility rail phase one`.

Each PR must include proof command(s), claim boundary, and rollback note.

## Exit criteria

- rail doc exists and is indexed.
- setup docs clearly state current public extension limitation.
- local fixture exists and registers `perl-lsp`.
- fixture launches `perllsp --stdio` via PATH or configured path.
- language suffix policy captured.
- validator command exists.
- raw Zed-like LSP receipt exists.
- manual validation template exists.
- server-native settings examples exist.
- upstream coordination playbook exists.
- DAP is explicitly deferred/scoped separately.
- phase-one closeout decision recorded.

## Claim boundary

**Proves**: maintained Zed support contract, explicit current extension gap, local fixture plan and receipts plan, Zed-like protocol smoke plan, and upstream coordination path.

**Does not prove**: Zed marketplace acceptance, maintainer agreement, all versions/platforms, full downloader maturity, DAP/debugger support, semantic-token quality outcomes, Neovim latency, LSP4IJ, or VS Code quality.

## Do not combine

Keep this rail separate from:

- LSP4IJ submission and LSP4IJ compatibility/support.
- VS Code quality.
- Neovim latency/performance.
- parser architecture/grammar changes.
- PR comments/gate control-plane work.
- ripr/tokmd.
- clippy/codecov/file-policy/release-prep ladders.

## Lane assignment

- **Lane**: `codex`.
- **Open phases**: `12`.
- **Next action**: Phase 1 doc/status contract and indexed rail presence.
