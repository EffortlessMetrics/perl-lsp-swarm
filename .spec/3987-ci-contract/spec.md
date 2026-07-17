# SWARM-CI-1: Repository Contract advisory

## Intent

Add one repository-owned, read-only advisory entry point for the cheap
current-head contracts that should run in a clean GitHub checkout. The entry
point must compose the existing `change_set` resolver and `ci_scope` classifier
instead of introducing a second change classifier or a workflow-local policy.

This is C1 of issue #3987. It establishes the exact-head receipt and the
invocation seam; it does not promote a new required check.

## Authorities

- #3987 owns the thin remote CI boundary.
- #3985 owns the shared affected-change classifier and pre-push proof boundary.
- `xtask/src/tasks/change_set.rs` owns base/head and changed-path identity.
- `xtask/src/tasks/ci_scope.rs` owns affected packages, reverse dependencies,
  wideners, risk tags, and selection explanations.
- `.ci/gate-policy.yaml` remains the gate-policy authority.
- Existing `Perl LSP Rust Small Result` and `ripr+ New Gap Gate` remain the
  protected required checks until a later migration proves equivalence.

## Command contract

```text
cargo xtask ci-contract \
  --base <base-ref-or-sha> \
  --head <head-ref-or-sha> \
  --receipt <path> \
  --summary <path>
```

The command runs in a clean checkout and fails closed when it cannot resolve
the requested repository/base/head identity. It writes a deterministic JSON
receipt and a concise Markdown summary. The default workflow invocation uses
the pull request base and head SHAs, never a mutable local branch name.

## Receipt schema

The receipt uses `ci-contract.v1` and contains:

```text
schema_version
repository
provider_action
base_sha
head_sha
changed_files
changed_surfaces
scope             # the existing ci_scope output, including selected lanes
checks[]          # id, reason, command, result class, detail
status
claim_boundary
```

Result classes are `SUCCESS`, `POLICY_FINDING`, `NOT_PROVEN`, `NOT_APPLICABLE`,
and `STALE`. `NOT_APPLICABLE` is reserved for an empty resolved range that
selects no checks. A command exit status of `1`, or explicit `WARN` output from
an advisory checker that exits `0`, is a policy finding. Instrument failures
use the repository command contract's exit status `2` and are `NOT_PROVEN`;
process timeouts are also `NOT_PROVEN`. A head mismatch observed before
or after execution is `STALE` and is always blocking for the advisory receipt.
The emitted status is the highest-precedence blocking class:

```text
STALE > NOT_PROVEN > POLICY_FINDING > SUCCESS > NOT_APPLICABLE
```

The receipt is advisory in C1. Its non-success status must be visible and must
not be translated into a protected branch status or a merge authorization.

## C1 contract checks

The first implementation runs only cheap, deterministic repository-owned
checks, selected from the changed surface:

- all changes: exact-range `git diff --check`;
- Rust changes: repository formatting check;
- workflow or shell changes: the repository workflow-contract self-test and
  trigger-policy checks; the self-test intentionally preserves the existing
  unarmed advisory boundary, while the separately owned workflow-contracts
  job remains responsible for external actionlint/zizmor execution;
- `.ci/` and `policy/` changes: the existing repository gate-policy checker;
- changelog changes: the existing Changie disposition checker.

The repository currently has no executable spec-graph validator for `.spec/`
changes. C1 therefore records those files through the exact diff and scope
receipt only; spec-graph enforcement remains a follow-up rather than being
misrepresented as a gate-policy check.

Each selected check records why it ran and its command. No compile, test, RIPR,
review, GitHub API, package publication, or merge operation belongs in C1.
The affected Rust/protocol proof remains the #3985/C2 boundary.

## Clean-checkout workflow

Add one advisory `Repository Contract` job to the existing PR workflow. It runs
only for non-draft current heads after the existing latest-SHA preflight,
uploads the receipt and summary, and leaves all current required jobs unchanged.
The job must use the exact event base/head SHAs and must not use a mutable
`origin/main` fallback for the evaluated identity.

## Tests and proof

Focused tests must cover:

1. docs-only selection has the exact-range check and no Rust proof;
2. Rust selection includes formatting and retains scope identity;
3. workflow and structured-file selections are deterministic;
4. command success/failure/instrument failure map to the documented classes;
5. the exact requested head is used by the diff check;
6. JSON and Markdown receipts preserve full base/head object IDs;
7. the command's resolved identity rejects malformed or missing base/head
   values rather than reporting a successful receipt.

Proof commands:

```text
cargo fmt -p xtask --check
cargo test -p xtask --bin xtask tasks::ci_contract
cargo clippy -p xtask --bin xtask -- -D warnings
git diff --check
cargo allow doctor
```

No cargo-allow exception is expected. Existing exceptions remain owned by the
current policy ledger; this slice must not add a test or panic carve-out.

## Non-goals and follow-ups

- C1 does not replace or rename required checks.
- C1 does not implement the affected behavioral proof from #3985.
- C1 does not collect review, Changie GitHub, ruleset, merge-group, or merge
  evidence beyond the local repository contract command it explicitly runs.
- C1 does not promote the advisory job or retire duplicate workflows.

C2 consumes the shared classifier for affected proof. C3 establishes local/
remote fixture parity and cost measurements. C4 wires the result into #3988,
ruleset migration, merge-group coverage, and legacy-check retirement.
