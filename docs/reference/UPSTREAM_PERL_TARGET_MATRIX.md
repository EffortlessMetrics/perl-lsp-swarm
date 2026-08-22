# Upstream Perl target matrix

The compiler harness treats an upstream test target as a versioned selection and invocation contract, not as a display name or a convenient list of directories.

The pinned Perl 5.42.2 authority is stored under:

```text
.ci/perl-core-harness/upstream-targets-5.42.2.v1/
```

`index.json` records the Perl commit, the claimed `Makefile.SH`, `t/TEST`, and `t/harness` blob identities, and the ordered target-part files. The validator assembles those parts into one canonical typed matrix before fingerprinting it. File partitioning is review structure, not denominator identity. This offline contract does not resolve or hash an upstream checkout, so the recorded source identities and target membership remain claims awaiting an independently generated source-bound receipt.

## Validate the authority

```bash
cargo run -p perl-core-harness --bin perl-core-harness-targets -- \
  check \
  .ci/perl-core-harness/upstream-targets-5.42.2.v1/ \
  .ci/perl-core-harness/upstream-targets-blead-drift.v1.json
```

The checked-in `blead` receipt is deliberately `not_proven`. It records an exact commit and exact topology-source blobs, but it does not claim target additions, removals, changes, or parity with Perl 5.42.2. Those conclusions require a separately generated observed matrix:

```bash
cargo run -p perl-core-harness --bin perl-core-harness-targets -- \
  check <pinned-matrix> <drift-receipt> <observed-matrix>
```

For `status: compared`, the validator binds the observed matrix fingerprint, Perl ref, resolved commit, and source blobs, then recomputes `added_target_ids`, `removed_target_ids`, and `changed_target_ids`. A source-identity receipt alone cannot assert an empty diff.

## Contract rules

The validator fails closed on:

- unknown fields in target, selector, preparation, exclusion, matrix, and drift payloads;
- malformed local or root-external selectors;
- target IDs, upstream names, or aliases owned by more than one target;
- missing, self-referential, incompatible, or cyclic variant bases;
- instrumentation chains that do not resolve, through environment-variant lineage, to a physical or selector denominator;
- missing, self-referential, or cyclic replacement lineage;
- replacement lineage without a nonempty reviewed reason;
- generated composites without an explicit overlap policy;
- duplicate or unsorted target identities;
- malformed, duplicate, unsorted, or internally inconsistent target topology;
- deletion or substitution of a matrix part without updating the bundle's file set.

Runner-switch order is part of target identity rather than a direct rejection rule.
A reordered switch list changes the target digest and matrix fingerprint, so the
ratcheted fingerprint assertion rejects the change.

Each physical or selector target records two authorities separately. `authority` names the requested entry point, such as a Make target. `selection_authority` names the scheduler or reviewed selector that actually defines membership, such as `t/TEST` or `t/harness`. Environment variants inherit the underlying selection authority unless they explicitly change it.

## Target classes

- **Physical series** own immutable source membership, such as `t/base`, `t/mro`, `test_reonly`, or one MANIFEST population.
- **Selector variants** change membership through upstream authority, such as the actual `t/TEST --core` selection. Its `core_root_lib` population is not ordinary root `lib`.
- **Environment variants** inherit membership while changing source interpretation, terminal policy, switches, parameters, or environment.
- **Generated composites** join independently identified targets. The historical repository core and full views declare `reject_overlap`; the current offline validator records that policy and validates membership references but does not expand selector populations to compute overlap. They are split by runner: `t/harness` admits direct `op/*.t` only, while `t/TEST` recursively reaches the separate nested `op/hook` member.
- **Preparation-only targets** describe build state without creating a compiler denominator.
- **Instrumentation-only targets** add process instrumentation without raising compatibility.

The four historical repository rows—core and full through `HarnessRunner::Harness`, and core and full through `HarnessRunner::Test`—remain visible so neither runner-dependent denominator can be confused with upstream `t/TEST --core` or Perl's default test target. The old runner-agnostic `legacy_custom_core` and `legacy_custom_full` identities are intentionally absent because no single membership set represents both execution paths.

Presentation fields such as `display_name` remain part of the matrix artifact and full matrix fingerprint, but they are excluded from per-target topology classification. A label edit does not become topology drift; a selector, preparation, variant, switch, environment, exclusion, or other invocation-contract edit still does.

## Claim boundary

A matrix row establishes recorded topology, identity, ownership, and selection intent only. It is not independently verified upstream membership, parse, compile, semantic, execution, platform, or performance evidence.

The current artifact deliberately does not claim independent upstream-source authority: no
resolver or source checkout is available in this contract's allowed offline surface. A
future source-bound receipt must bind the exact upstream object contents to the generated
membership before the matrix can claim upstream membership authority.

A target becomes compatibility authority only after exact membership is frozen into a comparison series, its evidence bundle is complete, and every failure or accepted semantic boundary is typed and governed. Missing capability, preparation, generated input, native extension, process, or environment state remains separate from product compiler failure.
