# CAND-INV-01 context

- Issue: #10949 (programme controller #8969, identity controller #8963).
- Basis: main@30efa8a (2026-09-07).
- One PR; control plane only. No candidate inclusion, ordering, insertion,
  rank, cap, route, serialization, or protocol response change.

## Problem

The shared `CompletionCandidate` envelope landed in PR #10145 with semantic and
source-anchored identity constructors, but no live producer uses them: every
candidate still reaches merge as a label. That is a safe compatibility stage
only while every source is known, classified, owned, and given an exit. Today it
is not, so an agent asked to "finish #8963" has to rediscover, every time, which
functions append candidates, which have canonical identity, which own an
insertion edit, which can report completeness, which route reaches them, and who
owns each migration.

Without that denominator two specific failures are invisible: #10229 can look
complete while a legacy source appends outside the shared route, and a focused
producer migration can fabricate identity from a label with the suite green.

## Decision

Add one versioned, source-derived contract and the task that reconciles it.

- `policy/completion-candidate-producers.toml` — `completion_candidate_producers.v1`.
  The authored half: one row per producer carrying candidate class, tier,
  identity, insertion plan, evidence, rank, source completeness, finalizer
  route, reach evidence, reached-by set, migration owner, and the limitations a
  reader must not infer past.
- `docs/architecture/completion-candidate-producers.md` — the generated
  projection, including a Mermaid route graph. Regenerated, never edited.
- `cargo xtask completion-candidates check | list | explain <id> | graph` —
  `check` reconciles and asserts the projection is current; `graph` regenerates
  it. This extends the `compat-inventory` pattern already in the tree rather
  than adding a second contract-checking family.

The task reads current source and checked contracts only. It never queries
GitHub and never infers state from issue status.

## Denominator and its three planes

A **producer** is a function that carries candidates: it takes a
`&mut Vec<Candidate>` append channel, or returns a type containing
`Vec<Candidate>`, where a candidate is a `CompletionItem` (today's compatibility
shape) or a `CompletionCandidate` (the migration target). Discovery is
`syn`-based over the tracked completion source.

One plane is not enough, so discovery reconciles three:

- **channel** — the producer population above, which also covers a return of a
  named carrier struct such as `CompletionFinalization`, discovered from source
  rather than listed;
- **construction** — every file that builds a `CompletionItem`. A file that
  constructs candidates but exposes no producer must carry a
  `[[construction_only]]` row naming the producer that owns its output;
- **delegation** — every `providers::` module the scanned surface reaches into.
  A module not scanned in full must carry a `[[delegations]]` row.

The second plane exists because a producer could otherwise hide behind a
different signature; it bounds that gap at file granularity. The third exists
because neither of the first two can see outside the scanned tree at all: a
facade inside the roots that returns candidates built in an unscanned module
would satisfy both while the real producer carried no disposition and no owner.
That is not hypothetical — it is how `htmx::complete_header_names` and
`file_completion::complete_file_paths` stayed out of the first draft of this
ledger, reached through the inventoried
`completion::completion::file_path` facade.

## Proof strategy

The checked-in ledger reconciled against the real tree is the valid fixture.
Unit fixtures corrupt it along one axis each and assert the validator refuses
for that reason, so a green suite means the axis was exercised rather than that
a synthetic fixture happened to be well formed. Mutations were also run against
real source and confirmed refused; `acceptance.md` lists them.

The route control is the load-bearing one. `reached_by` on a row whose function
an entry point calls directly is reconciled against that call site, so a row
cannot claim a reach the source contradicts.

## Two findings this inventory records and does not repair

Both are recorded as row limitations, owned elsewhere. #10949 changes no
behavior, so neither is fixed here.

1. `add_dancer2_keyword_completions` is called by `handle_completion_cancellable`
   and not by `handle_completion`. Which Dancer2 DSL keywords a user is offered
   therefore depends on which request path the editor took. Route convergence is
   #10229's; producer identity is #15014's.
2. `complete_regex_context` calls the shared deduplicate-and-sort and returns,
   after which the runtime finalizes again — a second sort for the regex
   context. Collapsing it is #10229's.

## Boundaries

- No producer migration, no candidate output change, no rank or cap change, no
  runtime route change, no LSP serialization change, no provider promotion.
- Discovery is syntactic. A producer that neither took the append channel nor
  returned a candidate vector would not appear as a row; the construction plane
  bounds that at file granularity and the delegation plane bounds it at module
  granularity, and the module documents the ceiling.
- Two rules are deliberately over-inclusive: a local whose initializer mentions
  the page is treated as holding it, and a named carrier is matched on its last
  path segment. Both would need type resolution to decide precisely, and both
  err toward a false alarm rather than a miss.
- The post-finalizer control is source-order, not control-flow aware. It can
  raise a false alarm on a body that finalizes inside one branch and
  contributes on another; it cannot miss an append on that axis. Neither
  shipped entry point has that shape. A control-flow analysis was judged
  disproportionate for a control-plane inventory — a checker complex enough to
  be wrong quietly is worse than one that occasionally asks a question.
- Reachability is proven only for rows an entry point calls directly. Rows
  reached through the provider call declare their reach, and every row records
  which of the two it is.
- The check is not registered as a CI gate in this PR. #14946 records that a
  gate added on `main` refuses every older-based PR's shard at preflight, and
  the closest analogue, `compat-inventory`, is likewise ungated. Gating is a
  follow-up once #14946 lands.
