# CAND-INV-01 checklist

Mapped to #10949's acceptance criteria. Implementation and proof observations
record the original work unless explicitly re-anchored below; they are not new
proof runs for #19126. Counts and CI registration are reconciled at
`fc5f238ccc6f74b42c1bf5e3dda7767ab7d23041` in [acceptance.md](acceptance.md).

- [x] Every standard completion candidate producer and append site is inventoried (revision-bound snapshot: 60 rows, `syn`-derived over 41 tracked files, across three discovery planes; see acceptance.md for the deleted `symbols.rs` delta and proof ceiling).
- [x] Identity, insertion, evidence, rank, completeness, finalizer route and legacy status are explicit on every row, as closed vocabularies rejected by serde when unknown — and a real producer cannot erase one to `not_applicable`, nor a routing seam acquire one.
- [x] Every nonterminal legacy row names one focused migration or retirement owner; a controller (`#8969`/`#8963`/`#9621`) is refused as an owner.
- [x] A new hidden producer fails the check — proven by source mutation, not only by fixture; an untracked one now stops the run rather than reporting green, and one whose append channel is spelled through a type alias is refused by name.
- [~] A post-finalizer append fails the check **in the straight-line entry body** — proven by source mutation, including ones smuggled through a renamed binding, a destructuring pattern, a rebuilt page, and a finalizer called as a method on the entry points' own impl. **Partial, deliberately.** `EntryBodyVisitor` does not descend into deferred bodies, so an append inside a closure, `async` block, or nested `fn` that *is* invoked is not seen. Descending indiscriminately is not the repair — it made an uncalled closure arm the finalizer at its definition point and refused correct source. An invocation-sensitive model is owned by **#15470**; this box is not claimed as fully satisfied.
- [x] Two same-named trait-impl producers in one file cannot fuse into a single row and inherit one disposition; the same holds for same-terminal-name self types and trait paths.
- [x] A producer the scanned tree delegates to, in a module not scanned in full, fails the check — including behind a grouped import, a renamed module, or a renamed `providers` namespace.
- [x] An ambiguous trailing function name that an entry point calls is refused rather than letting one row's reach ride on another's call site.
- [x] Generated `list`, `explain` and `graph` outputs are deterministic and source-derived; second generation is byte-identical.
- [x] Current-tree checked state drives validation. The task makes no network call and reads no issue status.
- [x] No user-visible completion behavior changes. The only product-source edits in this PR were the four mutation probes, all reverted; the diff touches `xtask`, `policy/`, `docs/architecture/` and `.spec/` only.
- [x] Reachability is reconciled against the entry-point call sites for direct-call rows, so a row cannot claim a route the source contradicts.
- [x] Route divergences are named, owned, and projected — including the Dancer2 keyword divergence between the two shipped entry points.
- [x] Producers no shipped entry point reaches are recorded with a reason rather than dropped.
- [x] A source change invalidates the audit through `source_digest` rather than leaving stale dispositions valid.
- [x] Producer classes with no existing migration leaf were given one (#15014) rather than left owned by a controller or absorbed into this tooling PR.
- [x] `.spec/10949-completion-candidate-producers/` records context, acceptance and this checklist.
- [x] New non-Rust files classify under the existing `docs/**` and `policy/**` allowlist entries; `cargo xtask non-rust inventory --check` passes.
- [x] The check is registered as `completion_candidate_ledger`, a required, non-quarantined merge-gate-tier gate in `.ci/gate-policy.yaml`, selected by `.github/workflows/ci.yml`'s policy shard. #14946 is closed; the former CI-registration deferral is spent. This box establishes registration, not GitHub merge enforcement.

Remaining separate work, with scope and historical reasons in `context.md`:

- [ ] GitHub merge enforcement of the policy shard/aggregate — the source inventory records the aggregate as advisory with `enforcement = "neither"`; the 2026-11-11 GitHub observation in acceptance.md excludes both contexts. Reconcile against live settings under #18495 / #17707 rather than inferring protection from the inner gate's `required: true`.
- [ ] Producer migrations — owned by #11002 / #11009 / #11015 / #11021 / #1701 / #8958 / #9478 / #10234 / #15014, not by this PR.
- [ ] Route convergence and the duplicate regex-context finalization — owned by #10229.
- [ ] Post-finalizer appends inside an **invoked** deferred body — owned by #15470. This is the residual half of #10949's *"a post-finalizer append fails the check"*, which is why the box above is marked partial rather than checked, and why this PR advances #10949 rather than closing it outright.
