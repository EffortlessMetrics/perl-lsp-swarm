# Explicit qualification request adapter (staged)

`scripts/ci/request_qualification.py` is the bounded #17900 request/result adapter.
It uses existing `gh api` authentication. No credentials, permissions, branch
protections, required contexts or ordinary CI triggers change in this PR.
The rehearsal workflow is non-required and accepts only explicit dispatch.
Draft pushes, ordinary pushes and readiness changes are not qualification requests.

This is source implementation and synthetic boundary coverage, not deployment of
late qualification. Production invokes the real `candidate_readiness.v1` join but
returns `NOT_PROVEN` while its authoritative selection, authenticated distinct
warm-proof and applicability owner ports remain uninstalled. There is no CLI, environment
flag or candidate JSON override. The installed workflow executes metadata only,
emits `not_proven`, and publishes no check on H. A successful metadata job is not
successful qualification. Existing mandatory producers remain necessary until a
separately reviewed coordinated cutover.

## Request and admission interface

The CLI operations `explain`, `request`, `recover`, `inspect` and `rehearse` take:

| Argument | Meaning |
| --- | --- |
| `--repository`, `--repository-id`, `--pr` | Expected native repository identity and open PR |
| `--expected-head` | Candidate commit H |
| `--expected-base` | Observed PR base B |
| `--expected-control` | Expected current default-branch control commit S |
| `--portfolio-sha256`, `--policy-sha256` | Selected proof portfolio and governing policy identity |
| `--app-id`, `--check-name` | Approved native result reporter and context |
| `--run-id`, `--run-attempt` | Saved native identity, required for inspect/rehearse |

Use real acquired values; these are expected identities, not assertions that the
candidate is eligible. The adapter reacquires repository, PR H/B, default-branch S,
H's tree and the fixed workflow's active native ID/path. Its SHA256 request key
binds all these subjects, including app/context, portfolio and policy. It refuses
movement instead of silently selecting new values. Calls are separate observations,
not an atomic GitHub transaction; final judgment must reacquire live state.

The trusted admission callback receives that acquired plan and returns an
`Eligibility(classification, reason)`, where classification is `READY`, `NOT_READY`,
`NOT_PROVEN` or `UNTRUSTED_REPLAY_REQUIRED`. Production runs the existing readiness
join; tests inject its missing owner boundaries. Before enabling dispatch, those
owners must supply real prequalification evidence, including current substantive review
and review-thread convergence, without requiring the future qualification result
or final merge guard (which would create a dependency cycle). Portfolio/policy
strings alone do not authenticate mandatory selection. This adapter does not
implement a second readiness engine or accept a caller-authored readiness receipt.

`explain` is GET-only, reports the admission decision and `effects: []`.
`request` admits before any dispatch. It adopts the lowest matching native run ID
from a complete inventory, or POSTs exactly one dispatch to the acquired default
branch of `.github/workflows/qualification-rehearsal.yml`. Dispatch carries one
string input, `request`, containing the complete acquired plan. Native run name is
`qualification:<request_key>`; accepted readback must match native repository,
workflow ID/path, event, S, key, run ID and attempt. API acknowledgement alone is
`NOT_PROVEN`, never `PR_IN_FLIGHT`. Only a read-back pending run is in flight.
An existing completed run returns `NOT_PROVEN` and an instruction to inspect its
saved attempt; it does not dispatch again.

After a lost POST response, preserve the returned subject/key and use `recover`
with the same expected inputs. Recovery never POSTs. A missing run stays unproven;
never blindly retry an uncertain request. If H/B/S changes after dispatch, the
operation refuses; the dispatch may already exist and must be inspected by its
original subject. No rerun, merge, check-write or auto-merge endpoint is allowed.

Concurrent dispatches may create multiple native metadata runs. The rehearsal
protects the running member from cancellation, but GitHub keeps only one pending
member by default: a newer pending run can cancel an older pending duplicate before
its election code executes. Surviving runs elect the lowest observed matching run
ID. This is metadata convergence, not an exactly-once
product-execution guarantee: listing/dispatch races and non-FIFO concurrency remain.
Do not place expensive product work behind this election without qualifying the
allocation protocol. Run/artifact inventories are deliberately bounded to one
complete page of 100; truncation refuses with an owner disposition rather than
silently ignoring history. Transport operations have a 30-second timeout and
one-MiB response bound; errors never expose token-bearing stderr or auto-retry.

## Native result/attempt binding

The staged `inspect` consumer accepts exactly one unexpired artifact named
`qualification-result-<key>-<attempt>` from the matching native run/repository/S.
Its ZIP must contain only `qualification-result.json`, bounded to one MiB. The
manifest binds schema version 1, repository/name+ID, PR, H/tree/B/S, portfolio and
policy digests, request key, workflow ID/path, native run ID and attempt.
Native artifact metadata has no independent attempt field: a future trusted
producer must derive the manifest/name from native execution context, never from
candidate JSON. The consumer also rereads the current native run attempt.

`not_proven` carries no qualification verdict. `success` requires positive selected
and executed counts, full selected execution, zero failures and successful native
control completion. `product_failure` requires executed failing proof and remains
`PRODUCT_OR_TEST_FAILURE`; cancellation, timeout and incomplete execution alone do not
become product failure or success. A verdict also requires a real native check ID
whose H, approved app, context, completed conclusion, external ID
`<key>:<run>:<attempt>` and details URL
`https://github.com/<repository>/actions/runs/<run>/attempts/<attempt>` all match.
Current subject and admission are reacquired after successful proof. Later adverse
admission on the same H preserves verified proof while refusing overall success.
H2, changed B/S, another attempt or mismatched proof identity invalidates this request.

The landed runtime and completion owners now supply this source contract, and
`qualification_publication.prepare` prepares the exact H/app payload through
GET-only reacquisition. No trusted publisher is installed here. Installing the
runtime caller and authenticating its actual selected/executed evidence remain
separate from the held publisher permission and native app/check association. The existing
`ci_subject` explicit mode can represent candidate H/B independently of dispatcher
S; ordinary dispatch semantics must remain unchanged. Product checkout must prove
H, with trusted control source kept separate from candidate code.

## Rehearsal and integration boundary

`rehearse-event` consumes native GitHub event/environment context, reacquires the
plan, rejects tampered request fields and emits only a NOT_PROVEN metadata artifact.
The workflow uses read-only contents/actions/PR permissions, immutable action pins,
and a default-branch job guard. Adapter-mediated requests check out S. Arbitrary
native dispatch can select another ref and its workflow can change the guard or
script; this workflow is **not a privileged trusted-execution boundary**. A future
privileged publisher needs a separately qualified control-source boundary.

The #10057 readiness join, authenticated portfolio/policy adapters, H/S runtime
and trusted native factory are landed source through #18067. Runtime activation
still requires their actual native importer/resource bindings and caller, qualified
allocation/duplicate behavior, trusted product/result publication, complete
producer/indirect-trigger consumption (#17903), and coordinated protection/trigger
cutover with fresh observed readback and rollback (#17080/#10065). These are precise integration interfaces, not blockers to reviewing
this staged source. No deployment, qualification run or merge is requested by a
push of this change. The independent final vision/repository judgment and protected
merge/readback remain separate from request acceptance and successful proof.

Focused verification: `python3 -B -m unittest scripts.ci.test_request_qualification`.
Existing `ci-gate-self-tests.yml` runs these inexpensive controls on relevant source
changes. No extra workflow lifecycle producer or parallel policy parser is added.
The existing non-Rust policy covers `scripts/**`, `docs/**` and workflow YAML; the
Rust-owned inventory/check remains mandatory in qualification.

## Executable source rehearsal and current integration mapping

The explicit rehearsal now runs these existing interfaces after native subject
acquisition, on the immutable control checkout S:

```
python3 -B scripts/ci/validate_gate_enforcement_contract.py --root . --receipt rehearsal-diagnostics/gate-enforcement-contract.json
python3 -B tests/test_docs_agents_contract_workflows.py
```

The first is the existing required-context/producer source mapper (18 configured
contexts, 17 mapped jobs at the inspected source); its static success explicitly
retains `live_enforcement_status: NOT_PROVEN`. The second is the exact command of
`docs_agents_contract_workflows` in `.ci/gate-policy.yaml`, an existing bounded
51-test diagnostic. Its log and the existing validator receipt are uploaded as
`qualification-diagnostics-<key>-<attempt>`, separately from the unchanged zero-
execution `not_proven` qualification manifest, including when diagnostics fail.
These commands are diagnostic execution on S, not product selection/execution on H.
The CLI integration test exercises the real process/native-context entry with a
bounded fake GET-only GitHub service, then the real source validator on a clean
tracked checkout, verifying diagnostic success cannot alter the result manifest.
It is synthetic native transport coverage, not an actual hosted run.

Concrete authorities and source gaps, inspected October 17:

| Seam | Actual implementation and remaining executable change |
| --- | --- |
| Semantic review | `check-pr-semantic-review-currentness.py` acquires reviews and evaluates cumulative markers; all required review sections and exactly one result declaration are mandatory. A marker string alone is rejected. |
| Known findings/threads | `check-pr-review-convergence` calls the paginated core and emits `pr_review_convergence.v2`; it handles native facts and cannot substitute for semantic review or committed proof. |
| Required obligations | `.ci/policies/required-checks.toml` and `validate_gate_enforcement_contract.py` map existing contexts/producers. They do not select changed-path product obligations or authenticate completed proof. |
| Risk/command selection | `xtask/src/tasks/ci_subject.rs` explicit mode plus `ci_scope.rs` own subject and risk-sensitive lane selection; `.ci/gate-policy.yaml` and the existing shard runner own gate commands. Do not copy their selection logic into this adapter. |
| Committed Rust Small proof | `rust_small_proof.rs` owns `rust_small_proof.v1`, all ten commands, exact clean source/toolchain/profile/census and outcome verification. Receipt freshness/presence helpers alone do not prove execution or producer custody. |
| Export/import owner | The #16134 source epoch `382a0d10c9402f47391ef35ad8b4b09cb230a61b` introduced `export_rust_small_receipt.py`, `rust_small_evidence.py`, isolated native import and canonical Rust verification through the pinned central workflow. Those interfaces remain the custody/semantic owners; central #246 stages the acquired-subject import hooks and optional command sidecar without installing their callback. |
| H versus S bridge | `rust_standard_result.policy_environment(..., acquire_plan=...)` now consumes separate adapter H/B and dispatcher S through the trusted H/S bridge. Its default legacy entry remains unchanged; `qualification_native_factory.checkout_candidate` implements exact clean H/tree checkout in source. Native central callback/factory installation remains required. |
| H/app publication | The caller and central `d69e3c9cbc55ee8c5d084facfe18432cd4c28618` use read-only checks access and native workflow jobs. No installed publisher emits this adapter's H/check/external-ID/attempt contract. No permission grant or alternate publisher is introduced here. |
| Entry join | `candidate_readiness.production` joins live canonical review/native findings and trusted typed owner ports. `NativeEntryOwners` implements selection, applicability and distinct warm-proof adapters; `compose_native` binds them to the existing runtime. Actual native importer/resource bindings and a trusted caller still need installation. `RustSmallOwners.enter_native` connects native election/current attempt to that join and capture; the final pre-merge guard must not authorize its own future CI. |

This finite rehearsal executes real source/policy code and retains its result while
preserving the distinct integration gaps. A hosted attempt requires publication
of the workflow/control source on the default branch first. Do not dispatch the
#16134 carrier's unapproved central pin or treat these diagnostic reports as
admission, complete mandatory proof or permission to merge.

## Composition handoff and cheap allocation proof

The source constructor is `qualification_native_factory.compose_native`: it binds
`NativeEntryOwners` to `RustSmallOwners`, installs the existing eligibility callback,
and materializes exact clean H separately from loaded clean S. Trusted native
installation must provide the actual prior-attempt importer
`(run_id, run_attempt, *, acquire_plan)`, current-attempt importer
`(*, acquire_plan)`, immutable canonical tool, and existing allocator context manager
`(plan, selection)`. These are control-code callbacks, not request JSON, module
names, local receipts or environment enable switches.

`execute_native` uses the existing native election before capture/allocation and
repeats admission/election after waiting for the resource lease. The focused
factory tests cover a lower-ID duplicate before allocation and one appearing
during the wait: neither starts the producer, writes proof-entry timing or exports
a packet; the latter releases the acquired lifetime. The positive fixture executes
an actual subprocess and private export, preserving receipt H and native envelope
S/run/attempt. Native API, review, canonical tool and allocator remain explicit
fixtures. This is cheap source-boundary proof, not exactly-once native execution,
live resource containment, authenticated hosted acquisition or product qualification.

The integration owner must accept the concrete native callback/caller handoff in
#17900 before another writer changes the existing adapter or central carrier.
Preserve `codex/qualification-adapter`; absence of a retrievable active executor
does not release its custody. See [native adapters](qualification-native-adapters.md)
and [activation order](qualification-cutover.md) for the existing source contracts
and separate runtime holds.
