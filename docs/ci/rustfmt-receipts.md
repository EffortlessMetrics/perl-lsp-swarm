# Candidate-bound rustfmt receipt paths

`scripts/ci/rustfmt_check.py` produces `rustfmt_check.v1` evidence;
`scripts/ci/verify_rustfmt_receipt.py` independently validates it. The normal CI
workflow invokes both before uploading the receipt.

Both commands interpret a relative `--receipt` from the selected `--root`, even
when invoked from another working directory. An absolute `--receipt` continues
to select that explicit file. The verifier checks the selected file for a leaf
symlink before resolving it; existing regular-file, size, candidate SHA/tree,
clean-worktree, digest, tool-version and Cargo-inventory checks still apply.
This rule changes receipt selection only; it adds no new allowed input paths or
receipt identity claims. Existing parent-symlink and absolute-path policy is
unchanged.

At source base `f8821cbc0306da72886f5fee24d89d7411b26483`, verification resolved a
relative receipt against the process directory while production used the root.
A same-named foreign receipt could therefore replace the producer-selected file.

Focused regression command:

```bash
python3 -B -m unittest scripts.ci.test_verify_rustfmt_receipt
```

The regression uses a real committed fixture worktree and canonical receipts with
the existing synthetic tool responses. It verifies valid root evidence despite a
foreign stale or missing file, refuses stale root evidence despite a foreign valid
file, and refuses a root-selected leaf symlink. These controls fail on the source
base. They do not establish native Windows, actual formatter execution, hosted
qualification or merge readiness.
