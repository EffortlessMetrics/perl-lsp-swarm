# Gemini CLI repository bridge

This is Gemini CLI's entrypoint, not a second operating contract. Use the current
issue or pull request, the checked-out repository, and [`AGENTS.md`](AGENTS.md) as
shared authority. Applicable package-local `AGENTS.md` and `CLAUDE.md` files still
govern their directories. Check
[`docs/agents/AUTHORITY_STATUS.md`](docs/agents/AUTHORITY_STATUS.md) before treating
an older agent, workflow, or orchestration document as current.

## Start or resume

- Confirm the repository, branch, issue or PR, candidate head, and mutation owner.
  Preserve unrelated work.
- For a new checkout or host shell, follow
  [Contributor quick start](CONTRIBUTING.md#quick-start). For tools installed by an
  earlier setup process, use
  [task-shell activation](docs/how-to/AGENT_CONTRIBUTING.md#activate-tools-in-the-task-shell).
- After activation, run `just agent-preflight`. It checks repository layout and setup
  state; it does not establish writer ownership or admit a Cargo build.
- Resume existing work through
  [session operations](docs/how-to/SESSION_OPERATIONS.md), not a stale summary.
- The current upstream branch is `origin/main`. If network or GitHub access is
  unavailable, keep remote state `NOT_PROVEN`; GitHub CLI is not a prerequisite for
  local inspection and reversible work.

## Use the existing route

Gemini CLI does not need a parallel lifecycle. Open the applicable repository
procedure directly when it is not discovered automatically:

- [`deliver-goal`](.agents/skills/deliver-goal/SKILL.md) for a durable multi-PR outcome;
- [`deliver-pr`](.agents/skills/deliver-pr/SKILL.md) for one issue, PR, candidate, or
  coherent claim;
- [`prepare-issue`](.agents/skills/prepare-issue/SKILL.md) or
  [`prepare-proof`](.agents/skills/prepare-proof/SKILL.md) for a bounded problem or
  proof seam;
- [`build-candidate`](.agents/skills/build-candidate/SKILL.md) for implementation and
  candidate challenge;
- [`finish-pr`](.agents/skills/finish-pr/SKILL.md) for review, qualification,
  integration, and reconciliation.

Follow the selected route's named transitions. Missing provider-native skill UI does
not authorize a second workflow or stopping after research, a plan, or green checks.

## Mutation and proof

- One writer may mutate a candidate at a time; read-only research and review are
  non-exclusive.
- Never use `git stash` in this shared multi-worktree repository. Use scoped restore or
  a branch-local WIP commit when preservation is required.
- Keep one coherent concern per PR. Commit count is not an acceptance rule; preserve
  useful published commits unless an authorized rewrite is required.
- Keep formatting separate from compiled proof:

  ```bash
  cargo fmt -p <package> -- --check
  ```

Before compiled work, read [Cargo storage admission](docs/agents/CARGO_STORAGE.md).
Choose the package and target that can falsify the claim, then preflight the exact
underlying request before execution from the same shell and worktree. For example:

```bash
scripts/cargo-admitted --preflight check -p <package> --profile agent --locked --lib
just agent-check <package> --lib
scripts/cargo-admitted --preflight test -p <package> --profile agent --locked --test <target>
just agent-test <package> --test <target>
```

The current `agent-check` and `agent-test` recipes use `cargo-admitted`; the test
recipe requires an explicit target selector. Choose `--lib`, a named target, or a
broader population according to the claim, not by default. A test-name filter alone
does not restrict which targets Cargo compiles. Keep all additional arguments equal
between preflight and execution. Use the direct admitted route for wrapper options.

Clippy has a separately qualified
[staged native route](docs/agents/CARGO_STORAGE.md#staged-native-clippy). Raw Cargo
aliases, `cargo-safe`, and the remaining legacy Just recipes are not admission.
Broad claim-level commands run only when the current route requires them and the host
is admitted for their actual work. Never weaken tests or required evidence to obtain
green status. Missing or contradictory evidence remains `NOT_PROVEN`.

## Provider boundary

Repository files define instructions and reusable validation entrypoints. They do not
publish or repair a provider's external saved environment, credentials, network
allowlist, or account permissions. `scripts/cloud-startup` validates an already
provisioned Linux task shell; it does not install Rust or Node, save a Codex or Claude
environment, or make `api.github.com` reachable.

When this bridge conflicts with higher current authority or actual repository behavior,
follow that authority and report the bridge defect.
