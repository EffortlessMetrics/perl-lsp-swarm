# Postfix statement-modifier conformance

The canonical Perl compatibility policy currently covers Perl 5.8 through 5.40.
`.github/workflows/perl-version-matrix.yml` runs the same postfix fixture once for
each version in that policy envelope.

The fixture exercises the six supported statement-modifier spellings (`if`,
`unless`, `while`, `until`, `for`, and `foreach`) at the runtime boundary. It also
checks loop-state progression, `$_` aliasing for postfix `for`, and iteration
cardinality. The runner records the exact Perl version and SHA-256 of the fixture;
the expected output is a deterministic oracle for this bounded behavior.

This proves version-invariant behavior across the admitted 5.8–5.40 matrix. It does
not claim coverage for Perl releases outside the repository's current support policy,
nor does it promote parser/compiler support from runtime execution alone. Missing or
unavailable matrix legs remain uncovered rather than passing by default.

The runner's truncated-output control ensures that a partial result cannot be treated
as a passing conformance observation.
