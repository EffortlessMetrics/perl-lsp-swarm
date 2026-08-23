# perl-lsp-rs

Use this crate when you need the internal implementation library behind the
public `perllsp` Cargo entry.

## When to use this crate

Use `perl-lsp-rs` when you want to work on or embed the real language server implementation:

- run the `perllsp` binary behind an editor such as VS Code, Neovim, Emacs, or Helix
- expose Perl LSP features over stdio or TCP through the public product facade
- embed the server entry point from Rust instead of shelling out to a binary

If you only need a parser, tokenizer, or a single feature provider, prefer the
smaller workspace crates such as `perl-parser`, `perl-lexer`, or the
`perl-lsp-*` provider crates.

## Public install path

```bash
cargo install perllsp
```

For a workspace-local install from this repository, use:

```bash
cargo install --path crates/perllsp
```

If you are hacking on the implementation package itself, use workspace package
commands such as `cargo build -p perl-lsp-rs` or `cargo test -p perl-lsp-rs`.
`perl-lsp-rs` is library-only and does not install a separate `perl-lsp`
command.

## Quick start

```bash
perllsp --stdio
perllsp --health
```

## Usage

```bash
perllsp --stdio                # stdio mode (default, for editor integration)
perllsp --socket --port 9257  # TCP socket mode
perllsp --health               # health check
perllsp --version              # version info
```

## Embedding from Rust

The `perl_lsp` library re-exports `LspServer`, `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError`, and a convenience `run_stdio()` entry point for embedding.

## Workspace role

This is the internal implementation library in the
[`perl-lsp`](https://github.com/EffortlessMetrics/perl-lsp) workspace. It delegates parsing to `perl-parser` and dispatches protocol, transport,
runtime, and provider work through the consolidated `perl-lsp-rs-core` package.
The `perllsp` crate owns the public executable identity.

## License

MIT OR Apache-2.0