# Cargo work on a constrained host

Use the existing [Cargo storage admission](../agents/CARGO_STORAGE.md) route.
A generic repository or writer preflight does not establish that a Cargo build
fits. Moving output under `DEVPLANE` does not add capacity unless it changes the
actual filesystem. Do not lower `MIN_FREE_GB` merely to make a command run.

## Inspect the exact operation before building

After selecting the host's storage root and the smallest relevant proof command:

```bash
scripts/cargo-admitted --preflight check -p perl-parser --lib --locked
```

This inspects the same effective private target/build, Cargo home, and temporary
paths used for execution. The shell entrypoint still checks the installed
pinned toolchain, with implicit rustup installation disabled; it does not build
Rust. For a capacity-only inspection without that toolchain probe, use:

```bash
python3 scripts/cargo_admitted.py --preflight check -p perl-parser --lib --locked
```

The Python preflight creates no directories, acquires no lease, launches no
Cargo process, and deletes nothing. Its `cargo-admitted preflight:` JSON report
is written to stderr. Exit 0 is a passing capacity snapshot with an observed
unoccupied lease path; exit 75 is refusal. An existing or dangling lease path
remains occupied pending its owner's verification. A missing or invalid capacity
observation is not a pass.

The report includes the current request scope even when the default disk floor
refuses it. Byte shortfalls and unprobeable destinations are explicit. The
existing resource descriptor on execution remains on stderr as well.

Neither preflight success nor an accepted budget reserves bytes. Execution must
recheck capacity after acquiring the existing repository/host lease. Separate
repositories, independent clones, raw Cargo, legacy wrappers, unrelated
processes, and independent later artifact consumers are outside that lease.
The root/native orchestrator must account for those consumers and own the build
lane. A preflight report cannot be replayed as permission for a later command.

## Supply an explicit host budget when the default does not apply

A host operator may select a smaller reserve only with a defensible sizing basis
for the exact work. The `--budget-file` option consumes one caller-owned JSON
policy, not a repository-wide default or an inferred cloud-machine class.

The document has exactly these fields:

| Field | Required value |
| --- | --- |
| `schema_version` | Integer `1`. |
| `scope` | The complete `scope` object from this command's preflight report. |
| `reserve_bytes` | Positive integer bytes to retain as the host safety reserve. |
| `expected_growth_bytes` | Positive integer bound on total **additional peak** storage across all admitted destinations. |
| `basis` | Nonempty reference to the measurement or approved conservative sizing basis. |

Do not copy the test fixture's numbers as host recommendations. No measured
small-cloud build envelope is shipped with this option. Existing artifacts are
already reflected in free space; budget the additional peak, including
intermediate artifacts, final outputs, dependency downloads, and temporary
files. A build profile change or cold cache can invalidate a warm-build bound.

Both byte counts and their sum must fit a signed 64-bit integer. Booleans,
fractional values, zero, negative values, nonfinite JSON, duplicate keys, unknown
fields/versions, and files larger than 64 KiB refuse. Use an absolute regular-file
path with no linked/junction components, consistent with the existing storage
path contract. The sizing reference is limited to 4,096 characters and must not
contain credentials or private log contents.

The scope binds hostname, platform, canonical invocation directory, canonical
worktree, effective storage paths, job count, the exact Cargo argument-vector
digest, and a digest of selected build/toolchain environment overrides. Argument
values and override values are not printed in the scope. Changing packages,
features, profile flags, program arguments, directories, output paths, jobs, or
captured overrides requires a matching newly qualified scope.

This is **not** a source-tree, installed-compiler, Cargo-config-file, quota, or
cache-state attestation. The operator must requalify the sizing basis when any
of those change materially. The file digest proves which declaration was read,
not that its estimate is correct. Reports explicitly retain `basis_verified:
false`; no software test or syntactically valid reference promotes that to a
measured host result.

Keep the policy file in host-owned configuration rather than the tracked
repository. For a policy already prepared at that location:

```bash
scripts/cargo-admitted --preflight \
  --budget-file "$HOME/.config/perl-lsp/cargo-budget.json" \
  check -p perl-parser --lib --locked

scripts/cargo-admitted \
  --budget-file "$HOME/.config/perl-lsp/cargo-budget.json" \
  check -p perl-parser --lib --locked
```

Wrapper options precede the Cargo command. Arguments after Cargo's `--` remain
program/test arguments, not wrapper options. The existing supported-command and
path/job-override refusals remain: Clippy, nextest, arbitrary aliases, and `xtask`
require separately admitted routes rather than raw-Cargo fallback.

`--budget-file` conflicts with **any presence**, including an empty value, of
`CARGO_STORAGE_POLICY`, `MIN_FREE_GB`, `MAX_USED_PCT`,
`CARGO_EXPECTED_GROWTH_GB`, or `CARGO_STORAGE_BUDGET_EVIDENCE`. Remove obsolete
capacity overrides explicitly; no option silently wins over another policy.

For each effective destination, execution requires:

```text
observed free bytes >= reserve bytes + total additional peak growth bytes
```

The total growth bound is conservatively required on every destination. It is
not multiplied by the number of paths sharing one volume. Conversely, the
wrapper does not apportion growth between distinct volumes; a large-volume
observation cannot fill a smaller volume's requirement. Per-filesystem budget
apportionment and host-wide reservations remain the executor programme's work.

## Compatibility, failure, and retained state

Without `--budget-file`, the existing percent policy and environment-based
`byte-budget` mode retain their thresholds, including the latter's minimum
40 GiB reserve. The new option does not change legacy `cargo-safe`, select a
policy from free space, change compile profiles, enable compiler caches, migrate
old outputs, or modify release/security/merge gates.

Execution repeats capacity observation inside the existing lease before
allocating build resources or attempting Cargo launch. If that observation or
another pre-launch preparation step fails, only this invocation's unlaunched
lease is released; already created directories and pre-existing evidence remain.
Once launch is attempted, spawn failure, interruption, and abnormal statuses
retain the conservative lease for owner verification. Cargo exit 0 or 101 keeps
the existing release behavior; neither status proves all descendants ended.

There is no runtime disk quota, growth watchdog, automatic cleanup, or host-wide
reservation in this repair. Arbitrary build scripts may write elsewhere. A
bounded policy makes a declared workload possible on a smaller host; it does not
make an unsupported workload safe or prove that the screenshot's build fits.

## Regression proof

```bash
python3 -m unittest discover -s scripts/tests -p 'test_cargo_admitted*.py'
```

The suite includes simulated 29 GiB admission/refusal, one-byte boundaries,
malformed/conflicting policy, scope changes, all-destination observations,
post-lease capacity loss, existing/dangling leases, and a real Git/Python process
fixture using fake Cargo to verify invocation and argument privacy. Fake Cargo
is not Rust build or footprint evidence. The existing offline real-Cargo A/B/A
regression remains opt-in with `CARGO_ADMITTED_REAL_BUILD_TEST=1` and requires an
already installed toolchain; its capacity observation is a fixture, not host
qualification.
