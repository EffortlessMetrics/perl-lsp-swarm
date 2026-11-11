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

After task-shell activation, run the cheap layout/setup preflight:

```bash
just agent-preflight
```

That is not Cargo capacity proof. Formatting remains separate:

```bash
cargo fmt -p <package> -- --check
```

Before compiled work, read
[Cargo storage admission](docs/agents/CARGO_STORAGE.md), choose the smallest exact
subject and target that can falsify the claim, and preflight and execute that same
request through `scripts/cargo-admitted`. For example:

```bash
scripts/cargo-admitted --preflight check -p <package> --locked
scripts/cargo-admitted check -p <package> --locked
scripts/cargo-admitted --preflight test -p <package> --all-targets --locked
scripts/cargo-admitted test -p <package> --all-targets --locked
```

Follow the separately qualified
[staged native Clippy route](docs/agents/CARGO_STORAGE.md#staged-native-clippy). Do not
substitute raw resource-bearing Cargo commands, legacy agent wrappers, or a universal
Nix gate for the repository's current admission and proof owners. Broad gates run only
when the selected claim requires them and the host is admitted for the actual work.

## Provider boundary

Repository configuration does not choose an Aider model, store API keys, grant network
access, or alter global/user configuration. Unavailable remote state, credentials, or
provider capability remains `NOT_PROVEN`; it does not block local reversible work that
does not depend on it.
