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

## Notes

- `check-githooks` reports missing or stale installed generated hooks as `NOT_PROVEN`.
- The commit hook does not run workspace-wide formatting, Clippy, tests, or RIPR.
- Use `git commit --no-verify` or `git push --no-verify` only under the documented bypass policy.
