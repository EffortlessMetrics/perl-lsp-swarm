# Dependency Review

`Dependency Review` is the pull-request delta gate for third-party dependency changes.

It answers one question:

> Does this pull request introduce or update a dependency with a high-or-critical known vulnerability or a license outside the repository allowlist?

## Trigger surface

The workflow runs only when a pull request changes:

- Cargo manifests or `Cargo.lock`;
- the VS Code extension package manifest or lockfile;
- the dependency-review policy or workflow itself.

The job is read-only and does not post or update pull-request comments. Findings remain in the check summary and logs.

The policy file is fetched from the pull request base commit, so the evaluated
head cannot weaken the rule that evaluates it. The checked-out head is still
used as the dependency-review action's comparison input.

## Policy

`.github/dependency-review-config.yml` sets:

- failure threshold: **high** severity;
- scopes: runtime, development, and unknown;
- license allowlist aligned with `deny.toml`;
- dependency-snapshot retry for workflows that submit dependency metadata shortly after the pull-request head appears.

An undetected license is reported by the upstream action but is not treated as a clean license proof. `cargo deny` remains the authoritative whole-Cargo-graph license and source check.

## Authority map

| Concern | Authority |
|---|---|
| Dependency delta introduced by the PR | GitHub Dependency Review |
| Cargo licenses, sources, bans, duplicate policy | `cargo deny` / `deny.toml` |
| Known Rust advisories in the checked-out graph | `cargo audit` |
| Repository, lockfile, image, configuration, and secret scanning | Trivy security workflow |
| Automated update proposals and advisory remediation | Dependabot |

Dependency Review does not replace any whole-graph scanner. It makes the proposed change legible before it merges.

## Failure handling

A failed API request, unavailable dependency graph, invalid configuration, or action crash fails the job. It must not be represented as a clean dependency delta.

## Local reproduction

The GitHub dependency-review API is event- and repository-backed, so the exact check is remote-only. Reproduce the underlying policy locally with:

```bash
cargo deny --locked check advisories licenses bans sources
cargo audit
npm --prefix vscode-extension audit
```

Those commands inspect current graph state; they do not reproduce GitHub's base-to-head dependency delta.
