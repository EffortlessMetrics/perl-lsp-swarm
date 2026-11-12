# Repository navigation

Start with root `AGENTS.md`, the current repository map in `CONTRIBUTING.md`, and the
workspace members and exclusions in `Cargo.toml`. Do not infer crate existence,
dependency tiers, or consolidation state from an old hand-maintained directory tree.
Read the owning package's manifest, source, tests, README, and local `AGENTS.md` or
`CLAUDE.md` before changing its architecture.

Common starting points are the parser, semantic analysis, LSP host/core, DAP, extension,
and repository tooling surfaces named in the current contributor map. Verify paths
against the checkout rather than creating a crate to match a remembered name.

Root `AGENTS.md` and `CLAUDE.md` are provider entrypoints, not a distinction between
implementation and orchestration ownership. Use the existing named repository skills;
open `.agents/skills/*/SKILL.md` directly when Kiro has not discovered them.

Kiro's `.kiro/specs/` documents remain task-specific design evidence. Read only the
specification relevant to the current claim and check its status against
`docs/agents/AUTHORITY_STATUS.md`, current code, and the live issue or PR. Historical
checklists do not establish current transaction state or a universal workflow.

Keep new crates under their owning family and update workspace membership when needed.
Keep one coherent concern per PR. Do not commit top-level `adr.md`, `specs.md`, or
`task_list.md`, or persist worker liveness and temporary orchestration state as doctrine.
