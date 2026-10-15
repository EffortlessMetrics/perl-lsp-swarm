# Proposed canonical Perl Rust Small transport

Status: consumer source wiring, awaiting live allowance and qualification approval.
The matching central lane is published at public PR #22 merge
`752fad709cf3da63a69d004af2867b0e77ad0af2`, exact Rust blob
`a01c520d215445bcf293beb18a4ee4ca98df6409`. The candidate caller and product
export are wired to that source; native admission remains dormant. No dispatch
or protection transition is established by this document. Earlier source
inventory/proposal below is retained as design history, superseded by this
published implementation where specified.

## Existing owners and non-duplication

On 2026-10-14 the central connector snapshot showed main at
`e692a6d5c69c618ca247a4dea835163fa2d6782a` with Rust workflow blob
`02ed597cd1a3411cec1c5b9cda27a00601a624e1`. Public
`34257023c808fd69854dd1cdc27be071901ccb05` has that same workflow blob and
source marker `5a154b85bddc183e1563cf9697cf98a0b264d5e2`. Its closed artifact
classifier supports only `ripr` and `failure-diagnostics`. The prepared central
branch `task/ripr-product-transport` at
`feda40f4220ff2b3df764dcc706170ad33b533d5` adds `ripr-product`, not Perl.
No open central PR or named branch in that snapshot provided a Perl lane.
The central task retains exclusive ownership of central repository edits.

Current `xtask rust-small-proof` produces **one canonical product file**:
`target/receipts/rust-small-proof.json`, schema `rust_small_proof.v1`.
It contains ten ordered step records (seven Cargo steps, scorecard census,
scorecard replay and diff hygiene). File count and step count are distinct.
The ten-file analysis portfolio in the existing RIPR lane is not this receipt.
Do not pad, relabel or synthesize ten files to satisfy that unrelated portfolio.
If the parent requires an additional ten-file Perl analysis portfolio, its real
producer/path/outcome contract must be supplied separately; it is not emitted
by Rust Small and cannot be established by this minimal transport.

## Fixed closed lane and members

Add `artifact_lane: perl-rust-small`, default off, only for
`EffortlessMetrics/perl-lsp-swarm`, standard profile, nonempty result_script and
upload_artifacts=true. Retain existing lane defaults, RIPR and diagnostics
contracts unchanged. Reject unknown lanes, unsupported repository/profile,
missing policy and inconsistent upload selection before checkout/product proof.
Do not require native-review-v1 for an authorized exact-SHA manual experiment.
Selecting native-review-v1 still uses its separate current closed admission rules.

Central prepare creates a new private directory beneath RUNNER_TEMP and exports
its absolute path as EM_CI_ARTIFACT_DIR. It starts empty, outside the checkout,
without links or pre-existing custody. It is not an arbitrary caller path.
The inactive consumer helper `scripts/ci/export_rust_small_receipt.py` copies:

| Member | Owner and meaning | Maximum bytes |
|---|---|---:|
| rust-small-proof.json | Exact canonical Rust receipt bytes; Rust owns semantics | 1,048,576 |
| perl-rust-small-export.json | Consumer copy digest/current context binding, not proof authority | 16,384 |
| em-ci-job-context.json | Reserved central finalized producer context; consumer cannot replace it | 16,384 |

Require exactly these three regular single-link members on successful proof;
expanded/compressed archive bounds are 2 MiB and three members, with no directories,
extra files, binaries, caches, logs, environment dumps or secrets. The context is
added/replaced only by immutable central packaging after proof finalization.
Reject consumer-supplied reserved context rather than trusting it.

The export manifest schema is `perl-rust-small-export.v1`: repository, SHA,
run_id, run_attempt, profile=standard, proof_started_at, result, and one member
{path, bytes, sha256}. These identifiers are strings consistent with native
central context. No caller approval Boolean or fixture verdict is proof.

## Source/run binding and semantic ownership

Rust captures a clean Git SHA, observed Rust/Cargo versions and canonical
profile/features/locked argv, rechecks subject stability, self-verifies its
success receipt, then writes it. Preserve this real proof and failure behavior.
It does not contain native run identity; a same-head receipt from an older
attempt must not be accepted based on SHA alone.

The exporter requires current central em-ci-job-context.v1 repository/SHA/run/
attempt/profile and proof_started_at; canonical clean subject/SHA and schema;
regular no-link source; bounded exact bytes; receipt mtime at or after current
proof entry; and an empty central-created destination. It never runs product
tests, fabricates missing step records, converts failure to success or replaces
the Rust semantic verifier. Its checks assume the governed filesystem contract,
not hostile concurrent path replacement. Central packaging/import must use
private no-follow descriptor-based custody independently.

Before selecting the lane, wire the existing canonical Rust owner verification
before export; verify a failed receipt through that same owner as diagnostic
evidence without masking the original product failure. A successful proof with
missing/unverified export must fail publication. Do not add a second semantic
verifier in Python or rerun the product portfolio in the policy job.

Authenticate producer through current native workflow/run/attempt/job and exact
evaluated source, full-SHA defining workflow, selected profile/route and native
artifact identity. Bind PR head and evaluated merge separately when applicable;
manual qualification binds reviewed_sha to GITHUB_SHA. Native authenticated
artifact metadata, producer outputs and context must agree. Check upload's
native artifact ID and digest, creation time after proof entry, per-member digest
and byte size. Repository/head strings in a ZIP are insufficient authentication.

## Publisher, importer, policy and failure behavior

Reuse the existing pinned uploader and authenticated bounded importer, adding a
closed lane selector instead of weakening RIPR's required-member list. Required
successful publication is blocking, retained fourteen days. Packaging, missing
receipt, native artifact discovery, download, digest, extraction or import
failure must fail the configured policy and aggregate and withhold success
outputs. The aggregate remains immutable and runs no caller code/archive import.

The policy receives only a central-verified private imported directory. It checks
transport envelope/native proof success and exporter digest against authenticated
bytes. Published central source now invokes the actual canonical Rust
`--verify-receipt` owner before permitting the existing main-red policy, so
`scripts/ci/verify_rust_small_import.sh` remains an unused reference rather
than starting a duplicate cold build. The central step runs that owner
against the clean checkout and observed exact 1.95.0 toolchain, then requires
receipt result success. A coherent diagnostic failure can pass shape verification
but must never make policy green. Full native producer authentication is still
mandatory; a refreshed old receipt mtime alone cannot establish attempt custody.

The verification adapter removes GH_TOKEN/GITHUB_TOKEN and enterprise variants
from Cargo/build-script children, disables toolchain auto-install, sets two
build jobs and isolates its target beneath RUNNER_TEMP. The fresh policy job
has no product compilation cache: its cold validator build is a real additional
cost that must fit the existing 35-minute policy/resource/storage envelope.
It runs no product steps and does not replace the producer proof. Missing toolchain,
validation failure or timeout is blocking; do not weaken verification to avoid
that cost. No cold local build was run for this preparation.
The separate metadata token must never reach Cargo/product or an archive command.

On entered product failure, retain only a coherent fresh actual failure receipt
that exists; preserve original failed status. Partial export, missing receipt,
pre-entry refusal, timeout or cancellation cannot establish success. A failure
attachment cannot satisfy the complete-call success predicate. Failed-job-only
reruns must not reuse old-attempt successful product or attachment evidence.
Eligible recovery must create fresh custody and repeat all identity checks.

## Future caller transaction

After independent central source/public-byte review and a separately confirmed
exact workflow allowance, update the existing draft caller together with pin
provenance and exact source/result contracts. Inputs are:

```yaml
profile: standard
script: .ci/rust-standard-proof.sh
result_script: .ci/rust-standard-result.sh
fetch_depth: 0
artifact_lane: perl-rust-small
upload_artifacts: true
```

Keep force_hosted's current isolation and force_target=auto for the bounded
manual experiment. Omit proof_admission for that experiment; retain the literal
false native job branch. Dispatch admission still requires pre_merge_admission
and exact reviewed_sha. No new token, permission, runner, protection or live pin
change is authorized by this proposal. Do not guess the future public SHA.

## Acceptance tests required from the central owner

1. Actual classifier positive/negative execution for each existing lane and the
   new closed selector; mutate only the Perl branch and show its detector fails.
2. Actual anchored publisher/importer execution with a complete canonical receipt
   and exact central context; compare retained bytes/digests, not invented success.
3. Same-head prior attempt, wrong repository/SHA/run/producer/defining workflow,
   stale mtime, missing proof entry and ambiguous native artifact rejection.
4. Missing/extra/duplicate members, unknown manifest keys, mismatched digest/size,
   malformed JSON, path escape, absolute/backslash paths, symlink/hardlink/special
   entries, ZIP bombs and compressed/expanded/member-count bounds rejection.
5. Missing receipt, exporter failure, uploader failure, authentication/download/
   import failure and policy refusal keep aggregate red and outputs absent.
6. Real partial product failure retains original failure and only genuine fresh
   receipt bytes; timeout/cancellation/pre-entry failures establish no success.
   Recovery repeats admission/import and rerun-all binds the new attempt.
7. Existing RIPR, ripr-product prepared work and failure-diagnostics source
   semantics and discriminating contracts remain intact; no permissive fallback.
8. Internal/generated-public exact bytes, actionlint and independent fixed-source
   review before publication. Source tests do not establish native qualification.

Consumer exporter controls execute exact bytes/current identity/failure outcome,
prior-attempt context, same-head stale receipt, wrong/dirty source, missing/oversized
receipt, symlink/hardlink/linked ancestor, existing staging and out-of-temp refusal.
It also rejects unrelated producer/workflow/event/ref context and nonprivate
destination mode. These checks bind to the current process environment; native
API producer authentication remains central's responsibility.
The exporter is now wired into the candidate proof; its fixtures are not product proof.
The owner-provided synthetic fixture is
`scripts/ci/fixtures/rust-small-proof-success.v1.json`, derived from Rust's
`success_receipt()` and canonical argv constants. Its sample subject/tool versions
are intentionally synthetic; for executable CLI shape/identity tests, substitute
the clean test checkout's SHA and observed rustc/cargo versions, never claim this
fixture as real product evidence. Actual retained native product bytes remain
unavailable. Five actual canonical CLI controls accepted that adapted fixture
and rejected omitted step, wrong argv, false success and wrong source. They reused
an existing immutable executable (no new Cargo build), whose unchanged owning
Rust task/admission source was compared with the earlier built source. The
policy adapter's five mock-Cargo tests separately prove exact command/credential
isolation/refusal behavior; mocks do not establish canonical semantic validity.
Final native qualification must additionally retrieve and verify the real Rust
receipt, real product/result outcomes, allocated Contabo revision-9 tool/image
identity, resource limits and post-job cleanup. Context/sccache summaries or
outer-only em-ci-result.json cannot replace these observations.
