# PLSP-ADR-0004: LSP Stack Extraction Governance

- Status: Accepted
- Date: 2026-05-21
- Deciders: perl-lsp maintainers
- Related Spec: [PLSP-SPEC-0026](../specs/PLSP-SPEC-0026-lsp-stack-extraction-boundary.md)
- Related Plan: [lsp-stack-extraction implementation plan](../../plans/lsp-stack-extraction/implementation-plan.md)

## Context

The current LSP implementation in this repository mixes reusable LSP stack primitives and Perl-specific language/runtime behavior. This raises extraction risk because boundary decisions can drift PR-by-PR unless the control plane is explicit.

The extraction is intentionally deferred until after the 0.14.1 release lane and after Angelo's LSP4IJ submission work lands against the current perl-lsp layout.

## Decision

We establish the following governance rails before any code movement:

1. **Timing rail**: No extraction PRs land before post-0.14.1 stabilization and post-Angelo submission.
2. **Boundary rail**: Extraction scope is limited to reusable LSP stack primitives:
   - protocol (JSON-RPC + method constants)
   - transport (content-length framing and transport interfaces)
   - capability registry/builder and feature profiles
   - runtime route table and handler traits
   - inline completion protocol types and backend traits
3. **Non-goal rail**: Perl language intelligence stays in perl-lsp crates:
   - parser/semantic analysis integration
   - deterministic Perl completions
   - Perl vendor streaming method (`textDocument/perlInlineCompletionStream`)
   - perltidy/Perl::Critic/test-runner specific runtime concerns
   - DAP protocol/runtime
4. **Proof rail**: Every extraction phase must preserve current advertised capability behavior and pass scoped proof commands declared in the companion spec.
5. **Dependency rail**: The extracted `lsp-stack` crate must not depend on Perl parser/semantic/workspace or DAP crates.

## Consequences

### Positive

- Reviewers can reject scope drift early using an explicit accepted boundary.
- Extraction can proceed incrementally (traits first, code movement second) without blocking release work.
- Future README and marketing claims remain tied to verifiable capability and conformance evidence.

### Trade-offs

- Additional up-front documentation introduces short-term process overhead.
- Some temporary re-export shims may be needed during migration windows.

## Follow-up Obligations

1. Land and keep updated [PLSP-SPEC-0026](../specs/PLSP-SPEC-0026-lsp-stack-extraction-boundary.md) as the executable contract.
2. Execute the extraction plan in narrow PRs defined in [plans/lsp-stack-extraction/implementation-plan.md](../../plans/lsp-stack-extraction/implementation-plan.md).
3. Maintain capability snapshot parity checks across extraction phases.
4. Keep root `features.toml` canonical; either regenerate or retire stale duplicate catalogs as part of extraction.
