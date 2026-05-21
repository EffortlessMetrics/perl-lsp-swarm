# Contributing to repo-native spec rails

When adding or editing durable specification artifacts, place them under `.perl-lsp-spec/`.

## Ownership rule

Durable rails belong to the repository-owned namespace, not to agent or tool session directories.

- Owned: `.perl-lsp-spec/`
- External/awareness-only: `.codex/`, `.spec/`, `.claude/`, `.jules/`

## Practical guidance

1. Keep proposal/spec/ADR/lane/closeout concerns separated.
2. Link artifacts through `.perl-lsp-spec/index.toml`.
3. Reference existing `policy/*.toml` ledgers instead of duplicating policy definitions.
4. Keep `docs/` explanatory; keep durable control-plane artifacts in `.perl-lsp-spec/`.
