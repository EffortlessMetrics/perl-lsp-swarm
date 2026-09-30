# Workflow Policy Lint

`cargo xtask workflow-policy-lint` validates every workflow under
`.github/workflows/` against a set of policy rules. It is the existing
mechanism for catching unsafe `pull_request_target` patterns, write-all
permissions, blanket cancel-in-progress, and similar foot-guns.

PR 11 of the CI economics rollout extends the linter with an opt-in
**lane-whitelist** check.

> Companion: [policy-ledgers.md](policy-ledgers.md), [perl-lsp-rollout-plan.md](perl-lsp-rollout-plan.md).

---

## Existing rules (errors unless noted)

| Code | Meaning |
|---|---|
| `PR_TARGET_CHECKOUT_HEAD` | `pull_request_target` workflow checks out PR head (unsafe) |
| `WRITE_ALL_PERMISSIONS` | `permissions: write-all` declared |
| `PR_CONTENTS_WRITE` | `pull_request` workflow requests `contents: write` (not allowlisted) |
| `UNTRUSTED_PR_SECRETS` | Untrusted PR code path consumes `secrets.*` |
| `REQUIRED_STYLE_MISSING_MERGE_GROUP` | Required-style workflow missing `merge_group` trigger |
| `REQUIRED_STYLE_SELF_FILTERED` | Required-style workflow path-filters itself |
| `BLANKET_CANCEL_IN_PROGRESS` | `cancel-in-progress` not gated for master/merge_group truth runs |
| `LABEL_EVENT_CANCELS_PR_RUN` | `pull_request labeled`/`unlabeled` workflow cancels in-progress runs |
| `UNPINNED_ACTION` | Third-party action not pinned to a commit SHA (warning) |

---

## New: `LANE_WHITELIST_MISSING` (advisory)

```bash
cargo xtask workflow-policy-lint --check-lane-whitelist
```

For each workflow under `.github/workflows/*.yml`, the linter checks whether
[`policy/ci-lane-whitelist.toml`](../../policy/ci-lane-whitelist.toml) has at
least one `[[lane]]` entry whose `workflow` field matches that file.

If neither a whitelist entry nor an `ALLOWLIST_WORKFLOW_LANE_MISSING`
allowlist entry covers the workflow, the linter emits a **warning** with
code `LANE_WHITELIST_MISSING`.

The check is **advisory only** — warnings, not errors. The `passed` status
in the receipt is unaffected. Promotion to error level is intentionally
deferred until the whitelist has stabilized.

### Allowlist

`ALLOWLIST_WORKFLOW_LANE_MISSING` in
[`xtask/src/tasks/workflow_policy_lint.rs`](../../xtask/src/tasks/workflow_policy_lint.rs)
covers release/utility workflows that are not part of the per-PR economics
map. Initial entries:

- Release/publish: `release.yml`, `publish-*.yml`, `docker-publish.yml`,
  `*-bump.yml`, `release-orchestration.yml`, `post-publish-smoke.yml`,
  `vscode-published-extension-smoke.yml`
- Post-merge utilities: `post-merge-corpus-ratchet.yml`,
  `post-merge-status.yml`, `docs-deploy.yml`
- Schedule-only / housekeeping: `tokmd.yml`,
  `triage-issues.yml`, `version-bump.yml`, `ci-gate-self-tests.yml`,
  `workflow-trigger-lint.yml`

To add a new workflow that should be exempt, add a constant entry there and
explain the reason in the commit message.

---

## When to enable in CI

The lane-whitelist check is currently invoked only when explicitly passed
the flag. To enable it on PRs, add to `.github/workflows/workflow-policy.yml`:

```yaml
- name: Run workflow policy lint
  run: cargo xtask workflow-policy-lint --check-lane-whitelist --receipt target/receipts/workflow-policy.json
```

Recommended approach: leave advisory-only first, observe the warning rate
on real PRs, then promote to error once the whitelist + allowlist are
stable.

---

## Output

### Selected subject and unavailable inputs

Use `cargo xtask workflow-policy-lint --root <checkout>` when invoking a cached
or relocated binary against a particular checkout. Without `--root`, the command
retains its compile-time project root; it does not silently substitute the current
working directory. Repository mode requires a usable root and a readable inventory
containing at least one `.yml` or `.yaml` file. Selected files must be readable and
parseable by the existing linter. This does not add workflow-schema validation:
blank or nonmapping YAML retains the existing rule coverage.

`--check-lane-whitelist` requires its policy file to exist, read, and parse; actual
lane findings remain advisory. The isolation registry remains optional: absence
does not clear self-hosted PR jobs, which still require declared profiles.
`--fixture` evaluates only that file without consulting a repository root, and
conflicts with both `--root` and `--check-lane-whitelist`.

Receipt version `1.0.0` and existing fields are retained. Additive `subject`
metadata records `mode`, `selection`, `path_identity_sha256`,
`workflow_file_count`, `scan_completed`, and which repository checks were requested.
The receipt schema accepts legacy receipts without `subject`; when present, its
fields are required and strictly typed, and unknown fields remain rejected.
The count is evaluated files, not validated workflow contracts. The identity hashes
the canonical selected path's OS-native bytes, not file contents or a source commit;
it avoids recording an absolute private path and is not a portable artifact digest.
Fixture receipts do not establish repository-wide coverage. `scan_completed` means
the selected checks finished, even when they found policy errors.

Input failures return nonzero and replace an explicitly requested receipt with
`passed: false`, incomplete scan metadata, and `WORKFLOW_POLICY_INPUT_UNAVAILABLE`.
If the filesystem refuses receipt replacement, the command returns nonzero but
cannot guarantee that an old file was removed. CLI argument errors occur before
scanning and likewise do not replace an existing receipt. Never accept a preexisting
receipt as a successful current invocation when its process failed; failed receipts
can still document policy or instrument errors. Receipt and fixture paths retain
their existing current-working-directory interpretation.

Focused proof:

```bash
cargo test -p xtask --bin xtask --locked workflow_policy_lint
cargo test -p xtask --test workflow_policy_root_cli --locked
```

The CLI regression copies the just-built binary and invokes it from an unrelated
directory against an explicit temporary root. A resolver-level control covers an
unavailable compile-time default. Neither test removes a live checkout or claims
execution of a binary whose actual build checkout was deleted.

Warnings appear as `::warning::` annotations in the GitHub workflow log.
The receipt JSON includes them in `issues[]` with `level: "warning"`,
`code: "LANE_WHITELIST_MISSING"`. The `passed` field of the receipt is
unaffected by warning-level issues.
