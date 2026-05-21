# Rails framework

`.rails/` is the durable Rails knowledge base.

`docs/` explains Rails to humans.

## Ownership boundaries

Rails owns the durable source-of-truth framework under `.rails/`, including proposals, specs, ADRs, lanes, templates, closeouts, support maps, policy references, receipts, and schemas.

Rails does not own external/tool-specific state:

- `.codex/` is Codex execution state and is not owned by Rails.
- `.spec/` is Spec Kit / speckit state and is not owned by Rails.
- `.claude/` and `.jules/` are external agent/session spaces and are not owned by Rails.

## Source-of-truth stack

- Proposals: why work exists
- Specs: behavior contracts and evidence requirements
- ADRs: durable architecture decisions
- Lanes: focused implementation trackers and sequencing
- Support maps: product claim to proof mapping
- Policy references: governed ledgers and enforcement sources
- Receipts: proof bundles
- Closeouts: what landed, what proved it, and what remains

## Artifact graph rule

Every Rails-owned artifact must be linked through `.rails/index.toml`.

No Rails-owned artifact path may live under `.codex/`, `.spec/`, `.claude/`, or `.jules/`.
