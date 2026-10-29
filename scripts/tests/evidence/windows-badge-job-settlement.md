# Windows badge Job Object settlement evidence

Evidence only, not a process-supervisor policy or target-deletion authorization.
Observed 2026-10-15 for PR #17963, supporting #11659 / #13694.

## Environment and subjects

Native Windows 11 `10.0.26300-SP0`, AMD64; CPython 3.14.6, MSC 1944 64-bit.
No Cargo, Rust builds, unrelated-process signaling, or OS/security changes.
One task-owned Python tree at a time, private H: temporary targets, child
self-exit capped at 30 seconds. Parent releases the production launcher only
after assignment to a private non-breakaway Job Object. Cleanup uses that job
and the test's exact Popen handle; it never signals a PID from a host scan.

Original candidate: `265a97e9a5f9f9af8e0e390c0a20f4106a83a81f`.
Handle-wait revision source blobs qualified by the first focused runs (candidate `ee62a4bef55fec4fd50b5078b3a9bd6794593b9b`):

- `scripts/generate-badges.py`: `c0a9a573e2a6fe4fdceac1a0157854f1b334e77b`
- `scripts/tests/test-generate-badges.py`: `f4d9e697b3fcb24217c3ce110fcef66e9d84c2c9`
- `scripts/tests/test_windows_badge_job_native.py`: `254d4bc8be8eee9abd815828735c5eb08020ff0b`

## Observed counterexample and repair

Before termination each native case independently observed:

```text
leader_exit=0 child_live=true job_active=1 target_release=sharing_violation_32
```

The descendant closes inherited output pipes, retains a native file handle
without delete sharing, and publishes a private receipt. The test binds its
exact native process handle, verifies membership in its own Job Object, and
checks that target removal fails with Windows sharing violation 32.

The original implementation's accounting-zero return was insufficient. Three
initial native cases failed on an unsignaled descendant process handle after
`terminate()` reported success. A separate filesystem-first probe against the
immutable original source established the resource consequence directly:

```text
ORIGINAL_ZERO_PROBE: immediate_wait=258 target=blocked_winerror_32
ORIGINAL_FINAL: handle_signaled=true target_removed=true
```

The repair binds verified handles from the job's process-ID list before
termination, requests termination, polls zero accounting and waits for captured
members to signal within the existing grace budget. Binding, enumeration, wait,
and cleanup failures remain explicit. Handles close even on interruption.
The documented incomplete-list count relation is checked alongside ERROR_MORE_DATA.

## Executed proof

Use an existing **task-owned** directory for `$proofRoot`; these commands create
and remove only their own private fixture children and target directories:

```powershell
$env:PERL_LSP_RUN_NATIVE_JOB_TESTS = '1'
$env:PERL_LSP_NATIVE_JOB_ROOT = $proofRoot
python scripts/tests/test_windows_badge_job_native.py -v
python scripts/tests/test-generate-badges.py WindowsJobSettlementTests -v
```

- Native owner, leader-only negative control and real `run_ripr` caller: 3 passed.
  Every case observed the live locked descendant before termination and an exited
  exact handle plus successful target removal afterward. The leader-only mutant's
  false success is rejected by the real native-handle oracle before final cleanup.
- Final-source real-caller test: 1 passed (9.540s), after the final short-list guard.
- Final portable controls: 13 passed, including empty-job success, accounting zero
  with an unsignaled member, query/bind failures, partial binding cleanup, interrupt
  cleanup, resizing and successful-but-incomplete list responses.
- Affected existing launch/capture controls: 9 passed.
- Native opt-in off: 3 skipped, with no fixture launches.
- Independent review: native 3 passed once in a separate task-owned temporary root;
  portable 13 passed; independent process-list ABI/resize control passed. Native
  review evidence predates interruption-only cleanup hardening. Root's full native
  run covers that hardening; the final caller run covers the short-list guard.
- Full native suite on source blob `21ee2984addf15a9aecb69ebbf93988f589734ff`:
  3 passed (16.869s). Carried forward for the two unchanged native cases after the
  short-list guard; native responses were complete, and the guard does not change
  their execution path. Final portable proof exercises both guard branches.
- Diff hygiene passed. Broad suites and expensive qualification were not selected.

Final native observations were:

```text
NATIVE_POST: child_exited=true target_release=removed
```

All fixture temporary directories were removed after the runs. Original source
and leader-only negative controls are failed/synthetic contract results, not
provisioning failures. Earlier hosted `draft-no-proof` checks skipped analysis;
they are neither product failures nor passing qualification.

## Late-birth review repair

The post-snapshot limitation was challenged before integration. A task-owned
spawner creates a child only after the real native `process_handles()` snapshot
returns. The late child joins the same Job Object, opens the private file without
delete sharing, and publishes readiness. The fixture verifies the child's exact
handle/membership and the sharing violation before requesting termination.

Against `ee62a4bef55fec4fd50b5078b3a9bd6794593b9b`, the added native test failed
because the owner returned an empty failure list despite missing the late child's
handle. Final cleanup still waited for that exact owned child and removed only
the private target. This identifies an unproved late-member exit; the red test
does not claim every scheduling interleaving retains the lock after return.

The repair samples lifetime `TotalProcesses` before member capture and after
termination, zero accounting and captured-handle waits. A changed lifetime total
or either unavailable sample rejects success. The lifetime total includes exited
members, so a late child cannot evade the detector by leaving active accounting.
Conservative refusal is intentional even if the new member was itself captured
and exited. This is a refusal check, not an atomic creation barrier.

New focused execution: portable 16 passed; native 4 passed (8.296s), including the
late-born-member refusal and the existing native success/negative/real-caller
controls. The native fixture additionally records both real lifetime totals and
retains its 30-second process lifetime cap. Independent execution on the final
source/fixture passed native 4 (6.806s) and portable 16 (0.182s), recording real
lifetime counts `[2, 3]` and refusing success. Removing the final count comparison
in memory produced one intended portable failure and no instrument errors.
The independent final review found no blocker within this refusal/captured-member
claim and confirmed alignment with honest resource ownership; it did not certify
shared-target deletion safety.

Source blobs for the review repair:

- `scripts/generate-badges.py`: `68f740adb0aec896530f5dcb76e7f5a6eb960a25`
- `scripts/tests/test-generate-badges.py`: `6d64320fa2f2b3108ebf503afa816f27550acfa6`
- `scripts/tests/test_windows_badge_job_native.py`: `f398b9478afc7df4c71badabb157f48ff1f559f0`

## Limits

This proves the bounded fixtures, captured-member exit waits and refusal when
lifetime membership changes across capture/settlement. It does not prove general
safe target deletion. An already-counted process could disappear from enumeration
before capture while completing exit; unchanged totals do not prove historical
members' resources released. Enumeration and termination are not atomic.
The counter is a DWORD; theoretical full-width rollover is not qualified.
Shared targets, external handles, arbitrary descendant-creation patterns,
canonical Cargo supervision, and other process owners remain unqualified.
No shared target deletion is authorized by these results.

Microsoft's [TerminateProcess contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess)
prescribes a process-handle wait for asynchronous termination. The
[process-ID list contract](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_process_id_list)
defines the variable-length list and incomplete-count condition. Microsoft
defines [TotalProcesses](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information)
as a lifetime total including terminated members. These support the added
observations and refusal; they do not make the snapshot atomic.

## Retained transcript fingerprints

Raw logs remain in the task's private `perl-lsp-task8-native-proof` and
`perl-lsp-task8-native-review` evidence directories on the selected desktop.
This table preserves their content identities without publishing private paths.

| Transcript | SHA-256 |
| --- | --- |
| `native-initial.log` | `0a4e17782162e822735b65f583ed410b216ad6d04bb166257547350bae42aaab` |
| `original-resource-probe.log` | `d39700930947d87cc77115bac45ece19691af9e00bf41b8cbd113ce9cf42b0a3` |
| `native-final.log` | `3a494c4a05e00b640d17f7a7513d7d31dbf34dee391c101071690bf86fa3e458` |
| `native-final-caller.log` | `d26496993f0924f9360ebce30fd1506ba8a5baa104e18762fd270d1d42b90ab8` |
| `portable-final.log` | `b8568237a6b238b8b436d6a1342b8b43a2bb7e369232fbe7f6b44a9650057ca9` |
| `affected-final.log` | `48c522f808bb0aa46d317bb70cee72db5f0cd4d6ee8acb534eedfb11e1bb7f4c` |
| `native-independent.log` | `2bebc4ac0a75c042f33785dd74339f9882a4f296b8bbd88239491417dc3a5fef` |
| `native-late-birth-red.log` | `17bc95d503e3d89f8b9f3c11f6aacf26833f8e1f69ddfb942211df6daa239d96` |
| `native-late-birth-final.log` | `03b67eecd6eda0e60105cbacbf6c45e9fcbc02bd1a6aa9dec0236e2f529fdf68` |
| `portable-late-birth-final.log` | `4476f0817043928ff9116d897042f7d621c3ceb632e545e7c7ba18e3ea24cfc6` |
| `native-late-birth-independent.log` | `c321adff0124655ae17e9607a98c5a487b21842c0898485099d8e118cd809370` |
| `portable-late-birth-independent.log` | `d0e64cad96f3fb603cb86ea873792bae8f039b189c8d0f95523e512326841c70` |
