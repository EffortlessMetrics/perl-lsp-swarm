# Reverse-sync audit: `perl-lsp` through `646c34bb`

## Status

This is a **provisional disposition packet** for issue #3161. It records the
post-sync incident set and the intended recovery sequence. It is not a
`sync-divergence` receipt, does not claim that `git cherry` has been run in a
checkout containing both live repositories, and does not authorize an ancestry
merge, release sync, version change, tag, or publication.

The authoritative JSON ledger and receipt must be generated after Gate 0 below
runs against immutable fetched SHAs in one Git object database. Patch-ID signs
are deliberately treated as unknown until that command runs: semantic
supersession does not imply `git cherry` patch equivalence.

## Observation

| Field | Value |
|---|---|
| Observed at | 2026-07-23 |
| Swarm repository | `EffortlessMetrics/perl-lsp-swarm` |
| Observed swarm `main` | `a86f254d64f968c308302a9a75d2459832124261` |
| Release repository | `EffortlessMetrics/perl-lsp` |
| Audited release `master` | `646c34bb8b7c9ff846c31e7732172696e4a8d646` |
| Last delivered sync PR merge | `2c5ca9a8d922b03c43a884a909bf00d6deef8051` (`perl-lsp#10005`) |
| Pinned swarm cut in that sync | `f6b7b2c6626fbefbf01c9c9934cac5789186f8b2` |
| Disposition authority | `perl-lsp-swarm#3161` |
| Forward-sync consumer | `perl-lsp-swarm#4348` |

The incident interval is:

```text
2c5ca9a8d922b03c43a884a909bf00d6deef8051..646c34bb8b7c9ff846c31e7732172696e4a8d646
```

GitHub reports 22 non-merge commits in that interval.

## Root cause

The history-preserving complete-tree merge was performed in `perl-lsp`. That
made the pinned swarm cut and its ancestors reachable from the release history,
but it did not make later `perl-lsp` commits reachable from swarm. A separate
reverse ancestry merge is required for that direction.

After `perl-lsp#10005`, shared-tree changes continued landing directly on
`perl-lsp/master` without the swarm mirror/equivalence proof required by the
sync protocol's hard invariant. Some of those changes were later implemented in
swarm under different commit IDs; others remain absent from the live swarm tree.

Exact commit ancestry, semantic equivalence, and patch-ID equivalence are
separate questions:

- a broader swarm implementation can make a source patch unnecessary while
  still appearing as a target-unique `+` row under `git cherry`;
- an exact patch-equivalent `-` row needs no target-unique ledger entry, but its
  source-to-swarm mapping still belongs in the audit evidence;
- a final `-s ours` ancestry merge can preserve exact source history without
  importing the older release tree, but only after every useful source-only
  change has been ported or explicitly dispositioned.

## Post-`#10005` incident inventory

The classifications below are evidence-backed **semantic working
dispositions**. Gate 0 determines which commits are target-unique `+` rows and
therefore require authoritative JSON ledger entries.

| `perl-lsp` commit | Subject | Working disposition | Recovery slice / evidence |
|---|---|---|---|
| `6f27b566a54487a585b3285b4692db11098fb389` | `fix(completion): preserve receiver in method edits (#10006)` | `port_to_swarm` | Slice C. Port only the missing `$object->register` end-to-end LSP `textEdit` regression. The source implementation is superseded by swarm's newer completion architecture and must not be replayed. |
| `8eafc633fa575ae56500aaba8f3a7eb01d6db075` | `feat(workspace): detect Carton and Carmel roots (#3553)` | `port_to_swarm` | Slice C. Add current-tree dependency-root detection and request-level proof. |
| `92d416cc7ce5f4d03fce90088dc0c0a97992fb6f` | `fix(format): match leading closers to opener stack (#9992)` | `already_equivalent_in_swarm` | Swarm `e1cb1ee7a42209172ced68c20f11707927c8e845` is a broader semantic superset. Do not replay. If Gate 0 emits `+`, retain this classification in the ledger. |
| `11f19d86ad1a52de9e900be1b8586f734be35054` | `fix(critic): route printf fixes through structured metadata (#9984)` | `already_equivalent_in_swarm` | Swarm `ae0c9fcc2a6439a4e53396fdcba5ea3f56f66f10` is a broader semantic superset. Do not replay. If Gate 0 emits `+`, retain this classification in the ledger. |
| `ddb8461bf527f21d49ced8f26471e1a3f3733eea` | `test(critic): isolate legacy engine diagnostic fixtures (#9994)` | `already_equivalent_in_swarm` | Swarm `6cb24158fe582c7ce01ea2348dfe55681e0f730e` is a broader semantic superset. Do not replay. If Gate 0 emits `+`, retain this classification in the ledger. |
| `8de3ad7ed92b31bf1e11df4b586210e2b5814121` | `fix(release): publish GHCR images as multi-arch manifests (#9971)` | `port_to_swarm` | Slice B. Reimplement against current workflow pins and verify both manifest platforms. |
| `76568184716ba9939fbe5d9664c4aba23196f993` | `test(workspace): exercise duplicate anchor fail-closed guard (#10014)` | `port_to_swarm` | Slice C. Adapt the test-only injection seam to the current workspace index. |
| `8752a75a2f44bf0271193f4494322d4e0749169a` | `fix(quality): require issue references on ignored tests (#4912)` | `port_to_swarm` | Slice B. Add the shared parser, gate, CI wiring, and current ignored-test references. |
| `fcbcb52b7dfdac3b0a173941627a4a78f142669f` | `fix(quality): enable missing docs warnings (#4911)` | `port_to_swarm` | Slice B. Recompute the affected current workspace crate set; do not copy a stale 17-crate list blindly. |
| `b75221b97b5a142f150db5423c8943d6aea52b6a` | `ci(nightly): schedule ignored corpus and latency lanes (#10015)` | `port_to_swarm` | Slice B. Reconcile with the current nightly workflow and current test names. |
| `edbf87b4c0d2ede33232a4613889b17a7fe9d617` | `fix(lsp): match qualified subroutine implementations (#6751)` | `port_to_swarm` | Slice C. Current swarm still compares the enclosing package and exact full subroutine name in the implementation provider. |
| `b9eb5b6c9b8bb00c11ff25d4718be968d072627e` | `ci(badge-endpoints): tolerate Actions PR-creation permission denial (#0000)` | `port_to_swarm` | Slice B. Keep generation mandatory and make PR delivery an explicit repository-variable opt-in. |
| `a1e38c4bf26745439567e822c859b344de74ebfb` | `docs(release): restore 0.15.1–0.17 provenance and note completeness (#0000)` | `port_to_swarm` | Slice A. Semantic port preserving newer swarm release truth. |
| `167b1313fdd8c8221b2379e9a18de23e06f1a69e` | `docs(release): restore channel actuals and v0.15.2 closeout (#9965)` | `port_to_swarm` | Slice A. Port evidence, validator coverage, and closeout facts without overwriting newer status prose. |
| `9b2d9e4d689fbf2c0f55873e7dd2b16629f95d21` | `docs(release): reconstruct v0.14.0 channel closeout (#9968)` | `port_to_swarm` | Slice A. Preserve the audited classification boundary. |
| `fecc9de71eebb87b6a34de12261e6378ab4bb530` | `docs(release): reconcile 0.15.0–0.17.0 container actuals (#0000)` | `port_to_swarm` | Slice A. Port the durable manifest, network-free checks, and evidenced actuals. |
| `489dcce1bca0da6cf81489bbe4e0d2af2d555719` | `docs(release): correct 0.13 lineage and 0.14 boundary (#9961)` | `port_to_swarm` | Slice A. Preserve corrections to nonexistent tag assumptions. |
| `a68e515e95d35c7359c48d7cbdf11b50fcc7942b` | `docs(release): pin live tag provenance and drift checks (#9963)` | `port_to_swarm` | Slice A. Port the provenance manifest, validator, tests, and full-history CI requirement. |
| `a92da1003c0773645e59bd266e4213dd1a2b7eda` | `docs(release): reconcile 0.15.0–0.17.0 channel ledger (#9965)` | `port_to_swarm` | Slice A. Apply the append-only correction against current release surfaces. |
| `ce593d9ff5fe9216aa72c3b650675a29a5d36d07` | `docs(vscode): align setup guide with extension manifest (#10030)` | `port_to_swarm` | Slice D. Use the current extension manifest as authority. |
| `42de2c1f5a64da4b27fdbb2f4a4a855ae7dd2595` | `docs(vscode): remove stale setting references (#10031)` | `port_to_swarm` | Slice D. Extend manifest-derived parity over every maintained documentation surface. |
| `646c34bb8b7c9ff846c31e7732172696e4a8d646` | `docs(vscode): remove stale schema setting (#10033)` | `port_to_swarm` | Slice D. Include `CONFIGURATION_SCHEMA.md` in parity coverage. |

Working semantic totals for this incident interval:

```text
19  port_to_swarm dispositions (including one test-only salvage)
 3  already_equivalent_in_swarm dispositions
22  source commits semantically accounted for
 ?  patch-equivalent `-` versus target-unique `+` signs until Gate 0 runs
```

The authoritative JSON ledger contains only target-unique `+` rows, with exactly
one classification per row.

## Gate 0: authoritative patch-ID reconciliation

Run in a full-history swarm checkout that contains both remote refs:

```bash
git fetch --prune origin main
git fetch --prune release master

AUDITED_RELEASE_SHA=646c34bb8b7c9ff846c31e7732172696e4a8d646
INCIDENT_BASE=2c5ca9a8d922b03c43a884a909bf00d6deef8051
SWARM_SHA="$(git rev-parse 'origin/main^{commit}')"
RELEASE_SHA="$(git rev-parse 'release/master^{commit}')"

# Fail closed if the release branch moved beyond the audited incident set.
test "${RELEASE_SHA}" = "${AUDITED_RELEASE_SHA}" || {
  echo "release/master advanced to ${RELEASE_SHA}; extend and regenerate the audit" >&2
  exit 1
}

# Resolve every immutable input before comparison.
git cat-file -e "${INCIDENT_BASE}^{commit}"
git cat-file -e "${SWARM_SHA}^{commit}"
git cat-file -e "${RELEASE_SHA}^{commit}"

# Exact incident list.
git rev-list --reverse --no-merges \
  "${INCIDENT_BASE}..${RELEASE_SHA}"

# Patch equivalence bounded to the incident interval and immutable observation.
git cherry -v "${SWARM_SHA}" "${RELEASE_SHA}" "${INCIDENT_BASE}"
```

Do not predict the `-`/`+` distribution. For every observed `+` result, create
one ledger row with one allowed classification and concrete evidence. A broader
swarm implementation can still yield `+` and should then be classified
`already_equivalent_in_swarm`. Record any observed `-` mapping in this audit or
the receipt evidence, but do not create a false target-unique ledger row for it.

The exact `SWARM_SHA`, `RELEASE_SHA`, command output, and exit status must be
recorded in the generated receipt. If `release/master` advances, extend the
incident inventory before generating either artifact.

Then generate and validate:

```text
docs/swarm/source-syncs/2026-07-23-recover-perl-lsp-646c34b.json
docs/swarm/source-syncs/2026-07-23-recover-perl-lsp-646c34b-receipt.json
```

## Recovery slices

### Slice A — release truth and provenance

Semantically port the seven release-history commits as one coherent train.
Preserve newer swarm truth and v0.18 preparation boundaries while adding the
missing provenance manifests, validators, tests, closeout evidence, and CI
wiring. This slice must land before #4347 generates final v0.18 preparation.

### Slice B — CI, quality, and workflow repairs

Port the GHCR manifest fix, ignored-test reference gate, missing-docs baseline,
nightly ignored lanes, and badge PR opt-in. Recompute current workspace/test
inventories and review workflow-policy interactions; do not apply stale hunks
blindly.

### Slice C — product and regression repairs

Port Carton/Carmel roots, qualified subroutine resolution, and duplicate-anchor
coverage. For `6f27b566`, port only the missing end-to-end completion regression;
do not replace swarm's newer completion architecture with the source
implementation.

### Slice D — VS Code documentation parity

Use `vscode-extension/package.json` as the setting and version authority. Remove
stale references from every maintained surface and keep the manifest-derived
parity test comprehensive.

## Older #3161 scope remains blocking

This audit covers only the post-`perl-lsp#10005` incident. Before recording
`perl-lsp/master` as an ancestor of swarm, #3161 must also disposition the older
source-unique set, including the competing DAP dispatcher architecture and any
still-useful source-only test receipts. Recording ancestry first would make
ancestry-based discovery of those older differences disappear.

Do not mechanically import the historical `crates/perl-dap/src/dispatcher/`
tree. Keep swarm's `debug_adapter/dispatch.rs` unless the explicit architecture
decision says otherwise.

## Final ancestry step

Only after all old and new target-unique work has a final disposition and every
selected content port is merged, open a dedicated ancestry-only PR from an
immutable swarm parent and merge the exact audited release SHA:

```bash
git fetch --prune origin main
git fetch --prune release master

AUDITED_RELEASE_SHA=646c34bb8b7c9ff846c31e7732172696e4a8d646
SWARM_PARENT_SHA="$(git rev-parse 'origin/main^{commit}')"
CURRENT_RELEASE_SHA="$(git rev-parse 'release/master^{commit}')"

test "${CURRENT_RELEASE_SHA}" = "${AUDITED_RELEASE_SHA}" || {
  echo "release/master advanced to ${CURRENT_RELEASE_SHA}; audit new commits first" >&2
  exit 1
}

git switch -C sync/record-perl-lsp-646c34b "${SWARM_PARENT_SHA}"
git merge --no-ff -s ours "${AUDITED_RELEASE_SHA}" \
  -m "sync: record perl-lsp/master ancestry through 646c34bb"
```

Required proof:

```bash
PARENTS="$(git show -s --format='%P' HEAD)"
FIRST_PARENT="$(printf '%s\n' "${PARENTS}" | awk '{print $1}')"
SECOND_PARENT="$(printf '%s\n' "${PARENTS}" | awk '{print $2}')"

# Exactly two parents, in the intended order.
test "$(printf '%s\n' "${PARENTS}" | wc -w)" -eq 2
test "${FIRST_PARENT}" = "${SWARM_PARENT_SHA}"
test "${SECOND_PARENT}" = "${AUDITED_RELEASE_SHA}"

# The ancestry merge changes no swarm tree content.
git diff --exit-code "${FIRST_PARENT}" HEAD

# The exact audited release head is now reachable.
git merge-base --is-ancestor "${AUDITED_RELEASE_SHA}" HEAD

git diff --check
cargo check --workspace --locked
```

The ancestry PR must be delivered with GitHub's **merge commit** method. Squash
or rebase delivery destroys the second-parent proof.

## Stop conditions

- Do not perform the next swarm-to-release complete-tree promotion while a
  useful source-only product/test change remains unported or unclassified.
- Do not land additional shared-tree development directly on `perl-lsp/master`.
- If `release/master` advances beyond the audited SHA, extend the inventory and
  regenerate the disposition packet before Gate 0 or the ancestry merge.
- Do not produce a receipt claiming commands that were not executed.
- Do not combine the product ports, release-truth train, and ancestry merge into
  one unreviewable megabranch.
