# Context: #10794 — compile one versioned workspace-symbol query profile and typed match evidence

## Origin

Q01 of the workspace-symbol query train #10643 (controller #10632). Ownership
prerequisite #8794, matching-equivalence oracle #8262, generation-owned
candidates #8756, row contract #10641, consumers #10645/#10642.

Reconciled against `origin/main@ab3cece9d` (worktree
`F:\Temp\opencode\wt-10794`, branch `codex/workspace-symbol-query-profile-substrate`).

## Reconciliation findings (current main)

- `git grep WorkspaceSymbolQueryProfile|MatchEvidence|QueryProfile` over `*.rs`
  returns nothing: the substrate is absent on main.
- #8794's full policy cut has **not** landed: `MIN_LOOSE_MATCH_QUERY_CHARS = 2`
  still lives in `crates/perl-symbol/src/types/mod.rs:31` and is re-exported by
  `perl-symbol::api`, `perl-workspace::workspace_index` (line 106), and
  `perl-lsp-rs-core::providers::symbol_query` (line 8).
- `cargo xtask check-architecture` (named in the issue's verification block) no
  longer exists as a subcommand. The current architecture recurrence surface is
  `cargo xtask layer-check` (`xtask/src/tasks/layer_check.rs`) plus
  `check-test-wiring`. This PR adds its recurrence rules there and runs the
  current spellings.
- No open PR or branch collides with this claim.

## Live matching/normalization/comparison call-site inventory

All sites live at `main@ab3cece9d`; counts are exact.

### A. Matcher/comparator helpers (duplicate authorities today)

1. `crates/perl-lsp-rs-core/src/providers/symbol_query/mod.rs`
   - `matches_query(name, query) -> bool` (lines 23–53): trim → lowercase →
     exact/prefix gate → loose-tier gate on lowercased char count vs
     `MIN_LOOSE_MATCH_QUERY_CHARS` → substring → subsequence.
   - `compare_names_by_query(a, b, query)` (66–85): tier asc, raw-name byte-len
     asc, raw-name lexicographic; re-lowercases both names per call.
   - private `match_tier` (94–104): numeric tier where **3 conflates a real
     subsequence match with a non-match** (the fallback conflation).
   - private `is_subsequence(haystack, needle)` (106–121).
   - re-export of `perl_symbol::MIN_LOOSE_MATCH_QUERY_CHARS`.
2. `crates/perl-workspace/src/workspace/workspace_index.rs`
   - inline matcher in `search_source_symbols` (4137–4196): scores 3 exact /
     2 prefix (short-query path) / 2 substring / 1 subsequence; `(uri,
     start_byte)` dedup; sort score desc then raw name asc (**no length
     tiebreak**); local lowercase per key per request.
   - inline closure `matches_query_text` in `search_generated_workspace_symbols`
     (4224–4231): contains when loose, starts_with when short; **never admits
     subsequence**; results sorted by `sort_workspace_symbols` (name/uri/range).
   - free fn `is_subsequence(needle, haystack)` (≈14414–14430).
   - re-export of `perl_symbol::MIN_LOOSE_MATCH_QUERY_CHARS`.

### B. Logical request paths that normalize/match independently today

3. **Full source-backed**: `WorkspaceIndex::search_symbols` →
   `search_source_symbols` (#1/#2 matcher), called twice from
   `perl-lsp-rs/src/runtime/workspace.rs::handle_workspace_symbols_v2` (lines
   ≈428, ≈477) — each call re-trims/re-lowercases the same query.
4. **Candidate-restricted**: `WorkspaceSymbolsProvider::search_with_candidates`
   (perl-lsp-rs-core `workspace_symbols/mod.rs` 348–377): `matches_query` per
   candidate bucket + `compare_names_by_query` sort. Measurement-only today
   (#8262 doc boundary), but it is a live second normalization site.
5. **Open-document full**: `WorkspaceSymbolsProvider::search` (394–419):
   `matches_query` per symbol + `compare_names_by_query` sort.
6. **Generated/framework**: `WorkspaceIndex::search_generated_workspace_symbols`
   (4208–4285): `matches_query_text` closure + `sort_workspace_symbols`;
   browse (trimmed-empty) queries return an empty vec here by design.
7. **Open-document text-fallback branch** — discovered during repair review,
   absent from the original inventory above (inventory-completeness defect of
   this packet, fixed here): `perl-lsp-rs/src/fallback/text.rs:178-201`
   `extract_text_based_symbols` lowercases the raw query independently
   (`query.to_lowercase()`, no trim), admits by folded substring containment
   only (no short-query gate, no subsequence tier), and its rows join the
   same logical open-document response as P3 via
   `runtime/workspace.rs::search_open_documents_for_symbols` (:638) →
   `runtime/document_access.rs:226`. Live reachability:
   `handle_workspace_symbols_v2` (:423 stale-index skip, :528 empty-index
   fall-through).

### C. Disposition of site 7 (recorded decision, not silently migrated)

Site 7 is in-scope by this issue's own rows (it serves the open-document
response), but Q01's parity mandate forbids migrating it onto
`match_searchable_key` this slice: its admission is untrimmed, ungated,
contains-only, which differs from every admitted profile-tier combination for
(a) whitespace-padded queries, (b) one-char queries, and (c) whitespace-browse
queries — three externally visible membership changes that would require
separately named correction fixtures this PR does not plan. Growing a new
ungated/untrimmed owner operation would exceed Q01's stated profile contract
and put a second admission authority inside the fresh owner for one degraded
branch. Per the #9147/#9888 orphan precedent it is therefore inventoried with
exact owner/handoff instead:

- owner/handoff: response assembly and per-path matcher consolidation belong
  to #10645/#10642; a parity-correct migration must land with a named
  correction fixture (or preserve membership bit-for-bit) when those stages
  touch the open-document composer;
- until then WS-QP-014 keeps its recorded scope: full-vs-accelerated index
  tiers of one logical request share one compiled digest (P1); site 7 remains
  a known independent normalizer inside the degraded open-document response.

### D. Untouched by this PR (owned elsewhere)

- Orphan parser-local provider tree: owned by #9147/#9888; not edited.
- `perl-symbol::SymbolIndex::search_prefix/search_fuzzy`: accelerator-only
  primitives under #8262's candidate contract; not workspace-symbol policy;
  their retirement belongs to #9268/#4798/#8106. This PR does not move policy
  back into `perl-symbol`.
- Row population/composition, final dedup/order/budget, LSP projection,
  service routing, resolve, streaming: owners #10641/#10645/#10642/#10644.

## Old/new owner

```text
old: threshold constant in perl-symbol + bool matcher in symbol_query +
     two inline matchers in perl-workspace + numeric fallback tier
new: one module above perl-symbol — perl-workspace::workspace_symbol_query —
     owning compile-once profile, typed None | evidence admission, total
     deterministic evidence comparator, legacy order projections, work receipt
```

`perl-workspace` is above `perl-symbol`, below `perl-lsp-rs-core`/`perl-lsp-rs`,
and already owns the live index-path search policy, so it is the narrowest
existing owner location. Provider-neutral: no LSP wire types are imported or
re-exported by the new module. `symbol_query::matches_query`/
`compare_names_by_query` become forwarding shims over the new owner so the
provider call sites and their extensive test suites keep compiling while the
numeric-fallback tier disappears structurally.

## Behavior parity classification

Preserved exactly (parity fixtures, no silent change):

- membership of every tier incl. short-query restriction measured on the
  lowercased query ('İ' expansion keeps loose tiers);
- provider comparator order: tier asc → raw len asc → raw lexicographic;
- index-path order: rank desc (exact 3 > prefix/substring 2 > subsequence 1)
  → raw name asc (no length tiebreak);
- generated-path admission (no subsequence tier) and its
  name/uri/range final sort; generated browse returns empty;
- `(uri, start_byte)` dedup in the index path (row identity is #10645/#10642).

Known pre-existing cross-path divergence (prefix-beats-substring ordering vs
merged rank) is preserved per path and pinned by fixtures; unifying it is
#10645/#10642 scope, not a silent correction here.

## Explicit non-normalizations recorded

No NFC/NFKC, accent folding, transliteration, locale collation, grapheme
segmentation, package canonicalization, sigil/qualification intent, boundary,
acronym, or multiword behavior is added. Case folding remains exactly
`str::to_lowercase()`; length gates count `char`s of the folded form.
