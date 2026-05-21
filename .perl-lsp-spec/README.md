# .perl-lsp-spec

This directory is the durable, repo-owned specification control plane for perl-lsp.

It is the long-term source of truth for proposal/spec/ADR/lane/closeout artifacts and their linkage.

## Scope

The `.perl-lsp-spec/` namespace owns durable specification rails:

- proposals (`why` and success criteria)
- specs (`what` behavior must hold)
- ADRs (`decision` and consequences)
- lanes (`how` execution is tracked)
- closeouts (`what landed and what proved it)
- support and policy references (claim/proof and enforcement linkage)

## External state namespaces

The following directories may exist but are external to this durable system:

- `.codex/`
- `.spec/`
- `.claude/`
- `.jules/`

Those namespaces may read from `.perl-lsp-spec/`, but this system does not own or mutate their scratch/session state.
