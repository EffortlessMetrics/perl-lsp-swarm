# Context: #11626 — exact current-tree status and safe offline frontier from module_train.v1

## Problem

The stable module train topology (`module_train.v1`, `.spec/11625-module-train-graph/train.manifest.json`, landed via #12043 / merge `7cc48f77`) is checked data, but nothing on the tree can answer, offline and deterministically: which module nodes' hard dependencies are satisfied on this exact tree, and which leaves may therefore proceed now. Every consumer (controllers, leaf packets, C03 live observation, #11114 evaluation) would re-derive a different graph from prose.

## Why this approach

Consume the C01 manifest strictly as DATA through one fail-closed xtask surface. The manifest already encodes everything the frontier needs: typed dependency classes (`#10858`), train roles, writer classes, conflict keys, claim-profile membership, the case/work-packet binding status, and the controller-satisfaction law (`limitations[1]`: controller-family dependencies are satisfied transitively; controllers never gate builders directly). Deriving from that data — rather than cloning or hardcoding it — keeps #11625 the single topology authority and makes every projection reproducible from bytes.

## Current state (honest, as of this bundle)

This PR lands **slice one** of the C02 acceptance:

- `cargo xtask module-train status --tree HEAD` — every node projected into the typed current-tree state vocabulary, with implementation presence kept independent from frontier state, and typed reason codes (blocking and visibility-only) per node.
- `cargo xtask module-train next --tree HEAD` — the safe offline parallel frontier: all and only hard-ready, role-valid, conflict-recorded leaves, with writer classes shown as ceilings (never quotas) and evidence/external limitations kept visible.
- A fail-closed loader: strict schema (unknown keys rejected), C01's structural laws (successor/reverse-edge identity, title fingerprints, uniqueness, role/buildable law, hard/evidence acyclicity, import-relation class agreement, wording laws), and a pinned canonical digest.

On the current tree (`112bc2cb2` at authoring) the derived frontier is `C02, E00A, M01, M07A`: C02's only hard dep (C01) is landed; E00A's only hard dep is the EVID controller (topology-satisfied); M01 and M07A carry no hard node deps (M01's E00A/E00B edges are evidence-class: visible limitations, not blockers). C03 is `blocked_hard` on C02; M00S carries the structurally-pending case/work-packet binding as a typed reason; L09G is hard-blocked on all six admitted cutovers.

**Not landed in this slice (recorded residuals, never guessed):**

- Per-node semantic implementation/retirement probes beyond the single C01 manifest probe (the E00-class, M-class, L09-class, P11-class probe contracts in the #11626 body). Until they exist, every non-C01 node's implementation presence is reported `not_proven`.
- `explain` (bounded static agent packet projection), `graph`, and `#11114` packet handoff.
- Arbitrary-tree checkout (`--tree` accepts `HEAD` only) and JSON output.
- Supersession and incomplete-current-tree state transitions: the vocabulary is present; a populated `supersessions` list fails closed pending a defined projection.

The full acceptance remains open on #11626; this slice is the deterministic frontier derivation with the manifest's digest binding, which is the foundation every remaining piece consumes.

## Authority and ownership

- Topology authority: #11625 (`module_train.v1`, revision route through C01's owner issue; semantic revisions invalidate these projections, which re-derive).
- This projection's owner: #11626 (C02). Live observation stays with #11627 (C03).
- Evidence authority: #8479 and the E00 family; case/work-packet bindings remain `structurally_pending` by manifest law and are treated as pending, never satisfied.
- Method authorities: #3983 (preparation), #3982 (writer admission; conflict keys are identities, not reservations), #10858 (typed dependency and claim-profile contracts).

## Durable laws consumed

- `not_proven` law: missing, ambiguous, or instrument-failed selectors are `not_proven`, never pass; implementation presence is never guessed from names or file existence.
- Issue identity law: issue numbers are reviewed proposition references, never executable evidence identities.
- Typed edge effects: `hard` blocks coding starts; `evidence`/`optional` stay visible without becoming false hard blockers; `external` stays authorization-blocked; cross-programme hard imports honestly report that this train's offline data cannot establish the home train's tree state.
- Role rejection: controllers, fan-in surfaces, external gates, and claims never appear as implementation starts.
- Independence: implementation presence, dependency readiness, evidence obligations, and support stages never collapse into one signal.
- Determinism: canonical presentation order, sorted reasons, no timestamps, no ambient paths; insertion order moves no byte.

## Encoding decisions and traceability

- **Canonical digest (pinned):** recursive content walk identical in shape to C01's (`n;`, `b:True;`, `i:<n>;`, escaped `s:...;`, sorted arrays, sorted object keys) with **byte-ordinal** sorting, SHA-256 uppercase hex. Pinned at `10BA2619…C104FB` for the current manifest revision. C01's own semantic SHA (`9B46B0F8…988090`, recorded as provenance and emitted in every output) uses PowerShell's culture-sensitive `Sort-Object`; byte-equality between culture and ordinal sorts is not a durable cross-language contract, so the pin is this tool's documented canonicalization. Either way, any manifest byte change moves the computed digest and fails the pin loudly.
- **Controller satisfaction:** hard edges to controller nodes are satisfied by the validated topology per manifest `limitations[1]` — data, not invention.
- **Binding-pending gate:** manifest `case_work_packet_bindings.consumers` with status ≠ `bound` produce a typed `blocked_evidence` contribution (`case_work_packet_binding:<status>`), visible even when a hard block dominates the state.
- **One real probe:** C01's declared positive surface is the validated manifest itself (node C01 / #11625); that is the only node reported `landed_current_tree`. Everything else is `not_proven` presence with frontier state computed from dependencies.

## Shared-mechanics disposition (#10554)

Verified against current `main`: no shared checked-train library exists (C01's closeout records the same finding; the two prior train artifacts are `.spec` data bundles). This slice adds Rust-side generic graph laws (successor identity, acyclicity, class agreement) inside one task module; it does not begin an extraction. The concrete-reuse gate remains OD1 routed to #10554. When a second consumer (e.g. C03 or the zed train) needs the same laws, extraction through #10554 is the route — not a private copy.

## Compatibility with the repository operating contract (`AGENTS.md`)

- No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`/`dbg!` in production code; errors are typed and contextual.
- Read-only: no network, no GitHub access, no branch/worktree mutation, no scheduling, no product behavior change. The only subprocess is local read-only `git` (`rev-parse`, `status --porcelain`).
- Focused proof only: scoped xtask tests, scoped fmt/clippy; no workspace-wide builds were run.

## Open decisions respected, not decided

- OD2 (#8479): case/work-packet identity binding stays structurally pending; this tool consumes the status, never promotes it.
- OD4 (#11114): packet-consumer selection stays with #11114; this slice makes no packet.
- Revision follow-up recorded in C01's closeout ("a later revision may add C02 to `case_work_packet_bindings.consumers` for completeness; law is already global"): deliberately NOT taken here — editing the manifest is a #11625 revision-route event that would invalidate and force re-derivation for zero semantic gain, since the binding law is global. Left available for a future classified revision.

## Adoption, rollback, transfer and stop

- **Adoption:** `cargo xtask module-train status --tree HEAD` / `next --tree HEAD` from the repo root.
- **Rollback:** revert this PR; the prior state (no offline projection) returns. Nothing else consumes the surface yet.
- **Transfer:** a semantic revision of the manifest updates the digest pin and re-derives; the revision route is #11625.
- **Stop conditions:** digest drift without a classified revision; a populated supersessions list; a manifest violating structural laws; a non-`HEAD` tree request. Each fails closed with a precise message.

## Links

- Controlling issue: #11626 (C02). Parent programme: #8133 / #4240.
- Topology data: `.spec/11625-module-train-graph/train.manifest.json` (#11625, merged #12043).
- Live successor: #11627 (C03). Dogfood consumer: #11114.
- Typed edges: #10858. Writer admission: #3982. Shared mechanics gate: #10554.
