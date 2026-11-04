# Current titles for a proved native merge group

`scripts/ci/native_queue_title.py` and `scripts/ci/pr_title_validator.js` supply the
actual-G source for the existing `validate-title` context. The installation remains
staged in `.ci/staged/17903-root-triggers.patch`. Ordinary PR/metadata/manual roots
are removed from this workflow; its only entry is native `merge_group.checks_requested`.

The source is built and tested, but **not activated**. The checked-in
`.ci/policies/native-queue-title.json` has `enabled: false` and no fabricated App
binding. The coordinated #17080 transaction must install the reviewed producer,
bind the real trusted H publisher, verify effective protections and exercise the
native canary. A local fixture does not prove how the installed native queue exposes
membership. If that surface cannot supply the join below, this leaf refuses.

## Proof boundary

The existing `native_queue.GitHub` transport now has a second fixed **read** query,
`Repository.mergeQueue(branch: ...)`. Its existing immediate-enqueue interface and
trusted authorization callback retain their meanings. This title leaf never invokes
enqueue, checks publication, labels, comments or merge.

1. Bind actual event B/G/ref, repository identity, run ID and attempt. Read both the
   current Actions run and exact attempt, the Actions App `15368` check suite, and
   the current native queue ref. A dispatch, stale attempt, different repository,
   different workflow or rewritten ref is not this subject.
2. Read the complete native queue connection. Require exact counts, no next page,
   unique entry/PR identities and contiguous positions. Find **one** entry whose
   native `baseCommit` and `headCommit` equal event B and G. Select its full ordered
   prefix, then bind every current PR, H, queue entry, title, author and complete
   labels. The branch name supplies no PR number; there is no associated-PR lookup.
3. For every H, authenticate the retained expected-head enqueue acknowledgement
   from the configured trusted App's current successful H check. An arbitrary JSON
   document, same-named check from another App, current content equality or a live
   queue entry without the retained acknowledgement is insufficient.
4. Require every constituent to carry the same App-authenticated reviewed producer
   generation and exact target closure. In a fresh private bare Git store, fetch
   fixed subjects from the fixed repository origin, prove B ancestry of G and
   compare G's producer objects and modes with that admitted target closure.
   Sequential `git merge-tree --write-tree` composition must equal G's exact tree.
   Private intermediate commits retain both the preceding composition and consumed
   H as parents so stacked edits/reverts keep the correct merge base. This makes no
   demand that actual squash G contain H in its ancestry or have two parents.
5. Re-observe the native subject after composition. Run the existing title rules
   for **every** proved member, then re-observe the run, prefix, H intake and title
   proposition before the Actions job can succeed. A changed H, title, author,
   exemption label, intake, expiry or authority generation invalidates that result.

The queue, title-label and H-check connections are bounded to 100 complete entries.
Truncation refuses; it never becomes a guessed member set. If `headCommit` exposes H
instead of actual G, or multiple entries claim the same B/G, membership remains
`NOT_PROVEN`. Same-tree/no-op heads do not dispense with exact acknowledged H identity.

The schema sources are GitHub's [Repository mergeQueue API](https://docs.github.com/en/graphql/reference/repos)
and [MergeQueue/Entry API](https://docs.github.com/en/graphql/reference/pulls). Their
field descriptions alone do not establish the installed service's full event-time
membership semantics. The canary must test the actual join, including complete
multi-member prefixes, reordering/withdrawal and same-tree/no-op constituents. Do not
enable an association or maximum-batch-size fallback if those observations fail.

## Authenticated intake, separate from G qualification

The Hermes publisher's existing `queue_enqueue` operation writes canonical JSON to
the already bound H check's `output.text` only after retaining the acknowledged
`expectedHeadOid=H` mutation. The shared schema is strict `hermes.queue-intake.v2`:

```json
{
  "schema_id": "hermes.queue-intake.v2",
  "schema_version": 2,
  "repository": "owner/repository",
  "pull_number": 42,
  "head_sha": "<H>",
  "entry_id": "<native entry id>",
  "admission_request_id": "<trusted admission id>",
  "enqueue_request_id": "<acknowledged enqueue id>",
  "base_sha": "<H admission base>",
  "expires_at": "<timezone-aware expiry>",
  "authority_sha256": "<trusted admission request digest>",
  "producer_generation_sha256": "<root-admitted reviewed workflow portfolio digest>",
  "producer_closure": {
    ".github": {"kind": "tree", "sha": "<reviewed tree OID>", "mode": "040000"},
    ".ci/policies/native-queue-title.json": {"kind": "blob", "sha": "<reviewed blob OID>", "mode": "100644"},
    "scripts/ci/native_queue.py": {"kind": "blob", "sha": "<reviewed blob OID>", "mode": "100644"},
    "scripts/ci/native_queue_title.py": {"kind": "blob", "sha": "<reviewed blob OID>", "mode": "100644"},
    "scripts/ci/pr_title_validator.js": {"kind": "blob", "sha": "<reviewed blob OID>", "mode": "100644"}
  }
}
```

The exact object must come from `check.app.id = configured publisher App`, the
configured check name, `head_sha = H`, completed/success status, and
`external_id = hermes-queue:<admission_request_id>:H`. Duplicate matching checks or
duplicate JSON keys refuse. The admission base is **not** assumed equal to later G's
event base. The exact intake and check identity are re-read before success; expired
or withdrawn H authority cannot be replaced by a copied old receipt.

This route avoids a dependency cycle: the acknowledged intake exists before G's
title job; the publisher's final `queue_qualify` consumes completed G jobs later.
The intake does not claim G proof. The trusted H App and the five Actions-bound G
contexts remain distinct publishers. No token impersonates Actions App `15368`.

The two v2 fields carry the conflict-checked union of the existing root-owned
workflow profiles' producer closures and the digest of their complete reviewed
portfolio, including the bound App/check identity. The existing authorized
`queue_admit` request binds the expected generation digest. Hermes freezes the
generation and closure before publishing H success and revalidates them at
enqueue and G qualification; a profile change cannot silently rewrite an old H
admission. The title leaf treats this as authenticated App evidence, not a hash
that candidate JSON can authorize or a digest reconstructed from the closure alone.

Every native prefix member must agree on both digest and exact closure. The closure
must include all configured title paths with their expected kinds, including the
entire `.github` tree; additional admitted producer roots are verified too. Every
entry has exactly `kind`, `sha` and `mode`. Only tree mode `040000` and blob modes
`100644`/`100755` are supported; symlinks, submodules, unsafe paths, missing objects
and mixed generations refuse. The old v1 intake cannot authorize this path.

## Source custody and title behavior

The G workflow has only contents/actions/checks/pull-requests/issues **read**
permissions. It checks out immutable protected B into `trusted-title-source` with
credentials unpersisted. Python and JavaScript execute only from that checkout;
candidate G is fetched only into a private bare Git store. B supplies the executed
Python/JavaScript and the current configured publisher App/name/policy. The intake
supplies the separately authorized target generation at G. A reviewed G may change
the workflow tree, helpers or future policy while this run continues to execute
only B's evaluator and current trust configuration. Requiring the target closure to
equal B would prevent both the first final cutover and later governed CI changes.
The receipt retains `producer_generation=B` for execution provenance and the
separate `producer_generation_sha256` for target authorization.

The independent trusted admission/qualification owner still enforces the actual
G caller's reviewed workflow/helper closure and exact native run/attempt/jobs:
a candidate-edited workflow cannot authenticate its own decision merely by
announcing this helper passed. The title leaf's read-only result does not replace
that installed protection boundary or authorize a new current App binding.

Before the first cutover G, the additive preparation must be installed on protected
B with this v2 reader and the actual qualified App/name policy. The checked-in
policy remains disabled until that authorized native installation. The root-owned
publisher then admits the reviewed final G closure, which can differ from B.
This resolves the source-generation cycle; it does not supply the first protected
installation, native workflow registration or effective settings authority, and
it does not authorize an ordinary-lifecycle CI bootstrap.

The JavaScript preserves the prior production rule body. Its exact archived source
is `.ci/retired-workflows/pr-title-check.yml`; native Node tests compare pass/fail,
exemptions, warnings and proposed housekeeping against it. G keeps each member's
Dependabot-author, `skip-title-check` and release-stamp exemptions; a merge-group
actor cannot exempt unrelated members. The old actor-only shortcut remains explicit
for authoring callers. A missing issue reference fails; a zero-valued placeholder
passes with the same `needs-issue-link` housekeeping; a missing referenced issue
continues to warn. A real reference prepares removal of that label.

The adapter records the original deduplicated-comment and label operations, and
calls no write API. Its result returns those exact proposed operations to the
trusted owning session's existing publisher/label owner. Run and reconcile this
housekeeping with the applicable authoring checks before enqueue; it does not create
a second approval or postgreen review gate. G logs any remaining housekeeping and
writes the proposal beside its run-local receipt, without acquiring publication
authority. `needs-issue-link` changes do not change the title validator's exemption;
`skip-title-check` changes do and invalidate the observed proposition.

## Bounded verification

```sh
python3 -m unittest scripts.ci.test_native_queue_title scripts.ci.test_pr_title_validator scripts.ci.test_native_queue scripts.ci.test_staged_qualification_workflow_cutover
```

The native Git fixtures include singleton squash, ordered multi-member composition,
stacked edit/revert, omitted/extra/conflicted members and transitive producer
substitution. An executed mutant that drops consumed H ancestry must fail the
stacked fixtures. API controls cover incomplete/ambiguous membership, stale attempts,
wrong Apps, missing/copied/expired intake, same-tree H rewriting and metadata races.
The workflow contract requires protected B execution, read-only permissions, all
members, and a final currentness read with no skip/continue-on-error escape. These
are source receipts; native qualification and protected M still need actual readback.
