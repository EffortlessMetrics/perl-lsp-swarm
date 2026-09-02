# History-preserving publication sync

Use this runbook when promoting one prepared release from
`EffortlessMetrics/perl-lsp-swarm` into the publication repository
`EffortlessMetrics/perl-lsp`.

This is a **repository integration** operation. It preserves swarm history,
keeps the prepared product tree coherent, applies only reviewed publication
context, and stops before any public release mutation.

Stable law lives in [`../swarm/sync-protocol.md`](../swarm/sync-protocol.md).
Per-release receipts live under [`../swarm/source-syncs/`](../swarm/source-syncs/).

## What happens each release

```text
preliminary release-only comparison
→ resolve real product/test divergence in swarm
→ freeze product
→ prepare exact swarm S
→ pin exact publication base R
→ final blocker-free reconciliation
→ final publication projection P
→ construct J0 and PR head J
→ protected perl-lsp review + Publication Sync Contract
→ merge by merge commit under expected-head guard
→ verify landed wrapper M
→ build the no-publish candidate from M
```

Do not begin the final transaction while product or release-preparation inputs
are still moving.

## Identities

Record these names in every release packet and PR:

```text
B  last completed reconciliation boundary
R  exact perl-lsp/master release base
S  exact prepared swarm commit
P  exact projected publication tree
J0 core join: parents [R,S], tree P
J  PR head: J0 semantics plus .github/publication-sync/ control files
M  GitHub merge-commit wrapper landing on perl-lsp/master
```

The current `Publication Sync Contract` stores `J0` in the packet field
`sync_join_sha`. This avoids the impossible requirement for commit `J` to
contain its own SHA. The validator re-derives `J0` from `J` using the same
parents, message, author/committer identity, and the tree with the control
directory removed.

## One-time repository setup

These are repository controls, not per-release chores. Verify them rather than
recreating them:

- `perl-lsp/master` requires the status context `Publication Sync Contract`;
- required PR/conversation-resolution rules remain active;
- `.github/workflows/publication-sync-contract.yml` exists in `perl-lsp`;
- `scripts/publication_sync_check.py` and its synthetic Git tests exist;
- `.github/PULL_REQUEST_TEMPLATE.md` carries
  `Publication-sync PR (yes/no): no` by default;
- the check runs read-only and has no publication credential.

An ordinary `perl-lsp` PR should report deterministic `not_applicable` from
`Publication Sync Contract`.

### Release-schema rollover

The currently deployed check was introduced for the v0.18 RC and its schema,
validator constants, and fixtures are release-pinned. Before using the
mechanism for another release, inspect:

```text
schemas/publication_sync.v2.schema.json
scripts/publication_sync_check.py
scripts/tests/test-publication-sync-contract.py
```

If their accepted release/schema identity does not match the target release,
land a reviewed control-plane rollover first. Do not copy the prior packet and
edit only the release string.

For field semantics, the executable `scripts/publication_sync_check.py` is the
authority used by the protected check. At the current v0.18 validator revision,
`sync_join_sha` names the derived core join `J0`, not the PR-head join `J`.
The schema description must agree with that executable contract. If the live
schema and validator disagree, stop before constructing a packet and repair or
roll the `perl-lsp` control-plane contract under a separate reviewed change;
this swarm runbook does not override an external schema by inference.

## Phase 1 — preliminary reconciliation

This phase may begin before final freeze. It reduces surprises; it is not the
final release proof.

### 1. Fetch both histories into one object database

From a swarm checkout with a `release` remote for `perl-lsp`:

```bash
git fetch origin main --prune
git fetch release master --prune
```

Resolve and record:

```bash
SWARM_NOW=$(git rev-parse origin/main)
RELEASE_NOW=$(git rev-parse release/master)
BOUNDARY=<last-completed-reconciliation-boundary>
```

Do not guess `BOUNDARY` from date, branch age, or the previous release tag. Use
the last accepted reconciliation closeout.

### 2. Scaffold the target-unique comparison

```bash
cargo xtask sync-divergence scaffold \
  --source "$SWARM_NOW" \
  --boundary "$BOUNDARY" \
  --target "$RELEASE_NOW" \
  --ledger docs/swarm/source-syncs/<release>-reconciliation.json
```

Then validate while classifying rows:

```bash
cargo xtask sync-divergence check \
  --source "$SWARM_NOW" \
  --boundary "$BOUNDARY" \
  --target "$RELEASE_NOW" \
  --ledger docs/swarm/source-syncs/<release>-reconciliation.json \
  --receipt target/receipts/source-sync-<release>.json
```

Each target-unique non-merge row ends in exactly one terminal disposition:

```text
port_to_swarm
already_equivalent_in_swarm
superseded_by_newer_architecture
deliberately_abandoned
release_lineage_only
```

Rules:

- product/test behavior needed for the release is ported into swarm through a
  normal bounded swarm PR;
- `already_equivalent_in_swarm` names an exact reachable source commit;
- competing implementations receive an architecture ruling before the row can
  become terminal;
- publication history/governance may be `release_lineage_only` only when it is
  not hiding product/test code;
- unresolved decisions stay blocked.

Do not turn the raw commit count into a port queue.

## Phase 2 — freeze and prepare the swarm subject

Finish the release freeze and deterministic preparation in swarm. The final
prepared subject is immutable input `S`:

```bash
S=<prepared_swarm_sha>
git cat-file -e "${S}^{commit}"
```

Record the preparation/topology/notes/API/public-claims/integrity digests that
the publication projection consumes.

From this point, a product or preparation change is a **repin**, not a casual
branch refresh. Repinning `S` invalidates final reconciliation and projection
proof tied to the old value.

## Phase 3 — pin R and run final reconciliation

Read the current publication base immediately before the final transaction:

```bash
git fetch release master --prune
R=$(git rev-parse release/master)
```

Stop direct product work in `perl-lsp` during the transaction. If `R` moves,
re-run the affected final steps against the new base.

The scaffold and check commands bind the ledger to the exact source, boundary,
and target identities. If `SWARM_NOW`, `S`, or `R` changes after a preliminary
scaffold, preserve the old ledger as an audit artifact and stop using it. The
available `sync-divergence scaffold` command refuses to overwrite an existing
ledger; create a new ledger at a new path with the final identities, then
re-adjudicate every row before running the final check. There is no supported
in-place refresh that preserves dispositions, so do not copy dispositions into
a newly populated ledger without re-checking their evidence. If the rows cannot
be re-adjudicated, the transaction remains `NOT_PROVEN`.

Run the final exact comparison using prepared `S`:

```bash
cargo xtask sync-divergence check \
  --source "$S" \
  --boundary "$BOUNDARY" \
  --target "$R" \
  --ledger docs/swarm/source-syncs/<release>-reconciliation.json \
  --receipt target/receipts/source-sync-<release>-final.json
```

The final result must be `pass` with:

```text
zero missing rows
zero invalid/unevidenced rows
zero unresolved decisions/blockers
every required port/equivalent reachable from S
```

Freeze the exact ledger bytes/digest used by the publication packet.

## Phase 4 — projection contract and prerequisite

The projection starts from the complete tree of `S`. Every intended difference
must be declared in the release's projection manifest before the join.

The swarm provides `cargo xtask sync-divergence scaffold` and
`cargo xtask sync-divergence check` for reconciliation, and
`cargo xtask publication-sync plan` for the manifest (#7972):

```bash
cargo xtask publication-sync plan \
  --manifest <publication_sync_manifest.v1.json> \
  --repo-root . \
  --receipt target/receipts/publication-sync-plan.json
```

`plan` is read-only. It validates the manifest against
`schemas/publication_sync_manifest.v1.schema.json`, verifies every declared
release-input digest, proves the declared reconciliation receipt passed and
reconciled exactly this `S` against this `R`, checks each projection row, and
emits a deterministic `pass|blocked|not_proven` receipt carrying the canonical
manifest digest. It changes no branch, worktree, or tree.

**`plan` is not a producer for `P`.** It judges a manifest you already have; it
does not derive the projected tree, and it does not yet inventory candidate
destination-context translations from the live `R` and `S` trees (#7973).

This runbook therefore still treats projection generation as a hard
prerequisite, not as an operation that this checkout can perform. Before a
release transaction, identify a separately reviewed producer whose output is
bound to `R`, `S`, the reconciliation evidence, and the manifest. If no accepted
producer is available, stop here. Do not infer one from a command name, invoke
an unimplemented `cargo xtask` path, or replace the missing producer with a
manual exclusion checklist. A `pass` from `plan` is evidence about the manifest
only; it is not authorization to publish.

The final manifest must cover, where applicable:

- repository/branch URLs and public links;
- bare issue/PR references whose repository context changes;
- public release wording and claims;
- swarm-only development/control-plane paths;
- publication-repository release lineage/governance paths;
- installer/archive/VSIX target composition;
- versions and effective artifact identity;
- live quality/ruleset/environment evidence required by the release.

Generate twice from identical inputs. Require identical normalized manifest
bytes and the same projected Git tree:

```text
P = expected_projected_tree
```

No unlisted path may differ from `S`.

## Phase 5 — construct J0 and J

Perform construction in a fresh disposable `perl-lsp` worktree/clone whose
object database contains both `R` and `S`.

### 1. Create the complete projected tree

Conceptually:

```bash
git switch --detach "$R"
git read-tree "$S"
# Apply only the accepted deterministic projection manifest.
P=$(git write-tree)
```

Do not use a recursive/per-file merge result as the product tree.

### 2. Fix one commit identity

`J0` and `J` must share parents, message, author/committer identity, and
timestamps so the release validator can re-derive the core join exactly.

Use one message file and fixed identity/date environment for both
`git commit-tree` operations. Do not run two ordinary `git commit` commands and
assume their metadata will happen to match.

Example shape:

```bash
MESSAGE_FILE=<reviewed-message-file>
export GIT_AUTHOR_NAME="$(git config user.name)"
export GIT_AUTHOR_EMAIL="$(git config user.email)"
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"
export GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"
STAMP=$(date +%s)
export GIT_AUTHOR_DATE="${STAMP} +0000"
export GIT_COMMITTER_DATE="${STAMP} +0000"

J0=$(git commit-tree "$P" -p "$R" -p "$S" < "$MESSAGE_FILE")
```

### 3. Write the three control files

Create exactly:

```text
.github/publication-sync/packet.yaml
.github/publication-sync/reconciliation-ledger.json
.github/publication-sync/projection-manifest.json
```

The packet is JSON-parseable YAML and follows the current
`schemas/publication_sync.v2.schema.json` contract. At minimum it binds:

```json
{
  "schema_version": "<current schema>",
  "release": "<current release>",
  "release_base_sha": "<R>",
  "prepared_swarm_sha": "<S>",
  "sync_join_sha": "<J0>",
  "expected_join_parents": ["<R>", "<S>"],
  "reconciliation_digest": "sha256:...",
  "publication_sync_manifest_digest": "sha256:...",
  "expected_projected_tree": "<P>",
  "published_channels": [],
  "release_cut": false
}
```

No fourth file belongs in the control directory.

### 4. Create PR-head join J

With the projected tree plus the exact control directory staged in the index:

```bash
J_TREE=$(git write-tree)
J=$(git commit-tree "$J_TREE" -p "$R" -p "$S" < "$MESSAGE_FILE")
```

Because the fixed identity/date environment is unchanged, the validator can
remove the control directory from `J`, recreate `J0`, and compare it exactly to
`sync_join_sha`.

Before pushing, prove locally:

```text
parents(J) == [R,S]
projected tree(J) == P
re-derived core join(J) == J0
control directory == exactly packet + ledger + manifest
diff(S,J) excluding control directory == exactly manifest paths
published_channels == []
release_cut == false
```

Use the repository's actual `Publication Sync Contract` validator for this
proof; do not maintain an independent shell reimplementation.

## Phase 6 — open the protected perl-lsp sync PR

Push `J` to one release-sync branch and open a PR with base `master`.

In the existing `perl-lsp` PR template set:

```text
- Publication-sync PR (yes/no): yes
```

The fresh packet plus that field are both required to enter sync mode.

The PR body records at least:

```yaml
release: <version>
boundary: <B>
release_base_sha: <R>
prepared_swarm_sha: <S>
expected_projected_tree: <P>
core_sync_join_sha: <J0>
pr_head_join_sha: <J>
reconciliation_digest: sha256:...
publication_sync_manifest_digest: sha256:...
published_channels: []
release_cut: false
```

Review the actual transaction propositions:

- reconciliation completeness;
- publication projection and destination context;
- parents/tree/diff identity;
- artifact/install/downloader/VSIX composition;
- public claims, versions, API migration, quality exceptions and live controls;
- no publication side effect.

Do not restart broad review merely because an unrelated SHA moved. A changed
`R`, `S`, reconciliation digest, projection manifest, expected tree, or actual
integration repair refreshes the affected review/proof.

## Phase 7 — pre-merge gate and merge method

`Publication Sync Contract` must report success on the declared sync PR. The
check is required on `perl-lsp/master` and runs from trusted base code with
read-only repository permissions.

Do not merge the sync PR with squash or rebase.

Use an expected-head guard at the irreversible integration step:

```bash
gh pr merge <PR_NUMBER> \
  --repo EffortlessMetrics/perl-lsp \
  --merge \
  --match-head-commit "$J"
```

This expected-head check is a race guard. It is not a requirement to rewrite
or re-review an otherwise unchanged candidate because another branch moved.
It guards only `J`; this command does not atomically assert that the pinned
release base `R` is still unchanged. Re-read `R` immediately before the merge
request and rely on the publication repository's protected transaction gate to
serialize that base, if one exists. If `R` moved after final reconciliation, or
no such gate can establish the exact base, stop and rebuild the transaction;
the merge is `NOT_PROVEN`.

## Phase 8 — record and verify M

After GitHub creates wrapper merge `M`:

```bash
M=$(gh pr view <PR_NUMBER> \
  --repo EffortlessMetrics/perl-lsp \
  --json mergeCommit \
  --jq .mergeCommit.oid)
```

Read the mutable publication head separately for freshness; do not use it as
the wrapper identity:

```bash
CURRENT_M=$(gh api repos/EffortlessMetrics/perl-lsp/commits/master --jq .sha)
```

Run the repository-owned post-merge verifier:

```bash
gh workflow run publication-sync-contract.yml \
  --repo EffortlessMetrics/perl-lsp \
  -f merge_sha="$M"
```

The verifier must establish:

```text
J is an ancestor of M
tree(M) == tree(J)
R is an ancestor of M
S is an ancestor of M
current perl-lsp/master == M
```

A squash/rebase landing fails this proof.

Record `M` in the sync closeout and hand exact `M` to the no-publish candidate
rehearsal. Do not rebuild the candidate from `J0`, `J`, a branch name, or a
later `master` by accident.

## Stop / invalidate matrix

| Observation | Required action |
| --- | --- |
| Release-only product/test row remains unresolved | Stop; resolve/port in swarm |
| Required port/equivalent is not reachable from final S | Stop; final reconciliation is invalid |
| S changes after final reconciliation | Repin; rerun reconciliation and projection |
| R changes before merge | Rebuild/recheck the transaction against new R |
| Manifest/digest/P changes | Rebuild J0/J; refresh affected review |
| Correct parents but wrong tree | Stop; projection/build defect |
| Correct tree but S ancestry missing | Stop; ancestry defect |
| Extra file in `.github/publication-sync/` | Stop; contract violation |
| `Publication Sync Contract` missing/skipped/not-proven | Stop; do not bypass |
| PR head moves after approval to merge | Re-read J and re-run affected proof before merge |
| GitHub lands by squash/rebase | Transaction failed; do not use result as release candidate |
| Product fix appears necessary in perl-lsp | Fix/mirror in swarm, repin affected release evidence |
| Any tag/registry/Marketplace/etc. mutation occurred | Stop; sync no-publish boundary was violated |

## Closeout checklist

- [ ] final reconciliation ledger is blocker-free for exact B/R/S;
- [ ] final projection is deterministic and every S→P deviation is owned;
- [ ] `J0` has parents `[R,S]` and tree `P`;
- [ ] `J` contains only `P` plus the exact three control files;
- [ ] `Publication Sync Contract` is green;
- [ ] substantive review findings are resolved/dispositioned;
- [ ] merge method is `merge` with expected-head protection;
- [ ] post-merge verification proves `J` survives in `M` with identical tree;
- [ ] exact `M` is recorded and handed to the no-publish candidate;
- [ ] no public channel mutated.

## What not to do

- Do not use `git merge -X theirs` or accept a per-file blended product tree.
- Do not carry a permanent hard-coded exclusion list between releases.
- Do not classify unresolved architecture work as a successful ledger row.
- Do not fix product/test behavior only in `perl-lsp` and hide it behind lineage.
- Do not squash or rebase the publication-sync PR.
- Do not use exact-head review receipts or status comments as review evidence.
- Do not tag or publish from this runbook.
