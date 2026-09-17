# Release artifact size shadow lane

Manual-only, read-only same-SHA A/B measurement of safe ICF on the native macOS
release artifacts. Controlling issue: [#5432][issue].

The lane exists so that adopting Rust's bundled Mach-O LLD with `--icf=safe` is a
measured, reversible decision about *this* repository's binaries rather than a
linker trick copied from another project's blog post. The workspace already
builds releases with `opt-level = 3`, full LTO, one codegen unit and stripped
debuginfo, and the release workflow strips both binaries again, so the remaining
ICF opportunity may well be negligible. **A no-win result is a successful
experiment, not a failed lane.**

## What the lane does

`.github/workflows/release-artifact-size-shadow.yml` is dispatched manually for
one governed target at a time. On that target's native runner it:

1. resolves the measurement subject and refuses to continue if the runner host is
   not the requested triple;
2. builds `perllsp` and `perl-dap` with no linker policy at all (the baseline);
3. stages, strips, packages and smokes those exact binaries, retaining one LSP
   and one DAP receipt per variant;
4. deletes the release build directory so the candidate is genuinely relinked;
5. repeats the build with exactly
   `-C linker=rust-lld -C linker-flavor=ld64.lld -C link-arg=--icf=safe`;
6. runs `release_artifact_size`, which emits the decision receipt.

Everything is staged under the gitignored `target/` tree. The instrument records
`subject_complete` only for a clean checkout, so a staging directory inside the
working tree would make every measurement `not_proven`.

## Running it

Dispatch `Release Artifact Size Shadow` from the Actions tab, once per target:

| Target | Native runner |
| --- | --- |
| `aarch64-apple-darwin` | `macos-14` |
| `x86_64-apple-darwin` | `macos-15-intel` |

The target is the only input. In particular there is no "repeat confirmed"
checkbox: `--repeat-confirmed` is promotion authority — it lets the instrument
treat a borderline 0.5%–1.0% reduction as confirmed — and a dispatcher ticking
a box is not evidence that a second measurement happened. A single run cannot
testify that it ran twice, so this lane never passes the flag and a borderline
win resolves to `not_proven` naming the unmet repeat requirement. Confirming
such a result is a separate, deliberate act performed with two receipts
actually in hand.

The receipts are uploaded as the `release-artifact-size-<target>` artifact and
the Markdown summary is written to the job summary.

## Reading the receipt

`release_artifact_size.v1` resolves each target independently:

| Recommendation | Meaning |
| --- | --- |
| `adopt` | Valid evidence and a material win: at least 50 bp *and* 128 KiB combined reduction, neither binary growing past 25 bp / 32 KiB, with the repeat requirement satisfied. |
| `do_not_adopt` | Valid evidence, no material win. The experiment succeeded; the answer is no. |
| `reject` | Candidate behaviour, package identity, or structural parity failed. |
| `not_proven` | The evidence or environment could not support a decision — cross-built host, unclean tree, missing or unbound smoke receipt, ungoverned triple, or declared flags that do not match what was built. |

`not_proven` is never a near-miss for `adopt`. Read the receipt's `limitations`
array: it names the exact fact that was missing.

## Claim boundary

The receipt proves a same-SHA post-strip size comparison plus a packaged-binary
LSP and DAP smoke on one native macOS target. It does **not** prove a startup
time or RSS improvement, does not speak for Linux or Windows, and confers no
release, publication, or support authority. Adoption — scoping the safe-ICF
flags to a proven row of `release.yml` — is a separate change made from these
receipts.

## Why the lane is shaped this way

Two failure modes would produce a confident but wrong receipt, so both are
closed structurally rather than by convention:

- **The candidate is compared against the baseline's runtime evidence.**
  `xtask lsp-ux-smoke` writes to one fixed path. `scripts/ci/release_artifact_size_smoke.sh`
  clears that path before each run and fails if it is not recreated, and the
  instrument independently requires each receipt's `binary` field to name the
  measured binary.
- **The two variants do not actually differ by the linker flags.**
  The baseline declares empty `RUSTFLAGS`, the candidate declares exactly the
  governed string, and the instrument rejects a measurement whose declared flags
  disagree with the policy.

Because the lane never runs on a pull request, its contract is proven by
`xtask/tests/release_artifact_size_shadow_workflow.rs` and
`xtask/tests/release_artifact_size_stage_script.rs`, which bind the workflow and
the staging adapter to the instrument's own constants in
`xtask/src/bin/release_artifact_size/policy.rs`.

The lane is deliberately absent from `policy/ci-lane-whitelist.toml`: it carries
no per-PR cost and is allowlisted in `ALLOWLIST_WORKFLOW_LANE_MISSING` alongside
the other release-time and utility workflows.

[issue]: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/5432
