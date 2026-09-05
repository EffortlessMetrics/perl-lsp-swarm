# Perl core harness baselines

This directory stores checked-in Perl core harness baselines.

Initial scaffold:

```bash
cargo xtask perl-core-harness discover \
  --perl-tree /path/to/prepared/perl5 \
  --host-perl perl \
  --profile base
```

Parse/compile-mode reports are written to:

```text
target/perl-core/reports/<profile>-parse.json
target/perl-core/reports/<profile>-compile.json
```

Selected execute-mode reports are written to:

```text
target/perl-core/reports/base-execute.json
```

The pinned advisory upstream smoke config is:

```text
.ci/perl-core-harness/upstream.toml
```

Preparation receipts are written to:

```text
target/perl-core/prepare/<ref>/prepare.json
```

Real-tree smoke receipts are written to:

```text
target/perl-core/smoke/<profile>/discovery.json
target/perl-core/smoke/<profile>/parse.json
target/perl-core/smoke/<profile>/compile.json
target/perl-core/smoke/<profile>/gap-map.json
target/perl-core/smoke/<profile>/smoke.json
```

Run the advisory integrated base lane against the pinned upstream ref:

```bash
just perl-core-integrated-base
```

Run the advisory integrated comp lane against the pinned upstream ref:

```bash
just perl-core-integrated-comp
```

Run the advisory integrated run lane against the pinned upstream ref:

```bash
just perl-core-integrated-run
```

Check the advisory real-upstream compile ratchets after smoke receipts exist:

```bash
just perl-core-upstream-compile-ratchet
```

Check the selected execute-base ratchet after the explicit execute receipt
exists:

```bash
just perl-core-execute-base-ratchet
```

The scheduled/manual `Perl Core Harness` workflow prepares the pinned upstream
tree, emits advisory `base`, `comp`, and `run` smoke receipts under
`target/perl-core/smoke/<profile>/`, and checks the real-upstream compile
reports against the checked-in upstream baselines.

## Semantic-boundary inventory

Each parse or compile report carries a `semantic_boundaries` array. It is the
human-readable audit trail for non-static compiler behavior; a passing compile
receipt may still contain a governed boundary, but never silently treats it as
an ordinary static fact.

Every entry records the normalized test path, stable boundary ID, source byte
span and kind, disposition, reason, confidence, whether it blocks compilation
or downstream static facts, lock scope, owning workstream, and its supporting
test path. The available dispositions are:

```text
implemented_static
statically_classified
ordinary_runtime
deferred_runtime
deferred_lifecycle
governed_compile_time_dynamic
source_locked_compatibility
unsupported
```

`unsupported` is compile-blocking. Unknown classification is rejected and is
not an admissible receipt disposition. `source_locked_compatibility`
must retain a `path_and_source` lock scope so upstream drift forces review;
those records are compatibility debt, not a claim of general Perl semantics.
Smoke and baseline validation also reject malformed boundary records (missing
ownership/proof fields, reversed spans, unknown dispositions, or inconsistent
source-lock confidence/blocking flags).

Compile-baseline v2 treats its inventory as accepted authority: `unknown`,
`unsupported`, and compile-blocking boundaries cannot be persisted there. A
boundary retirement uses `perl_core_harness.boundary_retirement.v1` and must
bind the prior series ID and manifest hash, the replacement measurement SHA,
the replacement report digest, a transition ID, an owning issue, and a
content-addressed evidence-bundle reference. A stale or still-present
retirement is rejected instead of being counted as debt retirement.

The durable ownership mechanism is the versioned
`.ci/perl-core-harness/semantic-boundary-registry.v1.json` registry. The
registry validator is intentionally offline and can check one or more accepted
baseline-v2 receipts plus their durable #5171 bundle indexes:

```bash
cargo xtask perl-core-harness boundaries \
  --registry .ci/perl-core-harness/semantic-boundary-registry.v1.json \
  --baseline path/to/compile-baseline-v2.json \
  --bundle path/to/evidence-bundle/index.json \
  --check --report
```

The empty checked-in registry is a mechanism scaffold. #4753 populates it from
the first accepted selected inventory. Until then, no current baseline is
claimed as governed debt merely because its boundaries are not yet registered.

## Failure-cluster triage

The offline triage command consumes one complete, published evidence bundle and
its compile report. It writes deterministic JSON and Markdown work-cluster
reports while keeping semantic-boundary debt candidates separate from product
failures:

```bash
cargo xtask perl-core-harness triage --bundle path/to/evidence-bundle/index.json --output target/perl-core/triage/base-compile
```

Cluster IDs are derived from the series-bound, typed failure signature rather
than file counts, paths outside the selected manifest, timestamps, or free-form
diagnostic prose. Unknown failure buckets fail closed. The current v1 report
uses the typed bucket, workstream, LSP-impact, stage, and profile/mode fields
available in the run receipt; richer parser/HIR/effect fields can be added only
through an explicit receipt-schema extension. Each cluster carries a direct
reproduction descriptor and requires exact-series proof before a claimed
resolution, baseline movement, or boundary retirement is accepted.

## Failure-cluster history

Persistent history is separate from single-bundle clustering. It records first
and last authoritative bundles, historical membership, ownership state, stage,
and explicit transition proof. A missing cluster remains active or unassigned;
absence alone never marks it resolved.

Write or update history from one validated bundle:

```bash
cargo xtask perl-core-harness triage --bundle path/to/evidence-bundle/index.json --output target/perl-core/triage/base-compile --history .ci/perl-core-harness/failure-cluster-history.v1.json --write-history
```

Check history without mutation:

```bash
cargo xtask perl-core-harness triage --bundle path/to/evidence-bundle/index.json --output target/perl-core/triage/base-compile --history .ci/perl-core-harness/failure-cluster-history.v1.json --check-history
```

History accepts an explicit `unassigned` state while work is being routed.
`resolved` requires an owner, resolution PR, resolution bundle, and a
versioned before/after transition; `accepted_debt` requires a boundary-registry
reference. These checks are offline and do not query or mutate GitHub.

## Typed compiler compatibility inputs

perl_core_harness.compiler_compatibility.v1 is the typed input state for
generated compatibility views. The loader consumes one input record per
independent series: its immutable series manifest, parse and compile reports,
compile baseline-v2, and complete evidence-bundle index. It rejects subject,
Perl SHA, runner, profile, denominator, membership, baseline, or bundle
identity drift before producing state.

The loader keeps parse, compile, boundary debt, active clusters, execution,
curated gold, differential oracle, and EIR as separate rails. Missing optional
rails are not_available, never zero or pass. The versioned schema is
schemas/compiler_compatibility.v1.schema.json; rendering and freshness
checks belong to #4749.

An execution-like rail that offers evidence also names the mechanism that
produced it, and a rail without evidence names none. The selected executor
recognizes fixture shapes and generates the behavior it reports, so its rail
reads `mechanism: fixture_replay` — read that field rather than the `reason`
prose, which carries no contract. Each execution-like rail admits only its own
mechanism, so replay evidence cannot reach the EIR or differential-oracle rail
by relabeling, and neither rail can become available until the evidence behind
it lands (#8254).

Or run the smoke against a user-supplied prepared upstream Perl tree:

```bash
cargo xtask perl-core-harness smoke \
  --perl-tree /path/to/prepared/perl5 \
  --host-perl perl \
  --profile base \
  --modes parse,compile
```

The first checked-in ratchet is:

```text
base-compile-baseline.json
```

It covers the generated two-file base compile fixture only. It does not claim a
real upstream Perl baseline or runtime execution. The real-upstream advisory
compile ratchets are separate:

```text
upstream-base-compile-baseline.json
upstream-comp-compile-baseline.json
upstream-run-compile-baseline.json
```

The selected execute-base ratchet is:

```text
base-execute-baseline.json
```

It covers only the explicit selected `base/if.t`, `base/cond.t`, `base/num.t`,
`base/pat.t`, `base/translate.t`, and `base/while.t` execute receipt. It does
not claim profile-wide execute, execute-base conformance, or a broad runtime
model.

Update any baseline explicitly with `perl-core-harness baseline --accept` after
reviewing an intentional change. The real-tree smoke and ratchets are
manual/advisory and produce receipts plus a gap map only; selected execute
ratchets run only allowlisted Perl programs and still do not claim runtime
conformance or promote a PR gate.
