# Acceptance Criteria: #11470 — standalone owned-state manifests and safe removal plans

This bundle is contract + checked validator + fixtures. It changes no
production behavior, deletes nothing, and mutates no PATH/profile/registry/
current selection. Proof surface: structural packet checks, validator unit
tests over the committed fixture set, and CLI validation runs.

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| Packet present at `.spec/11470-standalone-owned-state-manifest/` | `context.md`, `acceptance.md`, `checklist.md` exist with required sections | structural check in checklist |
| Currentness table | every sibling row cites live state verified against pinned main SHA `cce85d167` | context.md Status section |
| Canonical fixture set validates | all six scenario manifests, three plans, two results pass the checked validator | fixtures below; CLI runs in checklist |
| Malformed manifests fail named | unknown role, unbounded identity, ambiguous running-state, and traversing/non-canonical root spellings each rejected naming its invariant | negative fixtures + mutation battery |
| Result⇔plan⇔manifest binding | combined `--manifest --plan --result` run reconciles identities and per-row populations; foreign plan ids, moved digests, unplanned rows, duplicates, overlaps, and coverage gaps rejected naming their invariant | binding rules R09–R11 + CLI runs |
| No filesystem mutation anywhere in the lane | validator parses documents only; no walk, no delete, no env/PATH access | diff boundary + code review |

## §Contracts

Durable contract rows compiled from #11470. IDs stable for review reference.

| ID | Contract | Post-wake binding authority |
|---|---|---|
| SOS-C01 | `standalone_owned_state.v1` is a closed object: schema_version exact, unknown fields rejected, required rows cover install root identity (absolute path + sha256 + link policy), installer kind/schema, current/previous candidate identities, other-roots observations, entry rows, transaction lineage, enumeration completeness, redaction policy | this schema + example struct set |
| SOS-C02 | Bounded identity only: every present entry carries a digest (`sha256_content` or `directory_tree_digest`); filename, version, executable bit, or familiar path is never ownership; relative paths are exact — no absolute paths, drive letters, backslashes, parent segments, empty segments, or glob/shell metacharacters | entry rules M06–M09 in validator |
| SOS-C03 | Ownership classes are exactly the issue's eight values; class→retention disposition is one total fixed function; duplicate rows for one path are invalid | classification table in validator |
| SOS-C04 | Running-state is unambiguous both directions: `running_or_active` ⇔ non-empty process refs; refs on any other class are malformed; running state blocks destruction (`blocked_running`) | entry rule + plan legality |
| SOS-C05 | Absence is recorded honestly: absent rows carry unavailable identity and no stale digest; incomplete enumeration requires a reason and blocks every destructive action ("absence from incomplete enumeration is not ownership or safe absence") | enumeration + plan rules |
| SOS-C06 | User-edited markers are foreign: `user_modified` forces `foreign_or_user_owned`; foreign/package-route/unknown/malformed rows can never receive a destructive action | entry coherence + action legality |
| SOS-C07 | Root-link substitution cannot escape ownership: install root binds its digest over one canonical normalized absolute representation — posix `/seg`, drive `X:\seg`, or unc `\\host\share`, each without dot/parent segments, empty segments, trailing separators, mixed separators, or glob/device metacharacters; an equivalent respelling of the same physical root is invalid so one digest binds exactly one representation; substitution after planning fails currentness | canonical-root rules + bound_subject equality checks |
| SOS-C08 | `standalone_removal_plan.v1` is pure and total: exactly one disposition per manifest row (remove_exact/remove_marker/preserve/revalidate); no actions outside manifest rows; remove_marker ⇔ marker roles; `order_index` is the enforced execution order — exactly the canonical 0-based sequence matching array position, so duplicates, gaps, permutations, and reversal are invalid and array/index order are one authority | plan totality rules P01–P03 + order rule P04 |
| SOS-C09 | Destructive actions require exact currentness: complete enumeration, present row, owned class, no process refs, verified_identity_sha256 equal to the recorded digest, and lifecycle policy selected; rollback-retained rows are removed only under `full_removal_selected` (rollback ≠ uninstall) | destructive legality matrix |
| SOS-C10 | Plan binds subject at planning time: root path/digest plus manifest sha256 must equal current observation; any movement ⇒ refuse with `root_or_manifest_mismatch` semantics | stale-binding test + CLI |
| SOS-C11 | PATH cleanup composes with #11468/#11469 ownership: cleanup entries must be manifest marker rows paired with remove_marker actions; user-edited or foreign markers are excluded; skipped mode carries nothing | path_cleanup rules |
| SOS-C12 | Postconditions are mandatory and exact: hosted fresh-process proof required=true always; verify_entries_absent equals the destructive set exactly; verify_preserved equals the preserve-disposition population exactly — no unplanned rows, no duplicates, no overlap with the absent list; revalidate rows belong to neither postcondition list until revalidated | postcondition exact-set rules |
| SOS-C13 | Running-process policy is explicit and never kills: abort_on_running / wait_external_then_abort / require_manual_confirmation | enum + plan field |
| SOS-C14 | `standalone_uninstall_result.v1` vocabulary is closed (eleven values from the issue) and coherent: partial_failure ⇔ failed entries; partial failure never becomes success; already_absent_owned_state requires complete evidence; blocked/cancelled/instrument/not_proven/mismatch report zero removals; completed removal is not retryable (idempotent rerun reports already_absent_owned_state). Results bind the exact validated plan and current manifest observation: plan_id and bound_manifest_sha256 must match the plan's identity; removed rows reconcile to planned destructive actions only; preserved rows equal the plan's preserve population exactly for executed outcomes; failures reconcile to admitted actions; reported populations are duplicate-free and pairwise non-overlapping; nothing-executed outcomes claim no preserved rows; a destructive action missing from both removed and failed entries is rejected | result rules R01–R08 + binding rules R09–R11 |
| SOS-C15 | `not_applicable` exists only under #11417 conditional activation selection; missing manifest is never automatically clean absence | activation gate rule |
| SOS-C16 | Deterministic serialization: canonical key-sorted JSON output is stable across parses and runs (`--print-canonical`) | canonicalization function + test |

## §Falsifier map (issue negative controls → control)

| # | Issue falsifier | Contract row | Control in this PR |
|---|---|---|---|
| 1 | filename/path/root familiarity proves ownership | C02/C06 | mutation battery: glob/parent/drive rejections; foreign-row removal rejection (fixture `plan_invalid_*`, tests) |
| 2 | recursive/glob deletion removes user/foreign state | C02/C08/C09 | glob metacharacter rejection; `notes.txt`/binstall rows preserve-only in plans |
| 3 | missing manifest is clean absence without complete evidence | C14/C15 | `already_absent_owned_state` without `complete_evidence` rejected (test) |
| 4 | running/current candidate deleted prematurely | C04/C09 | `manifest_running_current.json` + aggressive-plan rejection (`blocked_running`); all-preserve plan validates |
| 5 | Cargo/Binstall/package/editor state removed | C06 | package-manager class rows preserve-only; removal attempt rejected by class guard |
| 6 | PATH cleanup removes unowned/user-edited state | C11 | cleanup composition tests: foreign marker entry rejected; skipped-with-entries rejected |
| 7 | root link substitution escapes ownership | C07 | symlink fixture classifies substituted dir `unknown_not_safe_to_delete`; plan binding fails on changed digests |
| 8 | partial failure becomes success | C14 | `removed` with failed entries rejected; partial fixture stays partial with retryable=true |
| 9 | rollback is uninstall | C09 | retain-policy plan removing retained rows rejected ("rollback and uninstall are distinct") |
| 10 | issue existence activates release-critical work | C15 | premature `not_applicable` rejected without activation selection |

## §Hazards

| Class | Invariant | Surface | Required adversarial test |
|---|---|---|---|
| Ownership laundering | class⇒retention total function; no third state | manifest validation | retention-drift mutation test |
| Silent overwrite of in-use files | running ⇔ refs; running blocks destruction | entry + plan rules | ambiguous-running fixtures/tests |
| Plan drift | digest-bound subject; totality both directions; order_index is the enforced canonical sequence | plan validation | stale binding, dropped/extra action, duplicate/gap/permutation/reversal order tests |
| Vocabulary escape | closed enums everywhere (serde unknown variant errors) | parse layer | unknown-role fixture |
| Dishonest outcomes | result coherence table plus exact result⇔plan⇔manifest binding | result validation | contradiction battery + foreign-plan/foreign-removed/coverage-gap/preserve-drift binding tests |

## §API-Shape

No workspace public API. Surfaces added: three JSON Schemas, one xtask
example binary (`standalone_owned_state`) with `--manifest/--plan/--result/
--print-canonical`, deterministic fixtures. Platform successors consume the
schemas/fixtures as-is; type names may be absorbed into their lanes later
without breaking these documents.

## §Test-Grid

Executable now (validator unit tests, `cargo test -p xtask --example
standalone_owned_state`):

| Fixture/Test | Kind | Discriminates |
|---|---|---|
| `manifest_canonical_full_install.json` | positive | full owned+foreign+package+marker+lineage state classifies and validates |
| `manifest_running_current.json` | positive | running row with pid+socket is representable without ambiguity |
| `manifest_symlink_substitution.json` | positive | refused link substitution lands in `unknown_not_safe_to_delete` with honest unavailable identity |
| `manifest_instrument_failed.json` | positive | incomplete enumeration + malformed row records honestly |
| `manifest_partial_deletion_retry.json` | positive | failed prior attempt + absent rows = idempotent retry state |
| `manifest_user_edited_path.json` | positive | user-edited marker is foreign, not owned |
| `manifest_invalid_unknown_role.json` | negative | closed role vocabulary rejects unknown roles visibly |
| `manifest_invalid_unbounded_identity.json` | negative | glob/parent targets are unbounded identity |
| `manifest_invalid_ambiguous_running.json` | negative | removable row carrying a live pid is ambiguous |
| `manifest_invalid_traversing_root.json` | negative | parent-segment root spelling is non-canonical unbounded identity |
| `plan_full_removal.json` / `plan_rollback_retained.json` | positive | policy distinction: retained rows removed vs preserved under the two lifecycle policies |
| `plan_blocked_running_all_preserve.json` | positive | running state yields zero destructive actions; revalidate row stays out of both postcondition lists |
| `plan_invalid_stale_binding.json` | negative | moved manifest digest refuses planning (root_or_manifest_mismatch) |
| `result_partial_failure_retryable.json` / `result_already_absent_complete_evidence.json` | positive | coherent outcome documents binding their exact plan and manifest identities |
| mutation battery + adversarial tests (canonical-root spellings, enforced order sequence, exact postcondition populations, result set laws, result⇔plan⇔manifest reconciliation) | adversarial | each safety invariant fires for the named reason, not incidental parse failure |

## §Blast-Radius

| Consumer | Surface | Impact now | Required update |
|---|---|---|---|
| This packet's successors (#11471/#11472) | schemas + fixtures + validator | none | implement executors against these documents |
| #11179/#11425–#11430 | candidate/selection identities | none | bind real digests when lanes land |
| #11467–#11469 | marker rows | none | compose ownership when persistence lands |
| CI/file policy | allowlist + inventory | registration only | included in this PR |

Must-not-touch boundary: everything outside `.spec/11470-*`,
`schemas/standalone_{owned_state,removal_plan,uninstall_result}.v1.schema.json`,
`fixtures/experience/install_owned_state/`,
`xtask/examples/standalone_owned_state.rs`, `policy/non-rust-allowlist.toml`,
`docs/policy/NON_RUST_INVENTORY.md`.
