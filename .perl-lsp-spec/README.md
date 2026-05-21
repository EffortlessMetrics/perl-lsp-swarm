# perl-lsp Spec Rails

This directory is the durable, repo-owned specification namespace for perl-lsp.

## Ownership doctrine

- `.perl-lsp-spec/` stores durable proposals, specifications, ADRs, and lane trackers.
- `docs/` explains process and implementation guidance for contributors.
- `policy/` stores live policy ledgers (for example, parser-gap and performance budgets) when those ledgers become active.
- `.codex/`, `.spec/`, `.claude/`, and `.jules/` are tool/external awareness namespaces and are not durable owned artifacts for this lane.

## Track A scope boundary

Track A establishes a receipt-driven parser-comparison lane and documentation fairness boundaries only. It does **not** change parser behavior or parser support status.
