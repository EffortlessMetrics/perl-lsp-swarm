# .perl-lsp-swarm-spec

This namespace is the durable, repo-native knowledge base for specification-method artifacts.

## Scope this namespace owns

- Proposals (`proposals/`): why work exists, alternatives, and success criteria.
- Specs (`specs/`): behavior contracts and evidence expectations.
- ADRs (`adr/`): durable architecture decisions.
- Lanes (`lanes/`): focused implementation trackers (including lane tracker files).
- Templates (`templates/`): reusable artifact forms.
- Closeouts (`closeouts/`): what landed, proof links, and what remains.

## Non-goals

This namespace does **not** own tool/session state.

## External namespace awareness

This repo may also contain tool-specific state directories.

- `.spec/` is reserved for Spec Kit / speckit workflows.
- `.codex/` is reserved for Codex execution state.
- `.claude/` and `.jules/` are session/tool-specific state when present.

The `.<repo>-spec/` model does not own, rewrite, migrate, or validate those directories.
It may overlap conceptually with them, but this namespace is the repo-native long-term knowledge base.
