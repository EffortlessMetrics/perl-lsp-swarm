# Non-release reconciliation preflight

This composition belongs to #17226. It keeps `sync-divergence` v2 unchanged;
the source-profile/public-admission owners remain #17225 and public #10121.

`scripts/source_reconciliation.py` independently inventories every public commit
after B, including shared history and merge ancestry. It records each commit's
complete changed-path set and exact current R/S entry modes and object IDs.
For merges it compares first-parent effects with side work and the side-head
entries. `git cherry S R B` contributes patch evidence but never determines the
whole population or proves current survival. Lazy blob fetching is disabled.
Missing historical blobs, a shallow graph, a foreign/stale native receipt, an
omitted work unit, or a displaced port produces `not_proven`, never empty success.

The explicit source vocabulary maps to the native five tokens through
`PRIMITIVE_MAP`. Public-context translation remains outside the native vocabulary;
it is never relabelled lineage-only to pass its product guard. It requires an
actual projection tree plus exact per-path row IDs, row digests and Git entries.
Only the four named public admission controls have an explicit control-test
classification. There is no global product/test exemption. Mixed Rust/runtime
units require a source repair or qualified semantic disposition.

Use exact SHAs, a complete common graph and a fresh output path:

```text
python scripts/source_reconciliation.py --repo <common-repository> \
  --source <S> --boundary <B> --target <R> \
  --primitive-receipt <native-sync-divergence-v2-receipt.json> \
  --ledger <reviewed-source-ledger.json> --projection-tree <P> \
  --receipt <new-source-reconciliation-packet.json>
```

Omit `--ledger` and provide `--scaffold <new-ledger.json>` for an honest unresolved
inventory. Omit unavailable producer evidence rather than creating dummy pass
fields. Output is still emitted with its independently derived population and
errors. Existing packets and input files are never overwritten, and Git metadata
in ordinary or linked worktrees is not an output destination.

The packet's `pass` means this structural reconciliation preflight passed. Its
explicit acceptance ceiling does **not** establish authenticated producer origin,
executed product behavior, complete-tree projection ownership, trusted-base source
admission, protected merge acceptance, or release authority. Reviewed semantic
judgments remain inputs, not independently authenticated facts. #17225/#10121 must
consume the exact packet digest and establish those additional applicable source
obligations independently; they must not admit a join from a verdict string alone.

Ports must be reachable from the final exact S and their credited current bytes
must survive. A new S/R/P changes the packet and requires fresh bindings. Preserve
historical ledgers and original commits. Source sync uses ordered two-parent merges
under existing protections; squash/rebase is not a valid substitute.
