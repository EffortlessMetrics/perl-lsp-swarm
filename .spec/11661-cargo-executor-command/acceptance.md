# Acceptance Criteria: #11661 — expose the canonical Cargo executor command and durable receipts

This packet is declarative. It changes no production behavior; its proof surface is
structural (required rows/sections present and stable, deterministic checks green
now) plus fixture designs that become executable only after the wake event named in
`checklist.md` Step 0.

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| Packet present at `.spec/11661-cargo-executor-command/` | `context.md`, `acceptance.md`, `checklist.md` exist with required sections | structural check in checklist |
| Currentness table | every prerequisite row cites live state verified against a pinned main SHA | context.md Status section |
| Receipt authorities consumed | rows name session-receipt schema pattern, publication_drift atomic write, gate-receipt path conventions — no new envelope invented | context.md current-main facts |
| Wake gate | command/receipt cut explicitly gated on #11642+#11647+#11660(+#9550) landing; #11650/#11653/#11659 consumption points named | checklist Step 0 |
| Falsifier coverage | issue falsifiers 1–17 each map to a contract row and a post-wake fixture | tables below |

All checks pass via the commands in `checklist.md` §Deterministic checking.
Formatted: `git diff --check` clean.

## §Contracts

Durable contract rows this packet compiles from #11661. Each names the source
authority it binds after wake. IDs are stable for review reference.

| ID | Contract | Post-wake binding authority |
|---|---|---|
| CEXC-C01 | One canonical command exposes the transaction with NO free-form Cargo/shell/raw-argv input surface | #9550 identity row + clap registration per current conventions |
| CEXC-C02 | Typed requests preserve exact subject: package IDs resolve through Cargo metadata/stable IDs; basename/cwd/rendered-text never select the subject | #11647 subject types |
| CEXC-C03 | Request packets bind schema, digest, exact subject, producer, currentness; stale/cross-subject packets fail closed | this receipt contract over #11647 request digest |
| CEXC-C04 | Missing/malformed request input (file/stdin) is instrument failure, never an empty default operation | fail-closed doctrine (session_receipt.rs:23-29) |
| CEXC-C05 | One versioned `cargo_operation_result.v1` object retains every executor plane incl. selected/executed work, state/capacity/process identities, timing decomposition, bounded logs+hashes, terminality/release, limitations | #11647 result planes; identities from #11650/#11653/#11659 |
| CEXC-C06 | Atomic publication: private complete build → invariant validation → unique temp under owned root → flush → content/operation-bound rename → read-back validate; publication failure downgrades reporting/instrument planes and can NEVER leave product success standing as complete proof | publication_drift/mod.rs:246-263 convention |
| CEXC-C07 | No predictable shared temp name; no overwrite of another current attempt; cleanup removes only positively-owned temporaries; stale valid-looking final files are never reused | C06 corollary, fixture F10 |
| CEXC-C08 | Human, JSON, explain, and exit status derive from ONE validated object and agree semantically; presentation order changes no semantic digest | #11647 composition law (projections derive, never compete) |
| CEXC-C09 | Exit mapping: 0 only = requested operation reached required success state incl. nonzero executed work + terminal-clean + valid receipt published; nonzero classes remain distinct (product vs not-proven/instrument vs cancelled/timeout/nonterminal); NOT_APPLICABLE is never inferred here | #11647 plane truth table |
| CEXC-C10 | Validation never trusts process exit, file name, or surrounding output; unknown schema/result variants fail closed and stay visible | C05/C09 |
| CEXC-C11 | Explain operates offline on a receipt, names every non-success plane in deterministic order, never reruns | C08 |
| CEXC-C12 | Narrow rerun packet preserves exact subject/model/target/filter/profiles/toolchain; rejects stale/cross-subject reuse; `--print` renders, never executes | this contract over landed domain |
| CEXC-C13 | Durable/public-safe output excludes secrets/tokens/full environment/private absolute paths/source bodies/unbounded logs/raw handles; redaction that removes proving evidence downgrades the affected plane instead of leaving it green | privacy rules + fail-closed doctrine |
| CEXC-C14 | Retention classification flows through existing policy owners; artifact/path existence never substitutes for receipt validation | gate-receipt retention conventions |
| CEXC-C15 | The command invokes #11660 programmatically only; no wrapper change, no caller migration, no Just/Nix/skills/hooks edits, no tool installation, no retry-until-green | issue non-goals; adapter owner #11662 |

## §Falsifier map (issue shift-left list → control)

| # | Issue falsifier | Contract row | Post-wake fixture |
|---|---|---|---|
| 1 | raw Cargo/shell argv bypass | C01 | F01 |
| 2 | display command reparsed as semantic input | C01/C02 | F02 |
| 3 | package basename/current dir becomes authority | C02 | F03 |
| 4 | missing/malformed input defaults to plausible operation | C04 | F04 |
| 5 | cross-subject or stale request/receipt validates | C03 | F05 |
| 6 | human/JSON/exit derive from different result paths | C08 | F06 |
| 7 | capacity/setup/instrument failure printed as product failure | C09 | F07 |
| 8 | exit zero despite zero work or nonterminal process | C09 | F08 |
| 9 | product success survives failed receipt publication | C06 | F09 |
| 10 | predictable temp/final path enables overwrite/reuse | C07 | F10 |
| 11 | artifact/path existence substitutes validation | C14/C10 | F11 |
| 12 | rerun silently changes subject/model/target/filter/profile | C12 | F12 |
| 13 | reproduce packet leaks secrets/private environment | C13 | F13 |
| 14 | unknown receipt variant ignored | C10 | F14 |
| 15 | redaction removes proving evidence while cell stays pass | C13 | F15 |
| 16 | command installs tooling or retries until green | C15 | F16 (static: no install/retry code path) |
| 17 | PR migrates callers / embeds policy in Just/Nix/skills | C15 | diff-boundary check (post-wake PR body map) |

## §Hazards

| Class | Invariant | Surface | Required adversarial test |
|---|---|---|---|
| Proof manufacture via bypass | typed-only inputs; reparsed display text rejected | fixtures F01/F02 | post-wake |
| Stale/cross-subject proof reuse | digest+subject bound at request AND receipt validation | F05 | post-wake |
| Publication-failure overclaim | product exit cannot manufacture valid receipt after failed publish; run class degrades to reporting/instrument non-success | F09 | post-wake |
| Plane collapse | capacity/setup/process/work-count/metrics/reporting stay distinct in every projection | F06/F07/F08 | post-wake |
| Privacy leak | secrets/env/paths absent from receipt+rerun surfaces; removal of evidence visible as downgrade | F13/F15 | post-wake |
| Test-encodes-the-bug | fixtures assert derived-from-one-object equivalence, not string echoes of renderer output | F06 | post-wake |

## §API-Shape

N/A — this bundle introduces no public API. Command spelling, registry row shape,
request schema, and `cargo_operation_result.v1` type names bind to #9550/#11647/
#11660 AFTER wake; pre-declaring them in Rust would fork authority (context.md,
alternatives rejected).

## §Test-Grid

Fixture designs for the post-wake builder (executable then; designs now). Each must
discriminate, not restate.

| Fixture | Kind | Design | Invariant |
|---|---|---|---|
| F01 `raw_cargo_argv_rejected_structurally` | adversarial | attempt CLI/packet forms carrying free-form cargo argv/shell strings; parser has no variant that accepts them (type-level + parse-level rejection) | C01 |
| F02 `display_command_not_semantic_input` | adversarial | feed a rendered command line back through every input channel; identical rejection everywhere; no reparse path exists | C01/C02 |
| F03 `basename_and_cwd_never_select_subject` | adversarial | two worktrees with same directory basename, different SHAs; requests bind metadata-resolved package IDs + commit identity, not names | C02 |
| F04 `missing_or_malformed_input_is_instrument_failure` | negative | absent file, unreadable stdin, malformed JSON, unknown field → typed instrument/not-proven class, exit ≠ 0, NO default request constructed | C04 |
| F05 `stale_cross_subject_rejected` | state | replay request packet / receipt across candidate SHA change and across subject swap; validation fails closed both directions | C03 |
| F06 `one_object_four_projections_agree` | property | render human/JSON/explain/exit from one receipt; permute presentation ordering; semantic digest identical; all four agree on class/subject/limitations | C08 |
| F07 `non_product_planes_never_become_product` | truth-table | inject setup-failure, capacity-blocked, instrument-failure observations; human text names the true plane; exit class is not product-failure | C09 |
| F08 `zero_work_and_nonterminal_cannot_exit_zero` | negative | zero selected/executed work with green product exit; killed child with surviving descendants; both exit nonzero with distinct classes | C09 |
| F09 `publication_failure_downgrades_not_overclaims` | fault-injection | make rename/read-back fail (occupied target, read-back mismatch); product planes may record product outcome but run class + exit become reporting/instrument non-success; no valid receipt exists | C06 |
| F10 `temp_final_paths_unpredictable_and_owned` | adversarial | concurrent attempts on one root: no shared predictable `.tmp`; second attempt cannot overwrite first's final file; cleanup touches only own temporaries | C07 |
| F11 `existence_is_not_validation` | adversarial | plant plausible receipt-shaped file at expected path; validate refuses without schema+digest+subject checks; artifact presence changes nothing | C10/C14 |
| F12 `rerun_preserves_exact_subject` | property | generate rerun packet, perturb each of subject/model/target/filter/profile; validator rejects every perturbation; unperturbed packet accepted offline | C12 |
| F13 `rerun_packet_privacy` | adversarial | run with secret-shaped env vars and private absolute paths; scan packet bytes: none present; bootstrap named as prerequisite, not installed | C13 |
| F14 `unknown_variant_fails_closed_visibly` | negative | receipt JSON with unknown schema version / unknown result variant → validation error names the unknown variant; never ignored | C10 |
| F15 `redaction_visible_as_downgrade` | truth-table | force log-hash redaction; affected plane renders as downgraded/limited, never green | C13 |
| F16 `no_install_no_retry_paths` | static/architecture | grep/recurrence check: no package installation, no retry loop around the transaction in command code | C15 |

## §Blast-Radius

| Consumer | Crate/surface | Dependency type | Impact now | Required update |
|---|---|---|---|---|
| This packet's future builder | xtask | reads packet | none | follow checklist order |
| #9549/#9554/#11630/#9567 callers | — | future consumers | none | migrate in their own lanes |
| scripts/cargo-safe, justfile | wrapper surface | replacement target | none | owned by #11662 |
| CI/docs gates | xtask structural checks | structural | none — markdown only | none |

Must-not-touch boundary: everything outside `.spec/11661-cargo-executor-command/`.
