# Staged workflow composition for explicit qualification

This branch composes #18066's adapter, #18069's admission/publication source at
`a2a0ab9a9c2ab16f0672eee3269efd7a09560683`, #18063's source perimeter, and
the two generated-PR dispatch repairs. It imports the bounded H/S bridge dependency
closure from #18068 at `b9cdc78b78467f75efcf9946f32239a31b7c8da9`; it does not
merge that carrier's unrelated source history. It is not an activated late qualification
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

## Implemented inactive owner composition

`qualification_runtime.RustSmallOwners` connects existing owner interfaces: a
trusted central capture/import callback, the H/S receipt exporter and result
mapping, the existing admission join, and the existing read-only H-bound payload
preparer. It requires exact selected portfolio/policy identity, an independently
captured H/tree/B/control/request key, authenticated private imported bytes and
producer run/attempt/check identity, and a typed canonical semantic disposition.
It rechecks checkout, receipt/context bytes and current request after verification.
Coherent product failure remains failure; JSON, exit zero, incomplete execution,
wrong commands, changed policy, stale attempts and moved subjects cannot become
success. The importer and semantic callbacks are trusted code boundaries, not
candidate data. There is no CLI, environment flag or installed workflow that
supplies these callbacks.

The focused runtime regression invokes the actual bridge/export/result/admission
and publication-preparation functions. Its native-import and canonical-semantic
callbacks are synthetic fixtures; this is not product qualification or a claim
that a typed object authenticates itself. The seven imported bridge/dependency
files are byte-identical to #18068. Only the bridge/export test closure is imported;
full carrier workflow/result tests require that owner's additional source and
were not qualified here.

A cross-repository fixture executes the actual central embedded capture,
authenticated import and custody functions before the actual Perl H/S composition
and payload preparation. It also rejects rewriting native check identity to H.
Only canonical semantic verification remains synthetic in this rehearsal:

```sh
python3 -B -m scripts.ci.rehearse_central_qualification /path/to/em-ci
```

The central checkout must contain the corresponding optional acquire-plan source
hooks. This command executes that checkout's existing fixture loader, not live
GitHub endpoints, product commands or a publisher.

## Actual runtime joins still needed

1. The adapter now calls the real #10057 admission join by default. It requires
   clean exact control S and existing current review/convergence authorities;
   its product selection, committed-proof and applicability ports remain
   uninstalled in production. `RustSmallOwners` supplies a source-level committed
   proof callback, but its bound authenticated importer and canonical Rust
   semantic/command-observation callback must still be installed by their owners.
   The fixed `qualification-rehearsal.yml` target remains a diagnostic: selected,
   executed and failed counts are zero, with no product check ID or allocation.
   Its existing control-S policy/docs diagnostics do not qualify product H.
2. The legacy roots are not callable consumers of an accepted allocation. CI's
   normal dispatch resolver requires head == GITHUB_SHA; selected qualification
   instead binds H/tree/B separately from native control S. The eventual central
   consumer must capture that binding, checkout exact clean H, execute the
   existing selected proof and import its native packet. Optional central
   capture/import hooks alone do not change the current checkout/emitter/Rust
   commands. In ci.yml, repository-contract still derives CONTRACT_HEAD only from
   PR/merge events; public-api-pr and semver-pr retain event/API assumptions.
   Compose these with authenticated selected scope before applying the root patch.
3. `Adapter.inspect` consumes an exact run/attempt artifact plus an approved check
   on H. `prepare_publication` prepares the existing Rust Small H check payload
   using current native S custody, then rechecks the live native attempt. It
   performs GETs only and returns PREPARED_NOT_PUBLISHED. No privileged workflow,
   checks:write grant, HTTP writer, token acquisition or activated pin is added.
   The final result/manifest publisher and its authorization remain a separate
   integration dependency. Preserve all five protected contexts and their
   substantive selected proof. Do not reinterpret the controller's green S check
   or a coherent failure receipt as qualification of H.

#17624 already implements the product-independent classifier/bootstrap in its
owner branch. Desktop owns its build/admission composition. Do not compile a second
classifier or make full migration completion a prerequisite for these source tests.

The parent's authenticated desktop observation at 08:24–08:26 UTC establishes
classic plus active ruleset protection; the cloud 403 is not an unknown-state
blocker. It does not qualify this candidate. The combined source, result mapping,
reviewed rollback and current protection must be reacquired immediately before an
explicitly authorized operational cutover. No merge into an unprotected stack base
counts as main qualification. Until those joins exist, keep the root patch unapplied.
