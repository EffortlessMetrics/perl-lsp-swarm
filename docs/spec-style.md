# Repo-native spec style

This repository keeps long-lived specification artifacts in `.<repo>-spec/`, with a clean separation between intent, contract, decision, implementation tracking, and outcome evidence.

## Method boundaries

Use separate artifacts so one document does not become proposal, spec, plan, CI policy, and release proof all at once:

- **Why**: proposal
- **What must be true**: spec
- **Decision**: ADR
- **How delivery is tracked**: lane tracker
- **What proves it**: verification references
- **What happened**: closeout

## Ownership rails

Owned rails for this method are intentionally narrow:

- `.<repo>-spec/` (durable knowledge base)
- `docs/` (human-facing guidance)
- `policy/` only when existing policy ledgers are part of the proof map
- `plans/` only when the repository already uses plans as implementation trackers

## External namespace awareness

Tool/session directories can exist in the same repository, but are awareness-only from this method's perspective:

- `.spec/`
- `.codex/`
- `.claude/`
- `.jules/`

Do not make those namespaces the source of truth for repo-native spec artifacts.
