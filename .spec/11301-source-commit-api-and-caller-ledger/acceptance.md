# Acceptance: #11301

| Obligation | Evidence |
| --- | --- |
| Initial and live contracts have distinct names and inputs | `WorkspaceIndex::index_initial_file*`, `index_initial_files_batch`, and `index_live_file` |
| Live source identity cannot be zero at the API boundary | `SourceCommit` stores `NonZeroU64` identity and `NonZeroU32` generation |
| Typed live outcomes preserve accepted, no-op, stale, and failure distinctions | `SourceCommitOutcome` plus workspace unit tests |
| Batch indexing remains load-bearing | Initial batch delegates to `index_files_batch`; existing batch tests remain in place |
| Every remaining direct caller has an owner, role, successor, and removal condition | `caller-ledger.toml` and deterministic validator |
| New compatibility or unledgered callers fail the gate | `scripts/ci/check_source_commit_api.py` |
| didOpen/didSave and unrelated lifecycle/provider semantics remain out of scope | text-sync callers are explicitly ledgered as deferred; no lifecycle authority moved |
