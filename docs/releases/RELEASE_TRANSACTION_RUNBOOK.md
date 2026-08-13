# Release transaction runbook

This is the sequence authority for `perl-lsp` releases.

It defines the reusable transaction from a converged development tree through
public channel closeout. A version-specific release controller defines the
product scope, accepted limitations, and current packet identities for one
release. Domain documents retain their narrower authorities:

| Document or authority | Owns |
|---|---|
| This runbook | Phase order, exact identity names, mutation boundaries, invalidation, recovery, and operator decisions |
| Version-specific release controller | Product scope, release track, accepted limitations, current blockers, and terminal release status |
| [`sync-protocol.md`](../swarm/sync-protocol.md) | Cross-repository reconciliation, publication projection, and history-preserving join law |
| [`README.md`](README.md) | Release-note provenance, classification, and claim wording |
| [`RELEASE_CLOSEOUT_AUDIT.md`](RELEASE_CLOSEOUT_AUDIT.md) | Public-channel verification and closeout |
| [`RELEASE.md`](../../RELEASE.md) | Publication mechanism details after authorization |
| Release topology and packet schemas | Package, target, artifact, channel, and evidence membership |

No other document may independently redefine the current release sequence.
Historical runbooks and incident records remain evidence, not live authority.

## Governing rules

1. **Exact subjects, not moving names.** Branch names may locate a candidate.
   Every retained release identity is an immutable commit, tree, digest, artifact,
   workflow/control, or public-channel subject.
2. **A later phase does not prove an earlier one.** A workflow starting, a tag
   existing, or one channel publishing cannot promote a missing packet.
3. **Missing evidence remains missing.** `NOT_PROVEN`, skipped, cancelled,
   timed-out, malformed, stale, mixed-subject, and instrument-failed evidence is
   not pass.
4. **Before the tag, repair is reversible.** Return to the earliest affected
   phase, produce new affected digests, and review again.
5. **After the tag, public identity is immutable.** Preserve the tag and bytes,
   report partial state, repair only the bounded channel path where safe, or cut
   a patch release. Do not retag or silently replace public subjects.
6. **Agents may prepare and verify.** Only a named human may authorize the exact
   candidate for irreversible publication.
7. **One release operator crosses the mutation boundary.** Read-only reviewers
   independently verify packets and public endpoints. Channel repairs are
   serialized.
8. **The release transaction does not absorb product work.** Product defects
   return to the owning development issue and invalidate affected release state.

## State machine

```text
CONVERGING
    product, controls, or accepted evidence can still change

FROZEN
    one exact product SHA, topology subject set, supported envelope,
    limitations, and invalidation policy are accepted

PREPARED
    deterministic version, dependency, notes, history, status, and
    publication-context inputs are applied without product change

RECONCILED
    every release-repository-unique product/test commit has one terminal,
    evidence-backed disposition against exact prepared swarm

PROJECTED
    every publication-context deviation is explicit and the expected public
    tree is deterministic

SYNCED
    the audited two-parent join lands through the protected publication PR;
    the audited join and landed wrapper remain distinct identities

CANDIDATE_READY
    one exact no-publish candidate packet reports `ship_candidate`;
    publication authority remains absent

APPROVED
    a named human approval binds to that exact candidate packet and subject set;
    immediate pre-mutation revalidation still must pass

PUBLISHED_PARTIAL
    the immutable tag exists, but one or more required public channels are not
    externally verified

PUBLISHED_VERIFIED
    every required public channel and claim is externally verified and durable
    release truth is reconciled
```

The state is conjunctive. A stronger result on one rail cannot compensate for a
missing result on another.

## Start one release instance

Create one version-specific transaction record before freeze. Recommended path:

```text
docs/releases/vX.Y.Z-transaction.md
```

Copy the following block and fill fields only from the owning packet. Keep
unknown values `null`; do not guess future SHAs, counts, URLs, dates, or channel
receipts.

```yaml
schema_version: perl_lsp_release_transaction.v1
release: X.Y.Z
track: public-beta|ga|other-reviewed-track
controller_issue: null
status: converging

frozen_product_sha: null
freeze_packet_digest: null
frozen_topology_digest: null

prepared_swarm_sha: null          # S
preparation_manifest_digest: null
prepared_topology_digest: null
release_note_catalog_digest: null
notes_hash: null
published_api_audit_digest: null
public_claims_digest: null
release_integrity_digest: null
lockfile_hashes: []

reconciliation_boundary_sha: null # B
reconciliation_digest: null
release_base_sha: null             # R
publication_sync_manifest_digest: null
expected_projected_tree: null
sync_join_sha: null                # J
landed_release_sha: null           # M

candidate_id: null
candidate_packet_digest: null
candidate_verdict: null            # ship_candidate|hold|not_proven
publish_authorized: false
published_channels: []

pre_mutation_verifier_digest: null
authorization_ref: null
approved_by: null
approved_at: null

release_tag: vX.Y.Z
release_tag_sha: null
channels: {}
closeout_packet_digest: null
incidents: []
```

The instance record is a projection of typed packet truth. It does not replace
those packets or become a second release database.

## Phase contract

Every phase has one entry condition, one accountable output, one terminal
result vocabulary, and one invalidation rule.

### 1. Converge and freeze the product

**Enter when:** development and control-plane work is still open.

**Do:**

- classify the live release-relevant queue;
- repair admitted product and control defects through their owning issues;
- close feature intake;
- validate terminal blocker packets;
- record the exact supported envelope, known limitations, topology subject set,
  and zero-budget trust failures.

**Output:**

```text
frozen_product_sha
freeze_packet_digest
frozen_topology_digest
```

**Allowed mutations:** ordinary development PRs and evidence/control repairs in
the development repository.

**Terminal result:** `frozen` or `not_proven`.

**Invalidate when:** product behavior, supported scope, topology membership,
mandatory evidence, or an accepted limitation changes.

### 2. Prepare release identity and narrative

**Enter when:** the freeze packet is exact and current.

**Do:**

- move every topology-published version and internal dependency together;
- consume the reviewed changelog/release-note catalog exactly once;
- apply published API migration guidance;
- project public claims at their earned strength;
- update staged release history and status without claiming publication;
- generate publication-context inputs;
- rerun generation and require no second diff;
- prove no product implementation change entered the preparation.

**Output:**

```text
prepared_swarm_sha = S
preparation_manifest_digest
prepared_topology_digest
notes_hash
release-note/API/public-claim/integrity digests
lockfile hashes
```

**Allowed mutations:** deterministic version, dependency, lockfile, note,
history, status, topology, and context-preparation changes in swarm.

**Terminal result:** `prepared`, `blocked`, or `not_proven`.

**Invalidate when:** the frozen product, prepared metadata, notes, public claims,
API migration, topology, or release-integrity input changes.

### 3. Reconcile publication lineage

**Enter when:** exact prepared swarm `S`, exact completed reconciliation boundary
`B`, and exact current publication base candidate exist.

**Do:**

- compare release-repository target commits against exact `S`, with `B` only as
  the history limit;
- exclude merge ancestry explicitly;
- require one row per genuinely target-unique non-merge commit;
- allow terminal evidence-backed dispositions only;
- require ports/equivalents to be reachable from `S`;
- keep release-lineage exclusions separate from product acceptance;
- block unresolved architecture decisions.

**Output:**

```text
reconciliation_digest
boundary/source/target SHAs
terminal row population
blocking_decisions: []
```

**Allowed mutations:** none in the final packet. A required product/test port
returns to swarm, creates a new `S`, and invalidates downstream work.

**Terminal result:** `pass`, `blocked`, or `not_proven`.

**Invalidate when:** `B`, `S`, release base, row evidence, or any required
product disposition changes.

### 4. Project the publication tree

**Enter when:** reconciliation passes and all reviewed release inputs are exact.

**Do:**

- start from the complete tree of `S`;
- inventory repository/branch/link/issue/public-wording translations;
- inventory release-lineage and swarm-only dispositions;
- validate artifact, installer, downloader, VSIX, package, version, claim,
  waiver, ruleset, and environment invariants;
- require one digest-bound row for every deviation from `S`;
- compute the expected publication tree twice and require identical bytes/tree.

**Output:**

```text
publication_sync_manifest_digest
expected_projected_tree
blockers: []
```

**Allowed mutations:** disposable projection worktree/object database only. No
release branch, tag, package, registry, Marketplace, image, or public channel.

**Terminal result:** `projected`, `blocked`, or `not_proven`.

**Invalidate when:** `R`, `S`, reconciliation, manifest row, release input
identity, or expected tree changes.

### 5. Arm the protected publication check

**Enter when:** the repository-side verifier exists and its reported context has
been rehearsed on ordinary and fixture sync PRs.

**Do:**

- confirm the exact check context name;
- make that context required on `perl-lsp/master`;
- retain required review-thread resolution;
- constrain the actual sync transaction to merge-commit landing;
- record live ruleset/classic-protection state and rollback evidence.

**Output:** live-settings receipt naming the required context and merge method.

**Allowed mutations:** reviewed GitHub branch/ruleset settings only. No source
join or publication.

**Terminal result:** `armed`, `blocked`, or `not_proven`.

**Invalidate when:** workflow/context identity, live required-check state,
review-thread policy, or allowed landing method changes.

### 6. Construct and land the audited join

Use four exact identities:

```text
R = exact perl-lsp/master release base
S = exact prepared swarm
J = audited two-parent complete-tree join
M = landed perl-lsp/master wrapper after protected GitHub merge
```

Expected graph:

```text
          M
         / \
        R   J
           / \
          R   S
```

**Enter when:** reconciliation, projection, and protected-check packets pass.

**Construct:**

```bash
git merge --no-ff --no-commit -s ours "$S"
git read-tree -u --reset "$S"
# Apply only manifest-listed publication operations.
git commit
```

**Require before PR merge:**

```text
parents(J) == [R, S]
tree(J) == expected_projected_tree
diff(S, J) == exactly manifest rows
required Publication Sync Contract == pass
no publication side effect
```

Land the sync PR with merge-commit method and an expected-head guard. Squash or
rebase is invalid for this transaction.

**Require after landing:**

```text
J is an ancestor of M
tree(M) == tree(J)
R is an ancestor of M
S is an ancestor of M
current perl-lsp/master == M
```

**Output:** sync packet with distinct `sync_join_sha: J` and
`landed_release_sha: M`.

**Allowed mutations:** one publication-repository branch/PR and the protected
merge. No tag or channel publication.

**Terminal result:** `synced`, `blocked`, or `not_proven`.

**Invalidate when:** `R`, `S`, reconciliation, manifest, expected tree, PR head,
or live protection changes before landing.

### 7. Build and review the exact no-publish candidate

**Enter when:** exact landed `M` and all input digests are current.

**Do against one candidate subject:**

- package every topology-published crate in order;
- build every topology-required archive and required member;
- build/install the exact VSIX and matching managed binaries;
- generate and validate checksums, SBOM, and attestation subject plan;
- run installed public-beta, upgrade, recovery, rollback, DAP-preview, lifecycle,
  and clean-shutdown evidence;
- verify notes, migration, public claims, links, targets, and install mappings;
- prove the rehearsal has no effective publication authority or public mutation;
- independently review each candidate proposition.

**Output:** content-addressed candidate packet:

```text
ship_candidate | hold | not_proven
publish_authorized: false
published_channels: []
release_cut: false
```

**Allowed mutations:** isolated build/install/test roots and durable private or
repository-governed evidence. No tag or public channel.

**Invalidate when:** candidate bytes, `M`, topology, workflow/control, notes,
claims, settings, or mandatory evidence changes.

### 8. Approve and immediately revalidate

**Enter when:** the candidate packet reports `ship_candidate` and independent
review is complete.

A named human records approval bound to the exact packet digest and subject set.
Approval does not float to a regenerated candidate.

Before publication authority is exposed, a read-only verifier must recheck:

- exact approval and candidate bytes;
- current `perl-lsp/master == M`;
- tag absence;
- topology, notes, claims, artifacts, checks, reviews, rulesets, environments,
  and credential/control identities;
- a fresh or still-exact publication-drift result;
- absence of required `failed`, `not_proven`, `invalid`, pending, skipped,
  cancelled, or stale evidence.

**Output:** one content-addressed, unused, bounded authorization input.

**Allowed mutations:** human approval record only. The verifier remains
read-only and cannot call the publisher.

**Terminal result:** `authorization_ready`, `blocked_drift`, `blocked_stale`,
`blocked_approval`, or `not_proven`.

**Invalidate when:** any approved subject, public base, channel set, control,
workflow attempt, or authorization-use state changes.

### 9. Publish the approved subject once

**Enter when:** an unused authorization input is current and the isolated
publisher has the reviewed channel-scoped authority.

**Sequence:**

1. create one immutable annotated tag at exact `M`;
2. build/publish the complete GitHub Release and supply-chain subjects;
3. publish the topology-derived crates.io set in dependency order;
4. publish the exact VSIX to Marketplace and Open VSX;
5. publish each required secondary channel or retain an explicit reviewed defer;
6. stop remaining channels after a partial failure until the incident is
   classified.

Do not reconstruct membership from a literal count. Do not move `latest`,
default, or stable aliases for a prerelease/public-beta track unless topology
explicitly requires it.

**Output:** immutable tag, public subjects, and a partial channel matrix.

**Terminal result:** `published_partial`, `failed`, or ready for external
verification. Publication workflow success is not `published_verified`.

### 10. Verify public truth and close out

**Enter when:** the tag exists and public channels have terminal observations.

From clean external environments, verify:

- tag target and provenance;
- exact GitHub Release assets, archive members, hashes, checksums, SBOM, and
  attestations;
- every topology package on crates.io and representative clean installs;
- Marketplace and Open VSX listing, package bytes, clean-profile install,
  activation, and managed binary identity;
- required Docker/package-manager channels and user-visible downstream PR
  merges;
- public install/upgrade/error paths and documented commands;
- public-beta, DAP-preview, platform, support, notes, and migration claims;
- no unmirrored release-repository product work.

Populate the per-release closeout audit, release history, notes/status, incident
and deferred-channel owners, and closeout packet.

**Output:** `closeout_packet_digest` with status `published_verified`, `partial`,
or `failed`.

Only `published_verified` closes the release controller.

## Command truth

The repository currently contains tools from more than one release-process
generation. Use this classification instead of inferring authority from command
existence.

### Available component checks

These commands prove only their stated component subject:

```text
cargo xtask install-surface-check
cargo xtask release artifact-check --dist <dir> --version <X.Y.Z>
cargo xtask release-notes --tag <vX.Y.Z> --output <path>
cargo xtask check-version-sync
bash scripts/check_release_history.sh
```

A component pass cannot substitute for freeze, sync, candidate, approval, or
public verification.

### Available but not final release authority

```text
cargo xtask sync-divergence check ...
```

The current implementation validates a source ref but still computes target
uniqueness with the historical boundary as the comparison upstream. Do not use
its successful receipt as final reconciliation authority until the exact-source
comparison and terminal ledger validator land.

### Planned, not currently available

The following command names describe the intended publication-projection tool
surface. Do not put them in an operator checklist as executable until their
implementation issues land:

```text
cargo xtask publication-sync plan
cargo xtask publication-sync project
cargo xtask publication-sync verify-join
cargo xtask publication-sync verify-landed
```

### Legacy direct-cut paths: prohibited for the current transaction

```text
cargo xtask release-turnkey X.Y.Z
just release-turnkey X.Y.Z
scripts/prepare-release.sh X.Y.Z
gh workflow run "Release Orchestration" --field version=X.Y.Z ...
```

These paths currently perform version-bump/orchestration mechanics and can reach
tag creation without consuming the exact no-publish candidate, R/S/J/M packet,
named human approval, and immediate pre-mutation verifier required by this
runbook. They are not valid entrypoints for the current release transaction.
They may return only after they become thin consumers of those exact inputs.

### Always prohibited

- manually creating, deleting, moving, or recreating the release tag;
- silently replacing public assets or marketplace bytes;
- using a one-parent snapshot, patch replay, archive copy, or per-file blended
  merge as the release sync;
- using a hard-coded historical exclusion list as current publication policy;
- using issue count, workflow colour, a branch name, or elapsed time as release
  readiness;
- publishing from workspace output or another candidate's artifact;
- calling partial publication complete.

## Failure and recovery matrix

| Failure point | Required response |
|---|---|
| Product/control finding before freeze | Repair through its owner; remain `CONVERGING` |
| Product change after freeze | Invalidate freeze and all downstream packets |
| Metadata/claim change after preparation | Regenerate preparation and all dependent digests |
| Release-only product/test work found during reconciliation | Port/resolve in swarm; create new `S`; repeat reconciliation |
| Release base `R` moves before join landing | Rebuild affected projection/join against new `R` |
| Wrong parents, wrong tree, or unlisted path in `J` | Reject join; do not patch it forward in `perl-lsp` |
| Sync PR squashed/rebased | Audited ancestry lost; rebuild and reland through merge commit |
| `J` absent from landed `M` or tree differs | Sync failed; do not build candidate |
| Candidate stage fails | `hold`; repair owner, rebuild candidate, review again |
| Candidate evidence missing/instrument-failed | `not_proven`; repair evidence, do not infer product result |
| Approved candidate or public base changes | Invalidate authorization and obtain new approval |
| Failure before tag | Stop with no public mutation; return to earliest affected phase |
| Failure after tag in one channel | Preserve tag/bytes, record partial matrix, repair one bounded channel idempotently |
| Bad immutable public artifact or false public claim | Preserve incident, stop silent replacement, prepare explicit patch release |

## v0.18 issue mapping

This table is an instance map, not permanent release law.

| Phase | Current owners |
|---|---|
| Converge and freeze | #6275, #5888, #6065, #6051 |
| Prepare | #4347, #6068 |
| Reconcile | #7968, #7969, #7971, #7983, #6064 |
| Project | #7972, #7973, #7976, #8009, #6356 |
| Arm protected check | #7978, #7982, #7647 |
| Construct and land | #4348 |
| Build candidate | #4350, #6069 |
| Approve and pre-mutation verify | #4351, #6070, and the candidate-bound verifier owners |
| Publish and close out | #4351, #6070, release topology, and the closeout audit |

## Operator procedure

For each release phase:

1. Read the release instance and current controller body.
2. Resolve every mutable input to an immutable identity.
3. Confirm the entry packet and invalidation set.
4. Run only the current phase in a clean, owned worktree or isolated executor.
5. Retain canonical packet bytes/digests and a bounded human summary.
6. Review the packet with a reader that did not produce the mutation.
7. Record the terminal disposition and exact next phase or return owner.
8. Never continue because a later workflow already started.
9. Expose publication authority only after named human approval and current
   read-only revalidation.
10. After closeout, remove only transaction-created branches, worktrees, scratch
    state, and redundant build material; retain durable packets and public proof.

Prefer transition-driven reads. Re-query when a subject or decision may have
changed, not on a sustained polling loop. Ordinary API, runner, or provider
failure is a typed instrument result and a reason to change the observation
route—not a reason to invent state or escalate routine ambiguity.

## Completion

A release is complete only when:

```text
one exact product was frozen
+ one deterministic preparation was reviewed
+ release lineage was reconciled
+ one deterministic public tree was projected
+ J and M were proven separately
+ one exact no-publish candidate earned ship_candidate
+ a named human approved that packet
+ the exact subject was published once
+ every required public channel was externally verified
+ durable release history, notes, incidents, deferrals, and lineage were closed
```

Anything less remains staged, blocked, `not_proven`, partial, or failed.