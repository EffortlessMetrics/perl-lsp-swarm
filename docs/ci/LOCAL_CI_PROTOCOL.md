# Local CI Protocol

Choose the smallest proof that can falsify the change, following
[AGENTS.md](../../AGENTS.md#choose-local-proof). Run focused proof first, then
affected package proof; broaden only when risk or the integration gate selects it.
For documentation-only changes, check links and the existing narrow contracts for
the edited surface. Record passed, failed, and unavailable proof against the tested
commit in the PR rather than updating historical PR #214 summaries.

Before compiled proof, follow [Cargo storage admission](../agents/CARGO_STORAGE.md).
Preflight and execute the same request from the same worktree and task shell,
preserving package, target, feature, profile, and test filters. For example, replace
`PACKAGE` with the owning package:

```bash
bash scripts/cargo-admitted --preflight test -p PACKAGE --locked &&
  bash scripts/cargo-admitted test -p PACKAGE --locked
```

Use the separately documented staged native Clippy route for linting. The `xtask`
alias is not admitted; use the storage contract's `run -p xtask --locked -- ...`
route when compiled xtask proof is selected.

## Existing aggregate recipes

The [justfile](../../justfile) retains these compatibility entry points:

- `just ci-gate`: broad local checks, including library tests, all-target checks,
  Clippy, and policy checks.
- `just ci-full`: a separate broad suite including core/LSP tests, UX tests, and
  documentation builds; it is not simply an extension of `ci-gate`.
- `just ci-policy`: policy checks that include `cargo xtask` invocations. It is
  not a build-free documentation check.

These recipes do not establish Cargo admission. Do not infer that every edit needs
an aggregate run from their names or legacy comments. Local proof also does not
replace current required GitHub checks or authorize a merge. Follow the
[late-qualification contract](../agents/DEVELOPMENT_METHOD.md#late-qualification-and-publication)
for publication and hosted qualification; this protocol changes no workflow trigger,
required context, or protection setting.
