# LSP feature development — compatibility pointer

This former how-to guide described retired LSP feature paths, absorbed provider crates, and unsupported “exactly five touch-points” implementation rules. It is not a current implementation contract.

For a new LSP feature or a correction to an existing one:

1. Define behavior, unsupported cases, and proof in the issue/spec.
2. Select the current owner from the architecture reference, workspace manifest, and package READMEs.
3. Update the implementation, capability policy, tests, generated artifacts, and user-facing documentation only where the current contract requires them.
4. Prove the claim with focused tests or receipts; a feature catalog entry alone is not implementation proof.
5. Run the narrowest affected checks, then the repository PR gate, recording baseline or not-proven results explicitly.

Current evidence and policy surfaces:

- [features.toml](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/features.toml)
- [LSP status](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/status/lsp.md)
- [architecture reference](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/reference/ARCHITECTURE.md)
- [verification protocol](https://github.com/EffortlessMetrics/perl-lsp-swarm/blob/main/docs/project/protocols/verification.md)

This compatibility page does not establish coverage, latency, readiness, or package ownership.
