# RIPR 0.10 check-output fixtures

These files are byte-for-byte copies of checked-in golden output from the
published `EffortlessMetrics/ripr` `v0.10.0` tag at commit
`c08d474a92d0edf97ad1e3d444ecb73fc5d439de`.

- `comment-only-check.json` comes from
  `fixtures/comment_only_diff/expected/check.json` (Git blob
  `0f55990773a20735aa5221f8a2b9959d21bc67c8`).
- `boundary-gap-check.json` comes from
  `fixtures/boundary_gap/expected/check.json` (Git blob
  `2a4d74b37c6d4a89638e8059de7a6080c2ca0f94`).

They lock the reviewed producer's `schema_version = "0.2"`, required summary
counts, empty-output shape, and actionable-finding shape. Tests derive malformed
and stale-schema negative controls from these producer-shaped inputs rather than
claiming those mutations were emitted by RIPR.
