# Acceptance Criteria: #11770 — Emacs semantic train revision governance ledger

This is a checked, declarative revision-governance contract. It implements
no xtask diff/change-check/impact command, live GitHub observation, packet
renderer, readiness or frontier computation, host proof, scheduler, support
claim or publication. Those remain owned by the planes and nodes the stable
graph declares.

## §Behavior

| Input / condition | Required result | Evidence boundary |
|---|---|---|
| The ten material movements of the Emacs train are queried | `revisions.ledger.json` records exactly the ten frozen movements (`emacs_train_revision.v1`): the #10894 authority insertion, the six decompositions (#8755, #8834, #8838, #11717, #11718, #10936), the #11768 governed-catalog insertion, the #9413 incorporation and the #11366 retarget | Coverage table law; entry set is checked against the exact expected (kind, subject, sequence) triples |
| A revision kind is queried | Five structural kinds (decompose, insert, incorporate, retarget, supersede) compose with the closed 23-value shared semantic-class vocabulary of #11770/#11375 | Vocabulary laws; unknown kinds and classes fail closed |
| A ledger reference is questioned | Every node id, issue number and external authority id resolves against the consumed `emacs_train.v1` manifest as data | Ledger-vs-manifest cross-validation |
| A decomposition is questioned | The successor set equals the subject's hard dependency children minus declared retained prerequisites, the parent stays a non-executing fan-in (or dogfood aggregator), and every child reaches the parent through its manifest successor set | Decomposition-wiring and fan-in-reachability laws |
| Identity preservation is questioned | Every acceptance cell maps to exactly one declared owner or explicit retirement; every unique work item is preserved or explicitly retired | Cell-exclusivity and work-drain laws |
| An insert is questioned | The manifest carries every declared wiring edge with exactly the declared class; #11768's fifteen governance edges are frozen edge-by-edge | Insert-wiring law |
| The incorporation boundary is questioned | #9413 stays a separate child train: no node added, boundary declared, external authority resolved | Incorporate-boundary law |
| The retarget is questioned | #11366's after-state (historical role plus the canonical candidate-adoption rule) matches the manifest bytes exactly | Retarget-state-match law |
| A metadata-only movement is proposed | Zero invalidations; semantic identity preserved; nothing churns | Metadata-neutrality law |
| A material movement is proposed | At least one typed invalidation with basis, reason and ruling evidence; required synchronization names exact controllers with typed actions | Material-invalidation and sync-explicitness laws |
| Ledger history is questioned | Entries are append-only: contiguous sequences, matching identifiers, frozen initial coverage, immutable entries | Append-only and movement-coverage laws |
| An automatic action is proposed | Rejected: every entry forbids automatic execution; the ledger performs and triggers no GitHub mutation | No-automatic-mutation law |
| The ledger is serialized twice | The canonical digest is identical under input order; two checker runs print byte-identical output | Order-invariance control and two-run proof |

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
|---|---|---|---|
| Silent node drop | A decompose entry lists exactly its split children | decomposition-wiring law | falsifiers 8, 1 |
| Phantom child | A successor that is not a dependency child is rejected | decomposition-wiring law | falsifier 9 |
| Fan-in orphaning | Every split child reaches the parent fan-in | fan-in-reachability law | falsifier 10 |
| Cross-family evidence | One client family cannot satisfy another's acceptance cell | cell-exclusivity law | falsifier 2 |
| Unwired insertion | Inserted authority without manifest wiring is rejected | insert-wiring law | falsifiers 3, 4 |
| Boundary erosion | A separate child train never becomes Emacs nodes | incorporate-boundary law | falsifier (surface vocabulary row) |
| Stage resurrection | Completed substrate cannot be recreated as new work | retarget-state-match law | falsifier 7 |
| Metadata churn | Harmless movement invalidates nothing | metadata-neutrality law | falsifier 6 |
| Work disappearance | Supersession and transfer never drop unique work | work-drain law | falsifier 5 |
| Vague synchronization | Free-form "update related work" is insufficient | sync-explicitness law | falsifier 12 |
| History mutation | Entries are never edited, removed or reordered | append-only and coverage laws | falsifier 13 |
| Live state in stable bytes | No SHA, timestamp, branch or check state in ledger bytes | value and byte scans | falsifier 14 |

## §Contracts

| Contract | Authority | How this bundle satisfies it |
|---|---|---|
| Governing issue | #11770 | Ten frozen movements recorded; impact laws encoded as checker laws; builder-packet fields (required_sync, forbidden automatic action) carried per entry |
| Reference revision contract | LSP4IJ #11375 | Semantic-class vocabulary composed exactly; proposal/impact discipline adapted, never duplicated |
| Stable graph | `emacs_train.v1` (#10918) | Cross-validated as data; E01 bytes remain immutable historical evidence |
| Durable architecture | #11716 | Consumed; no architecture decision made here |
| Generic mechanics gate | #10554 | Respected (OD1): no generic framework extracted |
| Spec method | #3983 and current `.spec` tooling | Bundle shape follows `SPEC_TEMPLATE.md` plus the four-file bundle precedent |
| Bundle precedents | `.spec/11764-controller-train-graph/`, `.spec/10918-emacs-train-graph/`, `.spec/11625-module-train-graph/`, `.spec/11763-issue-controller-architecture/` | Same discipline: embedded fail-closed checker, negative controls, two-run determinism, honest `not_proven` |

## §API-Shape

No Rust or public API is introduced. The ledger is data; the surfaces it
declares for later nodes:

| Item | Kind | Contract shape | Dup-risk / owner |
|---|---|---|---|
| `emacs_train_revision.v1` (`revisions.ledger.json`) | stable ledger | 5 revision kinds, 23 semantic classes, 13 invalidation surfaces, 6 sync actions, 15 ledger laws, 10 append-only entries; deterministic canonical digest | E01R #11770 (this bundle) |
| Canonical ledger digest | deterministic function | SHA-256 over order-canonical content; invariant under input order | E01R; consumed by future tooling claims |
| xtask diff / change-check / impact operations | executable | none here; the offline operations named by #11770 remain unbuilt tooling | separate tooling claim; `not_proven` here |
| Live collaboration consumption | executable | none here; the live overlay (#10930) consumes required_sync only with an exact live snapshot | #10930 |

## §Test-Grid

Fourteen required revision regressions in fixed order. Every mutation is
executed as an in-memory negative control by the embedded checker in
`checklist.md`; a conformant checker must reject each one deterministically.
Regression 14 carries two controls (a parsed-value injection and a raw-byte
injection); the acceptance-bullet mutation classes beyond the numbered list
(sequence tampering, unknown semantic class, permitted automatic execution,
kind misclassification, wiring undercoverage, incorporation claiming a node,
adoption-rule drift) carry their own controls; and an order-invariance
canonicalization control runs whose rejected subject is an order-sensitive
canonicalization — twenty-three controls in total.

| # | Falsifier mutation | Kind | Required verdict | First discriminating control |
|---:|---|---|---|---|
| 1 | A decompose entry drops one subject class (an acceptance cell is removed) | partial | rejected: every split child must own at least one cell | Remove the Linux-generation cell from REV-002; cell-coverage law must fail |
| 2 | The #8834 split lets Eglot evidence satisfy the lsp-mode cell | wrong-subject | rejected: each cell maps to exactly one owner and each child owns its own family's cells | Repoint the lsp-mode observation cell in REV-003 to ROOT_E_OBS; cell-coverage law must fail |
| 3 | The #10894 insertion leaves #8734 without the generic authority wiring | partial | rejected: the manifest must carry each declared wiring edge with exact class | Remove the #10894 evidence dependency from RUNCONF in a manifest copy; insert-wiring law must fail |
| 4 | The #11768 insertion leaves actual-host packets current without governed cells | partial | rejected: the fifteen governance edges are frozen edge-by-edge with exact classes | Flip the HOST_E29 governance edge class in REV-008; insert-wiring law must fail |
| 5 | Unique candidate work disappears during supersession or transfer | partial | rejected: every work item is preserved to a declared owner or explicitly retired | Point a REV-002 work item at an undeclared node; work-drain law must fail |
| 6 | A metadata-only edit churns all context and packets | instrument | rejected: metadata-only entries carry zero invalidations | Reclassify REV-002 as metadata_only while keeping its invalidations; metadata-neutrality law must fail |
| 7 | Completed #11366 is recreated as new work | wrong-subject | rejected: the retarget after-state must match the manifest's historical role | Flip REV-010's declared after-state role to implementation; retarget-state-match law must fail |
| 8 | A revision silently drops a node from a decomposition | partial | rejected: the successor set equals the hard children minus retained prerequisites exactly | Remove SUBJ_L from REV-002's successors; decomposition-wiring law must fail |
| 9 | A phantom successor invents a split child | partial | rejected: a successor that is not a dependency child is rejected | Add OBS to REV-002's successors; decomposition-wiring law must fail |
| 10 | Decomposition loses reachability to the fan-in | partial | rejected: every child's manifest successor set contains the parent | Remove SUBJ_FAN from SUBJ_CORE's successors in a manifest copy; fan-in-reachability law must fail |
| 11 | A ledger entry references a node absent from the manifest | instrument | rejected: every reference resolves against the consumed manifest | Rename a REV-002 successor to a ghost node; manifest-reference law must fail |
| 12 | Impact output claims synchronization complete without naming affected controllers | partial | rejected: required_sync names exact resolvable controllers with typed actions | Replace REV-002's controller with free-form text; sync-explicitness law must fail |
| 13 | The ledger mutates history: an entry is removed or a sequence is tampered | instrument | rejected: append-only sequences with frozen coverage | Remove REV-004 entirely; append-only/coverage law must fail |
| 14 | Live state enters stable ledger bytes | instrument | rejected: parsed values and raw bytes are scanned fail-closed | Append a live token to a REV-001 reason; live-state scan must fail |

## §Blast-Radius

| Surface | Effect |
|---|---|
| Repository bytes | Adds exactly the four files of this bundle; nothing else changes |
| Product/runtime | None — no Rust, configuration, generated artifact or executable surface changes |
| GitHub state | None — no issue, label, PR, review or metadata mutation |
| Later train nodes | Future Emacs revisions append entries here; the exact-tree, overlay, packet and receipt planes consume typed invalidations; the E01 manifest stays the frozen stable input |
| Rollback | Revert the single commit; no downstream durable state depends on it |

## Claim boundary

This bundle makes Emacs train revision governance durable: the versioned
`emacs_train_revision.v1` ledger schema, the initial append-only ledger
recording the ten frozen material movements with identity preservation and
typed invalidation semantics, cross-validation against the stable
`emacs_train.v1` manifest, and twenty-three fail-closed negative controls
with
a two-run determinism proof. It does not prove that the offline diff,
change-check or impact operations named by #11770 exist or pass (unbuilt
tooling, a separate claim), that the recorded movements are the complete
history of the Emacs issue graph (the governing issue's frozen list is the
authority), that any affected spec, context, packet or proof has actually
been re-derived (their owning planes hold that state), or that any live
candidate, check or review is current (the overlay plane owns live truth).
Those remain `not_proven` here.

## Non-goals

No xtask validator command, generic graph-diff framework, live GitHub
observer or mutator, automatic issue/body/label/PR update, packet renderer,
readiness or frontier computation, host execution, dogfood execution,
scheduler, support claim, merge authorization, release or publication.
