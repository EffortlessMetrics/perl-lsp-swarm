# Staged workflow composition for explicit qualification

This branch composes #18066's adapter, #18069's admission/publication source at
`a2a0ab9a9c2ab16f0672eee3269efd7a09560683`, #18063's source perimeter, and
the two generated-PR dispatch repairs. It imports the bounded H/S bridge dependency
closure from #18068 at `b9cdc78b78467f75efcf9946f32239a31b7c8da9`; it does not
merge that carrier's unrelated source history. It is not an activated late qualification
generation. No required check, protection, runner allowance or permission is changed.

The exact remaining activation joins and source rollback procedure are in
[qualification-cutover.md](qualification-cutover.md). That procedure requires
review against the actual final combined source and fresh native observations;
the local reversal proof does not establish runtime rollback.

The coordinated unapplied leaf changes `post-merge-status.yml` and
`aggregate-ci-lane-history.yml` to metadata-only dispatch. Tracked workflows and
their existing native Rust contract retain today's mandatory product dispatches,
including exact CI base/head inputs. Shell-spy regressions distinguish installed
and projected behavior. Dispatch removal must not land alone while generated
candidates still depend on those required producers.

## Reviewable root trigger diff

`.ci/staged/17903-root-triggers.patch` is an **unapplied** source/settings-projection
leaf for the coordinated cutover. It removes ordinary PR/Ready/update, merge-group
and every-main-push entry from `ci.yml`, `ripr.yml` and `em-ci-routed-rust.yml`, and
projects their policy event lists to manual dispatch in `required-checks.toml`. It preserves all job
bodies, context names, required flags, app bindings, substantive commands and
failure policy. The same leaf contains generated-PR dispatch removal and its
native Rust contract projection, so those changes remain unapplied too.
The native static policy contract remains structurally valid. It does not prove
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

`enter_native(event, central_capture)` now joins the existing exact dispatch
validation and duplicate election to the real owner readiness boundary before
central capture. It reacquires election, subject and the current in-progress
native attempt around admission/capture. An unelected duplicate never reaches
owner acquisition; missing owners retain NOT_PROVEN. A READY owner join returns
only READY_FOR_ALLOCATION, zero execution counts and no check ID or authority to
execute/merge. This native entry is source-level wiring, still uninstalled in the
metadata workflow; it does not replace the selection, provenance or allocator
owners with local fixtures.

The next execution seam is `execute_proof(proof_owner)`. After the existing
allocator admits work, trusted control code supplies its bound canonical producer
callback `(environment, *, acquire_plan) -> process_status`. The seam verifies
exact H/tree and native context before invocation, strips metadata credentials and
Git overrides from the child environment, reacquires context/request/checkout
before export, and invokes the existing exporter with the same plan acquirer.
It preserves failure process status and rejects signal/non-status values or an
exit status contradicting the receipt outcome. This is an invocation boundary,
not an allocator, command selector or canonical semantic verifier. Its child-process
regression uses a synthetic producer; actual product execution is not claimed.
The callback remains uninstalled in workflow entry points pending the canonical
producer owner's invocation binding.

The focused runtime regression invokes the actual bridge/export/result/admission
and publication-preparation functions. Its native-import and canonical-semantic
callbacks are synthetic fixtures; this is not product qualification or a claim
that a typed object authenticates itself. The seven imported bridge/dependency
files originated byte-identically from #18068. The exporter now adds an optional
trusted process-status check before any custody writes; the other six files are
unchanged from that import. Only the bridge/export test closure is imported;
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

## Operator-admitted actual local rehearsal

`RustSmallOwners.entry_committed_owner` is a distinct existing warm-proof port.
It defaults to unavailable; entry eligibility never calls the requested run's
`committed_proof` importer. `verified_qualification()` returns the canonical
current-run observation once, for reuse by publication preparation and
`qualification_completion.build`. Completion without an existing exact H/app
check retains `result: not_proven` and `check_run_id: null`.

After the operator admits the exact tool build and local proof resource scope,
run the existing trusted `[xtask, "rust-small-proof"]` warm pass separately in
clean H and preserve its receipt. The helper's actual mode requires that existing
receipt; it does not run a qualification producer to authorize its own entry.
Use the committed control source explicitly when the process cwd is H:

```sh
PYTHONPATH="$CONTROL" python3 -B "$CONTROL/scripts/ci/rehearse_central_qualification.py" \
  "$CENTRAL" --actual --xtask "$XTASK" --xtask-sha256 "$XTASK_SHA256" \
  --control-sha "$S" --workspace "$H_WORKTREE" \
  --warm-receipt "$WARM_RECEIPT" --output "$OUTPUT"
```

All supplied tool, checkout, receipt and output paths must be absolute. Output is
a fresh directory outside H. The trusted tool digest is checked before entry
verification and around production. Warm bytes are copied before the second pass
can replace H's default receipt. Selection names are projected from that warm
receipt, then accepted only after the actual Rust verifier validates the receipt
against clean H and its canonical ordered vocabulary. No second command catalog
is introduced.

The local entry join combines that actual committed warm proof with explicitly
fixture review/native facts. It also binds the existing `Adapter.explain` to
`owners.eligibility`, injecting only the fixture review methods into the existing
authorities. The production join still checks real clean control S and explain
reacquires the current subject. Both entry surfaces must be READY before the fixed second-pass
`[xtask, "rust-small-proof"]` producer in H. The producer loses metadata tokens,
Git overrides and the control `PYTHONPATH`/`PYTHONHOME`; admitted Cargo resource
environment remains intact. Actual central embedded capture, export, authenticated
import and custody code surrounds the executed bytes. `CanonicalOwner` invokes
the same trusted binary for current-run Rust semantic verification, then the
no-write completion module retains NOT_PROVEN without H-check publication.

The central import API/run/check metadata and review facts remain bounded fixtures;
H's first parent supplies the fixture base. These do not claim GitHub acquisition,
native producer authentication, production readiness or permission to merge.
`entry.json` labels that acquisition mode, and `report.json` distinguishes actual
Rust semantics from fixture facts. Actual start/finish timestamps delimit the
fixture native envelope. A failed real producer remains failed; the existing
success-only central importer is not fed fabricated success to consume it.
Its actual receipt/log and NOT_PROVEN failure report remain available instead.

This command neither builds its trusted tool nor allocates capacity, dispatches
hosted CI, publishes a check, activates the staged root patch or installs callbacks
into a product workflow. The operator retains resource admission and tool/source
association. Real native origin and publisher/runtime installation remain later
qualified joins.
