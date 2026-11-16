# Retained campaign packet adapter (#17897)

`retained_packet.py` consumes delivered ZIP bytes and saved Actions API artifact
and run records. It passes actual report objects to the shared evaluator checks;
it does not parse Cargo or libtest again. No workflow, admission, cleanup or
production qualification is activated. Existing owners retain execution custody.

The adapter accepts explicit expected run, attempt, artifact, head, report member
names and optional Cargo argv. These are caller inputs from the campaign owner,
not a replacement experiment manifest or authenticated attestation. Both a
self-consistent invented packet and an actual retained packet remain
`production_qualification: NOT_PROVEN`. Diagnostic exit 0 means rendered output;
`--check` returns 1 until the missing execution authorities are implemented.

| Fact | Existing authority consumed | Remaining boundary |
| --- | --- | --- |
| Requested cold/warm/edit cell | #17655 campaign owner | No authoritative requested-cell document is delivered by the pinned reporter; no new registry is invented. |
| Run/head/attempt | Actions run record `id`, `head_sha`, `run_attempt` | Saved JSON is not authenticated delivery custody. |
| Artifact/run/head | Actions artifact `id`, `workflow_run.id/head_sha` | Artifact record has no attempt identity; listing it for a run cannot prove its attempt. |
| Delivered bytes | SHA-256 of the actual ZIP against API `digest`; actual length against `size_in_bytes` | Missing, expired, inaccessible or corrupt delivery is unavailable proof. |
| Source and dirty edit | Report head checked against caller and API records | Copied head.txt does not attest source at execution. Dirty content/source-manifest/pin authority remains missing. |
| Effective Cargo argv | Caller argv digest compared with admitted descriptor `scope.cargo_argv_sha256` and operation | Matching digests do not independently attest the caller or execution-time source/configuration. |
| Toolchain/config/lock | Existing admitted scope and retained rustc/cargo inputs | No complete execution-time join is exported; no post-hoc binding is manufactured. |
| Package/target/features/profile | Existing reporter's canonical unit key and actual per-unit projection, checked through evaluator | Pinned reporter exports only its hash. It must add `unit_projection=unit_rows` at its existing final result boundary; missing projection stays missing. |
| Tests/ignored/outcomes | Existing `case_outcomes`, counts and digests | Shared evaluator checks multiplicities and ordinary outcome/footer consistency; identical counts are insufficient. |
| Owner/lease epoch/closure | Existing terminal owner descriptor, inode identities and settlement | Closure consistency is independent of source/attempt authority and execution success. |
| Measurement/reserve | Existing sampler summary and reserve observations | Sampled values are not continuous bounds or a supported reserve; open-unlinked, proof/cache/temp/packaging accounting remains required. |
| Attempt elapsed | Each report's command and sampler elapsed fields, kept separately for initial/retry | Summing command clocks is not cumulative gate or whole-experiment elapsed. |
| 1500/1320 policy | Existing gate-policy/execution owner | Watchdog versus declared performance budget mapping remains unresolved. No invented `mandatory_work` value is consumed. |

The retained ordinary artifact for run 38337480483, artifact 11787170608, head
540201d5dad7b7622628c61e463e4a366154b0b4 has SHA-256
0938878fc0267936aa0181d9b0527042e78a5448fed843c80b88d7fdd70ca1ab.
It contains both `summary.json` and `retry/summary.json`. The initial command timed out with status 124; its retry exited 101 after
1197.76140776 seconds; missing package metadata/projection and source/attempt
authority must remain visible. This is a real partial packet, not a success or a
savings denominator. Artifact expiry is 2026-10-27; retain downloaded bytes and
their acquisition record under the existing campaign evidence owner.

Run compiler-free controls from the repository root:

```sh
python3 -B -m unittest discover -s scripts/ci/qualification_packet_prototype -p 'test_*.py' -v
python3 -B scripts/ci/qualification_packet_prototype/prove_discrimination.py
```

Actual producer projection export and authenticated execution-time source,
operation, requested-cell and clock joins must be composed by #17623/#17654/
#17655 owners before production activation. The existing synthetic ceiling and
its tests are preserved. This patch does not close #17897 or qualify #17658.

## Diagnostic limits and validation surface

Archive input is capped at 128 MiB and each selected JSON report at 16 MiB.
Larger inputs refuse without extraction. Hashing and ZIP parsing consume the
same immutable bytes from one bounded read, so replacing the pathname cannot
substitute another report after the digest check. Duplicate members, corrupt
compressed bytes, duplicate JSON keys and nonfinite numbers refuse.

The existing evaluator uses host-native absolute-path checks. Validate retained
Linux receipts on Linux (including this host's existing Ubuntu WSL); a native
Windows invocation can refuse Linux lease/resource paths. This adapter does not
reinterpret foreign paths as local resources or claim Windows qualification of
the Linux measurement instruments.

The actual retry exits 101 in
`dancer_navigation_tests::dancer_route_target_definitions_to_named_sub` (36 pass,
one fail in that binary). The adapter preserves that original failure rather
than recasting every incomplete run as a timeout. Both original summaries have
nonzero vanished-entry observations; their sampled storage is not qualified.
