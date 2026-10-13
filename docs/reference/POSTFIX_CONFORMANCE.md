# Postfix statement-modifier conformance

The existing [Perl version workflow](../../.github/workflows/perl-version-matrix.yml)
selects the runtime versions for this probe. It currently includes selectors from
5.8 through 5.42; the workflow remains the source for that list. On pull requests,
the matrix runs only with `ci:perl-matrix`; scheduled and manual runs also exercise
it. Adding the probe does not make the matrix a required merge check.

The fixture exercises the six supported statement-modifier spellings (`if`,
`unless`, `while`, `until`, `for`, and `foreach`) at the runtime boundary. It also
checks loop-state progression, `$_` aliasing for postfix `for`, and iteration
cardinality. `bash scripts/ci/check-postfix-conformance.sh` prints the selected
interpreter's `perl -V` build/platform configuration, runtime version and fixture
SHA-256, validates syntax, and compares output with the expected event sequence.
These job logs bind the observation to the runtime and fixture actually executed.

A configured matrix is not evidence that its legs executed. Conformance is established
only for successful executions identified by those logs; missing, skipped, cancelled
or unavailable legs remain `not_run`/`unknown`. A pass does not establish universal
version invariance, parser/compiler support, diagnostic behavior, or label and
control-transfer semantics.

The runner tests its output comparator with both the actual result and a deliberately
truncated result missing the final `foreach` event. Both use the same predicate, so
an accept-all comparator fails the control instead of reporting conformance.

This is a bounded fixture contribution to [#13273](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13273),
not closure of its runtime/capability envelope. Canonical support-policy reconciliation,
version-specific compiler/provider behavior, and capability admission based on oracle
evidence remain outside this fixture. No support promotion follows from a local run.
