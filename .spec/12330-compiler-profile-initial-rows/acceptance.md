# Acceptance: #12330

| Obligation | Evidence |
| --- | --- |
| Every initial profile proposition has one stable explicit row ID | `LOCAL_ROWS`/`PROJECT_ROWS`/`EXECUTION_ROWS`/`MAINTAINED_ROWS` exact-set tables; `initial_rows_match_the_required_inventory_and_close_the_import_chain` |
| Every row names one exact subject, owner, evidence family, currentness/completeness/work law, limitation/exit and claim ceiling | Non-optional #12186 row fields + `CompilerProfileRow::validate` run over the whole inventory; `every_row_carries_the_full_field_set_and_a_canonical_owner` |
| Bounded selected observation rows consume #12291/#12139–#12141 without requiring #8722 | Six `lexical.observation-*` rows owned by the bounded packet with bounded coverage; `falsifier_02_integrated_publication_is_not_a_local_prerequisite`, `falsifier_03_integrated_publication_cannot_redefine_bounded_observations` |
| #8722 remains a separately owned later integrated-publication row source | `intelligence.integrated-publication-8722` explicit `Unsupported` row; `falsifier_02`, `falsifier_03`, `falsifier_08` |
| Existing canonical owners are reused; terminal proof packets are referenced, not reimplemented | Owner constants instantiate the canonical owner map only; `every_row_carries_the_full_field_set_and_a_canonical_owner` asserts every row owner references the map |
| Lower-profile imports preserve every exact row and limitation | Exact id+version+digest imports; `verify_import_closure` over the full chain; `imports_preserve_every_row_and_limitation_verbatim`, `falsifier_10_import_by_name_only_fails_closure` |
| Product, instrument, currentness, work, stage and claim dimensions remain independent | Closed `ClaimFamily`/`ProofClass`/`SourceTier` dimensions per row; `falsifier_04`, `falsifier_05`, `falsifier_06`, `falsifier_07` |
| Row and profile identity is deterministic and semantic | Pinned `semantic_fingerprint` digests; `initial_profile_digests_are_pinned`, `falsifier_09`, `falsifier_11`, `falsifier_12` |
| The inventory feeds #12187 without a second hand-maintained copy | `initial_profiles()` public constructors as single authority; `initial_profiles_are_the_single_authority_for_the_manifest` |
| No live candidate evaluation, product behavior, support or release action occurs | Data-only constructors; `inventory_performs_no_evaluation_or_product_behavior`, `falsifier_13_workflow_state_is_never_evidence`, `falsifier_14_claim_ceilings_match_subject_breadth` |
| The #12186 vocabulary is instantiated without a second type system | Module imports only `compiler_profile_contract` public types; all rows build through its validating constructors |

## Verification

```bash
cargo test -p xtask --locked compiler_profile_initial_rows   # 21 tests, all green
cargo fmt -p xtask -- --check                                # green
git diff --check                                             # green
```

`cargo clippy -p xtask --all-targets --locked -- -D warnings` is red on
pristine `main` for pre-existing reasons unrelated to this module (#12467
class); the new module introduces no clippy warnings of its own (checked by
filtering clippy diagnostics to `compiler_profile_initial_rows.rs`).
