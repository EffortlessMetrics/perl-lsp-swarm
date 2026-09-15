# Droid Manual Smoke-Test Contract

The only active Droid lane in this repository is the explicitly requested PR command
workflow in `.github/workflows/droid.yml`.

Automatic PR review in `.github/workflows/droid-review.yml` and automatic or scheduled
security execution in `.github/workflows/droid-security-scan.yml` remain paused. A
successful manual smoke test does not reactivate them or close the broader isolation work
in #6098.

## Supported invocation boundary

A current repository writer may invoke Droid from an open, same-repository pull request
using:

- `@droid review`
- `@droid security`
- `@droid security --full`
- `@droid fill`

Bare `@droid` follows the action's native review default.

Issue-only requests, closed pull requests, and fork pull requests fail before provider
secrets are supplied. The event guard requires an authorized human association, and the
safe action performs its native live permission check before model execution. Arbitrary
implementation requests such as `@droid fix the CI` are not a capability of this action.

## Expected runtime

A valid invocation must:

1. Run on GitHub-hosted `ubuntu-24.04`; it must not wait for the retired
   `droid-review` self-hosted label set.
2. Resolve the pull request's current exact head and preserve the safe action's native
   live actor-permission check.
3. Check out that head with `persist-credentials: false`.
4. Install Droid CLI `0.219.0` from Factory's versioned direct-binary path, verify the
   published SHA-256 checksum before execution, and disable in-process auto-updates.
5. Use `custom:MiniMax-M3-0` for review, security, and fill.
6. Keep Factory and MiniMax credentials step-scoped, clear inherited Anthropic routing,
   and keep Factory state under a private temporary home.
7. Disable raw/full debug output and remove the temporary home and prompt directory with
   an `always()` cleanup step.
8. Preserve exact-head checking through `expected_head_sha`.

The pinned CLI removes the mutable `curl | sh` installer from this lane. The pinned safe
action still installs its Bun dependencies at runtime and retains GitHub publication
authority; those remaining trust-boundary questions stay owned by #6098.

## Live acceptance

An ordinary pull-request timeline comment is an `issue_comment` event and runs the
workflow from the default branch, so it cannot prove a candidate workflow before merge.
A submitted pull-request review or inline review comment is a pull-request event and can
exercise this candidate at its PR head. Before merge, submit a fresh review containing
`@droid review` at the exact current head. After merge, repeat the smoke on a disposable
same-repository pull request to prove the default-branch installation.

Acceptance requires all of the following:

- the `Droid Tag` job is assigned to `ubuntu-24.04` and starts without a custom runner;
- the prerequisite, subject, checkout, pinned-CLI, and configuration steps pass;
- the log reports Droid CLI `0.219.0`;
- the action reaches MiniMax M3 inference;
- Droid publishes a useful review or an explicit no-findings result;
- no `droid-review-debug-*` artifact is produced;
- cleanup completes;
- the reviewed head still matches the pull request head.

A successful process exit without inference or publication is not a successful smoke
test. A stale-head rejection is a correct result, not a clean review.

## Review-output expectations

A clean review should state what was inspected, which checks were performed, why no
inline comments were emitted, residual risk, and what was observed versus merely
reported.

A finding should include severity, the concrete failure mode, a bounded fix direction,
a way to validate the fix, and the evidence supporting confidence.

## Failure diagnosis

| Symptom | Meaning | Action |
|---|---|---|
| Job remains queued without a runner | Regression: the workflow returned to unavailable custom labels | Restore `runs-on: ubuntu-24.04` |
| `gh: not found` | GitHub CLI bootstrap or PATH wiring failed | Check the pinned GitHub CLI install step |
| Droid installer uses `curl ... | sh` | Regression: the safe action did not receive the pinned executable path | Check `path_to_droid_executable` |
| Droid version differs from `0.219.0` | Pin or auto-update control failed | Check checksum verification and `FACTORY_DROID_AUTO_UPDATE_ENABLED=false` |
| MiniMax dashboard shows no matching call | BYOK settings were not loaded or inference was never reached | Check the temporary settings file and action logs |
| Review uses a model other than MiniMax M3 | Model selector regression | Check all three `custom:MiniMax-M3-0` inputs |
| Debug artifact appears | Secret-bearing diagnostic output escaped the intended boundary | Stop the lane and investigate |
| Permission or exact-head check fails | Invocation is unauthorized or stale | Reinvoke from the current open PR head with a current writer |
