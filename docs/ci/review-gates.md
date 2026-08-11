# Review in perl-lsp-swarm

## Primary review path

Serious review is part of the normal Claude/Codex development method. It does
not depend on an external reviewer being available.

For substantive work, the repository expects proportionate multi-round review:

```text
issue and premise challenge
→ proof and oracle challenge
→ implementation self-check
→ test hardening and simplification
→ mutable candidate challenge
→ findings and repair
→ useful submitted PR review
→ integration and landed-state reconciliation
```

Reviews should preserve useful scope, evidence and falsifiers, findings or an
explicit clean result, prior dispositions, what is established, residual risk,
and the next action. A later commit refreshes only affected semantic seams; it
does not erase useful prior review merely because the SHA changed.

Branch protection, required deterministic checks, unresolved findings, and the
accountable integration judgment remain authoritative. No external AI reviewer
is a substitute for that process.

The RIPR boundary follows the same split: the `ripr` sensor is advisory, but the
`ripr+ New Gap Gate` required check owns deterministic receipt integrity and
blocks named new gaps in changed production files. A merge-test ref is recorded
as `evaluated_head_sha`; pull-request runs separately record and validate
`pr_head_sha`. Missing or stale identity-bearing receipts are not clean results.

## Opportunistic external reviewers

CodeRabbit and Gemini are welcome extra evidence when they produce useful
findings. They are opportunistic:

- rate limits, quota exhaustion, unsupported files, or no response are ordinary
  availability facts;
- we do not wait, push empty commits, or build retry machinery for them;
- absence of output is not approval and does not block merge;
- useful findings are handled through the normal review/disposition path;
- generic summaries and boilerplate carry no special authority.

## Droid / Factory

Automatic Droid review is paused.

The lane repeatedly failed or produced incomplete review state and did not earn
a durable role in the review stack. The workflow remains as a statically skipped
historical surface so existing policy and receipts stay readable.

A future bounded experiment may reconsider Droid only when a current service
version is stable and demonstrates material review value beyond the normal
multi-round agent process. Until then, do not spend runner/model capacity on it.

## UB Review

Automatic UB Review is paused until the tool is useful.

UB Review may be relatively close to a valuable product, but the present swarm
integration produces too much boilerplate, unreliable sensor/publication state,
and insufficiently focused reviewer value. Running it on every PR consumes CI,
model, and maintainer attention before the product has earned that cost.

Development continues in `EffortlessMetrics/ub-review`. Re-enable the swarm
workflow only after real PR dogfood establishes all of the following:

1. **Useful reviewer output** — concise, relevant findings or a genuinely useful
   clean conclusion; little irrelevant machinery narration or duplicate noise.
2. **Good investigation** — reconstructs the PR claim, production consumers,
   proof, negative/fallback paths, and realistic counterexamples.
3. **Reliable operation** — sensors, models, packet assembly, and publication
   complete consistently; failures remain visible rather than turning into
   clean/pass results.
4. **Separate deterministic gate** — required CI evidence is evaluated by
   explicit receipts and rules, not solely by a model verdict.
5. **Measured value** — representative dogfood shows acceptable precision,
   recall, noise, latency, cost, and correction load compared with the existing
   review-forward process.

Until those conditions are met, UB Review is neither advisory PR traffic nor a
merge gate in this repository. Its upstream development and calibration are the
work; repeatedly running the unfinished integration is not.

## Re-enabling an external reviewer

Re-enabling Droid or UB Review requires one bounded PR with:

- the exact tool/action version;
- representative before/after dogfood;
- useful-output examples;
- failure and unavailable-state behavior;
- cost/latency evidence;
- a clear advisory or required boundary;
- an explicit rollback.

Do not create a provider-neutral review state machine merely to normalize bot
availability. The repository already has a serious review process; external
reviewers must add value to it rather than become another control plane.
