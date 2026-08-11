# Project Orientation

> For the documentation hub, see [README.md](../README.md). This page is a stable orientation for active contributors, not a live status report.

> **Current-state rule**: Use [CURRENT_STATUS.md](CURRENT_STATUS.md), the [roadmap](ROADMAP.md), and the [release status](status/release.md) for current claims. This page intentionally avoids duplicating release metrics and issue counts.

## You Are Here

perl-lsp is a Rust workspace for Perl parsing, semantic analysis, LSP, and native DAP support.

The current shipped line and evidence-backed subsystem status live in [CURRENT_STATUS.md](CURRENT_STATUS.md). Capability truth lives in [features.toml](../../features.toml), and exact workspace membership lives in the root [Cargo.toml](../../Cargo.toml).

## Read These First

1. [Current Status](CURRENT_STATUS.md) — shipped line and evidence routes
2. [Roadmap](ROADMAP.md) — plans, exit criteria, and deferrals
3. [Documentation Index](../INDEX.md) — routes by task and document type
4. [Contributing Guide](../../CONTRIBUTING.md) — contribution workflow
5. [Commands Reference](../reference/COMMANDS_REFERENCE.md) — build, test, and CI commands
6. [Architecture Reference](../reference/ARCHITECTURE.md) — current ownership seams
7. [LSP Development Guide](../tutorials/LSP_DEVELOPMENT_GUIDE.md) — contributor workflow, with current claims verified against code and tests

## Current Focus

Do not infer priorities from this page. Start with the active milestone and release blockers:

- [ROADMAP.md](ROADMAP.md)
- [status/index.md](status/index.md)
- [status/release.md](status/release.md)
- GitHub milestones and issues

Recurring work may include parser corpus coverage, LSP conformance, DAP preview hardening, distribution packaging, and merge-gate health; the linked sources determine which of those is active.

## Workspace Shape

The maintained workspace includes, among other packages:

- \`perl-ast\` — AST types and methods
- \`perl-parser-core\` — parsing, position/trivia infrastructure, and recovery
- \`perl-parser\` — public parser facade
- \`perl-semantic-analyzer\` and \`perl-workspace\` — semantic and workspace analysis
- \`perl-lsp-rs-core\` — consolidated LSP protocol, transport, runtime, governance, and providers
- \`perl-lsp-rs\` and \`perllsp\` — server and public binary surfaces
- \`perl-dap\` — native Debug Adapter Protocol surface
- \`perl-corpus\` — corpus fixtures and parser-accuracy evidence
- \`xtask\` — repository automation

The root manifest also records absorbed crates and excludes the legacy \`tree-sitter-perl\`, \`fuzz\`, and \`archive\` trees. Do not treat historical crate inventories as current workspace topology.

## Quick Commands

Use the narrowest command that matches the question:

\`\`\`bash
# Workspace validation
cargo check --workspace
cargo test --workspace

# Parser-focused validation
cargo test -p perl-parser
cargo test -p perl-parser-core

# LSP and DAP validation
cargo test -p perl-lsp-rs
cargo test -p perl-dap

# Run the server locally
cargo run -p perl-lsp-rs -- --stdio

# Repository formatting and governed checks
cargo fmt --all -- --check
cargo xtask fmt --check
\`\`\`

Check the [Commands Reference](../reference/COMMANDS_REFERENCE.md) and repository contribution instructions before using broader or release-specific gates.

## Where to Start a Change

- AST structure or methods: \`crates/perl-ast\`
- Syntax, parsing, or recovery: \`crates/perl-parser-core\`
- Public parser behavior: \`crates/perl-parser\`
- Semantic or workspace behavior: \`crates/perl-semantic-analyzer\` and \`crates/perl-workspace\`
- LSP providers, protocol, transport, or runtime: \`crates/perl-lsp-rs-core\`
- Server startup or binary packaging: \`crates/perl-lsp-rs\` and \`crates/perllsp\`
- DAP behavior: \`crates/perl-dap\`
- Corpus fixtures and accuracy manifests: \`crates/perl-corpus\`

Use the package README and the relevant issue/spec as the local contract before editing.

## Help and Verification

For user setup, use the installation and troubleshooting guides in [docs/INDEX.md](../INDEX.md). For contributor changes, follow [CONTRIBUTING.md](../../CONTRIBUTING.md), preserve claim boundaries, and record proof in the PR.

This page is intentionally an orientation map. It does not establish parser coverage, latency, stability, release readiness, or “production-ready” claims.
