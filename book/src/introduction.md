# Introduction

perl-lsp is a Rust workspace for Perl parsing, semantic analysis, Language Server Protocol support, native Debug Adapter Protocol support, and editor integration.

This book is a navigation surface. Current release, capability, coverage, and readiness claims belong to the linked status and policy authorities rather than to this introduction.

## Start here

- [Quick start](./quick-start.md) — install and run the server.
- [Installation](./getting-started/installation.md) — supported setup paths.
- [Editor setup](./getting-started/editor-setup.md) — configure an editor.
- [First steps](./getting-started/first-steps.md) — current contributor orientation.
- [LSP features](./user-guides/lsp-features.md) — advertised capability policy and current routes.
- [Known limitations](./user-guides/known-limitations.md) — bounded parser and product limitations.
- [Current status](./reference/status/index.md) — current evidence and release narrative.
- [Architecture](./architecture/overview.md) — current ownership seams.

## Workspace seams

The maintained workspace includes:

- `perl-ast` for AST types and methods;
- `perl-parser-core` and `perl-parser` for parsing infrastructure and the public parser facade;
- `perl-semantic-analyzer` and workspace packages for semantic analysis;
- `perl-lsp-rs-core`, `perl-lsp-rs`, and `perllsp` for LSP runtime, server integration, and the public binary;
- `perl-dap` for native DAP;
- `perl-corpus` for parser-accuracy fixtures and manifests.

The root manifest and package READMEs are authoritative for exact membership and narrower implementation details.

## Evidence boundary

A feature-policy entry does not prove implementation. A parser fixture does not prove complete Perl coverage. A green focused test does not prove cross-file, editor, or release behavior. Follow the relevant guide and status surface for the claim you need to make.

The documentation follows Diátaxis: tutorials teach, how-to guides solve tasks, references state contracts, and explanations record rationale.
