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
  state; it does not admit a Cargo build or prove host capacity.
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

- Before resource-bearing Cargo work, read
  [Cargo storage admission](docs/agents/CARGO_STORAGE.md), choose the smallest exact
  subject and target that can falsify the claim, then preflight and execute that same
  request through the admitted route. For example:

  ```bash
  scripts/cargo-admitted --preflight check -p <package> --locked
  scripts/cargo-admitted check -p <package> --locked
  scripts/cargo-admitted --preflight test -p <package> --all-targets --locked
  scripts/cargo-admitted test -p <package> --all-targets --locked
  ```

- Clippy has a separately qualified
  [staged native route](docs/agents/CARGO_STORAGE.md#staged-native-clippy). Do not
  substitute raw `cargo clippy` or legacy `just agent-clippy` for that qualification.
  Legacy `cargo-safe` and `just agent-*` commands retain compatibility behavior; they
  are not host admission for new agent work.
- Broad claim-level commands run only when the selected route requires them and the
  host is admitted for the actual work they perform.
- Use focused proof first and never weaken tests, checks, support claims, or required
  evidence to obtain green status. Missing or contradictory evidence is `NOT_PROVEN`.

## Provider boundary

Repository files define instructions and reusable validation entrypoints. They do not
publish or repair a provider's external saved environment, credentials, network
allowlist, or account permissions. `scripts/cloud-startup` validates an already
provisioned Linux task shell; it does not install Rust or Node, save a Codex or Claude
environment, or make `api.github.com` reachable.

When this bridge conflicts with higher current authority or actual repository behavior,
follow that authority and report the bridge defect.