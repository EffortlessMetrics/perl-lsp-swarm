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
- Check out the event's exact pull-request head SHA so the policy file and
  workflow inputs are from the evaluated head.
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
dependency-graph endpoints were unavailable. That is an external prerequisite,
not evidence that the dependency delta is safe. The PR remains draft until an
administrator enables the repository dependency graph/code-security capability
and a benign manifest delta proves a real comparison.

## Non-goals

- No dependency update or lockfile churn.
- No replacement of cargo-deny, cargo-audit, npm audit, Trivy, or Dependabot.
- No whole-graph scan disguised as delta proof.
- No green fallback when GitHub dependency data is unavailable.
- No required-check/ruleset migration in this slice.

## Acceptance

- [ ] Cargo and extension dependency deltas trigger the workflow.
- [ ] High/critical newly introduced advisories fail the action.
- [ ] License policy matches `deny.toml` without creating a second authority.
- [ ] Pinned action, least privilege, exact-head checkout, and fork behavior are
      structurally proven.
- [ ] A deliberate benign dependency delta produces a real base-to-head result.
- [ ] Unavailable dependency graph data fails explicitly as `NOT_PROVEN`.
