# Staged admission and H-bound publication bridge

This follows the request contract in [qualification-request.md](qualification-request.md).
It implements the `candidate_readiness.v1` join and prepares an exact publisher
payload. It does not install a privileged publisher or claim runtime activation.

## Admission implementation

`scripts/ci/candidate_readiness.py` joins existing authority observations, with no
second classifier, review engine, command selector, receipt parser or executor:

- canonical substantive review currentness, acquired live by the existing helper;
- current native review/finding facts, acquired by the existing paginated wrapper;
- the existing selector's exact subject/policy/portfolio and ordered commands;
- authenticated committed proof, already verified by its canonical owner;
- actual required-result applicability/failure dispositions.

The existing final-integration wrapper also blocks draft state. The entry join
consumes its native request/finding facts independently: a reviewed draft with no
pending reviewers or findings is eligible at that seam. It does not toggle Ready
or change final integration checks. Later adverse findings still refuse admission.

The in-process owner interface is concrete:

```
production(acquired_plan,
    selection_owner=existing_selector_adapter,
    committed_owner=authenticated_canonical_import_adapter,
    applicability_owner=existing_required_result_applicability_adapter)
```

Each callable returns the typed `Selection`, `CommittedProof` or tuple of
`IntegrationFinding` observations declared in this module. `Selection` binds the
request key, actual policy/portfolio digests, ordered selected command identities
and remaining integration obligations. `CommittedProof` binds that key, exact
executed command identities, canonical receipt digest and authenticated producer
reference; it preserves failure/untrusted/unavailable. These observations come
from trusted control code after existing owner validation, not request JSON or
agent-authored receipt files. Dataclass construction is not authentication; the
reviewed central importer remains the trust boundary.

The clean immutable control S guard runs before authority acquisition. Local Git
probes receive no metadata credentials or Git environment overrides. Review
commands retain existing metadata authentication but reject fixture environments
and Git overrides; instrument/signal exits cannot certify a pass. The join
executes no selected product command and never dispatches, writes a check or
merges. Missing future qualification is a next-stage obligation, not an entry
cycle; an applicable unresolved real failure remains `NOT_READY`.

The adapter's default callable now invokes this real join rather than claiming
the producer source is absent. Default owner selection/custody/applicability
ports are still uninstalled and therefore refuse `NOT_PROVEN`. Central composition
must bind the actual `pre_push_plan`/`ci_scope` and canonical imported-proof owners;
no current CLI or environment switch substitutes local JSON for those ports.
Synthetic trusted-owner tests exercise READY, current Cargo without nextest,
actually adopted RIPR selection, positive inapplicability, untrusted replay,
missing/wrong/stale proof, later adverse findings and absent future CI.

## H/S source repair

The isolated bridge candidate on #16134's `382a0d10` source changes only
`rust_small_subject.py`, its focused test, `export_rust_small_receipt.py` and
`rust_standard_result.py`. Optional trusted `acquire_plan` composition preserves
legacy entrypoints and installed isolated exporter behavior. The bridge requires:

1. an acquired native plan with repository/H/tree/B/S/request key;
2. matching independently captured `qualification_subject` in native central
   `em-ci-job-context.v1`, bound to S/run/attempt;
3. the actual clean Git checkout at H and its exact tree;
4. a second acquisition before export/result return.

Canonical product receipt bytes bind H; the native export custody envelope still
truthfully binds dispatcher S/run/attempt. Result policy maps the canonical proof
subject to H and effective control to S. Git inspection strips all metadata tokens
and Git overrides. The central source hooks now support this capture/dual
identity, but native entrypoints still omit the trusted callback. Legacy custody
refuses the new mode instead of accepting H as S. The central owner must wire its
actual producer/importer to this bridge. This branch's exporter additionally
compares an optional trusted process status to the exact receipt outcome before
writing custody bytes, so a contradictory producer cannot leave a success packet.
No existing carrier workflow or approved pin is silently replaced.

## Exact permission request (pending approval)

| Field | Proposed scope |
| --- | --- |
| Repository | `EffortlessMetrics/perl-lsp-swarm` only |
| Workflow/job | New `.github/workflows/qualification-publish.yml`, isolated `publish-rust-small` job, not installed by this change |
| App/token | Existing GitHub Actions App ID15368, ephemeral per-job `GITHUB_TOKEN`; no new credential or installation |
| Present permission | Proposed workflow/job absent. Current adapter/central caller have read-only checks access; existing title/liveness write jobs retain their unrelated roles. |
| Requested permission | `checks: write` for this job, with read-only `contents`, `actions`, `pull-requests`; no other write scope |
| Native write | One `POST /repos/EffortlessMetrics/perl-lsp-swarm/check-runs` for current verified H, name `Perl LSP Rust Small Result`, complete success or actual product failure |
| Bound data | H/tree/B/S, approved portfolio/policy, canonical receipt digest and authenticated producer, exact native request/run/attempt, external ID `<key>:<run>:<attempt>`, exact attempt URL |
| Purpose | Report authenticated selected product proof on actual H while dispatcher/control source remains S |
| Duration | Current job token lifetime only; no persistent app, repository, organization or branch-policy change |

GitHub's permission is technically repository-wide check-writing capability; it
cannot be restricted by GitHub to one check name. The reviewed publisher code
must enforce the narrower repository/context/H/identity scope. This residual
capability is part of the approval request, not hidden by the intended scope.

`scripts/ci/qualification_publication.py` prepares this exact payload using a fresh
native snapshot and actual in-progress control run/attempt. It only accepts a typed
`VerifiedQualification` from the existing authenticated canonical owner, rejects
JSON/diagnostic/empty/partial-success proof, and performs GETs only. The existing
adapter transport still refuses the prepared check-write endpoint. No enabling
flag, token acquisition, POST implementation or privileged workflow is installed.

After approval, the narrow trusted control job must authenticate artifact custody
and canonical semantics without executing candidate code with its write token;
reacquire H/B/S and current findings immediately before publication; converge
existing exact external-ID checks; recover an uncertain write by GET-only readback;
and verify returned native check ID/app15368/H/context/run/attempt association.
Publication failure or uncertain acknowledgement remains NOT_PROVEN. The existing
result consumer then validates the real check; neither payload preparation nor
readiness authorizes protected merge. Other mandatory contexts remain necessary.
