# Upstream Perl runner membership parity

The target matrix defines what an upstream Perl target means. A runner plan records what one exact runner actually selected for that target.

This first runner slice proves normalized membership and preserves runner order and scheduling as separate facts. It does **not** yet capture the effective per-file invocation produced by upstream `_scan_test`; every plan and comparison therefore reports `invocation_capture: not_proven`.

## Build a runner plan

Capture raw `--dumptests` output from the exact runner separately, then build a deterministic plan:

```bash
cargo run -p perl-core-harness --bin perl-core-harness-runner-plan -- \
  build \
  .ci/perl-core-harness/upstream-targets-5.42.2.v1/ \
  component_base \
  test \
  target/perl-core/raw/base-test.txt \
  target/perl-core/runner-plans/base-test.json
```

A harness plan can retain scheduling inputs without changing denominator identity:

```bash
cargo run -p perl-core-harness --bin perl-core-harness-runner-plan -- \
  build \
  .ci/perl-core-harness/upstream-targets-5.42.2.v1/ \
  component_base \
  harness \
  target/perl-core/raw/base-harness.txt \
  target/perl-core/runner-plans/base-harness.json \
  --jobs 4 --state-ordering
```

The plan binds:

- target-matrix fingerprint and target-contract digest;
- requested runner and canonical selection entrypoint;
- raw discovery digest;
- raw-to-canonical source mapping;
- source form and path class;
- order-preserving normalized discovery;
- sorted unique membership;
- scheduling inputs;
- direct-fallback and alternate-runner limitations;
- the explicit per-file invocation claim boundary.

Local `t/` paths, root `lib`, `dist`, `ext`, and `cpan` paths remain distinct. `.t` and `test.pl` are first-class script forms. A single leading `../` from the upstream `t/` working directory is normalized into repository source identity; traversal beyond that boundary fails.

## Compare two plans

```bash
cargo run -p perl-core-harness --bin perl-core-harness-runner-plan -- \
  compare \
  target/perl-core/runner-plans/base-test.json \
  target/perl-core/runner-plans/base-harness.json \
  target/perl-core/runner-plans/base-parity.json
```

`membership_status: parity` requires exact set equality. Runner order and scheduling may differ and remain visible through `order_equal` and `scheduling_equal`. A missing or extra file produces `mismatch`; one direct-fallback input produces `not_proven` even when the visible file set happens to match.

Validate either receipt offline with:

```bash
cargo run -p perl-core-harness --bin perl-core-harness-runner-plan -- \
  check target/perl-core/runner-plans/base-parity.json
```

## Current claim boundary

This mechanism does not invoke `t/TEST`, `t/harness`, Make, or Perl. It does not replace `profile_runner_args`, publish a comparison series, move an accepted baseline, or claim effective invocation parity.

The next runner slice must capture or derive upstream `_scan_test` facts for every selected file, including:

- source/script path after runner rewriting;
- working and return directories;
- executable and include-root roles;
- `TestInit=U1`, `U2T`, `A`, and `NC` distinctions;
- shebang `-t`/`-T` and variant precedence;
- UTF/deparse source transformation identity;
- direct-fallback missing context.

Until that evidence exists, membership parity is useful but insufficient for runner-equivalence or compiler-result movement.
