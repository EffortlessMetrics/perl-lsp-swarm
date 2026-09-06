# Acceptance: #11627 — read-only module live frontier

Scope of this slice: the `module_train_live.v1` snapshot join plus the four
`module-train live` subcommands, with fail-closed adapters, the pure action
classifier, the fixture corpus, and this packet. The issue's full acceptance
(checklist below) stays open where instruments do not exist yet; nothing is
guessed.

## Invariants (machine-checked)

- [x] One immutable `module_train_live.v1` snapshot joins live collaboration
      state to #11625 (manifest digest binding) and #11626
      (`LoadedManifest::node_statuses()`, semantics unmodified).
- [x] `observed_at` is outside the semantic digest; two normalizations of the
      same raw observation are byte-identical; candidate order permutation moves
      no byte.
- [x] Candidate ownership requires the explicit identity block plus manifest
      agreement; title/branch/author/labels/age/CI are diagnostics only.
- [x] Candidate states remain independent flags (never collapsed): existence,
      multiplicity, binding agreement, base/stack relation, review decision,
      checks, merge probing, dirty/unpushed unique work, instrument health.
- [x] Exactly one action per node; at most one action per writer/conflict
      surface; the action map is keyed by conflict key and asserted duplicate-free.
- [x] A viable canonical candidate is RESUME/REPAIR/RESTACK/REVIEW-ed before any
      duplicate START; two candidates are RECONCILE, ranked by nothing.
- [x] Controllers/fan-in/gates/claims bound as implementation get STOP; C02
      blocked nodes never START for absence of a PR; fan-in START requires child
      receipts (unobservable → NOT_PROVEN).
- [x] Instrument failure states (`failed`, `rate_limited`, `permission_denied`,
      `truncated`, `unavailable`) produce `NOT_PROVEN`/`instrument_failed`,
      never "no candidate", never pass. Truncation degrades precisely:
      open-window truncation gates absence-of-candidate decisions globally;
      merged-window truncation (unavoidable at this merge velocity) degrades
      only merged facts to a per-node limitation without gating viability.
- [x] MERGE_READY_RECOMMENDATION requires review currency, resolved threads and
      current receipts. Review currency and thread resolution became observable
      in #14237 (gated read-only GraphQL) and are now reported as blockers only
      when the instrument genuinely cannot bind them; current behavior receipts
      remain an unconditional typed blocker while #11619 has no producer, so the
      recommendation stays unreachable from live observation on one blocker
      instead of three. The branch is covered by synthetic-fact unit tests
      including the open-threads/core-receipt/exact-process false-greens.
- [x] Merged PRs are landed only on local-HEAD ancestry; otherwise pending
      current-tree probe. Main movement alone invalidates nothing.
- [x] All adapter subprocesses route through one read-only allowlist asserted by
      tests; network reads exist only in `refresh`; no other subcommand touches
      the network or any repository/GitHub state.
- [x] `live explain` composes static manifest node facts + C02 state + the live
      addendum (action now, why, unavailable facts and their consequence, next
      bounded action, closeout route when current).

## Shift-left falsifiers (tests written against the classifier/normalizer)

1. viable canonical candidate ignored → duplicate START: REJECTED (candidate branch wins).
2. two candidates ranked by recency/author/model/CI colour: REJECTED (RECONCILE; order-permuted bytes identical).
3. controller bound as implementation: REJECTED (STOP).
4. static-blocked leaf starts because no PR exists: REJECTED (BLOCKED with C02 reasons).
5. checks/proof/review on H1 satisfy moved H2: REJECTED (review currency never asserted; synthetic stale facts → REVIEW, no transfer).
6. merged PR absent from current tree called landed: REJECTED (pending-probe state).
7. issue closure/labels override C02 probes: REJECTED structurally (issue state not observed; fixture with stray issue data classifies identically).
8. wrong-base/malformed stack accepted: REJECTED (RECONCILE wrong_dependency_or_stack_relation).
9. dirty/unpushed unique work disposable: REJECTED (RECONCILE, unique-work reason).
10. one writer/conflict key allocated twice: REJECTED (action map keyed by conflict key, duplicate-free; duplicate binding → single RECONCILE).
11. fan-in/retirement starts while a hard-dep candidate is nonterminal: REJECTED (WAIT hard_dep_candidate_nonterminal).
12. fan-in starts without child receipts: REJECTED (NOT_PROVEN child_receipts_not_observable).
13. core receipt hides edit-profile non-pass: REJECTED (synthetic merge-ready blocked on edit profile).
14. exact-process evidence becomes broader support truth: REJECTED (receipt kinds evaluated independently).
15. API permission/rate-limit/truncation becomes absence/pass: REJECTED (instrument states → NOT_PROVEN).
16. unresolved threads/stale review omitted from merge-ready: REJECTED (threads/resolved is a required fact; false → NOT_PROVEN).
17. snapshot/main movement invalidates action: REJECTED (main movement alone changes no action).
18. any code path attempts GitHub/repository mutation: REJECTED (single choke point + allowlist assertion).

## Verification (this slice)

```text
cargo test -p xtask --locked --bin xtask module_train_live   -> all green
cargo xtask module-train live refresh --from-fixture <raw> --output <a>   (x2, byte-identical)
cargo xtask module-train live check   --snapshot <a>
cargo xtask module-train live next    --snapshot <a>
cargo xtask module-train live explain <fixture-node> --snapshot <a>
cargo xtask module-train live refresh --output <live.json>  (network; instruments recorded)
cargo fmt -p xtask -- --check
cargo clippy -p xtask --all-targets --locked -- -D warnings
git diff --check
```

## Non-goals (restated)

No scheduler, lease/work database, agent launch, branch/PR/review/comment/merge/
close/release/publication/support mutation, no product behavior change, no
semantic review engine, no mutable frontier store.
