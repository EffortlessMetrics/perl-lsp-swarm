# Context — #10645 retain the best matched key for each canonical row

Base: `main@8aaadee46` (post-#10794 substrate via PR #11960).

## Defect (current-source confirmed)

`WorkspaceIndex::search_source_symbols_with_profile`
(`crates/perl-workspace/src/workspace/workspace_index.rs`) iterates the
`search_index: HashMap<String(name_key), Vec<WorkspaceSymbol>>` buckets, scores
each admitted key through `match_searchable_key`, and deduplicates physical
results by `(uri, start_byte)` **first-insert-wins** before ranking. One
declaration is indexed under several keys (`symbol.name`,
`symbol.qualified_name`), so which key supplies the retained evidence depends
on `HashMap` iteration order. A later, stronger match for the same row is
discarded by the `seen` set and cannot be recovered by the final sort.

Consequence: `Package::run` queried with `run` may retain substring evidence
from the `Package::run` key while the bare `run` exact key was suppressed.
Key construction, hash seed, and iteration order change the final rank.

## Searchable-key inventory (current main)

| Producer site | Keys inserted | Roles today |
| --- | --- | --- |
| `rebuild_search_index` / `incremental_add_search` / `incremental_remove_search` | `symbol.name` (bare) and `symbol.qualified_name` (qualified, incl. legacy `'` separator spellings when produced upstream) | detected at query time: contains `::` or `'` → QualifiedName else BareName |
| `search_generated_workspace_symbols_with_profile` | `bare_name` and `entity.canonical_name` per GeneratedMember entity | GeneratedFrameworkProjection |
| open-document fallback `WorkspaceSymbolsProvider::search*` (`perl-lsp-rs-core`) | one bare-name key per stored symbol; no multi-key row exposure | BareName |

No other production site inserts several searchable keys for one row.

## Geometry / first-key dedup sites inventoried

- `search_source_symbols_with_profile` — the defect seam (this PR repairs).
- `search_generated_workspace_symbols_with_profile` — OR-admission over two
  keys discards all evidence (no ranking today); repaired to best-key
  selection with identical membership in this PR.
- Open-doc provider paths match one key per row — out of defect scope.
- Definition-candidate and reference dedup maps elsewhere in this file are
  not workspace-symbol row-search seams and are untouched.

## Canonical row identity status

#10641/#8756 (`CanonicalWorkspaceSymbolRowId`, generation-owned key→row maps)
are still OPEN; no such types exist on main. This PR therefore:

- aggregates per row using the **whole indexed-symbol value tuple** (name,
  kind, uri, full range, qualified_name, documentation, container_name,
  has_body, workspace_folder_uri, is_lexical) purely as *duplicate detection
  for index-internal clones*;
- never uses `(uri, start_byte)`, URI strings, names, containers, or
  serialized LSP equality as *semantic* identity;
- collapses strictly fewer rows than the old geometry key: two distinct
  projections sharing one anchor differ in some value field and stay distinct
  (old code collapsed them);
- leaves true canonical row identity to #10641/#8756; the aggregator itself is
  identity-agnostic (caller associates row payloads).

## Profile/evidence comparator ownership

Admission and intrinsic comparison stay owned by #10794
(`match_searchable_key`, `WorkspaceSymbolMatchEvidence::compare`). This PR
adds one accumulation authority (`BestRowMatchAccumulator`) that consumes
`compare()` plus a reviewed role-ordinal tie-breaker and refuses
profile-mismatched evidence (typed outcome, counted). It adds no tiers, no
query logic, and no per-version branching, so #10806/#10827 profile
extensions require no new aggregator.

## Handoff to #10642

New transport-neutral per-row result:
`BestWorkspaceSymbolRowMatch { profile_version, profile_digest,
winning_evidence (key/role/tier/positions), runner_up_evidence }`, exposed via
`search_source_symbols_ranked_with_profile/_with_receipt` together with
bounded work counters. #10642 owns cross-row dedup/order/budget later;
nothing here performs it.

## Distinguishing the four confusions

- multiple keys for one row → aggregated here (best wins);
- multiple retained rows for one entity/anchor → kept distinct (value-tuple
  grouping never merges them);
- same-name rows in several sources/roots → distinct uri/folder fields keep
  them distinct;
- cross-row semantic dedup/ranking/cap → #10642, explicitly out of scope;
  the request cap applies only after per-row best-key selection.
