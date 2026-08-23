# Pre-commit Integration

The repository-owned commit boundary is `cargo xtask precommit`, which validates the
exact staged tree. The installer also keeps the retained affected-proof pre-push hook
separate from the commit gate.

## Quick Start

```bash
cargo xtask ci-hygiene install-githooks
cargo xtask ci-hygiene check-githooks
```

The installed hooks are:

- `pre-commit`: rejects known placeholder identities, then runs `cargo xtask precommit`.
- `pre-push`: retains the affected/bounded push proof described by issue #3985.

The published `.pre-commit-hooks.yaml` is an optional external integration and routes
to the same `cargo xtask precommit` command; it is not a second repository policy.

## Changie validation

When a staged Changie input changes, the existing `changie_fragment_staged` commit
check materializes the frozen staged `.changie.yaml`, `aqua.yaml`, changes directory,
and configured project changelogs in a temporary sandbox. It then runs Changie's own
`batch --dry-run --keep` path for every configured project. The working tree and live
index are not consulted after the gate captures its tree OID.

The checker prefers the Changie version pinned by staged `aqua.yaml`, disables Aqua
lazy installation so a commit never introduces a network-backed tool install, and
falls back to `changie` on `PATH` (the Nix development shell supplies that binary).
Missing or broken tooling is `NOT_PROVEN`; a schema-invalid fragment or a Changie render failure
is `BLOCKED`. Repair or recreate fragments with `cargo change`.

## Notes

- `check-githooks` reports missing or stale installed generated hooks as `NOT_PROVEN`.
- The commit hook does not run workspace-wide formatting, Clippy, tests, or RIPR.
- Use `git commit --no-verify` or `git push --no-verify` only under the documented bypass policy.
