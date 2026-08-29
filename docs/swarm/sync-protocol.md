# perl-lsp publication sync protocol

`perl-lsp-swarm` is the active development source of truth.
It owns active product implementation, proof, release preparation, and the
current sync protocol.
`perl-lsp` is the release, history, and canonical package-lineage repo.
It is the publication repository and owns public release lineage and
publication-specific governance.

This document is the canonical stable contract for history-preserving
`perl-lsp-swarm` → `perl-lsp` release syncs. The copy of this document present
in `perl-lsp` is a snapshot promoted with the last sync; current protocol
authority remains here in swarm.

For the copy-safe operator procedure, use
[`docs/how-to/PUBLICATION_SYNC.md`](../how-to/PUBLICATION_SYNC.md). Exact SHAs,
digests, release names, translations, exclusions, and receipts belong in the
per-release transaction artifacts described in
[`docs/swarm/source-syncs/README.md`](source-syncs/README.md), not in this
stable protocol.

## Repository authority

| Repository | Authority |
| --- | --- |
| `perl-lsp-swarm/main` | Active development; product implementation, tests, compiler/LSP/DAP work, proof, freeze, release preparation, and current sync protocol |
| `perl-lsp/master` | Release lineage; publication-specific workflows/policy, public package lineage, and bounded emergency release fixes |

Normal product work starts and converges in swarm. Do not maintain parallel
implementation queues in both repositories.

An emergency product fix may begin in `perl-lsp` only when release safety
requires it. Mirror or supersede the product/test effect in swarm immediately
and invalidate any affected prepared-release evidence. Publication-repository
history may legitimately contain release-lineage-only changes that do not
belong in swarm.

## Sync-boundary invariant

The invariant is **not** “`perl-lsp/master` may never be ahead of swarm.”
Release lineage and publication governance can legitimately make it ahead.

At the final publication-sync boundary, every `perl-lsp`-unique product or test
change since the last completed reconciliation boundary must have a terminal,
evidence-backed disposition. There must be:

```text
zero unclassified release-repository product/test work
zero required product/test behavior that exists only in perl-lsp
zero unresolved architecture decision hidden inside an accepted sync ledger
```

The canonical reconciliation command is `cargo xtask sync-divergence`. Its
three subjects have distinct meanings:

```text
--source   exact swarm commit used for patch equivalence
--boundary last completed reconciliation boundary; history limit only
--target   exact perl-lsp release head being judged
```

The checker computes target-unique non-merge commits with the resolved source
as `git cherry`'s upstream and the boundary only as the lower history limit.
Its accepted terminal dispositions are:

```text
port_to_swarm
already_equivalent_in_swarm
superseded_by_newer_architecture
deliberately_abandoned
release_lineage_only
```

Unresolved decisions remain blockers; they are not a sixth successful
disposition. Required ports/equivalents must be reachable from the final
prepared swarm subject before the final ledger can pass.

## Release transaction identities

Use immutable identities once the final release transaction begins:

```text
R  = exact perl-lsp/master release base
S  = exact prepared swarm commit
P  = exact reviewed projected publication tree
J0 = core two-parent join: parents [R, S], tree P
J  = publication-sync PR head: audited join plus the committed control packet
M  = landed GitHub merge-commit wrapper on perl-lsp/master
```

The current protected publication check stores the packet under
`.github/publication-sync/`. Because a commit cannot contain its own SHA, the
packet's `sync_join_sha` identifies `J0`, the core join. The PR head `J` has the
same ordered parents and commit identity as `J0`, but its tree additionally
contains exactly these control files:

```text
.github/publication-sync/packet.yaml
.github/publication-sync/reconciliation-ledger.json
.github/publication-sync/projection-manifest.json
```

The live `Publication Sync Contract` re-derives `J0` from `J`, proves the
projection tree, and rejects extra control-directory content.

The expected protected-PR graph is therefore:

```text
               M
              / \
             R   J
                / \
               R   S

J minus the control directory re-derives J0
parents(J0) = [R, S]
tree(J0) = P
```

Do not collapse these identities into one ambiguous `release_repo_sha`.
Downstream release rehearsal consumes landed `M`; ancestry/projection proof
retains `J0` and `J`.

## Complete-tree rule

#### Mechanics: history-preserving complete-tree merge

The publication product tree starts from the complete prepared swarm tree.
Do not use a normal recursive/per-file merge as the product projection.

A previous release sync demonstrated the failure mode: per-file conflict
resolution mixed incompatible files inside `perl-dap`, producing missing-method
errors even though Git had resolved the text conflicts. The correct mechanism
records both parents while replacing the merge tree with the complete prepared
swarm tree before applying publication-specific projection.

Conceptually:

```bash
git merge -s ours --no-commit swarm/main
git read-tree -u --reset swarm/main
```

These stable topology markers use `swarm/main` to name the development source;
an actual release transaction replaces that moving ref with the pinned `S`:

```bash
git switch -c release/sync-vX.Y.Z "$R"
git merge --no-ff --no-commit -s ours "$S"
git read-tree -u --reset "$S"
# Apply only the reviewed publication projection.
# Add the exact publication-sync control packet.
git commit -m "release: history-preserving publication sync vX.Y.Z"
```

The exact per-release tooling/packet owns the concrete projection operations.
Do not restore historical files ad hoc during conflict resolution.

## Publication projection rule

The default publication tree is `S`. Every intentional deviation from `S`
must be declared before the join in the reviewed, digest-bound publication
projection manifest.

Typical classes include:

- repository or branch context;
- public links and issue references;
- public release wording and claims;
- publication-repository release lineage/governance;
- swarm-only development/control surfaces;
- generated publication context;
- installer/archive/VSIX identity composition.

Historical v0.17 exclusions such as particular agent directories or cleanup
scripts are evidence about that transaction only. They are **not** a permanent
exclude list. A current release reuses an exclusion only when the current
manifest explicitly owns and proves it.

No manifest exclusion may conceal current product or test divergence. Product
or test repair returns to swarm, lands there, and invalidates affected prepared
inputs.

## Protected landing

`perl-lsp/master` requires the repository-owned check named exactly:

```text
Publication Sync Contract
```

The live implementation is read-only and has two modes:

```text
ordinary PR
→ deterministic not_applicable success

explicit publication-sync PR
→ fail closed on packet, ancestry, projection, digest, or no-publish failure
```

Sync mode requires both repository-owned markers:

1. the PR introduces or changes `.github/publication-sync/packet.yaml`; and
2. the PR template field says `Publication-sync PR (yes/no): yes`.

A title substring is not authority. Missing, stale, blocked, `not_proven`,
cancelled, timed-out, or instrument-failed required evidence cannot become
success.

The actual publication-sync PR must land with **Create a merge commit**.
Ordinary repository PRs continue to use their normal merge policy; this
transaction is the exception because squash/rebase would destroy the ancestry
it exists to preserve.

At merge time use expected-head compare-and-swap protection. This is merge-race
safety, not review-currentness ceremony.

After GitHub creates `M`, run the contract's post-merge verifier and prove:

```text
J is an ancestor of M
tree(M) == tree(J)
R is an ancestor of M
S is an ancestor of M
current perl-lsp/master == M
```

## Review currentness

Exact Git/tree identities are load-bearing evidence for the release transaction.
They do not imply that every unrelated commit invalidates every review judgment.

Refresh only affected dimensions:

- changed `R` → rebuild/recheck the join against the new release base;
- changed `S` → rerun final reconciliation, projection, and affected release proof;
- changed reconciliation digest → re-adjudicate affected release-only work;
- changed projection manifest or expected tree → rerun projection/join review;
- conflict/integration repair → review the affected interaction;
- unrelated work outside the frozen transaction → no ceremonial full re-review.

Once final `R` and `S` are pinned, do not silently refresh them. A deliberate
repin starts a new transaction attempt and invalidates dependent packet bytes.

## No-publish boundary

The sync operation is reversible repository integration. It is not the release
cut.

During sync:

```yaml
published_channels: []
release_cut: false
```

No tag, GitHub Release, crates.io publication, Marketplace/Open VSX publish,
container push, package-manager publication, or other public channel mutation
belongs in the sync workflow or PR.

The exact landed `M` is handed to the no-publish candidate rehearsal. Public
publication remains a later, explicitly authorized transaction.

## Per-release lifecycle

Every release follows the same high-level lifecycle:

```text
preliminary release-only comparison
→ port/resolve real product/test divergence in swarm
→ freeze product
→ prepare exact S
→ pin exact R
→ final blocker-free reconciliation
→ final publication projection P
→ build J0/J and committed control packet
→ open protected perl-lsp sync PR
→ Publication Sync Contract + substantive review
→ merge by merge commit under expected-head guard
→ record/verify M
→ build exact no-publish candidate from M
```

The copy-safe commands, stop conditions, recovery matrix, and transaction file
layout are in the publication-sync how-to.

## Versioned enforcement

The currently deployed publication check was introduced for the v0.18 RC and
its packet/schema constants are release-pinned. Before a later release reuses
the mechanism, either:

- roll the validator/schema/tests to that release under a reviewed control PR;
  or
- land a separately reviewed release-parameterized schema that preserves the
  same fail-closed subject/digest rules.

Do not copy an old release packet and edit only the version string.

## Completion criteria

A publication sync is complete only when:

- final reconciliation is blocker-free for exact `R` and `S`;
- every publication-tree deviation from `S` is manifest-owned;
- `J0` has ordered parents `[R, S]` and tree `P`;
- `J` contains only `P` plus the exact control directory;
- `Publication Sync Contract` passes on the declared PR;
- the PR lands with merge-commit method and expected-head protection;
- post-merge verification proves `J` survives in `M` with identical landed tree;
- exact `M` is recorded for the no-publish candidate;
- no public channel mutated during the sync.
