# Contributing to Rails artifacts

Use `.rails/` as the durable knowledge base for this repository.

## Where to put durable artifacts

- Proposals / PRDs: `.rails/proposals/`
- Specs: `.rails/specs/`
- ADRs: `.rails/adr/`
- Lane trackers: `.rails/lanes/`
- Templates: `.rails/templates/`
- Closeouts: `.rails/closeouts/`
- Support claim mapping: `.rails/support/`
- Policy ledger references: `.rails/policy/`
- Receipts: `.rails/receipts/`
- Schemas: `.rails/schemas/`

## External namespaces are awareness-only

Do not migrate, rewrite, validate, or claim ownership of:

- `.codex/`
- `.spec/`
- `.claude/`
- `.jules/`

These may be listed in `.rails/index.toml` as external namespaces, but they are not Rails-owned.

## Guardrails

- Keep durable Rails content out of agent tool directories.
- Use focused lane trackers under `.rails/lanes/`; do not create a global active queue.
- Link every artifact through `.rails/index.toml`.
