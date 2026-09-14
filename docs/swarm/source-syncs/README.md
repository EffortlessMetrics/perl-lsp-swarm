# Publication-sync transaction artifacts

This directory holds immutable or release-bound evidence for one
`perl-lsp-swarm` → `perl-lsp` publication-sync transaction.

Stable policy does **not** live here. Read:

- [`../sync-protocol.md`](../sync-protocol.md) for the stable contract;
- [`../../how-to/PUBLICATION_SYNC.md`](../../how-to/PUBLICATION_SYNC.md) for the operator procedure.

## Why these files exist

Exact release identities change every release. Keeping them in the stable
protocol creates stale instructions and encourages operators to refresh prose
merely because a SHA moved.

Per-release artifacts instead bind the transaction to exact subjects:

```text
B  completed reconciliation boundary
R  publication-repository base
S  prepared swarm commit
P  projected publication tree
J0 core history-preserving join
J  publication-sync PR head carrying control files
M  landed publication-repository wrapper merge
```

## Recommended naming

Use the release identifier in filenames. For example:

```text
docs/swarm/source-syncs/v0.18.0-reconciliation.json
docs/swarm/source-syncs/v0.18.0-publication-sync.json
docs/swarm/source-syncs/v0.18.0-sync-closeout.json
```

Historical Markdown receipts in this directory remain evidence of earlier
transactions; they are not templates or current exclusion policy.

## Reconciliation artifact

Generated/validated by `cargo xtask sync-divergence`.

It records at least:

- exact source, target and boundary identities;
- the patch-equivalent target-unique population;
- changed paths;
- one terminal disposition per row;
- evidence and exact source-equivalent/port identities where required;
- excluded merge ancestry;
- blocking decisions;
- population digest and terminal verdict.

The final release reconciliation is accepted only when blocker-free and bound
to the final prepared `S` and current release base `R`.

## Publication projection artifact

This is the reviewed input that turns the complete prepared swarm tree into the
publication-context tree `P`.

It records:

- exact `R` and `S`;
- reconciliation/topology/notes/API/public-claims/integrity digests selected by
  the release;
- every translation/exclusion/preservation operation;
- source/base/expected-public digests for changed paths;
- destination-context and cross-file invariant evidence;
- exact expected projected Git tree;
- blockers and `NOT_PROVEN` state where applicable;
- the no-publish boundary.

Every intentional tree difference from `S` must be represented. A historical
exclusion from another release is not authority.

## Publication-repository control packet

The actual `perl-lsp` sync PR commits three release-bound files at:

```text
.github/publication-sync/packet.yaml
.github/publication-sync/reconciliation-ledger.json
.github/publication-sync/projection-manifest.json
```

The live `Publication Sync Contract` owns their exact schema. Do not copy a
schema or packet from this directory as a substitute for the release
repository's current validator/schema.

The current protected check uses a core join `J0` because a commit cannot
contain its own SHA. The packet's `sync_join_sha` names `J0`; the PR head `J`
adds the three control files while retaining the same ordered parents and
commit identity. See the operator runbook for construction.

## Closeout artifact

After merge, retain enough information to reconstruct what actually landed:

```yaml
release: ...
reconciliation_boundary: B
release_base_sha: R
prepared_swarm_sha: S
expected_projected_tree: P
core_sync_join_sha: J0
pr_head_join_sha: J
landed_release_sha: M
reconciliation_digest: sha256:...
publication_sync_manifest_digest: sha256:...
publication_sync_contract: pass
post_merge_verification: pass
published_channels: []
release_cut: false
```

Downstream no-publish rehearsal consumes exact `M`.

## Artifact rules

1. Mutable branch names are context, never the transaction identity.
2. Do not edit an accepted artifact in place after a subject/digest changes;
   regenerate the affected attempt and make invalidation visible.
3. Missing, stale, skipped, cancelled, timed-out, malformed, cross-subject or
   instrument-failed required evidence is not success.
4. Generated output should be byte-deterministic for identical inputs.
5. Public channel mutation is outside this directory's transaction.
