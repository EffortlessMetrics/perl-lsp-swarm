# Canonical Perl receipt verifier: Linux cloud readiness

Observed 2026-10-15. This is a bounded cold verifier experiment for PR #16134,
not Contabo product, native custody, activation, or full hosted policy proof.
No verifier defect requiring a source repair was reproduced.

## Exact subjects

- Perl commit: `f5e0f96537e8d71f6b592e6e798d073f6ead0e3e`.
- Perl tree: `3844ce977d23a5f03f556f4869d91d6bdf0c3464`.
- Published workflow: `EffortlessMetrics/em-ci-workflows/.github/workflows/rust.yml@752fad709cf3da63a69d004af2867b0e77ad0af2`.
- Independently fetched public workflow matched the retained source byte for byte:
  Git blob `a01c520d215445bcf293beb18a4ee4ca98df6409`, SHA256
  `192a7202bfc2caee4a6ee66f57a352f85a1b8197106c538b8fb354525ea3cac0`.

The checkout stayed clean at the Perl subject throughout compilation and all
behavioral proof. This documentation was added afterward. The caller owner
must integrate any documentation through its existing branch; no competing
workflow or migration PR is introduced.

## Experiment and admission

The host was Linux x86_64, glibc 2.41, Python 3.12.14, with a 16 GiB cgroup
memory limit and four CPU equivalents of quota (five CPUs in affinity).
Rust was already installed under `/workspace/tooling`; no installation occurred:

```text
rustc 1.95.0 (59807616e 2026-04-14)
cargo 1.95.0 (f2d3ce0bd 2026-03-21)
```

The target, intermediate outputs, and registry were cold. The published toolchain
preparation block passed in 0.025 seconds using that installed compiler. An absent
`9.99.0` toolchain, explicitly labeled as a synthetic control on the immutable
runner branch, refused with exit 78 without installing anything.

The measured Cargo request was:

```text
run -p xtask --locked -- rust-small-proof --verify-receipt <SYNTHETIC-success.json>
```

It ran through `scripts/cargo_admitted.py --budget-file <scoped-budget.json>`,
with `RUSTUP_TOOLCHAIN=1.95.0`, `RUSTUP_AUTO_INSTALL=0`, two build jobs, and the
policy's metadata credentials unset. Target and intermediate paths were private
to the canonical worktree. Cargo home and temporary paths were also on the
workspace volume. Initial free space was 31,793,438,720 bytes.

The operator declared a task-specific 12 GiB reserve plus 12 GiB total additional
peak ceiling before the first admission attempt. Preflight and execution admitted
the exact scoped request. This was an unverified conservative experiment budget,
not a generally qualified host profile or a change to the default 40 GiB reserve.
A two-second sampler requested cancellation at either storage ceiling or 2,100
seconds; no ceiling was reached. No storage or permission expansion followed a
refusal, and no denied Mac operation was retried.

The supported admission route disables incremental compilation and separates
target and intermediate paths. The published policy uses its own private target.
Thus this is cloud readiness evidence for the admitted equivalent, not an exact
measurement of the published job's entire environment.

## Observed results

| Observation | Result |
| --- | --- |
| Cold compile plus synthetic receipt verification | Exit 0; 854.314 seconds wall time; Cargo reported 14m 12s compilation |
| Sampled peak additional disk across the workspace volume | 1,809,408,000 bytes (1.685 GiB) |
| Retained disk growth at cold command completion | 1,714,892,800 bytes |
| Sampled minimum free space | 29,984,030,720 bytes (27.925 GiB) |
| Observed cgroup memory peak at cold completion | 5,805,961,216 bytes (5.407 GiB), including environment baseline and cache |
| Focused Rust helper tests, separate warm-dependency build | 62 passed, zero failed; 434.233 seconds wall time, 1.36 seconds test execution |
| Focused Python packet/export/import controls | 23 passed (6 wiring, 12 export, 5 import-adapter controls) |

The separate Rust command was
`test -p xtask --bin xtask --locked rust_small_proof:: -- --test-threads=2`,
with its own exact scoped admission. Its sampled additional disk peak was
375,304,192 bytes; total observed cgroup memory peak subsequently reached
7,471,448,064 bytes. Those test-build figures are not the verifier command's cost.

Both Cargo operations emitted `tree_settled: true`, proof
`kernel ECHILD (__WALL)`, no settlement errors, and `lease_released: true`.
The receipts also recorded `owned_child_table_available: false`. Normal completion
was proven; cancellation and detached-child discovery were not qualified here.
Build/cache resources and the immutable executable were retained, not cleaned.

An immutable copy of the actual built executable, SHA256
`de1269fc799872ca4f540e32f10d2a60cc4129b77598ed3333af15bc476979ba`,
accepted the adapted synthetic ten-record success fixture and rejected omitted
step, wrong argv, wrong source, dirty subject, swallowed failure/false success,
and missing Rust tools. The intended error was inspected for each refusal.
It also accepted a coherent `ProductFailure` diagnostic receipt as shape-valid;
that does not authorize policy success. No product step was executed.

Raw local logs are retained outside the source tree. Their SHA256 values are:

- Cold command: `10e9501c27a17eb2a64bd7271864d2e3af86ec2a55aa4ef0c74b31fe1122004e`.
- Rust tests: `7ec295981178d0a469e5ebbb563af8e1901e89063679d1f37b4e7febf1f4b409`.
- Executable control summary: `14dbbc6e491109691244066241eb6a0527df5ed6d859f0f8f6760da526c973f3`.

## Remaining proof boundary

The cold command fit within 35 minutes on this cloud host. The complete policy
job, including authentication/import, source checkout and verification, result
policy, finalization, and cleanup, remains unmeasured. The compiler was preinstalled;
hosted download/install failure and latency were not exercised. Sampled disk and
cgroup observations are not hard quotas or universal peak bounds.

Real Contabo revision-9 image/tool identity, actual product receipt and custody,
native check/run/attempt binding, native cancellation/recovery, full policy timing,
and post-job cleanup remain NOT_PROVEN. No workflow was dispatched, pin activated,
protection changed, release published, or deployment performed. This evidence
does not replace the caller owner's remaining qualification and integration work.
