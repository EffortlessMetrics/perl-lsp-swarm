# Spec style and durable rails

The perl-lsp repository keeps long-term specification artifacts in the repo-owned `.perl-lsp-spec/` namespace.

## Source-of-truth separation

Use the full chain as distinct artifacts:

- roadmap
- proposal / PRD
- behavior spec
- ADR (when architectural decisions are needed)
- lane tracker + implementation plan
- proof commands and evidence
- support-tier or policy references
- closeout

Do not collapse these concerns into one mixed document.

## Namespace model

- `.perl-lsp-spec/` = durable repo knowledge base and control-plane rails.
- `docs/` = human-facing explanation and contributor guidance.
- `policy/` = live enforcement ledgers, referenced where relevant.
- `plans/` = only if already part of the repo's non-agent planning surface.

## External agent and tool state

Directories such as `.codex/`, `.spec/`, `.claude/`, and `.jules/` are awareness-only for this system.

They are not owned by this durable spec namespace.
