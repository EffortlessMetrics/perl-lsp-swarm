# Explicit qualification request adapter (staged)

`scripts/ci/request_qualification.py` is the bounded #17900 request/result adapter.
It uses existing `gh api` authentication. No credentials, permissions, branch
protections, required contexts or ordinary CI triggers change in this PR.
The rehearsal workflow is non-required and accepts only explicit dispatch.
Draft pushes, ordinary pushes and readiness changes are not qualification requests.

This is source implementation and synthetic boundary coverage, not deployment of
late qualification. Production admission deliberately returns `NOT_PROVEN` because
#10057's `candidate_readiness.v1` producer is absent. There is no CLI, environment
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
`NOT_PROVEN` or `UNTRUSTED_REPLAY_REQUIRED`. Production has only the missing-producer
implementation; tests inject the future boundary. Before enabling dispatch, #10057
must supply real prequalification evidence, including current substantive review
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
serializes their concurrency group without cancellation and elects the lowest
observed matching run ID. This is metadata convergence, not an exactly-once
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

This is a proposed result bridge contract, not a claim that #16134's publisher
already emits it. No trusted publisher is installed here. Before wiring product
execution, agree its actual selected/executed evidence and native app/check
association with that owner, preserving every mandatory test/context. The existing
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

Required before runtime activation: real #10057 admission producer, authenticated
portfolio/policy derivation, qualified allocation/duplicate protocol, trusted
product/result publisher, complete producer/indirect-trigger mapping (#17903), and
coordinated protection/trigger cutover with fresh observed readback and rollback
(#17080/#10065). These are precise integration interfaces, not blockers to reviewing
this staged source. No deployment, qualification run or merge is requested by a
push of this change. The independent final vision/repository judgment and protected
merge/readback remain separate from request acceptance and successful proof.

Focused verification: `python3 -B -m unittest scripts.ci.test_request_qualification`.
Existing `ci-gate-self-tests.yml` runs these inexpensive controls on relevant source
changes. No extra workflow lifecycle producer or parallel policy parser is added.
The existing non-Rust policy covers `scripts/**`, `docs/**` and workflow YAML; the
Rust-owned inventory/check remains mandatory in qualification.
