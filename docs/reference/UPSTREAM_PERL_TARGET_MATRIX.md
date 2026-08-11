# Upstream Perl target matrix

The compiler harness treats an upstream test target as a versioned selection and invocation contract, not as a display name or a convenient list of directories.

The pinned Perl 5.42.2 authority is stored under:

```text
.ci/perl-core-harness/upstream-targets-5.42.2.v1/
```

`index.json` binds the Perl commit, the exact `Makefile.SH`, `t/TEST`, and `t/harness` blobs, and the ordered target-part files. The validator assembles those parts into one canonical typed matrix before fingerprinting it. File partitioning is review structure, not denominator identity.

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
- missing, self-referential, incompatible, or cyclic variant bases;
- instrumentation chains that do not resolve directly to a physical or selector denominator;
- missing, self-referential, or cyclic replacement lineage;
- replacement lineage without a nonempty reviewed reason;
- generated composites without an explicit overlap policy;
- duplicate or unsorted target identities;
- changes to ordered runner switches;
- deletion or substitution of any pinned Perl 5.42.2 target or topology-source identity.

Each physical or selector target records two authorities separately. `authority` names the requested entry point, such as a Make target. `selection_authority` names the scheduler or reviewed selector that actually defines membership, such as `t/TEST` or `t/harness`. Environment variants inherit the underlying selection authority unless they explicitly change it.

## Target classes

- **Physical series** own immutable source membership, such as `t/base`, `t/mro`, `test_reonly`, or one MANIFEST population.
- **Selector variants** change membership through upstream authority, such as the actual `t/TEST --core` selection. Its `core_root_lib` population is not ordinary root `lib`.
- **Environment variants** inherit membership while changing source interpretation, terminal policy, switches, parameters, or environment.
- **Generated composites** join independently identified targets. The historical repository core and full views require `reject_overlap`; direct `op/*.t` and nested `op/hook` are separate, disjoint members.
- **Preparation-only targets** describe build state without creating a compiler denominator.
- **Instrumentation-only targets** add process instrumentation without raising compatibility.

The historical `HarnessProfile::Core` and `HarnessProfile::Full` rows remain visible so they cannot be confused with upstream `t/TEST --core` or Perl's default test target.

## Claim boundary

A matrix row establishes topology, identity, ownership, and selection intent only. It is not parse, compile, semantic, execution, platform, or performance evidence.

A target becomes compatibility authority only after exact membership is frozen into a comparison series, its evidence bundle is complete, and every failure or accepted semantic boundary is typed and governed. Missing capability, preparation, generated input, native extension, process, or environment state remains separate from product compiler failure.
