# Synthetic qualification packet controls

This prototype publishes the discovery, regression tests and reviewed consistency
repair for [#17668](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/17668).
It runs only on explicitly synthetic requested cells. Every result retains
`production_qualification: NOT_PROVEN`, including `SYNTHETIC_PASS`.

The isolated directory has no workflow caller, admission authority, scheduler,
journal integration, cleanup authority or production gate. Existing producer and
campaign files are unchanged. The #17623 owner retains real campaign planning and
integration; #17479 and #17573 holds and the paused experiments remain in force.
The issue remains open because its production consumer and campaign join are absent.

## Discovery and repair

The initial prototype treated aggregate successful report fields as sufficient
while contradictory ordinary `outcomes` or `footer_counts` remained unchecked.
Starting with two `same` registrations and one ignored registration, substituting
`["same", "FAILED"]` in an outcome, or a footer reporting one failed case, still
produced synthetic success and 20% sampled savings. This was a private prototype
consumer defect, not an established historical production false promotion or a
bug in the unchanged reporter's diagnostic publication.

The repaired consumer reconciles case/ignored/failed multisets against outcomes
and strictly typed footer counts. It also checks canonical unit identities and
package counts, terminal sampler summaries, numeric types, and complete bindings.
Matching counts alone cannot prove equivalence. Missing/null retained attribution
is allowed; malformed present attribution refuses. Nonfinite JSON refuses even
after consistent rehashing. A valid outer closure cannot invent a missing inner
receipt. Missing source/attempt attestation remains NOT_PROVEN.

Execution, equivalence, measurement, reserve, timing, closure, component evidence
and savings retain separate verdicts. Missing evidence is NOT_PROVEN, invalid
evidence is REFUSED, and observed nonzero product status is FAILED; admission
status 75 is REFUSED. A partial ordinary reference cannot become a savings
denominator, while the candidate's independent observations remain inspectable.
A negative `sampled_peak_fraction` is a valid comparative observation: synthetic
completeness validates the requested evidence and does not promise improvement.

## Pinned producer contract

`producer_contract.json` records the exact existing producer:

- Head: `540201d5dad7b7622628c61e463e4a366154b0b4`.
- Path: `scripts/ci/report_routed_storage_qualification.py`.
- Git blob: `c91dec382450e74bc431b952007a32e85ebde9e1`.

The optional source inspection test reads this immutable Git object, verifies
its blob identity and inspects the AST without importing or executing it. It
skips explicitly if the object is absent from a shallow checkout. Fetching that
exact ref makes the inspection available; the other controls need no network:

```sh
git fetch --no-tags origin 540201d5dad7b7622628c61e463e4a366154b0b4
```

The producer always reports qualification NOT_PROVEN because it lacks
`candidate_source_attempt_attestation`. It exports canonical unit hashes but
does not export the unit projection. Cases/ignored lists retain multiplicity;
ordinary rows also export outcomes, failed cases, footer counts/status and
consistency. The fixture faithfully includes those ordinary fields. Storage
passes through the sampler's final summary; the actual metric is
`sampled_peak_allocated_bytes`, with `peak_is_bound: false`. Observations are not
continuous reserve guarantees or allocation upper bounds. Diagnostic exit 0 is
independent of qualification. None of these facts supplies execution authority.

## Compiler-free reproduction

Python's standard library is sufficient; controls were executed on Python 3.12.14.
From the repository root:

```sh
python3 -B -m unittest discover -s scripts/ci/qualification_packet_prototype -p 'test_evaluator.py' -v
python3 -B scripts/ci/qualification_packet_prototype/prove_discrimination.py
python3 -B scripts/ci/qualification_packet_prototype/fixtures.py > /tmp/qualification-synthetic.json
python3 -B scripts/ci/qualification_packet_prototype/evaluator.py /tmp/qualification-synthetic.json --check
```

The suite has 32 test groups. All pass, including both-route cross-plane
matrices, a legitimate zero-case harness and signed negative savings. Execution
requires object rows with typed case/ignored lists; empty lists are permitted.
An execution failure downgrades only equivalence PASS and preserves established
REFUSED or NOT_PROVEN evidence. The four new groups produced 17 assertion
failures and zero errors on the previous revision. The earlier empty-map
regression groups remain covered, including missing maps on both routes and
inspectable independent storage. Five earlier groups produced 30 assertion
failures, zero instrument errors,
against the earlier prototype. The public `prove_discrimination.py` command
executes a controlled always-pass consumer that ignores its input against the same oracle;
it returns success only when the positive fixture passes and the mutant produces
30 assertion failures with zero errors. This is discriminating proof, not a
second evaluator, replay of the original consumer implementation or execution of the pinned producer.

Remove a requested packet or attestation from a fixture to observe NOT_PROVEN.
The renderer still exits 0; `--check` exits 1. Missing/corrupt files also fail check
mode. No Cargo, compiler, benchmark, full campaign or hosted CI was used as local
proof, and no test command here schedules native work.

## Synthetic ceiling and integration boundary

The cold/warm/producer-edit/production-edit/edit-revert cells, source subjects,
attestation, toolchain, configuration and resource facts are invented controls.
This is not the unimplemented production #17655 manifest. Cancellation exists
only as mutated fixture data. Configuration validation deliberately models
synthetic test-profile units with matching feature lists, not Cargo's per-package
feature resolution. Owner fields model the consumed identity subset; the full
production descriptor's scope/argv provenance is not reconstructed here.

The declared route clock must match producer command time and meet the 1500-second
watchdog. A separate mandatory-work clock includes bootstrap/setup and must meet
1320 seconds. A 1400-second route with matching route evidence and 1200 seconds
of declared mandatory work passes this prototype; mandatory work at 1400 fails.
Actual production policy applicability of that clock split is NOT_PROVEN. A
self-consistent fabricated envelope remains synthetic evidence.

Production integration requires authoritative requested cells, source/run/attempt/
artifact attestation, applicable unit projection and policy-clock receipts, real
inner/outer receipts and whole-envelope measurements through packaging. These
must come through the existing #17623/#17654/#17655/#17666 owners and retained-packet
join. The synthetic entry point adds no production packet authority, protected check,
+retry, gate activation or cleanup path. Changelog, product migration and release changes are
not applicable to this isolated developer proof.

## Retained real-packet diagnostic extension (#17897)

`retained_packet.py` now consumes delivered campaign archives and saved API
records, using shared report checks in this evaluator. See
[the field mapping and remaining authorities](REAL_PACKET_MAPPING.md).
The synthetic entry point and its ceiling above are unchanged. The real adapter
also remains diagnostic: byte consistency, original outcomes and independent
closure are inspectable, but production qualification requires the missing
execution-time source/attempt, requested-cell, full accounting and clock joins.
No production gate, workflow or admission behavior is activated by this addition.
