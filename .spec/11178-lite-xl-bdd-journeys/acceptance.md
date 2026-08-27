# Acceptance Criteria: #11178 — bounded Lite XL client journeys and evidence boundaries

This is a checked, declarative BDD/spec-ledger contract. It implements no
fixture, Lua/Rust/shell behavior change, client composition, host run, receipt,
support tier, or generated-status machinery. Executable truth for every row
below is owned by the downstream leaves named in its evidence boundary; all
behavior rows remain `not_proven` until their own chains pass.

## §Behavior — baseline journey ledger

Normative wording is from the user's/editor's observable perspective. Exact Lua
identifiers, command names, JSON fields, hashes, and paths belong to #11181 and
the downstream leaves. Named evidence chains (each arrow a different owner, no
owner widening its input):

```text
Chain P  #11181 fixture cell → #11103 deterministic suite → #8960
         exact-process/protocol cell → #7122/#9016 support projection
Chain H  #11181 fixture cell → #11103 deterministic suite where applicable →
         #10673 real-host adapter → owning session lane
         (#10676/#10679/#10681/#10684/#10691/#10693) → #7122/#9016 projection
Chain L  this ledger → checked conformance/projection producers (#11172,
         #11170) → registry/docs consumers (#7122/#9016)
```

### Feature: Lite XL activates the intended provider safely

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary |
| --- | --- | --- | --- |
| `lite_xl.bdd.activate.01` | Each supported Perl extension (.pl .PL .pm .t .psgi .cgi .fcgi) activates independently and each unsupported class (POD/template/XS/shebang-only/untitled handled at .09) stays excluded on exact-provider attach | exact_source_core; actual host required | Chain H (#10676) |
| `lite_xl.bdd.activate.02` | perllsp and PerlNavigator remain distinct identities with one explicit winner or a visible conflict; table/preference order never silently selects | exact_source_core; actual host required | Chain H (#10676) |
| `lite_xl.bdd.activate.03` | A wrong or ambient PATH binary can never satisfy the selected provider row | exact_source_core; actual host required | Chain H (#10676) |
| `lite_xl.bdd.activate.04` | Project-controlled `.lite_lsp.lua` never executes implicitly; a sentinel proves inertness whenever trusted setup runs | exact_source_core; configuration_documented substrate; security-sensitive | Chain H (#10676); #10653 authority |
| `lite_xl.bdd.activate.05` | Safe project data and user-owned configuration remain distinct subjects; neither channel can forge the other's effect | exact_source_core; configuration_documented substrate; security-sensitive | Chain H (#10676) |
| `lite_xl.bdd.activate.06` | Explicit false, null, empty array/object values and duplicate configuration-item order remain byte-exact through responses and effects | exact_source_core; configuration_documented substrate | Chain P plus Chain H (#10676) |
| `lite_xl.bdd.activate.07` | Array-valued settings replace wholly instead of retaining inherited tails | exact_source_core; configuration_documented substrate | Chain P plus Chain H (#10676) |
| `lite_xl.bdd.activate.08` | Every session makes the nearest intended root/config subject explicit; umbrella or sibling subjects cannot satisfy it | exact_source_core; actual host required | Chain H (#10676/#10691) |
| `lite_xl.bdd.activate.09` | POD/template/XS/shebang-only/untitled inputs hold separate, explicit dispositions and never masquerade as activated Perl documents | exact_source_core; actual host required | Chain H (#10676) |

### Feature: Lite XL preserves protocol and session truth

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary |
| --- | --- | --- | --- |
| `lite_xl.bdd.protocol.01` | JSON null/array/object shape fidelity survives every codec round trip | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.02` | One client request ID is sent once; timeouts are terminal, never resurrected | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.03` | Every server-originated request receives exactly one client response | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.04` | Rate pressure delays/coalesces/rejects explicitly and never silently drops load-bearing state | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.05` | Bounded valid inbound framing decodes; malformed/oversized frames fail typed without semantic output | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.06` | Byte-limit-legal JSON above the nesting/node budget fails typed with no callback, partial value, or raw leak | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.07` | Malformed JSON produces one typed syntax failure with no incidental Lua exception, partial value, or callback | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.08` | Lone/malformed surrogates or invalid raw UTF-8 fail typed while valid BMP/non-BMP content is retained exactly | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.09` | Exactly one monotonic document version flows per server/document session | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.10` | initialize→initialized→didOpen completes without fixed sleeps or busy loops | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.11` | Large/full messages use canonical JSON with exact Content-Length | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.12` | Protocol failures/logs stay bounded and redacted in any recorded surface | protocol_baseline; client_simulation_proven ceiling | Chain P |
| `lite_xl.bdd.protocol.13` | Shutdown responds before exit and the process is reaped | protocol_baseline; client_simulation_proven ceiling | Chain P |

### Feature: Lite XL exposes only current diagnostics and read results

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary |
| --- | --- | --- | --- |
| `lite_xl.bdd.read.01` | The exact target diagnostic becomes visible after open | exact_source_core; actual host required | Chain H (#10676) |
| `lite_xl.bdd.read.02` | Newer malformed source state replaces exactly the prior target diagnostic | exact_source_core; actual host required | Chain H (#10676) |
| `lite_xl.bdd.read.03` | Repairs clear diagnostics only through current provider/version publication | exact_source_core; actual host required | Chain H (#10676) |
| `lite_xl.bdd.read.04` | Stale nonempty or stale empty publications can neither replace nor clear newer state | exact_source_core; actual host required | Chain H (#10676) |
| `lite_xl.bdd.read.05` | An old provider/server generation cannot publish anything | exact_source_core; actual host required | Chain H (#10693) |
| `lite_xl.bdd.read.06` | Inline and list diagnostic columns agree across non-BMP text | exact_source_core; actual host required | Chain H (#10684) |
| `lite_xl.bdd.read.07` | Hover/signature/definition/implementation/references/symbols identify expected facts of this project | exact_source_core; actual host required | Chain H (#10679) |
| `lite_xl.bdd.read.08` | Repeating a caret/query after an intervening edit cannot revive the pre-edit result | exact_source_core; actual host required | Chain H (#10679) |
| `lite_xl.bdd.read.09` | Navigation targets remain authoritative while previews stay observational and mutation-free | exact_source_core; actual host required | Chain H (#10679) |
| `lite_xl.bdd.read.10` | Multiple document symbols sharing one display path remain separately selectable with exact ranges, or the host breadth carries an explicit not_proven disposition | exact_source_core; actual host required | Chain H (#10679) |
| `lite_xl.bdd.read.11` | Wrong root/provider/decoy or nonempty-but-wrong read results cannot satisfy any read row | exact_source_core; actual host required | Chain H (#10679) |

### Feature: Lite XL applies edits as one validated transaction

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary |
| --- | --- | --- | --- |
| `lite_xl.bdd.edit.01` | Accepted completion inserts exactly the intended item | exact_source_core; actual host required | Chain H (#10681) |
| `lite_xl.bdd.edit.02` | Selecting completion without prior hover performs one current resolve before application, never a fabricated or duplicated resolve | exact_source_core; actual host required | Chain H (#10681) |
| `lite_xl.bdd.edit.03` | Distinct completion items sharing one display label remain separately selectable end-to-end, or the breadth carries explicit not_proven rather than invented provider output | exact_source_core; actual host required | Chain H (#10681) |
| `lite_xl.bdd.edit.04` | Resolve/additional-text-edit imports produce the exact intended bytes | exact_source_core; actual host required | Chain H (#10681) |
| `lite_xl.bdd.edit.05` | Unsorted same-document edits normalize against one pre-edit snapshot | exact_source_core; actual host required | Chain H (#10681) |
| `lite_xl.bdd.edit.06` | Invalid/overlapping edits fail before any buffer mutation | exact_source_core; actual host required | Chain H (#10681) |
| `lite_xl.bdd.edit.07` | Formatting produces exact bytes and is idempotent under re-application | exact_source_core; actual host required | Chain H (#10681) |
| `lite_xl.bdd.edit.08` | Caret/selection intent survives non-completion edits exactly | exact_source_core; actual host required | Chain H (#10681) |
| `lite_xl.bdd.edit.09` | Multi-file rename applies every admitted target across files | exact_source_core; actual host required | Chain H (#10681) |
| `lite_xl.bdd.edit.10` | Open dirty targets are never overwritten from disk state | exact_source_core; actual host required | Chain H (#10681) |
| `lite_xl.bdd.edit.11` | Post-edit re-query returns current facts and no older generation satisfies the query | exact_source_core; actual host required | Chain H (#10681) |
| `lite_xl.bdd.edit.12` | Unsupported WorkspaceEdit/resource forms fail visibly and non-partially | exact_source_core; actual host required | Chain H (#10681) |

### Feature: Lite XL preserves document, root, watcher, and lifecycle identity

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary |
| --- | --- | --- | --- |
| `lite_xl.bdd.lifecycle.01` | Untitled/unsupported buffers defer cleanly and the first supported save starts the exact server immediately | exact_source_core; actual host required | Chain H (#10676) |
| `lite_xl.bdd.lifecycle.02` | Save As closes the old URI exactly once and opens the new URI exactly once with no traffic gap or duplication | exact_source_core; actual host required | Chain H (#10676) |
| `lite_xl.bdd.lifecycle.03` | Supported-to-unsupported transitions stop Perl document traffic explicitly | exact_source_core; actual host required | Chain H (#10676) |
| `lite_xl.bdd.lifecycle.04` | Cross-root Save As recomputes provider/root/config to the newly selected subjects | workspace_fresh; actual host required | Chain H (#10691) |
| `lite_xl.bdd.lifecycle.05` | The selected intended root beats umbrella and sibling distractors in every answer | workspace_fresh; actual host required | Chain H (#10691) |
| `lite_xl.bdd.lifecycle.06` | Closed-file external create/change/delete updates exact workspace facts | workspace_fresh; actual host required | Chain H (#10691) |
| `lite_xl.bdd.lifecycle.07` | Watch registrations and callbacks are owned by their root/server generation | workspace_fresh; actual host required | Chain H (#10691) |
| `lite_xl.bdd.lifecycle.08` | Explicit restart terminates the old process before the replacement serves | exact_source_core; actual host required | Chain H (#10693) |
| `lite_xl.bdd.lifecycle.09` | Open documents replay exactly once onto a replacement session | exact_source_core; actual host required | Chain H (#10693) |
| `lite_xl.bdd.lifecycle.10` | Old-generation requests/diagnostics/timers/watchers cannot publish after replacement | exact_source_core; actual host required | Chain H (#10693) |
| `lite_xl.bdd.lifecycle.11` | Normal and forced-failure cleanup leave no orphan process/watch; cleanup failure is terminal `cleanup_failed`, never silent success | workspace_fresh; actual host required | Chain H (#10693) |

### Feature: Lite XL preserves path, position, and wire identity

| Scenario ID | User-visible behavior | Profile / evidence tag | Evidence boundary |
| --- | --- | --- | --- |
| `lite_xl.bdd.wire.01` | Local file URI/path round trips hold for every admitted platform shape | exact_source_core; actual host required | Chain H (#10684) |
| `lite_xl.bdd.wire.02` | Non-file/malformed/control-bearing URIs fail explicitly | exact_source_core; actual host required | Chain H (#10684) |
| `lite_xl.bdd.wire.03` | External showDocument uses safe argv/native handoff and reports a truthful outcome | exact_source_core; actual host required | Chain H (#10684); #10653-class security |
| `lite_xl.bdd.wire.04` | UTF-16/non-BMP ranges land at the exact intended editor character | exact_source_core; actual host required | Chain H (#10684) |
| `lite_xl.bdd.wire.05` | CRLF disposition is explicit at every synchronization boundary | exact_source_core; actual host required | Chain H (#10684) |
| `lite_xl.bdd.wire.06` | Actual didChange traffic matches the selected #7713 incremental envelope | exact_source_core; actual host required | Chain H (#10684) |
| `lite_xl.bdd.wire.07` | didOpen/change bytes above 50 KiB decode exactly; smaller substitutes prove nothing | exact_source_core; actual host required | Chain H (#10684) |
| `lite_xl.bdd.wire.08` | Wrong Content-Length or a partial frame creates no semantic result | exact_source_core; actual host required | Chain H (#10684) |

### Feature: Lite XL support claims remain subject- and stage-bound

| Scenario ID | User-visible claim law | Profile / evidence tag | Evidence boundary |
| --- | --- | --- | --- |
| `lite_xl.bdd.support.01` | Simulation, profile, composed-tree, exact-host, and public-host outcomes stay distinct evidence kinds that satisfy only their own ceilings | claim-law binding every profile | Chain L |
| `lite_xl.bdd.support.02` | Composed exact-source profile tree/component digests are load-bearing subject identity, not decoration | claim-law binding every profile; composed_exact_source | Chain L (#11170 producers) |
| `lite_xl.bdd.support.03` | One platform/version never satisfies another | claim-law binding every profile | Chain L (#10733 consumers) |
| `lite_xl.bdd.support.04` | Manual-binary and managed-lpm install routes stay independent proofs | claim-law binding every profile | Chain L (#9010 consumers) |
| `lite_xl.bdd.support.05` | candidate_ready/submitted/accepted/released/public-replayed states stay monotone-distinct; accepted-unreleased can never satisfy public support | claim-law binding every profile | Chain L (#10739/#9012 consumers) |
| `lite_xl.bdd.support.06` | Capability advertisement or command visibility never proves host consumption | claim-law binding every profile | Chain L (#11172 consumers) |
| `lite_xl.bdd.support.07` | Admitted advanced features and DAP rows remain independent rails that never fill core LSP scenarios | claim-law binding every profile | Chain L (#10767 consumers) |
| `lite_xl.bdd.support.08` | Quality sidecars never silently block or promote the bounded core | claim-law binding every profile | Chain L |
| `lite_xl.bdd.support.09` | A stale subject or fixture digest invalidates exactly the affected cells and nothing wider | claim-law binding every profile | Chain L (#9016/#7122 invalidation owners) |

## §Behavior — optional and stronger-profile inputs (never baseline blockers)

All optional rows are `consumes_if_available` additive inputs in the #10858
edge sense; their absence cannot block or extend the core, and none creates
`exact_source_actual_host` or public standing by itself.

| Scenario ID | Proposition | Profile relation | Boundary note |
| --- | --- | --- | --- |
| `lite_xl.bdd.opt.01` | Unexpected-death recovery restores a serving session within the bounded envelope | quality_breadth input | sidecar; failure of a zero-budget defect blocks core directly, never via promotion here |
| `lite_xl.bdd.opt.02` | Large-response responsiveness stays inside the envelope | quality_breadth input | sidecar; becomes core-blocking only when a defect breaks the claimed core envelope itself |
| `lite_xl.bdd.opt.03` | Activation dispositions beyond the admitted set are admitted measured-first | quality_breadth input | unadmitted classes stay unsupported until revised here |
| `lite_xl.bdd.opt.04` | Full version/platform matrix rows hold | stronger-profile input feeding platform rails | rows owned by #10733; upstream theoretical prerequisites are never a support floor |
| `lite_xl.bdd.opt.05` | Admitted multi-root and advanced LSP slices work end-to-end | separate rail; additive breadth | owner #10767; never merges into single-root core |
| `lite_xl.bdd.opt.06` | Upstream submission packets reach release state externally | external checkpoint input | owner #10739; internal merge cannot satisfy external stages |
| `lite_xl.bdd.opt.07` | Managed lpm package route installs working artifacts | stronger-profile input feeding managed_exact_source | owner #9010; manual route never substitutes |
| `lite_xl.bdd.opt.08` | Released upstream replay reproduces journeys against public artifacts | stronger-profile input feeding lite_xl_first_class_public | separate direct stage owner #9012; local evidence never relabels upward |

## Claim profiles (ledger membership)

| Profile | Membership rule | Ceiling |
| --- | --- | --- |
| `lite_xl_protocol_baseline` | the `protocol.*` family (13 rows) as this ledger's behavioral propositions; composition membership stays owned by the merged #11170 manifest (pristine-anchor anchor claim there, empty member set until a registration-class leaf lands) | synthetic/exact-process evidence only; no actual Lite XL support claim. Registration- and activation-set compatibility floors are carried by `activate.*` rows at their own ceiling and become prerequisites of stronger profiles, never of this baseline |
| `lite_xl_exact_source_core` | exactly the bounded real-editor rows: activate.01–09, read.01–11, edit.01–12, lifecycle.01–03/08–10, wire.01–08 | the bounded useful journey; nothing else blocks or widens it |
| `lite_xl_workspace_fresh` | core plus lifecycle.04–07/11 (root isolation, watchers, closed-file freshness, cleanup) | selected single-root freshness; never multi-root by naming |
| `lite_xl_first_class_public` | independent added stages with their own direct released/public evidence, owned by #9010/#9012/#10739; `opt.06–08` are `consumes_if_available` rail inputs to these stages and never enter any requirement set (#10858 `optional_edge_in_required_set`) | public standing requires its own direct replay/artifact evidence; internal merges cannot satisfy external stages |
| `lite_xl_quality_breadth` | opt.01–05 additive inputs plus any future quality families | additive only; optional rows never block or widen the core |

Laws: a stronger profile never erases a narrower valid one; membership is
explicit per row and inherited only by listed row IDs, never by naming;
optional/quality rows are `consumes_if_available` inputs whose targets never
enter any profile requirement set; missing chain links are
`not_proven`; every evidence kind (`client_simulation_proven`,
`composed_exact_source`, `exact_source_actual_host`, `managed_exact_source`,
`public_artifact_actual_host`) satisfies only its own ceiling; no scenario
supports any tier until its executable chain passes at that stage.

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
| --- | --- | --- | --- |
| Provider substitution | Only the exact selected perllsp provider satisfies activation/read/edit rows | activate.02/activate.03; F1 | ambient/wrong-server satisfaction rejected |
| Attach theater | Extension syntax detection never counts without actual attach | activate.01/activate.09; F2 | syntax-only pass rejected |
| Trust inversion | Project-controlled config stays inert/untrusted; user data distinct | activate.04/activate.05; F3 | executed-sentinel success rejected |
| Shape forgery | false/null/array/object cardinality and order are exact | activate.06/activate.07/protocol.01; F4 | coerced-shape/order mutations rejected |
| State dropping | Single-send IDs and exactly-one-response survive pressure | protocol.02/protocol.03; F5 | double-send/dropped-message passes rejected |
| Currentness forgery | Publications/replies belong to the current generation | read.03–read.05; F6 | stale replace/clear passes rejected |
| Revival | Post-edit queries cannot serve pre-edit answers | read.08/edit.11; F7 | stale revival rejected |
| Identity blur | Read answers identify the intended target/root | read.07/read.10/read.11; F8 | wrong-subject nonempty answers rejected |
| Preview corruption | Previews never mutate navigation targets | read.09; F9 | preview-mutation passes rejected |
| Application honesty | Returned/logged server results must be actually applied | edit.01/edit.06/edit.07/edit.09; F10/F15 | returned-but-unapplied passes rejected |
| Survivor damage | Edits preserve caret intent and unrelated sentinels | edit.08; F11 | caret-jump/sentinel-touch passes rejected |
| Manufactured freshness | External-change proof must not manufacture restarts/reopens | lifecycle.06/lifecycle.07; F12 | secret-restart passes rejected |
| Generation overlap | Replacement never overlaps old publishes | lifecycle.08/lifecycle.10; F13 | overlapping-process passes rejected |
| Size laundering | Large-wire proof uses the full admitted size | wire.07; F14 | downsized-substitute passes rejected |
| Stage promotion | Each stage kind satisfies only its own ceiling; labels monotone-distinct | support.01/support.02/support.05; F16 | relabeled staged-composition passes rejected |
| Platform substitution | Rows never cross platforms/versions | support.03; F17 | borrowed-platform passes rejected |
| Advertisement proof laundering | Capabilities never equal consumption | support.06; F18 | capability-bit passes rejected |
| Log leakage | Canonical evidence stays bounded/redacted | protocol.12; F19 | raw-log evidence rejected |
| Rail separation | DAP/advanced never fill LSP core rows | support.07; F20 | cross-rail satisfaction rejected |
| Profile erasure | Stronger profile never erases narrower valid one; optionals never block core | profile laws; opt.* rows | breadth-as-blocker mutations rejected |
| Mutable leakage | No live SHA/PR/check/writer/wall-clock state in durable bytes | all three files | live-state injection scan fails closed |
| Determinism | Same tree yields identical checker output twice | `checklist.md` proof | second run byte-clean |

## §Contracts

| Contract | Authority consumed | How this bundle satisfies it |
| --- | --- | --- |
| Checked spec directory shape | [`SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md), #3983 | canonical three-file packet; ledger evolution recorded in `context.md` |
| Product/host controllers | #8950 / #9008 / #10651; denominator migration #11176 | initial profile IDs consumed verbatim; later typed-train migration preserves meaning |
| Fixture manifest boundary | #11181 schema `lite_xl_fixture_expectations.v1`; #11170 composition anchors (`candidate_manifest.lua` profile IDs consumed by reference) | Chain P/H behavioral rows name a stable owning ID the manifest binds; the protocol-baseline composition anchor stays with the landed manifest; zero fixture bytes here |
| Deterministic simulation boundary | #11103 harness/journey suites under `clients/lite-xl/tests/` | protocol rows bind Chain P first link; host rows bind where applicable |
| Exact-process/profile boundary | #8960 | protocol_baseline ceiling consumes it; no new pin |
| Real-host lanes | #10673 adapter; #10676/#10679/#10681/#10684/#10691/#10693 sessions | each host row names its owning lane |
| Security authorities | #10653 project-config execution boundary; safe argv launch; traversal/out-of-root rejection | activate.04/.05, wire.03 guard rows; positive shapes only |
| Wire/position envelopes | #2298 / #7713 | wire.06 consumes the envelope by reference |
| Protocol truth lineages | #10657/#10785/#10833/#10845/#11136/#11143/#11147/#11151/#11155/#11186/#11194/#11197 | rows word their propositions from merged mechanics; mechanisms stay downstream |
| Edit/currentness lineages | #10671/#10754/#8986/#11108/#11124/#11128/#11189/#11188/#11198 | read/edit rows cite owning behaviors only |
| Distribution/public rails | #10733/#9010/#10739/#9012/#10767 | support/opt rows name owner boundaries only |
| Support registry tiers | `policy/lsp-client-support.toml`, projection #7122 / docs #9016 | tags reuse registered vocabulary; no tier changes here |
| Evidence semantics vocabulary | controlling-issue vocabulary (reused, not extended) | table below; no second ontology |
| Typed train/profile substrate | #10338 / #10858 / #11176 | profiles declare membership/ceilings consumable as train nodes |
| Sibling precedent | `.spec/11371-vim-bdd-journeys/`, emacs/zed bundles | same checked discipline; different programme, no overlap |

Evidence-semantics vocabulary consumed verbatim (machine-visible distinctions):

```text
configuration_documented    protocol_profile_proven    client_simulation_proven
composed_exact_source       exact_source_actual_host   managed_exact_source
accepted_unreleased         released_upstream          public_artifact_actual_host
unsupported                 not_proven                 instrument_failed
cleanup_failed
```

Client capability, server capability, actual request/consumption, actual edit
application, current exact, and bounded unversioned observations remain
observation classes defined by their existing producers; this packet only
forbids substituting them across stages.

## §API-Shape

No Rust, Lua, or public API is introduced. Semantic contract terms defined
here:

| Item | Kind | Shape | Dup-risk / owner |
| --- | --- | --- | --- |
| `lite_xl.bdd.<family>.<nn>` | stable scenario ID namespace | 81 IDs, eight fixed families/order, immutable once published | none found on main; this packet |
| `lite_xl_protocol_baseline` | claim profile ID | 13 protocol rows | named by issue; bound here |
| `lite_xl_exact_source_core` | claim profile ID | 46 bounded host rows | named by issue; bound here |
| `lite_xl_workspace_fresh` | claim profile ID | core + 5 freshness rows | named by issue; bound here |
| `lite_xl_first_class_public` | claim profile ID | public-stage membership rule | named by issue; bound here |
| `lite_xl_quality_breadth` | claim profile ID | additive sidecar rule | named by issue; bound here |

## §Test-Grid

All twenty controlling-issue false-green examples, fixed order. Each is a
design-level negative control: a downstream implementation, fixture, driver,
receipt, or projection is conformant only if every listed mutation fails
deterministically in that leaf's own negative controls.

| # | Scenario | Kind | Required verdict |
|---:|---|---|---|
| 1 | Another Perl server or an ambient perllsp binary satisfies the row | negative | reject; the exact selected perllsp provider identity is the proposition (activate.02/activate.03) |
| 2 | Syntax activation is counted without actual server/document attach | negative | reject; activation requires actual attach (activate.01) |
| 3 | Project Lua sentinel executes while setup is called trusted | negative | reject; project-controlled config stays inert and untrusted (activate.04/activate.05) |
| 4 | False becomes null, empty list becomes object, or response item order changes | negative | reject; JSON shape, cardinality, and order are exact (activate.06/protocol.01) |
| 5 | A request is transmitted twice or a required response/message is silently dropped | negative | reject; single-send request IDs and exactly-one-response laws hold (protocol.02/protocol.03) |
| 6 | A stale diagnostic empty list clears newer diagnostics | negative | reject; only current provider/version publications replace or clear (read.03/read.04/read.05) |
| 7 | The same cursor after an edit admits a stale hover/completion/format result | negative | reject; post-edit answers belong to the current generation (read.08/edit.11) |
| 8 | A non-empty hover/navigation/symbol result refers to the wrong target or root | negative | reject; answered identity must match the intended subject (read.07/read.11) |
| 9 | Preview mutation changes the navigation target | negative | reject; navigation targets stay authoritative, previews observational (read.09) |
| 10 | A completion/format/rename edit is returned or logged but not applied | negative | reject; actual validated application is the proposition (edit.01/edit.07/edit.09) |
| 11 | Final bytes are right but the caret jumps or an unrelated sentinel changes | negative | reject; caret intent survives edits and unrelated subjects are untouched (edit.08) |
| 12 | A watcher test secretly restarts or reopens the file | negative | reject; external-change freshness must not be manufactured by restarts (lifecycle.06/lifecycle.07) |
| 13 | Old and new server processes overlap or old callbacks publish after replacement | negative | reject; generation-owned processes and callbacks never overlap publishes (lifecycle.08/lifecycle.10) |
| 14 | The >50 KiB scenario silently uses a smaller file | negative | reject; large-wire decode exactness requires the full admitted size (wire.07) |
| 15 | The protocol trace is correct but the actual Lite XL UI/buffer result is wrong | negative | reject; trace evidence never substitutes for applied editor results (edit.01/read.01) |
| 16 | A staged patch/composed profile is labeled accepted/released/public | negative | reject; stage labels stay monotone-distinct with their own direct evidence (support.05/support.02) |
| 17 | One Linux row satisfies Windows/macOS | negative | reject; platforms never substitute (support.03/opt.04) |
| 18 | A server capability bit creates Lite XL feature support | negative | reject; advertisement is not consumption (support.06) |
| 19 | Raw/source-bearing logs enter canonical evidence | negative | reject; canonical evidence stays bounded and redacted (protocol.12) |
| 20 | A DAP or advanced result satisfies a core LSP profile row | negative | reject; advanced/DAP rails never fill LSP core scenarios (support.07) |

## §Blast-Radius

| Consumer / surface | Impact | Required update |
| --- | --- | --- |
| #11181 fixture/expectation cells | Binds the Chain P/H scenario IDs as owning-scenario keys (currently blocked on them); Chain L `support.*`/optional rows bind through their own named consumers instead and are never a fixture obligation | Consume IDs; do not redefine propositions |
| #11103 suites / #8960 cells | May cite scenario IDs in suite headers/cell provenance | Reference only; schemas unchanged |
| Host lanes #10676../#10673 adapter | Observe against named scenarios | Bind IDs into observations |
| #11170/#11172 producers / #7122/#9016 projections | Cite stable IDs downstream | Reference only |
| `policy/lsp-client-support.toml` | No change in this PR | Future lite-xl rows flow via #7122/#9016 once chains pass |
| Generated inventory surface | Listing gains this packet's three files whenever the sanctioned writer next runs | Outside this bundle; allowlist glob `.spec/**` keeps the gate green meanwhile |
| Product/runtime crates and Lua client | None | Must-not-touch |

Must-not-touch: `crates/`, `xtask/src` beyond the sanctioned inventory writer's
committed output, `clients/`, `.github/workflows/`, fixture sources, host
harnesses, receipts, support-registry values, CI workflows, external upstream
surfaces.

## §Coverage-Map

| #11178 acceptance/validation item | Covered by |
| --- | --- |
| One checked contract separates protocol/exact-source-core/workspace-fresh/public/quality profiles | profile membership tables; five explicit profile IDs |
| Every active core implementation/host leaf owns stable scenario IDs | §Behavior rows name owning chains (#10676../#10693, #11103, #8960, #11181); checker enforces exact 81-ID set |
| Security/protocol/currentness/edit/root-watcher/lifecycle/path-wire/distribution false greens explicit | §Test-Grid rows 1–20 in issue order; §Hazards mapping column |
| Scenario IDs map fixture→simulation→host→support without broadening | named Chain P/H/L columns; per-row owners |
| Unsupported/advanced/platform/external states dimensionally separate | support.03/.05/.06/.07 rows; opt rail notes; profile laws |
| Profiles collapsed ⇒ spec check fails | checker rejects missing/miscounted profile vocabulary and profile-blocked scenario IDs |
| Stages collapsed, missing owner, weak oracle ⇒ spec check fails | rows demand named owners; required-term checks (schema name, lanes) fail closed when removed |
| Cross-subject/platform/stale-state/returned-not-applied/unsafe-config/stale-generated ⇒ downstream controls exist | F1–F20 verdicts assigned to leaf negative controls |
| Generated spec/status outputs current/deterministic | generator surfaces absent on main (recorded evolution); generated inventory left to its sanctioned writer outside this bundle; two-run structural proof below |
| No implementation/composition/host run/support promotion/release occurs | scope boundary + blast radius + claim boundary below |

## Scope, rollback, and proof claims

- **In scope:** exactly the three files of `.spec/11178-lite-xl-bdd-journeys/`.
  The generated inventory projection is intentionally not part of this commit;
  it lists the packet through the sanctioned writer in a later regeneration.
- **Rollback:** revert this bundle's commit; issues retain full authority. Any
  downstream artifact already bound to these IDs reverts through its own owner,
  never by editing this bundle silently.
- **Transfer:** only with exact current evidence inventory and named receiving
  owner; otherwise `not_proven`.
- **Stop:** return to #11178 if a boundary above would need weakening to make a
  downstream check green, or current main contradicts a material decision.
- **Claim boundary:** proves a durable checked journey/evidence contract and
  deterministic structural inspection only. Proves no Lite XL behavior, host
  execution, fixture correctness, receipt, support tier, public artifact, or
  upstream state. All 73 baseline and 8 optional scenarios remain `not_proven`
  as behavior unless their own executable chains have already passed elsewhere,
  in which case those producers cite these IDs — they never widen them.
