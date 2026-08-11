# Architecture Overview for Contributors

This is the current contributor-facing architecture reference. It describes the boundaries that exist in the workspace today; exact membership and publish policy remain authoritative in [Cargo.toml](../../Cargo.toml), and package-local READMEs own narrower API details.

## Orientation

The repository is a multi-crate Rust workspace. The crate graph is deliberately split where a boundary carries a useful contract, but historical microcrate names are not evidence of current membership. Several former crates have been absorbed into larger packages; the workspace manifest records those absorptions.

Start with these surfaces:

- [Cargo.toml](../../Cargo.toml) for workspace members, exclusions, and publish allowlist;
- [perl-parser-core README](../../crates/perl-parser-core/README.md) for the low-level parser boundary;
- [perl-parser README](../../crates/perl-parser/README.md) for the higher-level parsing facade;
- [perl-lsp-rs-core README](../../crates/perl-lsp-rs-core/README.md) for the consolidated LSP implementation core;
- [perl-lsp-rs README](../../crates/perl-lsp-rs/README.md) for the server implementation and embedding boundary;
- [perllsp package](../../crates/perllsp/) for the public Cargo-installed binary;
- [current status](../project/CURRENT_STATUS.md) for support and verification state.

Do not copy historical crate counts, latency figures, coverage percentages, or readiness labels from archived guides into current documentation.

## Current crate families

| Family | Current role |
| --- | --- |
| Parser and syntax | perl-token, perl-ast, perl-lexer, perl-parser-core, perl-parser, perl-parser-pest, and parser comparison/harness packages |
| Semantic and workspace | perl-semantic-analyzer, perl-semantic-facts, perl-workspace, perl-workspace-core, perl-module, perl-symbol, perl-line-index, and related position/URI packages |
| LSP runtime | perl-lsp-rs-core owns consolidated protocol, transport, runtime, configuration, governance, and provider modules; perl-lsp-rs supplies the server implementation facade |
| User entry point | perllsp provides the public binary package; perl-lsp-rs remains the implementation package behind that entry point |
| Debugging | perl-dap owns the native Debug Adapter Protocol surface |
| Compatibility and tooling | tree-sitter-perl-c, tree-sitter-perl-rs, perl-tree-sitter-compat, perl-ci-hygiene, perl-kwalitee, xtask, and test/support packages |

The table is a navigation aid, not a complete crate inventory. When it conflicts with the manifest or a package-local README, the manifest or README wins for its scope.

## Parser flow

Perl source is handled by the native parser stack in layers:

1. The lexer/token packages preserve source-oriented token information and lexical context.
2. perl-parser-core owns the low-level parsing engine, AST-facing nodes, parse results, position/trivia infrastructure, and recovery boundaries.
3. perl-parser provides the higher-level parsing facade and re-exports the analysis, workspace, refactoring, and provider-oriented surfaces that downstream users commonly need together.
4. Semantic and workspace packages consume parser output for symbol, scope, module, index, and cross-file operations.

Parser changes should begin at the narrowest package that owns the behavior. Do not route parser-core work through an obsolete lexer/parser architecture guide or assume that every historical provider crate still exists independently.

## LSP flow

The user-facing path is:

1. An editor starts the perllsp executable, normally over stdio; the implementation also exposes the supported TCP/server modes described by its package documentation.
2. The server implementation in perl-lsp-rs accepts and dispatches LSP traffic.
3. Shared protocol, transport, runtime, configuration, governance, capability, and provider logic is owned by perl-lsp-rs-core.
4. Parser, semantic-analysis, workspace, and other focused packages supply the language data needed by providers.
5. Responses and notifications return through the server's protocol transport to the editor.

The exact request handler and provider paths are implementation details. Before changing one, follow the package-local instructions and focused tests rather than relying on path names from pre-collapse documentation.

## DAP boundary

perl-dap is the native Debug Adapter Protocol package. Its user-facing status and supported run modes are documented in [the debugging guide](../how-to/DEBUGGING.md), the package README, and the DAP status page. Native DAP support, legacy bridge compatibility, and scorecard proof are separate surfaces; a transport handshake is not by itself proof of an interactive attach journey.

## Workspace policy

The workspace excludes legacy tree-sitter-perl, fuzz, and archive components from the default workspace. Exclusion is not deletion: inspect the manifest and the relevant package documentation before building an excluded component directly.

Former crates may be represented by modules inside a surviving package. The comments in Cargo.toml record the intended ownership transition; they are more current than old links, generated inventories, or historical migration documents.

## How to choose a change seam

- Syntax or AST behavior: begin with perl-parser-core and its focused tests.
- Higher-level parser API or combined parser facade: begin with perl-parser.
- Scope, symbols, modules, or cross-file behavior: begin with the owning semantic/workspace package.
- LSP protocol, runtime, capability, or provider behavior: begin with perl-lsp-rs-core and its package-local guidance.
- Server startup, transport wiring, or embedding: begin with perl-lsp-rs or perllsp as appropriate.
- Debug adapter behavior: begin with perl-dap.
- Build, policy, status, or generated evidence: begin with the relevant xtask or perl-ci-hygiene command and its governing document.

Every change should identify its owning package, focused proof, and claim boundary in the issue or PR. Current behavior is established by code and executable evidence; this page is a map to those authorities, not a substitute for them.

## Related documentation

- [Documentation index](../INDEX.md)
- [Contributor orientation](../project/ORIENTATION.md)
- [Local CI validation](../project/CI_LOCAL_VALIDATION.md)
- [LSP contribution guide](../how-to/CONTRIBUTING_LSP.md)
- [Stability policy](STABILITY.md)
