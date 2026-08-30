# CI Cache Policy

Shared CI caches are performance infrastructure. A hit, miss, denied save, corrupt
entry, or eviction may change execution cost, but it must not change the product or
test verdict for the same exact source subject.

Candidate runs may restore reusable state. They may not publish shared state. Cache
**save** authority is restricted to an explicitly trusted event/ref and data-authority
context. The proof must exclude candidate execution or candidate-controlled cache
content; a branch-looking ref is not sufficient by itself.

> Companion: [cost-and-verification-policy.md](cost-and-verification-policy.md).

---

## Writer-authority rule

For every `Swatinem/rust-cache` invocation in a candidate-capable workflow job, keep
restore available and declare save authority explicitly. The preferred pattern names
both the trusted event classes and canonical branch refs:

```yaml
- uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6  # v2.9.2
  with:
    cache-on-failure: true
    cache-all-crates: true
    shared-key: <stable-key>
    save-if: ${{ (github.event_name == 'push' || github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') && (github.ref == 'refs/heads/master' || github.ref == 'refs/heads/main') }}
```

Under this guard:

- **Pull requests:** restore, run, do not save.
- **Merge groups:** restore, run, do not save.
- **Canonical branch contexts:** an intentional push, schedule, or dispatch whose ref
  is `main` or `master` may save when that workflow/job is designed to seed the cache.
- **Feature branches and tags:** may restore but do not satisfy the save condition.

A shorter ref-only guard can remain acceptable for an existing workflow only when an
executable contract pins its complete trigger set and proves that every candidate event
uses a candidate or integration ref. That is a reviewed exception to the preferred
shape, not a portable pattern: adding another trigger can invalidate the proof without
changing the cache step itself.

`pull_request_target` is different: its ref names the base branch, so a ref-only
`main`/`master` expression can evaluate true for a PR-originated run. A workflow
reachable through `pull_request_target` must either exclude that event explicitly from
save authority or carry a dedicated reviewed proof that no candidate checkout, payload
field, fetched artifact, generated key, or candidate-influenced output can enter the
cached path. Never infer safety there from `github.ref` alone.

The same caution applies to any indirect event whose ref identifies trusted repository
state while its inputs or downloaded artifacts may still be candidate-controlled.
Event semantics, executed subject, cache key authority, and cached-byte authority must
agree.

A workflow or job name is not authority. In particular, a workflow called `nightly`
remains in scope when it also subscribes to candidate events. Conversely, a job that is
statically unreachable from candidate execution or candidate-controlled content does
not need a textual candidate guard merely for uniformity.

The example uses a stable `shared-key` without `${{ hashFiles('Cargo.lock') }}` because
`Swatinem/rust-cache` already incorporates the lockfile hash into its internal keying;
adding it to `shared-key` prevents restore fallback when `Cargo.lock` changes (the action
uses `shared-key` as a restore prefix). Existing workflows that include the hash for
historical reasons are not changed by this writer-authority rollout.

---

## Direct cache-save actions

An explicit writer such as `actions/cache/save` must carry its own trusted event/ref
guard. Content readiness is necessary but is not writer authority by itself:

```yaml
- name: Save generated cache state
  if: |
    steps.prepare.outcome == 'success' &&
    github.event_name == 'workflow_dispatch' &&
    github.ref_name == github.event.repository.default_branch
  uses: actions/cache/save@<reviewed-commit>
  with:
    path: <cache-path>
    key: <cache-key>
```

The accepted condition depends on the reviewed lane. A scheduled default-branch writer,
for example, may use a different explicit event guard. The invariant is that a candidate
PR, merge group, `pull_request_target` payload, feature branch, tag, label,
candidate-provided input, downloaded candidate artifact, or candidate step output cannot
become the fact that authorizes publication or supplies unreviewed cached bytes.

A successful install, build, test, or corpus sweep may prove that bytes are eligible to
be cached. It does not prove that the current run is trusted to publish them.

---

## What this does not change

- Candidate cache restore remains available.
- Cache absence or rejection falls back to the ordinary cold execution path.
- Concurrency and cancellation policy are preserved.
- Keys, restore prefixes, payload identity, and useful-work measurement are separate
  design questions.
- Release and deployment cache lifecycles remain separately reviewed because they are
  infrequent and may carry different trust and retention requirements.

---

## Scope and reachability

This policy applies to every active cache consumer that can write from a workflow/job
reachable through `pull_request`, `merge_group`, `pull_request_target`, or an indirect
event carrying candidate-controlled inputs or artifacts. Hybrid
schedule/dispatch/candidate workflows remain in scope. Reachability and authority are
determined from the workflow trigger, containing job/step conditions, executed checkout
subject, expression inputs, artifact provenance, and cached path—not from the file name
or intended cadence.

The exhaustive active denominator is owned by the cache inventory rather than copied
into this document. Dormant composite actions and templates are not active behavior.

---

## Verification

The repository-owned workflow policy and active-cache inventory must reject:

- a candidate-reachable `Swatinem/rust-cache` step without an explicit `save-if`;
- a direct cache-save step guarded only by content success;
- a save condition that accepts pull-request refs, merge-group refs, feature branches,
  tags, labels, or candidate-controlled values;
- a ref-only `main`/`master` condition in a `pull_request_target`-reachable job without a
  dedicated base-only data-authority proof;
- an indirect writer that can cache candidate-provided or candidate-derived artifacts
  under an apparently trusted ref;
- an inventory row whose action, key, output path, reachability, executed subject,
  artifact provenance, or writer disposition has drifted from workflow source.

The same proof must accept intentional schedule/manual/default-branch writers, reviewed
base-only `pull_request_target` jobs whose cached bytes cannot be candidate-influenced,
and jobs statically excluded from candidate events. No separate required status context
is needed; the rule belongs in the existing workflow-policy/result plane.

This section is the executable acceptance contract for #13927, not a claim that the
current `check-candidate-writers` scanner already inspects cache actions, keys, paths, or
artifact provenance. Until #13927 lands, focused leaf contracts enforce individual
writer repairs; the existing candidate-writer scanner remains a separate control.

Expected effect after the active writer repairs land:

- candidate wall time remains approximately unchanged because restore still runs;
- cache write traffic is concentrated in reviewed trusted contexts;
- merge-ref and feature-ref cache churn no longer competes with canonical reusable
  entries;
- cache receipts can report restore/save facts without treating an action-level hit as
  proof of compilation or setup work avoided.

LEM impact appears in `target/ci/ci-actuals.json` once the CI actuals receipt is wired up.
