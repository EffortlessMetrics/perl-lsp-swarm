# Context: #12330 — checked initial row IDs and owner map for the four compiler profiles

Parent/controller: #12176. Train row: COMP-PROFILE-C02. Depends on: #12186
(landed vocabulary, PR #12427). Coordinates with: #12187 (manifest
serialization) / #12177 (evidence/evaluation train). Bounded selected
observation: #12291 / #12139 / #12140 / #12141. Full integrated publication
successor: #8722 (not a prerequisite).

The #12186 model landed with shape fixtures that prove representability only.
What was missing — and what this claim adds — is the checked initial row
inventory itself: one stable row ID, exact proposition (subject selector),
canonical product/evidence owner, receipt family/stage/proof axes,
currentness/completeness/work law, limitation/legacy-exit policy, and maximum
claim wording for every row of the four initial profiles. Without this
inventory the #12187 manifest tooling would have no checked content to
serialize, and the owner map researched under #12176 would have no
mechanically reviewable home.

This claim defines, in `xtask/src/compiler_profile_initial_rows.rs`:

1. The 22 own rows of `compiler_local_lexical.v1` (no imports): candidate
   identity, the six bounded #12291/#12139–#12141 parse+compile observations,
   invocation validity, accepted debt/parser/semantic state, PIR lexical
   contribution, compiler-backed references and occurrence denominator,
   rename authorization and independent edit application, the zero-work
   propositions, lifecycle, mutation execution, exact perllsp process, and
   the local claim ceiling.
2. The 19 own rows of `compiler_static_project.v1` (imports local verbatim):
   exact import identity, world snapshot/closure/graph/SCC/transitions,
   invalidation and stale-publication rejection, multi-root currentness,
   cross-file definition/references/rename/edit, representative lifecycle,
   cold equivalence, reuse/recompute work, performance envelope, claim
   ceiling.
3. The 19 own rows of `compiler_bounded_execution.v1` (imports project
   verbatim): executable identity, unsupported-fact catalog, PackageSubTable
   denominator, EIR lowering/verification/evaluation, hard limits, curated
   gold, hermetic oracle, both agreement rows, upstream row denominator,
   nonzero EIR/TAP work, zero legacy scaffold calls, the editor static
   boundary rows (`no project execution`, `editor_runtime_dependency=false`),
   explicit dynamic boundaries, claim ceiling.
4. The 19 own rows of `compiler_maintained_code_intelligence.v1` (imports
   execution verbatim): exact lower imports, upstream series denominator,
   selected provider/refactor rows, packaged identity/cells, manifest-selected
   client identity, actual-client launch/cells/lifecycle, work envelope and
   thresholds, target/route/nonzero work, legacy replacement and old-path
   proof, allowed limitations, machine/public claim ceiling,
   `support/release authority=false`, and the explicitly **unsupported**
   `intelligence.integrated-publication-8722` row — a closed typed state,
   never an omission and never a prerequisite to bounded rows.

Row counts accumulate through verbatim import preservation: 22 / 41 / 60 /
79 rows per profile.

## Placement decision

The inventory lands in `xtask` (`xtask/src/compiler_profile_initial_rows.rs`),
exposed as library API through `xtask/src/lib.rs`, beside the #12186 model —
the placement precedent set by that issue and required by this issue's
verification line (`cargo test -p xtask --locked compiler_profile_initial_rows`).
It instantiates the landed vocabulary through its public constructors only;
no second type system, no manifest/file syntax, no serde, no CLI, no receipt
adaptation, no evaluation, and no product behavior are added (issue
non-goals). #12187 consumes `initial_profiles()` without transcription.

## Owner map

Owner strings instantiate the canonical #12330 owner map
(`#12291/#12139–#12141`, `#8722`, `#12117–#12120/#12165`,
`#11665–#11670/#5214/#12109–#12111/#12191/#2660`, `#8669/#12156/#12157/#12079`,
`#4772/#4746/#2425/#2493/#8797/#5241/#8820`, `#6232/#7430/#9370`,
`#4770/#4773/#4775/#4777/#4779/#2447`, `#4760–#4767`, `#7422`,
`#6720/#6744/#7133/#6056`, `#4346/#6739/#7122`, `#9311/#9316/#9321`,
`#12125–#12129`, `#12176`). They are navigation/ownership identifiers only —
evidence authority stays with the closed typed dimensions; test
`falsifier_13_workflow_state_is_never_evidence` pins the separation.
