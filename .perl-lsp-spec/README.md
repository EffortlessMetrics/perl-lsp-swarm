# .perl-lsp-spec

This directory is the durable, repo-owned control plane for perl-lsp proposals, specs, ADRs, and lane trackers.

Tool-specific execution areas (for example `.codex/`, `.spec/`, `.claude/`, `.jules/`) may contain transient planning or automation artifacts, but normative project intent lives under `.perl-lsp-spec/`.

## Lanes

- `lanes/parser-differential/` — fairness and repeatability for parser comparisons.
- `lanes/receiver-facts-completion/` — receiver-fact to completion cutover.
- `lanes/tree-sitter-wording/` — wording and claim-boundary correctness for Tree-sitter docs.
