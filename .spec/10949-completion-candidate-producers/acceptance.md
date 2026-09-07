# CAND-INV-01 acceptance

Basis: main@30efa8a. All commands run from the repository root.

## Current inventory state

```text
53 producers across 18 candidate classes
 0 construction-only files
 0 post-finalizer appends
38 scanned source files
 2 shipped entry points
```

Every producer row currently carries `identity = legacy_label_compatibility`
(routers and finalizer seams carry `not_applicable`) and
`source_completeness = legacy_unreported`. That is the honest current state, not
a placeholder: no live producer uses the semantic or source-anchored
constructors PR #10145 landed, and no live producer reports whether it finished.
The second fact is what stops #10230 claiming a `Complete` outcome for any
request.

Three rows are recorded unreachable from every shipped entry point:
`CompletionProvider::get_completions` (test- and documentation-facing provider
entry), `CompletionProvider::add_file_completions` (dead
`#[allow(dead_code)]` compatibility wrapper), and
`workspace::add_use_module_completions` (superseded by the cached variant). Each
names why. They are recorded rather than dropped because an unreached producer
is the obvious wrong place for a future fix to land.

## Proof commands

```bash
cargo fmt -p xtask -- --check
cargo clippy -p xtask --all-targets --locked -- -D warnings
cargo test -p xtask --bin xtask --locked completion_candidates
cargo xtask completion-candidates check
cargo xtask completion-candidates list
cargo xtask completion-candidates explain \
  perl_lsp_rs::runtime::language::completion::LspServer::add_dancer2_keyword_completions
cargo xtask completion-candidates graph   # second run leaves no diff
cargo xtask non-rust inventory --check
git diff --check
```

## Falsifier results

Twenty-two ledger falsifiers, each corrupting the reconciled checked-in ledger
along one axis and asserting the refusal names that axis:

| Axis | Refused because |
| --- | --- |
| producer with no row | append/return seam absent from the ledger |
| row naming a dropped producer | no scanned source declares it |
| duplicate producer id | duplicate row |
| source edited without re-audit | `source_digest` mismatch |
| controller as implementation owner | `#8969` cannot own a row |
| owner that is not an issue | `migration_owner` must be an issue reference |
| permanent compatibility, no reason | unowned exception |
| producer outside the shared finalizer | forbidden route |
| unidentified candidate | forbidden identity |
| edit with no owner | forbidden insertion plan |
| unestablished evidence | forbidden evidence |
| unclassified rank | forbidden rank |
| catalogue identity on a resolved candidate | reserved for server-authored catalogues |
| declared reach the call site contradicts | reconciled against entry-point calls |
| unowned route divergence | no `route_divergence_owner` |
| provider-seam row dropping an entry point | must list every entry point |
| unreachable row with no reason | no `unreachable_reason` |
| dead row back in service | called directly while recorded unreachable |
| construction-only row for a producing file | needs a producer row |
| empty `limitations` | not a disposition |
| append after finalization | candidate never ranked |
| nondeterministic generation | second render differs |

Plus positive controls: the checked-in ledger reconciles, the checked-in
projection is current, generation is byte-identical on a second run, discovery
is non-vacuous (≥40 producers, both entry points still call the finalizer), and
the Dancer2 route divergence is observed directly in source.

Predicate-level fixtures prove the denominator does not drift by accident:
`&mut Vec<CompletionItem>` and its fully qualified form are append channels;
`&mut [CompletionItem]` (reorder in place), `&Vec<CompletionItem>` (read-only)
and `&mut Vec<String>` are not. `Vec<CompletionItem>`,
`(Vec<CompletionItem>, bool)` and `Vec<CompletionCandidate>` are return
channels; `Vec<String>` is not.

## Source mutations run against the real tree

Both were applied to product source, confirmed refused, and reverted.

1. **Hidden producer.** `pub fn add_sneaky_completions(completions: &mut
   Vec<CompletionItem>)` appended to
   `crates/perl-lsp-rs-core/src/providers/completion/completion/builtins.rs`.
   With the digest left stale, `check` refuses on the digest. With the digest
   accepted — the state an author reaches after re-auditing — `check` exits 1
   with: *"takes the candidate append channel but
   policy/completion-candidate-producers.toml has no row for it"*.
2. **Post-finalizer append.** `completions.push(injected_candidate());` inserted
   immediately after `sort_and_cap_completions` in `handle_completion`. `check`
   exits 1 with: *"candidate append after `sort_and_cap_completions` in
   handle_completion"*.

## Limitations

- Discovery is syntactic, and its ceiling is documented in the module header and
  the projection. A producer using neither the append channel nor a candidate
  return type would not appear as a row.
- Reachability is mechanically reconciled only for rows an entry point calls
  directly. Provider-seam rows declare their reach; the row records that it is
  declared.
- Row *dispositions* are an auditor's reading of the source, not a proof. The
  digest binds them to the exact source they were read against, so they cannot
  outlive it silently, but a wrong reading is only caught by review.
- The check is not a CI gate in this PR (see `context.md` boundaries).
