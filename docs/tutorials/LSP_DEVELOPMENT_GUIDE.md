# LSP Development Guide — compatibility pointer

The former guide described an earlier pre-collapse provider and workspace architecture, including retired constructor names, regex fallbacks, unsupported timing targets, and historical crate locations. It is retained as a compatibility entry point, but it is not a current implementation guide.

## Current contributor route

1. Read the [contributing guide](../../CONTRIBUTING.md) for repository workflow, issue linkage, review, and verification requirements.
2. Read the [architecture reference](../reference/ARCHITECTURE.md) to select the current package seam.
3. Use the [commands reference](../reference/COMMANDS_REFERENCE.md) for current local and CI commands.
4. Check [features.toml](../../features.toml), the relevant status page, and the issue/spec before making a capability claim.
5. Add or update focused tests and receipts with the implementation change; do not infer coverage, latency, readiness, or cross-file correctness from this page.

## Current ownership map

- AST structure and methods: \`crates/perl-ast\`
- Parsing, positions, trivia, and recovery: \`crates/perl-parser-core\`
- Public parser facade and parser-facing behavior: \`crates/perl-parser\`
- Semantic and workspace analysis: \`crates/perl-semantic-analyzer\` and \`crates/perl-workspace\`
- Protocol, transport, runtime, governance, and providers: \`crates/perl-lsp-rs-core\`
- Server implementation and startup: \`crates/perl-lsp-rs\`
- Public binary packaging: \`crates/perllsp\`
- Parser-accuracy fixtures and manifests: \`crates/perl-corpus\`

Former microcrates may now be modules inside these surviving packages. Confirm ownership in the workspace manifest and package README before choosing a file.

## Change workflow

For a new LSP feature or a correction to an existing provider:

1. Define the behavior and its unsupported or fallback cases in the issue/spec.
2. Locate the current owner from the architecture reference and manifest.
3. Add the smallest focused implementation or test slice.
4. Add a regression test, corpus expectation, or receipt that proves the claimed behavior.
5. Run the narrowest relevant package tests and formatting checks.
6. Run the repository's required PR gate and record any baseline or not-proven result explicitly.
7. Review the current PR head again after every material change.

Keep provider behavior, capability advertising, user-facing documentation, and generated status synchronized. A green parser test does not prove LSP behavior; a capability entry does not prove implementation; and a fixture selection does not prove complete language coverage.

## Useful evidence surfaces

- [Current status](../project/CURRENT_STATUS.md)
- [LSP feature policy](../reference/LSP_FEATURES.md)
- [Feature policy](../../features.toml) — canonical capability maturity and advertisement state
- [Architecture reference](../reference/ARCHITECTURE.md)
- [Verification protocol](../project/protocols/verification.md)
- [Commands reference](../reference/COMMANDS_REFERENCE.md)

Historical material remains available through Git history. It must not be used as current evidence for package ownership, performance, completeness, or release readiness.
