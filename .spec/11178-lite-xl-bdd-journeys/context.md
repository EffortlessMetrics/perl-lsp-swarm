# Context: #11178 — bounded Lite XL client journeys and evidence boundaries in the spec ledger

## Problem

The Lite XL support programme (#8950 product controller, #9008 host-evidence
controller, #10651 integration/proposition denominator) already landed its
implementation substrate: a staged Lua client under `clients/lite-xl/` with
deterministic seam suites and the #11103 journey harness, candidate-profile
composition (#11170), capability/affordance conformance (#11172), and dozens of
protocol/config/security/currentness fix lineages. The vim sibling encoded its
canonical journeys first (`.spec/11371-vim-bdd-journeys/`, merged PR lineage),
and the emacs/neovim programmes carry their own checked contracts. Lite XL
still has no earlier checked expression of product semantics. Without one,
every downstream agent must infer from fixture bytes or Lua suites whether a
behavior is baseline, quality-sidecar, or stronger-profile — and the planned
fixture manifest leaf (#11181) is explicitly **blocked until this issue
supplies stable scenario IDs**.

This bundle encodes one checked normative baseline for the generic-LSP Lite XL
client journeys so downstream leaves bind stable scenario IDs instead of
re-deriving meaning from #8950's full campaign history. It owns behavior
wording, scenario identity, claim-profile membership, and evidence boundaries
only.

## Why this approach (ledger-format evolution record)

Issue #11178 names "repository Gherkin + acceptance/spec ledger + generated
`docs/feature_status.md`" and commands `cargo xtask bdd` / `ac-status` /
`docs-check`. Current main has no Gherkin `.feature` runner surface, no
generated feature-status document, and none of those xtask subcommands
(verified against `origin/main` at claim time). The repository's existing,
shipped BDD/spec-ledger authority is the `.spec/` packet system governed by
[`docs/reference/SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md)
and #3983 conventions; the nearest structural twin is the merged Vim journeys
packet `.spec/11371-vim-bdd-journeys/`, exercised in the same discipline as the
emacs bundles (`.spec/11716-emacs-support-architecture/`,
`.spec/11717-emacs-train-specs/`) and the zed bundle
(`.spec/11709-zed-integration-architecture/`). Per the issue's own evolution
clause ("If command names have evolved, use current canonical equivalents and
record them in the PR review map"), this packet projects the Feature/Scenario
organization into that existing spec-ledger system: features map to journey
families, scenarios to stable-ID ledger rows, and step-level executable truth
stays downstream. Introducing a parallel `.feature` format or a feature-status
generator here is out of scope.

The three objects named by the issue stay distinct:

```text
BDD/spec ledger (this packet)
  what a user-visible Lite XL journey means
  which outcomes belong to which claim profile

fixture/expectation manifest (#11181)
  exact source/config/wire bytes, anchors, expected facts,
  wrong-subject controls

actual-host receipt (#9008 controller over #10673..#10693 lanes)
  what exact Lite XL/client/language/server/platform/profile
  subjects actually did
```

A Lua simulation test is not the product specification. A Gherkin-style ledger
sentence is not the semantic oracle. An exact-source composed tree is not
actual-host or public support. They bind through the stable scenario IDs
defined below.

## Subject substrate (consumed by reference, never re-pinned)

Unlike the Vim programme at its journeys milestone, no single pinned
`.ci/editor-clients/lite-xl-*` subject manifest exists yet; subject pinning for
Lite XL belongs to #10651 (integration authority) and #8960 (exact client
profile/fixture), with fixture cells owned by #11181. This packet therefore
anchors every behavioral statement to a named merged authority on current main
or a named issue authority, never to invented detail:

```text
clients/lite-xl/upstream/ + leaves/base/*        staged exact client source
clients/lite-xl/tests/harness.lua + journey_session_test.lua
                                                 #11103 deterministic journey harness (PR lineage incl. #12142)
clients/lite-xl/candidate_manifest.lua + compose.lua
                                                 #11170 composed exact-source profiles
capability_manifest_test.lua etc.                #11172 capability/affordance conformance
protocol/config/currentness/edit suites          merged fix lineages #10657/#10785/#10833/#10845/
                                                 #11124/#11128/#11143/#11147/#11151/#11155/#11186
security lineages                                #10653 project-config execution boundary; #4998 class
position/wire authorities                        #2298 / #7713 envelopes consumed by reference
```

Amendment issues recorded on #11178 and honored here: #11186 (codec
complexity budget), #11188 (completion resolve-before-apply), #11189
(duplicate completion labels, evidence-driven profile membership), #11194
(Unicode validity), #11197 (terminal JSON syntax errors), and #11198
(duplicate document-symbol identities).

This bundle pins no upstream commit digest; when #10651/#8960 land subject
pins, they bind these IDs without editing meaning here.

## Stable scenario ID namespace

Stable downstream-consumable IDs use the form:

```text
lite_xl.bdd.<family>.<nn>
```

Families, in fixed order:

```text
lite_xl.bdd.activate   Lite XL activates the intended provider safely
lite_xl.bdd.protocol   the client preserves protocol and session truth
lite_xl.bdd.read       only current diagnostics and read results are exposed
lite_xl.bdd.edit       edits apply as one validated transaction
lite_xl.bdd.lifecycle  document, root, watcher, and lifecycle identity hold
lite_xl.bdd.wire       path, position, and wire identity hold
lite_xl.bdd.support    support claims remain subject- and stage-bound
lite_xl.bdd.opt        optional and stronger-profile inputs (never core blockers)
```

IDs are immutable once published; retirement requires a new revision through
this owning issue, never silent reuse.

## Journey inventory (baseline = 73 scenarios, optional = 8)

Full normative wording, profile membership, and per-scenario evidence owners
are the §Behavior ledger in `acceptance.md`. Summary:

### Feature: Lite XL activates the intended provider safely

```text
lite_xl.bdd.activate.01–09  extension set activation independence; distinct
                            perllsp vs PerlNavigator identities with exactly
                            one winner or visible conflict (#10708); ambient-
                            binary rejection (#8997); inert project
                            .lite_lsp.lua sentinel (#10653); safe project data
                            vs user-owned config distinction; false/null/empty
                            cardinality and item-order exactness (#10845/
                            #11136/#11147); atomic array replacement (#11143);
                            nearest-root subject explicitness (#10715); separate
                            POD/template/XS/shebang/untitled dispositions
```

### Feature: Lite XL preserves protocol and session truth

```text
lite_xl.bdd.protocol.01–13  JSON shape fidelity; single-send request IDs with
                            terminal timeouts (#10657); exactly one response per
                            server request (#10785); explicit rate pressure;
                            bounded framing with typed malformed/oversized
                            failure (#11151); nesting-budget structural failure
                            (#11186); terminal JSON syntax errors (#11197);
                            typed Unicode failures retaining valid BMP/non-BMP
                            (#11194); monotonic document versions; sleep-free
                            initialize→initialized→didOpen; canonical large
                            messages with exact Content-Length; bounded redacted
                            logs (#11155); shutdown precedes reaped exit
```

### Feature: Lite XL exposes only current diagnostics and read results

```text
lite_xl.bdd.read.01–11  target diagnostic publication/replacement/clearing
                        laws (#11124/#11128); stale-generation rejection;
                        non-BMP column agreement; identity-bearing read family
                        (hover/signature/definition/implementation/references/
                        symbols); post-edit query freshness (#11108);
                        observational previews; duplicate symbol display paths
                        stay selectable or carry explicit host breadth
                        (#11198); wrong root/provider decoy rejection
```

### Feature: Lite XL applies edits as one validated transaction

```text
lite_xl.bdd.edit.01–12  completion insertion; resolve-before-apply lifecycle
                        without prior hover (#11188); duplicate-label items
                        remain separately selectable or explicit not_proven
                        breadth (#11189); resolve/import edit bytes; unsorted
                        edit normalization against one pre-edit snapshot;
                        fail-closed overlap rejection; idempotent formatting;
                        caret intent survival; multi-file rename completion;
                        dirty-open protection; post-edit currency; visible
                        non-partial unsupported-form failures (#10671/#10754/
                        #8986/#10681)
```

### Feature: Lite XL preserves document, root, watcher, and lifecycle identity

```text
lite_xl.bdd.lifecycle.01–11  untitled-first-save attach; Save As URI identity
                             once-per-side (#9001 lineage); supported→unsupported
                             traffic stop; cross-root recomputation; selected
                             root beating umbrella/sibling distractors;
                             closed-file external create/change/delete
                             freshness (#10691); generation-owned watchers;
                             restart-then-replace ordering (#10693); replay-once;
                             stale-generation publish rejection; orphan-free
                             normal and forced-failure cleanup (#8997/#10715)
```

### Feature: Lite XL preserves path, position, and wire identity

```text
lite_xl.bdd.wire.01–08  file URI round trips; explicit non-file/malformed URI
                        failure; safe argv/native showDocument handoff; UTF-16/
                        non-BMP exact landing; explicit CRLF disposition; actual
                        didChange envelope matching #7713; >50 KiB decode
                        exactness; Content-Length/partial-frame semantics
                        (#11162/#11165/#10873/#10684/#10665/#2298)
```

### Feature: Lite XL support claims remain subject- and stage-bound

```text
lite_xl.bdd.support.01–09  stage-kind distinctness; load-bearing composed-tree
                           digests (#11170); platform/version non-substitution
                           (#10733); manual-binary vs managed-lpm route
                           separation (#9010); monotone-distinct
                           candidate→released→public progression where
                           accepted-unreleased cannot satisfy public support
                           (#10739/#9012); advertisement ≠ consumption (#11172);
                           advanced/DAP rail separation (#10767); sidecar never
                           silently blocks/promotes core; narrow invalidation of
                           stale subjects/digests (#9016/#7122 consumers)
```

### Optional and stronger-profile inputs (never baseline blockers)

```text
lite_xl.bdd.opt.01–08  death-recovery envelope, responsiveness envelope,
                       expanded activation dispositions, full platform matrix
                       rows, admitted advanced multi-root/LSP slices, upstream
                       submission packets, managed lpm package route, released/
                       public artifact replay
```

Later leaves may extend this BDD authority through separate stable families via
revision here; they do not cram unimplemented breadth into baseline examples.

## Claim profiles

The five initial semantic profile IDs are exactly those named by the controlling
issue while #8950 remains the human-readable campaign authority; #11176 later
migrates profiles/edges onto the #10338/#10858 generic typed-train model
without changing their product meaning:

```text
lite_xl_protocol_baseline   registration, safe activation set,
                            initialize/configuration compatibility, clean
                            protocol process baseline; synthetic/exact-process
                            evidence ceiling only; no actual Lite XL support
lite_xl_exact_source_core   the bounded useful real-editor journey: safe open →
                            exact selected provider → push diagnostics →
                            completion/read family → formatting/rename
                            application → UTF-16/CRLF/large-document integrity
                            → first-save/Save As identity → restart + clean close
lite_xl_workspace_fresh     core plus nearest-selected-root/sibling isolation,
                            dynamic watcher registration, closed-file external
                            currentness, watcher/root cleanup; selected single-
                            root, never multi-root
lite_xl_first_class_public  independent additional stages: reviewed composed
                            candidate (#11170), upstream packets/manual state
                            (#10739), released subjects, managed/public install
                            routes (#9010), clean public replay (#9012), support
                            registry/docs projection (#7122/#9016)
lite_xl_quality_breadth     additive claims: death recovery, responsiveness
                            envelope, expanded activation dispositions,
                            version/platform matrix, advanced LSP slices
```

Profile laws:

1. A stronger profile never erases a narrower valid one.
2. No profile inherits another stage by naming convention; membership is
   explicit per row.
3. Optional/quality rows exist as additive inputs only; their absence cannot
   block or silently extend the bounded core, and a zero-budget defect that
   breaks the core claimed envelope blocks core directly — it does not promote
   the sidecar.
4. A source implementation, merged PR, simulation pass, composed exact-source
   tree, another client/build/platform, or a DAP receipt can never satisfy an
   actual-host row; each evidence kind satisfies only its own ceiling.
5. No scenario contributes to a support claim until its executable evidence
   chain passes at that chain's own stage.

## Evidence boundaries and chain

Every baseline scenario binds one downstream evidence chain; each arrow is a
different owner, and no owner may widen the proposition it receives:

```text
lite_xl.bdd.<id>
→ #11181 fixture/expectation cell (schema lite_xl_fixture_expectations.v1,
  owning-scenario binding required there)
→ #11103 deterministic client simulation suite where applicable
  (clients/lite-xl/tests/harness.lua + journey_session_test.lua)
→ #8960 exact-process/protocol evidence cell where applicable
→ actual-Lite-XL host cell(s) owned by #10676/#10679/#10681/#10684/#10691/
  #10693 under the #9008 controller and #10673 adapter
→ #7122 support-registry cell via #9016 projection
```

Machine-visible distinction tags reuse the repository's existing vocabulary —
the support registry tiers (`policy/lsp-client-support.toml`), the issue's own
evidence-semantics list, and #10858 edge classes — rather than inventing a
second ontology:

| Distinction | Existing vocabulary consumed |
| --- | --- |
| configuration documented | `configuration_documented` tier family |
| protocol compatibility proven | `protocol_profile_proven` |
| deterministic simulation proven | `client_simulation_proven` |
| composed exact-source tree | `composed_exact_source` (#11170 producers) |
| exact-source actual host | `exact_source_actual_host` (#9008 lanes) |
| managed package | `managed_exact_source` (#9010) |
| accepted but unreleased | `accepted_unreleased` (#10739 state machine) |
| released upstream | `released_upstream` |
| public artifact replayed | `public_artifact_actual_host` (#9012) |
| advanced/unadmitted breadth | `unsupported` / `not_proven` dispositions |
| instrument or cleanup failure | `instrument_failed` / `cleanup_failed` terminal states |

## Security boundary

Positive scenarios admit only values, paths, and configuration shapes governed
by #10653 and the existing server security authorities. Host fixtures must not
normalize, as ordinary successful behavior:

- project-controlled executable configuration (the `.lite_lsp.lua` sentinel
  must stay inert — activation of trusted setup never executes it);
- unsafe external launch strings (showDocument uses safe argv/native handoff
  only);
- traversal or out-of-root watcher/configuration paths;
- wrong-provider or PATH-ambient selection;
- raw sensitive protocol logs entering canonical evidence.

Absolute/traversal-class inputs remain separately governed/rejected
propositions and can never become positive behavior because a generic client
could transmit them.

## Authority and ownership

Consumed, never cloned: #8950/#9008/#10651 (controllers/denominator), #11170
via its merged composition producer, #11172 via its merged conformance
producer, #11103 via its merged harness/journey suites, #8960 (exact profile +
fixture), #10653 (project-config security), #10845/#11136/#11143/#11147
(config shape), #10657/#10785/#10833/#11151/#11155/#11186/#11194/#11197
(protocol truth), #11124/#11128/#11108 (currentness), #10671/#10754/#8986
(edit transaction), #8997/#9001/#10715 (activation/save identity); wire/
position: #10684, #11162, #11165, #10873, #2298, #7713; distribution and
public rails: #10733, #9010, #10739, #9012, and #10767; support registry:
#7122 and #9016; typed train/profile substrate: #10338, #10858, and
#11176; spec method: #3983 +
[`SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md) (spec method).

Owned downstream, named here as boundaries only:

```text
#11181  fixture/expectation manifest cells binding every Chain P/H scenario
        ID (currently blocked exactly on these IDs); Chain L support/opt rows
        bind through #7122/#9016 and distribution-rail consumers instead
#11103  deterministic client simulation bindings
#8960   exact-process/protocol evidence cells
#10673  real-host adapter over shared fail-closed supervision
#10676/#10679/#10681/#10684/#10691/#10693  actual-host proof lanes
#7122 / #9016  support-registry rows and receipt-gated docs
#10733/#9010/#10739/#9012/#10767  platform/package/submission/public rails
```

This bundle creates no fixtures, provisions no Lite XL, launches no client or
server, changes no Lua/Rust/shell behavior, composes no patches, produces no
receipts, awards no support, submits nothing upstream, and publishes no
package.

## Stable versus mutable information

Durable bytes here carry stable identities only: scenario IDs, profile names,
authority references, evidence-stage vocabulary, and named repository paths.
Current main SHA, open PR numbers, check colours, writers/models, live upstream
state, and wall-clock readiness never enter these files.

## Alternatives rejected

- **Create a new `.feature`-file subsystem plus a feature-status generator:**
  rejected; no such ledger exists on current main, and inventing a parallel
  format beside the shipped `.spec/` ledger authority is exactly the second-
  authority drift the campaign forbids. The evolution is recorded instead.
- **Wait for #11176's typed-train migration to define the graph first:**
  rejected; #11181 is blocked on stable scenario IDs now, and #11176 itself
  declares it must not invent a Lite-XL-only framework to move earlier. The
  initial profile IDs migrate later without changing product meaning.
- **Fold scenario definitions into #11181's manifest:** rejected; the fixture
  would become the first expression of product semantics, the precise
  inversion this issue exists to prevent.
- **Encode scenario IDs only inside receipts/suites:** rejected; receipts and
  Lua suites are downstream evidence; the proposition must pre-exist as a
  checked identity they can bind to.
- **Make quality/platform/public rows part of the baseline blocker set:**
  rejected; unimplemented breadth must not block the bounded core, and a
  stronger profile must never erase a narrower valid one.

## Prior art / duplicates

- `.spec/11371-vim-bdd-journeys/` (merged PR lineage of #11371) — nearest
  structural twin; same checked-discipline shape (stable-ID ledger, profile
  laws, evidence chains, falsifier grid, deterministic structural proof).
  Referenced, not duplicated; that packet owns Vim/vim-lsp journeys, not Lite
  XL.
- `.spec/11716-emacs-support-architecture/` and
  `.spec/11717-emacs-train-specs/` — emacs subject-tier/train authority; owns
  emacs cohorts.
- `.spec/11709-zed-integration-architecture/` — zed decision-map bundle; same
  SPEC_TEMPLATE projection pattern.
- Neovim action/observation contract (merged `xtask/src/native_neovim_actions/`
  lineage) — neighboring programme expressed in executable xtask contracts;
  different mechanism for a different maturity stage.
- `policy/lsp-client-support.toml` — registered support tiers; unchanged here.

No prior `.spec` packet encodes Lite XL user-journey scenarios; nothing here
duplicates an existing authority.

## Links

- Issue: [#11178](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11178)
- Controllers: #8950 / #9008 / #10651; denominator migrations #11176
- Fixture consumer (blocked on these IDs): #11181
- Simulation/exact-process consumers: #11103, #8960
- Actual-host lanes: #10673, #10676, #10679, #10681, #10684, #10691, #10693
- Support projection: #7122 / #9016; distribution rails #10733/#9010/#10739/#9012/#10767
- Amendments honored: #11186 / #11188 / #11189 / #11194 / #11197 / #11198
- Spec method: #3983 and `docs/reference/SPEC_TEMPLATE.md`

## Scope boundary

In scope: exactly this directory's `context.md`, `acceptance.md`, and
`checklist.md`. The machine-readable inventory projection
(`docs/policy/NON_RUST_INVENTORY.md`) is deliberately not part of this
bundle; it lists this directory through the sanctioned
`cargo xtask non-rust inventory --write` in a later regeneration, and the
`.spec/**` allowlist glob keeps the added files classified meanwhile.

Out of scope: `docs/policy/NON_RUST_INVENTORY.md` (generated elsewhere),
fixture sources/oracles (#11181), provisioning or launching Lite
XL, Lua/Rust/shell implementation changes, candidate composition, host
automation or receipts, support-registry mutation, docs prose beyond generated
projections, CI workflow edits, external upstream submission, any new BDD
runner infrastructure, and support promotion.
