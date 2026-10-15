# Immutable workflow source evidence

Current consumer source candidate selects public PR #22 merge
`752fad709cf3da63a69d004af2867b0e77ad0af2`, independently fetched from
EffortlessMetrics/em-ci-workflows at the immutable reference. The exact
`rust.yml` blob is `a01c520d215445bcf293beb18a4ee4ca98df6409`, retained
verbatim in `em-ci-rust-752fad70.yml`; SHA256 is
`192a7202bfc2caee4a6ee66f57a352f85a1b8197106c538b8fb354525ea3cac0`.
Independent PR #22 readback confirms base main, exact merged commit and
merged_at `2026-10-15T00:45:40Z`, supporting projection `main (2026-10-15)`.
The parent still owns live allowance/qualification approval; source wiring
is not a dispatch or a protection transition. The older source below remains
recovery evidence, not the selected candidate callee.

em-ci-rust-a3125de9.yml was independently fetched from
https://github.com/EffortlessMetrics/em-ci-workflows/blob/a3125de962a74e7ec2e127d41d7532f66c69334d/.github/workflows/rust.yml
on 2026-10-02 using the connected GitHub file reader and independently
retrieved again at the same full SHA on 2026-10-14.

The returned Git blob is 6d202cfd6861bb359381dfb128c1574c04505285. The local Git
blob hash matches; the SHA-256 of the exact bytes is
275203f28721f1b209b0b09695392e9996c3717981c1eb56ab7a412d483e22fc.
The copy is read-only evidence for the existing static enforcement validator;
GitHub executes the workflow at the caller's public full-SHA reference.

A self-consistent modified copy and digest do not prove remote provenance. Each
pin update requires independent retrieval/review against the immutable upstream
reference and native context/app/subject qualification. Do not edit the vendored
source to alter runtime behavior or replace it with a hand-written summary.

The caller/ledger branch projection `main (2026-10-02)` is backed by upstream
[distribution PR #17](https://github.com/EffortlessMetrics/em-ci-workflows/pull/17):
its base is `main`, merge commit is the exact pinned
`a3125de962a74e7ec2e127d41d7532f66c69334d`, and `merged_at` is
`2026-10-02T05:01:01Z`. An independent 2026-10-14 comparison against upstream
main reports that pinned commit as merge base, zero behind and one ahead.
This is a historical branch-commit projection, not a moving branch reference.
The action-pin authority compares action, SHA, projection kind and exact
comment/value text; explanatory prose stays outside the uses-line projection.
