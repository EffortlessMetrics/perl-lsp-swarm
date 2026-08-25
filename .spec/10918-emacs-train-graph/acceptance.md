# Acceptance Criteria: #10918 — canonical stable emacs_train.v1 topology graph

This is a checked, declarative contract. It implements no xtask validation
command, semantic revision evaluator, current-tree probe, readiness or
frontier computation, source-context resolution, live observer, packet
rendering, GitHub metadata work, host execution, scheduler, support claim,
release or publication. Those remain owned by the nodes this graph declares.

## §Behavior

| Input / condition | Required result | Evidence boundary |
|---|---|---|
| The complete Emacs support train is needed as data | `train.manifest.json` covers all 55 nodes (programme, E00 architecture, E01 stable train, E01R revision governance, shared reliability adoption, historical foundations, subject/adapter/profile/host/root/public/projection lanes, E02/E04/E06 planes, dogfood chain, journeys catalog, routing policy) with exact issue IDs | Node set is checked against the exact expected node/issue pairs |
| A node's contract is queried | Every node carries ID, issue, title fingerprint, train role, lane, chain, one-PR outcome, authority before/after, dependency classes, claim ceiling, writer/conflict identity, spec disposition, first falsifier, controls, proof, review questions, obligations, exits, rollback, successors, identity fields, limitations | Required-field completeness is structurally checked |
| An edge's basis is questioned | Every dependency records the statement it traces to: the #10918 canonical seed graph or corrected functional DAG, a #10918 comment correction, the #7979/#8706 programme header, or an E00 section | Provenance strings name the source |
| Graph-law ordering is questioned | The corrected DAG and the comment corrections are frozen edge-by-edge with their declared classes; weakening, removing or adding a forbidden edge fails | Checker law-edge and forbidden-edge tables |
| A controller, fan-in or external gate is routed to a builder | Rejected: those roles are never buildable; external action requires an explicit external authorization dependency | Role and authorization laws |
| Two nodes claim the same authority-after proposition or conflict key | Rejected: uniqueness is structural | Checker uniqueness laws |
| The existing candidate for #11366 is encoded | It appears only as the canonical adoption rule (pull 8026, confirmed by #10930), never as a node | Candidate-adoption block law |
| A semantic graph change is proposed | E01R #11770 owns classification and invalidation, including the metadata-only rule; the manifest never rewrites itself to pass | Revision-governance block |
| Optional or unavailable evidence rows | Remain explicit; missing or instrument-failed evidence is `not_proven`, never pass | Evidence-semantics block |
| The manifest is serialized twice | The canonical semantic digest is identical across input order; two checker runs print byte-identical output | Order-invariance control and two-run proof |

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
|---|---|---|---|
| Candidate-node substitution | Pull requests never appear as stable nodes | candidate-adoption law | falsifier 1 |
| Accidental serialization | #11366 never blocks #8734 or #8755 | forbidden-edge law | falsifier 2 |
| Observation authority | Adapters never invent host-observation semantics | adapter law | falsifier 3 |
| Ungoverned cells | Actual-host leaves never emit durable pass cells without the producer | producer law | falsifier 4 |
| Observation/semantics separation | #8834 observes stock roots without semantic verdicts | forbidden-edge law | falsifier 5 |
| Substrate independence | #8842 never waits for semantic journeys | forbidden-edge law | falsifier 6 |
| Substrate promotion bound | Substrate success never promotes semantic rows | ceiling law | falsifier 7 |
| Strict fan-in denominators | Partial denominators never enable complete-cut claims | denominator law | falsifier 8 |
| Optional-breadth neutrality | Optional and platform breadth never becomes hard by ordering | optional-class law | falsifier 9 |
| Policy authority bound | #9375 is never a receipt or support authority | role law | falsifier 10 |
| External authorization | External action never enters the ordinary frontier unauthorized | authorization law | falsifier 11 |
| Durable-byte hygiene | No live SHA, path, PR, check, review or model state in stable bytes | value and byte scans | falsifier 12 |
| Spec-reference discipline | Detailed leaf specs are referenced to #11717, never duplicated | spec-authority law | falsifier 13 |
| Path neutrality | Current source files are never stable semantic identity | path law | falsifier 14 |
| Determinism | Canonical serialization is invariant under input order | canonical digest | order-invariance control |
| Revision ownership | Every future semantic change has an invalidation owner | E01R node law | revision-governance block |

## §Contracts

| Contract | Authority | How this bundle satisfies it |
|---|---|---|
| Durable architecture | #11716 / `.spec/11716-emacs-support-architecture/` | Consumed as semantic input; its planes, ceilings, authority split and ordering are encoded, not re-derived or widened |
| Stable-graph corrections | #10918 review comments | Fan-in directions, #11766/#11768/#11770 additions and the four negative topology fixtures are encoded as laws |
| Programme controller and parent | #8706 / #7979 | Exact issue IDs only; controllers never enter builder frontiers |
| Leaf dependency statements | #7777–#8865 leaf bodies | Every edge traces to the #10918 seed graph or corrected DAG, a comment correction, the programme header, or an E00 section |
| Typed dependency/evidence semantics | #10858 | hard/evidence/optional/external classes consumed, not redefined |
| Evaluation vocabulary | #11114 | Declared as the dogfood chain's consumed vocabulary; never cloned |
| Context-plane ceiling | #11718 | Navigation context stays a consumed plane; the manifest encodes no source paths |
| Shared receipt and reliability authorities | #10527 / #10894 / #7777 / #11766 | Consumed, never cloned; no Emacs-local ontology |
| Shared packet contracts | #10872 / #10881 | Declared as E06's adapter targets; never cloned or merged |
| Train-mechanics extraction gate | #10554 | Respected: the manifest is programme-local; OD1 routes extraction to the gate |
| Writer admission | #4177 / #3982 / #3957 | Conflict keys are identities, not reservations; no writer registry created |
| Spec method | #3983 and current `.spec` tooling | Bundle shape follows `SPEC_TEMPLATE.md` plus the T01 four-file precedent |
| Bundle precedents | `.spec/11764-controller-train-graph/` (#11764), `.spec/11716-emacs-support-architecture/` (#11716), `.spec/10894-editor-host-reliability/` (#11766) | Same discipline: embedded structural checker, fail-closed negative controls, two-run determinism, honest not_proven |

## §API-Shape

No Rust or public API is introduced. The manifest is data; the names below
are the stable contract surfaces it declares for later nodes:

| Item | Kind | Contract shape | Dup-risk / owner |
|---|---|---|---|
| `emacs_train.v1` (`train.manifest.json`) | stable graph | 55 nodes, 124 typed edges with provenance, conflict keys, dispositions, controls; deterministic canonical digest | E01 #10918 (this bundle) |
| Canonical semantic digest | deterministic function | SHA-256 over order-canonical content; invariant under input order | E01; consumed by E01R #11770 |
| xtask validation operations | executable | none here; the offline check and graph operations named by #10918 remain unbuilt tooling | separate tooling claim; `not_proven` here |
| Semantic revision governance | executable | none here; E01R owns classification and invalidation | E01R #11770 |

## §Test-Grid

All fourteen required graph regressions of #10918, fixed order, as they bind
this manifest. Every mutation is executed as an in-memory negative control by
the embedded checker in `checklist.md`; a conformant checker must reject each
one deterministically. Falsifier 12 carries two controls (a parsed-value
injection and a raw-byte injection); the acceptance-bullet mutation classes
beyond the numbered list (stage inflation, duplicate owner, hard cycle,
controller selection) carry their own controls; and an order-invariance
canonicalization control runs whose rejected subject is an order-sensitive
canonicalization, not the shuffled input itself — twenty controls in total.

| # | Falsifier mutation | Kind | Required verdict | First discriminating control |
|---:|---|---|---|---|
| 1 | Pull 8026 is encoded as a stable node rather than a live-candidate reuse rule for #11366 | wrong-subject | rejected: the node set admits issue nodes only and the candidate appears solely in the adoption rule | Inject a node with issue 8026; expected node-set law must fail |
| 2 | #11366 blocks #8734 or #8755 by ordering | partial | rejected: the fixture substrate never blocks runner conformance or subject fan-in | Add a FIXT dependency to RUNCONF; forbidden-edge law must fail |
| 3 | #8776 or #8795 invents separate host-observation semantics instead of #11360 | wrong-subject | rejected: adapters carry hard observation edges and own no private semantics | Remove the OBS edge from ADP_E; adapter law must fail |
| 4 | A journey leaf produces durable pass cells without #11361 | partial | rejected: every actual-host leaf carries a hard producer edge | Remove the PROD edge from HOST_E29; producer law must fail |
| 5 | #8834 depends on completed semantic verdicts solely to observe stock root | partial | rejected: the observation fan-in carries matrix edges only | Add a ROOT_E_SEM dependency to ROOT_OBS_FAN; forbidden-edge law must fail |
| 6 | #8842 depends on every semantic journey | partial | rejected: the Linux substrate carries no host, profile or semantics edges | Add a HOST_E29 dependency to LINUX; forbidden-edge law must fail |
| 7 | #8842 substrate success promotes public semantic rows | wrong-subject | rejected: the substrate ceiling bounds promotion to install and fresh-process evidence | Replace the LINUX claim ceiling with a semantic-row claim; ceiling law must fail |
| 8 | A partial denominator enables #8858, #8862 or #8865 complete-cut claims | partial | rejected: strict fan-ins require complete hard denominators | Remove PUB_L_R from REG; denominator law must fail |
| 9 | Optional, upstream, macOS, Windows or TRAMP breadth becomes an initial-Linux hard dependency by ordering | partial | rejected: optional breadth edges stay optional | Flip the CERT #9310 edge to hard; optional-class law must fail |
| 10 | #9375 becomes a second receipt or support authority | wrong-subject | rejected: the policy keeps its evidence_policy role and bounded ceiling | Reclassify POLICY as implementation; role law must fail |
| 11 | External or upstream action enters the ordinary implementation frontier without authorization | partial | rejected: explicit authorization stays external-class and gate-only | Add a hard #EXPLICIT-AUTHORIZATION edge to SUBJ_CORE; authorization law must fail |
| 12 | Current SHA, path, PR, check, review or model state enters stable manifest bytes | instrument | rejected: parsed values and raw bytes are scanned fail-closed | Append a live token to a limitation; live-state scan must fail |
| 13 | A detailed per-leaf spec is duplicated in the train instead of referenced to #11717 | partial | rejected: every node's spec authority references #11717 with bounded prose | Point one spec authority elsewhere; spec-authority law must fail |
| 14 | A current source file or symbol is treated as stable semantic component identity instead of #11718 navigation | wrong-subject | rejected: identity fields and components carry semantic names only | Inject a source path into allowed components; path law must fail |

## §Blast-Radius

| Surface | Effect |
|---|---|
| Repository bytes | Adds exactly the four files of this bundle; nothing else changes |
| Product/runtime | None — no Rust, configuration, generated artifact or executable surface changes |
| GitHub state | None — no issue, label, PR, review or metadata mutation |
| Later train nodes | E01R, the E02/E04/E06 planes, the dogfood chain and every implementation lane consume `train.manifest.json` as the stable topology input; their issue bodies remain the per-node authority |
| Rollback | Revert the single commit; no downstream durable state depends on it |

## Claim boundary

This bundle makes the Emacs train's stable topology durable: the complete
55-node graph with 124 typed, provenance-traced edges, writer/conflict
identities, dispositions, controls, the canonical candidate-adoption rule and
revision ownership, plus a deterministic canonical digest and sixteen
fail-closed negative controls. It does not prove that the topology is the
semantically correct reading of every leaf body (that is this PR's review
job, and E01R's later), that the xtask validation operations work (unbuilt),
that the graph stays current as issues evolve (E01R owns invalidation), or
that any Emacs behavior, subject materialization, host journey, public
artifact, registry row or support claim holds (the lanes own those, unbuilt
or in flight). Those remain `not_proven` here.

## Non-goals

No xtask validator command, semantic revision evaluator, current-tree probe,
readiness or frontier computation, source-context resolution, live observer,
packet rendering, GitHub metadata work, host execution, dogfood execution,
scheduler, support claim, release or publication.
