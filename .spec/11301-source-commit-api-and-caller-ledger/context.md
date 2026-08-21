# Context: #11301 — source-commit API and caller ledger

Issue #11298 landed at the accepted base and made parsed candidates private until
the document/index commit seam. The remaining ambiguity is that `index_file` and
`index_files_batch` currently use generation zero for both initial discovery and
live work. Generation zero has no currentness identity, so it cannot reject a
late live candidate.

This candidate separates the names and contracts without taking ownership of
didOpen/didSave or other lifecycle authorities. Initial discovery/import uses
`index_initial_file`, `index_initial_file_str`, and `index_initial_files_batch`.
Live callers use `SourceCommit`, which requires owner-supplied non-zero opaque
identity and generation values, and receive typed accepted/no-op/stale/failure
outcomes. The old surfaces remain only as ledgered compatibility bridges while
their callers migrate in later bounded claims.

`index_files_batch` is load-bearing: the initial batch API delegates to the
landed private-candidate batch implementation so parsing remains private and the
single rebuild behavior is preserved.
