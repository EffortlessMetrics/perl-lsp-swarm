# Issue #4146 — PR Dependency Delta Review

## Decision

Salvage the existing draft PR #4155 as one bounded CI-policy slice. The lane
adds GitHub's dependency-review action for pull requests that change supported
dependency manifests or lockfiles. It does not replace the repository's
whole-graph scanners.

## Authority and claim boundary

GitHub Dependency Review is authoritative only for the base-to-head dependency
delta exposed by GitHub's dependency graph. `deny.toml`/cargo-deny remain the
Cargo source, license, ban, and duplicate authorities; cargo-audit remains the
Rust advisory scanner; npm audit covers the VS Code extension graph; Trivy and
Dependabot retain their existing repository and remediation roles.

The workflow must fail when the dependency-review instrument cannot obtain a
supported dependency snapshot. An unavailable graph, API error, malformed
configuration, fork-data limitation, or action failure is `NOT_PROVEN`; it is
never converted into a clean result by a fallback scanner or `continue-on-error`.

## Contract

- Trigger only for pull requests targeting `main` whose changed paths include
  Cargo manifests/lockfiles, the VS Code extension manifest/lockfile, or this
  workflow/configuration.
- Check out the event's exact pull-request head SHA so dependency graph inputs
  are from the evaluated head; separately sparse-check out the policy file from
  the protected base commit so the evaluated head cannot weaken the gate.
- Run `actions/dependency-review-action` v5 at an immutable commit with
  `contents: read` only and no PR-comment write permission.
- Block newly introduced high or critical vulnerabilities in runtime,
  development, and unknown scopes.
- Enforce the SPDX allowlist aligned with the current `deny.toml` policy.
- Keep snapshot retry bounded and report unavailable dependency data as a
  failed check.

## Verification

Local proof validates YAML/workflow structure, action pinning, policy syntax,
license alignment, path routing, and documentation. The live dependency delta
proof is remote-only because it depends on GitHub's base/head graph API.

The first exact-head run of PR #4155 reached the pinned v5 action and failed
with `Dependency review is not supported on this repository`; the live
dependency-graph endpoints were unavailable. That was an external prerequisite,
not evidence that the dependency delta was safe. After enabling only the
repository Dependency Graph setting, the exact-head rerun passed on commit
`a4686401c9c7eba369f1bb849e4a7e3b2870524b` (run `29618886582`). The current PR
changes only workflow, policy, documentation, and spec surfaces, so it does
not intentionally introduce a new dependency or vulnerable fixture. The
remote action did perform a real graph comparison and passed; an adverse
high/critical fixture remains intentionally unintroduced to avoid adding
security debt solely for test data.

## Non-goals

- No dependency update or lockfile churn.
- No replacement of cargo-deny, cargo-audit, npm audit, Trivy, or Dependabot.
- No whole-graph scan disguised as delta proof.
- No green fallback when GitHub dependency data is unavailable.
- No required-check/ruleset migration in this slice.

## Acceptance

- [x] Cargo and extension dependency deltas trigger the workflow by path scope.
- [x] High/critical newly introduced advisories are configured to fail the
      action; no intentionally vulnerable fixture is introduced in this lane.
- [x] License policy matches `deny.toml` without creating a second authority.
- [x] Pinned action, least privilege, exact-head checkout, protected-base policy,
      and path routing are structurally proven; fork limitations remain
      fail-closed by the action.
- [x] A real base-to-head graph comparison passed on the exact PR head.
- [x] Unavailable dependency graph data failed explicitly as `NOT_PROVEN` in
      the pre-enable run.
