# Verification

`perl-lsp` has three verification surfaces:

- README badges are public, repo-scoped trust markers.
- Pull request evidence is diff-scoped reviewer and agent feedback.
- Release evidence is shipped-truth proof for public version handoff.

Badges are the front panel. Generated evidence, CI receipts, and release artifacts remain the source of truth.

## README badges

### `ripr+`

`ripr+` is a repo-scoped static evidence badge. It counts unresolved static exposure gaps plus actionable test-efficiency findings under repository policy when `ripr` can produce the repo-scoped endpoint.

It is an inbox-zero signal, not coverage, runtime mutation proof, or correctness proof. Diff-scoped `ripr` artifacts belong in pull request summaries and CI artifacts, not public README badges.

If the badge endpoint reports `unavailable`, the endpoint generator could not obtain compatible repo-scoped `ripr+` output from the installed `ripr` binary and leaves the public marker neutral instead of publishing diff-scoped evidence as a repo badge.

### Release

The release badge shows the latest GitHub release. GitHub releases are the public version surface for this repository; crates.io downloads and docs.rs remain registry and documentation surfaces.

## Regeneration

Regenerate public badge endpoints:

```bash
rtk cargo xtask badges
```

Check committed endpoint drift:

```bash
rtk cargo xtask badges --check
```

Committed endpoint files live under `badges/`. Detailed reports stay under `target/` locally or in CI artifacts.

## Pull Request Evidence

Pull requests run `ripr` evidence, impacted evidence, fast gates, docs-sync,
publish preflight, example smoke checks, and targeted mutation when routing
rules require it. RIPR evidence is blocking for new severe PR gaps through
`rtk cargo xtask quality-gate --mode enforce-new-ripr ... --check`; existing
repo-wide RIPR+ debt remains on the burn-down path until the total-zero gate is
promoted.

The portable `ripr` surface is:

```bash
rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD
rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json
rtk cargo xtask ripr-review-comments --base origin/HEAD --head HEAD
rtk cargo xtask impacted-evidence
rtk cargo xtask ripr-pr-summary
rtk cargo xtask ripr-annotations
rtk cargo xtask ripr-pr --base origin/HEAD --head HEAD --check
rtk cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json --check
rtk cargo xtask ripr-review-comments --base origin/HEAD --head HEAD --check
rtk cargo xtask impacted-evidence --check
rtk cargo xtask ripr-pr-summary --check
rtk cargo xtask ripr-annotations --check
rtk cargo xtask quality-gate --mode enforce-new-ripr --ripr-receipt target/receipts/quality/ripr-plus.json --ripr-pr-receipt target/ripr/pr/repo-exposure.json --review-receipt target/ripr/review/comments.json --coverage-receipt target/receipts/quality/coverage-baseline.json --codecov codecov.yml --exceptions policy/quality-gate-exceptions.toml --receipt target/receipts/quality/quality-gate.json --summary target/receipts/quality/quality-gate.md --check
```

The default PR evidence base is `origin/HEAD`, so local proof follows the
repository's configured default branch. CI passes the pull request base
explicitly as `origin/${{ github.base_ref }}`. Diff-scoped RIPR receipts carry
both `base_sha` and `head_sha`; `quality-gate` rejects missing SHA fields and
marks receipts stale when the recorded base ref resolves to a different commit.

These commands write:

```text
target/ripr/pr/repo-exposure.json
target/ripr/pr/repo-exposure.md
target/ripr/pr/summary.md
target/ripr/review/comments.json
target/ripr/review/comments.md
target/ripr/review/annotations.txt
target/xtask/impacted-evidence/latest.json
target/xtask/impacted-evidence/latest.md
target/receipts/quality/ripr-plus.json
target/receipts/quality/quality-gate.json
target/receipts/quality/quality-gate.md
```

`ripr` may suggest focused tests or route targeted mutation. It does not edit
code, generate tests, run mutation, or make merge decisions by default.

Line-placeable review guidance is emitted as non-blocking warning annotations
from `comments[]` only. The aggregate quality-gate markdown summary carries the
PR proof block, blocking failure list, exact file/line/seam/gap id, suggested proof,
verify command, and receipt command. Summary-only findings stay in summaries and
artifacts; inline PR comments are disabled by default.

Pull request artifacts and summaries are diff-scoped. They must not be reused as repo-scope README badges.
