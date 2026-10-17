# Staged workflow composition for explicit qualification

This branch composes the exact #18066 adapter source at
`1a3d5e780212c0af0fd6586c286cfd7c692f2b39` with #18063's source perimeter and
the two generated-PR dispatch repairs. It is not an activated late qualification
generation. No required check, protection, runner allowance or permission is changed.

The actual `post-merge-status.yml` and `aggregate-ci-lane-history.yml` dispatch
steps retain title/history metadata and no longer launch CI/RIPR/Rust Small on
publication. Their existing native contracts and executable shell-spy regressions
are aligned. This partial source must not land alone while generated candidates
still depend on those old required producers.

## Reviewable root trigger diff

`.ci/staged/17903-root-triggers.patch` is an **unapplied** source/settings-projection
leaf for the coordinated cutover. It removes ordinary PR/Ready/update, merge-group
and every-main-push entry from `ci.yml`, `ripr.yml` and `em-ci-routed-rust.yml`, and
projects their policy event lists to manual dispatch in `required-checks.toml`. It preserves all job
bodies, context names, required flags, app bindings, substantive commands and
failure policy. The native static policy contract remains structurally valid. It does not prove
the preserved jobs execute after migration: event-specific guards still need the
selected-scope join below, even where a policy row now names manual dispatch.

RIPR's existing scheduled/explicit cache seed remains reachable. Its route job
excludes schedule and the seed keeps its canonical-ref guard. This is retained
baseline behavior, not new cache runtime proof. The native source diagnostic
still refuses RIPR's mixed schedule/qualification root; there is no exception
pretending that static source shape establishes complete admission.

The patch is deliberately not applied to tracked workflows. Removing triggers now
would strand required producers: the adapter below does not yet dispatch product
work. Existing active-generation trigger tests therefore keep their original
meaning; the isolated patched-tree tests exercise the staged generation. Release,
nightly, badge and post-publication workflows are outside this patch and unchanged.

Focused source rehearsal (no network/product build/dispatch):

```sh
python3 -B -m unittest scripts.ci.test_staged_qualification_workflow_cutover scripts.ci.test_generated_pr_metadata_dispatch scripts.ci.test_request_qualification
```

The fixture applies the real diff to tracked disposable copies, reads them with
the existing native catalog, checks retained job bytes and required-policy fields,
and rejects reintroduced Ready/synchronize roots outside ci.yml. It also calls the
actual production-default adapter against its existing fake GitHub boundary:
H differs from S, admission remains NOT_PROVEN and every API call is GET-only.

## Actual runtime joins still needed

These gaps are in current executable source, not inferred from open issue labels:

1. `request_qualification.py:unavailable_eligibility` returns NOT_PROVEN.
   `Adapter.request` exits before POST. Its only transport target is the fixed
   `qualification-rehearsal.yml`. That workflow calls `rehearse-event`, whose
   manifest has selected/executed/failed = 0 and no check ID. It exports no product
   admission output. The adapter owner must supply the real eligibility boundary
   and qualified allocation handoff before a root can consume it. Reuse existing
   semantic-review/currentness and convergence owners; do not add another readiness
   engine or infer readiness from a caller-provided hash.
2. The legacy roots are not callable consumers of that accepted run. CI's normal
   dispatch resolver requires head == GITHUB_SHA, whereas the adapter correctly
   binds control S separately from H/tree/B. The existing `ci_subject` explicit
   mode (`--event-name explicit`) can describe evaluated H/B without rewriting S;
   the eventual consumer must prove its checkout and native origin. In ci.yml,
   `repository-contract` still derives CONTRACT_HEAD only from PR/merge events;
   `public-api-pr` and `semver-pr` still have PR/push event guards and API scope
   assumptions. Merely applying this trigger patch would retain their source but
   fail to qualify those obligations. Compose them with authenticated selected
   scope when the allocator contract exists, before activation.
3. `Adapter.inspect` consumes an exact run/attempt artifact plus an approved
   check on H. No installed workflow produces that success manifest/check bridge.
   #16134's canonical Rust receipt/producer/result policy already exists in its
   owner branch, but is a different retained contract; its d69 source pin is
   explicitly unadmitted. Do not reinterpret a coherent ProductFailure receipt,
   metadata election, or the controller's green S check as qualification of H.
   Compose a trusted result translation and custody boundary with that owner,
   retaining all five currently protected contexts and selected mandatory proof.

#17624 already implements the product-independent classifier/bootstrap in its
owner branch. Desktop owns its build/admission composition. Do not compile a second
classifier or make full migration completion a prerequisite for these source tests.

The parent's authenticated desktop observation at 08:24–08:26 UTC establishes
classic plus active ruleset protection; the cloud 403 is not an unknown-state
blocker. It does not qualify this candidate. The combined source, result mapping,
reviewed rollback and current protection must be reacquired immediately before an
explicitly authorized operational cutover. No merge into an unprotected stack base
counts as main qualification. Until those joins exist, keep the root patch unapplied.
