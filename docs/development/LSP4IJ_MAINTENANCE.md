# LSP4IJ Integration Maintenance Runbook

This runbook describes how maintainers keep the repository-owned LSP4IJ Perl integration current, review upstream drift, prepare a bounded upstream-ready delta, and promote support only after the released subject is actually receipted.

It is the human-operable companion to #7772.

> **Current implementation boundary:** #7772 owns the final xtask command implementation and desired-state manifest. Until that lands, command names shown below are the required interface contract. If implementation chooses different names, the same change must update this runbook and its examples.

## What this runbook controls

The LSP4IJ integration is downstream product state, not an independent source of truth.

```text
perllsp / perl-dap product contracts
        ↓
repository-owned desired LSP4IJ material
        ↓
reviewable local delta
        ↓
manual upstream handoff
        ↓
upstream merge
        ↓
upstream release
        ↓
real released-host receipts
        ↓
support registry
        ↓
public documentation
```

Repository automation may maintain and prepare local material. It must not create or push an upstream branch, open/comment on an upstream issue or PR, or merge anything in `redhat-developer/lsp4ij`.

## Authority map

When a mismatch appears, repair the owning authority first.

| Concern | Authority |
| --- | --- |
| `perllsp --stdio` process identity | shipped CLI/product contract |
| configuration precedence/scope/timing | #6736 |
| generic server-native `perl.*` schema | #7768 |
| LSP4IJ settings projection | #7875 |
| release targets/archive layout/install mapping | #7876 plus release-topology authorities |
| file-family and support evidence | #7122 plus actual-host receipts |
| pinned released LSP4IJ template/profile truth | #7706 |
| actual IntelliJ/LSP4IJ LSP behavior | #7719 |
| actual IntelliJ/LSP4IJ DAP behavior | #7877 |
| desired upstream LSP/DAP/template/docs state | #7772 |
| public LSP documentation | #7726 |
| public DAP documentation | #7942 |

Do not fix a generated LSP4IJ copy when the canonical product contract is wrong elsewhere.

## Repository-owned state

#7772 should keep one reviewable LSP4IJ integration authority containing or referencing:

```text
pinned upstream release/ref/commit
pinned upstream fixture digests
minimum maintained LSP4IJ line

desired LSP template
  template.json
  settings.json
  settings.schema.json
  initializationOptions.json
  installer material / references
  file mappings
  user-facing docs expectations

desired DAP template
  adapter/install contract
  launch/attach example state
  file mappings
  docs expectations

submission lifecycle metadata
receipt/evidence references
desired-state digest
prepared-delta output
```

The exact path/layout is implementation detail. The semantic ownership above is not.

## Ordinary status and check

The #7772 interface must provide commands equivalent to:

```bash
cargo xtask integration lsp4ij status
cargo xtask integration lsp4ij check
```

### `status`

The output should answer what a maintainer needs to act:

```text
reviewed upstream release/ref/commit
pinned upstream fixture digest
desired integration digest
last compared upstream digest
local delta state
canonical inputs that made the desired state stale
submission lifecycle state
manually recorded upstream PR/ref when present
receipt/currentness disposition
```

A useful status result explains **why** state is stale. It should not merely say that files differ.

### `check`

Ordinary CI uses the deterministic/offline check.

A failure means one of these must be reconciled:

- a canonical product authority changed;
- the generated/checked LSP4IJ projection is stale;
- an intentionally pinned upstream fixture changed locally;
- submission/evidence metadata is inconsistent.

It does **not** mean moving upstream state should be fetched or silently accepted.

## Refresh against upstream

Moving upstream is an explicit maintenance operation.

The #7772 interface must provide a command equivalent to:

```bash
cargo xtask integration lsp4ij refresh --upstream-ref <exact-ref>
```

Use an exact review subject:

```text
released tag/version + resolved commit
or
explicit reviewed commit/ref
```

Do not make `main`, HEAD, or a floating branch the automatically accepted CI basis.

### Refresh sequence

1. Choose the exact upstream release/ref and record why it is being reviewed.
2. Resolve and record the exact commit.
3. Import only the bounded Perl LSP/DAP/template/docs surfaces needed by the integration fixture.
4. Review semantic changes before accepting the fixture update.
5. Run `status` and `check` again.
6. Separate upstream drift from changes in our own desired product state.
7. Commit the refreshed fixture as reviewable evidence.

A refresh proves what upstream contains. It does **not** prove that `perllsp` or `perl-dap` works through that host.

## Drift triage

Use the smallest owner that explains the delta.

| Drift | First owner/action |
| --- | --- |
| Upstream template changed; our product contract did not | refresh #7706 basis, inspect semantic delta, decide whether #7772 desired state changes |
| Canonical `perl.*` schema changed | repair/generate #7768 first, regenerate/check #7875 and #7772, rerun behavior-bearing config cells |
| Release target/archive layout changed | repair release topology/#7876 first, regenerate installer material, rerun affected managed-install cells |
| CLI/binary identity changed | repair canonical CLI authority, regenerate desired LSP/DAP command material, invalidate exact process receipts |
| Proven file-family set changed | update #7122 only from evidence, then regenerate mappings; do not add parser-only families |
| LSP4IJ capability/client behavior changed | refresh #7706; rerun affected protocol and real-host cells |
| `perl-dap` launch/config/capability changed | consume #6688/#7877; regenerate DAP material only where affected |
| Upstream docs/screenshots changed only | refresh/check documentation basis; do not rerun unrelated host semantics unless a behavior claim changed |

Do not normalize every mismatch into “regenerate everything.”

## Prepare an upstream-ready delta

The #7772 interface must provide a command equivalent to:

```bash
cargo xtask integration lsp4ij prepare-delta
```

The prepared review artifact should identify:

```text
upstream basis/ref/commit
desired-state digest
changed files
bounded patch or copied desired files
semantic summary split by LSP / installer / DAP / docs
canonical input refs that caused each change
unproven cells deliberately omitted
submission lifecycle state
```

Run preparation twice. The second run must produce the same result.

`prepare-delta` means **local upstream-ready material exists**. It does not mean anything was submitted externally.

## Manual upstream handoff

After the local delta is reviewed, the maintainer decides whether and when to submit it.

Human-owned sequence:

1. Choose/create the maintainer-owned upstream fork/branch manually.
2. Apply or transcribe the bounded prepared delta.
3. Follow LSP4IJ contribution requirements.
4. Open the upstream PR manually.
5. Record the external PR URL/number/ref as forensic identifiers in repository-owned lifecycle metadata.
   Treat those identifiers as pointers only. Revalidate `submitted_manually`, `merged_upstream`, and
   `released_upstream` from the live upstream subject before any promotion disposition.

Repository automation must not:

- create an upstream branch;
- push to a maintainer fork;
- open an upstream issue or PR;
- comment upstream;
- approve or merge upstream work.

## Lifecycle states

Keep local readiness, submission, merge, release, and support proof separate.

### `local_current`

Meaning: repository-owned desired state is reconciled with current canonical inputs and the pinned upstream basis.

Does not imply: upstream needs or contains a change.

### `local_delta_ready`

Meaning: deterministic upstream-ready material exists and has been reviewed locally.

Does not imply: a PR exists upstream.

### `submitted_manually`

Requires: a maintainer-supplied external PR/ref.

Does not imply: acceptance, merge, release, or support.

### `merged_upstream`

Requires: exact upstream merged PR/commit evidence.

Does not imply: users can obtain it from a released LSP4IJ build.

### `released_upstream`

Requires: an exact LSP4IJ release containing the merged change.

Does not imply: runtime compatibility.

### `released_and_receipted`

Requires: the released built-in subject passes the relevant actual-host receipt:

- #7719 for LSP cells;
- #7877 for DAP cells being promoted;
- #7876 managed-install receipt when installation state is part of the claim.

Only this state can promote the corresponding released-built-in support claims in #7122.

## Evidence invalidation matrix

Rerun only evidence whose subject changed materially.

| Changed subject | Required follow-up |
| --- | --- |
| LSP4IJ upstream template/capability | refresh #7706; assess protocol-profile and host invalidation |
| `perllsp` CLI/lifecycle | regenerate/check desired LSP material; rerun affected #7706/#7719 cells |
| canonical `perl.*` semantics | regenerate #7875/#7772; rerun behavior-bearing configuration cells |
| release targets/archive layout | regenerate #7876/#7772; rerun affected managed-install cells |
| file-family support evidence | regenerate mappings; run activation + semantic evidence before expansion |
| IDE/LSP4IJ version | create a new exact host subject; rerun affected #7719/#7877 cells |
| `perl-dap` capability/backend/template | rerun affected #7877 cells; regenerate DAP docs/state |
| docs-only wording | docs checks only unless the wording exposed a stale underlying claim |

Do not require unrelated expensive host receipts when the tested subject did not change.

## Post-release promotion

After upstream publishes a release containing our correction:

1. Refresh and pin the exact released LSP4IJ subject.
2. Verify the released **built-in** template/install path, not the local imported candidate.
3. Run the #7719 released-built-in LSP cohort.
4. Run the #7877 released-built-in DAP cohort only for debugger claims being promoted.
5. Run the managed-install receipt when installer/distribution state is part of the change.
6. Update #7122 from those receipts.
7. Regenerate/reconcile #7726 and #7942 documentation.
8. Check that no public claim still cites local imported material as if it were released built-in.

An upstream merge is never a public support-promotion trigger by itself.

## Partial acceptance and failure recovery

### Upstream accepts only part of the prepared delta

Record the exact accepted subset. Keep the remaining desired delta local. Do not mark the full desired state merged.

### Upstream merge changes before release

Refresh the exact merged commit/release candidate and compare it with the reviewed delta. Do not reuse the earlier local digest blindly.

### Released build differs from expectation

Treat the released artifact as the new subject. Refresh #7706 and rerun affected host/install cells before promotion.

### Real host disproves the corrected template

Keep the local/upstream lifecycle state factual, but downgrade the affected support cell. Repair the canonical or LSP4IJ-specific authority that explains the failure; do not manufacture support from the fact that upstream merged the change.

### DAP passes locally but fails managed-public-artifact cohort

Keep exact-source behavior and managed-install behavior as separate rows. Diagnose installer/archive/binary identity before changing debugger semantics.

### Support registry stays stale after a receipt

Fix the registry projection/invalidation logic. Do not hand-edit public prose to outrun #7122.

## PR review checklist

For any change to repository-owned LSP4IJ desired state:

```text
[ ] canonical source authority identified
[ ] generated/derived material refreshed
[ ] exact upstream basis unchanged or intentionally refreshed
[ ] LSP and DAP behavior evidence kept separate
[ ] no unreceipted file family added
[ ] no stale installer fallback/platform mapping introduced
[ ] no VS Code perl-lsp.* keys leaked into generic LSP4IJ server settings
[ ] local LSP4IJ status/check passes
[ ] prepare-delta is deterministic when relevant
[ ] no external write performed by automation
[ ] docs/support claims changed only when receipts justify promotion
```

## Review questions

Before merging maintenance work, a reviewer should be able to answer:

- Which canonical authority changed?
- Which generated/desired LSP4IJ artifact moved because of it?
- Did the pinned upstream basis change intentionally?
- Which evidence became stale?
- Which evidence remained valid and why?
- Is the delta only local, manually submitted, merged, released, or released-and-receipted?
- Does any public claim exceed that state?

If those answers require reconstructing commit history, the maintenance record is incomplete.

## Related issues and docs

- LSP4IJ epic: #7873
- desired upstream state/tooling: #7772
- pinned upstream/profile evidence: #7706
- settings projection: #7875
- installer topology: #7876
- LSP host receipt: #7719
- DAP host receipt: #7877
- support registry: #7122
- LSP user docs: #7726
- DAP user docs: #7942
