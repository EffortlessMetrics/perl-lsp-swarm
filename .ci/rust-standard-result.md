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
are its head SHA. Push/manual retain the prior non-applicable candidate-vs-main
refusal behavior. An absent, failed, cancelled, stale or ambiguous proof cannot
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
receipt. Central native negative run36971009614 proved that policy exit43 yields
failed aggregate and no receipt; successful run36969479066 proved exact-subject
receipt and zero hosted execution across all four allocated jobs.

The wrapper makes a disposable Python environment under RUNNER_TEMP, installs
PyYAML6.0.2 as a binary wheel, runs the prevention/evidence/refusal tests, and
executes the policy. uv, Python and git were observed in the actual owned canary.
The environment/cache is removed on exit; package availability fails closed.
Cold local setup previously took23.6 seconds and tests18 seconds; this is real
owned work, not a claim of zero overhead or a Contabo performance guarantee.

## Required-check transaction, pending native qualification and approval

Live main ruleset16664791 was read on2026-10-02 at05:57UTC. Its five contexts are
all bound to GitHub Actions integration15368. The exact current set is:

- Compile All Targets (bit-rot guard)
- Conflict marker check
- validate-title
- Perl LSP Rust Small Result
- ripr+ New Gap Gate

The proposed set changes only the fourth entry to
`Perl LSP Rust Small governed proof / Rust CI / Required`, retaining app15368.
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
settings. The connector's classic-protection read returned403, so no complete
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
all other required checks and review currentness. Preserve late-CI admission
from PR17077 when combining these candidates; that branch does not authorize
required-context migration. Broader cheap-ready/heavy-merge_group policy remains
separate and is not changed here.

## Rollback

Keep public92c170b052d046c3c98acf8a1a4aab4510dda42d admitted for rollback.
Restore the former caller/hosted result wrapper from PR16134 ancestor ac2cddf1
and qualify its old context on the rollback subject. With explicit settings
approval, replace the new context with the verified old app15368 context
atomically, or add old before removing new. Revert the consumer migration only
after the rollback producer and protection agree. No image, runner-group,
credential or policy weakening is part of this source candidate.
