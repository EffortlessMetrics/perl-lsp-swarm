# Native Rust Small entry adapters (inactive)

`qualification_native_adapters.NativeEntryOwners` supplies concrete selection,
applicability and distinct warm-proof callbacks to the existing runtime. This is
source composition, not an installed workflow, allocation, native qualification,
publisher permission or replacement gate policy.

## Binding

Trusted control code constructs the existing `CanonicalOwner` with the installed
xtask path/digest, exact control S and candidate H workspace. Construct:

```python
entry = NativeEntryOwners(
    adapter, native_environment,
    canonical_owner=canonical, control_root=absolute_clean_control_S,
    native_warm_import=import_prior_native_packet,
    warm_run_id=prior_run, warm_run_attempt=prior_attempt,
)
owners = RustSmallOwners(
    adapter, native_environment,
    selection_owner=entry.selection,
    applicability_owner=entry.applicability,
    entry_committed_owner=entry.entry_committed,
    native_import=import_current_native_packet,
    canonical_owner=canonical,
)
adapter.eligibility = owners.eligibility
```

The existing central authenticated importer is bound separately for warm import:
`import_prior_native_packet(run_id, run_attempt, *, acquire_plan) -> absolute Path`.
It must authenticate the artifact, producer steps, custody and exact bytes to that
specified native attempt, using its native legacy-H or captured H/S contract.
It must not reinterpret this as the current requested run or accept caller JSON
as authenticated provenance. The current-run importer retains its existing
`(*, acquire_plan)` signature. This adapter does not duplicate central ZIP or
artifact authentication. Neither callback is installed by this source.

Warm acquisition refuses its own run (including an earlier attempt), incomplete
native runs, wrong repository/H/App, a check not present in that specific native
attempt's complete jobs inventory, moving run/check identities, and mutated
packet bytes or custody. Native H warm packets bootstrap selection independently
of the current request key. An S warm packet additionally requires the exact
captured H/tree/B/S/key; it cannot bootstrap a previously unknown portfolio/key
by assertion. Missing warm import remains NOT_PROVEN through the existing entry
join; a local path alone is not an authenticated importer.

## Selection and digest

The canonical verifier validates warm receipt semantics against clean exact H.
Only then are its full ordered names/argv projected. Actual installed commands
are invoked with global `--worktree-root H` to override xtask's compiled source
root, in addition to cwd H:

- `rust-small-proof --verify-receipt ABS_RECEIPT`
- `pre-push-plan --base B --head H --format json`
- `ci-scope --base B --root H --format json`

Both planners remain authoritative for their outputs. Planning calls Cargo
metadata; the wrapper uses offline mode, pinned Rust/no auto-install, strips
metadata credentials, Git overrides and Python path overrides, and revalidates
the clean canonical subject afterward. Planning is not installed on the cheap
metadata-only workflow and does not grant resource admission.

`portfolio(plan)` returns `(actual_outputs, portfolio_sha256, prior_reference)`.
The portfolio SHA256 covers compact sorted-key JSON containing the complete
pre-push plan, CI scope, canonical Rust names/argv and required context/App map.
It excludes request_key, which itself includes that digest. Use H-native warm
proof and an undispatched provisional Subject to acquire that digest; create the
final Subject with it and reacquire the final plan before installing callbacks.
`selection(plan)` refuses a requested digest that differs. Policy SHA256 covers
actual `.ci/policies/required-checks.toml` bytes in clean S. These definitions are
explicit caller contracts, not automatic acceptance of earlier arbitrary hashes.

A bounded pre-push plan does not prove the full Rust Small lane. Conversely, the
full Rust Small lane does not prove unrelated local selected commands. Exact
successful argv already present in the Rust-verified warm receipt can satisfy
the matching local selection. Otherwise the same authenticated warm packet must
also carry `command-proof-set.json`, the existing command-evidence owner's
`command-proof-set.v1` output. Its digest and exact selected argv enter the
portfolio; the complete packet is reacquired after planning and at entry.
Missing evidence refuses; Rust Small's vocabulary and denominator remain intact.

`qualification_local_commands.produce(canonical, environment, acquire_plan=...,
output=absolute_fresh_private_path, command_timeout_secs=...)` invokes existing
trusted xtask `--worktree-root H command-evidence proof-set --spec ABS --json`.
The spec comes from the actual `pre-push-plan`, with exact ordered argv, cwd H and
candidate H. It preserves the owner's actual JSON and failure exit; it never
writes pass fields. The caller must hold the existing resource lease and enforce
its absolute deadline. Checked native workspace, canonical execution workspace
and captured `product_workspace` must agree before any execution. Metadata
credentials and Git/Python overrides are stripped; admitted Cargo paths remain.
Shell command selections refuse rather than being silently reinterpreted.

The binder consumes only an already authenticated packet. It checks the actual
owner schema, every selected command, native producer workspace, candidate H,
complete success/exit/termination, and serial timestamps within native proof
entry/finish (including whole-second capture precision). Consumer paths may
differ from producer paths. Native H warm packets can bootstrap without an S
capture; S packets require exact captured H/tree/B/S/key. Neither local JSON nor
a caller-supplied path authenticates native execution.

**The native packet extension is not installed.** The current central owner
accepts exactly its Rust receipt/export/context members and has no producer
workspace capture. A follow-on must admit only this optional closed filename,
bind its bytes through that same native run/attempt/artifact and custody path,
and capture the actual producer workspace before product execution. Until then,
ordinary package scoped check, Clippy/all-target, docs/workflow/editor selections
absent from Rust Small remain NOT_PROVEN. Source fixtures prove the adapter join,
not actual scoped Cargo execution or native provenance. Deferred planner outputs
and all required contexts remain integration obligations.

## Existing integration results and allocation

`applicability(plan)` acquires bounded complete H check history and legacy status
history. Each context/App or legacy context/creator keeps a known failure through
pending, cancelled, skipped or neutral replacements until later success from the
same source. Wrong required App, unknown states and full/truncated pages refuse.
Missing future required checks remain `absent` obligations; they are not product
success and do not replace the final gate. The inventory comes from current S's
policy; final live protection/union remains with existing enforcement/final-gate
owners. This source does not modify or establish classic/ruleset protection.

`allocate_and_execute(owners, event, capture, allocation_owner=..., proof_owner=...)`
joins actual native entry to the existing desktop allocator's context manager:
`allocation_owner(plan, selection)` must acquire/release the real native lease and
enforce its budgets. The adapter reacquires entry/election after waiting, retains
the original one-shot captured context, and calls existing `execute_proof` only
inside that lease. Refusal/exception releases it; non-ready entry does not request
allocation. No resource policy, allocator implementation or permission is added.
The producer must also invoke trusted prebuilt xtask with `--worktree-root H`.

Compiler-free tests exercise actual subprocess transport and runtime joins with
synthetic xtask/importer/native API/allocator boundaries. They establish source
behavior, not canonical Rust execution, authenticated native artifact import or
resource containment. Preserve the operator's separate build/admission procedure
and pending publisher decision. Cutover remains inactive.
