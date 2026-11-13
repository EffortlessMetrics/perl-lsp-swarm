# Perl conditional H authority observation

This source-only consumer advances [#19326](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/19326).
It does not install a publisher, grant queue entry, or replace
[candidate readiness](qualification-runtime-bridge.md). The existing Rust Small
publication preparation remains unchanged.

`scripts/ci/qualification_h_admission.py` joins the existing readiness authorities
with an independently authenticated conditional H grant and the existing Hermes
App-check verifier. Its only positive result is `H_AUTHORITY_OBSERVED`. Every
result has empty effects and denies publication, execution and merge authority;
G qualification is never proven by this observer. No workflow calls it today.

## Trusted control interface

`observe(acquire_plan, authorities, authorization_owner=..., hermes=...)` consumes:

- The existing acquired plan, including repository/PR/H/tree/B/control, request
  key and policy/portfolio identities. Extra owner fields are preserved unchanged.
- The existing `candidate_readiness.Authorities` methods. The consumer records
  their actual typed observations before applying `candidate_readiness.evaluate`;
  this retains selected commands and non-adverse findings that its reduced result
  does not include. It implements no second selection or review law.
- An `authorization_owner(Evidence)` supplied by reviewed trusted control code.
  It must independently acquire and authenticate conditional H authorization and
  its exact evidence-to-Hermes-request mapping, returning `Authorization` with the
  exact evidence, request, App/context and provenance references. Constructing this
  dataclass, copying evidence, hashing a request or returning READY is not
  authentication. No production authorization owner is supplied by this change.
- `HermesReadback`, constructed with trusted Hermes model and `CheckPublisher`
  source capabilities, protected policy, authenticated read transport and clock.
  Candidate paths, JSON and environment variables cannot install these ports.

The reader delegates request/receipt semantics to Hermes's existing
`validate_request`, `request_from_conditional_receipt` and `CheckPublisher.verify`.
It restricts its transport to check GETs, requires one matching App/name/external
identity, checks the exact requested numeric check ID, and accepts only completed
success in `H_ADMITTED`. It never calls `ensure`, `complete`, `fail` or
`Admission.execute` (which would also enqueue). Absent or uncertain results refuse;
there is no write or retry path.

After readback, the consumer reacquires subject, review, proof, selection, findings
and authorization, comparing independent snapshots. A closing policy/expiry check
runs after those potentially slow observations. These sampled reads are not an
atomic publication fence or a guarantee against later withdrawal.

All existing integration obligations are transported unchanged. The five historic
Actions contexts, Gate Enforcement Contract and selected `source_commit_api_check`
remain separate G obligations; their presence here is not success or a new fixed
inventory. Conditional H does not establish `H_ENQUEUED`, original enqueue
causality, current queue membership, actual G proof or protected M.

## Unresolved binding and narrow contract proposal

The inspected Hermes source is [#64 at 914087d2](https://github.com/EffortlessMetrics/hermes-deploy/tree/914087d2b973936815c2146009cad6d2bcfc323a).
Its closed `hermes.hosted-queue-admission.v1` request binds `proof_profile` and
`producer_generation_sha256`, but carries no explicit Perl policy/portfolio or
authenticated committed-proof reference. The accepted independently authenticated
mapping is **NOT_PROVEN**. The default consumer therefore returns NOT_PROVEN even
when readiness is READY. A nonempty mapping reference alone does not close it.

[Coordination with the existing Hermes owner](https://github.com/EffortlessMetrics/hermes-deploy/issues/60#issuecomment-6865189319)
asks whether protected profile/generation policy already supplies that exact
binding. If it does not, the proposal is to extend the existing request/receipt
contract narrowly with a versioned binding to the existing Perl subject,
policy/portfolio and canonical proof identity, validated and retained by its
current owner. Do not silently add fields to v1, derive authority from a digest,
or create a second publisher/receipt registry. No schema change is implemented here.

## Focused proof and limits

```bash
python3 -B -m unittest scripts.ci.test_qualification_h_admission
HERMES_SOURCE_ROOT=/path/to/clean/pinned/hermes-deploy \
  python3 -B -m unittest scripts.ci.test_qualification_h_admission
```

Without the test-only source location the actual-Hermes controls are explicitly
skipped, not claimed passed. With it, the harness verifies the pinned clean Git
source, loads the actual model/publisher and restores import state afterward.
Synthetic trusted-owner and native API responses test the actual read-only join;
they do not establish a production authorization adapter, installed credentials,
secret isolation, protected source generation, live API behavior or an atomic
effect boundary. No Cargo or product execution is needed for this source seam.

Rollback removes these three new files. Existing readiness, selector, importer,
publication preparation, permissions, workflows and protection remain unchanged.
