# Checklist: #12330

- [x] Verify currentness: no existing PR for #12330 (only merged #12427 for the #12186 substrate); branch off fresh `origin/main`.
- [x] Read the full issue body, the #12186 landed vocabulary, and the #12187 consumption contract.
- [x] Instantiate the #12186 vocabulary only — no second type system, no serde, no manifest/file syntax, no CLI, no evaluation, no product behavior.
- [x] Encode `compiler_local_lexical.v1` (22 own rows, no imports, no #8722 prerequisite).
- [x] Encode `compiler_static_project.v1` (19 own rows + verbatim local import = 41).
- [x] Encode `compiler_bounded_execution.v1` (19 own rows + verbatim project import = 60).
- [x] Encode `compiler_maintained_code_intelligence.v1` (19 own rows + verbatim execution import = 79, one explicit unsupported #8722 row).
- [x] Canonical owner map instantiated as navigation/ownership identifiers; every row owner references the map (tested).
- [x] Falsifier tests for all 14 issue falsifiers (see `inventory.md` mapping).
- [x] Acceptance/closure tests: exact row-ID inventory, import-chain closure, pinned digests, conjunctive required set, no-evaluation source scan.
- [x] Pin the four profile semantic digests as the semantic-drift gate.
- [x] Focused proof green: `cargo test -p xtask --locked compiler_profile_initial_rows`, `cargo fmt -p xtask -- --check`, `git diff --check`.
- [ ] #12187 manifest tooling consumes `initial_profiles()` without transcription (follow-up, not this PR).
- [ ] #12177 evaluation train consumes the checked inventory (follow-up, not this PR).

## Writer conflicts / rollback / stop conditions

- Single candidate branch `fix/issue-12330-compiler-profile-initial-rows`; one writer.
- Rollback = drop the additive module + `.spec` packet + one `mod` line; no runtime state to unwind.
- Stop conditions honored: no manifest parsing, no receipt adapters, no evaluation or product behavior, no new compiler/provider/world/EIR/client implementation, no duplicate receipt schema, no current status, no support/release/tag/publication action.
- Pre-existing conditions not owned by this PR: 3 repo-state-dependent xtask unit tests red on pristine main (#12467); `cargo clippy -p xtask --all-targets` red on pristine main (ungated). This module adds no clippy warnings of its own.
