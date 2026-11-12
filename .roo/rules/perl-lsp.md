# perl-lsp Roo workspace rules

Read root `AGENTS.md`, the current issue or PR, and applicable package-local `AGENTS.md`
and `CLAUDE.md` before editing. `docs/agents/AUTHORITY_STATUS.md` classifies older
instructions; a provider file does not override current repository behavior.

Keep one coherent concern per PR and one writer per candidate. Preserve unrelated work
and never use `git stash` in the shared multi-worktree repository. Use a real controlling
issue number when known; never fabricate a placeholder issue link.

Use the existing `.agents/skills/*/SKILL.md` route selected by `AGENTS.md`. Read the file
directly when Roo has not discovered it. Do not invent a separate lifecycle.

## Setup and focused proof

Follow `CONTRIBUTING.md` for setup and
`docs/how-to/AGENT_CONTRIBUTING.md` for task-shell activation. `just agent-preflight`
inspects layout; it does not establish writer ownership or Cargo capacity.

Read `docs/agents/CARGO_STORAGE.md` before compiled work. Choose the package and target
required by the claim, and use matching preflight and execution requests. Examples:

```bash
scripts/cargo-admitted --preflight check -p <package> --profile agent --locked --lib
just agent-check <package> --lib
scripts/cargo-admitted --preflight test -p <package> --profile agent --locked --test <target>
just agent-test <package> --test <target>
```

These two recipes use `cargo-admitted`. Keep extra arguments identical in both commands;
a test-name filter is not a compilation-target selector. Use the direct admitted route
for wrapper options and the separately qualified staged native Clippy route for lint.
Raw Cargo aliases, `cargo-safe`, and the remaining legacy Just recipes are not admission.
Do not run broad gates before every edit or push merely because this adapter exists.

Formatting does not require a compiled xtask:

```bash
cargo fmt -p <package> -- --check
```

Production-code and test-helper rules come from current `AGENTS.md` and package guidance.
Preserve explanatory test context when using `perl-test-must` helpers. Missing remote
access does not prohibit local reversible work; unsupported remote claims remain
`NOT_PROVEN`. Repository instructions do not grant provider or host permissions.
