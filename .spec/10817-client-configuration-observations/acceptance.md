# Acceptance Criteria: #10817 — adapt client and workspace configuration channels into typed observations

This packet is declarative. It changes no production behavior; its proof surface
is structural (required rows/sections present and stable) plus fixture designs
that become executable only after the wake event named in `checklist.md`.

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| Packet present at `.spec/10817-client-configuration-observations/` | `context.md`, `acceptance.md`, `checklist.md` exist with required sections | structural check below |
| Inventory receipts | every R1–R6 row cites a file:line that exists on the pinned main | spot-check via rg |
| Field-cohort table | records #11845/#11861/#11864/#5703 landed containments and open owners #4997/#10917/#7479 | no stale "testRunner as observation field" row |
| Wake gate | cutover steps are explicitly gated on #10813 + #7010 landing over #10807 | checklist step 0 |

All checks pass: commands in `checklist.md` §Deterministic checking.
Formatted: `git diff --check` clean.

## §Hazards

| Class | Invariant | Surface | Required adversarial test |
|---|---|---|---|
| ID/ref-space collision | N/A — no production code or ID space in this packet | n/a | n/a |
| Bounds/overflow | N/A — no production code | n/a | n/a |
| Protocol-safety | applies to future builder only; fixtures F4/F7 pre-encode malformed-envelope rejection | fixture designs in §Test-Grid | post-wake |
| Scanner literal/comment blindness | N/A — no scanner change | n/a | n/a |
| Test-encodes-the-bug | fixtures must discriminate provenance, not restate parser output (F1 is the discriminating control) | §Test-Grid | post-wake |
| Coverage/measurement integrity | N/A — docs-only change | n/a | n/a |

**Subsystem-specific defaults consulted**: LSP defaults apply to the post-wake
builder, not this spec-only bundle; recorded there rather than dropped.

## §Contracts

The durable contract rows this packet compiles (from #10817's design rulings A–F).
Each names the current source boundary it will bind after wake.

| Contract | Source document + section | How this packet satisfies it |
|---|---|---|
| CFGOBS-C01 initializationOptions observed before mutation | issue #10817 ruling A; inventory R1/R2 | adapter disposition recorded for capabilities.rs:705-725 and replay sites |
| CFGOBS-C02 didChangeConfiguration observed before mutation | ruling A; inventory R3 | imperative route decomposed in workspace.rs:1311-1470 disposition |
| CFGOBS-C03 unscoped result classified generic, never trusted | ruling C; inventory R5 | slot-0 position cannot manufacture `trusted_user_operator`; fixture F2 |
| CFGOBS-C04 per-root result binds exact request/slot/root/generation | ruling D; inventory R4/R6 | registry-bound identity replaces types.rs:91-98 pending tuple; fixture F3 |
| CFGOBS-C05 short/null/malformed result remains typed | ruling B; inventory R4 | per-slot failure states enumerated; fixture F5 |
| CFGOBS-C06 duplicate/late/superseded response produces one terminal observation, no mutation | ruling D; inventory R4 | fixture F4 |
| CFGOBS-C07 removed/re-added root rejects old observation | ruling D | fixture F6 |
| CFGOBS-C08 client-supplied trust/scope/name cannot strengthen source | ruling C | fixture F7 |
| CFGOBS-C09 same value from different provenance has different observation identity | ruling C | fixture F1 |
| CFGOBS-C10 present-unauthorized differs from absent/default | ruling B | folded into F1/F5 assertions |
| CFGOBS-C11 credentials/private paths redacted in receipts/debug | issue required-behavior list | receipt surface named for builder; fixture F8 |
| CFGOBS-C12 high-risk rows consume exact security owners | field-cohort table | table pins owner dispositions incl. closed containments |
| CFGOBS-C13 migrated channels have no direct raw setters | direct-write boundary | architecture recurrence check assigned to builder (step 8) |
| CFGOBS-C14 compatibility projection cannot strengthen/drop provenance | ruling F | projection constraints recorded in checklist step 6 |
| CFGOBS-C15 observation creation deterministic, map-order independent | issue deterministic-test 9 | fixture F9 |
| CFGOBS-C16 adapter emits observations only; no precedence/publication/consumer effect | acceptance boundary | non-goal restated in context + checklist stop conditions |
| CFGOBS-C17 #10386/#10387 can consume without reparsing raw transport | ruling A pipeline | observation payload carries denominator + identity fields |

## §API-Shape

N/A — this bundle introduces no public API. The future observation-contract API
is owned by #10813 over the #10807 denominator; pre-declaring it here would fork
authority.

## §Test-Grid

Fixture designs for the post-wake builder (executable then, designs now). Each is
a discriminating control, not a restatement of parser behavior. Exact-process
fixtures run against a captured server request/response cycle per the issue's
verification contract.

| Scenario | Kind | Fixture design (post-wake name) | Invariant |
|---|---|---|---|
| Same visible AI/test value from trusted adapter vs generic client | positive+adversarial | `same_value_different_source_yields_distinct_observations` — feed identical JSON through trusted-user-operator adapter and unscoped response slot; assert distinct observation identities and authority outcomes, identical parsed values | C03/C09/C10 |
| First unscoped result self-labels trusted | adversarial | `unscoped_slot_cannot_authorize_endpoint_or_external_root` — slot-0 payload containing endpoint-shaped and external-root-shaped fields yields unauthorized field states, no arming, no include-path widening | C03/C12 |
| Root A result satisfies root B / later generation | negative | `per_root_result_rejects_wrong_slot_root_generation` — correct-length array with swapped/mismatched slot mapping rejected as typed mismatch | C04 |
| Late/duplicate/superseded response | state | `response_terminal_once_and_non_mutating` — deliver success twice, then error, then timeout for same request id; exactly one terminal observation, effective stores byte-identical, pending count baseline | C06 |
| Short/null/malformed array with valid sibling | negative | `malformed_array_preserves_typed_failures_without_partial_update` — `[null]`, `[]`, `["not-an-object"]`, `{}` inputs yield exact per-slot failure states; valid sibling fields do not partially mutate unrelated accepted state | C05 |
| Removed root repopulated by stale response | state | `removed_root_observation_rejected` — respond for root removed between request and response; typed rejection, folder set unchanged | C07 |
| Client-supplied `scope`/`trusted`/client-name labels | adversarial | `client_labels_ignored_in_source_assignment` — payload carrying `"trusted": true, "scope": "machine"` still classified `generic_unscoped_client` | C08 |
| Credential/private path redaction | adversarial | `receipt_debug_output_redacts_credentials` — apiKey-shaped and absolute-private-path values absent from observation fingerprint/log/receipt surfaces | C11 |
| Map/field/slot permutation determinism | property | `observation_identity_map_order_independent` — permute JSON object key order and slot iteration; identical digests | C15 |
| Direct raw setter bypass on migrated channel | architecture | architecture recurrence check fails when a migrated channel calls `ServerConfig::update_from_value` / `WorkspaceConfig::update_from_value*` outside forwarding-parser role | C13 |
| Real envelope classification | integration | exact-process capture proves responses enter through #7010-correlated envelopes; method-shaped `$/perl-lsp/clientResponse` carries no authoritative observation | C04/C06 |

Negative-control requirement: mis-typed/unexpected client capability payloads
degrade to typed rejection (never panic, never silently widen authority). Fixtures
F2/F5/F7 encode this directly.

## §Blast-Radius

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| This packet's future builder | perl-lsp-rs-core, perl-lsp-rs | reads packet | none until wake | follow checklist |
| Children #10898/#10909/#10917 | — | consume substrate | none yet | blocked behind same wake |
| CI/docs checks | xtask | structural | none — markdown only | none |

Must-not-touch boundary: everything outside `.spec/10817-client-configuration-observations/`.
