# Non-release reconciliation preflight

This composition belongs to #17226. It keeps `sync-divergence` v2 unchanged;
the source-profile/public-admission owners remain #17225 and public #10121.

`scripts/source_reconciliation.py` independently inventories every public commit
after B, including shared history and merge ancestry. It records each commit's
complete changed-path set and exact current R/S entry modes and object IDs.
For each ordered two-parent merge it computes an automatic `merge-tree` baseline
in a temporary bare repository that reads the existing object store through an
alternate. It disables ambient configuration and replacement refs, writes no
objects to the source repository, and refuses unavailable or malformed proof.
Every path whose actual merge entry differs from that complete baseline, plus
every conflicted baseline path, is a resolution obligation. This includes side
work discarded back to the first parent even when the first-parent diff is empty;
clean same-file composition alone creates no resolution obligation. The packet
binds the Git version, ordered parents, baseline tree and exact current R/S entries.
`git cherry S R B` contributes patch evidence but never determines the
whole population or proves current survival. Lazy blob fetching is disabled.
Replacement refs are disabled on each Git call; legacy graft files and graft
environment overrides are rejected before deriving original ancestry.
Missing historical blobs, a shallow graph, a foreign/stale native receipt, an
omitted work unit, or a displaced port produces `not_proven`, never empty success.

The explicit source vocabulary maps to the native five tokens through
`PRIMITIVE_MAP`. Public-context translation remains outside the native vocabulary;
it is never relabelled lineage-only to pass its product guard. It requires an
actual projection tree plus exact per-path row IDs, row digests and Git entries.
P must be a literal immutable tree object SHA; branch names, HEAD and commit
objects cannot stand in for that tree identity.
Only the four named public admission controls have an explicit control-test
classification. There is no global product/test exemption. Mixed Rust/runtime
units require a source repair or qualified semantic disposition.
Lineage-only admission is limited to the explicitly reviewed historical document
paths in `LINEAGE_ONLY_PATHS`; a filename extension or directory cannot exempt
executable source, configuration, controls or active guidance.

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
The output parent must already exist and belong to the sole filesystem writer.
Creation uses no-follow directory descriptors on POSIX and a handle-relative
`NtCreateFile` on Windows; it does not create parent directories. Windows stream,
device and ambiguous filename aliases are rejected. These checks prevent link
substitution; they do not authenticate filesystem ownership or protect against
an actor relocating directories with permission to modify Git metadata.
Windows name parsing follows the
[documented no-reparse object attributes](https://learn.microsoft.com/en-us/windows/win32/api/ntdef/ns-ntdef-_object_attributes)
and [handle-relative file creation](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile).

The packet's `pass` means this structural reconciliation preflight passed. Its
explicit acceptance ceiling does **not** establish authenticated producer origin,
executed product behavior, complete-tree projection ownership, trusted-base source
admission, protected merge acceptance, or release authority. Reviewed semantic
judgments remain inputs, not independently authenticated facts. #17225/#10121 must
consume the exact packet digest and establish those additional applicable source
obligations independently; they must not admit a join from a verdict string alone.

Ports must be reachable from the final exact S and their credited current bytes
must survive. A port must actually change every credited original path; absence
alone is not a port. Credited deletions require an existing first-parent entry.
Source proof commits also require literal immutable commit SHAs.
The scaffold uses `source_reconciliation_ledger.v2`. Every merge retains its
`merge_ancestry` work unit and additionally requires one exact
`merge_resolution_dispositions` entry for each baseline difference or conflicted
path. Omitting, duplicating or substituting an
effect cannot pass. Each effect uses the same current-source port/equivalence,
product guard, architecture-successor and projection binding rules as ordinary
work. A null effect disposition leaves that merge unresolved. This records
reviewed semantic decisions; it does not establish their authority independently.
Historical v1 ledgers remain readable and still refuse all merge resolution
effects. They are never silently upgraded or granted an ancestry-only exception.
A new S/R/P changes the packet and requires fresh bindings. Preserve
historical ledgers and original commits. Source sync uses ordered two-parent merges
under existing protections; squash/rebase is not a valid substitute.
