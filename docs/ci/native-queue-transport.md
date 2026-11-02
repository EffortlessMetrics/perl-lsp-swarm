# Native queue transport

Owner: #17900 / #10058. Source implementation, **not deployed admission or queue activation**.
The controlling transition remains [qualification-cutover.md](qualification-cutover.md).

`scripts/ci/native_queue.py` implements the single-PR native transaction missing
from the legacy manual-dispatch adapter. It uses the existing `gh` authentication,
not a service, new credential, workflow dispatcher, or replacement readiness engine.
The legacy `request_qualification.Adapter` and its consumers are unchanged.

## What is implemented

`Adapter.observe()` and `recover()` acquire one exact PR and its native queue
entry. They bind the repository node/name, PR, expected H, target branch and
same-repository head. They distinguish absent custody, an armed future auto-merge,
actual native entry, closed PR, and observed merge. Queue entry head/base OIDs
remain separate from H and may be absent before construction. This observation
does not enumerate a merge group's complete membership or qualify its tree.

`Adapter.enqueue(authorize=...)` requires an explicit trusted in-process authority
callback returning literal `True`. The caller must freshly establish the existing
eligibility, conditional landing authorization, appropriate ordinary-PR profile,
all required H producers, and qualified source/enforcement/publisher generation.
The callback is a trusted code seam, not a self-authenticating object or receipt.
Candidate JSON, a CLI flag and an environment variable cannot install it here.

After that judgment, the adapter reacquires the native snapshot, then makes one
`enqueuePullRequest` mutation with `expectedHeadOid` and `jump: false`. It never
calls direct merge, enables auto-merge, changes workflow state, or publishes checks.
An already observed entry or terminal state is reported without a new mutation.

A fresh readback, not the mutation response, establishes current custody. Lost
responses get one read-only reconciliation, never an automatic second mutation.
This is not exactly-once execution across process restarts.
Absent or contradictory custody remains `ENQUEUE_NOT_CONFIRMED`; an absent entry
cannot prove that an earlier request never executed or was not later ejected.
A moved queue or observed same-H metadata change after submission is surfaced for
the existing owner to withdraw/disposition, not silently treated as cleared.
`MERGED_OBSERVED` does not establish that this request caused a protected landing.
Qualification and landing remain `NOT_PROVEN` in every transport result.

This is not atomic enforcement of all mutable labels, findings or review threads.
Those existing owner/invalidation controls must be joined before live use. The
scalar snapshot comparison protects only the fields actually acquired here. It
does not replace the complete protection observer or invent a new policy catalogue.
Unrelated base-tip movement is not itself a changed H or mandatory rebase.

## Read-only CLI

From the source tree, with the expected identity supplied by current evidence:

```sh
python3 -B -m scripts.ci.native_queue observe \
  --repository "$REPOSITORY" --repository-node-id "$REPOSITORY_NODE_ID" \
  --pr "$PR" --expected-head "$H" --base-branch "$BASE_BRANCH"
```

`recover` has the same inputs and is read-only even when no entry is found. Exit
zero means observation completed, not that qualification passed or a merge is
permitted. No CLI enqueue/authorization switch is installed. Production write
binding still belongs to #17900/#10057 and the accepted #17080/#10065 transaction.

## API choice and separate routes

The immediate GraphQL mutation is selected because it errors when immediate
admission is impossible instead of arming a future auto-merge. The expected-head
field prevents a request for H from silently targeting H2. Native acceptance of
that contract still needs an authorized repository canary; local fixtures do not
establish GitHub's deployed behavior.

The asynchronous REST merge API is not a fallback here. Its default action can
merge directly, and stacked requests have broader downstack scope and a separate
pending/result lifecycle. Native stacked PRs are explicitly refused by this
single-PR writer; a separately qualified profile must own their authorization and
async recovery. History-preserving sync and release authority remain separate.

Sources checked November 2, 2026:
- [Immediate queue admission versus deferred auto-merge](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/merging-a-pull-request-with-a-merge-queue)
- [GraphQL inputs, PR queue fields, and native entry states](https://docs.github.com/en/graphql/reference/pulls)
- [Asynchronous merge action and result semantics](https://docs.github.com/en/rest/pulls/pulls)
- [Stack mutation scope and API boundary](https://docs.github.com/en/pull-requests/reference/stacked-pull-requests-apis-and-webhooks)

## Local acceptance and remaining installation

```sh
python3 -B -m unittest scripts.ci.test_native_queue -v
```

The test invokes the production transport through an actual fake `gh` executable,
plus direct transaction controls and wrong-behavior mutations. It does not contact
GitHub. The native graph, product proof and authority callback are explicit
fixtures. The existing advisory command includes this module; no trigger, job,
permission, runner or required context is added.

Remaining acceptance is the trusted owner binding, complete G producers and
selected-obligation fan-in, real expected-App/H check evaluation, metadata
invalidation/withdrawal, and the protected first installation. Do not mark #17900
closed, open an ordinary CI-triggering PR, or activate settings because this
transport's local tests pass.
