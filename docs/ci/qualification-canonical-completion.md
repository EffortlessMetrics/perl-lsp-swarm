# Canonical callback and completion manifest (staged)

This is the concrete post-execution callback for
`qualification_runtime.RustSmallOwners`, plus the producer half of the existing
`Adapter.inspect` manifest contract. It installs no callback, workflow, token,
permission or HTTP write path. Publisher approval remains separate.

## Canonical callback

Trusted control composition constructs:

```python
owner = CanonicalOwner(
    verifier=absolute_prebuilt_verifier_path,
    verifier_sha256=trusted_installer_digest,
    control_sha=acquired_plan['control'],
    workspace=absolute_candidate_checkout,
)
# RustSmallOwners(..., canonical_owner=owner, ...)
```

The callable signature is `(receipt_path, acquired_plan, selection) ->
CommittedProof`. It invokes only the existing executable interface:

```
/absolute/trusted/xtask rust-small-proof --verify-receipt /absolute/imported/rust-small-proof.json
```

No Cargo build/product command runs in this callback. The canonical Rust owner
still validates the entire ordered argv vocabulary, exact clean H/toolchain,
census, outcomes and failure tail. The Python callback projects that owner's
verified observations; it contains no replacement list of canonical commands.
`Selection.selected` must equal the canonical receipt's ordered step names,
provided by the existing selector/normalizer, not guessed from fixture names.
Policy/portfolio/request-key identity must match the acquired plan.

The prebuilt executable path, SHA256 and association with control S are supplied
by the trusted build/tool installer. The constructor is not provenance or an
installer: candidate JSON or a caller-authored digest cannot qualify a binary.
The callback verifies regular executable custody and matching bytes before and
after invocation. It runs in the candidate H checkout with the existing pinned
Rust1.95/no-auto-install environment and strips metadata credentials and Git
overrides. The native private packet retains repository/S/run/attempt and the
captured H/tree/B/S/key plus producer-check ID; the central importer remains the
authentication authority.

Receipt/context bytes and device/inode/size/mtime/ctime/link-count metadata must
remain unchanged through verification. Changing bytes and restoring them cannot
hide a transient mutation. A verifier exit zero does not certify product success:
coherent `product_failure` maps to failure, while cancellation/instrument outcomes
remain unavailable. Rejection/timeout cannot become product failure or success.
Returned receipt digest and native producer reference match `RustSmallOwners`'s
existing association checks.

Tests execute a synthetic prebuilt binary to exercise the actual subprocess and
mapping path. They do not claim actual canonical Rust semantic execution. That
requires a trusted installer-produced binary for the exact composed control
source; do not compile or install one implicitly in ordinary admission.

## Completion manifest

```python
manifest = qualification_completion.build(
    adapter, verified_qualification, check_run_id=native_check_id_or_none
)
```

The input is the existing typed `VerifiedQualification` produced from native
custody and canonical semantic proof, not candidate JSON. No check is created by
this function. Without a native check ID, it retains the actual proof counts and
canonical receipt digest but emits `result: not_proven`, `check_run_id: null`.
Successful canonical proof alone does not qualify H.

With a check ID, GET-only readback must match the prepared repository/H, approved
App15368, exact Rust Small context, completed success or product-failure result,
external ID `<request_key>:<run>:<attempt>`, and exact native attempt URL. The
subject and current in-progress native S attempt are reacquired after readback.
The final manifest uses the existing schema version1 and exact fields consumed by
`Adapter.inspect`: repository/name+ID, PR, H/tree/B/S, policy/portfolio, key,
workflow ID/path, run/attempt, selected/executed/failed, result and check ID.
The optional canonical receipt digest is retained as supporting evidence.

The existing consumer runs after native control completion and rechecks artifact
origin, full manifest binding and real H/app/check association. Compiler-free
roundtrip tests execute this actual producer and consumer together with a
synthetic native GET service, preserving success, product failure and absent
publication. No native publication or product execution is claimed.

## Source composition interfaces

Extract `RustSmallOwners`' existing `VerifiedQualification` construction into a
shared `verified_qualification()` method and let publication preparation and
completion both consume it. This keeps canonical/native verification in one
place rather than duplicating the existing method body in a workflow.

Entry admission and current-run completion have different evidence boundaries.
`RustSmallOwners.committed_proof` imports a packet bound to the current native
run/attempt. That is post-execution proof and cannot be the prerequisite for
allocating its own execution. The entry adapter must consume already committed
proof or the existing isolated-replay result through separate entry-selection
and committed-entry-proof ports; missing ports must remain NOT_PROVEN, not fall
back to the current-run packet. The final pre-merge guard remains unchanged.

The central owner retains native subject capture, H checkout, selected execution,
artifact import and callback installation. The existing selection/build owners
retain authoritative portfolio/command selection, trusted prebuilt tool identity
and allocation. This source adds the canonical subprocess callback and completion
builder without changing their files or granting publisher permission.

Focused checks:

```
python3 -B -m unittest scripts.ci.test_rust_small_canonical_owner scripts.ci.test_qualification_completion
```

Add those modules and their source paths to the existing inexpensive CI self-test
route during central composition. No broad qualification is requested by this
source preparation.
