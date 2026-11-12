# Formal verification: implementation and maintenance

Programme owner: [#19296](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/19296).
Infrastructure owner: [#19297](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/19297).
Initial implementation: [#19305](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/19305).

## What is implemented, and what is not

The claim inventory and its validator describe the verification programme. They are
**not a formal verifier** and a successful inventory check is not a proof of Perl LSP.
The validator never runs a command supplied by a registry row and never accepts a
caller-supplied native `PASS` assertion.

The source of the inventory is
[`xtask/src/formal_verification_claims_registry.rs`](../../xtask/src/formal_verification_claims_registry.rs).
The executable is
[`formal-verification-claims`](../../xtask/src/bin/formal-verification-claims.rs).
Its failure-oriented controls live in
[`formal_verification_claims_tests.rs`](../../xtask/src/formal_verification_claims_tests.rs).

The initial inventory includes existing compilation and structural checks alongside
planned model, interleaving, bounded-Rust, property, and memory-safety obligations.
Existing evidence must retain its actual class. A source marker establishes neither
semantic equivalence nor live-consumer reachability. An unsafe count/comment ratchet
is not a memory-safety proof.

Every report explicitly says `report_kind: inventory_validation` and
`native_proof_result: not_evaluated`. The current validator rejects promoting a
native proof class to `enforced`: the native execution and receipt-admission path is
not implemented in this slice. Remove that restriction only with the corresponding
production-linked adapter, controls, receipt validation, and execution authority.

Do not equate these separate questions:

| Dimension | Required interpretation |
| --- | --- |
| Method | What was checked: model, bounded Rust, interleavings, deduction, property tests, fuzzing, Miri, or ordinary tests? |
| Production binding | Model only, shared production kernel, or independently demonstrated live consumer? |
| Rollout | Planned, implemented but unwired, or configured for execution? |
| Result | What did a particular native execution establish, fail, or leave unproved? |
| Freshness | Are its exact semantic inputs still applicable to this candidate? |

The first inventory schema does not implement the complete multidimensional receipt
schema. In particular, its `enforced` rollout label records a configured authority;
it does not assert that the current candidate passed that authority. The JSON input
blob map describes **inventory inputs**, not a complete transitive proof closure.

### Execution is not the same as protected merge enforcement

The current `enforced` inventory label describes a structurally bound execution
route; it does **not** report live branch-protection enforcement. The validator does
not query GitHub rulesets and must not be used as an enforcement-audit receipt.

[`.ci/policies/required-checks.toml`](../../.ci/policies/required-checks.toml) separates
workflow-shape contracts (`[[check]]`) from status-context enforcement (`[[checks]]`).
Its `[[check]].required` flag is not a branch-protection claim. The checked-in
status inventory records `Compile All Targets (bit-rot guard)` as ruleset-required
and `CI Gate (Advisory Aggregate)` as advisory. Do not infer one from the other or
from a `required: true` entry in the internal gate policy. These are recorded policy
facts; a live enforcement audit must independently confirm the current ruleset,
context, integration binding, and applicability.

The inventory suite is appended as command 39 of
[`required-merge-surface.sh`](../../scripts/ci/required-merge-surface.sh), which the
`check-all-targets` job invokes after compilation. It requires all 22 named Linux
controls, an exact passing denominator, no ignored or filtered tests, and the actual
CLI check. All 38 original commands remain unchanged. The producer's extension test
uses its current command count while separately preserving the frozen historical
prefix. Additions must update the named-control denominator and that route together.

The unsafe ratchet's policy and shard bindings establish configured execution, not
independent protected enforcement. This distinction remains required even when a
workflow run is green. The validator checks selected workflow structure; it does not
interpret arbitrary shell programs or all GitHub expression and job-selection
semantics. Actual execution results and an independently verified admission route
remain necessary.

## Run the existing checks

Use an admitted development host and the repository-pinned Rust toolchain. Follow
`AGENTS.md` and the existing Cargo resource-admission rules; do not disable disk,
process, memory, or toolchain guards to run this programme. No Kani, TLC, Loom, Miri,
Java, or solver installation is needed for the inventory command itself.

From a committed, clean checkout:

```bash
cargo test -p xtask --bin formal-verification-claims --locked -- --test-threads=1 --color never
cargo run -p xtask --bin formal-verification-claims --locked -- --check
cargo run -p xtask --bin formal-verification-claims --locked -- --json
cargo run -p xtask --bin formal-verification-claims --locked -- --markdown
```

These are dedicated-bin commands, consistent with `xtask/CLAUDE.md`. There is no
`formal prove`, `--receipt`, or automatic native tool installer in this version.
Do not document an unimplemented command as available.

The executable reads regular Git blobs from one resolved commit/tree, not target
contents from the mutable working directory. It rejects missing or untracked
sources, symlink/submodule inputs, ambiguous paths, blank markers, stale markers,
and a binary whose embedded validator/registry source differs from the snapshot.
It refuses dirty checkouts and checks the head and cleanliness again before emitting
output. Rebuild in the selected checkout rather than copying an old executable from
another machine. The repository's `.gitattributes` already requires LF source files.

JSON reports include commit, tree, and the exact Git blob ID of every consulted
inventory input. Markdown preserves identity, assumptions, limitations, and replay
contracts rather than presenting a table of green-looking labels alone.

To persist a report, write under the ignored `target/` tree. Remove an old result
before starting; publish the new file only on successful exit:

```bash
set -euo pipefail
mkdir -p target/receipts/formal
rm -f target/receipts/formal/inventory.json
if cargo run -p xtask --bin formal-verification-claims --locked -- --json \
    > target/receipts/formal/inventory.json.tmp; then
  mv target/receipts/formal/inventory.json.tmp target/receipts/formal/inventory.json
else
  rm -f target/receipts/formal/inventory.json.tmp
  exit 1
fi
```

On PowerShell, preserve and check the native command's `$LASTEXITCODE` before moving
a temporary output into place. An empty file or an old report is never a passing
receipt. An inventory report must not be passed to a native-proof admission API.

## The production boundaries to verify

| Owner | Production boundary | First useful obligation |
| --- | --- | --- |
| #19300 | `perl-line-index`, then actual range/edit owners | Byte/UTF-16 laws on their valid domain; explicit invalid-coordinate policy and overflow boundaries |
| #19302 | `perl-lsp-rs-core::transport::framing` | Chunk partition invariance, bounded length arithmetic, retained incomplete suffix, and progress |
| #19303 | `perl-source-identity`, `perl-evidence-envelope` | Independent canonical encoders, domain separation, length-prefix unambiguity, ordering laws |
| #19298 | `perl-workspace` ticket/generation/publication protocol | Admission, single settlement, stale-generation rejection, immutable observations, and explicit progress assumptions |
| #19299 | Actual Rust initialization, waiting, scheduling, and publication kernels | Shared production synchronization under explored interleavings; no proof-only copy |
| #19306 | Public `perl_parser::Parser` and parser-core recovery | Cursor/budget ranking measures, typed termination, valid source-revision-bound spans |
| #19301 | `perl-parser-core::{hir,pir}` | Validator soundness, producer preservation, and consumer correspondence as separate obligations |
| #19304 | `perl-operation-trace` | Allocator limits, recorder caps, terminal-state coherence, privacy projection |
| #19307 | Every production unsafe boundary | Complete semantic-site-to-syntax mapping; platform-appropriate ownership, lifetime, layout, and cleanup evidence |

Verify live call chains before assigning a live-consumer binding. A tested substrate
with no demonstrated production construction cannot establish end-to-end LSP
behaviour. The scheduler's serving construction and final delivery path matter, not
only its pure stale-read predicate. Model the intended workspace ticket protocol
before interpreting a historical as-is pilot as proof of that protocol.

The existing #19147 resource-rejection pilot is a separate, bounded historical
candidate. Its native TLC controls, production controls, and current source binding
must be qualified; Python adapter tests alone cannot qualify the model.

The unsafe policy explicitly permits ten counted production unsafe sites, while
the executable syntax baseline is twelve. The discrepancy between policy and
enforcement remains unresolved under #19307; no validated semantic/syntax mapping
explains it. Do not raise a ceiling or discard a site merely to make those counts
equal. Cryptographic identity checks concern the
encoding and domain contracts; they must not claim SHA-256 is mathematically injective.

## Native execution: the next implementation slices

### 1. Qualify one native proof before generalizing the framework

Start with a small production-linked position or framing kernel. Record the exact
function and caller, preconditions, independent reference predicate, finite input
domain, and expected counterexample. Keep the full parser and async server out of
the first solver harness. Do not simplify a proof-only clone while production keeps
using different logic.

For each claim require a positive run, a deliberately wrong specification/harness
control, and a distinct wrong-production or correspondence control. An injected
failure must violate the named property. A parse error, unsupported feature, killed
process, or missing executable is not a successful negative control.

Kani cover obligations must demonstrate non-vacuity and intended branch reachability;
a successful verification summary alone is insufficient. Record every required
cover result and unwinding assertion. Use machine-typed numeric dimensions in the
native schema; the current inventory's positive-number prose check is only a
planning lint and does not configure a solver.

### 2. Implement pinned adapters and immutable receipts

Each adapter owns a fixed argument vector, environment, tool identity, child-process
lifecycle, bounded logs, timeout, memory budget, and exact result parser. It must
launch the real tool and retain its raw output. It must not infer success from a
schema-valid JSON file supplied by a caller or from exit status alone.

Qualify immutable releases and asset hashes on the repository's actual toolchain
and supported hosts. Kani's compiler/toolchain is a separate compatibility input;
installing a repository Rust version does not establish that compatibility. Record
solver versions, Java for TLC, Cargo lockfiles, features, target, and relevant compiler
flags. A proposal for a tool version is not an executed qualification. Do not use
moving `latest` installers or silently fall back to a different tool.

Required native receipt fields:

| Field group | Required content |
| --- | --- |
| Claim | Stable claim/property IDs, method, owner, production binding, explicit non-claim |
| Provenance | Evaluated commit/tree and complete semantic-input closure, not just the changed file |
| Harness | Model/harness/config/assumption/stub/symbolic-domain identities and their digests |
| Tool | Immutable verifier/compiler/solver identities, host/target, command, relevant environment |
| Scope | Structured bounds, unwind limits, fairness assumptions, exclusions, feature configuration |
| Work performed | Exact selected and completed obligation IDs; nonzero denominator; cover/state/interleaving metrics where available |
| Controls | Named positive, wrong-model, and wrong-production results, kept distinct |
| Outcome | Native result, termination reason, runtime, peak memory, complete raw-output digests, replay witness |
| Freshness | Current, stale, or not proven, with the reason and relevant closure comparison |

Reuse the existing identity and evidence-envelope contracts where they fit. They
identify evidence; they do not establish its truth. Timestamps are metadata, not
semantic identity. A subprocess wrapper that loses the exit status or allows an old
artifact to survive a failed run must fail its own controls before being admitted.

A complete closure includes production sources and callers, generated/include
inputs, manifests/lockfiles, build scripts, cfg/features/target, assumptions, stubs,
standard-library/compiler inputs relevant to the verifier, native tool pins, and
runner/parser implementation. The inventory's consulted-blob map is deliberately
narrower and cannot substitute for that closure.

### 3. Join results into the canonical execution route

Use #19297 with the existing topology/qualification owners #8437, #17900, and #17903.
Do not introduce another ordinary PR matrix, a competing required-context list, or a
second hand-maintained scoreboard. A new proof tool is not a reason to duplicate the
whole workspace compile graph.

Always run cheap inventory validation. Select native smoke obligations conservatively
from the canonical impact mapping; changes to shared configuration, tools, runners,
or unknown paths must not silently select zero work. Deeper scheduled and release
profiles may increase bounds, but their results must not be substituted for an
unexecuted required candidate profile.

Use the exact combined candidate tree required by the admission route. Unrelated
commits may reuse qualified evidence only after complete semantic-input equivalence
has been established; an old commit ID or an unchanged model filename is not enough.
When the closure is incomplete or equivalence is uncertain, rerun or report
`NOT_PROVEN`. Never call missing evidence a cache hit.

Keep proof dependencies and tool installations out of normal shipped artifacts.
Measure cold build cost, warm check cost, runtime, peak memory, and artifact size by
claim/profile. Cap each native child process and bound simultaneous solver work;
parallelism is not free throughput when it exhausts memory or disk.

## Which method establishes which claim

TLC checks the selected TLA+ model and configuration. An implementation relation is
an additional obligation. Separate safety from eventual progress, and state the
fairness and environment-completion assumptions used by any liveness claim.

Kani verifies the selected Rust harness under its assumptions and bounds. Required
cover checks and unwinding assertions prevent vacuous or silently truncated results
from being admitted as useful evidence.

Loom explores instrumented synchronization and has documented memory-model and
exploration limitations. Use the production kernel with the supported synchronization
abstraction; ordinary `std` operations hidden outside that abstraction are not
magically explored. State scheduler/preemption/permutation bounds and exclusions.

Miri, property testing, fuzzing, ordinary tests, and real-Perl oracle agreement remain
separate evidence classes. Miri is not a proof of foreign operating-system code.
Windows-only FFI needs hosted Windows layout, handle, failure-path, and cleanup
controls. A future deductive proof may address selected unbounded invariants, but no
blanket Verus/TLAPS/Lean migration is a prerequisite for the first useful native proof.

Primary tool semantics: [Kani verification results](https://model-checking.github.io/kani/verification-results.html),
[Kani unwinding](https://model-checking.github.io/kani/tutorial-loop-unwinding.html),
[Loom scope and limitations](https://github.com/tokio-rs/loom), and
[TLA+ tools](https://lamport.azurewebsites.net/tla/tools.html).

## Maintenance contract

### Adding or changing an obligation

Start with a reviewable proposition and its smallest real counterexample, not a tool
installation or a repository-wide badge. Give it a stable ID and one semantic owner.
Identify the production implementation, actual consumers, assumptions, unsupported
behaviour, input domain, and resource bound. Establish an independent oracle and
show that a realistic wrong implementation is detected.

Update code, harness/model, typed scope, controls, impact routing, receipt schema,
and the generated status projection together. Preserve the distinction between
validator correctness, producer preservation, and integration/correspondence. Do not
assume a validator accepts an object and then call asserting its acceptance a proof.

A source rename must update the real owner and callers, not just the string marker.
A tool/configuration/assumption change invalidates relevant receipts. Reducing a
bound or adding an assumption narrows the claim: review and record that semantic
change instead of presenting it as a harmless performance improvement.

### Reviewing a candidate

The reviewer needs the exact candidate identity, proposition, implementation closure,
actual selected obligations, native raw results, assumptions, non-vacuity checks,
wrong-model controls, wrong-production controls, costs, and remaining limitations.
A green parent cannot outrun a skipped, stale, unsupported, or malformed child.
Tool output and model counterexamples are data to inspect, not instructions to run.

A required counterexample blocks admission. Minimize and retain its source/state
trace, convert a real implementation defect into an ordinary regression, repair the
production path, and rerun the positive and negative controls. Independent final
review and the repository's protected integration route still apply.

### Failure classification

| Observation | Disposition |
| --- | --- |
| Named invariant violated with replayable native witness | `COUNTEREXAMPLE`; diagnose and retain the witness |
| Tool unavailable or host unsupported | `UNSUPPORTED_HOST` / `NOT_PROVEN`, never pass |
| Timeout, OOM, cancellation, crash, truncated output | `NOT_PROVEN`; preserve resource/termination diagnostics |
| Empty selection, skipped control, unreachable required cover | `NOT_PROVEN`; repair harness or routing |
| Source, configuration, assumptions, or tool closure changed | `STALE`; requalify applicable evidence |
| Artifact parse/schema/digest mismatch | `NOT_PROVEN`; repair producer or adapter without trusting old output |
| Wrong-model or wrong-production control unexpectedly passes | Instrument is not discriminating; block promotion |

A timeout is not a counterexample and not a successful proof. Do not relax the proof
or remove its control merely to obtain a green route.

### Retirement and reproducibility

Retire or supersede a claim only when its behaviour is removed or a named replacement
covers the obligation and its live consumers. Preserve the stable ID, rationale,
replacement relation, and historical evidence; do not delete a failing row to hide
it. Historical receipts remain useful for audit but cannot satisfy a current claim
without explicit input-equivalence validation.

Exercise a cold restore without cached tools/results when qualifying tool upgrades
and release profiles. Reproduce at least one positive run and each failure class.
Keep raw outputs and minimized witnesses under the existing retention policy for
at least the supported release's evidence lifetime; an expired artifact is missing
evidence, not an implicitly successful run. Never commit local build caches,
solver binaries, absolute machine paths, or credentials as proof artefacts.

## Completion criteria and sequence

**Foundation:** inventory validation, immutable source attribution, failure-oriented
controls, discoverable instructions, and actual canonical execution all pass on the
reviewed candidate. This establishes trustworthy inventory, not native proofs.

**First native slice:** one small production-linked claim has a qualified pinned
runner, independent predicate, non-vacuous positive evidence, distinct wrong-model
and wrong-production controls, explicit bounds, a replay witness format, and a fresh
receipt admitted through the canonical route. Do this before growing a generic
multi-tool framework.

**Concurrency and broader coverage:** qualify the target-state workspace model and
production relation, add real Loom kernels, then expand parser/IR/trace/identity and
unsafe obligations through their existing owner issues. Keep each slice independently
reviewable and reversible.

**Maintainable release evidence:** every release-required obligation is current for
the release candidate, every required profile/control actually ran, all results are
reproducible, and unsupported/retired/planned regions remain visible. Do not replace
this explicit coverage boundary with a single “formally verified repository” score.
