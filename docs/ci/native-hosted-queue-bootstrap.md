# Server-free native queue source bootstrap

Status: isolated source candidate. Native acceptance, protection migration,
20-minute compliance and literal-zero prequeue starts are NOT_PROVEN.
No Contabo, SSH change, custom signer or new App grant is required by this design.

## Prerequisite and trusted-checker preparation

This source carrier explicitly depends on #18104's publication-only hooks at
`c60b999b12d4ae2191f47f910841178097f3cd95`, integrated without rewriting history.
It does not include or activate the held #19261 queue-only train. Publication
must use the normal reviewed hooks and existing protection; this dependency is
not permission to bypass either.

Alongside the four additions below, this carrier changes only
`xtask/src/tasks/file_policy.rs` relative to that merged prerequisite base. The
live Non-Rust workflow stays byte-identical at v9. The trusted checker strictly
preapproves the exact reviewed queue-only/protected-main v10 carrier, including
its bootstrap-compatible guard, before that workflow can be activated later.
Candidate code or fixtures cannot authorize themselves. Unchanged v10 acceptance
is limited to the exact compiled-in reviewed-current contract; versions 10 and
above cannot fall back to the legacy shape scanner.

The later workflow cutover must deploy those exact approved v10 bytes and retain
reviewed-current-v10 acceptance while retargeting next preapproval to v11. A
subsequent v11 landing must explicitly promote reviewed-current-v11 acceptance
and retarget next preapproval to v12. Merely changing the next fixture is not
proof of a future steady state.

## First source landing: inert immutable evaluator

Add these inert files, alongside the trusted-checker preparation above, under
existing protection and ordinary review:
- .github/workflows/queue-admission.yml (workflow_call only)
- scripts/ci/hosted_queue_admission.py
- scripts/ci/test_hosted_queue_admission.py
- this document

Do not change existing workflow triggers, title policy, check names, protection
requirements or queue routing in this first landing. The evaluator has no
independent trigger and no caller is installed by this subset. It reuses the
existing scripts/ci/check-pr-semantic-review-currentness.py from its pinned source.
That dependency must be present and reviewed in the first landing's real commit.
Its closure path inventory is explicit in source, not read from a future policy.
The first landing need not qualify or execute product tests on an H by this
new route; it must satisfy the repository's existing landing requirements.
An isolated source test is not permission to bypass those requirements.

Record the actual protected landed commit SHA. Never use an all-zero or invented
SHA. This is the immutable reusable code pin for the second source landing.

## Second source landing: prepared integration, not automatic activation

The remainder of the candidate is separate: native Actions intake adapter,
complete native title consumer, five reusable proof families/six required leaves,
actual-G root, tests, and disabled native transport policy. Existing direct event
roots and required-check names change here, not in the first subset.
The shipped queue-admission-request.yml is deliberately inert workflow_call-only
and local-reference; immutable-reference authentication refuses this local call.
Only a reviewed follow-on can replace it with the real first-landed SHA and
explicit pull_request_target labeled/main trigger. Bind the protected policy to
the identical real SHA. Do not enable the policy until native canaries and
approved protection migration make the complete installed route coherent.

The named admission job proves exact-H review only, never compilation/Rust/RIPR
success. Its native Actions artifact is authenticated by run/event/path/repo,
immutable referenced_workflows path+SHA, exact current attempt job/check, and
artifact run/head/name/digest. The title leaf independently observes the complete
native queue prefix and proves ordered B+H tree equality with actual G. Every
member must have the same admitted producer closure, which must equal G's exact
objects/modes. This preserves the previous strict closure constraint; arbitrary
mixed-generation stacks are not newly supported. Native queue entry observation
is not a claim that an agent verified an enqueue acknowledgement.

Newest admission is selected by unique native workflow run_number for H, then
that run's current attempt. No old success fallback follows newer queued, failed,
cancelled, partial-rerun, missing-artifact or wrong-generation admission. Complete
lists paginate up to1000 items; unstable, truncated or over-bound lists refuse.
Review convergence uses latest decisive state per author; ordinary comments do
not clear a change request. Canonical exact-H substantive review still must pass.
Independent review work and root publication can share the PR author's GitHub
login. Account inequality is not an additional admission requirement. The existing
canonical cumulative-review procedure owns review independence; publication identity
alone neither proves independent review nor permits author self-certification.
Current write/maintain/admin permission and canonical exact-H review/digest remain
mandatory. No alternate self-certification marker or metadata is introduced.

## Independent native enforcement decision remains open

Actions App15368 plus a check name does NOT enforce reviewed workflow identity.
A candidate G can replace or omit its verifier and emit a same-named success.
Internal run/receipt/closure checks cannot enforce a verifier that was omitted.
Canary success and matching H/G names alone do not close this security boundary.
An independent required-workflow rule or equivalent external enforcement must be
qualified before activation. No such rule, complete protection union or org-plan
capability has been established here.

GitHub's required-workflow rules support pull_request_target and merge_group but
ignore event type filters; their PRT behavior uses opened/synchronize/reopened.
Therefore making this label-only design a required workflow is not a transparent
installation step: it conflicts with the desired ordinary-development zero-start
lifecycle and requires an explicit architecture/efficiency decision. No rule or
permission changes are part of either source subset.
Official source:
https://docs.github.com/en/enterprise-cloud@latest/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets#require-workflows-to-pass-before-merging

## Required compatibility and native acceptance work

This candidate does not silently change the required-workflow catalog, old
Hermes-specific tests, or docs to make them green. Their source contracts need an
explicit coherent migration before the second subset is merge-qualified.
Non-Rust v10 workflow bytes are untouched. Existing protections remain in place
until a specifically approved equivalent migration; full additive protection
union is not inferred from a subset. Do not represent source changes as installed.

Canary the labeled immutable caller, run/check/job names, immutable reference,
artifact archive digest, exact H vs caller T, moved H, reviewer revocation, reruns,
missing/failed leaves, cancelled/partial G reruns and multi-member composition.
Verify actual runtime permissions and caller/reusable event contexts. H reusable
job prefix and ordinary G finalizer currently differ structurally. A shared required
context name is NOT_PROVEN; align publisher job structure or verify exact names
before proposing any protection change. Repeated YAML name text is not evidence. Current
repository target-event metadata proves H-bound runs/checks/artifacts, but the new
labeled/reusable route has not run. Official REST workflow-runs docs show SHA-pinned
referenced_workflows entries with path and sha and no ref; branch/tag references
have ref. A claimed receipt T is not enough without matching native referenced
workflow identity and protected caller source.

## Timing and cost: explicit blockers, not claimed compliance

Steven's limit is no CI run longer than20 minutes. The preserved declarations
are NOT compliant with that limit: RIPR135+12=147 minutes, Rust Small60+35=95,
CI's longest declared dependency chain1+2+75=78, title10, contract3. Root finalizer5
makes the parallel-composition declared critical-path exposure152 minutes, before
runner scheduling/queue delays. These are timeout ceilings, not measured runtime.
Dynamic matrix runner-minutes, real elapsed G duration and billed cost are not
proved. No proof obligation or timeout was weakened to manufacture20-minute
compliance. A separately reviewed deadline/efficiency repair remains prerequisite.

First subset produces no new automatically triggered runs. Later explicit
admission is a transitional prequeue hosted metadata job, not literal-zero
prequeue CI. Every labeled event allocates one cheap metadata job; wrong label/Draft
requests fail, not skip. A skipped required H check could otherwise count as success. Count workflow starts, skipped jobs, allocated jobs, elapsed
runtime and runner-minutes separately. Ordinary Draft/Ready/synchronize must not
trigger substantive product proof. Native merge limits do not combine CI builds;
actual multi-memberG membership and behavior require native evidence.
