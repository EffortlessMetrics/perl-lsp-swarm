# Implementation Checklist: #11627 — read-only module live frontier

## Current state

- [x] `.spec/11627-module-train-live/` bundle (this file's siblings) written
      before implementation.
- [x] `xtask/src/tasks/module_train_live.rs`: raw-observation model, read-only
      adapters (git local / git remote / gh pr list+view, plus one gated
      `gh api graphql` review read, through the single allowlisted choke
      point), deterministic normalizer, pure action
      classifier, check/next/explain renderers.
- [x] Additive public seam on #11626's module: `LoadedManifest::node_statuses()`,
      `node_static_facts()`, `controller_issue()`, public
      `CurrentTreeState::as_str` (no semantic change; C02 suite re-run green).
- [x] `module-train live refresh|check|next|explain` wired into the xtask CLI.
- [x] Fixture corpus under `xtask/tests/fixtures/module-train-live/`
      (`raw-corpus.json` with 11 PRs covering every candidate state family,
      `raw-clean-surface.json` for the START baseline).
- [x] 46 focused tests in `xtask/src/tasks/module_train_live_tests.rs`: all 18
      shift-left falsifiers, determinism, plus the bot-review repair tests
      (repo-bound gh queries, fail-closed detail reads, partial-trailer node
      retention, manifest-digest validation binding, cancelled checks carry
      no verdict),
      candidate-order permutation, `observed_at` outside the semantic digest),
      digest/action tamper detection, precise truncation degradation,
      privacy (bodies never stored).
- [x] Non-rust inventory regenerated over the final tree.

## Proof (scoped; run on the final tree)

```text
cargo test -p xtask --locked --bin xtask module_train_live -> 46 passed
cargo test -p xtask --locked --bin xtask module_train      -> 58 passed (C02 regression)
cargo fmt -p xtask -- --check                              -> clean
cargo clippy -p xtask --all-targets --locked -- -D warnings -> zero findings in this PR's files
cargo run -q -p xtask --locked -- module-train live refresh --from-fixture <raw> --output <a>  (x2)
  -> byte-identical (cmp)
cargo run -q -p xtask --locked -- module-train live check|next|explain C03 --snapshot <a>
cargo run -q -p xtask --locked -- module-train live refresh --output <live.json>  (network)
  -> instruments ok; merged-window limitation recorded; START frontier C02/E00A/M01/M07A;
     C03 BLOCKED on C02 (honest: C02's presence probe is a #11626 residual)
git diff --check -> clean
```

## Residuals (recorded on #11627; not proven here)

1. Review-thread observation — **closed by #14237**: `threads_resolved` is
   observed through one gated read-only `gh api graphql` document and fails
   closed (unobserved or truncated page, a head that moved between the list and
   the review read, or any GraphQL instrument failure leaves it unprovable,
   never "resolved").
   Review-head binding — **partially closed, deliberately**: #14237 observes
   whether each opinionated review sits on the head commit
   (`reviewed_commit_is_head`, from `latestOpinionatedReviews` so advisory
   comments do not distort it), but that comparison is a **diagnostic only**.
   Semantic review currency is NOT derived from it: `REVIEW_CURRENTNESS.md`
   ("Review is semantic, not exact-head") and `AGENTS.md` ("head SHA change
   alone -> no review invalidation") make a head SHA an invalid review-validity
   token, and materiality is not observable here. So
   `review_head_currency_not_observable` remains a typed blocker and
   `head_moved_after_review` is never raised from a commit delta.
   MERGE_READY_RECOMMENDATION therefore stays unreachable on two blockers
   (currency + receipts), not one.
2. Behavior-receipt/profile observation: typed blocker for fan-in/claim starts
   and merge-ready. **Blocked by #11619** (P11A exact-process receipt
   substrate, open): this tree has no `module-process` task and no
   `module_resolution_composition.v1` schema, so the receipt kinds have no
   producer to observe.
3. Explicit stack parsing (`explicit_stack_member`) and cross-PR base/head edge
   validation: fail-closed reserved vocabulary (`stack_relation != "none"`
   fails closed).
4. Supersession / `SUPERSEDE_RECOMMENDED` / `RETURN_TO_ISSUE` live reachability.
5. Issue-state observation: deliberately excluded (closure/labels are never
   authority; fixtures carry stray issue payloads to prove non-interference).
6. #11626 `explain` static packet composition when its residual lands.
7. `#11106` shared-authority extraction if a second consumer appears.
8. Merged-window truncation is permanent at current merge velocity for any
   bounded window; merged-candidate facts degrade to a recorded limitation
   (honest bound, not a completeness claim).

## Adoption / rollback

Adopt via the four `module-train live` subcommands (`refresh` is the only
networked call; everything else is offline from the immutable snapshot).
Rollback = revert this PR; C01/C02 artifacts are untouched (one additive
accessor aside, which carries no semantic change).
