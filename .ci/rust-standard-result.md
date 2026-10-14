# Owned Rust Small policy and check migration

This candidate continues PR16134 at ac2cddf1ab10c58edd38dbcce14dccf271a0503e.
It invokes the canonical Rust Small proof once through public Rust workflow
`a3125de962a74e7ec2e127d41d7532f66c69334d`, with the repository policy in a
separate disposable owned job and the final aggregate in a fresh immutable job.
There is no consumer-hosted result wrapper. This candidate is not activated:
the old required context remains enforced until the settings transaction below.

## Exact policy and evidence

The policy validates central v1 proof evidence against the native repository,
GITHUB_SHA, run ID, attempt and standard profile. For pull_request, GITHUB_SHA is
the tested merge commit; the main-red probe separately queries checks on the
webhook PR head. For merge_group, both probe subject and effective workflow tree
are its head SHA. Push retains the prior non-applicable candidate-vs-main
refusal behavior. Admitted manual dispatch binds both probe subject and workflow
tree to reviewed_sha, equal to GITHUB_SHA, and runs refusal for that source. An absent, failed, cancelled, stale or ambiguous proof cannot
enter the policy. A draft skips the entire reusable call before allocation;
there is no old draft-result job left to mirror an unrelated run's success.

The shell preserves the previous trusted-main classifier, exact source reads,
50 polls of 30 seconds, final fail-closed refusal, and the existing bootstrap/API
warning semantics. Only transport changed from gh to a bounded Python GET reader.
Before wiring, the extracted shell was byte-compared to the inline probe at the
ancestor above after those mechanical substitutions. A fixed digest now protects
that reviewed extraction; event-binding and refusal tests exercise the real
policy entry point. RIPR's separate draft mirror is retained and tested.

EM_CI_SELECTED_PROOF_RESULT=success is an internal prerequisite, not a completed
call or final success. The immutable central aggregate independently evaluates
the proof and requires native policy-job success before creating/uploading its
receipt. Central native negative run 36971009614 proved that policy exit 43 yields
failed aggregate and no receipt; successful run 36969479066 proved exact-subject
receipt and zero hosted execution across all four allocated jobs.

The wrapper makes a disposable Python environment under RUNNER_TEMP, installs
PyYAML 6.0.2 as a binary wheel, runs the prevention/evidence/refusal tests, and
executes the policy. uv, Python and git were observed in the actual owned canary.
The environment/cache is removed on exit; package availability fails closed.
Cold local setup previously took 23.6 seconds and tests 18 seconds; this is real
owned work, not a claim of zero overhead or a Contabo performance guarantee.

## Required-check transaction, pending native qualification and approval

Live main ruleset 16664791 was read on 2026-10-02 at 05:57 UTC. Its five contexts are
all bound to GitHub Actions integration15368. The exact current set is:

- Compile All Targets (bit-rot guard)
- Conflict marker check
- validate-title
- Perl LSP Rust Small Result
- ripr+ New Gap Gate

The proposed set changes only the fourth entry to
`Perl LSP Rust Small governed proof / Rust CI / Required`, retaining app 15368.
This spelling is a prediction from the caller/central names; the candidate's
native check-run name, app ID, head SHA, run and attempt must be read back before
requesting the exact settings change. The central proof receipt must bind the
tested merge SHA, and the native PR check must be observed on the intended PR
head. Neither a draft skip nor an old-head success qualifies this transaction.

The new context is initially nonrequired while this PR is unmerged. The old
required context's missing producer deliberately blocks integration; it must not
be faked by a successful no-op or a check created through the Checks API. The
single proof also qualifies the new context, so no duplicate full build is needed.
Main-red refusal must pass unchanged before promotion; a real inherited failure
is a blocker, not a reason to weaken the guard.

At action time, inspect both rulesets and classic protection in authenticated
settings. The connector's classic-protection read returned 403, so no complete
protection-union claim is made here. Obtain explicit approval for the exact
before/after set once native evidence exists. Prefer one atomic replacement if
the supported settings surface allows it. Otherwise add the verified new bound
context, verify both are required, then remove only the old context and verify
the final set. Never remove the old protection first. Update the declaration
through its owner to reflect the approved enforcement. This candidate already
assembles the proposed required-checks.toml row and matching documentation/tests;
the explicit candidate notice distinguishes it from current live settings.
The exact proposed row is OWNED_RUST_DRAFT_ROW in the existing enforcement
validator tests, backed by the independently fetched immutable callee YAML.
Its insertion replaces the old context row/job mapping together with the
workflow-shape row, required-check documentation and native
quality_ci_wiring_policy context assertion. These assembled edits change tested
source bytes; one deliberate final qualification must cover this complete
candidate, and earlier native proof does not cover it.

Before protected merge, re-read current head/base, check app/subject/attempt,
all other required checks and review currentness. PR #17077 is withdrawn and
its title-check proposal is not an active dependency. #9603 owns the broader
event-boundary work. This caller admits costly proof only for merge_group or
an explicit pre-merge dispatch with pre_merge_admission=true and reviewed_sha
equal to the dispatched full SHA. Draft/Ready/synchronize and landed pushes
allocate no proof; their skips are not verification. The parent coordinates
admission after source review. Do not dispatch as part of draft publication.

## Rollback

Keep public 92c170b052d046c3c98acf8a1a4aab4510dda42d admitted for rollback.
Restore the former caller/hosted result wrapper from PR16134 ancestor ac2cddf1
and qualify its old context on the rollback subject. With explicit settings
approval, replace the new context with the verified old app 15368 context
atomically, or add old before removing new. Revert the consumer migration only
after the rollback producer and protection agree. No image, runner-group,
credential or policy weakening is part of this source candidate.

## Continuation and bounded qualification — 2026-10-14

The existing branch incorporates main `4cc7960e61b685ce3b7e4cb9fe43b929bf900972`
without rewriting its published lineage. Current-main product proof, efficient
artifact admission, trusted replacement filter and RIPR exporter are retained.
The retired result job's cancellation controls remain executable in
`scripts/ci/fixtures/rust-small-result-main-4cc7960e.yml`, extracted verbatim
from that main revision. They qualify recovery diagnostics only; they do not
claim the new central aggregate emits the same messages. Central cancellation,
missing proof and earlier-attempt evidence remain blocking.

Independent upstream retrieval on 2026-10-14 reconfirmed blob
`6d202cfd6861bb359381dfb128c1574c04505285` and the recorded SHA-256. Standard
proof has a 75-minute timeout and `CARGO_TARGET_DIR=$RUNNER_TEMP/cargo-target`.
The callee's primary toolchain is 1.98.1; this repository requests its exact
1.95.0 compatibility toolchain and the callee verifies the executable version.
The caller's `RUST_VERSION` alone is not that guarantee.

Admission owner: the coordinating parent, after focused source review. No run
is dispatched by this continuation. Bound qualification must record:

1. The reviewed PR head, current base, actual integration SHA and tree. Rebuild
   the integration subject if a material conflict or interaction changes it.
2. A native merge_group execution on standard capacity using the pinned caller,
   canonical `rust-small-proof` and repository policy. An explicit dispatch
   requires `pre_merge_admission=true`, `reviewed_sha` equal to its ref SHA and
   `force_target=auto`; the policy binds refusal to that exact source. This is
   preparatory proof and does not establish native PR/merge-group association.
3. Terminal proof, policy and immutable aggregate outcomes, uploaded exact
   subject envelope, run ID, attempt, native nested check name, app and subject.
   Failed-job-only recovery is insufficient: rerun the complete supported graph.
4. Current other required checks, main-red refusal, fresh review and authenticated
   complete protection union before a separately approved settings transaction.

The em-ci #228 source grant repair and parent-reported live group application
are admission evidence, not consumer qualification. Full canonical configuration
remains NOT_PROVEN where organization variables/package reads are unavailable.
No access, credentials, protections, required checks or release settings change
in this continuation. Rollback still requires matched producer and protection.
