# Perl LSP for Claude Code

This `perl-lsp-rs` plugin connects Claude Code's native LSP integration to the separately installed `perllsp` server from EffortlessMetrics.

It is an LSP package only. It does not bundle a language-server binary, use MCP, or proxy another Perl language server.

## Install `perllsp`

Install the server through a supported first-party channel and verify the exact binary Claude will see on `PATH`:

```bash
cargo binstall perllsp
perllsp --version
perllsp --health
```

If `cargo-binstall` is not available:

```bash
cargo install perllsp
```

Release archives are also available from the public `EffortlessMetrics/perl-lsp` repository for supported targets.

## Install the plugin

### Local dogfood from this repository

From a checkout containing the root `.claude-plugin/marketplace.json`:

```bash
claude plugin marketplace add . --scope local
claude plugin install perl-lsp-rs@effortlessmetrics --scope local
```

### Public marketplace

After this package has been promoted and proven from `EffortlessMetrics/perl-lsp`:

```bash
claude plugin marketplace add EffortlessMetrics/perl-lsp
claude plugin install perl-lsp-rs@effortlessmetrics
```

The public route is a support claim only after the repository's public-artifact Claude receipt is current. The package itself does not silently download or replace `perllsp`.

## Launch contract

The plugin launches exactly:

```text
perllsp --stdio
```

and binds the language server workspace to:

```text
${CLAUDE_PROJECT_DIR}
```

Project-specific semantics remain in `.perl-lsp.toml`; this plugin does not embed user include paths or machine-specific configuration.

## Initial file activation

The package maps these extensions to Perl:

- `.pl` and `.PL`
- `.pm`
- `.t`
- `.psgi`
- `.cgi`
- `.fcgi`

Activation and semantic support are separate evidence cells. This package does not claim `.pod`, `.xs`, mixed-language template formats, `cpanfile`, or extensionless/shebang-only scripts through Claude's extension mapping.

## Claude Code capability boundary

Claude's native LSP tool exposes a narrower surface than a full editor. Support claims are limited to operations directly exercised through Claude Code, such as definition, references, hover, document/workspace symbols, implementation, call hierarchy, and post-edit diagnostics where the host exposes them.

Server capabilities such as completion, signature help, rename, code actions, formatting, semantic tokens, and inlay hints remain editor/server features unless Claude Code exposes and independently proves them.

## Conflicting Perl LSP plugins

If another enabled Claude plugin claims the same Perl extension, verify which plugin actually owns the file before interpreting results. Disable the conflicting Perl LSP plugin when necessary so the active server is the expected EffortlessMetrics `perllsp` binary.

## Development validation

From the marketplace repository root:

```bash
claude plugin validate .
cargo test -p xtask --test claude_plugin_package
```

Real-host support requires the separate Claude compatibility receipts; static validation only proves package intent.

## License

The plugin package follows the repository's dual-license terms: MIT OR Apache-2.0.
