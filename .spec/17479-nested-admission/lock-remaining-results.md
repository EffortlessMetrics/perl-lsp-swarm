# #17479: five remaining lock measurements qualified

The five exact remaining lock-partition tests pass on source `755d1bc95a714d773ff4e85da1ab7c0afc956471`, using native Linux Rust/Cargo/Clippy 1.95.0 and the retained private worktree target/build roots. Five tests passed, zero failed or ignored, 4,787 filtered; test execution took 4.82 seconds. The entire admitted batch took 12.484 seconds, with the gate taking 10.718 seconds. This is a bounded manual qualification, not closure of the whole issue or activation of a hosted gate.

## Scope and topology

The exact tests are under `tasks::check_lint_policy::tests::lock_partition::`:

- `clippy_row_covers_parking_lot_guards_and_not_standard_library`
- `discards_outside_the_governed_forms_are_covered_by_neither_row`
- `explicit_drop_discards_are_covered_by_no_clippy_lint_at_any_level`
- `let_underscore_must_use_owns_owned_and_mapped_guard_discards`
- `rustc_row_covers_standard_library_guards_and_not_parking_lot`

Their original Rust bodies and diagnostic oracles are unchanged. The finite admitted route now supports their existing exact lint tuples, five literal exact libtest names, one test thread and one original admission owner. All five prerequisite Clippy members must exist before launching the harness. Runtime success requires the exact five current passes, current positive diagnostic sites, successful instrument terminals, immutable actual Cargo-reported harness capture and original-owner settlement.

| Supported topology | Root bootstrap | Owning Cargo test | Distinct Clippy measurements | Total build/test invocations |
|---|---:|---:|---:|---:|
| Each exact test admitted separately | 5 | 5 | 5 | 15 |
| One finite batch | 1 | 1 | 5 | 7 |

The separate topology is a source-based counterfactual, not five measured timings. Version/metadata probes are excluded. Actual measurement receipts and source establish the batch's seven commands; process sampling observed only three short-lived Cargo processes and is not a complete invocation census. Qualification required two native attempts, fourteen build/test commands in total. The second attempt reused the harness compiled in the first, so these timings are not a before/after speedup comparison.

## Diagnostics and compatibility repair

| Measurement | Lint flags after `--` | Diagnostics | Cargo artifacts fresh/total |
|---|---|---:|---:|
| Clippy row | `-A let_underscore_lock --force-warn clippy::let_underscore_lock` | 2 | 9/10 |
| Union boundary | `--force-warn let_underscore_lock --force-warn clippy::let_underscore_lock` | 4 | 9/10 |
| Sweep | union flags plus `-W clippy::all`, `pedantic`, `nursery`, `restriction` | 36 | 9/10 |
| Must-use row | both lock rows allowed, `--force-warn clippy::let_underscore_must_use` | 5 | 9/10 |
| Rustc row | `-A clippy::let_underscore_lock --force-warn let_underscore_lock` | 2 | 9/10 |

Each command remains offline, quiet, library-only, no-deps and JSON instrumented. The fixture's nine dependencies were fresh; its own artifact rebuilt for each distinct lint tuple. All five actual untruncated raw JSON streams are retained with source, argv, roots, snapshot, original owner and successful diagnostic validation.

The first attempt on `a88779388…` compiled the changed harness and ran five tests: four passed, the sweep failed because the adapter rejected a span. It took 541.856 seconds. Its owner settled normally. The failed sweep's raw stream and an immutable failing harness were not retained; that attempt remains a failed trace, not qualification or retained-executable replay.

The repaired adapter accepts empty-text nongoverned advice without using it as liveness and tracks `(statement, lint)` pairs. Optional standard-library must-use warnings may overlap required rustc warnings; they cannot replace the required rustc positives. Duplicate identical pairs, governed held-guard findings and any sweep finding on an explicit-drop statement still refuse. The successful native sweep emitted two rustc lock warnings, two borrowed parking_lot warnings and five must-use warnings (two standard-library, three owned/mapped). Its blanket restriction advice carried an empty-text primary span in bound `clippy.toml`. Replaying this retained current stream through the original adapter reproduces its span refusal; equivalence to the unavailable earlier stream is not proven. The unchanged Rust sweep oracle evaluated the full stream and passed its explicit-drop silence ruling.

## Freshness, resources and closure

The winning outer bootstrap completed in 0.27 seconds and positively reported all three ring units up to date. Nested Cargo reported **384/384 artifacts fresh**, including ring and the actual current xtask main-bin test harness. Ring fingerprint bytes and rlib identity/hash were unchanged before and after both attempts; no attributable ring compiler was sampled. The first attempt had 383/384 fresh artifacts and rebuilt only the harness after the Rust routing change.

| Attempt | Whole duration | Sampled aggregate memory peak | Peak additional volume use | Minimum free space |
|---|---:|---:|---:|---:|
| Initial, 4 pass / 1 fail | 541.856 s | 11.798 GiB | 341,037,056 bytes | 23.369 GiB |
| Repaired, 5 pass | 12.484 s | 7.589 GiB | 377,413,632 bytes | 23.334 GiB |

Volume use is initial free minus minimum sampled free, not net retained growth. Most winning-attempt use was the 377,052,296-byte immutable harness copy. Its SHA-256 is `cf978f6e2585fd9df34386a324c3a52b70be6c792b1bb4d3a82dd78336eb7672`; the copy is read-only and was bound to the actual Cargo artifact before owner release.

Each attempt used a fresh exact-scope budget: unverified 14 GiB additional-growth forecast plus 8 GiB reserve, jobs=1, incremental=0, retained outputs and unchanged compiler environment. A 0.25-second observer would cancel through the original owner's pidfd below 10 GiB free, at 14 GiB aggregate memory, after 5,400 seconds or on unexpected ring compilation. These are experiment premises and sampled aggregate measurements, not quotas, process RSS or cold-sizing proof. Memory-event counters stayed unchanged.

Both owners separately proved kernel `ECHILD (__WALL)`, zero errors/pending descendants/cancellation, and matching original lease/marker release. Winning snapshots and actual executable bytes were copied and identity-checked under the original live owner.

## Controls and remaining qualification

The final scoped Python suite ran 186 tests: 185 passed, one opt-in product-build test skipped. Ten focused batch controls passed and were independently repeated. Five deliberate boundary mutants failed assertions without instrument errors. Native Rust formatting, Python syntax and diff whitespace checks passed. Independent review checked the repaired source and raw native evidence, including hashes, diagnostic streams, freshness, resource arithmetic and owner closure.

The [evidence packet](lock-remaining-evidence.json) contains both attempt denominators, the finite immutable plan, exact tools/configuration/private roots, actual harness and five measurement receipts, positive ring traces, ownership proofs, raw-file hashes, controls and limits. The [source plan](lock-remaining-plan.md) and manual gate policy remain unactivated. Main was freshly read as `95a722963…`; candidate and main workflow triggers had zero matching push/create events for the existing quiet branch.

Cold sizing, general five-mode compatibility, full preparation/runtime, Windows/macOS, broader issue closure and the earlier cancelled ring rebuild's cause remain **NOT_PROVEN**. Profiles, storage policy, original dependency closure, existing caches and canonical gate policy were preserved. No public comment, PR, merge or hosted workflow was activated.
