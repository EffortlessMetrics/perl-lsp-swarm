# Upstream Perl runner membership parity

The target matrix defines what an upstream Perl target means. A runner plan records what one declared runner selected from one caller-declared raw-discovery byte stream for that target; it does not prove that the named runner produced those bytes.

This slice proves normalized membership and preserves runner order and **declared scheduling inputs** as separate facts. It does **not** observe the scheduling state actually used by the runner, prove which upstream runner produced a given raw-discovery byte stream, or capture the effective per-file invocation produced by upstream `_scan_test`; every plan and comparison therefore carries explicit limitations for all three boundaries. A raw-discovery stream is a **caller-declared input**: the same hand-written bytes can be declared for both runners, and the receipts never upgrade that declaration into an observation.

## Build a runner plan

Capture raw `--dumptests` output from the exact runner separately, then build a deterministic plan:

```bash
cargo run -p perl-core-harness --bin perl-core-harness-runner-plan -- \
  build \
  .ci/perl-core-harness/upstream-targets-5.42.2.v1/ \
  component_base \
  test \
  target/perl-core/raw/base-test.txt \
  target/perl-core/runner-plans/base-test.json \
  --frame canonical_repository_path
```

A harness plan can retain declared scheduling inputs without changing denominator identity:

```bash
cargo run -p perl-core-harness --bin perl-core-harness-runner-plan -- \
  build \
  .ci/perl-core-harness/upstream-targets-5.42.2.v1/ \
  component_base \
  harness \
  target/perl-core/raw/base-harness.txt \
  target/perl-core/runner-plans/base-harness.json \
  --frame canonical_repository_path --jobs 4 --state-ordering
```

Current plans are `runner_plan.v2` receipts. The raw discovery frame must be declared at build
time; use `--frame runner_t_directory_relative` for paths emitted relative to the upstream
`t/` directory, `--frame repository_root_relative` for repository-root-relative paths, or
`--frame canonical_repository_path` for already canonical repository-relative paths. The frame
is retained on every source row and participates in the plan digest. The v1 schema remains
available for archival validation, but v1 plans are not accepted as current authority.

The plan binds:

- target-matrix fingerprint and target-contract digest;
- requested runner and canonical selection entrypoint;
- exact raw-discovery digest;
- raw-to-canonical source mapping;
- source form, path class, and invocation-context class recomputed from every raw path;
- order-preserving normalized discovery;
- sorted unique membership;
- declared scheduling inputs;
- the mandatory `scheduling_inputs_are_declared_not_observed` limitation;
- the mandatory `raw_discovery_stream_is_declared_input_not_observed_runner_output` limitation;
- direct-fallback and alternate-runner limitations;
- the explicit per-file invocation claim boundary.

Local `t/` paths and root `lib`, `dist`, `ext`, and `cpan` paths remain distinct. `.t` and `test.pl` are first-class script forms. One leading `../` from the upstream `t/` working directory is normalized into repository source identity; traversal beyond that boundary fails. The filtered `core_root_lib` population does not accept the ordinary root-lib population wholesale.

## Authority-check one plan

A plan is not trusted merely because its digests look hexadecimal. Checking rebuilds it from the supplied matrix, target contract, raw discovery, and serialized scheduling declarations, then requires exact typed-state equality:

```bash
cargo run -p perl-core-harness --bin perl-core-harness-runner-plan -- \
  check-plan \
  .ci/perl-core-harness/upstream-targets-5.42.2.v1/ \
  target/perl-core/raw/base-test.txt \
  target/perl-core/runner-plans/base-test.json
```

Changing a selector, runner entrypoint, source item, source form, raw-discovery bytes, declared scheduling field, limitation, contract digest, or matrix fingerprint fails this check. This does not prove the runner actually used the declared scheduling values; that requires a runner-produced observation in a later slice.

## Compare two authoritative plans

```bash
cargo run -p perl-core-harness --bin perl-core-harness-runner-plan -- \
  compare \
  .ci/perl-core-harness/upstream-targets-5.42.2.v1/ \
  target/perl-core/runner-plans/base-test.json \
  target/perl-core/raw/base-test.txt \
  target/perl-core/runner-plans/base-harness.json \
  target/perl-core/raw/base-harness.txt \
  target/perl-core/runner-plans/base-parity.json
```

The command authority-checks both plans before comparing them. The parity receipt retains SHA-256 digests for the canonical typed content of the left and right plans plus exact digests for both raw-discovery byte streams. JSON whitespace or object-key spelling does not change a plan digest after parsing; a typed field change does.

`membership_status: parity` requires exact set equality between two **distinct non-fallback upstream runner kinds**, normally `test` and `harness`. A missing or extra file produces `mismatch`. A direct-fallback input or same-runner comparison produces `not_proven`, even when the visible file sets match. The corresponding limitation is mandatory and a forged `parity` or `mismatch` report fails validation.

Runner order and declared scheduling inputs may differ and remain visible through `order_equal` and `scheduling_equal`. The latter means only that the two serialized declarations are equal; every parity receipt must carry `scheduling_equality_compares_declared_inputs_not_observed_runner_state`.

Every parity receipt must also carry `membership_parity_compares_declared_discovery_streams_not_observed_runner_output`: a `parity` result establishes that the two caller-declared streams select the same normalized membership for one target contract, not that the named upstream runners produced or executed those memberships. A copied stream declared for both runners therefore still validates as `parity`, and stripping this limitation from any plan or report fails validation.

## Authority-check a parity receipt

```bash
cargo run -p perl-core-harness --bin perl-core-harness-runner-plan -- \
  check-parity \
  .ci/perl-core-harness/upstream-targets-5.42.2.v1/ \
  target/perl-core/runner-plans/base-test.json \
  target/perl-core/raw/base-test.txt \
  target/perl-core/runner-plans/base-harness.json \
  target/perl-core/raw/base-harness.txt \
  target/perl-core/runner-plans/base-parity.json
```

This rebuilds both plans from their authorities, recomputes the comparison, and requires the serialized parity report to equal that result exactly. A detached report cannot survive typed plan-content, discovery, order, declared scheduling, runner, or limitation changes.

## Current claim boundary

This mechanism does not invoke `t/TEST`, `t/harness`, Make, or Perl. It does not observe effective runner scheduling, prove the upstream production identity of any raw-discovery stream, replace `profile_runner_args`, publish a comparison series, move an accepted baseline, or claim effective invocation parity. Binding a discovery stream to a versioned runner-produced observation receipt (command, upstream source identity, completion status, and stdout bytes) is future work, not a property of this receipt.

The next runner slice must capture or derive upstream `_scan_test` facts for every selected file, including:

- source/script path after runner rewriting;
- working and return directories;
- executable and include-root roles;
- `TestInit=U1`, `U2T`, `A`, and `NC` distinctions;
- shebang `-t`/`-T` and variant precedence;
- UTF/deparse source transformation identity;
- direct-fallback missing context;
- runner-produced discovery observation receipts that bind a stream to the runner, entrypoint, upstream source identity, exact invocation, completion status, and stdout bytes that produced it;
- runner-produced scheduling observations where scheduling claims are needed.

Until that evidence exists, membership parity is useful but insufficient for runner equivalence or compiler-result movement.
