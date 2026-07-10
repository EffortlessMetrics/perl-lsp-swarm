# Fresh Facts Fast — Done-Condition Proof Suite

**Status:** living spec · **Program:** Fresh Facts Fast (off-lock async parse worker, #3396) · **Executes AFTER:** #3618 merges (off-lock parse worker + `Weak` Arc-cycle / self-join fix)

This document is the durable, turnkey definition of "done" for the Fresh Facts
Fast program. It names the deterministic tests, real-Perl editor canaries, and
receipts that — taken together — *prove* the program complete, binds each to the
substrate seam it exercises (file:line), inventories what already exists versus
what is still a gap, and gives a builder a turnkey design for every gap so it can
be closed without re-researching the substrate.

> **What "done" means here.** The program is done when the invariants below are
> each proven by a *deterministic* (barrier/channel/shutdown-signal, never
> sleep-based) test that actually **runs** (not a false-green 0-test behind an
> unset feature gate), plus real-Perl provider honesty canaries and the Neovim
> latency receipt, all as durable repository artifacts. Completion is tracked on
> the per-condition Implemented/Covered/Gap axis in the checklist at the end —
> not a single "done" flag.

> **Branch status (verified 2026-07-10 against `origin/main` @ `c93911bf0` and
> PR #3618 @ `209b9cf56`, both open/unmerged for #3618).** `crates/perl-lsp-rs/src/runtime/parse_worker.rs`
> **does not exist on `origin/main`** — it, and the `LspServer::run_post_parse_side_effects`
> / `commit_parse_effect_if_current` / `document_generation_still_current` oracle
> in `text_sync.rs`, ship only on the still-open PR #3618 ("perf(lsp): latest-only
> off-lock parse worker"). Every section below whose "Existing coverage" cites
> `parse_worker.rs` or those `text_sync.rs` functions — **§0's seam table, §1, §2,
> §3a, §3b, §4, §5, §6, and the worker-metrics assertions in §8** — describes
> that branch, not a present-tense inventory of `origin/main`. It is an accurate
> *landing plan* for #3396/#3618, not a claim that these tests run today outside
> that branch. **§7 (provider honesty canaries) is added BY THIS PR** — it lives
> on this PR's branch (`test/done-condition-canary-gaps`) and becomes an
> `origin/main` fact only once this PR merges, not before; do not read "COVERED"
> in §7/§7a/§7b as an existing-on-`main`-today claim while this PR is still open.
> What genuinely predates this PR **and is on `origin/main` today** is the
> substrate §7 exercises: `DocumentState::current_parsed` / `latest_parsed` /
> `publish_parsed_if_current` (`document.rs:450/429/469`) and
> `test_apply_text_change_without_reparse` / `test_publish_parse_for_current_generation`
> (`test_api.rs:161/201`). The base `ux_neovim_ranged_typing_medium_file_receipt`
> test in §8 (minus the worker-specific fields) also predates this PR and is on
> `main` today. Re-verify this banner's SHAs, and this PR's merge state, before
> trusting any "COVERED" verdict below as a main-branch fact rather than a
> branch-pending one.

---

## 0. Substrate seam map

Every proof below binds to one of these production seams. **None of the
`parse_worker.rs` / `text_sync.rs`-oracle rows in this table exist on
`origin/main` today** — line numbers are against PR #3618 (open, unmerged) at
head `209b9cf56` (mechanisms stable since `pr3618-final` = `e5b6c2dca`,
an ancestor of that head; re-grep the symbol if a line has drifted further).
The `document.rs` row is the exception: it is on `origin/main` now.

| Seam | Location | On `main`? | What it guarantees |
|------|----------|:----------:|--------------------|
| `Coordinator::enqueue` (coalesce-replace, single lock) | `crates/perl-lsp-rs/src/runtime/parse_worker.rs:309` | ❌ PR #3618 only | At most one pending job per URI; `jobs_coalesced` bumped at `:323` |
| `Coordinator::take_next` (atomic pop from `ready`+`pending`) | `parse_worker.rs:330` | ❌ PR #3618 only | No TOCTOU orphan between the ready queue and the pending map |
| `Coordinator::finish` (re-queue latest / release URI) | `parse_worker.rs:354` | ❌ PR #3618 only | Newer edit that landed mid-parse is re-queued; URI released otherwise |
| `catch_unwind` + `FinishGuard` in the worker loop | `parse_worker.rs:650`–`673` (guard type `:406`–`415`) | ❌ PR #3618 only | A panicking job never orphans its URI or shrinks the pool |
| `Weak<LspServer>` downgrade in `on_published` (#3618, breaks the Arc cycle) | `crates/perl-lsp-rs/src/runtime/mod.rs:1192`–`1202` (`install_default_parse_worker`); `ParseWorker::drop`'s unconditional join loop is `parse_worker.rs:752`–`759` and has no self-join guard of its own — see §3b Correction | ❌ PR #3618 only | The callback only ever holds a transient strong ref, so the server's strong count can reach zero without a worker thread being forced to join itself — narrows, but per §3b does not structurally prove closed, the self-join-from-callback-thread window |
| `process_job` publish transaction (`Arc::ptr_eq` + `publish_parsed_if_current`) | `parse_worker.rs:832`–`845` (`Arc::ptr_eq` at `:842`) | ❌ PR #3618 only | Document-instance identity + generation freshness gate, single lock acquisition |
| `DocumentState::publish_parsed_if_current` | `crates/perl-lsp-rs/src/state/document.rs:469` | ✅ on `main` | A stale-generation snapshot publishes nothing |
| `DocumentState::current_parsed` (freshness-correct read) | `document.rs:450` | ✅ on `main` | Returns `None` when the last published snapshot is older than the text generation (the pending-parse gap) |
| `LspServer::run_post_parse_side_effects` | `crates/perl-lsp-rs/src/runtime/text_sync.rs:1058` | ❌ PR #3618 only | Deferred side effects (symbols, index, diagnostics) route through the freshness oracle |
| `commit_parse_effect_if_current` (the oracle) + `document_generation_still_current` | `text_sync.rs:1194` (method), `:1252` (free fn), `:1232` (core) | ❌ PR #3618 only | Re-validates `(document_instance, generation)` **at the moment of commit** |
| Test barriers / panic injector (test-only) | `parse_worker.rs:446`–`542` (`ParseWorkerTestBarrier`, `ParseWorkerPanicInjector`) | ❌ PR #3618 only | Zero-sleep deterministic pause/release + panic injection |
| Worker metrics (test-API read) | `parse_worker.rs:191`–`270` | ❌ PR #3618 only | `jobs_started/coalesced/rejected_stale/published/panicked` — the assertion surface |

---

## 1. Coalescing — latest-only per URI

**🔶 Not on `origin/main` — lands via PR #3618 (open, unmerged).** Everything
in this section describes `parse_worker.rs`, which does not exist on `main`
today (see the banner at the top of this document).

**Invariant.** A rapid burst of N edits to one URI parses far fewer than N
times, and exactly one generation (the final one) publishes. An older,
not-yet-started job is silently replaced by a newer edit's job.

**Seam.** `Coordinator::enqueue` coalesce-replace (`parse_worker.rs:309`,
`jobs_coalesced` at `:323`); final publish gated by `process_job` (`:832`–`845`).

**Existing coverage — COVERED (deterministic) on PR #3618, not yet on `main`.**
- `rapid_burst_coalesces_to_far_fewer_jobs_than_edits` — `parse_worker.rs:1098`.
  Enqueues 20 edits, waits via `worker.wait_until_settled` (condvar-based,
  `Coordinator::wait_until_settled` at `:381` — **not** a sleep), asserts
  `jobs_published == 1`, `jobs_started < 20`, `jobs_coalesced > 0`, and the
  published generation equals the final edit.
- Receipt-level corroboration: `ux_neovim_ranged_typing_medium_file_receipt`
  asserts `worker_jobs_discarded_or_coalesced > 0` and `worker_jobs_published == 1`
  across a 20-ranged-edit burst (`ux_neovim_ranged_typing_latency_receipt.rs:367`,
  assertions ~`:409`–`413`). This test file exists on `main` today, but with an
  older shape that lacks the `worker_jobs_*` fields cited here — see §8.

**Feature-gate / run.** Inline `#[cfg(test)]` module — runs under
`cargo test -p perl-lsp-rs --lib` (no feature flag; the test-only barrier types
are `#[cfg(any(test, feature = "expose_lsp_test_api"))]` and the `test` cfg is
active in a `--lib` test build). Verify it actually runs:
`cargo test -p perl-lsp-rs --lib rapid_burst_coalesces -- --nocapture` must
report `1 passed`, not `0 filtered out`.

**Receipt.** `jobs_started` / `jobs_coalesced` / `jobs_published` counters and
the `worker_jobs_discarded_or_coalesced` receipt field.

---

## 2. Close/reopen — document-instance identity across generation reset

**🔶 Not on `origin/main` — lands via PR #3618 (open, unmerged).** Everything
in this section describes `parse_worker.rs`/`text_sync.rs` seams that do not
exist on `main` today (see the banner at the top of this document).

**Invariant.** A parse job queued against document instance A must never publish
into a *different* instance B that a `didClose`+`didOpen` cycle installed at the
same URI — **even when B's fresh generation counter has coincidentally reached
the same numeric value** A's job is trying to publish. Identity is `Arc::ptr_eq`
on the generation handle, not a `u32` compare.

**Seam.** `process_job` `Arc::ptr_eq(&doc.generation, &job.generation_handle)`
(`parse_worker.rs:842`); the same identity check in the deferred-side-effect
oracle `document_generation_still_current` (`text_sync.rs:1232`).

**Existing coverage — COVERED (deterministic) on PR #3618, not yet on `main`.**
- `stale_job_cannot_publish_into_a_reopened_document_instance` —
  `parse_worker.rs:1263`. Pauses A's gen-1 job at the barrier, replaces the map
  entry with a brand-new `DocumentState` (fresh `Arc<AtomicU32>`, bumped to the
  **same** numeric generation 1), releases A, asserts `jobs_rejected_stale >= 1`,
  zero side-effect callbacks, and that instance B is untouched (`Arc::ptr_eq`
  still B, `current_parsed()` still `None`).

**Feature-gate / run.** Inline `#[cfg(test)]` — `cargo test -p perl-lsp-rs --lib
stale_job_cannot_publish_into_a_reopened`.

**Receipt.** `jobs_rejected_stale` counter; the empty side-effect call log.

---

## 3. Panic / shutdown — worker survives panic + drains and joins on shutdown

**🔶 Not on `origin/main` — lands via PR #3618 (open, unmerged).** Both 3a and
3b describe `parse_worker.rs`, which does not exist on `main` today (see the
banner at the top of this document).

Two distinct invariants; the program needs **both**.

### 3a. Panic survival — COVERED (deterministic) on PR #3618, not yet on `main`

**Invariant.** A job that panics inside `process_job` (pathological parser input,
or injected) must (1) be recorded, (2) never publish, (3) release its URI so a
later edit to the same URI still parses, and (4) keep the worker thread alive.

**Seam.** `catch_unwind` + `FinishGuard` (`parse_worker.rs:650`–`673`); the
one-shot `ParseWorkerPanicInjector` (`:518`–`542`).

**Existing coverage.** `panicking_job_still_releases_its_uri_and_the_worker_keeps_processing`
— `parse_worker.rs:1328`. Arms the panic injector for gen 1, asserts
`jobs_panicked >= 1` and `jobs_published == 0`, then enqueues gen 2 to the *same*
URI and asserts it publishes — proving the URI was released and the pool survived.

### 3b. Shutdown drain + self-join safety (#3618) — **GAP**

**Invariant.** (i) Dropping the `ParseWorker` requests shutdown and joins every
worker thread, draining any jobs still in `ready` before exit (`take_next`
returns `None` only when `ready` is empty *and* shutdown was requested). (ii)
Dropping the **last** `Arc<LspServer>` from *inside* a worker thread's
`on_published` callback (the `Weak::upgrade()` temp going out of scope) must
NOT self-join-deadlock.

**Correction (verified 2026-07-10 against `parse_worker.rs` on PR #3618 head
`209b9cf56`).** This section previously described the (ii) fix as "the #3618
`Drop` guard skips joining the current thread's own handle" at
`parse_worker.rs:767`–`774`. That is not what the code does: `impl Drop for
ParseWorker` (`parse_worker.rs:752`–`759`) is an unconditional
`for handle in handles.drain(..) { let _ = handle.join(); }` loop with no
thread-identity check at all. The actual (ii) fix lives one layer up, in
`LspServer::install_default_parse_worker` (`crates/perl-lsp-rs/src/runtime/mod.rs:1192`–`1202`):
`on_published`'s closure captures `cb_server: Weak<LspServer>` via
`Arc::downgrade(self)` rather than a strong `Arc`, so the callback only ever
holds a transient strong ref (`cb_server.upgrade()`) for the duration of
`run_post_parse_side_effects`, breaking the `LspServer -> ParseWorker ->
worker threads -> on_published -> Arc<LspServer>` reference cycle at its
root — `ParseWorker::drop` itself is unchanged and still has no self-join
protection if a worker thread ever *does* end up dropping the last strong
`Arc<ParseWorker>`.

**Existing coverage.** Implicit only — every worker test drops the `ParseWorker`
at scope end, so a broken join would hang the suite. There is **no dedicated
regression test** that (a) asserts queued-but-unstarted jobs are drained on
shutdown, or (b) reproduces the self-join-from-callback-thread path described
in (ii) above. The guard is currently protected by code inspection + the fact
that the suite doesn't hang — a classic "the instrument is the only witness"
gap.

**Update (verified 2026-07-10, informational only — does not change the GAP
verdict above).** PR #3618's current head (`209b9cf56`, still open/unmerged)
already added `dropping_the_server_joins_the_installed_parse_worker_threads`
(`parse_worker.rs` test module, added past line `1447`) — a bounded-timeout
test that drops the server's last strong `Arc<LspServer>` **on a dedicated
thread it spawns for the purpose** (`std::thread::spawn(move || { drop(server); ... })`)
and asserts the join returns. This is a real, useful regression guard for the
Weak-downgrade fix generally, but it does **not** reproduce scenario (ii) as
stated: the drop happens from an external thread, not from inside a worker
thread's own `on_published` callback holding the last strong ref. Do not read
this test as closing (ii) — the self-join-from-callback-thread path remains
unproven either way. This PR (#3649) does not depend on, verify, or take
credit for that test; it is cited here only so the next reader of #3618 knows
to re-assess this section precisely (not just "closed?") once that PR merges.

**Two deterministic regression tests are still needed** (drain-on-shutdown;
self-join-from-callback-thread) to close this gap — tracked for design and
implementation via #3618, not sketched here. A prior revision of this section
proposed a specific self-join test design; it was removed after review found
the sketch could not actually reproduce the callback-thread-holds-the-last-ref
scenario as written (the side-effect barrier pauses *before* `on_published`
runs, i.e. before `Weak::upgrade()` — a real witness needs the barrier/handshake
placed *inside* the callback, after a successful `upgrade()`, so the external
strong reference can be dropped while that callback is still holding its own).
This is exactly the kind of prescriptive detail a done-condition **contract**
doc shouldn't carry speculatively — the actual test belongs in #3618, reviewed
against the real code at the time it's written, not pre-designed here against
a moving target.

**Receipt.** `jobs_panicked` (3a, existing); for 3b, once written: a
bounded-timeout "drop returned" boolean the test asserts — the absence of a
hang IS the receipt.

---

## 4. Cross-document progress — one doc's pending parse never blocks another

**🔶 Not on `origin/main` — lands via PR #3618 (open, unmerged).** Everything
in this section describes `parse_worker.rs`, which does not exist on `main`
today (see the banner at the top of this document).

**Invariant.** While one URI's parse is stalled (paused mid-publish, or a slow
large-file parse), edits to a *different* URI still parse and publish. Distinct
URIs dispatch to distinct pool threads; only same-URI edits serialize.

**Seam.** The bounded pool + per-URI `active` set (`QueueState.active`,
`parse_worker.rs:284`); `PARSE_WORKERS = 4` (`:143`).

**Existing coverage — COVERED (deterministic) on PR #3618, not yet on `main`.**
- `one_document_paused_does_not_block_another_documents_publish` —
  `parse_worker.rs:1142`. Pauses doc A at the barrier (never released until
  cleanup), enqueues doc B, asserts B publishes and fires side effects while A is
  still paused/unpublished. Explicitly the test that would catch a
  single-global-worker regression.

**Feature-gate / run.** Inline `#[cfg(test)]` — `cargo test -p perl-lsp-rs --lib
one_document_paused_does_not_block`.

**Receipt.** Per-URI side-effect call log records B's `(uri, generation)` while
A's is absent.

---

## 5. Edit-during-analysis — an edit mid-analysis rejects the stale effect

**🔶 Not on `origin/main` — lands via PR #3618 (open, unmerged).** `run_post_parse_side_effects`,
`commit_parse_effect_if_current`, and `document_generation_still_current` do
not exist in `text_sync.rs` on `main` today, and the worker-level test lives
in `parse_worker.rs`, also PR #3618-only (see the banner at the top of this
document). `text_sync/tests.rs` itself exists on `main`, but not these two
tests within it.

**Invariant.** If a newer edit (gen N+1) lands after gen N's parse published but
before gen N's *deferred side effects* (symbol reindex, workspace index,
diagnostics) commit, those side effects must be dropped — publication validity ≠
side-effect validity. The race window is real and reachable; the oracle closes it.

**Seam.** `run_post_parse_side_effects` (`text_sync.rs:1058`) →
`commit_parse_effect_if_current` (`:1194`/`:1252`); the worker's separate
`side_effect_barrier` pause point (`parse_worker.rs:882`).

**Existing coverage — COVERED (deterministic, two layers) on PR #3618, not yet on `main`.**
- Worker-level race reachability:
  `side_effect_barrier_pauses_after_publish_and_a_newer_edit_can_commit_while_paused`
  — `parse_worker.rs:1388`. Proves publish lands (gen 1 current), the callback is
  withheld, a real gen-2 edit commits, and the document is already at gen 2 when
  gen 1's side effect fires — i.e. the window the oracle must guard is real.
- Direct oracle proof: `stale_generation_side_effects_never_reindex_symbols` —
  `text_sync/tests.rs:1051`. Calls `run_post_parse_side_effects` with a ticket for
  a superseded generation; asserts the stale symbol never enters the index and
  the kept symbol survives.
- Real-worker end-to-end: `stale_side_effects_never_commit_through_the_real_worker_after_a_newer_edit`
  — `text_sync/tests.rs:1119`. Installs the real worker, uses the side-effect
  barrier, lands a real N+1 edit while N is paused, releases N, asserts N's
  symbol reindex never reached the index.

**Feature-gate / run.** Worker-level test: `--lib`. Oracle + real-worker tests
are in `text_sync/tests.rs` (module tests): `cargo test -p perl-lsp-rs --lib
stale_generation_side_effects`, `... stale_side_effects_never_commit`. The
real-worker one needs `workspace` (default) for `symbol_index`.

**Receipt.** The `symbol_index` search-prefix assertions (stale absent, kept
present) are the durable receipt.

---

## 6. Stale-effect rejection — gen N never commits over gen N+1 (publish gate)

**🔶 Partially not on `origin/main`.** The `document.rs` unit-level gate below
(`publish_parsed_if_current_rejects_stale_expected_generation` etc.) **is on
`main` today**. The worker-level tests (`stale_generation_is_rejected_latest_generation_publishes`,
`rejected_publish_never_invokes_the_side_effect_callback`) are in
`parse_worker.rs`, which is PR #3618-only (see the banner at the top of this
document).

**Invariant.** A completed parse for generation N must never overwrite a document
that has already advanced to N+1. The publish gate rejects it; a rejected publish
fires zero side effects.

**Seam.** `DocumentState::publish_parsed_if_current` (`document.rs:469`) — checks
both `current_generation() == expected_generation` and `snapshot.generation ==
expected_generation`; wired at `process_job` (`parse_worker.rs:842`).

**Existing coverage.**
- `stale_generation_is_rejected_latest_generation_publishes` —
  `parse_worker.rs:1028` **(PR #3618 only, not on `main`)**. Pauses N (gen 1),
  lands N+1 (gen 2) as a *separately dequeued* job (not coalesced — exercises
  the "already started, rejected at publish" path), releases N, asserts
  `jobs_rejected_stale >= 1`, `jobs_published == 1`, final generation 2, side
  effects only for gen 2.
- `rejected_publish_never_invokes_the_side_effect_callback` —
  `parse_worker.rs:1222` **(PR #3618 only, not on `main`)**. Isolated
  assertion: supersede gen 1 while paused, release, assert
  `jobs_rejected_stale >= 1` and the callback count is exactly 0.
- Unit-level gate: `publish_parsed_if_current_rejects_stale_expected_generation`,
  `..._accepts_matching_generation`, `..._rejects_mismatched_snapshot_generation`
  — `document.rs:620`–`645` **(on `main` today)**.

**Feature-gate / run.** All inline `#[cfg(test)]` — `cargo test -p perl-lsp-rs
--lib publish_parsed_if_current`, `... stale_generation_is_rejected`,
`... rejected_publish_never_invokes`.

**Receipt.** `jobs_rejected_stale` / `jobs_published` counters; the zero-call
side-effect log.

---

## 7. Provider honesty canaries — every user-facing provider stays honest through the gap

**🆕 Added by this PR (#3649) — on this PR's branch, not yet on `origin/main`.**
The substrate this section exercises (`DocumentState::current_parsed` /
`latest_parsed`, the `test_apply_text_change_without_reparse` /
`test_publish_parse_for_current_generation` helpers) predates this PR and is
on `main` today; the tests themselves (§7, §7a, §7b) are this PR's own
contribution and become a main-branch fact only once this PR merges.

**Invariant.** While `current_parsed()` is `None` (text generation ran ahead of
the last published snapshot), NO provider may present a **stale** fact (the
superseded identifier) or an **unearned fresh** fact (a claim about the
not-yet-parsed new text). Fail-closed providers produce nothing; the two declared
exceptions (completion, symbols) may answer only from a bounded text/regex
fallback over the *current* text, never the stale AST.

**Seam.** Every provider reads `DocumentState::current_parsed()` (`document.rs:450`)
rather than `latest_parsed()`; the gap is forced by the #3589 helpers
`test_apply_text_change_without_reparse` / `test_publish_parse_for_current_generation`,
and driven for real by the worker's pre-publish `test_barrier`.

**Existing coverage — COVERED for 10 providers (deterministic), synthetic + real.**
File: `crates/perl-lsp-rs/tests/pending_parse_provider_freshness_tests.rs`.

| Provider | Gap policy | Test | Line |
|----------|-----------|------|------|
| Semantic tokens (full + range) | emit nothing (no gen-N claim from N-1 AST) | `semantic_tokens_emit_nothing_during_pending_parse_gap` | `:385` |
| Hover | no stale AST claim | `hover_degrades_during_pending_parse_gap` | `:311` |
| Signature help | no stale AST claim (falls back to the name-only builtin table, never the AST) | `signature_help_no_stale_claim_during_pending_parse_gap`, `signature_help_never_answers_from_stale_ast_with_matching_name_during_pending_parse_gap` | (see file; also asserted inline in the headline canary) |
| Definition (navigation) | fail closed | `definition_fails_closed_during_pending_parse_gap` | `:335` |
| References | fail closed, never leak `foo` | `references_fail_closed_during_pending_parse_gap` | `:362` |
| Rename | fail closed (zero edits) | `rename_fails_closed_during_pending_parse_gap` | `:417` |
| Safe-delete | fail closed (fallback decision) | `safe_delete_fails_closed_during_pending_parse_gap` | `:449` |
| Document symbols | current-text regex only, never stale AST | `document_symbols_never_leak_stale_identifier_during_pending_parse_gap` | `:490` |
| Call hierarchy | fail closed (null) | `prepare_call_hierarchy_fails_closed_during_pending_parse_gap` | `:511` |
| Completion | bounded fallback may answer, never `foo` | `completion_uses_bounded_fallback_during_pending_parse_gap` | `:541` |

Plus the two headline cross-provider canaries on one shared `sub foo -> bar` edit:
- `sub_foo_to_bar_cross_provider_freshness_canary` (synthetic gap) — `:132`.
  Now also walks signature help (gap assertion + post-publish resolve-to-`bar`
  assertion), closing §7a.
- `sub_foo_to_bar_cross_provider_freshness_canary_real_async_worker` (real
  installed worker + barrier) — `:658` (lands with #3618; not yet present on
  `origin/main` at the time this PR was authored — re-grep if adding
  signature-help there too is desired as a follow-up)
- No-gap regression guard: `providers_answer_normally_with_no_pending_parse_gap`
  — `:570` (ensures the gap logic doesn't misfire when there is no gap).

**Signature-help test API.** `LspServer::test_handle_signature_help` was added to
`crates/perl-lsp-rs/src/runtime/test_api.rs` (mirroring `test_handle_hover`) to
expose `handle_signature_help` for these canaries — it did not previously exist.

**7a. Signature-help — COVERED (closed).** `handle_signature_help`'s
user-defined-function branch (`get_user_function_signature`) requires
`doc.current_parsed()`'s AST, so it is skipped entirely during the gap; the
name-only builtin-function-table fallback does not recognize arbitrary
identifiers (`foo` is not a Perl builtin), so no stale claim surfaces. Proven by
`signature_help_no_stale_claim_during_pending_parse_gap` (standalone, mirrors
`hover_degrades_during_pending_parse_gap`) and by the two new assertion blocks
added to `sub_foo_to_bar_cross_provider_freshness_canary` (gap: `!json_contains(&sig_gap, "foo")`;
post-publish: `json_contains(&sig1, "bar")` and `!json_contains(&sig1, "foo")`).

*Revert-proves-red note (deep review, #3649).* The `foo` -> `bar` rename
fixture shared by the two tests above cannot by itself distinguish "gap
handled honestly" from "gap handled by silently reading `latest_parsed()`
instead of `current_parsed()`": the mutated code would look up `bar` in the
stale AST that only defines `foo`, miss by name regardless of which
snapshot is consulted, and the assertion (`!json_contains(&sig, "foo")`)
would pass either way — verified experimentally by reverting
`current_parsed()` -> `latest_parsed()` in `signature_help.rs` and observing
the original two assertions stay green. `signature_help_never_answers_from_stale_ast_with_matching_name_during_pending_parse_gap`
closes that hole with a same-named, signature-changing fixture (`sub calc {}`
-> `sub calc($x, $y) {}`) where a stale-AST answer is name-matchable but
produces a distinguishable wrong label (`"sub calc"`, 0 params); this test
was confirmed to fail under the same mutation and pass on the real
`current_parsed()`-gated implementation. It also asserts a post-publish
positive resolve (the fresh 2-parameter `calc` signature appears once
`test_publish_parse_for_current_generation` closes the gap), mirroring the
headline canary's gap/post-publish asymmetry.

**7b. Diagnostics honesty during the gap — COVERED (closed for document-pull).**
The internal side-effect staleness was already proven (§5), and
`pull_diagnostics_freshness_tests.rs` already had a "pending-parse gap tests"
section (`pull_document_diagnostic_stays_fresh_during_pending_parse_gap`,
`pull_workspace_diagnostic_omits_gapped_doc_from_items`,
`pull_workspace_diagnostic_resumes_after_pending_parse_gap_closes`) covering the
error-introduction direction and the post-gap-closure direction. The specific
gap this PR closes is the **error-fixing** direction *during* the gap for
single-document pull: `pull_document_diagnostic_does_not_report_a_fixed_syntax_error_as_current_during_pending_parse_gap`
opens a document with a syntax error (asserts `PL001` is reported), applies a
fix via `test_apply_text_change_without_reparse` (forcing the gap open), and
asserts the pull-diagnostic response taken *during* the gap does not carry the
now-fixed `PL001` diagnostic — i.e. the pull path reflects the current text
honestly rather than resurrecting a stale AST-derived diagnostic. Push
diagnostics (`textDocument/publishDiagnostics` notifications) have no
comparable test-only capture API in `test_api.rs` today; that surface is not
covered by this PR and is noted as residual scope, not silently assumed closed.

**Feature-gate / run.** `#![cfg(all(feature = "workspace",
feature = "expose_lsp_test_api"))]`. A bare `--test
pending_parse_provider_freshness_tests` compiles **0 tests** and false-greens.
Run: `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --features
expose_lsp_test_api --test pending_parse_provider_freshness_tests -- --nocapture`
and confirm a non-zero passed count. Diagnostics honesty test:
`#![cfg(feature = "expose_lsp_test_api")]` in `pull_diagnostics_freshness_tests.rs`
— same run pattern, `--test pull_diagnostics_freshness_tests`.

**Receipt.** The `json_contains(&result, "foo") == false` assertions across every
provider, plus the post-publish resolve-to-`bar` assertions, are the durable
receipt for §7a. For §7b, the `code == "PL001"` presence/absence assertions
before, during, and after the gap are the durable receipt.

---

## 8. Neovim latency receipts — no full-parse / no parent-map rebuild in didChange

**Invariant.** On the async path (`incremental_eager = false`, production default
since #3412, worker installed), `didChange` performs **zero** full parses and
**zero** parent-map builds before returning — it applies the text edit, bumps the
generation, enqueues, and returns. The worker starts ≤ N jobs for N edits,
publishes exactly one (the final) generation, and discards/coalesces ≥ 1.

**Seam.** `didChange` enqueue-and-return path (`text_sync.rs`, gated by
`install_default_parse_worker` eligibility); the `didChange.full_parse` /
`didChange.parent_map` timing spans that must be absent on the async path.

**🔶 Correction (verified 2026-07-10): the file exists on `main`, but the
worker-shape assertions below do not — same function name, two different
tests.** `crates/perl-lsp-rs/tests/ux_neovim_ranged_typing_latency_receipt.rs`
exists on `origin/main` today, and it already has a test named
`ux_neovim_ranged_typing_medium_file_receipt` (at `main`'s current `:298`) —
but that version's AFTER/BEFORE split is `incremental_eager` off vs. on (both
still fully **synchronous**, no `ParseWorker` installed at all), and it
asserts `parse_jobs_started == ranged_edits` — i.e. **one synchronous full
parse per edit**, the opposite of the "zero full parses" claim below. The
`did_change_full_parse_count` / `did_change_parent_map_count` /
`worker_jobs_*` fields and the AFTER-is-the-real-worker semantics described
here **do not exist on `main`** — they are added by PR #3618 (open,
unmerged), which changes this same test's AFTER scenario to install the real
`ParseWorker` instead of just toggling `incremental_eager`. Do not run
`ux_neovim_ranged_typing_medium_file_receipt` on `main` expecting it to prove
the worker-path invariant this section claims; it currently proves a
different (still-relevant, still-synchronous) invariant.

**Existing coverage — COVERED (shape-deterministic) on PR #3618, not yet on `main`.**
File: `crates/perl-lsp-rs/tests/ux_neovim_ranged_typing_latency_receipt.rs`.
- `ux_neovim_ranged_typing_medium_file_receipt` — `:367` on PR #3618 head
  `209b9cf56` (`:298` on `main`, testing the different invariant above).
  Drives 20 ranged edits on a realistic ~78 KB fixture through the real
  installed worker (AFTER) and the sync fallback (BEFORE). Asserts on AFTER:
  `did_change_full_parse_count == 0` (`:409`), `did_change_parent_map_count == 0`
  (`:413`), `jobs_started <= 20`, `jobs_published == 1`,
  `jobs_discarded_or_coalesced > 0`. Asserts on BEFORE the Phase-2 invariant
  (one sync full parse per edit) as a differential control. Every provider
  (completion/hover/semantic-tokens/references) must return after the burst
  settles (`test_wait_for_parse_worker_settled`, condvar — no sleep). CI
  asserts SHAPE only; millisecond timings are informational (#1373).

**Feature-gate / run.** `#![cfg(feature = "expose_lsp_test_api")]`. A bare
`--test ux_neovim_ranged_typing_latency_receipt` compiles **0 tests** and
false-greens (documented in the file header). Run:
`cargo test -p perl-lsp-rs --features expose_lsp_test_api --test
ux_neovim_ranged_typing_latency_receipt -- --nocapture` — `--nocapture` is
required to emit the `PERL_LSP_TIMING_RECEIPT {...}` payloads.

**Receipt.** The printed `PERL_LSP_TIMING_RECEIPT` JSON (two payloads:
`after_async_parse_worker_phase3`, `before_eager_on_pre_3412_baseline`) with
`did_change_full_parse_count`, `did_change_parent_map_count`, and `worker_jobs_*`
fields — a durable, greppable artifact.

---

## 9. Completion checklist

Each done-condition tracked on the Implemented (production seam exists) · Covered
(deterministic proof exists that runs) · Gap axis, plus an explicit **On `main`?**
column — verified 2026-07-10 against `origin/main` @ `c93911bf0` and PR #3618
(open, unmerged) @ `209b9cf56`. A row can be "Covered" and still ❌ on `main`:
that means the proof is real and passing on the cited branch, not that it runs
in CI today outside that branch. "🆕 this PR" means the row's proof lives on
*this* PR's branch (`test/done-condition-canary-gaps`, also open/unmerged) and
becomes a main-branch fact only once this PR merges — not before.

| # | Done-condition | Implemented | Covered (deterministic, runs) | On `main`? | Gap |
|---|----------------|:-----------:|:-----------------------------:|:----------:|-----|
| 1 | Coalescing (latest-only per URI) | ✅ | ✅ `rapid_burst_coalesces…` + receipt | ❌ PR #3618 | — |
| 2 | Close/reopen instance identity | ✅ | ✅ `stale_job_cannot_publish_into_a_reopened…` | ❌ PR #3618 | — |
| 3a | Panic survival | ✅ | ✅ `panicking_job_still_releases_its_uri…` | ❌ PR #3618 | — |
| 3b | Shutdown drain + self-join (#3618) | ⚠️ Weak-downgrade breaks the cycle (`runtime/mod.rs:1192`); `ParseWorker::drop` itself has no self-join guard | ⚠️ implicit only + one external-thread-drop test (`209b9cf56`) that doesn't reproduce the callback-thread case | ❌ PR #3618 | **GAP** — no dedicated drain / self-join-from-callback-thread regression test; see §3b Correction |
| 4 | Cross-document progress | ✅ | ✅ `one_document_paused_does_not_block_another…` | ❌ PR #3618 | — |
| 5 | Edit-during-analysis (stale side-effect drop) | ✅ | ✅ 3 layers (worker barrier + oracle + real-worker) | ❌ PR #3618 | — |
| 6 | Stale-effect rejection (publish gate) | ✅ | ✅ `stale_generation_is_rejected…`, `rejected_publish_never_invokes…` (PR #3618); unit gate `publish_parsed_if_current_*` | ⚠️ partial — unit gate on `main`, worker-level tests PR #3618 only | — |
| 7 | Provider honesty canaries (10 providers) | ✅ | ✅ per-provider + synthetic cross-provider canary | 🆕 this PR | — |
| 7a | — signature-help canary | ✅ (reads `current_parsed`) | ✅ `signature_help_no_stale_claim_during_pending_parse_gap` + `signature_help_never_answers_from_stale_ast_with_matching_name_during_pending_parse_gap` + headline canary assertions | 🆕 this PR | — |
| 7b | — diagnostics honesty-through-gap canary | ✅ | ✅ `pull_document_diagnostic_does_not_report_a_fixed_syntax_error_as_current_during_pending_parse_gap` (document-pull); workspace-pull already covered | 🆕 this PR | — |
| — | — real-async cross-provider canary (`…_real_async_worker`) | ✅ (needs installed `ParseWorker`) | ✅ | ❌ PR #3618 | — |
| 8 | Neovim receipts (no full-parse/parent-map in didChange) | ✅ | ✅ `ux_neovim_ranged_typing_medium_file_receipt` (worker-shape assertions) | ❌ PR #3618 (same-named test exists on `main` today but asserts a *different*, still-synchronous invariant — see §8 Correction) | — |

**Merge-gate note.** The two branch-protection required checks
(`Perl LSP Rust Small Result`, `ripr+ New Gap Gate`) must be green on the SHA
that lands each new test. The feature-gated integration tests (§7, §8) do NOT run
under a bare `--test` invocation — any CI lane proving these must pass
`--features expose_lsp_test_api` (and `workspace` for §7), or it green-lights 0
tests. Separately, and orthogonally: any lane citing §0–§6 or the worker-shape
half of §8 as "covered" must be running against PR #3618 (or a branch that has
merged it) — those seams and tests do not exist on `origin/main` as of this
writing.

---

## 10. Remaining gap

1. **§3b — Shutdown drain + self-join-from-callback-thread regression test.**
   The production fix (#3618's `Weak<LspServer>` downgrade in
   `install_default_parse_worker`, `runtime/mod.rs:1192`) breaks the `LspServer
   <-> ParseWorker` reference cycle at its root, but `ParseWorker::drop`
   (`parse_worker.rs:752`–`759`) itself still has no self-join guard — see the
   §3b Correction for why this document's earlier "Drop guard skips joining its
   own handle" description was wrong. PR #3618's `209b9cf56` added a
   regression test that drops the server's last `Arc` from a dedicated
   external thread (proving the cycle-break generally), but that test does
   not reproduce the callback-thread-holds-the-last-ref case §3b(ii)
   describes — it's protected only by "the suite doesn't hang," which a
   future refactor of `ParseWorker::drop` or the `Weak` callback could
   reintroduce undetected. Two small deterministic tests still needed
   (drain-on-shutdown; self-join-from-callback-thread with a bounded-timeout
   watchdog, run from *inside* a worker thread's own `on_published`
   invocation). Tracked for closure via #3618.

§7a and §7b (signature-help and diagnostics honesty-through-gap canaries) are
closed **by this PR** — see §7 for the new test names and receipts. They are
not yet an `origin/main` fact while this PR is open; they become one on merge.
What is already on `origin/main` today, independent of this PR landing, is
narrower: the `document.rs` freshness primitives (§0, §6's unit-level gate)
and the pre-existing (differently-scoped) `ux_neovim_ranged_typing_medium_file_receipt`
test discussed in §8's Correction. §6 is **partial** on `main` today — its
`document.rs` unit-level gate (`publish_parsed_if_current_*`) is there now,
but its worker-level tests (`stale_generation_is_rejected_latest_generation_publishes`,
`rejected_publish_never_invokes_the_side_effect_callback`) are not; do not
read §6 as either "fully on main" or "fully branch-only."

Conditions 1, 2, 3a, 4, 5 (worker layer), 6 (worker layer), and the
worker-shape half of 8 are **fully and deterministically covered on PR #3618**
— not yet a present-tense fact about `origin/main`. Once #3618 merges, those
rows become main-branch facts. Once *this* PR (#3649) merges, §7/§7a/§7b
become main-branch facts on their own, independent timeline. This document's
per-section "not on `main`" / "added by this PR" notes should be revisited
against whichever of the two has landed (or re-verified and left in place if
either lands via squash/rebase with different line numbers — re-grep first).
