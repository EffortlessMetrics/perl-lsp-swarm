# Aider repository bridge

Aider loads this file and [`AGENTS.md`](AGENTS.md) as persistent read-only context.
This file supplies Aider-specific mechanics only; it is not a second repository
operating contract.

## Start or resume

Establish the current issue or pull request, branch, candidate, and mutation owner before
editing. Preserve unrelated work. Use Aider's repo map to locate likely files, then add
only task-specific material with `/read`, including applicable package-local
`AGENTS.md` or `CLAUDE.md`, the controlling issue/specification, and the selected
`.agents/skills/*/SKILL.md` procedure.

Do not preload README, roadmaps, status projections, the capability catalog, or broad
source trees merely because they may become relevant. Read them when the current claim
actually depends on them.

Aider does not need a parallel lifecycle. Follow the route selected by `AGENTS.md` and
open the corresponding `.agents/skills/` procedure directly when provider-native skill
discovery is unavailable.

## Mutation and commits

Repository configuration intentionally disables Aider auto-commits, dirty-repository
commits, automatic lint, and automatic tests. Keep those actions explicit so the current
flow can preserve unrelated state, stage intended paths, use the governed commit/PR
contract, and admit the exact proof command.

Never use `git stash` in this shared multi-worktree repository. One writer may mutate a
candidate at a time; read-only research and review remain non-exclusive.

## Proof

After task-shell activation, run `just agent-preflight` for layout/setup inspection.
That is not writer admission, hook-readiness proof, or Cargo capacity proof. Mutation
ownership remains a separate fact established through the selected issue/PR route.
Formatting remains separate:

```bash
cargo fmt -p <package> -- --check
```

Before compiled work, read [Cargo storage admission](docs/agents/CARGO_STORAGE.md).
Choose the package and target needed by the claim. Preflight the exact underlying
request, then execute from the same shell and worktree:

```bash
scripts/cargo-admitted --preflight check -p <package> --profile agent --locked --lib
just agent-check <package> --lib
scripts/cargo-admitted --preflight test -p <package> --profile agent --locked --test <target>
just agent-test <package> --test <target>
```

These two Just recipes use the admitted route; `agent-test` requires a target selector.
A test-name filter does not restrict compilation targets. Keep extra arguments identical
in preflight and execution; use the direct admitted route for wrapper-specific options.

Clippy uses the separately qualified
[staged native route](docs/agents/CARGO_STORAGE.md#staged-native-clippy). Raw Cargo
aliases, `cargo-safe`, and the remaining legacy Just recipes are not admission. Broad
gates run only when the claim requires them and the host is admitted for their work.

## Provider boundary

Repository configuration does not choose an Aider model, store API keys, grant network
access, or alter global/user configuration. Unavailable remote state, credentials, or
provider capability remains `NOT_PROVEN`; it does not block local reversible work that
does not depend on it.
