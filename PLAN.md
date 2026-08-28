# Plan: #11549 — Conjunctive install-route classification and preferred-route selection

> Status: DURABLE TRACKED PLAN (refreshed 2026-08-28 against #11549, #11575, and
> the current dependency issues). This file is repository-carried planning
> authority for scope, prerequisites, and proof obligations; issue #11549 is
> the authority for human product rulings. It is not an executable input
> contract until the validated route schema/catalog owned by #10333 exists.

## 0. Claim and entry

**Claim:** perl-lsp's install surfaces assert route facts across seven
independent hard dimensions: identity/topology, platform/target,
product-unit/lifecycle, integrity/provenance, freshness/channel/publication,
PATH/session/execution, and receipt binding. Today those dimensions are
asserted inconsistently across 13 prose surfaces (70 claim rows, 12 findings),
and no mechanism selects which install route a user should be told to take.
  Issue #11549 delivers (1) a conjunctive route classifier derived
deterministically
from the validated route schema/catalog owned by #10333, sequenced through
Issue #10334, with the canonical route denominator supplied by #11434 and evidence
producers such as #11432, and (2) a preferred-route selection whose *ordering*
is explicit, human-owned product data — never derived.

**Entry flow:** `$prepare-issue` → this plan → `$prepare-proof`/`$build-candidate`
in a later lane. Sibling state at authoring time:

**Human-ruling gate:** H1–H7 are explicit maintainer decisions, not defaults that
the implementer may adopt. Until each row is ruled in the authoritative issue, this
plan is a provisional design only: no preferred-route ordering, recommendation,
selection policy, or implementation may be treated as operative. A later lane may
prepare classification proof and fixtures only against the validated catalog; it
must stop at `provisional(human-pending)` for the selection surface.

- #11575 inventory **landed** on `origin/main`:
  `docs/distribution/INSTALL_CLAIM_SURFACES.md` — 13 surfaces (S01–S13), 70
  claim rows (C101–C1309), 12 findings (FND-1–FND-12). Its "Family handoff
  notes → For #11549" section is the direct requirement source.
- Dependency map: **#10333 owns the route schema/catalog** and is currently
  open and blocked by #11164; **#10334 owns sequencing/fan-in** and is not the
  schema authority; **#11434 owns the canonical route denominator**; and
  **#11432 plus the other named producers own evidence inputs**. #11549 starts
  only after those authorities publish a validated contract. The closed,
  superseded #12858 attempt is historical context only and cannot supply route
  rows, projection contexts, producer joins, publication/channel bindings, or
  fail-closed route states.

**Non-goals:** judging/rewriting prose wording strength (#10342), canonical
fragment generation (#10339), release receipts themselves (#7831 family),
registry producer-side dispositions (#9104), literal-pin linting (#10342 CI
cutover, which owns FND-10's allowlist), and any doc rewrite (FND-11 belongs to
`distribution-docs-sync`).

---

## 1. What the inventory says the classifier must honor

Derived from `docs/distribution/INSTALL_CLAIM_SURFACES.md`
(`git show origin/main:docs/distribution/INSTALL_CLAIM_SURFACES.md`):

- **13 surfaces, 70 claim rows.** Surfaces S01–S13 span root README, canonical
  install guide, CI guide, composite action + catalog, maintained example,
  upgrade/troubleshooting/editor guides, tutorial, extension manifest +
  marketplace copy, and 7 editor guides with live acquisition commands (S13).
  All rows are `curated` provenance; **zero generated fragments exist behind any
  row** (#10339 gap) — the catalog join is the only machine-readable backing
  until fragment generation lands.
- **8-value drift vocabulary** per row: `current`, `pending`, `stale_example`,
  `future_example`, `mutable_pin`, `cross_surface_drift`, `source_drift`,
  `volatile_number`. These are *independent results*, not a scalar quality —
  the classifier must keep them that way.
- **Three conjunctive dimensions named for #11549** (handoff notes, verbatim
  obligations):
  - **(a) Windows ARM64** — tracked source builds it (`release.yml:121-125`;
    C405, C501), user prose matches only published v0.17.0 assets (C210, C1204:
    "no native ARM64"). Treat **asset availability as receipt-bound per channel
    until an ARM64 Windows release ships**; neither prose direction may collapse
    into "supported" (FND-4, owned by `#11549-classifier`).
  - **(b) SHA256SUMS enforcement** — fail-closed canonical C207 vs fail-open
    residue C1005 describing the *same* `scripts/install.sh` (FND-7).
  - **(c) Product-unit membership** — `BUILD_FROM_SOURCE=1` installs perllsp
    only (C208) vs archives shipping the server+adapter pair (C209); FND-11
    shows tracked `install.ps1:221-229` already ships both executables, so
    tracked-installer behavior and published receipts diverge.
- **Subject-identity binding** — four-name collision field (C203, C1101):
  `perl-lsp` = foreign crates.io package (**anti-claim**: do-not-install),
  `perllsp` = server, `perl-dap` = adapter, `perl-lsp-rs` /
  `EffortlessMetrics.perl-lsp` = extension IDs.
- **Channel independence** (C201, C703, Distribution Matrix): GitHub Releases,
  VS Code Marketplace, Open VSX, crates.io, Homebrew, Docker, source builds are
  **separate receipts**; presence on one proves nothing about another.
- **Findings with route-relevant blast radius:** FND-1 (`@master` action pins ×4
  surfaces), FND-2 (stale example versions), FND-3 (`latest` defaults endorsed
  ×5 sites), FND-4 (ARM64 three-way split — **owned here**), FND-6 (unpinned
  `cargo install --git`), FND-7 (checksum-mode contradiction), FND-9
  (PowerShell-breakage annotation at 4 sites, split issue refs #4348/#5461),
  FND-11 (installer prose behind tracked source), FND-12 (unpinned
  no-checksum archive curl in VS_CODE_SETUP — first archive surface outside
  S02/S10/S01).

---

## 2. Conjunctive-classification design

### 2.1 Route subject model

Define a **route** as a named acquisition path a user can be told to take.
The exact route denominator, route IDs, projection contexts, and producer joins
must come from the validated catalog owned by #10333, sequenced/fanned in by
Issue #10334 and closed over the canonical denominator from #11434. The following are
planning families only; they are not an accepted catalog or a claim-to-route
join and must not be implemented as a hard-coded substitute:

| route_id | Backing claims | Notes |
| --- | --- | --- |
| VS Code Marketplace / Open VSX | Extension and managed-binary acquisition | Exact route rows and gallery context come from the catalog |
| Identity-bound archive / POSIX bootstrap | Release archive and bootstrap variants | Checksum and publication bindings come from the catalog |
| Homebrew / Cargo registry / unpinned Cargo git | Separate channel subjects | No cross-channel inference |
| Setup Action release/source modes | CI-oriented route families | Explicit ref and product-unit context |
| Windows zip / source-local builds | Platform and product-unit variants | Receipt-bound support remains separate |
| Unproven channels | Scoop, Chocolatey, winget, Docker, or other deferred channels | Only catalog-provided channels may enter here |

Out-of-scope rows (must be *explicitly* excluded, not silently unjoined):
C801 (diagnostic advice), C1001/C1002/C1008 (probes/posture), C1102
(virtual-workspace note), C1201/C101 (`volatile_number` badges), C1309
(`lsp-mcp` adjacent tool), C106/C216 (verification semantics — join as
post-install probes metadata, not route selection), C703 (channel
independence frame — a rule, not a route).

### 2.1.1 Inventory projection/exclusion oracle

The landed #11575 inventory is the complete audit input for this plan, not the
future route denominator. A validator must parse every literal claim ID in its
claim-row table and require an exact-one disposition in the future join data:

1. a claim is projected to one or more exact catalog route rows and projection
   contexts, or
2. it is listed in an explicit exclusion record containing the claim ID and a
   reason (`diagnostic_surface`, `verification_metadata`, `channel_rule`,
   `volatile_metadata`, `adjacent_product`, or `non_install_dependency`).

The validator rejects an unknown ID, duplicate disposition, missing ID, range
shorthand, or exclusion reason not in that closed vocabulary. Restatement rows
may project to the same route only when their exact IDs are independently listed;
they do not expand the denominator. The accepted join must explicitly dispose of
currently unlisted rows such as C107, C108, C1205, and C1208: generic-client and
Open VSX rows must be projected when route-relevant, while non-install dependency
or metadata rows may be excluded only with an allowed explicit reason. Open VSX
(C1208) must not disappear merely because it is a separate channel. The same exact-once
rule applies to FND-1 through FND-12: each finding must be joined to a route,
recorded as a route-independent constraint, or explicitly excluded with a reason.
This oracle is the acceptance condition for inventory traceability; the 70-row
prose count and the planning-family table are not substitutes for it.

### 2.2 Classification = conjunction of independent per-dimension verdicts

For each exact catalog route row and projection context, classification is the
**AND-join over all seven required hard dimensions**. Never reduce to a scalar
or infer a route from prose claim count. The seven dimensions below are the
authoritative aggregation; a missing or unjoined required dimension yields
`unproven`, and no dimension may be silently absorbed into another:

1. **Identity and topology** — product units yielded (`perllsp` / `perl-dap` /
   `extension`) + identity names bound, honoring the collision map
   (`perl-lsp` → rejected-foreign). Source: `product_units` dimensions (C208,
   C209, C210) + C203/C1101.
2. **Platform and target** — per-OS/arch coverage with the **three-way**
   `windows_arm64` record (`user_prose`, `tracked_source`,
   `published_receipt`) kept separate exactly as the validated route catalog
   must model it. Effective support = **receipt-bound**: `tracked_source=built` does NOT
   yield `supported` while `published_receipt=absent` (FND-4 disposition owned
   here).
3. **Product-unit and lifecycle completeness** — server/adapter membership,
   installation, first-use, repair, upgrade, rollback, and removal cells do
   not compensate for one another.
4. **Integrity and provenance** — `sha256sums_enforcement` mode and independent
   provenance/checksum identity. A contradiction inside
   the conjunction (C207 fail-closed vs C1005 fail-open on the same
   `scripts/install.sh`) resolves **pessimistically to `contradicted`**, never
   to the optimistic value, until `distribution-docs-sync` lands FND-7.
5. **Freshness, channel, and publication** — worst-of joined drift statuses,
   currentness, public publication, and public verification remain separate:
   any joined `mutable_pin` | `cross_surface_drift` | `source_drift` |
   `stale_example` | `future_example` caps the route below `proven_current`;
   `pending` yields `pending_gate(issue)`; `volatile_number` is inert (does not
   gate routes; FND-8 is copy metadata).
6. **PATH, session, and execution** — fresh-process resolution, exact host
   lookup, transport, cleanup, and process settlement are explicit dimensions.
7. **Receipt axis** — channel-independence: each route inherits exactly the
   receipt channel(s) its catalog row cites; no cross-channel inference (C201/C703).

The result retains a fixed-order vector of seven dimension verdicts, followed by
an overall summary. The vector is authoritative when failures overlap; an overall
summary must never erase a failing dimension:

```text
route_verdict {
  identity_topology,
  platform_target,
  product_unit_lifecycle,
  integrity_provenance,
  freshness_channel_publication,
  path_session_execution,
  receipt,
  overall,
}
```

Each dimension uses `proven_current`, `receipt_bound_partial`, `pending_gate`,
`contradicted`, or `unproven`. `not_recommended` is a selection annotation, not
a replacement for a dimension result. For deterministic summaries, compute
`overall` from the complete vector using this fixed severity order:
`contradicted` > `pending_gate` > `unproven` > `receipt_bound_partial` >
`proven_current`; ties retain all dimension names, claim IDs, findings, and
values in numeric dimension order. Thus a route can expose both
`integrity=contradicted{C207,C1005,FND-7}` and
`product_unit_lifecycle=unproven`; the summary is `contradicted` but the
unproven lifecycle failure remains machine-visible. No first/last claim or
single scalar may select the winning failure.

### 2.3 Where it lives (artifact + generator shape)

Follow the validated route-catalog generator pattern — deterministic, generated,
schema-closed:

- Extend the repository's eventual classification generator (the command,
  artifact filenames, schema path, and API are deliberately deferred to the
  validated contract) so it **consumes the exact validated route schema/catalog
  published by #10333** (input digest and producer revisions recorded) plus a
  small curated **route-join table** (`policy/install-route-join.toml` or a
  static map in the generator). The join representation must be selected only
  after #10333 publishes its accepted contract; this plan does not name or
  imply a v2 filename, module, schema, or generator API.
- Emit the classification artifact and closed schema required by that accepted
  contract, with a regenerate-and-compare byte-identity check wired beside the
  catalog's own validation gate.
- Classification (§2.2) is **pure derivation** — it must be mechanical and
  testable. The only curated inputs are: route→claim join, the anti-claim
  identity map, and the pessimistic-contradiction rule. **Preference ordering
  is NOT derived here** (§3).
- Sequencing note: #10334 fans in the producer evidence but does not replace
  #10333's schema authority; #11434 supplies denominator closure and #11432
  supplies its named evidence producer. Do not build against the closed #12858
  branch or any artifact/API from that attempt. Re-derive exact route joins, projection contexts, and falsifier
  fixtures if the validated input contract changes.

---

## 3. Preferred-route selection: algorithm options and tradeoffs

Selection input is an exact context tuple:
`(editor_family, editor_identity, os, os_version, arch, target_triple, libc,
platform_capabilities, desired_product_units, context, risk_posture)`, where
`editor_family` distinguishes VS Code-compatible managed clients from generic
LSP clients (Emacs, Neovim, Helix, Sublime, and other clients),
`target_triple`/`libc` distinguishes GNU from musl projections,
`platform_capabilities` includes Windows ARM emulation capability/version, and
`risk_posture` is either `strict` or `permissive`. `strict` may select only
`proven_current` routes with independent integrity/provenance and complete
lifecycle evidence; `permissive` may expose partial or pending routes as
explicitly annotated diagnostics but may not select a contradicted, unproven,
or capability-incompatible route. Unknown context fields refuse selection
rather than guessing. Once H1–H7 are explicitly
ruled, output may be an ordered route recommendation with per-route verdicts and
gate citations. Before then, any selection output is only a provisional diagnostic;
it must not recommend or silently order a route.

### Option A — Static precedence table (curated data only)

A `policy/install-route-preference.toml` maps context keys to **ordered route
lists**; selection validates each listed route's classification verdict before
emitting it (demote/annotate anything below `proven_current`).

- **Pros:** ordering is explicit auditable product authority; fully
  deterministic; trivially testable; no inference to explain; stale entries
  fail loudly against regenerated verdicts.
- **Cons:** combinatorial surface (platform × units × context); every new route
  needs manual placement; ordering drift is silent unless verdict-gating is
  strict.

### Option B — Scored/lexicographic derivation

Derive ordering from weighted dimensions (integrity mode > receipt freshness >
drift > unit completeness > platform match) with tie-breakers.

- **Pros:** less manual data; new routes slot in automatically.
- **Cons:** **weights are product authority smuggled in as constants** — the
  exact failure this plan must avoid; explanations become "the scoring said
  so"; contradictions need special-casing anyway; can silently contradict
  documented guidance (e.g., INSTALLATION.md C202's fastest-path enumeration is
  convenience-first, while the bootstrap identity contract is
  integrity-first — a scorer must pick one without authority to do so).
- **Verdict: reject** as the ordering mechanism; acceptable only *inside* a
  context bucket as a deterministic tie-break among equal-verdict routes, if
  ratified by an EXPLICIT-HUMAN row.

### Option C — Constraint filter + small curated order (recommended hybrid)

Mechanical **hard filter** eliminates routes whose verdict disqualifies them
for the request (platform unsupported per receipt; integrity mode below floor
(`fail_open_conditional`/`contradicted` fails; `verify_present_no_mode` is
context-tolerant); product-unit mismatch; identity-collision rejection).
Survivors are ordered by a **small explicit context policy table** seeded from
the de facto order already taught by S02's C202 enumeration (extension →
manual archive → other-editor download → local cargo), with CI context seeded
from S03 (pinned action first). Option A's table, but only where prose already
implies an order + a refusal rule that is pure derivation.

- **Pros:** the filter is provable and conjunctive (matches the issue title);
  curated data stays small (one order per context, not per platform×units);
  consistent with existing doc behavior — the classifier *formalizes* what
  INSTALLATION.md already teaches rather than inventing policy.
- **Cons:** still requires EXPLICIT-HUMAN rows (§5) for orderings prose does
  not settle; two moving parts (filter + table).

**Design recommendation: Option C, pending H1–H7.** Option B is rejected because
preference ordering is product authority and must remain reviewable data, not
emergent constants. This recommendation does not authorize implementation of a
preferred-route policy or emission of recommendations before all seven rulings.

---

## 4. Discriminating falsifiers

Each falsifier distinguishes a correct classifier/selector from a plausible
wrong one. All are cheap (pure functions over catalog + table; no network,
no installs). Claim IDs and route names below are examples carried forward
from the #11575 inventory; once #10333's schema is validated, #10334's fan-in
is complete, and #11434 closes the denominator, each fixture must be
rebound to the exact catalog row, route ID, and projection context rather than
assuming the former prose-row denominator.

1. **Receipt-binding (FND-4, routed from the landed inventory to this
   classifier).**
   Query `(windows, aarch64, {perllsp}, editor)`. Wrong implementations join
   `tracked_source=built` (C405/C501) or `user_prose=x64_fallback_build_from_source`
   (C1204) into "archive download supported". Correct output: **no
   receipt-backed native route** while `published_receipt_v0_17_0=absent`;
   either a refusal citing FND-4 or an explicit x64-fallback/build-from-source
   recommendation labeled receipt-bound. The fixture must include Windows version
   and emulation capability; Windows 10 ARM cannot use the x64 fallback, while
   Windows 11 ARM may do so when the capability is observed. A classifier that prints "supported"
   for Windows ARM64 fails.
2. **Conjunction-contradiction (FND-7).** Route `posix-bootstrap` joins C207
   (`fail_closed_required`) and C1005 (`fail_open_conditional`). Integrity
   verdict must be `contradicted{C207,C1005,FND-7}` — any implementation
   emitting `fail_closed_required` (took canonical) or `fail_open_conditional`
   (took first/last row) fails.
3. **Identity collision (C203/C1101).** A query or join keyed on the string
   `perl-lsp` must not resolve to the `cargo-registry perllsp` route; the
   anti-claim map must reject it with the collision note. An implementation
   that substring-matches crate names fails.
4. **Product-unit mode separation (C208 vs C209 vs FND-11).**
   `(source-build, {perllsp, perl-dap})` → source-build classifies **server-only**
   (C208 is receipt-bound for published behavior; FND-11 says the tracked
   installer ships the adapter but that is not a published receipt).
   `(manual-archive, {perllsp, perl-dap})` → pair OK. An implementation that
   merges `build_from_source_units` with `archive_units_claimed`, or that lets
   `tracked_installer_ships_adapter=true` upgrade the source-build route to
   pair-complete, fails.
5. **Stale-preference demotion.** Preference table lists a route whose joined
   rows include `mutable_pin` (e.g., `github-action` under FND-1/2/3, or
   `cargo-git` under FND-6): selection must demote/annotate it below any
   `proven_current` competitor — never reorder the table to hide the verdict.
   `cargo-git` must never outrank `cargo-registry`.
6. **Pending-gate honesty (FND-9).** `(windows, x86_64, {perllsp})`: the table
   may list `manual-archive` first. Under H2(a), `powershell-installer` must
   appear as `pending_gate(#5461/#4348)`/`not_recommended`; under H2(b), its
   intentional omission must be recorded as the ruled policy. An implementation
   that treats either ruling as the other fails.
7. **Determinism.** `cargo xtask install-route-classification check` is
   byte-identical across runs and machines: classification output contains no
   timestamps, ambient state, or catalog-order dependence beyond stable claim
   IDs. Any run-to-run diff fails.
8. **Denominator closure.** Every exact route row and projection context in the
   validated catalog is classified exactly once. A join referencing an unknown
   claim or route ID, or an unjoined row, fails the check. The 70 prose claim
   rows from #11575 are evidence inputs and do not define the classifier's
   route denominator by themselves.
9. **Cross-channel inference block (C201/C703).** A claim's receipt on channel
   X must never satisfy another route's receipt requirement (e.g., GitHub
   Releases v0.17.0 receipt must not make `homebrew-tap` `proven_current`).
   An implementation with a global "release exists" fact fails.
10. **Independent checksum/provenance binding.** A route with matching
    `SHA256SUMS` text but no independently bound artifact and release identity
    must remain `unproven`; a checksum string copied from a different channel
    must not satisfy the integrity/provenance axis.
11. **Candidate versus installed state.** A candidate artifact that has been
    built or uploaded but has no installed, verified product-unit observation
    must not satisfy installation or first-use cells. Conversely, an installed
    local build must not be emitted as a public publication receipt.
12. **PATH/session/execution isolation.** A route that resolves only through
    the current shell's PATH, an inherited session, or an ambient working
    directory must remain unproven for a fresh-process route. A fresh lookup,
    transport, cleanup, and settled process must each be present; one cannot
    stand in for the others.
13. **Lifecycle closure.** A route with install and first-use evidence but no
    repair, upgrade, rollback, or removal cell remains incomplete. A lifecycle
    cell from another product unit or channel must not close this route.
14. **Publication and verification separation.** A private/candidate upload or
    an unverified public listing must not become `proven_current`; publication,
    checksum/provenance verification, and currentness are separate predicates.
15. **No-route and ambiguity closure.** If every route fails a hard dimension,
    output must be an explicit no-route result with reasons. If two exact rows
    or contexts are ambiguous, selection must refuse rather than choose by
    input order, prose frequency, or a fallback command.
16. **Context and fallback isolation.** An editor route must not satisfy a CI,
    server-only, or manual context without an explicit catalog projection.
    Missing preferred policy or a failed hard filter must not silently fall back
    to `latest`, an unpinned command, or another context's route.
17. **Composite simultaneous failures.** A fixture with integrity contradiction
    and incomplete lifecycle must retain both dimension verdicts in the fixed
    vector and produce the same `overall=contradicted` summary regardless of
    catalog or claim input order. A scalar-only result, or a result that drops
    the lifecycle failure, fails.
18. **Selection-context and risk isolation.** Identical requests differing only
    in editor family, target/libc, observed Windows emulation capability, or
    `strict` versus `permissive` must either select the corresponding valid
    projection or refuse explicitly. A route that ignores any supplied field
    fails; `permissive` must not turn a contradiction into a selection.

---

## 5. EXPLICIT-HUMAN decision rows

Preference ordering is **product authority, not derivable** from source or
inventory. These rows cannot be resolved by the implementer; each needs a
maintainer ruling in issue #11549 (an issue comment suffices). The listed options
are proposals for that ruling, not defaults. If a row is unanswered, its state is
`human-pending`: no ordering, recommendation, or operative selection policy may
be emitted.

| # | Decision | Options | Unanswered state | Consequence of getting it wrong |
| --- | --- | --- | --- | --- |
| H1 | Global ordering principle: convenience-first (C202 teaches extension → archive → …) vs integrity-first (identity-bound verify > ease) | (a) convenience-first per C202; (b) integrity-first reorder | `human-pending`; no ordering | Classifier formalizes guidance that contradicts maintainer intent across every context |
| H2 | Windows x86_64 policy while published PowerShell installer is broken (#5461/#4348) | (a) manual-archive first, powershell shown as `pending_gate`; (b) omit powershell entirely; (c) lead with scoop/choco/winget verify-first | `human-pending`; no route recommendation | Users routed to a 404 installer, or gate visibility lost (FND-9's four-site spread re-grows) |
| H3 | Windows ARM64 until an ARM64 release ships (FND-4 — routed to #11549 by name) | (a) recommend x64-fallback + build-from-source labeled receipt-bound; (b) refuse with "no receipt-backed route"; (c) follow prose (contradicts receipts) | `human-pending`; no route recommendation | Either false "supported" claim or an unnecessarily barren answer; FND-4 disposition must be defensible |
| H4 | macOS server-only ranking: homebrew-tap vs cargo-registry (both `current`; tap freshness unproven per C1304/C1305 caveats) | (a) tap first (native UX); (b) cargo-registry first (version receipts); (c) context-split (editor=tap, headless=cargo) | `human-pending`; no ordering | A preferred route whose freshness caveat the docs themselves flag |
| H5 | Unpinned mutability policy: `cargo-git` (FND-6) and `latest` endorsements (FND-3) | (a) classify `not_recommended`, never selected; (b) selectable with warning; (c) leave unclassified | `human-pending`; no selection | Classifier endorses what FND-3 calls a moving target the receipt does not cover |
| H6 | perl-dap (adapter) acquisition policy for non-VS Code users | (a) archive pair route; (b) separate `cargo install --locked perl-dap` (C702); (c) defer — server-only answer, adapter on request | `human-pending`; no ordering | Users get a server without a debugger, or build-from-source surprises (C208) |
| H7 | Unproven channels (`unproven-channels`: scoop/choco/winget C212, Docker) in selection output | (a) never selected, visible in a "unproven" appendix; (b) fully omitted; (c) selectable with verify-first instruction (as C212 prose does) | `human-pending`; no selection | Either dead output weight or an implied endorsement the receipts don't back |

*(Count: 7 EXPLICIT-HUMAN rows.)*

---

## 6. Implementation sizing, title, and next actions

**Suggested conventional title:**
`feat(distribution): add conjunctive install-route classification and preferred-route selection (#11549)`

**Sizing: M after the human-ruling gate** — one candidate writer, one coherent
claim. Roughly: route-join
table + verdict derivation (~250–400 lines incl. static tables, following the
existing catalog-generator idiom), artifact + closed schema +
regen-check wiring (~150–250), selection filter + context policy table
(~100–200), and falsifier tests (§4) (~300). No production-crate changes; no
doc rewrites. **Optional split if review prefers:** slice 1 = classification
artifact (S/M), slice 2 = selection + policy table (S), sharing §2.1's join
table. Do NOT pull FND-7/FND-11 doc syncs or #10342 linting into this claim.

**Hard prerequisite check at build time:** require explicit rulings for H1–H7 in
addition to the validated route
schema/catalog from #10333, with #10334's fan-in complete, #11434's canonical
denominator, and the relevant #11432/evidence-producer revisions. Require exact
route IDs, projection contexts, producer joins, publication/channel bindings,
and fail-closed route states. If any authority is absent or structurally
changes, stop at `NOT_PROVEN` and re-derive §2 and its fixtures; do not
substitute the closed #12858 attempt.

**Proof strategy:** all §4 falsifiers as focused `cargo test -p xtask` cases
plus the byte-identity regen check; `just doctor`, `cargo fmt -p xtask --
--check`, `cargo clippy -p xtask --all-targets --locked -- -D warnings`,
`cargo test -p xtask --all-targets --locked`. No CI-cycle dependency beyond the
standard gates.

**Residual risks:**

- The #10333 route schema/catalog may change before implementation, or #10334
  fan-in/#11434 denominator closure may be incomplete → re-derive §2.1,
  hard-dimension bindings, and affected fixtures; the human preference rulings
  remain separate.
- H1–H7 contain proposed options only, not defaults or maintainer rulings. Until
  each is explicitly ruled in the authoritative issue, all seven rows are
  **non-operative**: the plan and any resulting artifact remain
  `provisional(human-pending)` and **non-preferred**. The implementer must not
  turn a proposed option into a recommendation, and the authority gap must
  remain machine-visible rather than silently choosing it.
- Prose drift (new surfaces) after inventory audit commit `20174d50c` →
  denominator closure falsifier catches unmodeled rows only after the inventory
  itself is re-audited (#11575 cadence), so the check is bounded by that
  document's freshness, by design.
