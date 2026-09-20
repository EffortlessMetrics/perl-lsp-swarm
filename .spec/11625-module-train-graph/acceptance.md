# Acceptance Criteria: #11625 — canonical stable module_train.v1 implementation and proof graph

This is a checked, declarative contract. It implements no xtask validation
command, current-tree probe, frontier computation, live observer, packet
rendering, GitHub metadata work, product behavior, scheduling, support
claim, release or publication. Those remain owned by the nodes this graph
declares.

## §Behavior

| Input / condition | Required result | Evidence boundary |
|---|---|---|
| The complete module train is needed as data | `train.manifest.json` covers all 52 nodes (programme and functional controllers, E00 denominator, M00S spec source, request/admission, parser-fact hierarchy, resolver chain, geometry and public boundary, L09 live cutover family, provider proofs, P11 exact-process family, claims, C-series successors) with exact issue IDs | Node set is checked against the exact expected node/issue pairs |
| A node's contract is queried | Every node carries ID, issue, title fingerprint, train role, lane, chain, one-PR outcome, authority before/after, dependency classes, claim ceiling, writer/conflict identity, spec disposition, first falsifier, controls, proof, review questions, obligations, exits, rollback, successors, identity fields, limitations | Required-field completeness is structurally checked |
| An edge's basis is questioned | Every dependency records the statement it traces to: a #11625 canonical graph section, a #11625 comment, or a leaf-body header group | Provenance strings name the source |
| Graph-law ordering is questioned | The canonical sections plus the leaf-declared precision edges are frozen edge-by-edge with their declared classes; weakening, removing or adding a forbidden edge fails | Checker law-edge and forbidden-edge tables |
| A controller, fan-in or external gate is routed to a builder | Rejected: exactly the nine reviewed controllers are non-buildable; controllers never enter the builder frontier | Role-map and controller-list laws |
| Two nodes claim the same authority-after proposition or conflict key | Rejected: uniqueness is structural | Checker uniqueness laws |
| A case or work-packet identity is needed | The manifest binds issue identities only and keeps the relation structurally pending until the E00 family materializes stable identifiers; it never invents local case identifiers | Binding-status and identifier-scan laws |
| A claim profile is evaluated | The stable graph names membership only; full closeout requires the union of the resolution-core and semantic-edit denominators | Frozen profile sets and superset law |
| A semantic graph change is proposed | The #11625 revision route owns classification and invalidation, including the metadata-only rule; the manifest never rewrites itself to pass | Revision-governance block |
| The manifest is serialized twice | The canonical semantic digest is identical across input order; two checker runs print byte-identical output | Order-invariance control and two-run proof |

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
|---|---|---|---|
| Controller selection | Controllers never become implementation leaves | frozen role map | falsifier 1 |
| Candidate-node substitution | Pull requests never appear as stable nodes | expected-node-set law | falsifier 2 |
| Dependency-class collapse | hard, evidence, external and optional stay distinct | frozen law-edge classes | falsifier 3 |
| Evidence-vs-product | E00 rows never become product implementation | frozen role map | falsifier 4 |
| Spec-source overreach | #10592 never becomes a frontier or scheduler | ceiling pattern law | falsifier 5 |
| Second lookup | #8170 never becomes a second candidate lookup algorithm | ceiling pattern law | falsifier 6 |
| Proof-as-cutover | Provider proofs never gate the live cutover substrate | frozen role map and forbidden edges | falsifier 7 |
| Early retirement | #11026 never precedes terminal consumer rows | law-edge denominator | falsifier 8 |
| Proof overreach | P11 proofs never repair product; the fan-in never executes missing scenarios | proof-component and buildability laws | falsifier 9 |
| Profile collapse | A core pass never hides the semantic-edit profile | closeout superset law | falsifier 10 |
| Cross-programme copying | Editor-intelligence and other homes are imported, never copied | chain-home and issue-copy laws | falsifier 11 |
| Conflict collision | Two nodes never share one conflict key or authority | uniqueness laws | falsifier 12 |
| Determinism | Canonical serialization is invariant under input order | canonical digest | falsifier 13 / order control |
| Live-state hygiene | No branch, pull, model or liveness state enters stable bytes | value and byte scans | falsifier 14 |

## §Contracts

| Contract | Authority | How this bundle satisfies it |
|---|---|---|
| Canonical graph sections | #11625 body | E00, M00S, M01–M10, L09, provider proof, P11 and claims encoded with section provenance |
| C-series successor graph | #11625 comment | C01 → C02 → C03 encoded as hard edges; #11114 imported as the evaluation consumer |
| C01 entry gate | #11625 comment | Case and work-packet bindings stay structurally pending; no invented identifiers; issue numbers are not evidence identities |
| Leaf dependency statements | module leaf bodies | Every leaf-declared dependency preserved with leaf-body provenance |
| Evidence denominator | #10977 and the E00 family | Encoded as evidence-role nodes with the structurally pending case binding |
| Exact-process closeout | #11624 | Encoded as the pure fan-in over a complete hard child denominator |
| Typed dependency and claim-profile semantics | #10858 | hard/evidence/optional/external classes and profile denominators consumed, not redefined |
| Train-mechanics extraction gate | #10554 | Respected: programme-local data bundle, no extraction begun or decided, OD1 routes the gate |
| Writer admission | #3982 and neighbors | Conflict keys are identities, not reservations; no writer registry created |
| Spec method | #3983 and current `.spec` tooling | Bundle shape follows the four-file train-graph precedents |
| Bundle precedents | `.spec/11764-controller-train-graph/`, `.spec/10918-emacs-train-graph/` | Same discipline: embedded structural checker, fail-closed negative controls, two-run determinism, honest not_proven |

## §API-Shape

No Rust or public API is introduced. The manifest is data; the names below
are the stable contract surfaces it declares for later nodes:

| Item | Kind | Contract shape | Dup-risk / owner |
|---|---|---|---|
| `module_train.v1` (`train.manifest.json`) | stable graph | 52 nodes, 197 typed edges with provenance, conflict keys, dispositions, controls, profiles; deterministic canonical digest | C01 #11625 (this bundle) |
| Canonical semantic digest | deterministic function | SHA-256 over order-canonical content; invariant under input order | C01; consumed by the revision route |
| xtask validation operations | executable | none here; the named offline check, graph, list and explain-static operations remain unbuilt tooling | separate tooling claim; `not_proven` here |
| Current-tree frontier | executable | none here; C02 #11626 owns probes, frontier and packets | C02 #11626 |
| Live observation | executable | none here; C03 #11627 owns the read-only live join | C03 #11627 |

## §Test-Grid

All fourteen required shift-left falsifiers of #11625, fixed order, as they
bind this manifest. Every mutation is executed as an in-memory negative
control by the embedded checker in `checklist.md`; a conformant checker must
reject each one deterministically. Falsifiers 9, 11 and 14 carry doubled
controls; the acceptance-bullet mutation classes beyond the numbered list
(hard cycle, duplicate owner, invented case identity, premature binding
promotion, unauthorized external action) carry their own controls; and an
order-invariance canonicalization control runs whose rejected subject is an
order-sensitive canonicalization, not the shuffled input itself —
twenty-two controls in total.

| # | Falsifier mutation | Kind | Required verdict | First discriminating control |
|---:|---|---|---|---|
| 1 | A controller (e.g. #4240) is reclassified as an implementation leaf | wrong-subject | rejected: the frozen role map fixes every node's role; controllers never enter the builder frontier | Flip a controller's train_role; role-map law must fail |
| 2 | A pull request number is encoded as a stable node | wrong-subject | rejected: the node set admits issue nodes with exact expected pairs only | Inject a node with a pull-number issue; expected-node-set law must fail |
| 3 | An evidence edge is collapsed to hard (or any two classes merged) | partial | rejected: law edges carry exactly their declared classes | Flip the E00A→#8497 edge to hard; law-edge class law must fail |
| 4 | An E00 evidence row is treated as product implementation | wrong-subject | rejected: the evidence family keeps its evidence role and bounded ceiling | Flip an E00 node's role; role-map law must fail |
| 5 | #10592 is treated as the current frontier or scheduler | wrong-subject | rejected: the spec source's ceiling renounces frontier and scheduling | Replace its ceiling with a scheduling claim; ceiling law must fail |
| 6 | #8170 is treated as a second candidate lookup | wrong-subject | rejected: the overlay ceiling renounces lookup algorithms | Replace its ceiling with a lookup claim; ceiling law must fail |
| 7 | A provider helper or proof is treated as the live cutover | wrong-subject | rejected: proof nodes keep the proof role and never gate L09A | Flip a proof node's role; role-map law must fail |
| 8 | #11026 is allowed before all admitted consumer rows are terminal | partial | rejected: the retirement denominator includes every consumer row | Remove the L09F→L09G edge; law-edge law must fail |
| 9 | A P11 proof repairs product, or the fan-in executes missing scenarios | wrong-subject | rejected: proofs cannot repair product and the fan-in is never buildable | Make P11F buildable or add a production component to a proof node; buildability and proof-component laws must fail |
| 10 | A core profile pass hides the non-passing edit profile | partial | rejected: full closeout requires the union of core and edit denominators | Drop P11D from full closeout; superset law must fail |
| 11 | An editor-intelligence, URI or lifecycle node is copied instead of imported | wrong-subject | rejected: one home programme per node and no home-train issue as a node | Flip a node's chain home or its issue to the editor-intelligence issue; chain and issue-copy laws must fail |
| 12 | Two nodes are emitted with one conflict key or shared authority | partial | rejected: conflict keys and authority-after propositions are unique | Duplicate a conflict key; uniqueness law must fail |
| 13 | Deterministic bytes change with insertion order | instrument | rejected: the canonical digest is invariant under input order | Shuffle nodes, edges and successors; digest comparison must hold |
| 14 | The graph embeds live branch, pull, model or liveness state | instrument | rejected: parsed values and raw bytes are scanned fail-closed | Append a live token to a limitation; live-state scan must fail |

## §Blast-Radius

| Surface | Effect |
|---|---|
| Repository bytes | Adds exactly the four files of this bundle; nothing else changes |
| Product/runtime | None — no Rust, configuration, generated artifact or executable surface changes |
| GitHub state | None — no issue, label, pull, review or metadata mutation |
| Later train nodes | C02 #11626 and C03 #11627 consume `train.manifest.json` as the stable topology input; #10592 compiles packets alongside; every module leaf consumes its node contract; issue bodies remain the per-node authority |
| Rollback | Revert the single commit; no downstream durable state depends on it |

## Claim boundary

This bundle makes the module train's stable topology durable: the complete
52-node graph with 197 typed, provenance-bound edges, writer and
conflict identities, dispositions, controls, six frozen claim profiles, the
structurally-pending case binding law, the controller rejection list, and a
deterministic canonical digest, plus twenty-two fail-closed negative
controls. It does not prove that the topology is the semantically correct
reading of every leaf body (that is this review's job, and the #11625
revision route's later), that the xtask module-train operations work
(unbuilt), that the E00 case identities exist (structurally pending), that
the graph stays current as issues evolve (the revision route owns
invalidation), or that any module behavior, cutover, receipt, profile or
support claim holds (the lanes own those, unbuilt). Those remain
`not_proven` here.

## Non-goals

No xtask validator command, current-tree probe, frontier or readiness
computation, live observer, packet rendering, GitHub metadata work, product
implementation, scheduler, support claim, release or publication.
