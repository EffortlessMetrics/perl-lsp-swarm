# CAND-INV-01 checklist

Mapped to #10949's acceptance criteria.

- [x] Every standard completion candidate producer and append site is inventoried from current source (58 rows, `syn`-derived over 42 tracked files, across three discovery planes).
- [x] Identity, insertion, evidence, rank, completeness, finalizer route and legacy status are explicit on every row, as closed vocabularies rejected by serde when unknown — and a real producer cannot erase one to `not_applicable`, nor a routing seam acquire one.
- [x] Every nonterminal legacy row names one focused migration or retirement owner; a controller (`#8969`/`#8963`/`#9621`) is refused as an owner.
- [x] A new hidden producer fails the check — proven by source mutation, not only by fixture; an untracked one now stops the run rather than reporting green, and one whose append channel is spelled through a type alias is refused by name.
- [x] A post-finalizer append fails the check — proven by source mutation, including ones smuggled through a renamed binding, a destructuring pattern, and a finalizer called as a method on the entry points' own impl.
- [x] Two same-named trait-impl producers in one file cannot fuse into a single row and inherit one disposition; the same holds for same-terminal-name self types and trait paths.
- [x] A producer the scanned tree delegates to, in a module not scanned in full, fails the check — including behind a grouped or renamed import.
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

Deliberately not done, with reasons in `context.md`:

- [ ] The check is registered as a CI gate — deferred while #14946 stands, since a gate added on `main` refuses every older-based PR's shard at preflight. The closest analogue (`compat-inventory`) is likewise ungated.
- [ ] Producer migrations — owned by #11002 / #11009 / #11015 / #11021 / #1701 / #8958 / #9478 / #10234 / #15014, not by this PR.
- [ ] Route convergence and the duplicate regex-context finalization — owned by #10229.
